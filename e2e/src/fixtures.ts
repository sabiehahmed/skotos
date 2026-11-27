import crypto from 'node:crypto';
import { test as base, expect, type Page } from '@playwright/test';
import { SkotosHttpClient, type SkotosApiClient } from './api-client';
import { joinUrl, loadConfig, type E2EConfig } from './config';
import { createDb, type SkotosDbClient } from './db';

export { expect };

export const SKOTOS_KEY_REGEX = /^[a-z0-9][a-z0-9_-]{0,62}$/;
export const SKOTOS_STRICT_SLUG_REGEX = /^[a-z0-9][a-z0-9-]{1,62}$/;

let keySeq = 0;

/**
 * Generates a collision-free, lowercase Skotos resource key matching the strictest intersection
 * of project/env/flag/org-slug rules (^[a-z0-9][a-z0-9-]{1,62}$ and <= 63 chars).
 */
export function uniqueKey(prefix = 'e2e'): string {
  const sanitized = prefix
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');

  const ts = Date.now().toString(36);
  const seq = (++keySeq).toString(36);
  const rand = crypto.randomBytes(4).toString('hex');
  const suffix = `${ts}-${seq}-${rand}`;

  const maxPrefixLen = Math.min(28, 63 - suffix.length - 1);
  const basePrefix = sanitized.length > 0 ? sanitized : 'e2e';
  const safePrefix =
    basePrefix.slice(0, Math.max(1, maxPrefixLen)).replace(/-+$/, '') || 'e2e';

  return `${safePrefix}-${suffix}`;
}

export function uniqueEmail(prefix = 'e2e-user'): string {
  return `${uniqueKey(prefix)}@skotos.test`;
}

export function uniqueTokenName(prefix = 'e2e-token'): string {
  return uniqueKey(prefix);
}

// ============================================================================
// CleanupTracker — Strict LIFO Teardown with 404 Tolerance & Full Error Capture
// ============================================================================

export interface CleanupDeleteClient {
  delete(path: string): Promise<{ status: number; rawText?: string }>;
}

export interface CleanupFailure {
  readonly label: string;
  readonly error: unknown;
}

export interface CleanupReport {
  readonly total: number;
  readonly completed: number;
  readonly skipped404: number;
  readonly errors: readonly CleanupFailure[];
}

type CleanupStepOutcome = 'completed' | 'skipped404';

interface CleanupEntry {
  readonly label: string;
  readonly run: () => Promise<CleanupStepOutcome>;
}

function hasStatus404(err: unknown): boolean {
  return (
    typeof err === 'object' &&
    err !== null &&
    'status' in err &&
    (err as { status?: unknown }).status === 404
  );
}

export class CleanupTracker {
  private readonly defaultApi?: CleanupDeleteClient;
  private readonly entries: CleanupEntry[] = [];

  constructor(defaultApi?: CleanupDeleteClient) {
    this.defaultApi = defaultApi;
  }

  public get size(): number {
    return this.entries.length;
  }

  public add(label: string, fn: () => Promise<void>): void {
    this.entries.push({
      label,
      run: async () => {
        await fn();
        return 'completed';
      },
    });
  }

  private addDelete(label: string, resourcePath: string, api?: CleanupDeleteClient): void {
    const client = api ?? this.defaultApi;
    this.entries.push({
      label,
      run: async () => {
        if (!client) {
          throw new Error(
            `Cleanup failed for ${label}: no API client provided to DELETE ${resourcePath}`,
          );
        }
        try {
          const res = await client.delete(resourcePath);
          if (res.status === 200 || res.status === 202 || res.status === 204) {
            return 'completed';
          }
          if (res.status === 404) {
            return 'skipped404';
          }
          const detail = res.rawText ? ` (${res.rawText.slice(0, 200)})` : '';
          throw new Error(
            `Cleanup failed for ${label}: DELETE ${resourcePath} returned HTTP ${res.status}${detail}`,
          );
        } catch (err) {
          if (hasStatus404(err)) {
            return 'skipped404';
          }
          throw err;
        }
      },
    });
  }

  public trackProject(projectKey: string, api?: CleanupDeleteClient): void {
    this.addDelete(
      `project:${projectKey}`,
      `/api/v1/projects/${encodeURIComponent(projectKey)}`,
      api,
    );
  }

  public trackEnvironment(projectKey: string, envKey: string, api?: CleanupDeleteClient): void {
    this.addDelete(
      `environment:${projectKey}/${envKey}`,
      `/api/v1/projects/${encodeURIComponent(projectKey)}/environments/${encodeURIComponent(envKey)}`,
      api,
    );
  }

  public trackFlag(projectKey: string, flagKey: string, api?: CleanupDeleteClient): void {
    this.addDelete(
      `flag:${projectKey}/${flagKey}`,
      `/api/v1/projects/${encodeURIComponent(projectKey)}/flags/${encodeURIComponent(flagKey)}`,
      api,
    );
  }

  public trackToken(tokenId: string, api?: CleanupDeleteClient): void {
    this.addDelete(`token:${tokenId}`, `/api/v1/tokens/${encodeURIComponent(tokenId)}`, api);
  }

  public trackMember(memberId: string, api?: CleanupDeleteClient): void {
    this.addDelete(`member:${memberId}`, `/api/v1/members/${encodeURIComponent(memberId)}`, api);
  }

