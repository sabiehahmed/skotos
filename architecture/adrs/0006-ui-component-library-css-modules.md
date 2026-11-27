# ADR-0006: Standalone UI Component Library & Zero-Tailwind CSS Modules Architecture

- **Status:** Accepted
- **Date:** 2026-11-27
- **Deciders:** Skotos Core Team

## Context and Problem Statement

Skotos's Management Dashboard includes dense, mission-critical engineering interfaces: targeting rule builders, rollout sliders, environment diff modals, audit timelines, experimentation charts, AI prompt playgrounds, and observability views. Mixing raw UI primitives, styles, and product business logic inside a single app leads to inconsistent styling, fragile refactors, and poor testability.

## Decision Drivers

- **Strict UI ownership boundary**: The main product application (`apps/web`) must **never** own UI components — all reusable UI components must live in a dedicated, standalone UI component library package.
- **Accessible, composable shadcn-style component API** (built on headless accessibility primitives such as Radix UI) **without Tailwind CSS**.
- **CSS Modules (`.module.css`) + CSS Custom Properties (design tokens)** for deterministic scoping, zero utility-class clutter, fast Vite bundling, and first-class theming (light/dark + environment color coding).
- **Latest React + Latest TypeScript + Vite + Vitest** across both the UI library and main dashboard app.

## Considered Options

1. **Standalone `@skotos/ui` workspace package (React + TS + Vite library mode + Vitest + Radix UI primitives + CSS Modules `.module.css`, zero Tailwind), consumed exclusively by `apps/web`**
2. Components co-located inside `apps/web/src/components` with Tailwind CSS
3. Third-party monolithic UI kit (MUI / Ant Design)

## Decision Outcome

Chosen option: **Standalone `@skotos/ui` (`packages/ui`) with Radix UI primitives + CSS Modules (`.module.css`) and strict boundary enforcement**:

### 1. Package Ownership & Architectural Boundary

| Package                          | Owns                                                                                                                                                                                                                                                                                                                                                           | Forbidden                                                                                               |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| **`packages/ui` (`@skotos/ui`)** | Design tokens (`tokens.css`), typography, themes (light/dark), ALL UI primitives (Button, Input, Select, Dialog, Popover, DropdownMenu, Switch, Tabs, Tooltip, Toast, Badge, Table, Card, CodeBlock, DiffViewer, Slider, Charts, Layout shells, RuleBuilder widgets), component unit/a11y tests (`Vitest` + Testing Library), and component showcase/workbench | Product API calls (`@skotos/api-client`), router state, auth tokens, or Skotos business-domain fetching |
| **`apps/web`**                   | Routing, auth session integration, TanStack Query data fetching via `@skotos/api-client`, domain state, and page composition importing UI strictly from `@skotos/ui`                                                                                                                                                                                           | Defining reusable UI components, importing `@radix-ui/*` directly, or introducing Tailwind CSS          |

### 2. Styling Standard (Zero Tailwind)

- **Design Tokens (`packages/ui/src/styles/tokens.css`)**:
  - Semantic CSS Custom Properties (`--sk-color-bg`, `--sk-color-surface`, `--sk-color-border`, `--sk-color-text`, `--sk-color-accent`, `--sk-color-danger`, `--sk-env-accent`, `--sk-space-*`, `--sk-radius-*`, `--sk-font-sans`, `--sk-font-mono`, `--sk-shadow-*`).
  - Theme switching via `data-theme="light" | "dark"` on root.
- **Component Styles (`*.module.css`)**:
  - Every component in `packages/ui` ships with a co-located `ComponentName.module.css` using CSS custom properties and `data-variant` / `data-size` / `data-state` attributes (pairing cleanly with Radix UI state attributes like `data-state="open"`).
  - Lightweight `clsx` helper (or `class-variance-authority` optional with CSS Module class maps) for composing variants cleanly.
- **Build & Test**:
  - Built with **Vite** (library mode emitting ESM + `.d.ts` + bundled `styles.css`, or direct workspace source resolution in dev for instant HMR).
  - Every component has **Vitest** + `@testing-library/react` unit & interaction tests inside `packages/ui`.

### 3. Automated Boundary Enforcement

- `apps/web` does **not** list `@radix-ui/*` or styling frameworks in its `package.json` dependencies.
- ESLint configuration in `apps/web` enforces:
  - `no-restricted-imports` disallowing direct `@radix-ui/*` imports or relative cross-package reaches into `packages/ui/src/*` (must import from `@skotos/ui` public barrel).
  - Zero `tailwindcss` anywhere in `package.json` across the entire monorepo.
  - Component PR rule: any new UI primitive or reusable visual control is added and tested in `packages/ui` first, then consumed in `apps/web`.

### Positive Consequences

- Crystal-clear separation of concerns: UI design system evolves, themes, and tests independently from product workflows.
- Zero Tailwind dependency or build bloat; clean, inspectable CSS Modules with standard CSS variables.
- `@skotos/ui` can be reused across future frontends (e.g., Docs portal, Admin console, Embeddable Flag Debugger widget, or open-source self-hosted UI) with zero copy-paste.

### Negative Consequences / Tradeoffs

- Adding a new UI pattern takes one extra step (implement & export in `packages/ui` before wiring it in `apps/web`).
  - _Mitigation_: pnpm workspace linking with Vite HMR gives instant live-reload across `packages/ui` -> `apps/web` during development with zero manual rebuild step.

## Links & References

- [ADR-0001: Core Tech Stack & Polyglot Monorepo](0001-tech-stack-and-monorepo.md)
- [UI & UX Specification](../../design/ui-ux.md)
