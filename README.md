# Skotos

**Skotos** is a LaunchDarkly-style feature flagging, targeting, and experimentation platform designed for low-latency flag evaluation, real-time streaming updates, and enterprise-grade governance.

> **Status:** Phase 4 Complete (ClickHouse Telemetry, Data Export, A/B & A/A Experimentation, Guarded Rollouts, Engineering Insights, 2/4/6-Stage Migrations, Synced Segments, Python/Java/Mobile SDKs & VS Code Extension) — Ready for Phase 5

## Quick Start (Local Full Stack)

Prerequisites: Docker (with Compose), Rust (`cargo`), Node.js 22+, `pnpm`, `curl`, `jq`, and `python3`.

```bash
# Start Postgres, NATS JetStream, MinIO + all 4 Rust services + @skotos/web,
# seed realistic demo flags/experiments/guardrails, and verify live end-to-end health:
make dev-up

# Or include ClickHouse (:8123) as the Phase 4 columnar analytics backend:
make dev-up-analytics

# Inspect health, tail logs, re-run smoke test, or tear down:
make dev-status
make dev-logs
make smoke
make dev-down                                # stop app services (keep Docker infra)
./scripts/dev-stack.sh down --infra          # also stop Docker containers
./scripts/dev-stack.sh down --clean          # stop everything and wipe volumes + state
```

| Service                   | Local URL                   | Notes                                                 |
| ------------------------- | --------------------------- | ----------------------------------------------------- |
| Management Dashboard SPA  | `http://localhost:5173`     | Sign in: `admin@skotos.local` / `SkotosDevPass123!`   |
| Control Plane API         | `http://localhost:8080`     | `/readyz`, `/api/v1/...` (proxied at `:5173/api/...`) |
| Flag Delivery Edge        | `http://localhost:8081`     | DB-free SSE (`/stream/v1/all`) & `/sdk/v1/evalx/...`  |
| Event Ingestion           | `http://localhost:8082`     | `/readyz`, `/events/v1/bulk`                          |
| Relay Proxy               | `http://localhost:8083`     | `/status`, `/stream/v1/all`, `/sdk/v1/evalx/...`      |
| MinIO / NATS / ClickHouse | `:9001` / `:8222` / `:8123` | Local backing infrastructure consoles & monitors      |

## Repository Structure

```text
skotos/
├── crates/         # Rust libraries: skotos-domain, skotos-eval, skotos-storage, skotos-auth, skotos-telemetry (Postgres + ClickHouse), skotos-fanout
├── services/       # Rust services: control-api (Axum + utoipa), delivery-edge (DB-free SSE/evalx), event-ingest, relay-proxy (DB-free)
├── sdks/           # Official SDKs + OpenFeature providers: rust-server, go-server, node-server, python-server, java-server, edge, browser, react, mobile
├── tools/          # Developer CLIs & AI tooling: skotos-cli, skotos-code-refs, skotos-mcp (stdio + HTTP MCP server)
├── integrations/   # Ecosystem providers: terraform-provider-skotos (Go), vscode-skotos (VS Code / Cursor extension)
├── packages/       # TypeScript packages: @skotos/ui (Radix + CSS Modules, zero Tailwind), @skotos/api-client
├── apps/           # Frontend apps: @skotos/web (React + Vite Management Dashboard SPA)
├── spec/           # Canonical Golden Evaluation Test Suite (124 vectors) and OpenAPI 3.1 schema
├── deploy/         # Local Docker Compose dev stack (PostgreSQL 16, NATS JetStream, MinIO, optional ClickHouse analytics profile)
├── e2e/            # Playwright real-browser end-to-end & WCAG 2.1 AA accessibility verification
├── docs/           # Product vision, feature catalog, requirements, roadmap, implementation plan, and RFCs
├── design/         # Domain model, evaluation semantics, API contracts, SDKs, and UI/UX
└── architecture/   # System architecture, delivery pipelines, reliability, security, and ADRs
```

### Quick Navigation

- **Product & Planning (`docs/`)**
  - [Docs Index](docs/README.md)
  - [Product Vision](docs/product-vision.md)
  - [LaunchDarkly Feature Catalog & Cost Strategy](docs/launchdarkly-feature-catalog.md)
  - [Implementation Plan](docs/implementation-plan.md)
  - [Domain Glossary](docs/glossary.md)
  - [Requirements](docs/requirements.md)
  - [Roadmap](docs/roadmap.md)
  - [RFCs](docs/rfcs/README.md)
- **Technical Design (`design/`)**
  - [Design Index](design/README.md)
  - [Domain Model](design/domain-model.md)
  - [Evaluation Engine Semantics](design/evaluation-engine.md)
  - [API Contracts](design/api-contracts.md)
  - [SDK Architecture](design/sdk-architecture.md)
  - [UI & UX Specification](design/ui-ux.md)
- **System Architecture (`architecture/`)**
  - [Architecture Index](architecture/README.md)
  - [System Overview](architecture/system-overview.md)
  - [Flag Delivery Pipeline](architecture/flag-delivery-pipeline.md)
  - [Scalability & Reliability](architecture/scalability-and-reliability.md)
  - [Security & Multi-Tenancy](architecture/security-and-tenancy.md)
  - [Architecture Decision Records (ADRs)](architecture/adrs/README.md)

## Documentation Quality Checks

Validate Markdown formatting and lint rules across all planning documents:

```bash
markdownlint-cli2 "README.md" "docs/**/*.md" "design/**/*.md" "architecture/**/*.md"
prettier --check "README.md" "docs/**/*.md" "design/**/*.md" "architecture/**/*.md"
```
