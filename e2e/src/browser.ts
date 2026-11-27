import fs from 'node:fs';
import path from 'node:path';
import { chromium } from '@playwright/test';

export type BrowserResolutionSource =
  | 'env:E2E_CHROME_EXECUTABLE'
  | 'playwright-native'
  | 'playwright-cache-scan'
  | 'env:CHROME_EXECUTABLE'
  | 'system-chrome';

export interface ResolvedBrowser {
  readonly executablePath?: string;
  readonly source: BrowserResolutionSource;
}

export interface BrowserResolveDeps {
  readonly env: NodeJS.ProcessEnv;
  readonly nativeExecutablePath: () => string | undefined;
  readonly isExecutable: (filePath: string) => boolean;
  readonly readDir: (dirPath: string) => string[];
}

export const SYSTEM_CHROME_CANDIDATES: readonly string[] = Object.freeze([
  '/usr/bin/google-chrome',
  '/usr/bin/google-chrome-stable',
  '/usr/bin/chromium',
  '/usr/bin/chromium-browser',
]);

const DEFAULT_BROWSERS_PATH = '/opt/ms-playwright';

function defaultIsExecutable(filePath: string): boolean {
  try {
    fs.accessSync(filePath, fs.constants.X_OK);
    return fs.statSync(filePath).isFile();
  } catch {
    return false;
  }
}

function defaultReadDir(dirPath: string): string[] {
  try {
    return fs.readdirSync(dirPath);
  } catch {
    return [];
  }
}

function defaultNativeExecutablePath(): string | undefined {
  try {
    return chromium.executablePath();
  } catch {
    return undefined;
  }
}

function headlessShellCandidates(rootDir: string, revision: string): string[] {
  const dir = path.join(rootDir, `chromium_headless_shell-${revision}`);
  return [
    path.join(dir, 'chrome-headless-shell-linux64', 'chrome-headless-shell'),
    path.join(dir, 'chrome-linux', 'headless_shell'),
  ];
}

function fullChromiumCandidates(rootDir: string, revision: string): string[] {
  const dir = path.join(rootDir, `chromium-${revision}`);
  return [
    path.join(dir, 'chrome-linux64', 'chrome'),
    path.join(dir, 'chrome-linux', 'chrome'),
  ];
}

function parseRevisionDirs(entries: readonly string[], prefix: string): string[] {
  const regex = new RegExp(`^${prefix}-(\\d+)$`);
  const parsed: Array<{ rev: number; revStr: string }> = [];
  for (const entry of entries) {
    const match = regex.exec(entry);
    if (match) {
      parsed.push({ rev: Number.parseInt(match[1], 10), revStr: match[1] });
    }
  }
  parsed.sort((a, b) => b.rev - a.rev);
  return parsed.map((item) => item.revStr);
}

/**
 * Resolves the Chromium executable with deterministic 6-step precedence so tests reuse
 * pre-installed /opt/ms-playwright binaries or system Chrome without downloading browsers.
 */
