import { expect, test } from '@playwright/test';
import { assertNoSeriousA11yViolations } from '../../src/ui-helpers';

test.describe('Phase 2 Live Stack End-to-End (Segments, Webhooks, Usage & Automation + WCAG AA)', () => {
  test('manages segments, signed webhooks, and flag usage/automation triggers with zero WCAG AA violations', async ({
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

    const ts = Date.now();
    const segKey = `pw-seg-${ts}`;
    const segName = `Playwright Segment ${ts}`;
    const whName = `Playwright Webhook ${ts}`;
    const flagKey = `pw-p2-flag-${ts}`;
    const flagName = `Playwright Phase 2 Flag ${ts}`;

    await page.goto('/');

    // 1. Sign in
    await expect(page.getByRole('button', { name: 'Sign in' })).toBeVisible();
    await page.getByLabel('Email').fill('admin@skotos.local');
    await page.getByLabel('Password').fill('SkotosDevPass123!');
    await page.getByRole('button', { name: 'Sign in' }).click();
    await expect(page.getByRole('heading', { name: 'Feature Flags', level: 1 })).toBeVisible();

    // 2. Navigate to Segments, verify WCAG AA, create a segment, and clean up
    await page.getByRole('button', { name: 'Segments' }).click();
    await expect(page.getByRole('heading', { name: 'Segments', level: 1 })).toBeVisible();
    await assertNoSeriousA11yViolations(page);

    await page.getByRole('button', { name: /Create segment/i }).first().click();
    const segDialog = page.getByRole('dialog');
    await segDialog.getByLabel(/^Name\b/).fill(segName);
    await segDialog.getByLabel(/^Key\b/).fill(segKey);
    await segDialog.getByRole('button', { name: /^Create segment$/i }).click();
    await expect(page.getByText(segName)).toBeVisible();

    const segDeleteStatus = await page.evaluate(async (key) => {
      const res = await fetch(
        `/api/v1/projects/default/environments/development/segments/${encodeURIComponent(key)}`,
        { method: 'DELETE', credentials: 'include' },
      );
      return res.status;
    }, segKey);
    expect(segDeleteStatus).toBe(204);

    // 3. Navigate to Webhooks, verify WCAG AA, create a signed webhook, verify one-time secret
    await page.getByRole('button', { name: 'Webhooks' }).click();
    await expect(page.getByRole('heading', { name: 'Webhooks', level: 1 })).toBeVisible();
    await assertNoSeriousA11yViolations(page);

    await page.getByRole('button', { name: /Add webhook/i }).first().click();
    const whDialog = page.getByRole('dialog');
    await whDialog.getByLabel(/^Name\b/).fill(whName);
    await whDialog.getByLabel(/^Target URL\b/).fill('https://hooks.example.com/pw-phase2');
    await whDialog.getByRole('button', { name: /^Create webhook$/i }).click();
    await expect(whDialog.getByText('Save your signing secret')).toBeVisible();
    await whDialog.getByRole('button', { name: 'Done' }).click();
    await expect(page.getByText(whName)).toBeVisible();

    // Clean up created webhook via browser fetch
    await page.evaluate(async (nameToClean) => {
      const listRes = await fetch('/api/v1/webhooks', { credentials: 'include' });
      const body = (await listRes.json()) as { items: Array<{ id: string; name: string }> };
      const target = body.items.find((w) => w.name === nameToClean);
      if (target) {
        await fetch(`/api/v1/webhooks/${encodeURIComponent(target.id)}`, {
          method: 'DELETE',
          credentials: 'include',
        });
      }
    }, whName);

    // 4. Create a flag, open Usage & Automation tab, verify WCAG AA, create a trigger URL
    await page.getByRole('button', { name: 'Feature Flags' }).click();
    await expect(page.getByRole('heading', { name: 'Feature Flags', level: 1 })).toBeVisible();
    await page.getByRole('button', { name: /Create Flag/i }).first().click();
    const flagDialog = page.getByRole('dialog');
    await flagDialog.getByLabel(/^Name\b/).fill(flagName);
    await flagDialog.getByLabel(/^Key\b/).fill(flagKey);
    await flagDialog.getByRole('button', { name: /^Create Flag$/i }).click();

    await expect(page.getByText(flagName)).toBeVisible();
    await page.getByText(flagName).click();
    await expect(page.getByRole('heading', { name: flagName, level: 1 })).toBeVisible();

    await page.getByRole('tab', { name: 'Usage & Automation' }).click();
    await expect(page.getByRole('heading', { name: 'Evaluation Usage (Last 30 Days)' })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Copy Targeting to Another Environment' })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Scheduled Changes' })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Flag Triggers (APM Kill-Switch URLs)' })).toBeVisible();
    await assertNoSeriousA11yViolations(page);

    await page.getByLabel(/^Description\b/).fill('Datadog p99 alert kill-switch');
    await page.getByRole('button', { name: /Create Trigger URL/i }).click();
    await expect(page.getByText('Copy your one-time trigger URL now')).toBeVisible();
    await expect(page.getByText(/POST \/api\/v1\/triggers\//)).toBeVisible();

    // Clean up test flag
    const flagDeleteStatus = await page.evaluate(async (key) => {
      const res = await fetch(`/api/v1/projects/default/flags/${encodeURIComponent(key)}`, {
        method: 'DELETE',
        credentials: 'include',
      });
      return res.status;
    }, flagKey);
    expect(flagDeleteStatus).toBe(204);

    // 5. Zero runtime errors
    expect(pageErrors).toEqual([]);
    expect(consoleErrors.filter((msg) => !msg.includes('401'))).toEqual([]);
  });
});
