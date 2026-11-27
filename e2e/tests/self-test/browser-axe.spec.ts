import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { expect, test } from '@playwright/test';
import { loginAs } from '../../src/fixtures';
import {
  assertNoSeriousA11yViolations,
  confirmChangeReviewDialog,
  dismissDialogWithEscape,
  getAlert,
  getBanner,
  getDialog,
  getEnvAccent,
  getEnvironmentBanner,
  getHeading,
  getTheme,
  normalizeHexColor,
  openCommandPalette,
  runA11yAudit,
  selectComboboxOption,
  selectTab,
  setTheme,
  toggleSwitch,
  toggleTheme,
  verifyDialogFocusTrap,
  waitForToast,
} from '../../src/ui-helpers';

const ACCESSIBLE_HTML = `<!doctype html>
<html lang="en" data-theme="dark">
<head>
  <meta charset="utf-8" />
  <title>Skotos Accessible Fixture</title>
  <style>
    :root {
      --sk-env-accent: #DC2626;
      --bg: #111827;
      --fg: #F9FAFB;
      --surface: #1F2937;
      --border: #4B5563;
      --accent-text: #FFFFFF;
    }
    html[data-theme="light"] {
      --bg: #FFFFFF;
      --fg: #111827;
      --surface: #F3F4F6;
      --border: #4B5563;
    }
    body {
      margin: 0;
      padding: 16px;
      font-family: system-ui, sans-serif;
      background: var(--bg);
      color: var(--fg);
    }
    header, nav, main {
      margin-bottom: 12px;
    }
    button, input {
      background: var(--surface);
      color: var(--fg);
      border: 1px solid var(--border);
      padding: 6px 10px;
    }
    [role="region"] {
      border-left: 4px solid var(--sk-env-accent);
      padding: 8px 12px;
      background: var(--surface);
    }
    [role="dialog"] {
      position: fixed;
      inset: 80px auto auto 80px;
      background: var(--surface);
      color: var(--fg);
      border: 2px solid var(--border);
      padding: 16px;
      z-index: 100;
    }
    [role="listbox"] {
      background: var(--surface);
      color: var(--fg);
      border: 1px solid var(--border);
      padding: 4px;
    }
  </style>
</head>
<body>
  <header>
    <button id="theme-toggle" type="button" aria-label="Switch to light theme">Toggle theme</button>
    <button id="open-palette-btn" type="button">Open Command Palette</button>
  </header>

  <nav aria-label="Primary">
    <a href="#flags" style="color: inherit;">Feature Flags</a>
  </nav>

  <div role="region" aria-label="Environment banner">Production environment</div>

  <main id="flags">
    <h1>Skotos Flags</h1>

    <div>
      <button
        id="env-combobox"
        type="button"
        role="combobox"
        aria-label="Environment"
        aria-haspopup="listbox"
        aria-expanded="false"
      >Development</button>
    </div>

    <div role="tablist" aria-label="Flag sections">
      <button id="tab-targeting" type="button" role="tab" aria-selected="true" aria-controls="panel-targeting">Targeting</button>
      <button id="tab-history" type="button" role="tab" aria-selected="false" aria-controls="panel-history">History</button>
    </div>

    <section id="panel-targeting" role="tabpanel" aria-labelledby="tab-targeting">
      <button id="checkout-switch" type="button" role="switch" aria-checked="false">Checkout V2</button>
    </section>

    <section id="panel-history" role="tabpanel" aria-labelledby="tab-history" hidden>
      <p>Revision history entries</p>
    </section>

    <div id="toast-region" role="status" aria-live="polite"></div>
  </main>

  <div id="cmd-palette" role="dialog" aria-modal="true" aria-label="Command Palette" hidden>
    <label for="cmd-input">Search commands</label>
    <input id="cmd-input" type="text" />
    <button id="cmd-go" type="button">Go to Flags</button>
    <button id="cmd-close" type="button">Close</button>
  </div>

  <script>
    const html = document.documentElement;
    const themeBtn = document.getElementById('theme-toggle');
    themeBtn.addEventListener('click', () => {
      const next = html.getAttribute('data-theme') === 'dark' ? 'light' : 'dark';
      html.setAttribute('data-theme', next);
      themeBtn.setAttribute('aria-label', next === 'dark' ? 'Switch to light theme' : 'Switch to dark theme');
    });

    const tabs = Array.from(document.querySelectorAll('[role="tab"]'));
    tabs.forEach((tab) => {
      tab.addEventListener('click', () => {
        tabs.forEach((t) => {
          const selected = t === tab;
          t.setAttribute('aria-selected', String(selected));
          const panel = document.getElementById(t.getAttribute('aria-controls'));
          if (panel) panel.hidden = !selected;
        });
      });
    });

    const sw = document.getElementById('checkout-switch');
    const toastRegion = document.getElementById('toast-region');
    sw.addEventListener('click', () => {
      const checked = sw.getAttribute('aria-checked') === 'true';
      sw.setAttribute('aria-checked', String(!checked));
      toastRegion.textContent = !checked ? 'Flag turned on' : 'Flag turned off';
    });

    // Portal-style listbox appended to document.body (mirrors Radix Portal behavior)
    const combo = document.getElementById('env-combobox');
    let portalListbox = null;
    combo.addEventListener('click', () => {
      if (portalListbox) {
        portalListbox.remove();
        portalListbox = null;
        combo.setAttribute('aria-expanded', 'false');
        return;
      }
      portalListbox = document.createElement('div');
      portalListbox.setAttribute('role', 'listbox');
      portalListbox.setAttribute('aria-label', 'Environments');
      ['Development', 'Production'].forEach((label) => {
        const opt = document.createElement('div');
        opt.setAttribute('role', 'option');
        opt.setAttribute('aria-selected', String(combo.textContent === label));
        opt.textContent = label;
        opt.addEventListener('click', () => {
          combo.textContent = label;
          combo.setAttribute('aria-expanded', 'false');
          portalListbox.remove();
          portalListbox = null;
        });
        portalListbox.appendChild(opt);
      });
      document.body.appendChild(portalListbox);
      combo.setAttribute('aria-expanded', 'true');
    });

    // Command palette with genuine Tab / Shift+Tab focus trap and Escape focus return
    const dialog = document.getElementById('cmd-palette');
    const cmdInput = document.getElementById('cmd-input');
    const openPaletteBtn = document.getElementById('open-palette-btn');
    const closeBtn = document.getElementById('cmd-close');
    let previouslyFocused = null;

    function openDialog() {
      previouslyFocused = document.activeElement;
      dialog.hidden = false;
      cmdInput.focus();
    }

    function closeDialog() {
      dialog.hidden = true;
      if (previouslyFocused && typeof previouslyFocused.focus === 'function') {
        previouslyFocused.focus();
      }
    }

    openPaletteBtn.addEventListener('click', openDialog);
    closeBtn.addEventListener('click', closeDialog);

    document.addEventListener('keydown', (e) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        openDialog();
        return;
      }
      if (!dialog.hidden && e.key === 'Escape') {
        e.preventDefault();
        closeDialog();
        return;
      }
      if (!dialog.hidden && e.key === 'Tab') {
        const focusables = Array.from(dialog.querySelectorAll('input, button'));
        const first = focusables[0];
        const last = focusables[focusables.length - 1];
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    });
  </script>
</body>
</html>`;

