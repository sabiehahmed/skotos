# Architecture Decision Records (ADRs)

ADRs capture significant architectural choices, the context behind them, the alternatives considered, and their consequences.

## Process

1. Copy [`0000-template.md`](0000-template.md) to `NNNN-short-title.md`.
2. Fill in Context, Decision Drivers, Considered Options, Decision Outcome, and Consequences.
3. Update the index table below when an ADR is drafted, accepted, or superseded.

## ADR Index

| ID   | Decision                                                                            | Status   | Target Phase |
| ---- | ----------------------------------------------------------------------------------- | -------- | ------------ |
| 0000 | [ADR Template](0000-template.md)                                                    | Template | —            |
| 0001 | Core Tech Stack & Monorepo Structure (Backend, Frontend, Build System)              | Planned  | Phase 0      |
| 0002 | Primary Data Store & Flag Schema Strategy (Relational vs JSONB vs Hybrid)           | Planned  | Phase 0      |
| 0003 | Real-Time Streaming & Fanout Backbone (Redis vs NATS vs Kafka vs PG Notify)         | Planned  | Phase 0      |
| 0004 | Multi-SDK Evaluation Engine Strategy (Shared Core vs Ported + Golden Tests)         | Planned  | Phase 0      |
| 0005 | Event Ingestion & Analytics Storage (ClickHouse vs Postgres/Timescale vs Buffering) | Planned  | Phase 0      |
