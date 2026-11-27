# Project: Skotos — Phase 1 (Core Feature Flag Platform, Evaluation Engine & Management Dashboard)

## Architecture

Skotos is an open-source, LaunchDarkly-compatible feature management platform built as a single polyglot monorepo with strict separation across planes and packages:

- **Backend Planes (Rust)**: `Axum`, `Tokio`, `Tower`, `SQLx` (PostgreSQL 16+), `utoipa` (OpenAPI 3.1), `argon2`, `regex` (RE2 linear-time), `sha1`, `semver`, `tracing`. Rust Edition **`2021`** (`rust-version = "1.85"`, toolchain `1.100.0` installed).
- **Primary Datastore & Local Infra**: PostgreSQL 16+ (hybrid relational hierarchy + versioned `JSONB` configs + transactional outbox + monthly partitioned `audit_logs`), plus NATS JetStream (`nats:2 -js`) and MinIO (`minio/minio`) provisioned in `deploy/docker/docker-compose.dev.yml` (and included from root `compose.yml` so bare `docker compose up -d` works).
  - **Phase 1 vs Phase 2 Infra Boundary**: Phase 1A provisions `postgres:16`, `nats:2 -js`, and `minio` via Docker Compose. Phase 1C (`services/control-api`) connects **only to PostgreSQL** (`/readyz` checks Postgres) and atomically persists `outbox_events` and the `segments` table. Outbox fanout (`crates/skotos-fanout`), NATS/MinIO publishing, `services/delivery-edge`, `services/event-ingest`, `flag_eval_rollups`, `webhooks`, `flag_triggers`, `scheduled_changes`, Segments UI, Insights Charts, and Env Copy/Promote are **Phase 2**.
- **Pluggable Dual-Path Authentication (`crates/skotos-auth`)**: Uniform `IdentityProvider` trait supporting both Cloud SaaS (`ExternalAuthAdapter` — OIDC/JWT JWKS verifier for WorkOS AuthKit / Auth0 / Clerk) and Self-Hosted (`SelfHostedAuthAdapter` — Argon2id email/password + HMAC-signed `HttpOnly` `skotos_session` cookie + idempotent bootstrap admin user), alongside `ApiTokenVerifier` (`sk_api_...` SHA-256) and built-in RBAC (`Owner`, `Admin`, `Writer`, `Reader`).
- **Frontend & Strict UI Ownership Boundary (`ADR-0006`)**:
  - `packages/ui` (`@skotos/ui`): Owns **100%** of UI components (50 components: all 48 from `design/ui-ux.md` + `Label`/`FormField` + `AlertDialog`), `--sk-*` CSS Custom Properties (`tokens.css`, `themes.css`, `reset.css`), light/dark themes (`data-theme`), `--sk-env-accent`, co-located `*.module.css`, 15 `@radix-ui/react-*` primitives, `clsx`, `lucide-react`, Vite library build, lightweight dev workbench, and Vitest + Testing Library + axe tests. **FORBIDDEN in `@skotos/ui`**: `@skotos/api-client`, routers, React Query, auth tokens, or Tailwind.
  - `apps/web` (`@skotos/web`): Pure static SPA (React 19, TypeScript latest, Vite, Vitest, `@tanstack/react-router`, `@tanstack/react-query`) consuming **100%** of UI from `@skotos/ui` and **100%** of API calls from generated `@skotos/api-client`. **FORBIDDEN in `apps/web`**: owning reusable UI components (`apps/web/src/components/ui/**`), importing `@radix-ui/*` or `radix-ui`, reaching into `packages/ui/src/*`, raw `<button>`/`<input>`/`<select>`/`<textarea>`/`<dialog>`/`<table>`/`<hr>` elements, or Tailwind CSS.
  - **Monorepo-wide**: **ZERO `tailwindcss`** anywhere in `package.json`, lockfiles, CSS, or config files.

### Locked Cross-Document Decisions (Canonical Resolutions)

All milestones MUST adhere to these locked resolutions of cross-doc ambiguities from the Step 0 Survey:

1. **`.gitignore` Hygiene**: Ignore `.agents/teamwork/` specifically (NEVER `.agents/` wholesale, because `.agents/environment.json` is tracked in git), plus `playwright-report/` and `test-results/`.
2. **Environment & Concurrency Constraints (2 vCPUs / 7.8 GiB RAM)**:
   - Docker Engine (`29.10.0`) + `docker compose` (`v5.8.0`) are the sole local Postgres/NATS/MinIO runtime (no native host `psql`/`postgres`/`sqlx-cli`/`just`). Always use `docker compose` (space, never hyphenated `docker-compose`) and `Makefile` (GNU Make 4.3).
   - Embed SQLx migrations via `sqlx::migrate!("./migrations")` exposed through a Rust CLI flag/binary (`cargo run -p control-api -- --migrate-only`) and use runtime-checked `sqlx::query` / `sqlx::query_as` + comprehensive real-Postgres integration tests (no `sqlx-cli` or `.sqlx/` offline cache required for `cargo check --workspace`).
   - Bound Cargo parallelism (`CARGO_BUILD_JOBS=2`), never run overlapping concurrent Rust workspace builds, bound Vitest workers (`--maxWorkers=2`), and pin local `eslint` (`^9`) invoked via `pnpm exec eslint` (not global `eslint@10`).
   - Reuse pre-installed Playwright browsers (`PLAYWRIGHT_BROWSERS_PATH=/opt/ms-playwright`) or `/usr/bin/google-chrome` in E2E tests.
