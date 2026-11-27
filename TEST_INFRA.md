# Test Infrastructure — Skotos Phase 1 Opaque-Box E2E Test Suite

## 1. Overview & Testing Philosophy

This document defines the architecture, tooling, isolation strategy, and 4-tier coverage contract for the **Skotos Phase 1 End-to-End (E2E) Test Suite** located under [`e2e/`](e2e/).

- **Opaque-Box & Requirement-Driven**: Every test is authored strictly from `.agents/teamwork/ORIGINAL_REQUEST.md` and [`PROJECT.md`](PROJECT.md) (`§ Feature Inventory`, `§ Locked Cross-Document Decisions`, and `§ Interface Contracts`). The suite has **zero imports** from `crates/`, `services/`, `packages/`, or `apps/`.
- **Workspace Isolation**: `e2e/` is a completely self-contained Node/TypeScript/Playwright project with its own `e2e/package.json` and lockfile, intentionally excluded from root `pnpm-workspace.yaml` so it never collides with the Implementation Track.
- **Zero Tailwind Invariant**: Neither `e2e/package.json` nor any file under `e2e/` may depend on or reference `tailwindcss`.
- **No Mocks in Tier 1–4 Tests**: Tiers 1–4 execute exclusively against a real running Skotos Phase 1 stack (Docker Compose PostgreSQL 16 + NATS JetStream + MinIO, `services/control-api`, and `apps/web`). Only `e2e/tests/self-test/` uses a local ephemeral Node HTTP stub server to self-verify the test harness mechanics before the product stack is live.

---

## 2. Target Stack & Environment Alignment

### 2.1 Environment Variables

| Variable | Default | Purpose |
|---|---|---|
| `CONTROL_API_URL` | `http://127.0.0.1:8080` | Base URL of live `services/control-api` |
| `WEB_APP_URL` | `http://127.0.0.1:5173` | Base URL of live `apps/web` SPA (Vite dev/preview or static server) |
| `DATABASE_URL` | `postgres://skotos:skotos@127.0.0.1:5432/skotos` | PostgreSQL 16+ connection string for ACID outbox/revision/audit/hash DB verification |
| `E2E_ADMIN_EMAIL` | `admin@skotos.local` | Self-hosted bootstrap `Owner` email (matches `SKOTOS_BOOTSTRAP_EMAIL`) |
| `E2E_ADMIN_PASSWORD` | `SkotosDevPass123!` | Self-hosted bootstrap `Owner` password (matches `SKOTOS_BOOTSTRAP_PASSWORD`) |
| `PLAYWRIGHT_BROWSERS_PATH` | `/opt/ms-playwright` | Reuses pre-installed Playwright browser binaries without re-downloading |
| `E2E_CHROME_EXECUTABLE` | `/usr/bin/google-chrome` | Optional fallback Chrome binary if bundled Chromium revision mismatches |
| `E2E_WORKERS` | `1` | Playwright worker count (bounded to `1`–`2` for 2-vCPU / 7.8 GiB RAM safety) |
| `E2E_REPO_ROOT` | `..` (resolved repo root) | Path to Skotos monorepo root for architectural boundary (`E2E-14`) checks |

### 2.2 Hardware & Browser Reuse Policy
- Designed for **2 vCPUs / 7.8 GiB RAM**: Playwright defaults to `workers: 1` (configurable up to `2`), headless Chromium, and bounded timeouts so tests do not trigger OOM or CPU starvation alongside Docker + Rust `control-api` + Vite.
- Browser resolution prefers pre-installed `/opt/ms-playwright` Chromium or `/usr/bin/google-chrome` (`channel: 'chrome'` / `executablePath`) — no network browser download required during test runs.

---

## 3. Directory Structure

