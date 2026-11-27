import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";
import { checkUiBoundaries } from "./check-ui-boundaries.mjs";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const tempDirs = [];

function makeFixture(files = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "skotos-boundary-test-"));
  tempDirs.push(dir);

  const defaults = {
    "package.json": JSON.stringify({ name: "fixture-root", private: true }, null, 2),
    "pnpm-lock.yaml": "lockfileVersion: '9.0'\nimporters:\n  .:\n    devDependencies: {}\n",
    "packages/ui/package.json": JSON.stringify(
      {
        name: "@skotos/ui",
        private: true,
        dependencies: { "@radix-ui/react-dialog": "^1.0.0" },
      },
      null,
      2,
    ),
    "packages/ui/src/styles/tokens.css": ":root { --sk-color-bg: #ffffff; }\n",
    "apps/web/package.json": JSON.stringify(
      {
        name: "@skotos/web",
        private: true,
        dependencies: { "@skotos/ui": "workspace:*", react: "^19.0.0" },
      },
      null,
      2,
    ),
    "apps/web/src/App.css": "/* zero @tailwind here */\n.app { color: var(--sk-color-text); }\n",
  };

  const merged = { ...defaults, ...files };
  for (const [relPath, content] of Object.entries(merged)) {
    const absPath = path.join(dir, relPath);
    fs.mkdirSync(path.dirname(absPath), { recursive: true });
    fs.writeFileSync(absPath, content, "utf8");
  }

  return dir;
}

