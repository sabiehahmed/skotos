# Requirements

## Functional Requirements

### P0 — Core MVP (Minimum Usable Platform)

- **Workspace Hierarchy**: Organizations, Projects, Environments, and environment-scoped SDK credentials (SDK Key, Client-Side ID).
- **Flag Management**: Create, update, toggle, and archive boolean and multivariate (string, number, JSON) flags with per-environment configurations.
- **Targeting Engine**:
  - Flag ON/OFF kill switch and `offVariation`.
  - Individual context key targeting per variation.
  - Ordered targeting rules with core operators (`in`, `eq`, `neq`, `contains`, `startsWith`, `endsWith`, `matches`, numeric comparisons, semver comparisons).
  - Deterministic percentage rollouts with configurable `bucketBy` attribute.
  - Default `fallthrough` rule.
- **Real-Time Delivery**:
  - Server-side SSE streaming endpoint delivering initial snapshot (`put`) and incremental updates (`patch`, `delete`).
  - Polling fallback with `ETag` / `If-None-Match`.
  - Client-side evaluation endpoint returning evaluated flag maps for a provided context.
- **Initial SDKs**: At least one server-side SDK and one browser/client-side SDK verified against a shared golden evaluation test suite.
- **Basic Audit Log**: Immutable record of who changed which flag/environment, timestamp, and before/after diff.
- **Management Dashboard**: Web UI for managing projects, environments, flags, targeting rules, and SDK keys.

### P1 — Production Readiness

- **Reusable Segments**: Rule-based segments and large static list segments (e.g., 100k+ keys).
- **Flag Prerequisites**: Cross-flag dependency evaluation with cycle detection at write time.
- **Evaluation Analytics**: Ingest summary evaluation counts, active variation breakdown, last-evaluated timestamps, and stale flag detection.
- **RBAC & Team Access**: Built-in roles (Owner, Admin, Writer, Reader) plus environment-level restrictions and personal/service API access tokens.
- **Webhooks & Integrations**: Signed outbound webhooks on flag mutations (Slack, Datadog, PagerDuty, custom HTTP).
- **Relay Proxy**: Standalone lightweight proxy binary/container with in-memory or Redis backing store.
- **Live Flag Debugger**: Real-time stream of evaluation events for debugging targeting in dev/staging.

### P2 — Enterprise & Experimentation

- **Approval Workflows & Change Requests**: Peer review gates and scheduled flag changes before applying mutations to production.
- **Custom RBAC Policies**: Resource-scoped allow/deny policy engine (tag-based, project-based, environment-based).
- **Experimentation**: Metric definitions, exposure tracking, guardrails, and Bayesian/Frequentist lift analysis.
- **Flag Triggers**: Secret URL triggers for automated rollback from APM/alerting tools.
- **Multi-Region Edge Delivery**: Geo-distributed streaming POPs and CDN-backed flag snapshots.
- **SSO / SAML / OIDC & SCIM**: Enterprise identity federation.

## Non-Functional Requirements (Target Budgets)

| Dimension                        | Target                                                        | Notes                                                                         |
| -------------------------------- | ------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| **Server SDK local evaluation**  | `< 1 ms` p99                                                  | Zero network hop on hot evaluation path                                       |
| **Client-side eval API latency** | `< 50 ms` p99 (regional)                                      | Cached ruleset evaluated at edge/delivery tier                                |
| **Flag change propagation**      | `< 200 ms` p95, `< 1 s` p99                                   | From control-plane commit to connected SDK SSE delivery                       |
| **Delivery plane availability**  | `99.99%`                                                      | Strictly decoupled from control-plane outages                                 |
| **SDK survivability**            | Indefinite during outage                                      | Last-known-good state + code fallbacks                                        |
| **Consistency model**            | Strong in Control Plane; monotonic eventual in Delivery Plane | Monotonic per-environment version / revision numbers prevent stale overwrites |
| **PII & Privacy**                | Zero leakage of private attributes                            | Redacted at SDK boundary before telemetry transmission                        |
| **Auditability**                 | 100% of control-plane mutations logged                        | Transactional with state write (outbox / same DB tx)                          |
