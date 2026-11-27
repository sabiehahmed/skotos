#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const E2E_DIR = path.resolve(SCRIPT_DIR, '..');
const REPO_ROOT = path.resolve(E2E_DIR, '..');
const DEFAULT_CONFIG_PATH = path.join(E2E_DIR, 'playwright.config.ts');
const PLAYWRIGHT_CLI_PATH = path.join(E2E_DIR, 'node_modules', '@playwright', 'test', 'cli.js');

const VALID_TIERS = new Set(['1', '2', '3', '4', 'all']);
const ALL_TIER_PROJECTS = ['tier1', 'tier2', 'tier3', 'tier4'];

const USAGE = `Skotos Phase 1 Opaque-Box E2E Test Runner

Usage:
  node e2e/scripts/run-e2e.mjs --tier <1|2|3|4|all> [--list] [-- <playwright args...>]
  node e2e/scripts/run-e2e.mjs --self-test [--list] [-- <playwright args...>]
  node e2e/scripts/run-e2e.mjs --help

Options:
  --tier <1|2|3|4|all>   Select a single tier (1..4) or run all tiers sequentially (1 -> 2 -> 3 -> 4)
  --self-test            Run the live-stack-free harness self-test suite
  --list                 List discovered tests without executing them
  -h, --help             Show this help message
  --                     Forward all following arguments verbatim to Playwright

Environment Variables (defaults):
  CONTROL_API_URL          http://127.0.0.1:8080
  WEB_APP_URL              http://127.0.0.1:5173
  DATABASE_URL             postgres://skotos:skotos@127.0.0.1:5432/skotos
  E2E_ADMIN_EMAIL          admin@skotos.local
  E2E_ADMIN_PASSWORD       SkotosDevPass123!
  PLAYWRIGHT_BROWSERS_PATH /opt/ms-playwright
  E2E_CHROME_EXECUTABLE    <unset; auto-detects native /opt/ms-playwright or Chrome>
  E2E_WORKERS              1 (strictly 1 or 2)
  E2E_REPO_ROOT            ${REPO_ROOT}
`;

function failUsage(message) {
  console.error(`[run-e2e] Error: ${message}\n`);
  console.error(USAGE);
  process.exit(2);
}

function isProjectFlag(arg) {
  return arg === '--project' || arg.startsWith('--project=');
}

function parseCliArgs(argv) {
  let tier = null;
  let selfTest = false;
  let listMode = false;
  let help = false;
  const passthrough = [];

  let inExplicitPassthrough = false;
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];

    if (inExplicitPassthrough) {
      if (isProjectFlag(arg)) {
        failUsage('Cannot pass --project directly; use --tier <1|2|3|4|all> or --self-test instead.');
      }
      passthrough.push(arg);
      continue;
    }

    if (arg === '--') {
      inExplicitPassthrough = true;
      continue;
    }

    if (arg === '--help' || arg === '-h') {
      help = true;
      continue;
    }

    if (arg === '--self-test') {
      selfTest = true;
      continue;
    }

    if (arg === '--list') {
      listMode = true;
      continue;
    }

    if (arg === '--tier') {
      if (tier !== null) {
        failUsage('Duplicate --tier flag provided.');
      }
      const next = argv[i + 1];
      if (next === undefined || next.startsWith('-')) {
        failUsage('Missing value for --tier. Valid values: 1, 2, 3, 4, all.');
      }
      if (!VALID_TIERS.has(next)) {
        failUsage(`Invalid --tier value "${next}". Valid values: 1, 2, 3, 4, all.`);
      }
      tier = next;
      i++;
      continue;
    }

    if (arg.startsWith('--tier=')) {
      if (tier !== null) {
        failUsage('Duplicate --tier flag provided.');
      }
      const value = arg.slice('--tier='.length);
      if (!value) {
        failUsage('Missing value for --tier. Valid values: 1, 2, 3, 4, all.');
      }
      if (!VALID_TIERS.has(value)) {
        failUsage(`Invalid --tier value "${value}". Valid values: 1, 2, 3, 4, all.`);
      }
      tier = value;
      continue;
    }

    if (isProjectFlag(arg)) {
      failUsage('Cannot pass --project directly; use --tier <1|2|3|4|all> or --self-test instead.');
    }

    passthrough.push(arg);
  }

  return { tier, selfTest, listMode, help, passthrough };
}

function normalizeCustomConfigArgs(passthrough, callerCwd) {
  let hasCustomConfig = false;
  const normalized = [];

  for (let i = 0; i < passthrough.length; i++) {
    const arg = passthrough[i];
    if (arg === '--config' || arg === '-c') {
      hasCustomConfig = true;
      normalized.push(arg);
      const next = passthrough[i + 1];
      if (next !== undefined) {
        normalized.push(path.isAbsolute(next) ? next : path.resolve(callerCwd, next));
        i++;
      }
      continue;
    }
    if (arg.startsWith('--config=')) {
      hasCustomConfig = true;
      const val = arg.slice('--config='.length);
      const resolved = val && !path.isAbsolute(val) ? path.resolve(callerCwd, val) : val;
      normalized.push(`--config=${resolved}`);
      continue;
    }
    if (arg.startsWith('-c=')) {
      hasCustomConfig = true;
      const val = arg.slice('-c='.length);
      const resolved = val && !path.isAbsolute(val) ? path.resolve(callerCwd, val) : val;
      normalized.push(`-c=${resolved}`);
      continue;
    }
    normalized.push(arg);
  }

  return { hasCustomConfig, args: normalized };
}

