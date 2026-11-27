import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { expect, test } from '@playwright/test';

const THIS_DIR = path.dirname(fileURLToPath(import.meta.url));
const E2E_DIR = path.resolve(THIS_DIR, '../..');
const REPO_ROOT = path.resolve(E2E_DIR, '..');
const RUNNER_PATH = path.join(E2E_DIR, 'scripts', 'run-e2e.mjs');
const PLAYWRIGHT_ESM_URL = pathToFileURL(
  path.join(E2E_DIR, 'node_modules', '@playwright', 'test', 'index.mjs'),
).href;

function buildCleanChildEnv(overrides: Record<string, string | undefined> = {}): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = { ...process.env };
  // Strip outer Playwright worker env vars so nested Playwright runs stay completely isolated.
  delete env.TEST_WORKER_INDEX;
  delete env.TEST_PARALLEL_INDEX;
  delete env.PW_TEST_HTML_REPORT_OPEN;
  delete env.PLAYWRIGHT_HTML_OPEN;
  delete env.E2E_WORKERS;

  for (const [key, value] of Object.entries(overrides)) {
    if (value === undefined) {
      delete env[key];
    } else {
      env[key] = value;
    }
  }
  return env;
}

function runCli(args: string[], envOverrides: Record<string, string | undefined> = {}) {
  return spawnSync(process.execPath, [RUNNER_PATH, ...args], {
    cwd: REPO_ROOT,
    env: buildCleanChildEnv(envOverrides),
    encoding: 'utf8',
    timeout: 30_000,
  });
}

function createSyntheticTierWorkspace(options: {
  failTier?: number;
  captureEnvTier?: number;
}): {
  tmpDir: string;
  configPath: string;
  traceFile: string;
  envFile: string;
} {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'skotos-e2e-runner-'));
  const traceFile = path.join(tmpDir, 'trace.log');
  const envFile = path.join(tmpDir, 'env.json');

  for (const n of [1, 2, 3, 4]) {
    const tierDir = path.join(tmpDir, `tier${n}`);
    fs.mkdirSync(tierDir, { recursive: true });

    const extraLines: string[] = [];
    if (options.captureEnvTier === n) {
      extraLines.push(
        `fs.writeFileSync(${JSON.stringify(envFile)}, JSON.stringify({`,
        `  PLAYWRIGHT_BROWSERS_PATH: process.env.PLAYWRIGHT_BROWSERS_PATH ?? null,`,
        `  CONTROL_API_URL: process.env.CONTROL_API_URL ?? null,`,
        `  E2E_WORKERS: process.env.E2E_WORKERS ?? null,`,
        `  E2E_REPO_ROOT: process.env.E2E_REPO_ROOT ?? null,`,
        `  E2E_CHROME_EXECUTABLE: process.env.E2E_CHROME_EXECUTABLE ?? null,`,
        `}));`,
      );
    }
    if (options.failTier === n) {
      extraLines.push('expect(1).toBe(2);');
    } else {
      extraLines.push(`expect(${n}).toBe(${n});`);
    }

    const specSource = [
      `import fs from 'node:fs';`,
      `import { expect, test } from ${JSON.stringify(PLAYWRIGHT_ESM_URL)};`,
      `test('synthetic tier${n}', () => {`,
      `  fs.appendFileSync(${JSON.stringify(traceFile)}, 'tier${n}\\n');`,
      ...extraLines.map((line) => `  ${line}`),
      `});`,
      '',
    ].join('\n');

    fs.writeFileSync(path.join(tierDir, `tier${n}.spec.ts`), specSource, 'utf8');
  }

  const configPath = path.join(tmpDir, 'playwright.config.mjs');
  const configSource = `
export default {
  fullyParallel: false,
  workers: 1,
  retries: 0,
  outputDir: ${JSON.stringify(path.join(tmpDir, 'out'))},
  reporter: 'list',
  projects: [
    { name: 'tier1', testDir: ${JSON.stringify(path.join(tmpDir, 'tier1'))}, testMatch: '**/*.spec.ts' },
    { name: 'tier2', testDir: ${JSON.stringify(path.join(tmpDir, 'tier2'))}, testMatch: '**/*.spec.ts' },
    { name: 'tier3', testDir: ${JSON.stringify(path.join(tmpDir, 'tier3'))}, testMatch: '**/*.spec.ts' },
    { name: 'tier4', testDir: ${JSON.stringify(path.join(tmpDir, 'tier4'))}, testMatch: '**/*.spec.ts' },
  ],
};
`;
  fs.writeFileSync(configPath, configSource, 'utf8');

  return { tmpDir, configPath, traceFile, envFile };
}

