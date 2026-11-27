# Request for Comments (RFCs)

RFCs are used to propose substantial product capabilities, API additions, protocol changes, or cross-cutting refactors before implementation begins.

## When to Write an RFC

- Introducing a new public API surface or wire protocol change (e.g., SSE event format, SDK contract).
- Adding major domain concepts (e.g., big segments, approval workflows, experimentation).
- Making breaking changes to flag schemas or evaluation semantics.

_(For narrower architectural technology/tradeoff choices, write an [ADR](../../architecture/adrs/README.md) instead.)_

## Process

1. Copy [`0000-template.md`](0000-template.md) to `NNNN-short-title.md`.
2. Fill in motivation, design details, alternatives, and rollout plan.
3. Review, iterate, and update status (`Draft` -> `In Review` -> `Accepted` / `Rejected` / `Superseded`).

## Index

| ID   | Title                            | Status   | Date |
| ---- | -------------------------------- | -------- | ---- |
| 0000 | [RFC Template](0000-template.md) | Template | —    |
