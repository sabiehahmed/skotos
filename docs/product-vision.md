# Product Vision

## Mission

Enable engineering and product teams to decouple **code deployment** from **feature release** with sub-millisecond local flag evaluation, sub-second global rule propagation, and safe operational guardrails.

## Target Personas

1. **Application Engineers** — Integrate lightweight server-side and client-side SDKs to gate features, run dark launches, and trigger instant kill switches without redeploying.
2. **Product & Release Managers** — Manage progressive rollouts, beta user targeting, scheduled releases, and A/B experiments through a self-serve dashboard.
3. **Platform / SRE Teams** — Enforce approvals, audit every flag mutation, deploy on-prem/VPC Relay Proxies, and rely on zero-downtime resilience even if the control plane is unreachable.

## Core Capability Pillars

1. **Feature Flag Lifecycle**
   - Boolean, string, number, and JSON multivariate flags.
   - Multi-project, multi-environment isolation (`development`, `staging`, `production`, custom).
   - Flag prerequisites, tags, maintainers, deprecation, and archival workflows.
2. **Deterministic Targeting & Progressive Rollouts**
   - Multi-kind evaluation contexts (`user`, `organization`, `device`, custom kinds).
   - Individual targets, complex rule clauses, reusable user/context segments, and consistent percentage rollouts.
3. **Real-Time Streaming Delivery**
   - Push-based Server-Sent Events (SSE) updates from control plane changes to connected SDKs in milliseconds.
   - Local in-memory evaluation on server SDKs and secure pre-evaluated flag payloads for browser/mobile SDKs.
4. **Governance & Operational Safety**
   - Full immutable audit trail with structured diffs.
   - Role-Based Access Control (RBAC), custom policies, approval workflows, and environment protection rules.
5. **Observability & Experimentation**
   - High-throughput evaluation telemetry (flag usage counts, variation distribution, stale flag detection).
   - Custom metric tracking and statistical experimentation attached to flag variations.
6. **Self-Hostable Edge & Relay Proxy**
   - Optional Relay Proxy that sits inside customer VPCs to multiplex SDK connections and serve cached flag definitions during upstream outages.

## Finalized Strategic Scope

- **Distribution**: **Multi-tenant Cloud SaaS first**, engineered with lean infrastructure so we also ship a clean **Docker Compose / Helm self-hosted edition** for enterprise VPC customers ([ADR-0001](../architecture/adrs/0001-tech-stack-and-monorepo.md)).
- **Cost Advantage**: Full LaunchDarkly feature parity built on Rust, PostgreSQL, NATS JetStream, zero-egress R2/S3, and ClickHouse (Phase 4+) to undercut LaunchDarkly pricing by **5x–10x** ([LaunchDarkly Feature Catalog](launchdarkly-feature-catalog.md)).
- **Initial SDK Launch Suite**: TypeScript/Node Server, Browser/React Client, Go Server, and Rust Server — each with first-class **OpenFeature** providers ([ADR-0004](../architecture/adrs/0004-sdk-evaluation-engine.md)).
- **Experimentation & Observability**: Sequenced into Phases 4–6 behind clean `TelemetryStore` and Warehouse-Native interfaces ([ADR-0005](../architecture/adrs/0005-analytics-and-observability-storage.md)).
