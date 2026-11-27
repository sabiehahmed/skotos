# Skotos Implementation Plan

> **Architecture Status:** Finalized (ADRs 0001–0006 Accepted). Ready to begin **Phase 1**.

This plan translates our finalized architecture ([ADRs](../architecture/adrs/README.md)) and full feature parity map ([LaunchDarkly Feature Catalog](launchdarkly-feature-catalog.md)) into concrete milestones, packages, verification gates, and engineering deliverables.

---

## 1. Finalized Architecture Summary (Non-Negotiable Invariants)

| Layer                         | Finalized Decision                                                                                                                                                         | ADR                                                                          |
| ----------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| **Backend Services**          | Rust everywhere (`Axum`, `Tokio`, `SQLx`, `async-nats`, `object_store`, `utoipa`, `tracing`)                                                                               | [ADR-0001](../architecture/adrs/0001-tech-stack-and-monorepo.md)             |
| **Frontend App**              | React (latest) + TypeScript (latest) + Vite + Vitest static SPA (`apps/web`)                                                                                               | [ADR-0001](../architecture/adrs/0001-tech-stack-and-monorepo.md)             |
| **UI Component Library**      | Standalone `@skotos/ui` (`packages/ui`) using Radix UI primitives + CSS Modules (`.module.css`) + CSS tokens (**zero Tailwind**). **`apps/web` never owns UI components.** | [ADR-0006](../architecture/adrs/0006-ui-component-library-css-modules.md)    |
| **Monorepo**                  | Single polyglot repo: Cargo workspace + `pnpm` workspace + `spec/` + `sdks/`                                                                                               | [ADR-0001](../architecture/adrs/0001-tech-stack-and-monorepo.md)             |
| **Primary Data Store**        | PostgreSQL 16+ (normalized hierarchy + versioned `JSONB` flag/segment configs + Transactional Outbox + partitioned `audit_logs`)                                           | [ADR-0002](../architecture/adrs/0002-primary-datastore-and-schema.md)        |
| **Authentication**            | Pluggable `IdentityProvider` trait: External Auth Provider (WorkOS/Auth0/Clerk) for Cloud SaaS + self-contained OIDC/Argon2id adapter for Self-Hosted VPC                  | [ADR-0002](../architecture/adrs/0002-primary-datastore-and-schema.md)        |
| **API Contracts**             | Semantic instruction patches (`turnFlagOn`, `addRule`, etc.) + `utoipa` OpenAPI 3.1 -> auto-generated `@skotos/api-client`                                                 | [ADR-0001](../architecture/adrs/0001-tech-stack-and-monorepo.md)             |
| **Streaming & Snapshots**     | NATS JetStream + pre-compressed zero-egress R2/S3 snapshots + Ping-to-Re-evaluate for client MAU                                                                           | [ADR-0003](../architecture/adrs/0003-streaming-and-snapshot-delivery.md)     |
| **SDKs & Evaluation**         | Canonical `crates/skotos-eval` + pure idiomatic SDKs verified in CI against `spec/evaluation-tests/` + OpenFeature providers                                               | [ADR-0004](../architecture/adrs/0004-sdk-evaluation-engine.md)               |
| **Analytics & Observability** | Postgres summary rollups (Phase 1–2) -> ClickHouse + R2/S3 (Parquet & Zstd `rrweb` blobs) + Warehouse-Native SQL (Phase 4–6)                                               | [ADR-0005](../architecture/adrs/0005-analytics-and-observability-storage.md) |

---

## 2. Phase 1 Detailed Build Plan — Core Control Plane, Evaluation Engine & Dashboard MVP

**Goal:** Ship a rock-solid, end-to-end usable feature flag management platform running locally via Docker Compose (`Postgres + NATS + MinIO`) with 100% golden evaluation test coverage, a tested standalone `@skotos/ui` CSS-Modules design system, and a complete `apps/web` management dashboard.

### Milestone 1A: Monorepo Scaffold, Tooling & Local Dev Stack

- **Deliverables**:
  - Root `Cargo.toml` workspace configuring `crates/*` and `services/*` with shared lints (`clippy::all`, `rustfmt`).
  - Root `pnpm-workspace.yaml` + `package.json` + shared `tsconfig.base.json` + ESLint flat config enforcing `@skotos/ui` ownership boundaries (forbidding `@radix-ui/*`, Tailwind, or local reusable UI component directories inside `apps/web`).
  - `deploy/docker/docker-compose.dev.yml` spinning up `postgres:16`, `nats:2` (`-js`), and `minio` (S3/R2-compatible local bucket) with healthchecks.
  - Root `Makefile` / `justfile` commands (`dev-infra`, `migrate`, `test-rust`, `test-ui`, `test-web`, `gen-openapi`, `lint-all`).
  - GitHub Actions CI workflow running Rust checks, Vitest suites, ESLint boundary rules, and Markdown lint checks.