```text
e2e/
├── package.json                          # Self-contained dependencies (@playwright/test, @axe-core/playwright, pg, typescript, tsx)
├── tsconfig.json                         # Strict TypeScript config (noEmit typecheck target)
├── playwright.config.ts                  # Playwright projects (tier1, tier2, tier3, tier4) + browser/worker config
├── scripts/
│   └── run-e2e.mjs                       # Unified CLI runner: --tier 1|2|3|4|all, --list, --self-test, env checks
├── src/
│   ├── config.ts                         # Typed env configuration & URL normalization
│   ├── api-client.ts                     # Opaque-box fetch client (cookies, Bearer sk_api_, If-Match/ETag, RFC 9457)
│   ├── db.ts                             # PostgreSQL pg Pool helper for outbox/revision/audit/hash/partition queries
│   ├── eval-helpers.ts                   # Pure SHA-1 60-bit basis-point math, multi-context & patch payload builders
│   ├── fixtures.ts                       # Playwright custom fixtures, unique resource namespacing & automatic cleanup
│   └── ui-helpers.ts                     # Accessible ARIA/role-based UI interaction helpers + @axe-core/playwright helper
└── tests/
    ├── self-test/                        # Harness unit & ephemeral HTTP stub self-tests (runs without live Skotos stack)
    ├── tier1/                            # Tier 1: Feature Coverage (Happy Path, >= 5 tests × 16 features = >= 80 tests)
    ├── tier2/                            # Tier 2: Boundary & Corner Cases (>= 5 negative/edge tests × 16 features = >= 80 tests)
    ├── tier3/                            # Tier 3: Cross-Feature Combinations (>= 24 pairwise interaction tests)
    └── tier4/                            # Tier 4: Real-World Operator Scenarios (>= 10 multi-step E2E journeys)
```

---

## 4. Test Runner & Execution Commands

All commands run from `/home/antigravity/sabiehahmed/skotos/e2e` (or via `--prefix e2e`):

- **Install standalone dependencies** (isolated from root workspace):
  - Documented and verified in `E2E-M1` (recorded verbatim in `TEST_READY.md`).
- **TypeScript typecheck**:
  - `npm --prefix e2e run typecheck`
- **Harness self-tests (no running Skotos stack required)**:
  - `npm --prefix e2e run self-test`
- **List discovered tests per tier (zero syntax/import errors check)**:
  - `node e2e/scripts/run-e2e.mjs --tier all --list`
  - `node e2e/scripts/run-e2e.mjs --tier 1 --list` (and `2`, `3`, `4`)
- **Run against a live running Skotos Phase 1 stack**:
  - Single tier: `node e2e/scripts/run-e2e.mjs --tier 1` (supports `1`, `2`, `3`, `4`)
  - All tiers sequentially (`1 -> 2 -> 3 -> 4`): `node e2e/scripts/run-e2e.mjs --tier all`

---

## 5. 4-Tier Testing Methodology & Feature Coverage Plan (N = 16)

| Tier | Purpose | Minimum Threshold | Verification Channel |
|---|---|---|---|
| **Tier 1 — Feature Coverage** | Isolated happy-path behavioral verification of every feature independently (progressive testability) | `>= 5` tests × 16 features = **`>= 80` tests** | Simplest direct channel (HTTP REST API, PostgreSQL check, Playwright UI, or CLI lint check) |
| **Tier 2 — Boundary & Corner Cases** | Invalid inputs, empty/max bounds, malformed JSON, `400`/`401`/`403`/`404`/`409`/`422` RFC 9457 errors, rollout sum `!= 100000`, prerequisite cycles, cross-tenant isolation, blank protected comments, token privilege escalation, last-Owner protection, negative boundary lint probes | `>= 5` tests × 16 features = **`>= 80` tests** | Direct API, DB rollback checks, Playwright UI error/guard assertions, isolated temp-dir lint probes |
| **Tier 3 — Cross-Feature Combinations** | Pairwise interactions across major feature pairs (shared state, data flow, RBAC × protected env × audit, patch × draft simulate × restore × 409, multi-context × 15 ops × prereqs × rollouts, env/flag cross-seeding × key rotation × outbox, UI cross-component flows) | **`>= 24` tests** (`>= N`) | Multi-step API + DB + Playwright UI combinations |
| **Tier 4 — Real-World Application Scenarios** | End-to-end multi-step operator workflows spanning DB + Control API + `skotos-eval` + Playwright Browser UI + Audit Log + API Tokens covering `E2E-01` through `E2E-14` | **`>= 10` tests** (`>= max(5, N/2) = 8`) | Full-stack Playwright + HTTP API + PostgreSQL end-to-end journeys |

