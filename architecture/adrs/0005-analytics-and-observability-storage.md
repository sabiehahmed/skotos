# ADR-0005: Event Ingestion, Analytics & Observability Storage

- **Status:** Accepted
- **Date:** 2026-11-27
- **Deciders:** Skotos Core Team

## Context and Problem Statement

Skotos will eventually support high-volume flag evaluation insights, Experimentation (Bayesian/Frequentist/CUPED/Bandits), Guarded Rollouts, AI Config monitoring, and full Observability (OTel logs, distributed traces, error monitoring, and session replays) — while maintaining ultra-low COGS and keeping Phase 1–2 deployment lightweight.

## Decision Drivers

- Avoid operating heavy analytics/OLAP infrastructure during Phases 1–2 before telemetry volume justifies it.
- Design a clean storage abstraction from day one so upgrading to ClickHouse + R2 in Phase 4 requires zero API or SDK breaking changes.
- Achieve 10x–20x lower observability & replay storage COGS than commercial APM/LaunchDarkly pricing.
- Offload heavy experimentation compute to customer data warehouses wherever possible.

## Considered Options

1. **Two-stage architecture behind `TelemetryStore` trait: PostgreSQL-only summary counters in Phases 1–2 -> ClickHouse + zero-egress R2/S3 (Parquet + compressed `rrweb` blobs) + Warehouse-Native SQL pushdown in Phases 4–6**
2. ClickHouse + S3/R2 from Phase 1
3. PostgreSQL + TimescaleDB / DuckDB permanently

## Decision Outcome

Chosen option: **Phased `TelemetryStore` architecture — PostgreSQL counters first (Phases 1–2), ClickHouse + Zero-Egress R2/S3 + Warehouse-Native SQL (Phases 4–6)**:

### 1. SDK-Side Summary Aggregation (Day 1)

- Server and client SDKs aggregate evaluations locally over a 5-second flush window into compact `summary` buckets (`flagKey`, `version`, `variation`, `count`, `contextKinds`) and submit gzipped batches to `services/event-ingest`.
- Raw per-evaluation `feature`/`debug` events are emitted **only** when explicitly enabled for an experiment, guarded rollout, or live debugger session.

### 2. Phases 1–2: Lightweight PostgreSQL Counters

- `crates/skotos-telemetry` writes rolled-up hourly/daily evaluation counts and `last_evaluated_at` timestamps to a compact PostgreSQL table (`flag_eval_rollups`) via async batched upserts.
- Powers flag activity badges, variation split charts, and stale flag detection with **zero extra database containers**.

### 3. Phases 4–6: ClickHouse + Zero-Egress R2/S3 + Warehouse-Native

- **ClickHouse** handles high-volume ingestion via `services/event-ingest` (including native OTLP HTTP/gRPC):
  - Evaluation summaries, exposure events, custom metric events, migration metrics, OTel logs, distributed traces, error events, and AI/LLM traces.
  - Aggressive Zstd/Delta/LowCardinality compression + materialized views for instant dashboard/guardrail queries + TTL tiering to Parquet on R2/S3.
  - Ingestion filter & sampling rules execute in Rust at the ingestion edge _before_ ClickHouse writes.
- **Session Replays (Phase 6)**:
  - Open-source `rrweb` streams are Zstd-compressed and uploaded directly to **zero-egress R2/S3** via presigned URLs; ClickHouse stores only compact searchable metadata pointers (`session_id`, `context_key`, `flag_variations`, `error_count`, `blob_key`).
- **Warehouse-Native Experimentation (Phase 5)**:
  - Executes parameterized SQL (exposure joins, metrics, CUPED covariates) directly inside the customer's Snowflake, BigQuery, Redshift, Databricks, or Postgres warehouse; Skotos computes Frequentist/Bayesian/Sequential statistics on the returned sufficient statistics (`n`, `sum`, `sum_sq`), driving Skotos compute COGS near zero.

### Positive Consequences

- Phase 1–2 local dev and self-hosted footprint stays tiny (just Postgres + NATS).
- Zero breaking changes when scaling into Phase 4–6 Experimentation and Observability.
- Columnar ClickHouse + R2 object blobs + Warehouse-Native SQL delivers 10x+ cost advantage over Elasticsearch/Datadog/LaunchDarkly observability tiers.

### Negative Consequences / Tradeoffs

- Maintaining two `TelemetryStore` backends (`PostgresTelemetryStore` for lightweight/small self-hosted installs and `ClickHouseTelemetryStore` for SaaS/scale) requires keeping the trait interface clean.
  - _Mitigation_: Scope the Phase 1–2 Postgres backend strictly to flag evaluation summaries and `last_evaluated_at`, while Observability & hosted Experimentation explicitly target ClickHouse (or customer warehouse).

## Links & References

- [LaunchDarkly Feature Catalog & Cost Strategy](../../docs/launchdarkly-feature-catalog.md)
- [Scalability & Reliability](../scalability-and-reliability.md)
