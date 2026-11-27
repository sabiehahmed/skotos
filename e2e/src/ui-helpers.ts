import { AxeBuilder } from '@axe-core/playwright';
import { expect, type Locator, type Page } from '@playwright/test';

// ============================================================================
// Theme & CSS Custom Property Helpers (Zero Product/CSS-Module Coupling)
// ============================================================================

export async function getTheme(page: Page): Promise<string | null> {
  return page.evaluate(() => document.documentElement.getAttribute('data-theme'));
}

export async function getCssCustomProperty(
  page: Page,
  propertyName: string,
  selector?: string,
): Promise<string> {
  return page.evaluate(
    ({ prop, sel }) => {
      const target = sel ? document.querySelector(sel) : document.documentElement;
      if (!target) return '';
      return getComputedStyle(target).getPropertyValue(prop).trim();
    },
    { prop: propertyName, sel: selector },
  );
}

export async function getEnvAccent(page: Page, selector?: string): Promise<string> {
  if (selector) {
    return getCssCustomProperty(page, '--sk-env-accent', selector);
  }
  const onRoot = await getCssCustomProperty(page, '--sk-env-accent');
  if (onRoot) {
    return onRoot;
  }
  return page.evaluate(() => {
    const styled = document.querySelector('[style*="--sk-env-accent"]');
    if (styled) {
      const value = getComputedStyle(styled).getPropertyValue('--sk-env-accent').trim();
      if (value) return value;
    }
    return getComputedStyle(document.body).getPropertyValue('--sk-env-accent').trim();
  });
}

export function normalizeHexColor(color: string): string {
  const trimmed = color.trim().toLowerCase();
  if (/^#[0-9a-f]{6}$/.test(trimmed)) {
    return trimmed;
  }
  const shortHex = /^#([0-9a-f])([0-9a-f])([0-9a-f])$/.exec(trimmed);
  if (shortHex) {
    return `#${shortHex[1]}${shortHex[1]}${shortHex[2]}${shortHex[2]}${shortHex[3]}${shortHex[3]}`;
  }
  const rgbMatch =
    /^rgba?\(\s*(\d{1,3})\s*[,\s]\s*(\d{1,3})\s*[,\s]\s*(\d{1,3})(?:\s*[,/]\s*[\d.%]+)?\s*\)$/.exec(
      trimmed,
    );
  if (rgbMatch) {
    const r = Math.max(0, Math.min(255, Number.parseInt(rgbMatch[1], 10)));
    const g = Math.max(0, Math.min(255, Number.parseInt(rgbMatch[2], 10)));
    const b = Math.max(0, Math.min(255, Number.parseInt(rgbMatch[3], 10)));
    return `#${r.toString(16).padStart(2, '0')}${g.toString(16).padStart(2, '0')}${b.toString(16).padStart(2, '0')}`;
  }
  return trimmed;
}

export async function toggleTheme(page: Page): Promise<string | null> {
  const control = page
    .getByRole('button', { name: /theme|dark mode|light mode|switch to (dark|light)/i })
    .or(page.getByRole('switch', { name: /dark|theme/i }))
    .first();

  await expect(control).toBeVisible();
  const before = await getTheme(page);
  await control.click();
  await expect.poll(() => getTheme(page)).not.toBe(before);
  return getTheme(page);
}

export async function setTheme(page: Page, target: 'light' | 'dark'): Promise<void> {
  const initial = await getTheme(page);
  if (initial === target) {
    return;
  }
  for (let attempt = 0; attempt < 3; attempt++) {
    const next = await toggleTheme(page);
    if (next === target) {
      return;
    }
  }
  const actual = await getTheme(page);
  throw new Error(`Unable to switch theme to "${target}" (ended on "${actual ?? 'null'}")`);
}

// ============================================================================
// Semantic WAI-ARIA Locators & Interaction Helpers
// ============================================================================

export function getBanner(page: Page): Locator {
  return page.getByRole('banner');
}

export function getEnvironmentBanner(page: Page, name?: string | RegExp): Locator {
  return page.getByRole('region', { name: name ?? /environment/i });
}

export function getHeading(
  page: Page,
  name: string | RegExp,
  level?: 1 | 2 | 3 | 4 | 5 | 6,
): Locator {
  return page.getByRole('heading', level !== undefined ? { name, level } : { name });
}