### Feature Matrix (N = 16)

1. **F01** — Health, Readiness & Public Discovery (`/healthz`, `/readyz`, `/api/v1/auth/config`, CORS)
2. **F02** — Authentication, Bootstrap & Session Lifecycle (`E2E-01`)
3. **F03** — Projects CRUD & Workspace Management (`E2E-02`)
4. **F04** — Environments CRUD, Color Coding (`--sk-env-accent`) & Protection Rules (`E2E-02`, `E2E-04`)
5. **F05** — Environment Keys & Zero-Downtime Key Rotation (`E2E-10`)
6. **F06** — API Access Tokens CRUD & Bearer Authentication (`E2E-11`)
7. **F07** — Members & Built-In 4-Role RBAC (`Owner`/`Admin`/`Writer`/`Reader`) (`E2E-12`)
8. **F08** — Feature Flags CRUD, Variations & Lifecycle (`E2E-03`)
9. **F09** — Semantic Instruction Patches (23 variants), Optimistic Concurrency (`409`) & RFC 9457 Errors (`E2E-04`, `E2E-07`)
10. **F10** — Targeting Rules, 15 Canonical Operators, Prerequisites & SHA-1 Rollouts (`E2E-05`)
11. **F11** — Evaluation Simulation (`/simulate`) Saved vs Draft & Multi-Context Parity (`E2E-06`)
12. **F12** — Revision History & Monotonic One-Click Restore (`E2E-08`)
13. **F13** — Global Audit Log Search, Filtering & Cursor Pagination (`E2E-09`)
14. **F14** — Transactional ACID Outbox, Partitioned Audit & Database Integrity
15. **F15** — `@skotos/ui` + `apps/web` Theme Persistence, Keyboard Navigation & WCAG 2.1 AA A11y (`E2E-13`)
16. **F16** — Architectural Boundary & Zero-Tailwind CI Guardrails (`E2E-14`)

---

## 6. Data Isolation, Cleanup & Safety Invariants

1. **Per-Test Unique Resource Keys**: Every test generates unique `projectKey`, `envKey`, `flagKey`, member email, and token name (`e2e-<tier>-<ts>-<rand>`) so tests never collide with pre-existing data or one another.
2. **Automatic Fixture Teardown**: Playwright custom fixtures track created projects, tokens, and members and delete/revoke them in `finally` teardown hooks (tolerating already-deleted resources cleanly).
3. **Zero Repository Mutation in Boundary Tests**: Negative boundary lint tests (`F16` Tier 2 / `E2E-14`) ALWAYS create an isolated temporary scratch copy under `os.tmpdir()` (`mktemp`-style `fs.mkdtemp`), inject forbidden imports (`@radix-ui/*`, `radix-ui`, `tailwindcss`, `@skotos/api-client`, raw `<button>`), run ESLint/boundary checks targeting the temp file/copy, verify non-zero exit + expected diagnostic, and remove the temp dir in `finally` — NEVER modifying tracked repository files.
4. **Strict Anti-Cheating Assertions**: Every test MUST assert real behavioral outcomes (exact HTTP status, `application/problem+json` error fields, persisted DB/API state across reload, exact `EvaluationDetail.value`/`variationIndex`/`reason`, visible ARIA UI state, zero serious/critical axe violations). Placeholder tests, `expect(true).toBe(true)`, `test.skip()` bypasses, or silent error swallowing are strictly prohibited and audited by the Forensic Auditor.
