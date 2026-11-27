import path from 'node:path';
import { fileURLToPath } from 'node:url';

export interface E2EConfig {
  readonly controlApiUrl: string;
  readonly webAppUrl: string;
  readonly databaseUrl: string;
  readonly adminEmail: string;
  readonly adminPassword: string;
  readonly playwrightBrowsersPath: string;
  /** Explicit E2E_CHROME_EXECUTABLE override only; undefined lets Playwright prefer native cache. */
  readonly chromeExecutable: string | undefined;
  /** Ambient fallback Chrome binary (CHROME_EXECUTABLE or /usr/bin/google-chrome). */
  readonly chromeExecutableFallback: string;
  readonly workers: 1 | 2;
  readonly repoRoot: string;
}

export interface LoadConfigOptions {
  readonly e2eDir?: string;
  readonly cwd?: string;
}

export const E2E_DEFAULTS = Object.freeze({
  controlApiUrl: 'http://127.0.0.1:8080',
  webAppUrl: 'http://127.0.0.1:5173',
  databaseUrl: 'postgres://skotos:skotos@127.0.0.1:5432/skotos',
  adminEmail: 'admin@skotos.local',
  adminPassword: 'SkotosDevPass123!',
  playwrightBrowsersPath: '/opt/ms-playwright',
  chromeExecutableFallback: '/usr/bin/google-chrome',
  workers: 1 as const,
});

const DEFAULT_E2E_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

export function normalizeBaseUrl(
  raw: string | undefined,
  fallback: string,
  envName: string,
): string {
  const trimmed = raw?.trim();
  const candidate = (trimmed && trimmed.length > 0 ? trimmed : fallback).replace(/\/+$/, '');
  let parsed: URL;
  try {
    parsed = new URL(candidate);
  } catch {
    throw new Error(`Invalid URL for ${envName}: "${raw ?? ''}"`);
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    throw new Error(
      `Unsupported protocol for ${envName}: "${parsed.protocol}" (expected http: or https:)`,
    );
  }
  return candidate;
}

export function normalizeDatabaseUrl(
  raw: string | undefined,
  fallback: string = E2E_DEFAULTS.databaseUrl,
): string {
  const trimmed = raw?.trim();
  const candidate = trimmed && trimmed.length > 0 ? trimmed : fallback;
  if (!/^postgres(ql)?:\/\//i.test(candidate)) {
    throw new Error(
      `Invalid DATABASE_URL "${raw ?? ''}": must start with postgres:// or postgresql://`,
    );
  }
  try {
    new URL(candidate);
  } catch {
    throw new Error(`Invalid DATABASE_URL "${raw ?? ''}": malformed connection URI`);
  }
  return candidate;
}

export function parseWorkers(raw: string | undefined): 1 | 2 {
  if (raw === undefined) {
    return 1;
  }
  const trimmed = raw.trim();
  if (trimmed === '' || !/^[+-]?\d+$/.test(trimmed)) {
    return 1;
  }
  const parsed = Number.parseInt(trimmed, 10);
  if (!Number.isFinite(parsed) || Number.isNaN(parsed)) {
    return 1;
  }
  if (parsed <= 1) {
    return 1;
  }
  if (parsed >= 2) {
    return 2;
  }
  return 1;
}

export function resolveRepoRoot(
  raw: string | undefined,
  options?: LoadConfigOptions,
): string {
  const e2eDir = options?.e2eDir ? path.resolve(options.e2eDir) : DEFAULT_E2E_DIR;
  const trimmed = raw?.trim();
  if (!trimmed) {
    return path.resolve(e2eDir, '..');
  }
  if (path.isAbsolute(trimmed)) {
    return path.resolve(trimmed);
  }
  const cwd = options?.cwd ? path.resolve(options.cwd) : process.cwd();
  return path.resolve(cwd, trimmed);
}

export function joinUrl(baseUrl: string, pathname: string): string {
  const cleanBase = baseUrl.replace(/\/+$/, '');
  if (!pathname) {
    return cleanBase;
  }
  if (/^https?:\/\//i.test(pathname)) {
    return pathname;
  }
  const cleanPath = pathname.startsWith('/') ? pathname : `/${pathname}`;
  return `${cleanBase}${cleanPath}`;
}

export function loadConfig(
  env: NodeJS.ProcessEnv = process.env,
  options?: LoadConfigOptions,
): E2EConfig {
  const explicitChrome = env.E2E_CHROME_EXECUTABLE?.trim();
  const ambientChrome = env.CHROME_EXECUTABLE?.trim();
  const adminEmail = env.E2E_ADMIN_EMAIL?.trim();
  const adminPassword =
    env.E2E_ADMIN_PASSWORD !== undefined && env.E2E_ADMIN_PASSWORD !== ''
      ? env.E2E_ADMIN_PASSWORD
      : E2E_DEFAULTS.adminPassword;
  const browsersPath = env.PLAYWRIGHT_BROWSERS_PATH?.trim();

  return Object.freeze({
    controlApiUrl: normalizeBaseUrl(
      env.CONTROL_API_URL,
      E2E_DEFAULTS.controlApiUrl,
      'CONTROL_API_URL',
    ),
    webAppUrl: normalizeBaseUrl(env.WEB_APP_URL, E2E_DEFAULTS.webAppUrl, 'WEB_APP_URL'),
    databaseUrl: normalizeDatabaseUrl(env.DATABASE_URL, E2E_DEFAULTS.databaseUrl),
    adminEmail: adminEmail && adminEmail.length > 0 ? adminEmail : E2E_DEFAULTS.adminEmail,
    adminPassword,
    playwrightBrowsersPath:
      browsersPath && browsersPath.length > 0
        ? browsersPath
        : E2E_DEFAULTS.playwrightBrowsersPath,
    chromeExecutable: explicitChrome && explicitChrome.length > 0 ? explicitChrome : undefined,
    chromeExecutableFallback:
      ambientChrome && ambientChrome.length > 0
        ? ambientChrome
        : E2E_DEFAULTS.chromeExecutableFallback,
    workers: parseWorkers(env.E2E_WORKERS),
    repoRoot: resolveRepoRoot(env.E2E_REPO_ROOT, options),
  });
}

export const config: E2EConfig = loadConfig();
