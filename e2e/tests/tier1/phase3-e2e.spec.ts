import { expect, test } from '@playwright/test';
import { assertNoSeriousA11yViolations } from '../../src/ui-helpers';

test.describe('Phase 3 Live Stack End-to-End (Governance, Pipelines, Approvals, Live Debugger + WCAG AA)', () => {
  test('exercises Custom RBAC Roles, Teams/SSO, Release Pipelines, Approvals Queue, and Live Debugger with zero WCAG AA violations', async ({
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
    const roleKey = `pw-role-${ts}`;
    const roleName = `Playwright Role ${ts}`;
    const teamKey = `pw-team-${ts}`;
    const teamName = `Playwright Team ${ts}`;
    const pipeKey = `pw-pipe-${ts}`;
    const pipeName = `Playwright Pipeline ${ts}`;
    const flagKey = `pw-p3-flag-${ts}`;
    const flagName = `Playwright Phase 3 Flag ${ts}`;

    await page.goto('/');

    // 1. Sign in as bootstrap owner
    await expect(page.getByRole('button', { name: 'Sign in' })).toBeVisible();
    await page.getByLabel('Email').fill('admin@skotos.local');
    await page.getByLabel('Password').fill('SkotosDevPass123!');
    await page.getByRole('button', { name: 'Sign in' }).click();
    await expect(page.getByRole('heading', { name: 'Feature Flags', level: 1 })).toBeVisible();

    // 2. Custom RBAC Roles (/settings/roles) — create, verify WCAG AA, clean up
    await page.getByRole('button', { name: 'Custom Roles' }).click();
    await expect(page.getByRole('heading', { name: 'Custom RBAC Roles', level: 1 })).toBeVisible();
    await assertNoSeriousA11yViolations(page);

    await page.getByRole('button', { name: /Create Custom Role/i }).click();
    const roleDialog = page.getByRole('dialog');
    await roleDialog.getByLabel(/^Role Key\b/).fill(roleKey);
    await roleDialog.getByLabel(/^Role Name\b/).fill(roleName);
    await roleDialog.getByRole('button', { name: /^Create role$/i }).click();
    await expect(page.getByText(roleName)).toBeVisible();

    const delRoleStatus = await page.evaluate(async (k) => {
      const r = await fetch(`/api/v1/roles/${encodeURIComponent(k)}`, {
        method: 'DELETE',
        credentials: 'include',
      });
      return r.status;
    }, roleKey);
    expect(delRoleStatus).toBe(204);

    // 3. Teams, SSO & Compliance (/settings/teams) — create team, verify WCAG AA, clean up
    await page.getByRole('button', { name: 'Teams & SSO' }).click();
    await expect(
      page.getByRole('heading', { name: 'Teams, SSO/SCIM & Compliance', level: 1 }),
    ).toBeVisible();
    await assertNoSeriousA11yViolations(page);

    await page.getByRole('button', { name: /Create Team/i }).click();
    const teamDialog = page.getByRole('dialog');
    await teamDialog.getByLabel(/^Team Key\b/).fill(teamKey);
    await teamDialog.getByLabel(/^Team Name\b/).fill(teamName);
    await teamDialog.getByRole('button', { name: /^Create team$/i }).click();
    await expect(page.getByText(teamName)).toBeVisible();

    const delTeamStatus = await page.evaluate(async (k) => {
      const r = await fetch(`/api/v1/teams/${encodeURIComponent(k)}`, {
        method: 'DELETE',
        credentials: 'include',
      });
      return r.status;
    }, teamKey);
    expect(delTeamStatus).toBe(204);

    // 4. Release Pipelines & Applications (/projects/default/pipelines) — create pipeline, verify WCAG AA, clean up
    await page.getByRole('button', { name: 'Release Pipelines' }).click();
    await expect(
      page.getByRole('heading', { name: 'Release Pipelines & Applications', level: 1 }),
    ).toBeVisible();
    await assertNoSeriousA11yViolations(page);

    await page.getByRole('button', { name: /New Pipeline/i }).click();
    const pipeDialog = page.getByRole('dialog');
    await pipeDialog.getByLabel(/^Pipeline Key\b/).fill(pipeKey);
    await pipeDialog.getByLabel(/^Pipeline Name\b/).fill(pipeName);
    await pipeDialog.getByRole('button', { name: /^Create pipeline$/i }).click();
    await expect(page.getByText(pipeName)).toBeVisible();

    const delPipeStatus = await page.evaluate(async (k) => {
      const r = await fetch(`/api/v1/projects/default/release-pipelines/${encodeURIComponent(k)}`, {
        method: 'DELETE',
        credentials: 'include',
      });
      return r.status;
    }, pipeKey);
    expect(delPipeStatus).toBe(204);

    // 5. Create Flag -> Submit Approval Request -> Review in Approvals Queue (/approvals) + WCAG AA
    await page.getByRole('button', { name: 'Feature Flags' }).click();
    await page.getByRole('button', { name: /Create Flag/i }).first().click();
    const flagDialog = page.getByRole('dialog');
    await flagDialog.getByLabel(/^Name\b/).fill(flagName);
    await flagDialog.getByLabel(/^Key\b/).fill(flagKey);
    await flagDialog.getByRole('button', { name: /^Create Flag$/i }).click();
    await expect(page.getByText(flagName)).toBeVisible();

    await page.getByText(flagName).click();
    await expect(page.getByRole('heading', { name: flagName, level: 1 })).toBeVisible();

    // Toggle targeting and submit for peer approval
    await page.getByTestId('detail-targeting-switch').click();
    await page.getByRole('button', { name: /Review & Save/i }).click();
    const revDialog = page.getByRole('dialog');
    await revDialog.getByLabel(/Change comment/i).fill(`Approval test ${ts}`);
    await revDialog.getByRole('button', { name: /Request Approval/i }).click();

    await page.getByRole('button', { name: 'Approvals' }).click();
    await expect(page.getByRole('heading', { name: 'Approval Requests', level: 1 })).toBeVisible();
    await expect(page.getByText(`Approval test ${ts}`)).toBeVisible();
    await assertNoSeriousA11yViolations(page);

    // Cancel approval & clean up test flag
    await page.evaluate(async (key) => {
      const listRes = await fetch('/api/v1/approvals', { credentials: 'include' });
      const body = (await listRes.json()) as {
        items: Array<{ id: string; flagKey: string; status: string }>;
      };
      for (const a of body.items) {
        if (a.flagKey === key && a.status === 'pending') {
          await fetch(`/api/v1/approvals/${encodeURIComponent(a.id)}`, {
            method: 'DELETE',
            credentials: 'include',
          });
        }
      }
      await fetch(`/api/v1/projects/default/flags/${encodeURIComponent(key)}`, {
        method: 'DELETE',
        credentials: 'include',
      });
    }, flagKey);

    // 6. Live Events Debugger (/projects/default/debugger) — verify SSE page + WCAG AA
    await page.getByRole('button', { name: 'Live Debugger' }).click();
    await expect(page.getByRole('heading', { name: 'Live Events Debugger', level: 1 })).toBeVisible();
    await assertNoSeriousA11yViolations(page);

    // 7. Zero unexpected runtime errors
    expect(pageErrors).toEqual([]);
    expect(
      consoleErrors.filter((msg) => !msg.includes('401') && !msg.includes('EventSource')),
    ).toEqual([]);
  });
});