- **Exit Criteria**: `docker compose up -d`, `cargo check --workspace`, and `pnpm test` all pass cleanly on a fresh checkout.

### Milestone 1B: Canonical Domain, Evaluation Engine & Golden Test Suite

- **Deliverables**:
  - `crates/skotos-domain`:
    - Strongly typed structs for `Organization`, `Project`, `Environment`, `FeatureFlag`, `Variation`, `FlagEnvConfig`, `TargetingRule`, `Clause`, `ClauseOp`, `Rollout`, `Prerequisite`, `Segment`, `Context` (single + multi-context), `EvaluationReason`, and `FlagPatchInstruction` (semantic patch enum + deterministic patch applier + validator).
  - `crates/skotos-eval`:
    - Pure, allocation-light flag evaluator implementing [`design/evaluation-engine.md`](../design/evaluation-engine.md): kill switch, prerequisite DAG evaluation + cycle guard, individual targets, rule clauses (all operators + RE2 `regex` + semver + RFC 3339 date + nested JSON pointers), SHA-1 consistent percentage rollouts, and exact `EvaluationReason` output.
  - `spec/evaluation-tests/`:
    - Canonical JSON golden test suites (`basic.json`, `operators.json`, `rollouts.json`, `multi-context.json`, `prerequisites.json`, `segments.json`, `errors.json`) with 100+ test vectors.
- **Exit Criteria**: `cargo test -p skotos-eval` executes and passes 100% of `spec/evaluation-tests/` plus property/fuzz tests for bucketing distribution and cycle safety.

### Milestone 1C: PostgreSQL Schema, Dual-Path Auth & Control Plane API (`services/control-api`)

- **Deliverables**:
  - `crates/skotos-storage`:
    - SQLx migrations creating `organizations`, `members`, `projects`, `environments`, `environment_keys`, `flags`, `flag_env_configs`, `flag_env_revisions`, `segments`, `audit_logs` (partitioned), `outbox_events`, and `api_tokens`.
    - Transactional mutation repository: applies validated `FlagPatchInstruction[]`, checks `expectedVersion`, bumps `version` + environment `revision`, appends `flag_env_revisions`, `audit_logs`, and `outbox_events` in a **single ACID transaction**.
  - `crates/skotos-auth`:
    - `IdentityProvider` trait + `ExternalAuthAdapter` (JWT/JWKS verifier compatible with WorkOS AuthKit / Auth0 / Clerk) + `SelfHostedAuthAdapter` (Argon2id + signed `HttpOnly` session cookie + local dev bootstrap user) + API Token (`sk_api_...`) verifier + built-in RBAC (`Owner`, `Admin`, `Writer`, `Reader`).
  - `services/control-api`:
    - Axum REST router for Projects, Environments, Key Rotation, Flags (CRUD + Semantic `PATCH`), Revisions + Restore, `POST .../simulate`, Audit Log search, and `/healthz` + `/readyz`.
    - `utoipa` OpenAPI 3.1 binary exporting `spec/openapi/control-api.json`.
  - `packages/api-client`:
    - Automated TypeScript client & types generated from `spec/openapi/control-api.json` (`openapi-typescript` + `openapi-fetch`).
- **Exit Criteria**: Integration tests against real Docker Postgres verify transactional outbox/audit atomicity, optimistic concurrency conflicts (`409 Conflict`), RBAC enforcement (`403 Forbidden`), and OpenAPI-to-TS client generation with zero type errors.

### Milestone 1D: Standalone UI Component Library (`packages/ui` / `@skotos/ui`)

- **Deliverables**:
  - Vite library setup (`React` latest, `TypeScript` latest, `Vitest`, `@testing-library/react`, `@radix-ui/*`, **zero Tailwind**).
  - Design system foundation (`src/styles/tokens.css`, `reset.css`, `themes.css`): cohesive light & dark palettes, typography scale (UI sans + crisp monospace for keys/JSON), spacing/radius/elevation tokens, and `--sk-env-accent` CSS variable theming.
  - Complete Phase 1 component set (each with co-located `*.module.css` and `*.test.tsx`):
    - **Layout & Shell**: `ThemeProvider`, `AppShell`, `Sidebar`, `TopBar`, `PageHeader`, `EnvironmentBanner`, `Card`, `Stack`, `Grid`, `Separator`.
    - **Primitives & Forms**: `Button`, `IconButton`, `Input`, `Textarea`, `Select`, `Combobox`, `Checkbox`, `Switch`, `ToggleGroup`, `Slider`, `TagInput`.
    - **Overlays & Navigation**: `Dialog`, `Drawer`, `Popover`, `DropdownMenu`, `Tooltip`, `Tabs`, `Breadcrumb`, `Pagination`, `ToastProvider`.
    - **Data & Domain Widgets**: `Badge`, `StatusDot`, `Alert`, `EmptyState`, `Skeleton`, `CodeBlock`, `DataTable`, `JsonEditor`, `DiffViewer`, `RuleBuilder`, `RolloutAllocator`, `AuditTimeline`.