test.describe('CLI Runner (e2e/scripts/run-e2e.mjs)', () => {
  test('--help prints usage and exits 0', () => {
    const res = runCli(['--help']);
    expect(res.status).toBe(0);
    expect(res.stdout).toContain('--tier <1|2|3|4|all>');
    expect(res.stdout).toContain('--self-test');
    expect(res.stdout).toContain('--list');
  });

  test('--tier 1..4 --list each exit 0 cleanly on unpopulated tier directories', () => {
    for (const tier of ['1', '2', '3', '4']) {
      const res = runCli(['--tier', tier, '--list']);
      expect(res.status, `tier${tier} stderr: ${res.stderr}`).toBe(0);
      expect(res.stdout).toContain(`Listing tier${tier}`);
    }
  });

  test('--tier all --list exits 0 and lists tier1 through tier4 in order', () => {
    const res = runCli(['--tier', 'all', '--list']);
    expect(res.status, `stderr: ${res.stderr}`).toBe(0);

    const idx1 = res.stdout.indexOf('Listing tier1');
    const idx2 = res.stdout.indexOf('Listing tier2');
    const idx3 = res.stdout.indexOf('Listing tier3');
    const idx4 = res.stdout.indexOf('Listing tier4');

    expect(idx1).toBeGreaterThanOrEqual(0);
    expect(idx2).toBeGreaterThan(idx1);
    expect(idx3).toBeGreaterThan(idx2);
    expect(idx4).toBeGreaterThan(idx3);
  });

  test('--self-test --list exits 0 and discovers all 5 self-test specs', () => {
    const res = runCli(['--self-test', '--list']);
    expect(res.status, `stderr: ${res.stderr}`).toBe(0);
    expect(res.stdout).toContain('cli-runner.spec.ts');
    expect(res.stdout).toContain('api-client.spec.ts');
    expect(res.stdout).toContain('eval-helpers.spec.ts');
    expect(res.stdout).toContain('fixtures-and-config.spec.ts');
    expect(res.stdout).toContain('browser-axe.spec.ts');
  });

  test('rejects invalid CLI invocations and invalid E2E_WORKERS with exit code 2', () => {
    const invalidInvocations: Array<{
      args: string[];
      env?: Record<string, string | undefined>;
      expectedMsg: RegExp;
    }> = [
      { args: [], expectedMsg: /Must specify either --tier/i },
      { args: ['--list'], expectedMsg: /Must specify either --tier/i },
      { args: ['--tier'], expectedMsg: /Missing value for --tier/i },
      { args: ['--tier='], expectedMsg: /Missing value for --tier/i },
      { args: ['--tier', '--list'], expectedMsg: /Missing value for --tier/i },
      { args: ['--tier', 'foo'], expectedMsg: /Invalid --tier value "foo"/i },
      { args: ['--tier', '0'], expectedMsg: /Invalid --tier value "0"/i },
      { args: ['--tier', '5'], expectedMsg: /Invalid --tier value "5"/i },
      { args: ['--tier=tier1'], expectedMsg: /Invalid --tier value "tier1"/i },
      { args: ['--tier', '1', '--tier', '2'], expectedMsg: /Duplicate --tier/i },
      { args: ['--self-test', '--tier', '1'], expectedMsg: /Cannot combine --self-test with --tier/i },
      { args: ['--tier', '1', '--project=tier2'], expectedMsg: /Cannot pass --project/i },
      { args: ['--tier', '1', '--', '--project', 'tier2'], expectedMsg: /Cannot pass --project/i },
      {
        args: ['--tier', '1', '--list'],
        env: { E2E_WORKERS: '4' },
        expectedMsg: /Invalid E2E_WORKERS="4"/i,
      },
      {
        args: ['--tier', '1', '--list'],
        env: { E2E_WORKERS: '0' },
        expectedMsg: /Invalid E2E_WORKERS="0"/i,
      },
      {
        args: ['--tier', '1', '--list'],
        env: { E2E_WORKERS: 'abc' },
        expectedMsg: /Invalid E2E_WORKERS="abc"/i,
      },
    ];

    for (const item of invalidInvocations) {
      const res = runCli(item.args, item.env);
      expect(res.status, `args=${JSON.stringify(item.args)}`).toBe(2);
      expect(res.stderr, `args=${JSON.stringify(item.args)}`).toMatch(item.expectedMsg);
    }
  });

  test('unknown Playwright option fails fast with non-zero exit identifying the flag', () => {
    const res = runCli(['--tier', '1', '--list', '--bogus-flag']);
    expect(res.status).not.toBe(0);
    expect(`${res.stderr}\n${res.stdout}`).toContain('--bogus-flag');
  });

  test('unpopulated --tier 1 in real execution mode fails non-zero (no false green)', () => {
    const res = runCli(['--tier', '1']);
    expect(res.status).not.toBe(0);
    expect(`${res.stderr}\n${res.stdout}`).toMatch(/No tests found/i);
  });

  test('--tier all executes tier1 -> tier2 -> tier3 -> tier4 strictly sequentially', () => {
    const ws = createSyntheticTierWorkspace({});
    try {
      const res = runCli(['--tier', 'all', '--', '--config', ws.configPath, '--reporter=list']);
      expect(res.status, `stderr: ${res.stderr}\nstdout: ${res.stdout}`).toBe(0);
      const lines = fs.readFileSync(ws.traceFile, 'utf8').trim().split('\n');
      expect(lines).toEqual(['tier1', 'tier2', 'tier3', 'tier4']);
    } finally {
      fs.rmSync(ws.tmpDir, { recursive: true, force: true });
    }
  });

  test('--tier all stops immediately on first failing tier (tier3 and tier4 never run)', () => {
    const ws = createSyntheticTierWorkspace({ failTier: 2 });
    try {
      const res = runCli(['--tier', 'all', '--', '--config', ws.configPath, '--reporter=list']);
      expect(res.status).not.toBe(0);
      const lines = fs.readFileSync(ws.traceFile, 'utf8').trim().split('\n');
      expect(lines).toEqual(['tier1', 'tier2']);
      expect(res.stderr).toContain('tier2 failed');
    } finally {
      fs.rmSync(ws.tmpDir, { recursive: true, force: true });
    }
  });

  test('propagates default and custom environment variables without forcing E2E_CHROME_EXECUTABLE', () => {
    const ws = createSyntheticTierWorkspace({ captureEnvTier: 1 });
    try {
      const res = runCli(['--tier=1', '--', `--config=${ws.configPath}`, '--reporter=list'], {
        PLAYWRIGHT_BROWSERS_PATH: undefined,
        E2E_CHROME_EXECUTABLE: undefined,
        CONTROL_API_URL: 'http://127.0.0.1:9999',
        E2E_WORKERS: '2',
      });
      expect(res.status, `stderr: ${res.stderr}\nstdout: ${res.stdout}`).toBe(0);
      const captured = JSON.parse(fs.readFileSync(ws.envFile, 'utf8')) as Record<
        string,
        string | null
      >;
      expect(captured.PLAYWRIGHT_BROWSERS_PATH).toBe('/opt/ms-playwright');
      expect(captured.CONTROL_API_URL).toBe('http://127.0.0.1:9999');
      expect(captured.E2E_WORKERS).toBe('2');
      expect(captured.E2E_REPO_ROOT).toBe(REPO_ROOT);
      expect(captured.E2E_CHROME_EXECUTABLE).toBeNull();
    } finally {
      fs.rmSync(ws.tmpDir, { recursive: true, force: true });
    }
  });
});