3. **`variationId` (UUID) vs `variationIndex` (usize)**: Control Plane DB, `FlagPatchInstruction`, and `apps/web` editors reference stable `variationId: Uuid`. `skotos-domain::compile_eval_flag(&FeatureFlag, &FlagEnvConfig)` compiles `variationId -> variationIndex: usize` for `crates/skotos-eval` and `spec/evaluation-tests/`. `POST .../simulate` returns BOTH `variationIndex` and `variationId` (plus `variationName`).
4. **Canonical 15 Clause Operators**: `in`, `startsWith`, `endsWith`, `contains`, `matches` (linear-time RE2 via Rust `regex` crate), `lessThan`, `lessThanOrEqual`, `greaterThan`, `greaterThanOrEqual`, `before`, `after`, `semverEqual`, `semverLessThan`, `semverGreaterThan`, `segmentMatch` — plus `negate: bool`. (`eq`/`neq`/`exists` are NOT part of the canonical set.)
5. **Exact SHA-1 Bucketing Math & Missing `contextKind`**:
   - Hash algorithm: **SHA-1 only**. Input: `"{seed}.{bucketValue}"` if `seed` is set; else `"{flagKey}.{salt}.{bucketValue}.{secondary}"` if `_meta.secondary` is non-empty; else `"{flagKey}.{salt}.{bucketValue}"`.
   - Take first 15 hex chars (top 60 bits, big-endian) as `h: u64` in `0..=0x0FFF_FFFF_FFFF_FFFF`.
   - Exact cross-language integer basis points: `bucket = ((h as u128 * 100_000u128) / (1u128 << 60)) as u64` strictly in `0..=99_999` (0.001% precision, zero IEEE-754 drift across Rust/Go/TS/Python). Walk cumulative weights and select first variation where `bucket < cumulative_weight`, with fallback to last variation if `sum < 100_000`.
   - Missing `contextKind` or unhashable `bucketBy` (non-string, non-integer): deterministic fallback `bucket = 0` (selects first variation with weight > 0, or first variation if all 0), `inExperiment = false`.
   - Enable `serde_json` `float_roundtrip` feature in `crates/skotos-domain` and `crates/skotos-eval`.
6. **Negation & Segment Precedence**:
   - Missing `contextKind`, invalid `AttributeRef`, missing/`null` attribute, or empty array `[]` returns `false` **WITHOUT** applying `negate` (exception: `segmentMatch` DOES apply `negate`, so missing segment + `negate: true` -> `true`).
   - Segment precedence: `included`/`includedContexts` FIRST (`true`), then `excluded`/`excludedContexts` (`false`), then `rules[]`. Use a push/pop path stack for prerequisite and segment cycle guards so diamond DAGs succeed and cycles return `ERROR(MALFORMED_FLAG)`.
   - SemVer supports 1/2-component shorthand (`"1"` -> `"1.0.0"`, `"1.2"` -> `"1.2.0"`), pre-release `<` release, ignores build metadata, rejects leading `"v"`.
