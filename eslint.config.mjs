import js from "@eslint/js";
import reactPlugin from "eslint-plugin-react";
import globals from "globals";
import tseslint from "typescript-eslint";

const WEB_FORBIDDEN_IMPORTS_MESSAGE =
  "apps/web must NEVER import Radix UI, Tailwind, class-variance-authority, or @skotos/ui internals (ADR-0006). Import UI components strictly from '@skotos/ui'.";

const UI_FORBIDDEN_IMPORTS_MESSAGE =
  "@skotos/ui must remain 100% pure UI with zero API client, router, query, Tailwind, or apps/web coupling (ADR-0006).";

export default [
  // Standalone global ignores object (ESLint 9 flat config)
  {
    ignores: [
      "**/dist/**",
      "**/build/**",
      "**/coverage/**",
      "**/node_modules/**",
      "**/target/**",
      "e2e/**",
      ".agents/**",
      "playwright-report/**",
      "test-results/**",
    ],
  },

  js.configs.recommended,
  ...tseslint.configs.recommended,

  // Browser/TS baseline across workspace packages and apps
  {
    files: ["packages/**/*.{ts,tsx}", "apps/**/*.{ts,tsx}"],
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: "module",
      globals: {
        ...globals.browser,
        ...globals.es2022,
      },
    },
    rules: {
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
    },
  },

  // Node scripts and root/package Vite & Vitest config files
  {
    files: [
      "*.{js,mjs,cjs}",
      "scripts/**/*.{js,mjs,cjs}",
      "**/vite.config.ts",
      "**/vitest.config.ts",
    ],
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: "module",
      globals: {
        ...globals.node,
      },
    },
    rules: {
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
    },
  },

  // React 19 flat config scoped to packages/ui and apps/web
  {
    files: ["packages/ui/**/*.{ts,tsx}", "apps/web/**/*.{ts,tsx}"],
    ...reactPlugin.configs.flat.recommended,
    ...reactPlugin.configs.flat["jsx-runtime"],
    plugins: {
      react: reactPlugin,
    },
    languageOptions: {
      ...reactPlugin.configs.flat.recommended.languageOptions,
      ...reactPlugin.configs.flat["jsx-runtime"].languageOptions,
      globals: {
        ...globals.browser,
        ...globals.es2022,
      },
    },
    settings: {
      react: {
        version: "19",
      },
    },
    rules: {
      ...reactPlugin.configs.flat.recommended.rules,
      ...reactPlugin.configs.flat["jsx-runtime"].rules,
      "react/prop-types": "off",
    },
  },

  // Boundary 1: apps/web MUST NOT import Radix UI, Tailwind, CVA, or @skotos/ui internals
  {
    files: ["apps/web/**/*.{ts,tsx}"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              regex: "^@radix-ui(/|$)",
              message: WEB_FORBIDDEN_IMPORTS_MESSAGE,
            },
            {
              regex: "^radix-ui(/|$)",
              message: WEB_FORBIDDEN_IMPORTS_MESSAGE,
            },
            {
              regex: "^(@tailwindcss/|tailwindcss(/|$)|tailwind-merge(/|$))",
              message: WEB_FORBIDDEN_IMPORTS_MESSAGE,
            },
            {
              regex: "^class-variance-authority(/|$)",
              message: WEB_FORBIDDEN_IMPORTS_MESSAGE,
            },
            {
              regex: "^@skotos/ui/(?!styles\\.css$|tokens\\.css$).+",
              message: WEB_FORBIDDEN_IMPORTS_MESSAGE,
            },
            {
              regex: "(^|[/\\\\])packages[/\\\\]ui([/\\\\]|$)",
              message: WEB_FORBIDDEN_IMPORTS_MESSAGE,
            },
          ],
        },
      ],
      "no-restricted-syntax": [
        "error",
        {
          selector:
            "ImportExpression[source.value=/^(@radix-ui|radix-ui|tailwindcss|@tailwindcss|tailwind-merge|class-variance-authority)(\\/|$)|^@skotos\\/ui\\/(?!styles\\.css$|tokens\\.css$).+|(^|[\\/\\\\])packages[\\/\\\\]ui([\\/\\\\]|$)/i]",
          message: WEB_FORBIDDEN_IMPORTS_MESSAGE,
        },
      ],
    },
  },

  // Boundary 2: apps/web/src/components/ui/** shadow UI library is forbidden (ADR-0006)
  {
    files: ["apps/web/src/components/ui/**/*.{ts,tsx,js,jsx}"],
    rules: {
      "no-restricted-syntax": [
        "error",
        {
          selector: "Program",
          message:
            "apps/web/src/components/ui is forbidden (ADR-0006): reusable UI components MUST live in packages/ui (@skotos/ui).",
        },
      ],
    },
  },

  // Boundary 3: apps/web/src/**/*.tsx (excluding tests) MUST NOT render raw forbidden HTML primitives
  {
    files: ["apps/web/src/**/*.tsx"],
    ignores: ["**/*.test.tsx", "**/*.spec.tsx"],
    rules: {
      "react/forbid-elements": [
        "error",
        {
          forbid: [
            {
              element: "button",
              message: "use <Button> or <IconButton> from '@skotos/ui' (ADR-0006)",
            },
            {
              element: "input",
              message: "use <Input>, <Checkbox>, or <Switch> from '@skotos/ui' (ADR-0006)",
            },
            {
              element: "select",
              message: "use <Select> or <Combobox> from '@skotos/ui' (ADR-0006)",
            },
            {
              element: "textarea",
              message: "use <Textarea> from '@skotos/ui' (ADR-0006)",
            },
            {
              element: "dialog",
              message: "use <Dialog>, <AlertDialog>, or <Drawer> from '@skotos/ui' (ADR-0006)",
            },
            {
              element: "table",
              message: "use <DataTable> from '@skotos/ui' (ADR-0006)",
            },
            {
              element: "hr",
              message: "use <Separator> from '@skotos/ui' (ADR-0006)",
            },
          ],
        },
      ],
    },
  },

  // Boundary 4: packages/ui MUST NOT import API client, routers, React Query, Tailwind, or apps/web
  {
    files: ["packages/ui/**/*.{ts,tsx}"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              regex: "^@skotos/api-client(/|$)",
              message: UI_FORBIDDEN_IMPORTS_MESSAGE,
            },
            {
              regex: "^@tanstack/react-(query|router)(/|$)",
              message: UI_FORBIDDEN_IMPORTS_MESSAGE,
            },
            {
              regex: "^react-router(-dom)?(/|$)",
              message: UI_FORBIDDEN_IMPORTS_MESSAGE,
            },
            {
              regex: "^(@tailwindcss/|tailwindcss(/|$)|tailwind-merge(/|$))",
              message: UI_FORBIDDEN_IMPORTS_MESSAGE,
            },
            {
              regex: "(^|[/\\\\])apps[/\\\\]web([/\\\\]|$)|^@skotos/web(/|$)",
              message: UI_FORBIDDEN_IMPORTS_MESSAGE,
            },
          ],
        },
      ],
      "no-restricted-syntax": [
        "error",
        {
          selector:
            "ImportExpression[source.value=/^(@skotos\\/api-client|@tanstack\\/react-(query|router)|react-router(-dom)?|tailwindcss|@tailwindcss|tailwind-merge|@skotos\\/web)(\\/|$)|(^|[\\/\\\\])apps[\\/\\\\]web([\\/\\\\]|$)/i]",
          message: UI_FORBIDDEN_IMPORTS_MESSAGE,
        },
      ],
    },
  },
];
