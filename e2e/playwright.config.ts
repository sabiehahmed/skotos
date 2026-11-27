import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from '@playwright/test';
import { resolveChromiumExecutable } from './src/browser';
import { loadConfig, parseWorkers } from './src/config';

const E2E_DIR = path.dirname(fileURLToPath(import.meta.url));
const cfg = loadConfig(process.env, { e2eDir: E2E_DIR });
const workers = parseWorkers(process.env.E2E_WORKERS);
const resolvedBrowser = resolveChromiumExecutable();

export default defineConfig({
  fullyParallel: false,
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  workers,
  timeout: 60_000,
  expect: {
    timeout: 10_000,
  },
  outputDir: path.join(E2E_DIR, 'test-results'),
  reporter: [
    ['list'],
    ['html', { outputFolder: path.join(E2E_DIR, 'playwright-report'), open: 'never' }],
  ],
  use: {
    browserName: 'chromium',
    headless: true,
    baseURL: cfg.webAppUrl,
    viewport: { width: 1280, height: 800 },
    actionTimeout: 15_000,
    navigationTimeout: 20_000,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'off',
    launchOptions: {
      ...(resolvedBrowser.executablePath ? { executablePath: resolvedBrowser.executablePath } : {}),
      chromiumSandbox: process.env.E2E_CHROMIUM_SANDBOX === '1',
      args: ['--disable-dev-shm-usage'],
    },
  },
  projects: [
    {
      name: 'self-test',
      testDir: path.join(E2E_DIR, 'tests/self-test'),
      testMatch: '**/*.spec.ts',
    },
    {
      name: 'tier1',
      testDir: path.join(E2E_DIR, 'tests/tier1'),
      testMatch: '**/*.spec.ts',
    },
    {
      name: 'tier2',
      testDir: path.join(E2E_DIR, 'tests/tier2'),
      testMatch: '**/*.spec.ts',
    },
    {
      name: 'tier3',
      testDir: path.join(E2E_DIR, 'tests/tier3'),
      testMatch: '**/*.spec.ts',
    },
    {
      name: 'tier4',
      testDir: path.join(E2E_DIR, 'tests/tier4'),
      testMatch: '**/*.spec.ts',
    },
  ],
});
