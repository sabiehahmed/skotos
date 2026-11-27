# Roadmap

> **Note:** This is a high-level phase skeleton created during initial setup. We will refine tasks, timelines, and exact stack choices into an actionable implementation plan once core ADRs are finalized.

## Phase 0 — Planning & Architecture (Current)

- [x] Initialize `docs/`, `design/`, and `architecture/` workspace structure
- [ ] Finalize core technical decisions (ADRs 0001–0005): language/runtime stack, databases, streaming transport, monorepo layout
- [ ] Draft OpenAPI specifications for Control Plane, Delivery Plane, and Event Ingestion APIs
- [ ] Define canonical JSON schema for flags, segments, contexts, and the golden evaluation test suite
- [ ] Create detailed Phase 1 implementation plan

## Phase 1 — Control Plane & Evaluation Engine Core

- [ ] Provision local development stack (Docker Compose: primary DB, cache/broker)
- [ ] Implement Organization, Project, Environment, and Flag CRUD APIs with transactional versioning and audit logs
- [ ] Implement canonical Evaluation Engine and pass 100% of golden evaluation test cases
- [ ] Build initial Management Dashboard (flag list, flag editor, targeting rule builder, environment switcher)

## Phase 2 — Real-Time Delivery & First SDKs

- [ ] Build Flag Delivery service (snapshot cache, SSE stream fanout, ETag polling, client-side evaluation endpoint)
- [ ] Connect Control Plane mutations to Delivery Plane via transactional outbox + pub/sub
- [ ] Build first Server-Side SDK (streaming, local store, offline/bootstrap mode, golden test compliance)
- [ ] Build first Client-Side / Browser SDK (React/JS)

## Phase 3 — Segments, Prerequisites & Governance

- [ ] Add reusable Segments (dynamic rules + big static segments) and Flag Prerequisites with cycle validation
- [ ] Add RBAC, API access tokens, and environment protection settings
- [ ] Add signed outbound Webhooks and Audit Log search/filtering UI

## Phase 4 — Analytics, Telemetry & Relay Proxy

- [ ] Build Event Ingestion pipeline (batching, summary aggregation, variation usage counters, stale flag detection)
- [ ] Add Live Flag Debugger UI
- [ ] Build standalone Relay Proxy (in-memory & Redis stores)

## Phase 5 — Enterprise Workflows & Experimentation

- [ ] Approval workflows, scheduled changes, and rollback triggers
- [ ] Experimentation metrics and statistical analysis engine
- [ ] Multi-region delivery hardening, load testing, and operational runbooks