  public async runAll(options?: { throwOnError?: boolean }): Promise<CleanupReport> {
    const pending = this.entries.splice(0, this.entries.length);
    let completed = 0;
    let skipped404 = 0;
    const errors: CleanupFailure[] = [];

    for (let i = pending.length - 1; i >= 0; i--) {
      const entry = pending[i];
      try {
        const outcome = await entry.run();
        if (outcome === 'skipped404') {
          skipped404++;
        } else {
          completed++;
        }
      } catch (error) {
        errors.push({ label: entry.label, error });
      }
    }

    const report: CleanupReport = {
      total: pending.length,
      completed,
      skipped404,
      errors,
    };

    if (options?.throwOnError && errors.length > 0) {
      const summary = errors
        .map((f) => `${f.label}: ${f.error instanceof Error ? f.error.message : String(f.error)}`)
        .join('; ');
      throw new AggregateError(
        errors.map((f) => f.error),
        `Cleanup encountered ${errors.length} failure(s): ${summary}`,
      );
    }

    return report;
  }
}

// ============================================================================
// Browser Login Helpers (Opaque-Box WAI-ARIA Locators)
// ============================================================================

export async function loginAs(
  page: Page,
  credentials: { email: string; password: string },
  webAppUrl?: string,
): Promise<void> {
  const baseUrl = webAppUrl ?? loadConfig().webAppUrl;
  const currentUrl = page.url();
  let alreadyOnLogin = false;
  try {
    alreadyOnLogin = new URL(currentUrl).pathname.startsWith('/login');
  } catch {
    alreadyOnLogin = false;
  }

  if (!alreadyOnLogin) {
    await page.goto(joinUrl(baseUrl, '/login'));
  }

  const emailInput = page.getByLabel(/email/i);
  await expect(emailInput).toBeVisible({ timeout: 15_000 });
  await emailInput.fill(credentials.email);

  // <input type="password"> has no implicit ARIA textbox role — always locate via label.
  const passwordInput = page.getByLabel(/password/i);
  await expect(passwordInput).toBeVisible();
  await passwordInput.fill(credentials.password);

  const submitButton = page.getByRole('button', { name: /sign in|log in/i });
  await submitButton.click();

  await page.waitForURL((url) => !url.pathname.startsWith('/login'), { timeout: 15_000 });
  await expect(page.getByRole('main').or(page.getByRole('navigation')).first()).toBeVisible({
    timeout: 15_000,
  });
}

export async function loginAsAdmin(page: Page, cfg?: E2EConfig): Promise<void> {
  const resolved = cfg ?? loadConfig();
  await loginAs(
    page,
    { email: resolved.adminEmail, password: resolved.adminPassword },
    resolved.webAppUrl,
  );
}

// ============================================================================
// Playwright Custom Test Fixtures (Lazy-Evaluated Per Test / Worker)
// ============================================================================

export interface E2ETestFixtures {
  e2eConfig: E2EConfig;
  anonApi: SkotosApiClient;
  adminApi: SkotosApiClient;
  cleanup: CleanupTracker;
  loginAsAdmin: (page: Page) => Promise<void>;
}

export interface E2EWorkerFixtures {
  db: SkotosDbClient;
}

export const test = base.extend<E2ETestFixtures, E2EWorkerFixtures>({
  e2eConfig: async ({}, use) => {
    await use(loadConfig(process.env));
  },

  anonApi: async ({ e2eConfig }, use) => {
    const client = new SkotosHttpClient({ baseUrl: e2eConfig.controlApiUrl });
    await use(client);
  },

  adminApi: async ({ e2eConfig }, use) => {
    const client = new SkotosHttpClient({ baseUrl: e2eConfig.controlApiUrl });
    const loginRes = await client.login({
      email: e2eConfig.adminEmail,
      password: e2eConfig.adminPassword,
    });
    if (loginRes.status !== 200) {
      throw new Error(
        `adminApi fixture failed to authenticate bootstrap admin "${e2eConfig.adminEmail}" at ${e2eConfig.controlApiUrl}/api/v1/auth/login: HTTP ${loginRes.status} (${loginRes.rawText.slice(0, 300)})`,
      );
    }
    try {
      await use(client);
    } finally {
      try {
        await client.logout();
      } catch {
        // Best-effort teardown logout (ignore if test already logged out or invalidated session).
      }
    }
  },

  cleanup: async ({ adminApi }, use, testInfo) => {
    const tracker = new CleanupTracker(adminApi);
    try {
      await use(tracker);
    } finally {
      const report = await tracker.runAll({ throwOnError: false });
      if (report.errors.length > 0) {
        const details = report.errors
          .map(
            (f) =>
              `- ${f.label}: ${f.error instanceof Error ? (f.error.stack ?? f.error.message) : String(f.error)}`,
          )
          .join('\n');
        console.error(`[e2e cleanup] ${report.errors.length} teardown error(s):\n${details}`);
        await testInfo.attach('cleanup-errors.txt', {
          body: Buffer.from(details, 'utf8'),
          contentType: 'text/plain',
        });
        if (testInfo.status === 'passed') {
          throw new AggregateError(
            report.errors.map((f) => f.error),
            `Cleanup failed after passing test (${report.errors.length} error(s)):\n${details}`,
          );
        }
      }
    }
  },

  loginAsAdmin: async ({ e2eConfig }, use) => {
    await use(async (page: Page) => {
      await loginAsAdmin(page, e2eConfig);
    });
  },

  db: [
    async ({}, use) => {
      const cfg = loadConfig(process.env);
      const client = createDb(cfg.databaseUrl, { max: 3 });
      try {
        await use(client);
      } finally {
        await client.close();
      }
    },
    { scope: 'worker' },
  ],
});