export function getAlert(page: Page, text?: string | RegExp): Locator {
  const base = page.getByRole('alert');
  return text !== undefined ? base.filter({ hasText: text }) : base;
}

export function getDialog(page: Page, name?: string | RegExp): Locator {
  return page.getByRole('dialog', name !== undefined ? { name } : undefined);
}

export function getAlertDialog(page: Page, name?: string | RegExp): Locator {
  return page.getByRole('alertdialog', name !== undefined ? { name } : undefined);
}

export async function selectTab(
  page: Page,
  name: string | RegExp,
  container?: Locator,
): Promise<Locator> {
  const root = container ?? page;
  const tab = root.getByRole('tab', { name });
  await expect(tab).toBeVisible();
  await tab.click();
  await expect(tab).toHaveAttribute('aria-selected', 'true');
  return tab;
}

export async function toggleSwitch(
  page: Page,
  name: string | RegExp,
  container?: Locator,
): Promise<Locator> {
  const root = container ?? page;
  const sw = root.getByRole('switch', { name });
  await expect(sw).toBeVisible();
  await sw.click();
  return sw;
}

export async function selectComboboxOption(
  page: Page,
  comboboxName: string | RegExp,
  optionName: string | RegExp,
  container?: Locator,
): Promise<void> {
  const root = container ?? page;
  const trigger = root.getByRole('combobox', { name: comboboxName });
  await expect(trigger).toBeVisible();
  await trigger.click();

  // Radix Portals render listbox/options directly under document.body, so query on page.
  const listbox = page.getByRole('listbox');
  await expect(listbox).toBeVisible();
  const option = page.getByRole('option', { name: optionName });
  await expect(option).toBeVisible();
  await option.click();
}

export async function openCommandPalette(page: Page): Promise<Locator> {
  const shortcut = process.platform === 'darwin' ? 'Meta+k' : 'Control+k';
  await page.keyboard.press(shortcut);
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  return dialog;
}

async function isFocusStrictlyInside(dialog: Locator): Promise<boolean> {
  return dialog.evaluate((el) => {
    const active = document.activeElement;
    if (!active || active === document.body || active === document.documentElement) {
      return false;
    }
    return el.contains(active);
  });
}

export async function verifyDialogFocusTrap(
  page: Page,
  dialog: Locator,
  tabSteps = 8,
): Promise<void> {
  await expect(dialog).toBeVisible();

  let inside = await isFocusStrictlyInside(dialog);
  if (!inside) {
    await page.keyboard.press('Tab');
    inside = await isFocusStrictlyInside(dialog);
  }
  if (!inside) {
    throw new Error('Dialog focus trap check failed: initial focus is not contained inside dialog');
  }

  for (let step = 1; step <= tabSteps; step++) {
    await page.keyboard.press('Tab');
    const stillInside = await isFocusStrictlyInside(dialog);
    if (!stillInside) {
      const escapedTag = await page.evaluate(() => {
        const a = document.activeElement;
        return a ? `${a.tagName.toLowerCase()}${a.id ? `#${a.id}` : ''}` : 'null';
      });
      throw new Error(
        `Dialog focus trap escaped on Tab step ${step}/${tabSteps} (activeElement=${escapedTag})`,
      );
    }
  }

  for (let step = 1; step <= 2; step++) {
    await page.keyboard.press('Shift+Tab');
    const stillInside = await isFocusStrictlyInside(dialog);
    if (!stillInside) {
      const escapedTag = await page.evaluate(() => {
        const a = document.activeElement;
        return a ? `${a.tagName.toLowerCase()}${a.id ? `#${a.id}` : ''}` : 'null';
      });
      throw new Error(
        `Dialog focus trap escaped on Shift+Tab step ${step}/2 (activeElement=${escapedTag})`,
      );
    }
  }
}

export async function dismissDialogWithEscape(
  page: Page,
  dialog: Locator,
  expectedFocusReturn?: Locator,
): Promise<void> {
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
  if (expectedFocusReturn) {
    await expect(expectedFocusReturn).toBeFocused();
  }
}

