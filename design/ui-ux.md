# UI & UX Specification

The Management Dashboard is where engineers and product managers inspect, configure, and safely roll out features across environments.

## Information Architecture

```text
Workspace / Project Switcher + Global Environment Selector
├── Feature Flags
│   ├── Flag List (search, tags, status filters, inline quick-toggle, last-evaluated badge)
│   └── Flag Detail
│       ├── Targeting (kill switch, prerequisites, individual targets, rule builder, fallthrough, off variation)
│       ├── Variations (edit labels, values, descriptions)
│       ├── Insights / Usage (evaluations over time, variation breakdown)
│       ├── Settings (client-side availability, tags, maintainer, archive/delete)
│       └── History (flag-scoped audit diffs)
├── Segments
│   ├── Segment List
│   └── Segment Editor (included/excluded keys, dynamic rules, referencing flags)
├── Contexts
│   └── Discovered Contexts Directory (inspect which variation each flag serves to a specific user/org)
├── Live Debugger
│   └── Real-time evaluation event stream filterable by flag, context key, and variation
├── Audit Log
│   └── Filterable chronological timeline with human-readable before/after diffs
└── Settings
    ├── Projects & Environments (SDK keys, rotation, protection rules)
    ├── Team Members & Roles (RBAC)
    ├── API Access Tokens
    └── Webhooks & Integrations
```

## Critical UX Safeguards

Feature flag mistakes in production can cause immediate outages. The UI must include deliberate safety friction where it matters:

1. **Environment Color Coding**: Every environment has a high-contrast visual indicator (e.g., red/amber banner for `production`, green/blue for `dev`) visible across all flag pages.
2. **Change Review Modal (Production Protection)**: For protected environments, saving targeting edits shows a clear human-readable diff summary (e.g., _"Rollout for Rule #2 changed from 10% -> 50%"_), requires an optional/mandatory change comment, and asks for explicit confirmation.
3. **Interactive Rule Simulator ("Test Context")**: Embedded right inside the Flag Targeting page so users can test a sample JSON context against unsaved draft rules and see the exact matched variation and `EvaluationReason` before publishing.
4. **Copy Settings Across Environments**: One-click workflow to compare and promote targeting rules from `staging` -> `production` with a visual diff preview.

## Open Questions for Planning

- Will the dashboard be a single-page app (e.g., React/Vite or Next.js) served alongside the API or deployed separately?
- Do we want a CLI tool (`skotos-cli`) in Phase 1/2 alongside the Web UI for terminal-first workflows?
