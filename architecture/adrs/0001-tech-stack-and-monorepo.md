# ADR-0001: Core Tech Stack & Polyglot Monorepo

- **Status:** Accepted
- **Date:** 2026-11-27
- **Deciders:** Skotos Core Team

## Context and Problem Statement

Skotos aims to deliver full LaunchDarkly feature parity at **5x–10x lower infrastructure cost (COGS)** across both multi-tenant Cloud SaaS and self-hosted enterprise VPC deployments (Docker Compose / Helm). We must choose a backend stack, frontend stack, API contract workflow, and repository structure that optimize for:

- Extreme concurrent SSE connection density (`100k–250k+` idle sockets per small VM).
- Sub-millisecond edge flag evaluation with predictable p99 tail latency (zero GC pauses).
- Lightweight, single-binary distribution for edge nodes, Relay Proxy, CLI, and CI code-reference scanners.
- Clean frontend architecture with strict UI component reuse and zero SSR compute overhead.

## Decision Drivers

- Infrastructure cost efficiency per Service Connection and per 1K Client-Side MAU.
- Zero-GC tail latency and minimal memory footprint per open connection.
- Strong compile-time correctness across database queries, evaluation rules, and API schemas.
- Clean separation between reusable UI components and product business logic.
- Fast onboarding and atomic cross-layer changes (backend schema + OpenAPI + TS client + UI + golden tests).

## Considered Options

### Backend

1. **Rust for all backend planes (`Axum`, `Tokio`, `SQLx`, `async-nats`, `object_store`, `utoipa`)**
2. Go for all backend planes
3. Hybrid TypeScript/Node.js Control Plane + Go/Rust Delivery Edge
4. TypeScript/Node.js everywhere

### Frontend

1. **React (latest) + TypeScript (latest) + Vite + Vitest SPA with a standalone CSS-Modules UI library (`@skotos/ui`, zero Tailwind)**
2. React + Vite + Tailwind/shadcn inside a single app
3. Next.js (App Router) SSR

### Repository Layout

1. **Single polyglot monorepo (Cargo workspace + pnpm workspace + shared golden spec)**
2. Multi-repo split per service and package

## Decision Outcome

### 1. Backend: Rust Everywhere

Chosen option: **Rust** across all backend services and native binaries:

- **HTTP / SSE**: `axum` + `tokio` + `tower`
- **Database**: `sqlx` (PostgreSQL async driver with compile-time checked queries and SQL migrations)
- **Streaming Fanout**: `async-nats` (NATS JetStream)
- **Object Storage**: `object_store` (Cloudflare R2 / AWS S3 / MinIO)
- **OpenAPI Codegen**: `utoipa` generating OpenAPI 3.1 directly from Rust request/response DTOs
- **Observability**: `tracing` + `opentelemetry`
- **Shared Core**: `crates/skotos-eval` powers Control Plane simulation, Delivery Edge `evalx`, Relay Proxy, and the Rust Server SDK directly with zero logic duplication.

### 2. Frontend: React + Vite + Vitest SPA & Standalone `@skotos/ui`

Chosen option: **React (latest) + TypeScript (latest) + Vite + Vitest** as a pure static SPA (zero SSR compute cost; served from CDN/R2 in SaaS or embedded into the Rust binary for self-hosted deployments). All UI components live exclusively in `@skotos/ui` using Radix UI primitives + CSS Modules (`.module.css`) with zero Tailwind (detailed in [ADR-0006](0006-ui-component-library-css-modules.md)).

### 3. API Contracts: Semantic Instruction Patches + Generated TS Client

Control Plane mutations use **semantic instruction patches** (e.g., `turnFlagOn`, `addRule`, `updateFallthrough`, `addTargets`) rather than raw whole-document overwrites. Rust `utoipa` emits an OpenAPI 3.1 schema that generates `packages/api-client` automatically for `apps/web`, CLI, and Terraform tooling.

### 4. Repository Structure: Single Polyglot Monorepo

```text
skotos/
├── Cargo.toml                  # Rust workspace root
├── pnpm-workspace.yaml         # JS/TS workspace root
├── crates/
│   ├── skotos-domain/          # Core domain types, semantic patches, validation
│   ├── skotos-eval/            # Canonical Rust flag evaluation engine
│   ├── skotos-storage/         # Postgres repositories, migrations, outbox
│   ├── skotos-auth/            # IdentityProvider trait (SaaS external + self-hosted) & RBAC
│   ├── skotos-fanout/          # NATS JetStream publisher/subscriber & R2/S3 snapshot store
│   └── skotos-telemetry/       # TelemetryStore trait (PG counters -> ClickHouse)
├── services/
│   ├── control-api/            # Control Plane REST API + OpenAPI generator + outbox/scheduler worker
│   ├── delivery-edge/          # High-density SSE streaming, ETag polling, client evalx
│   ├── event-ingest/           # High-throughput SDK summary & telemetry ingestion
│   └── relay-proxy/            # Open-source standalone VPC Relay Proxy binary
├── packages/
│   ├── ui/                     # @skotos/ui — ALL UI components (Radix + .module.css, zero Tailwind)
│   └── api-client/             # @skotos/api-client — OpenAPI-generated typed TS client
├── apps/
│   └── web/                    # Main Management Dashboard SPA (consumes @skotos/ui only)
├── sdks/
│   ├── js-server/              # Node/TS Server SDK + OpenFeature provider
│   ├── js-client/              # Browser & React Client SDK + OpenFeature provider
│   ├── go-server/              # Go Server SDK + OpenFeature provider
│   └── rust-server/            # Rust Server SDK (reuses crates/skotos-eval)
├── spec/
│   ├── openapi/                # Exported OpenAPI 3.1 specs
│   └── evaluation-tests/       # Language-agnostic Golden Evaluation Test Suite
├── deploy/
│   ├── docker/                 # Local dev & self-hosted Docker Compose stacks
│   └── helm/                   # Kubernetes Helm charts (SaaS & Enterprise VPC)
├── docs/
├── design/
└── architecture/
```

### Positive Consequences

- Industry-leading memory density (~2–4 KB per idle SSE connection) and zero GC spikes make Service Connection and MAU COGS negligible.
- One compile-time-verified domain model across Control Plane, Delivery Edge, Relay Proxy, and OpenAPI schema.
- Atomic PRs spanning Rust API + generated TS client + `@skotos/ui` + `apps/web` + golden tests.
- Self-hosted deployments run as compact static binaries + Postgres + NATS.

### Negative Consequences / Tradeoffs

- Slower Rust compilation times compared to Go/TS.
  - _Mitigation_: Granular Cargo workspace crates, `sccache` / `mold` linker in CI, and thin service binaries wrapping focused library crates.
- Higher upfront discipline required for Rust async/lifetimes.
  - _Mitigation_: Standardize on `axum` + `sqlx` + `thiserror` idioms and enforce clean crate boundaries from Phase 1.

## Links & References

- [ADR-0006: Standalone UI Component Library & CSS Modules](0006-ui-component-library-css-modules.md)
- [System Overview](../system-overview.md)
