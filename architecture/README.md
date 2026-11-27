# System Architecture & Decisions

This folder defines Skotos's distributed system topology, real-time flag delivery mechanics, reliability guarantees, security model, and Architecture Decision Records (ADRs).

## Contents

| Document                                                    | Purpose                                                                         |
| ----------------------------------------------------------- | ------------------------------------------------------------------------------- |
| [System Overview](system-overview.md)                       | Three-plane architecture separating Control, Delivery, and Analytics            |
| [Flag Delivery Pipeline](flag-delivery-pipeline.md)         | Outbox -> fanout -> SSE streaming propagation and reconnect storm handling      |
| [Scalability & Reliability](scalability-and-reliability.md) | Fault isolation, blast-radius reduction, HA topology, and failure modes         |
| [Security & Multi-Tenancy](security-and-tenancy.md)         | Key isolation, RBAC, tenant boundaries, private attributes, and audit integrity |
| [ADRs](adrs/README.md)                                      | Architecture Decision Records log and template                                  |

## Related Folders

- Product vision, requirements, and roadmap live in [`../docs/`](../docs/README.md).
- Domain model, evaluation engine, API contracts, SDKs, and UI specs live in [`../design/`](../design/README.md).
