# Roadmap

All Phase 0 architectural decisions are finalized in [`architecture/adrs/`](../architecture/adrs/README.md) and expanded into engineering deliverables in [`implementation-plan.md`](implementation-plan.md).

## Phase 0 — Planning & Architecture (Complete)

- [x] Initialize `docs/`, `design/`, and `architecture/` workspace structure
- [x] Catalog full LaunchDarkly feature parity matrix & cost-effective architecture strategy ([`launchdarkly-feature-catalog.md`](launchdarkly-feature-catalog.md))
- [x] Finalize ADRs 0001–0006 (Rust backend, React/Vite + standalone `@skotos/ui` CSS Modules, PostgreSQL hybrid schema + dual-path auth, NATS JetStream + R2 snapshots + Ping-to-Re-evaluate, Golden Test Suite SDKs, Phased Postgres -> ClickHouse telemetry)
- [x] Author detailed phased implementation plan ([`implementation-plan.md`](implementation-plan.md))

## Phase 1 — Core Control Plane, Evaluation Engine & Dashboard MVP (Complete)

- [x] **1A**: Monorepo bootstrap (Cargo + pnpm workspaces, Docker Compose `postgres + nats + minio`, ESLint `@skotos/ui` boundary enforcement, CI)
- [x] **1B**: `crates/skotos-domain`, `crates/skotos-eval`, and `spec/evaluation-tests/` Golden Test Suite (124 fixtures)
- [x] **1C**: `crates/skotos-storage` (migrations + transactional outbox/audit), `crates/skotos-auth` (SaaS external + self-hosted adapters), `services/control-api` (semantic patches + `/simulate` + `utoipa` OpenAPI -> `packages/api-client`)
- [x] **1D**: Standalone `@skotos/ui` library (Radix UI + `.module.css`, zero Tailwind, design tokens, Vitest + WCAG 2.1 AA axe coverage)
- [x] **1E**: `apps/web` Management Dashboard consuming `@skotos/ui` + `@skotos/api-client` exclusively

## Phase 2 — Real-Time Delivery Plane, Event Ingest & First SDK Suite (Next)

- [ ] Outbox -> R2/S3 pre-compressed snapshots -> NATS JetStream publisher
- [ ] `services/delivery-edge` (Server SSE `put`/`patch`/`delete`, ETag polling, Client Ping-to-Re-evaluate SSE + `evalx`)
- [ ] `services/event-ingest` (5s SDK summary rollups -> PostgreSQL counters)
- [ ] Segments, Prerequisites, Flag Scheduling, Flag Triggers, Signed Webhooks
- [ ] Launch SDKs + OpenFeature providers: TS/Node Server, Browser/React Client, Go Server, Rust Server

## Phase 3 — Enterprise Governance, Relay Proxy, DevTools & Scale

- [ ] Custom RBAC policies, Teams, Mandatory Approval Workflows, SAML/SCIM & Self-Hosted OIDC
- [ ] Flag Workflows, Release Pipelines & Release Plans
- [ ] Big Segments (Roaring Bitmaps + R2/Redis), open-source `services/relay-proxy`, Edge SDKs, Live Debugger
- [ ] `skotos-code-refs` CI scanner, `skotos-cli`, Terraform provider, Hosted/Local MCP Server, Slack/Jira/Datadog integrations

## Phase 4 — ClickHouse Telemetry, Guarded Rollouts & Experimentation Core

- [ ] ClickHouse `TelemetryStore` backend + Real-Time Data Export sinks
- [ ] Guarded Rollouts (Release Guardian + auto-rollback + SRM checks), Adaptive Triggers, Engineering Insights
- [ ] A/B/n Experimentation (Frequentist & Bayesian, Funnel/Metric Groups, Mutual Exclusion)
- [ ] Flag-scoped Feature Monitoring, 2/4/6-Stage Migration Flags, Synced Segments, Python/Java/Mobile SDKs

## Phase 5 — Warehouse-Native, Full OTel Observability & AgentControl

- [ ] Warehouse-Native Experimentation (Snowflake/BigQuery/Redshift/Databricks/Postgres), CUPED, Sequential Testing, Multi-Armed Bandits, Global Holdouts
- [ ] Full OTel Logs, Distributed Traces, Error Monitoring, Ingestion Sampling Rules, Dashboards & Alerts
- [ ] AgentControl / AI Configs (Completion & Agent modes, Prompt Snippets, Tools Library, Agent Graphs, BYOK Playground, Guardrails, LLM Observability)

## Phase 6 — Session Replays, Online AI Evals & Autonomous Release Autopilot

- [ ] `rrweb` Session Replays stored Zstd-compressed in zero-egress R2/S3
- [ ] AI Datasets, Offline Batch Evals & Online LLM-as-a-Judge (BYOK) wired to Guardrails & Experiments
- [ ] AI Release Autopilot GitHub App (risk classifier, auto-flagging, auto-cleanup, auto-fix) & Observability auto-remediation agent
