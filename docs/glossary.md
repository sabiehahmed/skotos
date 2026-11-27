# Domain Glossary

Shared ubiquitous language across Skotos APIs, data models, SDKs, and UI.

| Term                       | Definition                                                                                                                                                 |
| -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Organization / Account** | Top-level tenant boundary owning members, billing, SSO configuration, and projects.                                                                        |
| **Project**                | Logical grouping of related feature flags and environments (e.g., `core-web`, `checkout-service`).                                                         |
| **Environment**            | Isolated deployment stage within a project (e.g., `dev`, `staging`, `prod`) with independent targeting rules and SDK credentials.                          |
| **Feature Flag**           | A named decision point with a unique `key`, variation schema (boolean, string, number, JSON), and per-environment targeting configuration.                 |
| **Variation**              | One of the possible return values a flag can serve (e.g., `true`/`false`, or `control`/`variant-a`/`variant-b`).                                           |
| **Context**                | Structured entity being evaluated against a flag. Supports single contexts (e.g., `kind: "user"`) or multi-contexts (`user` + `organization` + `device`).  |
| **Context Kind**           | Category describing a context (`user`, `org`, `device`, `service`), enabling rules that target across multiple entity types simultaneously.                |
| **Targeting Rule**         | Ordered list of `clauses` combined with logical `AND`; when all clauses match, the rule serves a fixed variation or percentage rollout.                    |
| **Clause**                 | Single predicate comparing a context attribute against one or more values via an operator (e.g., `in`, `startsWith`, `semverGreaterThan`, `segmentMatch`). |
| **Segment**                | Reusable named group of contexts (explicitly included/excluded keys plus dynamic rules) shared across multiple flags in an environment.                    |
| **Prerequisite**           | Dependency requiring another flag in the same environment to evaluate to a specific variation before the current flag proceeds.                            |
| **Percentage Rollout**     | Deterministic assignment of contexts across variations based on a hash of `(flagKey, salt, bucketByAttribute)` mapped to `[0, 100000)`.                    |
| **Fallthrough**            | Default variation or percentage rollout served when targeting is ON and no individual target or rule matches.                                              |
| **Off Variation**          | Variation returned immediately when a flag's targeting toggle is OFF or a prerequisite check fails.                                                        |
| **SDK Key**                | Secret credential for server-side SDKs granting access to full raw flag and segment rules for an environment.                                              |
| **Mobile Key**             | Credential for mobile SDKs to request server-evaluated flag values for a given context.                                                                    |
| **Client-Side ID**         | Public environment identifier for browser SDKs (only flags explicitly opted into client-side availability are exposed).                                    |
| **Evaluation Reason**      | Structured metadata explaining _why_ a variation was served (`OFF`, `TARGET_MATCH`, `RULE_MATCH`, `FALLTHROUGH`, `PREREQUISITE_FAILED`, `ERROR`).          |
| **Relay Proxy**            | Optional edge/VPC service that maintains a single stream to Skotos and fans out flag updates locally to many SDK instances.                                |
| **Private Attribute**      | Context attribute used during rule evaluation but redacted before telemetry events leave the SDK.                                                          |