7. **Canonical Table Name**: `flag_env_revisions` (reconciling ADR-0002's `flag_env_config_revisions`).
8. **Optimistic Concurrency & Errors**: `PATCH /api/v1/projects/{projectKey}/flags/{flagKey}` accepts `expectedVersion` in JSON body and/or `If-Match` header (if both present and disagree -> `400`; if mismatch with DB -> `409 Conflict` with `currentVersion`). All `4xx`/`5xx` errors return RFC 9457 `application/problem+json` with `type`, `title`, `status`, `detail`, `instance`, `code`, and optional `currentVersion` / `errors`.
9. **Frontend Router & Icons**: `@tanstack/react-router` + `@tanstack/react-query` in `apps/web`; `lucide-react` encapsulated inside `@skotos/ui`; `@skotos/ui` uses `@radix-ui/react-slot` (`asChild`) / callbacks to stay 100% router-agnostic.

---

## Feature Inventory

Every Phase 1 feature discovered across Survey 1, Survey 2, and Survey 3 is assigned to a milestone (zero unassigned).

| # | Feature | Description | Milestone | Source |
|---|---|---|---|---|
| 1 | Root Cargo Workspace & Rust Lints | `Cargo.toml` workspace (`crates/*`, `services/*`), Edition 2021, shared deps, `clippy::all`, `rustfmt.toml`, compilable Phase 1 stubs | M1 (1A) | survey_1 (`implementation-plan.md:33`, `ADR-0001`) |
| 2 | Root pnpm Workspace & Strict TS Config | `pnpm-workspace.yaml` (`packages/*`, `apps/*`), root `package.json` scripts, `tsconfig.base.json`, stub packages with passing Vitest baseline | M1 (1A) | survey_1 (`implementation-plan.md:34`, `ADR-0001`) |
| 3 | ESLint Flat Config & Monorepo Boundary Guardrails | Local `eslint` ^9 flat config enforcing two-way `@skotos/ui` <-> `apps/web` isolation (`@radix-ui/*`, `radix-ui`, deep UI imports, raw HTML form/table elements, API/router bans in UI) + `pnpm lint:boundaries` zero-Tailwind check | M1 (1A) | survey_1, survey_3 (`ADR-0006:30-53`, `implementation-plan.md:34`) |
| 4 | Docker Compose Local Dev Stack & `.env.example` | `deploy/docker/docker-compose.dev.yml` (`postgres:16`, `nats:2 -js -m 8222`, `minio/minio` with healthchecks) + root `compose.yml` `include:` + `.env.example` | M1 (1A) | survey_1 (`implementation-plan.md:35,38`, `ADR-0002/0003`) |
| 5 | Root `Makefile`, CI Pipeline & `.gitignore` Hygiene | `Makefile` (`dev-infra`, `dev-infra-down`, `migrate`, `test-rust`, `test-ui`, `test-web`, `gen-openapi`, `lint-all`, `check`), `.github/workflows/ci.yml` (Rust + TS + Boundaries + Docs), `.gitignore` ignoring `.agents/teamwork/` | M1 (1A) | survey_1 (`implementation-plan.md:36-38`, `README.md:46-48`) |
| 6 | Workspace & Identity Domain Entities | `Organization`, `PlanTier`, `BuiltInRole`, `Member`, `ClientSideAvailability`, `Project`, `Environment`, `EnvironmentKeyKind`, `EnvironmentKey`, `ApiToken` (`serde` camelCase + `utoipa::ToSchema` + validation) | M2 (1B) | survey_2 (`F01`, `domain-model.md:31-35`) |
| 7 | Feature Flag, Variation & Per-Env Config Entities | `FlagKind`, `Variation` (stable UUID + typed JSON value validation), `FeatureFlag`, `FlagEnvConfig`, `FlagEnvConfigBody`, `Prerequisite`, `Target`, `TargetingRule`, `Clause`, `ClauseOp` (15 ops), `VariationOrRollout`, `Rollout`, `RolloutKind`, `WeightedVariation`, `FlagEnvRevision` | M2 (1B) | survey_2 (`F02`, `F03`, `domain-model.md:36-45`) |
| 8 | Segment Entity & Compiled Eval Models | `Segment`, `SegmentKind`, `SegmentTarget`, `SegmentRule` + `EvalFlag`/`EvalSegment` + `compile_eval_flag()` (`variationId` -> `variationIndex`) | M2 (1B) | survey_2 (`F04`, `G1`, `implementation-plan.md:44`) |
| 9 | Multi-Context & RFC 6901 `AttributeRef` | Single & Multi `Context`, `ContextMeta` (`privateAttributes`, `secondary`), strict context validation, literal vs `/`-prefixed RFC 6901 JSON Pointer parsing (`~1`/`~0`/`~01`), `_meta` access block | M2 (1B) | survey_2 (`F05`, `domain-model.md:55-76`, `evaluation-engine.md:50`) |
| 10 | Semantic Instruction Patch Engine (`FlagPatchInstruction`) | Discriminated 23-variant enum (18 core + 5 metadata: `updateName`/`Description`/`Tags`/`Maintainer`/`Temporary`), pure deterministic `apply_flag_patch` + validator + structured diff + DAG cycle & variation-in-use validators | M2 (1B) | survey_2 (`F06`, `G8`, `api-contracts.md:21-64`) |
| 11 | Canonical Rust Evaluation Pipeline (`crates/skotos-eval`) | Synchronous zero-I/O `EvalStore` trait + 7-step pipeline: Context validation -> Existence -> Kill switch (`OFF`) -> Prerequisites + diamond-safe cycle guard -> Individual Targets -> Targeting Rules -> Fallthrough -> Typed Getter `WRONG_TYPE` check + exact `EvaluationReason` | M2 (1B) | survey_2 (`F07`, `F08`, `F09`, `evaluation-engine.md:5-38`) |
| 12 | Clause Evaluation (15 Ops) & Segment Matching | All 15 canonical operators (`in`, string ops, linear-time RE2 `matches`, numeric, RFC 3339/epoch ms dates, SemVer 2.0.0 + shorthand, `segmentMatch`), array OR + missing-attr non-negation rule, segment `included > excluded > rules` + rollout + diamond-safe cycle guard | M2 (1B) | survey_2 (`F10`, `F11`, `G2`, `G4`, `G12`) |
| 13 | Deterministic SHA-1 Rollout Bucketing & Private Attribute Redaction | SHA-1 60-bit exact integer basis-point bucketing (`0..=99_999`), custom `bucketBy`/`seed`/`secondary`, missing `contextKind` fallback (`bucket=0`, `inExperiment=false`), plus `redact_context()` (`_meta.privateAttributes`, `_meta.redactedAttributes`) | M2 (1B) | survey_2 (`F12`, `F13`, `G3`, `ADR-0004:32-33`) |
| 14 | Golden Evaluation Test Suite (100+ Vectors) & Property Tests | `spec/evaluation-tests/` (`README.md`, `schema.json`, `basic.json`, `operators.json`, `rollouts.json`, `multi-context.json`, `prerequisites.json`, `segments.json`, `errors.json`, `redaction.json`) totaling >= 115 vectors + `skotos-eval` golden runner + `proptest` bucketing distribution & cycle/fuzz safety | M2 (1B) | survey_2 (`F14`, `implementation-plan.md:47-49`, `ADR-0004`) |
| 15 | `@skotos/ui` Design Token System & CSS Modules Architecture | `tokens.css`, `themes.css` (`light`/`dark` + `--sk-env-accent` color-mix), `reset.css`, `ThemeProvider`/`useTheme`, co-located `*.module.css`, zero Tailwind | M3 (1D) | survey_3 (`1-3`, `ADR-0006:35-42`) |
| 16 | `@skotos/ui` Layout, Shell, Typography & Feedback Components | `AppShell`, `Sidebar`, `TopBar`, `PageHeader`, `EnvironmentBanner`, `Card`, `Stack`, `Grid`, `SplitPane`, `Separator`, `Text`, `Heading`, `CodeBlock`, `Badge`, `StatusDot`, `Alert`, `ToastProvider`/`useToast`, `Skeleton`, `EmptyState`, `Spinner` (20 components + `ThemeProvider` = 21) | M3 (1D) | survey_3 (`4-23`, `design/ui-ux.md:20-21`) |
| 17 | `@skotos/ui` Primitives, Forms, Overlays & Navigation Components | `Button`, `IconButton`, `Input`, `Textarea`, `Label`/`FormField`, `Select`, `Combobox`, `Checkbox`, `Switch`, `ToggleGroup`, `Slider`, `TagInput`, `KeyValueEditor`, `Dialog`, `AlertDialog`, `Drawer`, `Popover`, `DropdownMenu`, `Tooltip`, `Tabs`, `Breadcrumb`, `Pagination`, `CommandPalette` (23 components wrapping 15 `@radix-ui/react-*` primitives + `lucide-react` icon exports) | M3 (1D) | survey_3 (`24-46`, `design/ui-ux.md:22-23`) |
| 18 | `@skotos/ui` Data & Domain Widgets | `DataTable`, `JsonEditor`, `DiffViewer`, `RuleBuilder` (15 canonical ops + `negate` + JSON pointer), `RolloutAllocator` (integer basis points summing to `100000`), `AuditTimeline` (6 components; total 50) | M3 (1D) | survey_3 (`47-52`, `design/ui-ux.md:24`) |
| 19 | `@skotos/ui` Vite Library Build, Dev Workbench & Vitest + Axe A11y Suite | ESM + `.d.ts` + bundled `dist/styles.css` build, monorepo dev HMR resolution, `packages/ui/workbench/` showcase, co-located `*.test.tsx` (unit + keyboard + `vitest-axe` zero violations) for every component | M3 (1D) | survey_3 (`53`, `implementation-plan.md:69,76`, `ADR-0006:44-45`) |
| 20 | PostgreSQL 16+ Migrations & Multi-Tenant Repositories (`crates/skotos-storage`) | Embedded SQLx migrations for 12 tables (`organizations`, `members`, `projects`, `environments`, `environment_keys`, `flags`, `flag_env_configs`, `flag_env_revisions`, `segments`, monthly-partitioned `audit_logs` + `audit_logs_default`, `outbox_events`, `api_tokens`) + tenant-bound (`org_id`) CRUD repos | M4 (1C) | survey_2 (`F15`, `F16`, `G5`, `G6`, `implementation-plan.md:54-55`) |
| 21 | ACID Transactional Mutations, Outbox, Audit & Revision Restore | Single ACID tx for flag create/patch/restore/delete/env create/key rotate: `FOR UPDATE` locking, `require_comments` check, `expectedVersion`/`If-Match` `409 Conflict`, write-time prerequisite cycle & variation-in-use checks, monotonic `version` & `environments.revision` bump, atomic `flag_env_revisions` + `audit_logs` + `outbox_events` | M4 (1C) | survey_2 (`F17`, `F18`, `implementation-plan.md:56,64`) |
| 22 | Dual-Path `IdentityProvider`, API Tokens, Env Keys & RBAC (`crates/skotos-auth`) | `IdentityProvider` trait, `ExternalAuthAdapter` (JWT/JWKS RS256/ES256/EdDSA), `SelfHostedAuthAdapter` (Argon2id + HMAC-signed `HttpOnly` cookie + idempotent bootstrap admin), `ApiTokenVerifier` (`sk_api_...`), prefixed env key generation/rotation (`sk_sdk_`, `sk_mob_`, `sk_cid_`), `Authorizer` RBAC (`Owner`, `Admin`, `Writer`, `Reader`) | M4 (1C) | survey_2 (`F19-F23`, `G11`, `implementation-plan.md:58`) |
| 23 | Control Plane REST API, `/simulate` & Ops (`services/control-api`) | Axum router implementing all `1C-Spec` + `1C-UI-Req` endpoints (Health/Ready, Auth, Projects, Envs, Keys/Rotate, Flags CRUD + Semantic PATCH, Simulate, Revisions + Restore, Audit Log, API Tokens, Members), RFC 9457 `application/problem+json`, cursor pagination, CORS w/ credentials, `--migrate-only` CLI, graceful shutdown | M4 (1C) | survey_2 (`F24`, `F25`, `F27`, `G7`, `G10`, `implementation-plan.md:60`) |
| 24 | OpenAPI 3.1 Export & `@skotos/api-client` Generation | `utoipa` OpenAPI 3.1 `export-openapi` binary emitting `spec/openapi/control-api.json` -> `openapi-typescript` + `openapi-fetch` generating `packages/api-client` with typed `createSkotosClient`, `SkotosApiError`, zero-drift CI verification, and real Docker Postgres integration tests | M4 (1C) | survey_2 (`F26`), survey_3 (`55`, `implementation-plan.md:61-64`) |
| 25 | `apps/web` Core Shell, Routing, Auth & Workspace Switcher | Vite SPA + `@tanstack/react-router` + `@tanstack/react-query` + `@skotos/api-client` + `@skotos/ui` only; Dual-path `/login` & `/auth/callback`, auth guard, `AppShell`/`TopBar`/`Sidebar`/`CommandPalette` (`Cmd/Ctrl+K`), Project & URL-synced Environment Switcher driving `--sk-env-accent` & `EnvironmentBanner` | M5 (1E) | survey_3 (`56`, `57`, `implementation-plan.md:81-84`) |
| 26 | `apps/web` Feature Flags List & Lifecycle | `/projects/$projectKey/flags`: searchable/filterable `DataTable`, inline `Switch` (with protected-env Change Review `Dialog` intercept), Quick Create Flag `Dialog`, Archive/Restore/Delete with typed-key `AlertDialog`, RBAC-aware disabled controls | M5 (1E) | survey_3 (`58`, `implementation-plan.md:85`) |
| 27 | `apps/web` Flag Detail (Targeting, Embedded Simulator, Variations, Settings, History) | `/projects/$projectKey/flags/$flagKey`: `SplitPane` Targeting workbench (`RuleBuilder`, `RolloutAllocator`, targets, prerequisites if present, fallthrough/off variation, semantic `FlagPatchInstruction[]` diff, Change Review `Dialog` w/ `DiffViewer` + comment, `409 Conflict` recovery, unsaved leave guard), Embedded `/simulate` Test Context Debugger (Form + JSON, draft vs saved), Variations tab (in-use guard), Settings tab, History tab (`AuditTimeline` + `DiffViewer` + monotonic Restore) | M5 (1E) | survey_3 (`59-63`, `implementation-plan.md:86`) |
| 28 | `apps/web` Global Audit Log & Settings (Projects, Envs/Keys, Tokens, Members) + Vitest Suite | `/audit-log` (filters, cursor pagination, `DiffViewer`), `/settings/projects`, `/settings/environments` (color, protection, `sk_cid_` copy, zero-downtime key rotation one-time secret modal), `/settings/tokens` (one-time `sk_api_` create & revoke), `/settings/members` (RBAC roles), plus full Vitest + RTL + MSW test suite & strict ESLint boundary pass | M5 (1E) | survey_3 (`64-69`, `implementation-plan.md:87-89`) |
| 29 | Independent Opaque-Box E2E Test Suite (Tiers 1–4) | Standalone `e2e/` suite authored from `ORIGINAL_REQUEST.md` & `PROJECT.md` covering all Phase 1 user journeys (`E2E-01`–`E2E-14`), API/eval/auth/RBAC/concurrency/UI/a11y/boundary contracts across Tiers 1–4 (`TEST_INFRA.md` + `TEST_READY.md`) | E2E Track | survey_3 (`E2E-01`–`E2E-14`, Project Pattern Dual Track) |
| 30 | Final Milestone: 100% E2E Pass (Tiers 1–4) + Tier-5 Adversarial Coverage Hardening | Phase 1: sequential E2E Tiers 1 -> 2 -> 3 -> 4 passing 100% against real Docker Compose + `control-api` + `apps/web`; Phase 2: white-box Tier-5 adversarial Challengers inspecting implementation + tests to close edge-case/security/concurrency/eval/UI gaps | M6 (Final) | Project Pattern Dual Track & Final Milestone |

*(Phase 2 Out-of-Scope Boundary: `crates/skotos-fanout`, `crates/skotos-telemetry`, `services/delivery-edge`, `services/event-ingest`, `webhooks`, `flag_triggers`, `scheduled_changes`, Segments UI, Flag Insights/Charts, Env Copy/Promote, and multi-language SDKs.)*

---

## Milestones

| # | Name | Scope | Dependencies | Status |
|---|---|---|---|---|
| M1 (1A) | Monorepo Scaffold, Infra & Guardrails | Root Cargo & pnpm workspaces, Phase 1 stubs, `deploy/docker/docker-compose.dev.yml` + root `compose.yml`, `.env.example`, ESLint `@skotos/ui` boundaries + zero-Tailwind CI script, `Makefile`, `.github/workflows/ci.yml`, `.gitignore` `.agents/teamwork/` | none | IN_PROGRESS (`c57fff7a67f15a3135196844953b08bd`) |
| M2 (1B) | Domain, Eval Engine & Golden Suite | `crates/skotos-domain`, `crates/skotos-eval`, `spec/evaluation-tests/` (115+ vectors across 8 suites + `proptest`) | M1 (1A) | PLANNED |
| M3 (1D) | `@skotos/ui` Standalone Component Library | `packages/ui` (50 Radix + `.module.css` components, `--sk-*` tokens/themes, zero Tailwind, Vite lib build, workbench, Vitest + RTL + axe) | M1 (1A) | PLANNED |
| M4 (1C) | Storage, Auth, Control API & API Client | `crates/skotos-storage` (12 tables + ACID outbox/audit/revisions), `crates/skotos-auth` (SaaS + Self-Hosted + API Tokens + RBAC), `services/control-api` (REST + `/simulate` + `utoipa`), `spec/openapi/control-api.json` -> `packages/api-client` | M1 (1A), M2 (1B) | PLANNED |
| M5 (1E) | `apps/web` Management Dashboard | `apps/web` SPA (TanStack Router/Query, consuming `@skotos/ui` + `@skotos/api-client` exclusively, all Phase 1 views, Vitest + RTL + MSW) | M1 (1A), M3 (1D), M4 (1C) | PLANNED |
| E2E Track | Independent Opaque-Box E2E Test Suite | `TEST_INFRA.md`, standalone `e2e/` test harness + Tiers 1–4 requirement-driven test cases (Playwright + API + DB + boundary checks), publishes `TEST_READY.md` | none (parallel) | IN_PROGRESS (`10c824ca8316761606a79d6ba7873ed0`) |
| M6 (Final) | E2E 100% Pass & Tier-5 Adversarial Hardening | Phase 1: sequential E2E Tiers 1 -> 2 -> 3 -> 4 (100% pass); Phase 2: white-box Tier-5 adversarial Challengers -> Worker -> Reviewer loop | M5 (1E), E2E Track (`TEST_READY.md`) | PLANNED |

---

## Interface Contracts

### 1. `crates/skotos-domain` <-> `crates/skotos-eval`
- **Direction**: `skotos-eval` depends on `skotos-domain` only (both pure synchronous Rust, zero async/SQL/HTTP I/O; both enable `serde_json` `float_roundtrip`).
- **Domain Exports**: All entity/config/patch structs (`#[serde(rename_all = "camelCase")]`, optional `openapi` feature deriving `utoipa::ToSchema`), `Context` (`Single` / `Multi`), `AttributeRef`, `ClauseOp` (15 variants), `EvaluationDetail`, `EvaluationReason`, `EvaluationErrorKind`, `FlagPatchInstruction` (23 variants), `apply_flag_patch()`, `validate_flag_env_config()`, `detect_prerequisite_cycle()`, `compile_eval_flag(&FeatureFlag, &FlagEnvConfig) -> Result<EvalFlag, DomainError>`, `EvalFlag`, `EvalSegment`.
- **Eval Exports**:
  - `pub trait EvalStore { fn get_flag(&self, key: &str) -> Option<&EvalFlag>; fn get_segment(&self, key: &str) -> Option<&EvalSegment>; }`
  - `evaluate(store, flag_key, context, default_value) -> EvaluationDetail`
  - Typed evaluators: `bool_variation`, `string_variation`, `number_variation`, `json_variation`
  - `redact_context(context, config) -> serde_json::Value`

### 2. `crates/skotos-domain` <-> `crates/skotos-storage`
- **Direction**: `skotos-storage` depends on `skotos-domain` + `sqlx`. Never leaks `sqlx::PgRow` outside `skotos-storage`.
- **Tenant Safety**: Every repository function requires `org_id: Uuid` as its first parameter and explicitly filters `WHERE org_id = $1`.
- **Migrations**: Embedded via `sqlx::migrate!("./migrations")` in `crates/skotos-storage/src/lib.rs` (`pub async fn run_migrations(pool: &PgPool) -> Result<(), StorageError>`).
- **Error Mapping**: `StorageError::{ NotFound, AlreadyExists, VersionConflict { expected, actual }, Validation(DomainError), CommentRequired, VariationInUse(Vec<String>), PrerequisiteCycle, ForbiddenLastOwner, Database(sqlx::Error) }`.
- **ACID Mutation Guarantee**: `patch_flag`, `create_flag`, `restore_flag_revision`, `delete_flag`, `create_environment`, `rotate_environment_key` execute state updates + monotonic version/revision bumps + `flag_env_revisions` + `audit_logs` + `outbox_events` in ONE PostgreSQL transaction.

### 3. `crates/skotos-auth` <-> `services/control-api`
- **Core Structs**: `ActorType { Member, ApiToken, System }`, `AuthenticatedPrincipal { org_id, actor_type, member_id, api_token_id, email, display_name, role: BuiltInRole, team_ids, role_ids, token_scopes }`.
- **Traits**:
  - `IdentityProvider` (`mode() -> AuthMode`, `authenticate(bearer: Option<&str>, session_cookie: Option<&str>) -> Result<AuthenticatedPrincipal, AuthError>`) implemented by `ExternalAuthAdapter` (OIDC JWT/JWKS RS256/ES256/EdDSA) and `SelfHostedAuthAdapter` (Argon2id + HMAC-SHA256 `skotos_session` cookie + idempotent bootstrap admin).
  - `ApiTokenVerifier` (`sk_api_...` checked first in Axum auth extractor via SHA-256 hash lookup).
  - `Authorizer` (`authorize(&self, principal, action: Action, resource: &ResourceRef) -> Result<(), AuthzError>`).
- **RBAC Matrix**: `Reader` (all GET + `/simulate`), `Writer` (+ Flag create/patch/archive/restore), `Admin` (+ Flag hard delete, Project/Env CRUD, Key rotation, API tokens, Member management excluding Owner), `Owner` (+ all org & Owner management; cannot demote/remove last Owner).

### 4. `services/control-api` (OpenAPI 3.1) <-> `packages/api-client` <-> `apps/web`
- **Base Path & Content Types**: `/api/v1/*` (`application/json`), errors (`application/problem+json` RFC 9457), probes `/healthz` & `/readyz`.
- **Phase 1 Endpoints**:
  - Public: `GET /healthz`, `GET /readyz`, `GET /api/v1/auth/config`, `POST /api/v1/auth/login`
  - Auth: `POST /api/v1/auth/logout`, `GET /api/v1/auth/me`
  - Projects: `GET/POST /api/v1/projects`, `GET/PATCH/DELETE /api/v1/projects/{projectKey}`
  - Environments & Keys: `GET/POST /api/v1/projects/{projectKey}/environments`, `GET/PATCH/DELETE /api/v1/projects/{projectKey}/environments/{envKey}`, `GET /api/v1/projects/{projectKey}/environments/{envKey}/keys`, `POST /api/v1/projects/{projectKey}/environments/{envKey}/keys/rotate`
  - Flags: `GET/POST /api/v1/projects/{projectKey}/flags`, `GET/PATCH/DELETE /api/v1/projects/{projectKey}/flags/{flagKey}`
  - Simulate & Revisions: `POST /api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/simulate`, `GET /api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/revisions`, `POST /api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/restore/{version}`
  - Audit Log: `GET /api/v1/audit-log` (cursor-paginated `{ items, nextCursor, hasMore }`)
  - API Tokens: `GET/POST /api/v1/tokens`, `DELETE /api/v1/tokens/{tokenId}`
  - Members: `GET/POST/PATCH/DELETE /api/v1/members`, `/api/v1/members/{memberId}`
- **Codegen Pipeline**: `cargo run -p control-api --bin export-openapi` -> `spec/openapi/control-api.json` -> `pnpm --filter @skotos/api-client gen` (`openapi-typescript` -> `src/schema.d.ts`) + `openapi-fetch` wrapper exporting `createSkotosClient`, `paths`, `components`, `SkotosApiError`.
- **CORS & Cookies**: `control-api` sets `Access-Control-Allow-Credentials: true` for explicit `SKOTOS_CORS_ORIGINS` (default `http://localhost:5173`); `apps/web` configures Vite `/api` proxy in dev and `credentials: 'include'` in `createSkotosClient`.

### 5. `packages/ui` (`@skotos/ui`) <-> `apps/web` (`@skotos/web`)
- **Exports of `@skotos/ui`**:
  - `.` -> all 50 components + `ThemeProvider`/`useTheme`/`ToastProvider`/`useToast` + curated `lucide-react` icon exports + pure TypeScript prop types (including `RuleBuilderRule`, `RolloutValue`, `AuditTimelineEntry`, `DiffInstructionSummary`).
  - `./styles.css` -> bundled tokens, themes, reset, and all component CSS Modules.
  - `./tokens.css` -> standalone `--sk-*` CSS Custom Properties.
- **Boundary Enforcement**: Verified automatically by `pnpm lint` (`eslint.config.mjs`) and `pnpm lint:boundaries` (zero Tailwind monorepo-wide, zero Radix/deep-UI/raw-HTML-primitives in `apps/web`, zero API/router/query imports in `packages/ui`).

---

## Code Layout

Strict exclusive file ownership across milestones and tracks:

```text
skotos/
├── Cargo.toml                         # Owned by M1 (updated only if needed by M2/M4)
├── rustfmt.toml                       # Owned by M1
├── pnpm-workspace.yaml                # Owned by M1 (packages/*, apps/*)
├── package.json                       # Owned by M1
├── tsconfig.base.json                 # Owned by M1
├── eslint.config.mjs                  # Owned by M1
├── scripts/check-ui-boundaries.mjs    # Owned by M1
├── compose.yml                        # Owned by M1 (includes deploy/docker/docker-compose.dev.yml)
├── deploy/docker/docker-compose.dev.yml # Owned by M1 (postgres:16, nats:2 -js, minio)
├── .env.example                       # Owned by M1
├── Makefile                           # Owned by M1
├── .github/workflows/ci.yml           # Owned by M1
├── .gitignore                         # Owned by M1 (adds .agents/teamwork/, playwright-report/, test-results/)
├── crates/
│   ├── skotos-domain/                 # Stub in M1 -> Full impl in M2
│   ├── skotos-eval/                   # Stub in M1 -> Full impl in M2
│   ├── skotos-storage/                # Stub in M1 -> Full impl + migrations/ in M4
│   └── skotos-auth/                   # Stub in M1 -> Full impl in M4
├── services/
│   └── control-api/                   # Stub in M1 -> Full impl + src/bin/export_openapi.rs in M4
├── spec/
│   ├── evaluation-tests/              # Owned by M2 (README.md, schema.json, 8 JSON suites, 115+ vectors)
│   └── openapi/                       # Owned by M4 (control-api.json generated by export-openapi)
├── packages/
│   ├── ui/                            # Stub in M1 -> Full @skotos/ui impl + workbench + Vitest in M3
│   └── api-client/                    # Stub in M1 -> Full @skotos/api-client generated code in M4
├── apps/
│   └── web/                           # Stub in M1 -> Full Management Dashboard SPA + Vitest/MSW in M5
├── e2e/                               # Owned EXCLUSIVELY by E2E Testing Track (self-contained package.json, Playwright + API/DB/boundary tests; excluded from pnpm-workspace.yaml)
├── PROJECT.md                         # Project Orchestrator scope document
├── TEST_INFRA.md                      # E2E Testing Track test architecture document
└── TEST_READY.md                      # E2E Testing Track completion signal
```

## Verification & Exit Criteria

All Phase 1 verification gates (executed independently by Reviewers, Challengers, Auditors, and Final Milestone):
- `docker compose up -d` (Postgres 16, NATS JetStream, MinIO all healthy)
- `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace` (including 100% of `spec/evaluation-tests/` 115+ vectors, `proptest` suites, real Docker Postgres atomicity/concurrency/RBAC/tenant-isolation integration tests)
- `make gen-openapi && git diff --exit-code spec/openapi/ packages/api-client/`
- `pnpm --filter @skotos/ui test && pnpm --filter @skotos/ui build`
- `pnpm --filter @skotos/web test && pnpm --filter @skotos/web build`
- `make lint-all` (ESLint `@skotos/ui` boundary rules + zero-Tailwind check + Rust clippy/fmt + `markdownlint-cli2` + `prettier --check` on docs)
- E2E Testing Track suite (`TEST_READY.md` command): 100% pass across Tiers 1–4 + Tier-5 white-box adversarial coverage hardening with zero remaining gaps.