const INACCESSIBLE_HTML = `<!doctype html>
<html>
<head><title>Broken A11y Fixture</title></head>
<body>
  <button></button>
  <img src="data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7" />
  <div role="not-a-real-role">Bad ARIA role</div>
</body>
</html>`;

const MIXED_HTML = `<!doctype html>
<html lang="en">
<head><title>Mixed A11y Fixture</title></head>
<body>
  <main>
    <h1>Healthy Content</h1>
    <p>Accessible section with clear contrast.</p>
  </main>
  <aside id="broken-region" aria-label="Broken Region">
    <button></button>
  </aside>
</body>
</html>`;

const BROKEN_DIALOG_HTML = `<!doctype html>
<html lang="en">
<head><title>Broken Dialog Fixture</title></head>
<body>
  <button id="outside" type="button">Outside Action</button>
  <div role="dialog" aria-modal="true" aria-label="Broken Dialog">
    <button id="inside" type="button">Inside Action</button>
  </div>
</body>
</html>`;

const LOGIN_HTML = `<!doctype html>
<html lang="en">
<head><title>Skotos Login</title></head>
<body>
  <main>
    <h1>Sign in to Skotos</h1>
    <form id="login-form">
      <label for="email">Email</label>
      <input id="email" name="email" type="email" required />
      <label for="pwd">Password</label>
      <input id="pwd" name="password" type="password" required />
      <button type="submit">Sign in</button>
    </form>
  </main>
  <script>
    document.getElementById('login-form').addEventListener('submit', (e) => {
      e.preventDefault();
      const email = document.getElementById('email').value;
      const pwd = document.getElementById('pwd').value;
      if (email === 'admin@skotos.local' && pwd === 'SkotosDevPass123!') {
        window.location.href = '/dashboard';
      }
    });
  </script>
</body>
</html>`;

