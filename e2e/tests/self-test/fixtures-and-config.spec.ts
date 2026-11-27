import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { resolveChromiumExecutable } from '../../src/browser';
import {
  E2E_DEFAULTS,
  joinUrl,
  loadConfig,
  normalizeBaseUrl,
  normalizeDatabaseUrl,
  parseWorkers,
  resolveRepoRoot,
} from '../../src/config';
import { PHASE1_TABLES, SkotosDb, SkotosDbClient, createDb, sha256Hex } from '../../src/db';
import {
  CleanupTracker,
  SKOTOS_KEY_REGEX,
  SKOTOS_STRICT_SLUG_REGEX,
  uniqueEmail,
  uniqueKey,
  uniqueTokenName,
  type CleanupDeleteClient,
} from '../../src/fixtures';

test.describe('Fixtures, Config, Browser Resolver & DB Pure Helpers', () => {
  test('uniqueKey() generates 500 collision-free keys matching strict Skotos regexes', () => {
    const generated = new Set<string>();
    for (let i = 0; i < 500; i++) {
      const key = uniqueKey('e2e-tier1');
      expect(key.length).toBeLessThanOrEqual(63);
      expect(SKOTOS_KEY_REGEX.test(key), `invalid key: ${key}`).toBe(true);
      expect(SKOTOS_STRICT_SLUG_REGEX.test(key), `invalid slug: ${key}`).toBe(true);
      generated.add(key);
    }
    expect(generated.size).toBe(500);
  });

  test('uniqueKey(), uniqueEmail(), and uniqueTokenName() sanitize edge-case prefixes safely', () => {
    const prefixes = ['My_Bad!! Key/99', '', '   ', '---!!!___', 'X'.repeat(200)];
    const keys = prefixes.map((p) => uniqueKey(p));
    expect(new Set(keys).size).toBe(prefixes.length);
    for (const key of keys) {
      expect(key.length).toBeLessThanOrEqual(63);
      expect(SKOTOS_KEY_REGEX.test(key)).toBe(true);
      expect(SKOTOS_STRICT_SLUG_REGEX.test(key)).toBe(true);
    }

    const emailA = uniqueEmail('QA User!');
    const emailB = uniqueEmail('QA User!');
    expect(emailA).not.toBe(emailB);
    expect(emailA).toMatch(/^[a-z0-9][a-z0-9-]+@skotos\.test$/);

    const tokenName = uniqueTokenName('Admin Token');
    expect(SKOTOS_STRICT_SLUG_REGEX.test(tokenName)).toBe(true);
  });

  test('CleanupTracker runs all tracked resources in strict LIFO order and drains idempotently', async () => {
    const calls: string[] = [];
    const mockApi: CleanupDeleteClient = {
      async delete(resourcePath: string) {
        calls.push(`DELETE ${resourcePath}`);
        return { status: 204 };
      },
    };

    const tracker = new CleanupTracker(mockApi);
    tracker.trackProject('proj-1');
    tracker.trackEnvironment('proj-1', 'env-1');
    tracker.trackFlag('proj-1', 'flag-1');
    tracker.trackToken('tok-1');
    tracker.trackMember('mem-1');
    tracker.add('custom-last', async () => {
      calls.push('custom-last');
    });

    expect(tracker.size).toBe(6);

    const firstReport = await tracker.runAll();
    expect(firstReport).toEqual({
      total: 6,
      completed: 6,
      skipped404: 0,
      errors: [],
    });
    expect(calls).toEqual([
      'custom-last',
      'DELETE /api/v1/members/mem-1',
      'DELETE /api/v1/tokens/tok-1',
      'DELETE /api/v1/projects/proj-1/flags/flag-1',
      'DELETE /api/v1/projects/proj-1/environments/env-1',
      'DELETE /api/v1/projects/proj-1',
    ]);
    expect(tracker.size).toBe(0);

    // Second runAll() must be an idempotent no-op
    const secondReport = await tracker.runAll();
    expect(secondReport).toEqual({
      total: 0,
      completed: 0,
      skipped404: 0,
      errors: [],
    });
    expect(calls).toHaveLength(6);
  });

  test('CleanupTracker tolerates both HTTP 404 responses and thrown 404 errors cleanly', async () => {
    const api404Status: CleanupDeleteClient = {
      async delete() {
        return { status: 404 };
      },
    };
    const api404Throw: CleanupDeleteClient = {
      async delete() {
        const err = Object.assign(new Error('Already gone'), { status: 404 });
        throw err;
      },
    };

    const tracker = new CleanupTracker(api404Status);
    tracker.trackProject('already-deleted-project');
    tracker.trackToken('already-revoked-token');
    tracker.trackMember('already-deleted-member', api404Throw);

    const report = await tracker.runAll({ throwOnError: true });
    expect(report).toEqual({
      total: 3,
      completed: 0,
      skipped404: 3,
      errors: [],
    });
  });

  test('CleanupTracker collects errors without aborting earlier cleanups and throws AggregateError when requested', async () => {
    const executed: string[] = [];
    const api500: CleanupDeleteClient = {
      async delete(resourcePath: string) {
        executed.push(resourcePath);
        return { status: 500, rawText: 'db down' };
      },
    };

    const tracker = new CleanupTracker(api500);
    tracker.add('first-registered', async () => {
      executed.push('first-registered');
    });
    tracker.add('throwing-middle', async () => {
      executed.push('throwing-middle');
      throw new Error('boom');
    });
    tracker.trackProject('broken-project');

    const report = await tracker.runAll({ throwOnError: false });
    expect(executed).toEqual([
      '/api/v1/projects/broken-project',
      'throwing-middle',
      'first-registered',
    ]);
    expect(report.total).toBe(3);
    expect(report.completed).toBe(1);
    expect(report.skipped404).toBe(0);
    expect(report.errors).toHaveLength(2);
    expect(report.errors.map((e) => e.label)).toEqual([
      'project:broken-project',
      'throwing-middle',
    ]);

    // Verify throwOnError throws AggregateError containing all failed labels
    const tracker2 = new CleanupTracker(api500);
    tracker2.trackToken('bad-token');
    tracker2.add('bad-step', async () => {
      throw new Error('secondary boom');
    });
    await expect(tracker2.runAll({ throwOnError: true })).rejects.toThrow(AggregateError);
  });

  test('config.ts applies defaults, normalizes URLs, preserves passwords, and keeps chromeExecutable unset by default', () => {
    const defaultsCfg = loadConfig({});
    expect(defaultsCfg.controlApiUrl).toBe(E2E_DEFAULTS.controlApiUrl);
    expect(defaultsCfg.webAppUrl).toBe(E2E_DEFAULTS.webAppUrl);
    expect(defaultsCfg.databaseUrl).toBe(E2E_DEFAULTS.databaseUrl);
    expect(defaultsCfg.adminEmail).toBe(E2E_DEFAULTS.adminEmail);
    expect(defaultsCfg.adminPassword).toBe(E2E_DEFAULTS.adminPassword);
    expect(defaultsCfg.playwrightBrowsersPath).toBe('/opt/ms-playwright');
    expect(defaultsCfg.chromeExecutable).toBeUndefined();
    expect(defaultsCfg.chromeExecutableFallback).toBe('/usr/bin/google-chrome');
    expect(defaultsCfg.workers).toBe(1);

    const customCfg = loadConfig({
      CONTROL_API_URL: '  https://api.example.com/// ',
      WEB_APP_URL: 'http://127.0.0.1:4173/',
      DATABASE_URL: 'postgresql://u:p@localhost:5432/db',
      E2E_ADMIN_EMAIL: ' ops@skotos.test ',
      E2E_ADMIN_PASSWORD: '  keep-spaces!  ',
      CHROME_EXECUTABLE: '/opt/custom/chrome',
      E2E_CHROME_EXECUTABLE: '/usr/local/bin/my-chrome',
      E2E_WORKERS: '2',
    });
    expect(customCfg.controlApiUrl).toBe('https://api.example.com');
    expect(customCfg.webAppUrl).toBe('http://127.0.0.1:4173');
    expect(customCfg.databaseUrl).toBe('postgresql://u:p@localhost:5432/db');
    expect(customCfg.adminEmail).toBe('ops@skotos.test');
    expect(customCfg.adminPassword).toBe('  keep-spaces!  ');
    expect(customCfg.chromeExecutable).toBe('/usr/local/bin/my-chrome');
    expect(customCfg.chromeExecutableFallback).toBe('/opt/custom/chrome');
    expect(customCfg.workers).toBe(2);

    expect(() => normalizeBaseUrl('not-a-url', E2E_DEFAULTS.controlApiUrl, 'CONTROL_API_URL')).toThrow(
      /Invalid URL for CONTROL_API_URL/,
    );
    expect(() =>
      normalizeBaseUrl('ftp://127.0.0.1:21', E2E_DEFAULTS.controlApiUrl, 'CONTROL_API_URL'),
    ).toThrow(/Unsupported protocol/);
    expect(() => normalizeDatabaseUrl('mysql://localhost/skotos')).toThrow(/Invalid DATABASE_URL/);

    expect(joinUrl('http://127.0.0.1:8080/', '/api/v1/flags')).toBe(
      'http://127.0.0.1:8080/api/v1/flags',
    );
    expect(joinUrl('http://127.0.0.1:8080', 'healthz')).toBe('http://127.0.0.1:8080/healthz');
  });

  test('config.ts clamps E2E_WORKERS to [1, 2] and resolves E2E_REPO_ROOT independently of cwd', () => {
    const cases: Array<[string | undefined, 1 | 2]> = [
      [undefined, 1],
      ['', 1],
      ['   ', 1],
      ['abc', 1],
      ['NaN', 1],
      ['Infinity', 1],
      ['-Infinity', 1],
      ['1.5', 1],
      ['2px', 1],
      ['-5', 1],
      ['0', 1],
      ['1', 1],
      ['2', 2],
      ['3', 2],
      ['16', 2],
      ['999', 2],
    ];
    for (const [input, expected] of cases) {
      expect(parseWorkers(input), `input=${String(input)}`).toBe(expected);
    }

    // Default repoRoot resolves parent of e2e/ regardless of caller cwd
    const defaultRoot = resolveRepoRoot(undefined, { cwd: '/tmp' });
    expect(fs.existsSync(path.join(defaultRoot, 'PROJECT.md'))).toBe(true);
    expect(fs.existsSync(path.join(defaultRoot, 'TEST_INFRA.md'))).toBe(true);

    // Explicit relative and absolute overrides
    expect(resolveRepoRoot('./custom-repo', { cwd: '/tmp/workspace' })).toBe(
      '/tmp/workspace/custom-repo',
    );
    expect(resolveRepoRoot('/opt/skotos-alt', { cwd: '/tmp/workspace' })).toBe('/opt/skotos-alt');
  });

  test('browser.ts resolveChromiumExecutable follows exact 6-step precedence and detects host browser', () => {
    // 1a. Explicit valid E2E_CHROME_EXECUTABLE
    const explicit = resolveChromiumExecutable({
      env: { E2E_CHROME_EXECUTABLE: '/custom/chrome' },
      isExecutable: (p) => p === '/custom/chrome',
      nativeExecutablePath: () => undefined,
      readDir: () => [],
    });
    expect(explicit).toEqual({
      executablePath: '/custom/chrome',
      source: 'env:E2E_CHROME_EXECUTABLE',
    });

    // 1b. Explicit invalid E2E_CHROME_EXECUTABLE throws immediately (never silently falls back)
    expect(() =>
      resolveChromiumExecutable({
        env: { E2E_CHROME_EXECUTABLE: '/missing/chrome', CHROME_EXECUTABLE: '/usr/bin/google-chrome' },
        isExecutable: (p) => p === '/usr/bin/google-chrome',
        nativeExecutablePath: () => undefined,
        readDir: () => [],
      }),
    ).toThrow(/E2E_CHROME_EXECUTABLE/);

    // 2. Native Playwright match when both chromium-1255 and chromium_headless_shell-1255 exist
    const nativeMatch = resolveChromiumExecutable({
      env: { PLAYWRIGHT_BROWSERS_PATH: '/opt/ms-playwright' },
      nativeExecutablePath: () => '/opt/ms-playwright/chromium-1255/chrome-linux64/chrome',
      isExecutable: (p) =>
        p === '/opt/ms-playwright/chromium-1255/chrome-linux64/chrome' ||
        p ===
          '/opt/ms-playwright/chromium_headless_shell-1255/chrome-headless-shell-linux64/chrome-headless-shell',
      readDir: () => [],
    });
    expect(nativeMatch).toEqual({ executablePath: undefined, source: 'playwright-native' });

    // 3a. Revision drift: native rev 1300 missing, picks highest revision 1255 over 1200
    const revScan = resolveChromiumExecutable({
      env: { PLAYWRIGHT_BROWSERS_PATH: '/opt/ms-playwright' },
      nativeExecutablePath: () => '/opt/ms-playwright/chromium-1300/chrome-linux64/chrome',
      readDir: () => ['chromium-1200', 'chromium-1255', 'firefox-1572'],
      isExecutable: (p) =>
        p === '/opt/ms-playwright/chromium-1200/chrome-linux/chrome' ||
        p === '/opt/ms-playwright/chromium-1255/chrome-linux64/chrome',
    });
    expect(revScan).toEqual({
      executablePath: '/opt/ms-playwright/chromium-1255/chrome-linux64/chrome',
      source: 'playwright-cache-scan',
    });

    // 3b. Legacy chrome-linux & headless_shell fallback when full chromium dirs absent
    const shellScan = resolveChromiumExecutable({
      env: { PLAYWRIGHT_BROWSERS_PATH: '/opt/ms-playwright' },
      nativeExecutablePath: () => undefined,
      readDir: () => ['chromium_headless_shell-1100', 'chromium_headless_shell-1255'],
      isExecutable: (p) =>
        p === '/opt/ms-playwright/chromium_headless_shell-1255/chrome-linux/headless_shell',
    });
    expect(shellScan).toEqual({
      executablePath: '/opt/ms-playwright/chromium_headless_shell-1255/chrome-linux/headless_shell',
      source: 'playwright-cache-scan',
    });

    // 4. Ambient CHROME_EXECUTABLE fallback
    const ambient = resolveChromiumExecutable({
      env: { PLAYWRIGHT_BROWSERS_PATH: '/empty', CHROME_EXECUTABLE: '/opt/chrome/chrome' },
      nativeExecutablePath: () => undefined,
      readDir: () => [],
      isExecutable: (p) => p === '/opt/chrome/chrome',
    });
    expect(ambient).toEqual({
      executablePath: '/opt/chrome/chrome',
      source: 'env:CHROME_EXECUTABLE',
    });

    // 5. Well-known system Chrome fallback
    const system = resolveChromiumExecutable({
      env: { PLAYWRIGHT_BROWSERS_PATH: '/empty' },
      nativeExecutablePath: () => undefined,
      readDir: () => [],
      isExecutable: (p) => p === '/usr/bin/google-chrome',
    });
    expect(system).toEqual({
      executablePath: '/usr/bin/google-chrome',
      source: 'system-chrome',
    });

    // 6. Actionable error when nothing found
    expect(() =>
      resolveChromiumExecutable({
        env: { PLAYWRIGHT_BROWSERS_PATH: '/empty' },
        nativeExecutablePath: () => undefined,
        readDir: () => [],
        isExecutable: () => false,
      }),
    ).toThrow(/Unable to locate a Chromium or Google Chrome executable/);

    // Verify real host environment resolves cleanly without downloading anything
    const realHost = resolveChromiumExecutable();
    expect([
      'playwright-native',
      'playwright-cache-scan',
      'env:CHROME_EXECUTABLE',
      'system-chrome',
      'env:E2E_CHROME_EXECUTABLE',
    ]).toContain(realHost.source);
  });

  test('db.ts validates canonical Phase 1 tables, SHA-256 hashing, and lazy pool lifecycle without live Postgres', async () => {
    expect(SkotosDb).toBe(SkotosDbClient);
    expect(PHASE1_TABLES).toHaveLength(12);
    expect(PHASE1_TABLES).toContain('flag_env_revisions');
    expect(PHASE1_TABLES).not.toContain('flag_env_config_revisions' as unknown as string);
    expect(PHASE1_TABLES).toContain('audit_logs');
    expect(PHASE1_TABLES).toContain('outbox_events');
    expect(PHASE1_TABLES).toContain('segments');

    const digest = sha256Hex('sk_api_test_secret');
    expect(digest).toBe(
      crypto.createHash('sha256').update('sk_api_test_secret', 'utf8').digest('hex'),
    );
    expect(digest).toMatch(/^[0-9a-f]{64}$/);

    // createDb rejects invalid URLs immediately
    expect(() => createDb('')).toThrow(/Invalid PostgreSQL connection string/);
    expect(() => createDb('http://127.0.0.1:5432/skotos')).toThrow(
      /Invalid PostgreSQL connection string/,
    );

    // Lazy pool creates and closes cleanly without connecting to Postgres
    const validDb = createDb('postgres://skotos:skotos@127.0.0.1:5432/skotos', { max: 3 });
    expect(validDb.isConfigured()).toBe(true);
    await validDb.close();
    // Second close() is idempotent
    await validDb.close();

    // Unconfigured SkotosDbClient reports isConfigured=false and throws cleanly on query()
    const unconfigured = new SkotosDbClient('');
    expect(unconfigured.isConfigured()).toBe(false);
    await expect(unconfigured.query('SELECT 1')).rejects.toThrow(/not configured/);
    await unconfigured.close();
  });
});