export async function confirmChangeReviewDialog(
  page: Page,
  options?: {
    comment?: string;
    confirmButtonName?: string | RegExp;
  },
): Promise<void> {
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();

  if (options?.comment !== undefined) {
    const commentField = dialog.getByLabel(/comment|reason/i);
    await expect(commentField).toBeVisible();
    await commentField.fill(options.comment);
  }

  const confirmBtn = dialog.getByRole('button', {
    name: options?.confirmButtonName ?? /confirm|save|apply|restore/i,
  });
  await expect(confirmBtn).toBeVisible();
  await confirmBtn.click();
}

export async function waitForToast(page: Page, text: string | RegExp): Promise<Locator> {
  const toast = page
    .getByRole('status')
    .or(page.getByRole('alert'))
    .filter({ hasText: text })
    .first();
  await expect(toast).toBeVisible();
  return toast;
}

// ============================================================================
// @axe-core/playwright Accessibility Helpers (WCAG 2.1 AA)
// ============================================================================

export type AxeImpact = 'minor' | 'moderate' | 'serious' | 'critical';

export const DEFAULT_WCAG_TAGS = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'] as const;

const DEFAULT_BLOCKING_IMPACTS: readonly AxeImpact[] = ['serious', 'critical'];

export type AxeResults = Awaited<ReturnType<AxeBuilder['analyze']>>;
export type AxeViolation = AxeResults['violations'][number];

export interface A11yAuditOptions {
  readonly tags?: string[];
  readonly failOnImpacts?: AxeImpact[];
  readonly include?: string | string[];
  readonly exclude?: string | string[];
  readonly disableRules?: string[];
}

export interface A11yAuditSummary {
  readonly results: AxeResults;
  readonly allViolations: AxeResults['violations'];
  readonly blockingViolations: AxeResults['violations'];
  readonly formattedReport: string;
}

export function formatAxeViolations(violations: readonly AxeViolation[]): string {
  if (violations.length === 0) {
    return 'No WCAG accessibility violations found.';
  }
  return violations
    .map((v) => {
      const impact = (v.impact ?? 'unknown').toUpperCase();
      const lines: string[] = [
        `[${impact}] ${v.id}: ${v.help}`,
        `  Help: ${v.helpUrl}`,
      ];
      for (const node of v.nodes) {
        const target = node.target
          .map((t) => (Array.isArray(t) ? t.join(' >> ') : String(t)))
          .join(', ');
        const html = node.html.replace(/\s+/g, ' ').trim().slice(0, 160);
        const failure = (node.failureSummary ?? '').replace(/\s+/g, ' ').trim();
        lines.push(`  - Target: ${target}`);
        lines.push(`    HTML: ${html}`);
        if (failure) {
          lines.push(`    Failure: ${failure}`);
        }
      }
      return lines.join('\n');
    })
    .join('\n\n');
}

export async function runA11yAudit(
  page: Page,
  options: A11yAuditOptions = {},
): Promise<A11yAuditSummary> {
  const tags = options.tags ?? [...DEFAULT_WCAG_TAGS];
  const blockingSet = new Set<string>(options.failOnImpacts ?? DEFAULT_BLOCKING_IMPACTS);

  let builder = new AxeBuilder({ page }).withTags(tags);

  if (options.include !== undefined) {
    const selectors = Array.isArray(options.include) ? options.include : [options.include];
    for (const selector of selectors) {
      builder = builder.include(selector);
    }
  }
  if (options.exclude !== undefined) {
    const selectors = Array.isArray(options.exclude) ? options.exclude : [options.exclude];
    for (const selector of selectors) {
      builder = builder.exclude(selector);
    }
  }
  if (options.disableRules && options.disableRules.length > 0) {
    builder = builder.disableRules(options.disableRules);
  }

  const results = await builder.analyze();
  const blockingViolations = results.violations.filter((v) =>
    blockingSet.has(v.impact ?? 'moderate'),
  );
  const formattedReport = formatAxeViolations(blockingViolations);

  return {
    results,
    allViolations: results.violations,
    blockingViolations,
    formattedReport,
  };
}

export async function assertNoSeriousA11yViolations(
  page: Page,
  options: A11yAuditOptions = {},
): Promise<A11yAuditSummary> {
  const summary = await runA11yAudit(page, options);
  if (summary.blockingViolations.length > 0) {
    throw new Error(
      `Found ${summary.blockingViolations.length} blocking WCAG 2.1 AA violation(s):\n\n${summary.formattedReport}`,
    );
  }
  return summary;
}