const DASHBOARD_HTML = `<!doctype html>
<html lang="en">
<head><title>Skotos Dashboard</title></head>
<body>
  <main>
    <h1>Dashboard</h1>
    <button id="trigger-change" type="button">Save Changes</button>
    <div id="result-alert" role="alert" hidden></div>
  </main>
  <div id="review-dialog" role="dialog" aria-modal="true" aria-label="Review Changes" hidden>
    <label for="review-comment">Change comment</label>
    <textarea id="review-comment"></textarea>
    <button id="review-confirm" type="button">Confirm Changes</button>
  </div>
  <script>
    const dialog = document.getElementById('review-dialog');
    const resultAlert = document.getElementById('result-alert');
    document.getElementById('trigger-change').addEventListener('click', () => {
      dialog.hidden = false;
    });
    document.getElementById('review-confirm').addEventListener('click', () => {
      const comment = document.getElementById('review-comment').value;
      dialog.hidden = true;
      resultAlert.textContent = 'Applied: ' + comment;
      resultAlert.hidden = false;
    });
  </script>
</body>
</html>`;

let server: http.Server;
let baseUrl: string;

test.describe('Real Headless Browser, UI Helpers & @axe-core/playwright', () => {
  test.beforeAll(async () => {
    server = http.createServer((req, res) => {
      const pathname = new URL(req.url ?? '/', 'http://127.0.0.1').pathname;
      const routes: Record<string, string> = {
        '/accessible': ACCESSIBLE_HTML,
        '/inaccessible': INACCESSIBLE_HTML,
        '/mixed': MIXED_HTML,
        '/broken-dialog': BROKEN_DIALOG_HTML,
        '/login': LOGIN_HTML,
        '/dashboard': DASHBOARD_HTML,
      };
      const html = routes[pathname];
      if (!html) {
        res.writeHead(404, { 'Content-Type': 'text/plain' });
        res.end('Not found');
        return;
      }
      res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
      res.end(html);
    });

    await new Promise<void>((resolve) => {
      server.listen(0, '127.0.0.1', () => resolve());
    });
    const addr = server.address() as AddressInfo;
    baseUrl = `http://127.0.0.1:${addr.port}`;
  });

  test.afterAll(async () => {
    await new Promise<void>((resolve, reject) => {
      server.close((err) => (err ? reject(err) : resolve()));
    });
  });

  test('verifies theme, --sk-env-accent, ARIA controls, CommandPalette focus trap/Escape, and passing WCAG 2.1 AA axe audit', async ({
    page,
  }) => {
    await page.goto(`${baseUrl}/accessible`);

    expect(await getTheme(page)).toBe('dark');
    expect(normalizeHexColor(await getEnvAccent(page))).toBe('#dc2626');
    expect(normalizeHexColor('rgb(220, 38, 38)')).toBe('#dc2626');
    expect(normalizeHexColor('#ABC')).toBe('#aabbcc');

    await expect(getBanner(page)).toBeVisible();
    await expect(getEnvironmentBanner(page)).toContainText('Production environment');
    await expect(getHeading(page, 'Skotos Flags', 1)).toBeVisible();

    expect(await toggleTheme(page)).toBe('light');
    await setTheme(page, 'dark');
    expect(await getTheme(page)).toBe('dark');

    await selectTab(page, /history/i);
    await expect(page.getByText('Revision history entries')).toBeVisible();
    await selectTab(page, /targeting/i);

    const sw = await toggleSwitch(page, /checkout v2/i);
    await expect(sw).toHaveAttribute('aria-checked', 'true');
    await waitForToast(page, /flag turned on/i);

    // Portal listbox combobox selection
    await selectComboboxOption(page, /environment/i, 'Production');
    await expect(page.getByRole('combobox', { name: /environment/i })).toHaveText('Production');

    // CommandPalette shortcut, focus trap, and Escape focus restoration
    const triggerBtn = page.getByRole('button', { name: /open command palette/i });
    await triggerBtn.focus();
    const dialog = await openCommandPalette(page);
    await verifyDialogFocusTrap(page, dialog, 6);
    await dismissDialogWithEscape(page, dialog, triggerBtn);

    // Passing WCAG 2.1 AA audit proving axe genuinely inspected live DOM nodes
    const audit = await assertNoSeriousA11yViolations(page);
    expect(audit.blockingViolations).toHaveLength(0);
    expect(audit.results.passes.length).toBeGreaterThan(0);
  });

  test('assertNoSeriousA11yViolations rejects inaccessible HTML with detailed WCAG 2.1 AA violation report', async ({
    page,
  }) => {
    await page.goto(`${baseUrl}/inaccessible`);

    const summary = await runA11yAudit(page);
    const ruleIds = new Set(summary.blockingViolations.map((v) => v.id));
    expect(summary.blockingViolations.length).toBeGreaterThanOrEqual(4);
    expect(ruleIds.has('html-has-lang')).toBe(true);
    expect(ruleIds.has('button-name')).toBe(true);
    expect(ruleIds.has('image-alt')).toBe(true);
    expect(ruleIds.has('aria-roles')).toBe(true);

    let thrownMessage = '';
    try {
      await assertNoSeriousA11yViolations(page);
    } catch (err) {
      thrownMessage = err instanceof Error ? err.message : String(err);
    }
    expect(thrownMessage).toContain('html-has-lang');
    expect(thrownMessage).toContain('button-name');
    expect(thrownMessage).toContain('CRITICAL');
    expect(thrownMessage).toContain('SERIOUS');
    expect(thrownMessage).toContain('dequeuniversity.com');
  });

  test('scoped include and exclude Axe options accurately isolate broken vs healthy regions', async ({
    page,
  }) => {
    await page.goto(`${baseUrl}/mixed`);

    // Full page fails due to empty button inside #broken-region
    await expect(assertNoSeriousA11yViolations(page)).rejects.toThrow(/button-name/);

    // Excluding #broken-region passes cleanly
    const cleanAudit = await assertNoSeriousA11yViolations(page, { exclude: '#broken-region' });
    expect(cleanAudit.blockingViolations).toHaveLength(0);
    expect(cleanAudit.results.passes.length).toBeGreaterThan(0);

    // Including only #broken-region rejects with button-name
    await expect(
      assertNoSeriousA11yViolations(page, { include: '#broken-region' }),
    ).rejects.toThrow(/button-name/);
  });

  test('verifyDialogFocusTrap rejects when a broken dialog fails to trap focus', async ({
    page,
  }) => {
    await page.goto(`${baseUrl}/broken-dialog`);

    const brokenDialog = getDialog(page, 'Broken Dialog');
    await expect(brokenDialog).toBeVisible();
    await page.locator('#inside').focus();

    await expect(verifyDialogFocusTrap(page, brokenDialog, 4)).rejects.toThrow(
      /focus trap escaped/i,
    );
  });

  test('loginAs handles <input type="password"> via label and confirmChangeReviewDialog submits comment', async ({
    page,
  }) => {
    await loginAs(
      page,
      { email: 'admin@skotos.local', password: 'SkotosDevPass123!' },
      baseUrl,
    );
    await expect(page).toHaveURL(/\/dashboard$/);
    await expect(getHeading(page, 'Dashboard', 1)).toBeVisible();

    await page.getByRole('button', { name: /save changes/i }).click();
    await confirmChangeReviewDialog(page, { comment: 'Approved rollout' });
    await expect(getAlert(page, /Applied: Approved rollout/)).toBeVisible();
  });
});