- **Exit Criteria**: `pnpm --filter @skotos/ui test` passes all unit/accessibility tests, `pnpm --filter @skotos/ui build` emits clean ESM + `.d.ts` + CSS bundle, and zero Tailwind exists anywhere in the package.

### Milestone 1E: Main Management Dashboard (`apps/web`)

- **Deliverables**:
  - React + Vite + Vitest SPA importing **100% of visual components from `@skotos/ui`** and **100% of API calls from `@skotos/api-client`** via TanStack Query.
  - Screens & Flows:
    - Auth flow (plugging into SaaS external auth callback or Self-Hosted login page).
    - Workspace/Project switcher & color-coded Global Environment selector.
    - **Flags List**: search, tag/status filtering, inline kill-switch toggle (with confirmation modal on protected envs), quick create flag modal.
    - **Flag Detail**: Targeting tab (kill switch, individual targets, `RuleBuilder`, `RolloutAllocator`, fallthrough, off variation), embedded **Test Context Simulator** (`POST .../simulate`), Variations tab, Settings tab, and History tab (`DiffViewer` + one-click restore).
    - **Audit Log**: Filterable chronological timeline showing actor, timestamp, comment, and semantic patch diffs.
    - **Settings**: Projects, Environments, SDK Key / Client-Side ID copy & zero-downtime key rotation, API Access Tokens.
- **Exit Criteria**: ESLint UI-ownership check passes with zero violations, Vitest integration tests pass, and an operator can create a project/env/flag, edit targeting rules, simulate contexts, inspect diffs, and restore revisions end-to-end.

---

## 3. Phase 2 — Real-Time Delivery Plane, Event Ingest & First SDK Suite

**Goal:** Achieve sub-200ms end-to-end streaming updates from dashboard toggle to live applications, with complete Control Plane outage isolation and OpenFeature compatibility.

- **`crates/skotos-fanout` + Outbox Worker**:
  - Claims `outbox_events` (`FOR UPDATE SKIP LOCKED`), compiles & pre-compresses (`gzip` + `zstd`) full environment snapshots + client-side subsets to **R2/S3/MinIO**, and publishes incremental events to **NATS JetStream** (`skotos.env.{env_id}.events`).
- **`services/delivery-edge` (Rust/Tokio)**:
  - In-memory LRU ruleset + pre-compressed `put` buffer, single NATS subscription per active `env_id`, `tokio::sync::broadcast` fanout with slow-consumer eviction.
  - Endpoints: `GET /stream/v1/all` (Server SSE), `GET /sdk/v1/latest-all` (ETag polling), `GET /stream/v1/eval/{cid}` (Client Ping-to-Re-evaluate SSE), `GET/REPORT /sdk/v1/evalx/{cid}/...` (stateless `skotos-eval` client evaluation).
- **`services/event-ingest` + `crates/skotos-telemetry` (PG Rollups)**:
  - `POST /events/v1/bulk` & `/events/v1/client/{cid}` accepting gzipped 5-second SDK summary buckets -> async batched upserts into PostgreSQL `flag_eval_rollups` powering flag usage charts & stale detection.
- **Core Flag & Automation Additions**:
  - Rule-based & small list-based Segments, Flag Prerequisites (with write-time DAG cycle check), Flag Templates/Views/Bulk Targeting/Env Copy, Flag Scheduling (Postgres job queue), Flag Triggers (rate-limited APM rollback URLs), Signed Outbound Webhooks.
- **Phase 2 SDK Suite (all verified against `spec/evaluation-tests/` in CI)**:
  - `@skotos/node-server-sdk` + OpenFeature Node provider
  - `@skotos/browser-sdk` + `@skotos/react-sdk` + OpenFeature Web provider
  - `skotos-go-sdk` + OpenFeature Go provider
  - `skotos-server-sdk` (Rust) + OpenFeature Rust provider
- **Verification Gate**: Synthetic canary load test verifying `< 200 ms` p95 propagation, 50k+ concurrent idle SSE connections on a single modest test node, and 100% SDK survivability when PostgreSQL is stopped.

