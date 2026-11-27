# Architecture Decision Records (ADRs)

ADRs capture significant architectural choices, the context behind them, the alternatives considered, and their consequences.

## Process

1. Copy [`0000-template.md`](0000-template.md) to `NNNN-short-title.md`.
2. Fill in Context, Decision Drivers, Considered Options, Decision Outcome, and Consequences.
3. Update the index table below when an ADR is drafted, accepted, or superseded.

## ADR Index

| ID   | Decision                                                                                                                        | Status   | Date       |
| ---- | ------------------------------------------------------------------------------------------------------------------------------- | -------- | ---------- |
| 0000 | [ADR Template](0000-template.md)                                                                                                | Template | —          |
| 0001 | [Core Tech Stack & Polyglot Monorepo (Rust + React/Vite + Cargo/pnpm)](0001-tech-stack-and-monorepo.md)                         | Accepted | 2026-11-27 |
| 0002 | [Primary Data Store, Hybrid Schema & Dual-Path Auth](0002-primary-datastore-and-schema.md)                                      | Accepted | 2026-11-27 |
| 0003 | [Real-Time Streaming Fanout, R2/S3 Snapshots & Ping-to-Re-evaluate Client Delivery](0003-streaming-and-snapshot-delivery.md)    | Accepted | 2026-11-27 |
| 0004 | [Multi-SDK Evaluation Engine & Golden Test Suite Strategy](0004-sdk-evaluation-engine.md)                                       | Accepted | 2026-11-27 |
| 0005 | [Event Ingestion, Analytics & Observability Storage (Postgres -> ClickHouse + R2)](0005-analytics-and-observability-storage.md) | Accepted | 2026-11-27 |
| 0006 | [Standalone UI Component Library (`@skotos/ui`) & Zero-Tailwind CSS Modules](0006-ui-component-library-css-modules.md)          | Accepted | 2026-11-27 |
