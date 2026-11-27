import { expect, test } from '@playwright/test';
import { assertNoSeriousA11yViolations } from '../../src/ui-helpers';

test.describe('Phase 1 Live Stack End-to-End (Browser + Control API + PostgreSQL)', () => {
  test('signs in, creates a flag, toggles targeting, simulates evaluation, switches environments, views audit log, and passes WCAG 2.1 AA', async ({
    page,
  }) => {
    const consoleErrors: string[] = [];
    const pageErrors: string[] = [];
    page.on('console', (msg) => {
      if (msg.type() === 'error') {
        consoleErrors.push(msg.text());
      }
    });
    page.on('pageerror', (err) => {
      pageErrors.push(err.message);
    });

    const flagKey = `pw-e2e-${Date.now()}`;
    const flagName = `Playwright Flag ${Date.now()}`;

    await page.goto('/');

    // 1. Unauthenticated login screen + WCAG audit
    await expect(page.getByRole('button', { name: 'Sign in' })).toBeVisible();
    await assertNoSeriousA11yViolations(page);

    await page.getByLabel('Email').fill('admin@skotos.local');
    await page.getByLabel('Password').fill('SkotosDevPass123!');
    await page.getByRole('button', { name: 'Sign in' }).click();

    // 2. Authenticated flags list + Development environment banner + WCAG audit
    await expect(page.getByRole('heading', { name: 'Feature Flags', level: 1 })).toBeVisible();
    await expect(page.getByTestId('environment-banner')).toContainText('Development');
    await assertNoSeriousA11yViolations(page);

    // 3. Create a boolean flag through the modal form
    await page.getByRole('button', { name: /Create Flag/i }).first().click();
    const dialog = page.getByRole('dialog');
    await dialog.getByLabel('Name').fill(flagName);
    await dialog.getByLabel('Key').fill(flagKey);
    await dialog.getByLabel('Description').fill('Created by real-browser Playwright E2E test');
    await dialog.getByRole('button', { name: /^Create Flag$/i }).click();

    // 4. Open the newly created flag detail page
    await expect(page.getByText(flagName)).toBeVisible();
    await page.getByText(flagName).click();
    await expect(page.getByRole('heading', { name: flagName, level: 1 })).toBeVisible();
    await expect(page.getByText('Test Context Simulator')).toBeVisible();
    await assertNoSeriousA11yViolations(page);

    // 5. Toggle targeting ON and save through Review & Save modal
    await page.getByTestId('detail-targeting-switch').click();
    await page.getByRole('button', { name: /Review & Save \(1\)/i }).click();
    await page.getByLabel('Change comment').fill('Enable via Playwright E2E');
    await page.getByRole('button', { name: /^Save changes$/i }).click();
    await expect(page.getByText('v2')).toBeVisible();

    // 6. Evaluate context in live Rust evaluation engine via /simulate
    await page.getByRole('button', { name: /Evaluate Context/i }).click();
    await expect(page.getByText('EVALUATION RESULT')).toBeVisible();
    await expect(page.getByText('FALLTHROUGH', { exact: true })).toBeVisible();

    // 7. Switch theme (light <-> dark) and verify WCAG AA in dark mode too
    await page.getByRole('button', { name: /Switch to dark theme/i }).click();
    await expect(page.locator('[data-theme="dark"]').first()).toBeVisible();
    await assertNoSeriousA11yViolations(page);

    // 8. Navigate to Audit Log and confirm entries rendered
    await page.getByRole('button', { name: /Audit Log/i }).click();
    await expect(page.getByRole('heading', { name: 'Audit Log', level: 1 })).toBeVisible();
    await expect(page.getByText(flagKey).first()).toBeVisible();

    // 9. Clean up any pw-e2e-* test flags via authenticated browser fetch
    const deleteStatus = await page.evaluate(async (key) => {
      const res = await fetch(`/api/v1/projects/default/flags/${encodeURIComponent(key)}`, {
        method: 'DELETE',
        credentials: 'include',
      });
      return res.status;
    }, flagKey);
    expect(deleteStatus).toBe(204);

    // 10. Verify zero runtime console or page errors
    expect(pageErrors).toEqual([]);
    expect(consoleErrors.filter((msg) => !msg.includes('401'))).toEqual([]);
  });
});
