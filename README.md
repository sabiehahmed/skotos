# Skotos

**Skotos** is a LaunchDarkly-style feature flagging, targeting, and experimentation platform designed for low-latency flag evaluation, real-time streaming updates, and enterprise-grade governance.

> **Status:** Phase 1 Complete (Core Control Plane, Evaluation Engine, `@skotos/ui`, and Management Dashboard MVP) — Ready for Phase 2

## Repository Structure

```text
skotos/
├── crates/         # Rust workspace libraries: skotos-domain, skotos-eval, skotos-storage, skotos-auth
├── services/       # Rust backend services: control-api (Axum + utoipa OpenAPI 3.1)
├── packages/       # TypeScript packages: @skotos/ui (Radix + CSS Modules, zero Tailwind), @skotos/api-client
├── apps/           # Frontend apps: @skotos/web (React + Vite Management Dashboard SPA)
├── spec/           # Canonical Golden Evaluation Test Suite (124 vectors) and OpenAPI 3.1 schema
├── deploy/         # Local Docker Compose dev stack (PostgreSQL 16, NATS JetStream, MinIO)
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
