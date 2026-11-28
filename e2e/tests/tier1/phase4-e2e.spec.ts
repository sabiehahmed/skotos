import { expect, test } from '@playwright/test';
import { assertNoSeriousA11yViolations } from '../../src/ui-helpers';

test.describe('Phase 4 Live Stack End-to-End (Experiments, Guarded Rollouts, Insights & Migrations + WCAG AA)', () => {
  test('exercises Metrics, Layers, Experiments, Guarded Rollouts, Engineering Insights, Data Export, and Migration Flags with zero WCAG AA violations', async ({
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
    const flagKey = `pw-p4-flag-${ts}`;
    const flagName = `Playwright Phase 4 Flag ${ts}`;
    const metricKey = `pw-metric-${ts}`;
    const metricName = `Playwright Conversion ${ts}`;
    const layerKey = `pw-layer-${ts}`;
    const layerName = `Playwright Layer ${ts}`;
    const expKey = `pw-exp-${ts}`;
    const expName = `Playwright Experiment ${ts}`;
    const rolloutName = `Playwright Guarded Rollout ${ts}`;
    const sinkName = `Playwright R2 Sink ${ts}`;

    await page.goto('/');

    // 1. Sign in as bootstrap owner
    await expect(page.getByRole('button', { name: 'Sign in' })).toBeVisible();
    await page.getByLabel('Email').fill('admin@skotos.local');
    await page.getByLabel('Password').fill('SkotosDevPass123!');
    await page.getByRole('button', { name: 'Sign in' }).click();
    await expect(page.getByRole('heading', { name: 'Feature Flags', level: 1 })).toBeVisible();

    // 2. Create a Flag and verify Feature Monitoring + 2/4/6-Stage Migration Flag UI
    await page.getByRole('button', { name: /Create Flag/i }).first().click();
    const flagDialog = page.getByRole('dialog');
    await flagDialog.getByLabel(/^Name\b/).fill(flagName);
    await flagDialog.getByLabel(/^Key\b/).fill(flagKey);
    await flagDialog.getByRole('button', { name: /^Create Flag$/i }).click();
    await expect(page.getByText(flagName)).toBeVisible();

    await page.getByText(flagName).click();
    await expect(page.getByRole('heading', { name: flagName, level: 1 })).toBeVisible();
    await page.getByRole('tab', { name: 'Usage & Automation' }).click();
    await expect(page.getByRole('heading', { name: 'Feature Monitoring (Last 24h)' })).toBeVisible();
    await expect(page.getByRole('heading', { name: '2 / 4 / 6-Stage Migration Flag' })).toBeVisible();
    await page.getByRole('button', { name: /Save Migration Stage/i }).click();
    await expect(page.getByText(/Stage: off/i)).toBeVisible();

    // 3. Experiments, Metrics & Mutual Exclusion Layers (/projects/default/experiments) + WCAG AA
    await page.getByRole('button', { name: 'Experiments' }).click();
    await expect(page.getByRole('heading', { name: 'Experimentation', level: 1 })).toBeVisible();
    await assertNoSeriousA11yViolations(page);

    // Create a Metric
    await page.getByRole('tab', { name: /Metrics & Funnels/i }).click();
    await page.getByRole('button', { name: /Create Metric/i }).first().click();
    const metricDialog = page.getByRole('dialog');
    await metricDialog.getByLabel(/^Metric Key\b/).fill(metricKey);
    await metricDialog.getByLabel(/^Name\b/).fill(metricName);
    await metricDialog.getByLabel(/^SDK Event Key\b/).fill(`ev_${ts}`);
    await metricDialog.getByRole('button', { name: /^Create Metric$/i }).click();
    await expect(page.getByText(metricName)).toBeVisible();

    // Create a Mutual Exclusion Layer
    await page.getByRole('tab', { name: /Exclusion Layers/i }).click();
    await page.getByRole('button', { name: /Create Layer/i }).click();
    const layerDialog = page.getByRole('dialog');
    await layerDialog.getByLabel(/^Layer Key\b/).fill(layerKey);
    await layerDialog.getByLabel(/^Name\b/).fill(layerName);
    await layerDialog.getByRole('button', { name: /^Create Layer$/i }).click();
    await expect(page.getByText(layerName)).toBeVisible();

    // Create & Start an Experiment
    await page.getByRole('tab', { name: /Experiments \(/i }).click();
    await page.getByRole('button', { name: /Create Experiment/i }).click();
    const expDialog = page.getByRole('dialog');
    await expDialog.getByLabel(/^Experiment Key\b/).fill(expKey);
    await expDialog.getByLabel(/^Name\b/).fill(expName);
    await expDialog.getByRole('button', { name: /^Create Experiment$/i }).click();
    await expect(page.getByText(expName)).toBeVisible();

    // 4. Guarded Rollouts (/projects/default/rollouts) + WCAG AA
    await page.getByRole('button', { name: 'Guarded Rollouts' }).click();
    await expect(page.getByRole('heading', { name: 'Guarded Rollouts', level: 1 })).toBeVisible();
    await assertNoSeriousA11yViolations(page);

    await page.getByRole('button', { name: /Start Guarded Rollout/i }).click();
    const grDialog = page.getByRole('dialog');
    await grDialog.getByLabel(/^Rollout Name\b/).fill(rolloutName);
    await grDialog.getByRole('button', { name: /^Start Guarded Rollout$/i }).click();
    await expect(page.getByText(rolloutName)).toBeVisible();

    // 5. Engineering Insights & Data Export (/projects/default/insights) + WCAG AA
    await page.getByRole('button', { name: /Insights & Export/i }).click();
    await expect(
      page.getByRole('heading', { name: 'Engineering Insights & Data Export', level: 1 }),
    ).toBeVisible();
    await expect(page.getByText('DEPLOYMENT FREQUENCY (30D)')).toBeVisible();
    await assertNoSeriousA11yViolations(page);

    await page.getByRole('tab', { name: /Data Export Sinks/i }).click();
    await page.getByRole('button', { name: /Add Export Sink/i }).click();
    const sinkDialog = page.getByRole('dialog');
    await sinkDialog.getByLabel(/^Sink Name\b/).fill(sinkName);
    await sinkDialog.getByRole('button', { name: /^Create Sink$/i }).click();
    await expect(page.getByText(sinkName)).toBeVisible();

    // 6. Clean up all test entities via API
    await page.evaluate(
      async ({ fKey, mKey, lKey, eKey, sName }) => {
        await fetch(
          `/api/v1/projects/default/environments/development/experiments/${encodeURIComponent(eKey)}`,
          { method: 'DELETE', credentials: 'include' },
        );
        await fetch(`/api/v1/projects/default/layers/${encodeURIComponent(lKey)}`, {
          method: 'DELETE',
          credentials: 'include',
        });
        await fetch(`/api/v1/projects/default/metrics/${encodeURIComponent(mKey)}`, {
          method: 'DELETE',
          credentials: 'include',
        });
        await fetch(`/api/v1/projects/default/flags/${encodeURIComponent(fKey)}`, {
          method: 'DELETE',
          credentials: 'include',
        });
        const sinksRes = await fetch('/api/v1/data-export/sinks', { credentials: 'include' });
        const sinksBody = (await sinksRes.json()) as {
          items: Array<{ id: string; name: string }>;
        };
        for (const s of sinksBody.items) {
          if (s.name === sName) {
            await fetch(`/api/v1/data-export/sinks/${encodeURIComponent(s.id)}`, {
              method: 'DELETE',
              credentials: 'include',
            });
          }
        }
      },
      { fKey: flagKey, mKey: metricKey, lKey: layerKey, eKey: expKey, sName: sinkName },
    );

    // 7. Zero unexpected runtime errors
    expect(pageErrors).toEqual([]);
    expect(
      consoleErrors.filter((msg) => !msg.includes('401') && !msg.includes('404') && !msg.includes('EventSource')),
    ).toEqual([]);
  });
});
