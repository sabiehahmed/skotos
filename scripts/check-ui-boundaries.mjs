import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SKIP_DIRS = new Set([
  "node_modules",
  ".git",
  "target",
  "dist",
  "build",
  "coverage",
  ".agents",
  "e2e",
  "playwright-report",
  "test-results",
]);

const TAILWIND_PKG_RE = /^(@tailwindcss\/|tailwindcss$|tailwind-merge$)/i;
const TAILWIND_CONFIG_RE = /^tailwind\.config\..+$/i;
const TAILWIND_LOCKFILE_RE =
  /(?:^|[\s'":/])(@tailwindcss\/[a-z0-9._-]+|tailwindcss|tailwind-merge)(?:@|:|['"\s]|$)/i;
const WEB_FORBIDDEN_PKG_RE = /^(@radix-ui\/|radix-ui$|class-variance-authority$)/i;
const UI_FORBIDDEN_PKG_RE =
  /^(@skotos\/api-client$|@skotos\/web$|@tanstack\/react-query$|@tanstack\/react-router$|react-router$|react-router-dom$)/i;

const TAILWIND_CSS_CHECKS = [
  {
    pattern: /^\s*@tailwind\b/m,
    description: "Tailwind CSS v3 '@tailwind' directive is forbidden",
  },
  {
    pattern: /^\s*@import\s+["']tailwindcss(?:\/[^"']*)?["']/m,
    description: "Tailwind CSS v4 '@import \"tailwindcss\"' directive is forbidden",
  },
  {
    pattern: /^\s*@plugin\s+["']@tailwindcss\//m,
    description: "Tailwind CSS v4 '@plugin \"@tailwindcss/...\"' directive is forbidden",
  },
];

const PACKAGE_DEP_OBJECT_SECTIONS = [
  "dependencies",
  "devDependencies",
  "peerDependencies",
  "optionalDependencies",
  "overrides",
];

const PACKAGE_DEP_ARRAY_SECTIONS = ["bundledDependencies", "bundleDependencies"];

function toRelPath(rootDir, absPath) {
  return path.relative(rootDir, absPath).split(path.sep).join("/");
}

function walkFiles(dir, onFile) {
  if (!fs.existsSync(dir)) {
    return;
  }

  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }

  for (const entry of entries) {
    if (entry.isDirectory() && SKIP_DIRS.has(entry.name)) {
      continue;
    }

    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      walkFiles(fullPath, onFile);
    } else if (entry.isFile()) {
      onFile(fullPath, entry.name);
    }
  }
}

function collectDependencyNames(pkg, includeOverrides = false) {
  const results = [];
  const sections = includeOverrides
    ? PACKAGE_DEP_OBJECT_SECTIONS
    : PACKAGE_DEP_OBJECT_SECTIONS.filter((s) => s !== "overrides");

  for (const section of sections) {
    const obj = pkg[section];
    if (obj && typeof obj === "object" && !Array.isArray(obj)) {
      for (const name of Object.keys(obj)) {
        results.push({ section, name });
      }
    }
  }

  for (const section of PACKAGE_DEP_ARRAY_SECTIONS) {
    const arr = pkg[section];
    if (Array.isArray(arr)) {
      for (const name of arr) {
        if (typeof name === "string") {
          results.push({ section, name });
        }
      }
    }
  }

  if (
    includeOverrides &&
    pkg.pnpm &&
    typeof pkg.pnpm === "object" &&
    pkg.pnpm.overrides &&
    typeof pkg.pnpm.overrides === "object" &&
    !Array.isArray(pkg.pnpm.overrides)
  ) {
    for (const name of Object.keys(pkg.pnpm.overrides)) {
      results.push({ section: "pnpm.overrides", name });
    }
  }

  return results;
}

function stripCssComments(css) {
  return css.replace(/\/\*[\s\S]*?\*\//g, "");
}

/**
 * Audits monorepo UI ownership & zero-Tailwind boundaries (ADR-0006).
 * @param {string} rootDir
 * @returns {{ ok: boolean, violations: Array<{ rule: string, file: string, message: string }>, stats: { packageJsonCount: number, cssFileCount: number } }}
 */
export function checkUiBoundaries(rootDir) {
  const resolvedRoot = path.resolve(rootDir);
  const violations = [];
  const packageJsonFiles = [];
  let cssFileCount = 0;

  // Walk repository for package.json manifests and forbidden tailwind.config.* files
  walkFiles(resolvedRoot, (fullPath, name) => {
    if (name === "package.json") {
      packageJsonFiles.push(fullPath);
    }
    if (TAILWIND_CONFIG_RE.test(name)) {
      violations.push({
        rule: "no-tailwind-config",
        file: toRelPath(resolvedRoot, fullPath),
        message: `Forbidden Tailwind config file detected: ${toRelPath(resolvedRoot, fullPath)}`,
      });
    }
  });

  // 1. Check every workspace package.json for Tailwind packages
  for (const pkgPath of packageJsonFiles) {
    const relPath = toRelPath(resolvedRoot, pkgPath);
    let pkg;
    try {
      pkg = JSON.parse(fs.readFileSync(pkgPath, "utf8"));
    } catch (err) {
      violations.push({
        rule: "invalid-package-json",
        file: relPath,
        message: `Failed to parse JSON: ${err instanceof Error ? err.message : String(err)}`,
      });
      continue;
    }

    for (const { section, name } of collectDependencyNames(pkg, true)) {
      if (TAILWIND_PKG_RE.test(name)) {
        violations.push({
          rule: "no-tailwind-package",
          file: relPath,
          message: `Forbidden Tailwind dependency '${name}' found in '${section}'`,
        });
      }
    }

    // 4. apps/web/package.json: forbid Radix UI and class-variance-authority
    if (relPath === "apps/web/package.json") {
      for (const { section, name } of collectDependencyNames(pkg, false)) {
        if (WEB_FORBIDDEN_PKG_RE.test(name)) {
          violations.push({
            rule: "no-radix-in-web",
            file: relPath,
            message: `Forbidden dependency '${name}' in apps/web '${section}' (ADR-0006: must live in @skotos/ui)`,
          });
        }
      }
    }

    // 5. packages/ui/package.json: reverse manifest guard (no API client, routers, or React Query)
    if (relPath === "packages/ui/package.json") {
      for (const { section, name } of collectDependencyNames(pkg, false)) {
        if (UI_FORBIDDEN_PKG_RE.test(name)) {
          violations.push({
            rule: "no-domain-deps-in-ui",
            file: relPath,
            message: `Forbidden app/domain dependency '${name}' in packages/ui '${section}' (ADR-0006)`,
          });
        }
      }
    }
  }

  // 2. Check pnpm-lock.yaml for Tailwind entries
  const lockfilePath = path.join(resolvedRoot, "pnpm-lock.yaml");
  if (fs.existsSync(lockfilePath)) {
    const lockContent = fs.readFileSync(lockfilePath, "utf8");
    const lines = lockContent.split(/\r?\n/);
    for (let i = 0; i < lines.length; i += 1) {
      const line = lines[i];
      if (!line || line.trimStart().startsWith("#")) {
        continue;
      }
      const match = line.match(TAILWIND_LOCKFILE_RE);
      if (match) {
        violations.push({
          rule: "no-tailwind-lockfile",
          file: "pnpm-lock.yaml",
          message: `Forbidden Tailwind package '${match[1]}' referenced in pnpm-lock.yaml at line ${i + 1}`,
        });
      }
    }
  }

  // 6. Forbid apps/web/src/components/ui (directory, file, or symlink)
  const localUiPath = path.join(resolvedRoot, "apps", "web", "src", "components", "ui");
  try {
    fs.lstatSync(localUiPath);
    violations.push({
      rule: "no-local-ui-dir",
      file: "apps/web/src/components/ui",
      message:
        "Forbidden path 'apps/web/src/components/ui' exists (ADR-0006: all reusable UI components MUST live in packages/ui)",
    });
  } catch {
    // Path does not exist — expected
  }

  // 7. Scan CSS files under packages/ and apps/ for Tailwind directives/imports
  for (const topDir of ["packages", "apps"]) {
    walkFiles(path.join(resolvedRoot, topDir), (fullPath, name) => {
      if (!name.endsWith(".css")) {
        return;
      }
      cssFileCount += 1;
      const rawCss = fs.readFileSync(fullPath, "utf8");
      const stripped = stripCssComments(rawCss);
      const relPath = toRelPath(resolvedRoot, fullPath);

      for (const check of TAILWIND_CSS_CHECKS) {
        if (check.pattern.test(stripped)) {
          violations.push({
            rule: "no-tailwind-css",
            file: relPath,
            message: `${check.description} (${relPath})`,
          });
        }
      }
    });
  }

  return {
    ok: violations.length === 0,
    violations,
    stats: {
      packageJsonCount: packageJsonFiles.length,
      cssFileCount,
    },
  };
}

const isDirectExecution =
  typeof process.argv[1] === "string" &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (isDirectExecution) {
  const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  const targetDir = process.argv[2] ? path.resolve(process.argv[2]) : repoRoot;
  const result = checkUiBoundaries(targetDir);

  if (!result.ok) {
    console.error(
      `UI boundary check FAILED with ${result.violations.length} violation(s) in ${targetDir}:`,
    );
    for (const v of result.violations) {
      console.error(`  [${v.rule}] ${v.file}: ${v.message}`);
    }
    process.exit(1);
  }

  console.log(
    `UI boundary check PASSED (${result.stats.packageJsonCount} package.json manifests, ${result.stats.cssFileCount} CSS files audited; 0 violations).`,
  );
}