afterEach(() => {
  while (tempDirs.length > 0) {
    const dir = tempDirs.pop();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

describe("checkUiBoundaries", () => {
  it("passes on the live Skotos repository", () => {
    const result = checkUiBoundaries(REPO_ROOT);
    expect(result.violations).toEqual([]);
    expect(result.ok).toBe(true);
    expect(result.stats.packageJsonCount).toBeGreaterThanOrEqual(4);
    expect(result.stats.cssFileCount).toBeGreaterThanOrEqual(1);
  });

  it("passes on a clean fixture with @radix-ui in packages/ui, CSS comments, and ignored e2e/", () => {
    const dir = makeFixture({
      "e2e/package.json": JSON.stringify({
        name: "e2e",
        devDependencies: { tailwindcss: "^4.0.0" },
      }),
    });
    const result = checkUiBoundaries(dir);
    expect(result.violations).toEqual([]);
    expect(result.ok).toBe(true);
  });

  it("rejects tailwindcss in root package.json", () => {
    const dir = makeFixture({
      "package.json": JSON.stringify({
        name: "fixture-root",
        devDependencies: { tailwindcss: "^4.0.0" },
      }),
    });
    const result = checkUiBoundaries(dir);
    expect(result.ok).toBe(false);
    expect(result.violations).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ rule: "no-tailwind-package", file: "package.json" }),
      ]),
    );
  });

  it("rejects @tailwindcss/vite in apps/web/package.json", () => {
    const dir = makeFixture({
      "apps/web/package.json": JSON.stringify({
        name: "@skotos/web",
        devDependencies: { "@tailwindcss/vite": "^4.0.0" },
      }),
    });
    const result = checkUiBoundaries(dir);
    expect(result.ok).toBe(false);
    expect(result.violations).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ rule: "no-tailwind-package", file: "apps/web/package.json" }),
      ]),
    );
  });

  it("rejects tailwind-merge in packages/ui/package.json", () => {
    const dir = makeFixture({
      "packages/ui/package.json": JSON.stringify({
        name: "@skotos/ui",
        dependencies: { "tailwind-merge": "^3.0.0" },
      }),
    });
    const result = checkUiBoundaries(dir);
    expect(result.ok).toBe(false);
    expect(result.violations).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ rule: "no-tailwind-package", file: "packages/ui/package.json" }),
      ]),
    );
  });

  it("rejects Tailwind references in pnpm-lock.yaml without false-positiving non-tailwind names", () => {
    const cleanDir = makeFixture({
      "pnpm-lock.yaml":
        "lockfileVersion: '9.0'\n# comment mentioning tailwindcss\npackages:\n  not-tailwindcss@1.0.0:\n    resolution: {}\n",
    });
    expect(checkUiBoundaries(cleanDir).ok).toBe(true);

    const dirtyDir = makeFixture({
      "pnpm-lock.yaml":
        "lockfileVersion: '9.0'\npackages:\n  '@tailwindcss/oxide@4.0.0':\n    resolution: {}\n  tailwindcss@4.0.0:\n    resolution: {}\n",
    });
    const result = checkUiBoundaries(dirtyDir);
    expect(result.ok).toBe(false);
    expect(result.violations.filter((v) => v.rule === "no-tailwind-lockfile")).toHaveLength(2);
  });

  it("rejects tailwind.config.* anywhere in the repository", () => {
    const dir = makeFixture({
      "apps/web/tailwind.config.ts": "export default {};\n",
      "tailwind.config.mjs": "export default {};\n",
    });
    const result = checkUiBoundaries(dir);
    expect(result.ok).toBe(false);
    expect(result.violations.filter((v) => v.rule === "no-tailwind-config")).toHaveLength(2);
  });

  it.each(["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"])(
    "rejects @radix-ui/react-dialog in apps/web/package.json %s",
    (section) => {
      const dir = makeFixture({
        "apps/web/package.json": JSON.stringify({
          name: "@skotos/web",
          [section]: { "@radix-ui/react-dialog": "^1.0.0" },
        }),
      });
      const result = checkUiBoundaries(dir);
      expect(result.ok).toBe(false);
      expect(result.violations).toEqual(
        expect.arrayContaining([
          expect.objectContaining({ rule: "no-radix-in-web", file: "apps/web/package.json" }),
        ]),
      );
    },
  );

  it("rejects umbrella radix-ui and class-variance-authority in apps/web/package.json", () => {
    const dir = makeFixture({
      "apps/web/package.json": JSON.stringify({
        name: "@skotos/web",
        dependencies: {
          "radix-ui": "^1.0.0",
          "class-variance-authority": "^0.7.0",
        },
      }),
    });
    const result = checkUiBoundaries(dir);
    expect(result.ok).toBe(false);
    expect(result.violations.filter((v) => v.rule === "no-radix-in-web")).toHaveLength(2);
  });

  it("rejects API client, router, and React Query dependencies in packages/ui/package.json", () => {
    const dir = makeFixture({
      "packages/ui/package.json": JSON.stringify({
        name: "@skotos/ui",
        dependencies: {
          "@skotos/api-client": "workspace:*",
          "@tanstack/react-query": "^5.0.0",
          "@tanstack/react-router": "^1.0.0",
          "react-router-dom": "^7.0.0",
        },
      }),
    });
    const result = checkUiBoundaries(dir);
    expect(result.ok).toBe(false);
    expect(result.violations.filter((v) => v.rule === "no-domain-deps-in-ui")).toHaveLength(4);
  });

  it("rejects apps/web/src/components/ui directory", () => {
    const dir = makeFixture({
      "apps/web/src/components/ui/Button.tsx": "export const Button = () => null;\n",
    });
    const result = checkUiBoundaries(dir);
    expect(result.ok).toBe(false);
    expect(result.violations).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ rule: "no-local-ui-dir", file: "apps/web/src/components/ui" }),
      ]),
    );
  });

  it("rejects Tailwind CSS v3 and v4 directives under packages/ and apps/", () => {
    const dir = makeFixture({
      "packages/ui/src/styles/bad-v3.css": "@tailwind base;\n@tailwind utilities;\n",
      "apps/web/src/bad-v4.css": '@import "tailwindcss";\n@plugin "@tailwindcss/typography";\n',
    });
    const result = checkUiBoundaries(dir);
    expect(result.ok).toBe(false);
    const cssViolations = result.violations.filter((v) => v.rule === "no-tailwind-css");
    expect(cssViolations).toHaveLength(3);
  });
});