function countSpecFilesRecursive(dirPath) {
  if (!fs.existsSync(dirPath)) {
    return 0;
  }
  let count = 0;
  const stack = [dirPath];
  while (stack.length > 0) {
    const current = stack.pop();
    let entries;
    try {
      entries = fs.readdirSync(current, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      const fullPath = path.join(current, entry.name);
      if (entry.isDirectory()) {
        stack.push(fullPath);
      } else if (entry.isFile() && entry.name.endsWith('.spec.ts')) {
        count++;
      }
    }
  }
  return count;
}

function validateAndBuildEnv(baseEnv) {
  const childEnv = { ...baseEnv };

  if (childEnv.E2E_WORKERS !== undefined) {
    const trimmed = childEnv.E2E_WORKERS.trim();
    if (trimmed === '') {
      childEnv.E2E_WORKERS = '1';
    } else if (!/^[12]$/.test(trimmed)) {
      failUsage(
        `Invalid E2E_WORKERS="${childEnv.E2E_WORKERS}". Hardware-safe bounds require strictly "1" or "2".`,
      );
    } else {
      childEnv.E2E_WORKERS = trimmed;
    }
  } else {
    childEnv.E2E_WORKERS = '1';
  }

  const defaults = {
    CONTROL_API_URL: 'http://127.0.0.1:8080',
    WEB_APP_URL: 'http://127.0.0.1:5173',
    DATABASE_URL: 'postgres://skotos:skotos@127.0.0.1:5432/skotos',
    E2E_ADMIN_EMAIL: 'admin@skotos.local',
    E2E_ADMIN_PASSWORD: 'SkotosDevPass123!',
    PLAYWRIGHT_BROWSERS_PATH: '/opt/ms-playwright',
    E2E_REPO_ROOT: REPO_ROOT,
  };

  for (const [key, defaultValue] of Object.entries(defaults)) {
    const current = childEnv[key];
    if (current === undefined || (key !== 'E2E_ADMIN_PASSWORD' ? current.trim() === '' : current === '')) {
      childEnv[key] = defaultValue;
    }
  }

  // Preserve E2E_CHROME_EXECUTABLE ONLY if explicitly provided non-empty by caller.
  if (childEnv.E2E_CHROME_EXECUTABLE !== undefined && childEnv.E2E_CHROME_EXECUTABLE.trim() === '') {
    delete childEnv.E2E_CHROME_EXECUTABLE;
  }

  return childEnv;
}

function main() {
  const parsed = parseCliArgs(process.argv.slice(2));

  if (parsed.help) {
    process.stdout.write(USAGE);
    process.exit(0);
  }

  if (parsed.selfTest && parsed.tier !== null) {
    failUsage('Cannot combine --self-test with --tier. Specify one or the other.');
  }

  if (!parsed.selfTest && parsed.tier === null) {
    failUsage('Must specify either --tier <1|2|3|4|all> or --self-test.');
  }

  const childEnv = validateAndBuildEnv(process.env);

  if (!fs.existsSync(PLAYWRIGHT_CLI_PATH)) {
    console.error(
      `[run-e2e] Error: Playwright CLI not found at ${PLAYWRIGHT_CLI_PATH}.\n` +
        'Install isolated E2E dependencies first:\n' +
        '  PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 npm --prefix e2e install --no-audit --no-fund',
    );
    process.exit(1);
  }

  const { hasCustomConfig, args: passthroughArgs } = normalizeCustomConfigArgs(
    parsed.passthrough,
    process.cwd(),
  );

  const projects = parsed.selfTest
    ? ['self-test']
    : parsed.tier === 'all'
      ? ALL_TIER_PROJECTS
      : [`tier${parsed.tier}`];

  for (const project of projects) {
    const isTierProject = project !== 'self-test';
    const modeLabel = parsed.listMode ? 'Listing' : 'Running';
    console.log(`[run-e2e] === ${modeLabel} ${project} ===`);

    if (parsed.listMode && isTierProject && !hasCustomConfig) {
      const specCount = countSpecFilesRecursive(path.join(E2E_DIR, 'tests', project));
      if (specCount === 0) {
        console.log(`[run-e2e] ${project}: directory not yet populated (0 spec files)`);
      }
    }

    const pwArgs = [PLAYWRIGHT_CLI_PATH, 'test'];
    if (!hasCustomConfig) {
      pwArgs.push('--config', DEFAULT_CONFIG_PATH);
    }
    pwArgs.push('--project', project);

    if (parsed.listMode) {
      pwArgs.push('--list');
      // Pass --pass-with-no-tests ONLY when listing tier1..tier4 (NEVER in --self-test,
      // NEVER during real test execution). Playwright still enforces failOnLoadErrors: true.
      if (isTierProject) {
        pwArgs.push('--pass-with-no-tests');
      }
    }

    pwArgs.push(...passthroughArgs);

    const result = spawnSync(process.execPath, pwArgs, {
      cwd: E2E_DIR,
      env: childEnv,
      stdio: 'inherit',
    });

    if (result.error) {
      console.error(`[run-e2e] Failed to spawn Playwright for ${project}: ${result.error.message}`);
      process.exit(1);
    }

    const exitCode = result.status ?? (result.signal ? 128 : 1);
    if (exitCode !== 0) {
      if (projects.length > 1) {
        console.error(
          `[run-e2e] ${project} failed (exit ${exitCode}). Stopping before remaining tiers.`,
        );
      }
      process.exit(exitCode);
    }
  }

  if (parsed.tier === 'all') {
    if (parsed.listMode) {
      console.log('[run-e2e] Completed listing all tiers (tier1..tier4).');
    } else {
      console.log('[run-e2e] All tiers (tier1..tier4) passed.');
    }
  }
}

main();
