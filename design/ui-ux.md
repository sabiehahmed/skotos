# UI & UX Specification (Finalized Architecture)

## Strict Frontend Component Ownership ([ADR-0006](../architecture/adrs/0006-ui-component-library-css-modules.md))

The frontend is split into two strictly decoupled packages inside our `pnpm` workspace:

1. **`packages/ui` (`@skotos/ui`) — Standalone UI Component Library**:
   - Built with **React (latest)**, **TypeScript (latest)**, **Vite**, **Vitest**, **Radix UI accessible primitives**, and **CSS Modules (`.module.css`)** + CSS custom properties (**zero Tailwind**).
   - Owns **100% of UI components**, design tokens (`tokens.css`), light/dark themes, layout shells, form controls, dialogs, tables, badges, rollout sliders, rule-builder widgets, diff viewers, charts, and component tests.
   - Contains **zero** business-domain API calls or router coupling.
2. **`apps/web` — Main Management Dashboard SPA**:
   - Built with **React (latest)**, **TypeScript (latest)**, **Vite**, **Vitest**, TanStack Router/Query, and `@skotos/api-client`.
   - **NEVER owns UI components**: imports all visual components exclusively from `@skotos/ui` and focuses purely on routing, auth session state, API queries/mutations, and domain page composition.
   - Enforced by ESLint boundary rules (no `@radix-ui/*` imports, no Tailwind, no reusable UI primitives defined inside `apps/web`).

## Phase 1 Core `@skotos/ui` Component Catalog

| Category                       | Components Owned by `@skotos/ui`                                                                                                                                   |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Tokens & Layout**            | `ThemeProvider`, `AppShell`, `Sidebar`, `TopBar`, `PageHeader`, `EnvironmentBanner`, `Stack`, `Grid`, `SplitPane`, `Card`, `Separator`                             |
| **Typography & Feedback**      | `Text`, `Heading`, `CodeBlock`, `Badge`, `StatusDot`, `Alert`, `ToastProvider`, `Skeleton`, `EmptyState`, `Spinner`                                                |
| **Inputs & Controls**          | `Button`, `IconButton`, `Input`, `Textarea`, `Select`, `Combobox`, `Checkbox`, `Switch`, `ToggleGroup`, `Slider`, `TagInput`, `KeyValueEditor`                     |
| **Overlays & Navigation**      | `Dialog`, `Drawer`, `Popover`, `DropdownMenu`, `Tooltip`, `Tabs`, `Breadcrumb`, `Pagination`, `CommandPalette`                                                     |
| **Data & Flag Domain Widgets** | `DataTable`, `JsonEditor`, `DiffViewer`, `RuleBuilder` (clause rows, operator picker, value chips), `RolloutAllocator` (weighted percentage bars), `AuditTimeline` |

## Information Architecture (`apps/web`)

```text
Workspace / Project Switcher + Global Environment Selector (Color-Coded Banner)
├── Feature Flags
│   ├── Flag List (search, tags, status filters, inline quick-toggle, last-evaluated badge)
│   └── Flag Detail
│       ├── Targeting (kill switch, prerequisites, individual targets, RuleBuilder, fallthrough, off variation)
│       ├── Test Context Simulator (live dry-run via /simulate showing variation + EvaluationReason)
│       ├── Variations (manage typed values, names, descriptions)
│       ├── Insights / Usage (variation split & evaluation volume)
│       ├── Settings (client-side availability, tags, maintainer, archive/delete)
│       └── History (version timeline with DiffViewer & one-click restore)
├── Segments
│   ├── Segment List
│   └── Segment Editor (included/excluded keys, dynamic rules, referencing flags)
├── Contexts & Live Debugger (Phase 3)
├── Releases & Experiments (Phase 3–5)
├── AI Configs & Observability (Phase 5–6)
├── Audit Log
│   └── Filterable chronological AuditTimeline with semantic patch diffs
└── Settings
    ├── Projects & Environments (SDK keys, rotation, protection rules)
    ├── Team Members & Roles (RBAC)
    ├── API Access Tokens
    └── Webhooks & Integrations
```

## Critical Operational Safeguards

1. **Environment Color Coding**: `EnvironmentBanner` renders a high-contrast accent bar driven by `--sk-env-accent` (e.g., crimson/amber for `production`, emerald/blue for `dev`) across all flag and segment screens.
2. **Change Review Modal (Production Protection)**: Protected environments open a `Dialog` containing a human-readable `DiffViewer` of the semantic instructions, a change comment field (required when `requireComments = true`), and explicit confirmation before submitting `PATCH`.
3. **Interactive Rule Simulator ("Test Context")**: Embedded in the Flag Targeting view; runs unsaved draft instructions against `POST .../simulate` (`crates/skotos-eval`) and displays the matched variation and exact `EvaluationReason` before publishing.
4. **Copy / Promote Across Environments**: Compare and promote targeting configs from `staging` -> `production` with a visual diff preview.
