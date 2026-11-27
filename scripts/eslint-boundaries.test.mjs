import path from "node:path";
import { fileURLToPath } from "node:url";
import { ESLint } from "eslint";
import { describe, expect, it } from "vitest";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const eslint = new ESLint({
  cwd: REPO_ROOT,
  overrideConfigFile: path.join(REPO_ROOT, "eslint.config.mjs"),
});

async function lintSnippet(code, relFilePath) {
  const results = await eslint.lintText(code, {
    filePath: path.join(REPO_ROOT, relFilePath),
  });
  return results[0]?.messages ?? [];
}

describe("ESLint @skotos/ui <-> apps/web boundary enforcement", () => {
  describe("apps/web forbidden imports (negative tests)", () => {
    const forbiddenWebImports = [
      ["@radix-ui/react-dialog", 'import * as Dialog from "@radix-ui/react-dialog"; export { Dialog };'],
      ["radix-ui umbrella", 'import { Dialog } from "radix-ui"; export { Dialog };'],
      ["radix-ui subpath", 'import { Dialog } from "radix-ui/internal"; export { Dialog };'],
      ["tailwindcss side-effect", 'import "tailwindcss"; export const x = 1;'],
      ["@tailwindcss/vite", 'import tw from "@tailwindcss/vite"; export { tw };'],
      ["tailwind-merge", 'import { twMerge } from "tailwind-merge"; export { twMerge };'],
      [
        "class-variance-authority",
        'import { cva } from "class-variance-authority"; export { cva };',
      ],
      ["@skotos/ui/src internal", 'import { Button } from "@skotos/ui/src/Button"; export { Button };'],
      ["@skotos/ui/dist internal", 'import { Button } from "@skotos/ui/dist/index.js"; export { Button };'],
      [
        "relative packages/ui/src import",
        'import { Button } from "../../packages/ui/src/Button"; export { Button };',
      ],
      ["relative packages/ui import", 'import * as ui from "../packages/ui"; export { ui };'],
      ["re-export from @radix-ui", 'export { Dialog } from "@radix-ui/react-dialog";'],
      [
        "type-only import from @radix-ui",
        'import type { DialogProps } from "@radix-ui/react-dialog"; export type P = DialogProps;',
      ],
    ];

    it.each(forbiddenWebImports)("rejects %s in apps/web", async (_label, code) => {
      const messages = await lintSnippet(code, "apps/web/src/pages/FlagsPage.tsx");
      expect(messages).toEqual(
        expect.arrayContaining([
          expect.objectContaining({ ruleId: "no-restricted-imports", severity: 2 }),
        ]),
      );
    });

    it("rejects dynamic import() of forbidden packages in apps/web", async () => {
      const messages = await lintSnippet(
        'export const load = () => import("tailwindcss");',
        "apps/web/src/pages/DynamicImport.ts",
      );
      expect(messages).toEqual(
        expect.arrayContaining([
          expect.objectContaining({ ruleId: "no-restricted-syntax", severity: 2 }),
        ]),
      );
    });

    it("rejects any file placed under apps/web/src/components/ui/**", async () => {
      const messages = await lintSnippet(
        "export function LocalButton() { return <div>nope</div>; }",
        "apps/web/src/components/ui/LocalButton.tsx",
      );
      expect(messages).toEqual(
        expect.arrayContaining([
          expect.objectContaining({ ruleId: "no-restricted-syntax", severity: 2 }),
        ]),
      );
    });
  });

  describe("apps/web forbidden raw HTML elements (negative tests)", () => {
    const forbiddenElements = [
      ["button", "export function C() { return <button type=\"button\">Click</button>; }"],
      ["input", "export function C() { return <input type=\"text\" />; }"],
      ["select", "export function C() { return <select><option value=\"a\">A</option></select>; }"],
      ["textarea", "export function C() { return <textarea />; }"],
      ["dialog", "export function C() { return <dialog open>Hi</dialog>; }"],
      ["table", "export function C() { return <table><tbody><tr><td>Cell</td></tr></tbody></table>; }"],
      ["hr", "export function C() { return <hr />; }"],
    ];

    it.each(forbiddenElements)("rejects raw <%s> in apps/web/src/**/*.tsx", async (_tag, code) => {
      const messages = await lintSnippet(code, "apps/web/src/pages/Form.tsx");
      expect(messages).toEqual(
        expect.arrayContaining([
          expect.objectContaining({ ruleId: "react/forbid-elements", severity: 2 }),
        ]),
      );
    });
  });

  describe("apps/web allowed patterns (positive tests)", () => {
    it("allows public @skotos/ui, styles, tokens, and @skotos/api-client imports", async () => {
      const code = [
        'import "@skotos/ui/styles.css";',
        'import "@skotos/ui/tokens.css";',
        'import { SkotosShellStub } from "@skotos/ui";',
        'import { API_CLIENT_NAME } from "@skotos/api-client";',
        "export function Page() {",
        "  return <SkotosShellStub title={API_CLIENT_NAME} />;",
        "}",
      ].join("\n");

      const messages = await lintSnippet(code, "apps/web/src/pages/GoodPage.tsx");
      expect(messages).toEqual([]);
    });

    it("allows raw <button> in apps/web test files while STILL banning @radix-ui imports in tests", async () => {
      const allowedTest =
        'export function TestHarness() { return <button type="button">Trigger</button>; }';
      expect(await lintSnippet(allowedTest, "apps/web/src/pages/Page.test.tsx")).toEqual([]);

      const forbiddenTestImport =
        'import * as Dialog from "@radix-ui/react-dialog"; export { Dialog };';
      const messages = await lintSnippet(forbiddenTestImport, "apps/web/src/pages/Page.test.tsx");
      expect(messages).toEqual(
        expect.arrayContaining([
          expect.objectContaining({ ruleId: "no-restricted-imports", severity: 2 }),
        ]),
      );
    });
  });

  describe("packages/ui forbidden imports (negative tests)", () => {
    const forbiddenUiImports = [
      ["@skotos/api-client", 'import { API_CLIENT_NAME } from "@skotos/api-client"; export { API_CLIENT_NAME };'],
      [
        "@skotos/api-client subpath",
        'import type { paths } from "@skotos/api-client/schema"; export type P = paths;',
      ],
      ["@tanstack/react-query", 'import { useQuery } from "@tanstack/react-query"; export { useQuery };'],
      ["@tanstack/react-router", 'import { Link } from "@tanstack/react-router"; export { Link };'],
      ["react-router", 'import { useNavigate } from "react-router"; export { useNavigate };'],
      ["react-router-dom", 'import { Link } from "react-router-dom"; export { Link };'],
      ["tailwindcss", 'import "tailwindcss"; export const x = 1;'],
      ["@tailwindcss/postcss", 'import tw from "@tailwindcss/postcss"; export { tw };'],
      ["relative apps/web import", 'import App from "../../apps/web/src/App"; export { App };'],
      ["@skotos/web package", 'import * as web from "@skotos/web"; export { web };'],
    ];

    it.each(forbiddenUiImports)("rejects %s in packages/ui", async (_label, code) => {
      const messages = await lintSnippet(code, "packages/ui/src/BadComponent.tsx");
      expect(messages).toEqual(
        expect.arrayContaining([
          expect.objectContaining({ ruleId: "no-restricted-imports", severity: 2 }),
        ]),
      );
    });

    it("rejects dynamic import() of @skotos/api-client in packages/ui", async () => {
      const messages = await lintSnippet(
        'export const load = () => import("@skotos/api-client");',
        "packages/ui/src/DynamicBad.ts",
      );
      expect(messages).toEqual(
        expect.arrayContaining([
          expect.objectContaining({ ruleId: "no-restricted-syntax", severity: 2 }),
        ]),
      );
    });
  });

  describe("packages/ui allowed patterns (positive tests)", () => {
    it("allows @radix-ui primitives, raw HTML primitives, and @tanstack/react-table in packages/ui", async () => {
      const modalCode = [
        'import * as Dialog from "@radix-ui/react-dialog";',
        "export function Modal() {",
        "  return (",
        "    <Dialog.Root>",
        '      <button type="button">Open</button>',
        "    </Dialog.Root>",
        "  );",
        "}",
      ].join("\n");
      expect(await lintSnippet(modalCode, "packages/ui/src/Modal.tsx")).toEqual([]);

      const tableCode = [
        'import { useReactTable } from "@tanstack/react-table";',
        "export const tableFactory = useReactTable;",
      ].join("\n");
      expect(await lintSnippet(tableCode, "packages/ui/src/DataTable.tsx")).toEqual([]);
    });
  });
});