---

## 4. Phase 3 — Enterprise Governance, Relay Proxy, DevTools & Scale

**Goal:** Undercut LaunchDarkly's Enterprise tier by shipping high-value governance and operational scale tools at low marginal cost.

- **Governance**: Custom RBAC resource-path allow/deny policy engine, Teams, Mandatory Approval Workflows (1–5 reviewers + ServiceNow/custom approval hooks), full WorkOS SAML/SCIM + Self-Hosted OIDC/SAML parity, unlimited audit log archival to R2 Parquet.
- **Release Automation**: Flag Workflows (multi-step progressive rollouts), Release Pipelines & Release Plans, Applications & app version tracking.
- **High-Scale Segments & Delivery**: Big Segments (compressed Roaring Bitmaps in R2 + Redis/SQLite sync), Scheduled Segment Changes, `services/relay-proxy` (free open-source Rust static binary with Proxy/Daemon/Auto-Config modes), Edge SDKs (Cloudflare Workers, Vercel Edge, Fastly), Live Events Debugger (ephemeral NATS tap).
- **Developer Ecosystem**: `skotos-code-refs` Rust CLI + GitHub/GitLab CI actions (extinction checks), `skotos-cli`, `terraform-provider-skotos`, Hosted & Local MCP Server, Slack/Teams/Jira/Linear/Datadog/PagerDuty integrations.

---

## 5. Phase 4 — ClickHouse Telemetry, Guarded Rollouts & Experimentation Core

**Goal:** Plug in ClickHouse behind `TelemetryStore` for high-volume analytics, safe automated rollouts, and core A/B testing.

- **ClickHouse Backend**: Migrate/enable `ClickHouseTelemetryStore` (Zstd compression, materialized views, TTL -> R2 Parquet) + Real-Time Data Export sinks (Kinesis, Pub/Sub, Snowflake, S3, Segment).
- **Guarded Rollouts & Adaptive Triggers**: Periodic ClickHouse guardrail evaluator with SRM detection, automatic rollback/pause, multi-phase audience releases, Release Dashboard, and Engineering Insights (DORA + flag health).
- **Experimentation Core**: A/B/n full-stack experiments, Frequentist & Bayesian engines over sufficient statistics, SRM & A/A testing, flexible metric types & funnel metric groups, mutual exclusion layers.
- **Feature Monitoring & Migrations**: Per-variation error/latency/log charts on flag pages, 2/4/6-stage Migration Flags with consistency/latency/error tracking, Synced Segments (Amplitude, Hightouch, Fivetran/Census, RudderStack, Segment), VS Code/JetBrains extensions, Python/Java/iOS/Android SDKs.

---

## 6. Phase 5 — Warehouse-Native Experimentation, Full OTel Observability & AgentControl

**Goal:** Match LaunchDarkly's modern full-platform capabilities at 10x lower telemetry & AI COGS.

- **Advanced Experimentation**: Warehouse-Native SQL pushdown (Snowflake, BigQuery, Redshift, Databricks, Postgres), CUPED variance reduction, Sequential testing, Multi-Armed Bandits, Global Holdouts, Audience filtering & Experiment approvals.
- **Full Observability (OTel)**: Native OTLP Logs, Distributed Traces, Error Monitoring (R2 source maps + grouping), Rust edge ingestion sampling/filter rules, Custom Dashboards, Constant & Anomaly Alerts.
- **AgentControl / AI Configs**: Completion & Agent mode configs streamed via the Flag Delivery Plane, Prompt Snippets, Tools Library, Agent Graphs, Multi-provider AI SDKs, BYOK Prompt Playground, AI Guardrails/PII redaction, and GenAI LLM Observability (tokens, latency, cost).

---

## 7. Phase 6 — Session Replays, Online AI Evals & Autonomous Release Autopilot

**Goal:** Complete AI-era runtime control parity without runaway cloud or GPU overhead.

- **Session Replays**: `rrweb` capture -> Zstd chunks uploaded directly to zero-egress R2/S3 via presigned URLs + ClickHouse metadata pointers linked to flag variations & errors.
- **AI Evaluation Suite**: Curated Datasets, Offline Batch Evals, and Online LLM-as-a-Judge / Custom Judges (BYOK) wired directly into Guarded Rollouts & Experiments.
- **Autonomous Release Autopilot & Auto-Remediation**: GitHub App (PR change risk classifier, auto-flagging, auto-release, auto-cleanup PRs, auto-archival, auto-fix PRs on rollback) and embedded Observability auto-remediation agent (Vega-style) powered by MCP + customer BYOK LLMs.
