# ADR-0002: Primary Data Store, Schema Strategy & Auth Architecture

- **Status:** Accepted
- **Date:** 2026-11-27
- **Deciders:** Skotos Core Team

## Context and Problem Statement

The Control Plane requires strong ACID consistency for flag mutations, version increments, audit logs, RBAC enforcement, approval workflows, and scheduled changes — while keeping infrastructure simple and cheap for both Cloud SaaS and self-hosted enterprise VPCs. We also need a clean authentication boundary supporting an external auth provider (e.g., WorkOS / Auth0 / Clerk) in Cloud SaaS alongside a self-contained auth path for self-hosted VPC deployments.

## Decision Drivers

- Consolidate Control Plane persistence into a single battle-tested ACID store to minimize operational cost.
- Guarantee zero dual-write drift between flag updates, audit logs, and delivery outbox events.
- Preserve fast flag snapshot compilation and rich historical version diffs.
- Keep Cloud SaaS auth fast to ship (with enterprise SAML/SCIM) without coupling core authorization or self-hosted deployments to a SaaS vendor.

## Considered Options

### Primary Data Store & Flag Schema

1. **PostgreSQL 16+ with hybrid relational hierarchy + versioned `JSONB` flag/segment rules + Transactional Outbox**
2. Fully normalized relational tables for every rule/clause/value
3. Document database (MongoDB / DynamoDB) for all Control Plane entities

### Authentication

1. **Pluggable `IdentityProvider` trait in `crates/skotos-auth`: External Auth Provider (WorkOS recommended) for Cloud SaaS + first-party self-hosted adapter (OIDC / local Argon2id)**
2. Hard-wired external auth vendor across all deployments
3. Custom SAML/SCIM built from scratch before Phase 1 launch

## Decision Outcome

### 1. PostgreSQL 16+ Hybrid Relational + Versioned `JSONB`

Chosen option: **PostgreSQL 16+** as the sole Control Plane database:

- **Normalized relational tables** for tenant isolation, identity mapping, RBAC, keys, and workflows:
  - `organizations`, `projects`, `environments`, `environment_keys`
  - `members`, `teams`, `team_memberships`, `roles`, `api_tokens`
  - `flags` (project-level metadata, variations, tags, archive status)
  - `approval_requests`, `scheduled_changes`, `workflow_runs`, `webhooks`, `flag_triggers`
- **Versioned `JSONB` documents** for runtime targeting rules:
  - `flag_env_configs` (`flag_id`, `env_id`, `version`, `on`, `config_jsonb`, `updated_at`)
  - `flag_env_config_revisions` (append-only version history enabling instant version restore and structured diffs)
  - `segments` (`env_id`, `key`, `version`, `kind`, `segment_jsonb`)
- **Transactional Outbox & Audit Trail (Single ACID Transaction)**:
  - Every Control Plane mutation increments `version`, writes `flag_env_configs` + `flag_env_config_revisions`, appends to partitioned `audit_logs`, and inserts an `outbox_events` row atomically inside one PostgreSQL transaction.
- **Postgres-Backed Scheduler**:
  - Scheduled flag/segment changes and workflow delays poll `scheduled_changes` using `SELECT ... FOR UPDATE SKIP LOCKED` — eliminating the need for a separate workflow/cron cluster in Phases 1–3.

### 2. Pluggable Dual-Path Authentication (`crates/skotos-auth`)

Chosen option: **External Auth Provider for Cloud SaaS + Dedicated Self-Hosted Auth Adapter** behind a uniform internal `IdentityProvider` trait:

- **Cloud SaaS Adapter** (e.g., **WorkOS AuthKit + SSO + Directory Sync**, or Auth0/Clerk):
  - Handles user login, MFA, email verification, enterprise SAML/OIDC SSO, and SCIM provisioning with rapid time-to-market.
  - Maps external `subject_id` / `org_id` to internal Skotos `member_id` and `organization_id` records.
- **Self-Hosted Adapter** (for Docker Compose / Enterprise VPC):
  - Supports customer-provided OIDC/SAML IdP (Okta, Entra ID, Keycloak) plus local Argon2id email/password with secure `HttpOnly` sessions — zero external SaaS phone-home dependency.
- **Unified Authorization Layer**:
  - Once verified at the HTTP middleware boundary, both adapters yield the exact same `AuthenticatedPrincipal { member_id, org_id, roles, scopes }`. All RBAC, custom policies, approvals, and audit logging operate purely on Skotos internal identities.

### Positive Consequences

- One primary database handles flags, segments, RBAC, schedules, approvals, outbox, and audit logs cleanly in both SaaS and self-hosted modes.
- Zero dual-write bugs between flag edits, audit trail, and streaming propagation.
- Cloud SaaS gets production-grade auth + enterprise SSO/SCIM quickly without blocking Phase 1, while self-hosted customers remain 100% self-contained.

### Negative Consequences / Tradeoffs

- Maintaining two auth adapters (SaaS external vs self-hosted) adds a small integration surface.
  - _Mitigation_: Keep the `IdentityProvider` trait narrow (token/session verification + member sync webhook/endpoint) so 99% of authz code is shared.
- `JSONB` rules require strict application-level validation before write.
  - _Mitigation_: Strongly typed Rust domain structs (`crates/skotos-domain`) with `serde` + DAG cycle and schema validation run prior to every DB write.

## Links & References

- [Domain Model](../../design/domain-model.md)
- [Security & Multi-Tenancy](../security-and-tenancy.md)