export function resolveChromiumExecutable(
  overrides?: Partial<BrowserResolveDeps>,
): ResolvedBrowser {
  const env = overrides?.env ?? process.env;
  const nativeExecutablePath = overrides?.nativeExecutablePath ?? defaultNativeExecutablePath;
  const isExecutable = overrides?.isExecutable ?? defaultIsExecutable;
  const readDir = overrides?.readDir ?? defaultReadDir;

  const checkedPaths: string[] = [];

  // 1. Explicit E2E_CHROME_EXECUTABLE override — fail fast if invalid/missing.
  const explicitOverride = env.E2E_CHROME_EXECUTABLE?.trim();
  if (explicitOverride && explicitOverride.length > 0) {
    checkedPaths.push(explicitOverride);
    if (!isExecutable(explicitOverride)) {
      throw new Error(
        `E2E_CHROME_EXECUTABLE is set to "${explicitOverride}", but the binary does not exist or is not executable.`,
      );
    }
    return {
      executablePath: explicitOverride,
      source: 'env:E2E_CHROME_EXECUTABLE',
    };
  }

  const browsersPath = env.PLAYWRIGHT_BROWSERS_PATH?.trim() || DEFAULT_BROWSERS_PATH;

  // 2. Native Playwright revision match (verify both main binary and headless shell).
  let nativePath: string | undefined;
  try {
    nativePath = nativeExecutablePath();
  } catch {
    nativePath = undefined;
  }

  if (nativePath) {
    checkedPaths.push(nativePath);
    if (isExecutable(nativePath)) {
      const fullRevMatch = /(?:^|[\\/])chromium-(\d+)(?:[\\/]|$)/.exec(nativePath);
      const headlessRevMatch = /(?:^|[\\/])chromium_headless_shell-(\d+)(?:[\\/]|$)/.exec(
        nativePath,
      );

      if (headlessRevMatch) {
        return { executablePath: undefined, source: 'playwright-native' };
      }

      if (fullRevMatch) {
        const rev = fullRevMatch[1];
        const parentCacheDir = path.resolve(nativePath, '..', '..', '..');
        const rootsToCheck =
          parentCacheDir === browsersPath ? [browsersPath] : [browsersPath, parentCacheDir];
        let matchedShell = false;
        for (const root of rootsToCheck) {
          for (const shellCandidate of headlessShellCandidates(root, rev)) {
            checkedPaths.push(shellCandidate);
            if (isExecutable(shellCandidate)) {
              matchedShell = true;
              break;
            }
          }
          if (matchedShell) break;
        }
        if (matchedShell) {
          return { executablePath: undefined, source: 'playwright-native' };
        }
      } else {
        return { executablePath: undefined, source: 'playwright-native' };
      }
    }
  }

  // 3. Descending revision scan of PLAYWRIGHT_BROWSERS_PATH (chromium-* then chromium_headless_shell-*).
  const cacheEntries = readDir(browsersPath);
  for (const rev of parseRevisionDirs(cacheEntries, 'chromium')) {
    for (const candidate of fullChromiumCandidates(browsersPath, rev)) {
      checkedPaths.push(candidate);
      if (isExecutable(candidate)) {
        return { executablePath: candidate, source: 'playwright-cache-scan' };
      }
    }
  }
  for (const rev of parseRevisionDirs(cacheEntries, 'chromium_headless_shell')) {
    for (const candidate of headlessShellCandidates(browsersPath, rev)) {
      checkedPaths.push(candidate);
      if (isExecutable(candidate)) {
        return { executablePath: candidate, source: 'playwright-cache-scan' };
      }
    }
  }

  // 4. Ambient CHROME_EXECUTABLE env var.
  const ambientChrome = env.CHROME_EXECUTABLE?.trim();
  if (ambientChrome && ambientChrome.length > 0) {
    checkedPaths.push(ambientChrome);
    if (isExecutable(ambientChrome)) {
      return { executablePath: ambientChrome, source: 'env:CHROME_EXECUTABLE' };
    }
  }

  // 5. Well-known Linux system Chrome/Chromium binaries.
  for (const systemCandidate of SYSTEM_CHROME_CANDIDATES) {
    checkedPaths.push(systemCandidate);
    if (isExecutable(systemCandidate)) {
      return { executablePath: systemCandidate, source: 'system-chrome' };
    }
  }

  // 6. Actionable failure reporting all inspected locations.
  throw new Error(
    [
      'Unable to locate a Chromium or Google Chrome executable for Playwright E2E tests.',
      `PLAYWRIGHT_BROWSERS_PATH=${browsersPath}`,
      `E2E_CHROME_EXECUTABLE=${env.E2E_CHROME_EXECUTABLE ?? '<unset>'}`,
      `CHROME_EXECUTABLE=${env.CHROME_EXECUTABLE ?? '<unset>'}`,
      `Checked paths (${checkedPaths.length}): ${checkedPaths.join(', ') || '<none>'}`,
      'Set PLAYWRIGHT_BROWSERS_PATH to a pre-installed Playwright browser cache or set E2E_CHROME_EXECUTABLE=/usr/bin/google-chrome.',
    ].join('\n'),
  );
}
