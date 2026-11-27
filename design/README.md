# Technical Design Specifications

This folder contains detailed technical designs for Skotos's core domain model, flag evaluation semantics, external APIs, SDK contracts, and dashboard UX.

## Contents

| Document                                  | Purpose                                                                                     |
| ----------------------------------------- | ------------------------------------------------------------------------------------------- |
| [Domain Model](domain-model.md)           | Core entities, relationships, multi-context schema, and flag versioning                     |
| [Evaluation Engine](evaluation-engine.md) | Deterministic rule evaluation order, hashing/bucketing, operators, and golden test strategy |
| [API Contracts](api-contracts.md)         | Control Plane REST, Delivery Streaming/Polling, Client Eval, and Event Ingestion APIs       |
| [SDK Architecture](sdk-architecture.md)   | Server-side vs client-side SDK behavior, caching, reconnection, and offline guarantees      |
| [UI & UX Specification](ui-ux.md)         | Dashboard information architecture, targeting builder, and core operational workflows       |

## Related Folders

- Product vision, requirements, and roadmap live in [`../docs/`](../docs/README.md).
- Distributed architecture, delivery pipelines, and ADRs live in [`../architecture/`](../architecture/README.md).
