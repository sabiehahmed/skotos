# Security & Multi-Tenancy (Finalized Architecture)

## Dual-Path Authentication Architecture (`crates/skotos-auth`)

Per [ADR-0002](adrs/0002-primary-datastore-and-schema.md), authentication is abstracted behind a narrow Rust `IdentityProvider` trait so Cloud SaaS and self-hosted enterprise VPC deployments share **100% of authorization, RBAC, and tenant-isolation logic**:

| Deployment Mode       | Auth Adapter                                                                              | Capabilities                                                                                                                                                 |
| --------------------- | ----------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Cloud SaaS**        | External Auth Provider (e.g., **WorkOS AuthKit + SSO + Directory Sync**, Auth0, or Clerk) | Hosted login, MFA, social/OIDC login, Enterprise SAML 2.0 SSO, SCIM user/team provisioning                                                                   |
| **Self-Hosted / VPC** | First-Party Self-Hosted Adapter                                                           | Local Argon2id email/password + `HttpOnly` signed sessions, plus direct customer OIDC/SAML IdP (Okta, Entra ID, Keycloak) with zero external SaaS dependency |

Both adapters validate incoming dashboard requests and resolve a uniform internal `AuthenticatedPrincipal { member_id, org_id, team_ids, role_ids, token_scopes }`.

## Credential & Key Isolation Model

Every environment issues distinct credentials with strict least-privilege separation:

| Credential           | Prefix       | Audience                                      | Capabilities                                                                       | Storage & Rotation                                                                                                                              |
| -------------------- | ------------ | --------------------------------------------- | ---------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| **SDK Key**          | `sk_sdk_...` | Server-side SDKs & `skotos-relay`             | Read full raw flag & segment ruleset for one environment; submit events            | Stored hashed (SHA-256 + prefix lookup) in `environment_keys`; supports zero-downtime key rotation with dual-active grace window (`expires_at`) |
| **Mobile Key**       | `sk_mob_...` | Native mobile apps                            | Request server-evaluated flag values for a context; submit events                  | Never returns raw targeting rules or segment keys                                                                                               |
| **Client-Side ID**   | `sk_cid_...` | Public browser JS                             | Request server-evaluated values ONLY for flags opted into `clientSideAvailability` | Public non-secret environment identifier; strict CORS + per-env rate limiting                                                                   |
| **API Access Token** | `sk_api_...` | CI/CD, `skotos-cli`, Terraform, MCP           | Control Plane CRUD scoped to explicit RBAC role/policies                           | Cryptographic SHA-256 hash stored in `api_tokens`; plaintext shown once at creation                                                             |
| **Flag Trigger URL** | `sk_trg_...` | External APM tools (Datadog, PagerDuty, etc.) | Narrow single-action toggle (`turnFlagOn` / `turnFlagOff`) on one flag/env         | Unguessable high-entropy token + per-trigger rate limit                                                                                         |

## Multi-Tenant Isolation

- Every PostgreSQL query in `crates/skotos-storage` explicitly binds `org_id` from `AuthenticatedPrincipal`.
- Delivery Plane caches, R2 object paths (`snapshots/{env_id}/...`), and NATS JetStream subjects (`skotos.env.{env_id}.events`) are keyed by opaque UUID `env_id`, never guessable slugs alone.
- `services/delivery-edge` and `services/event-ingest` enforce per-environment rate limits, body-size caps, and connection quotas to prevent noisy-neighbor impact.

## Authorization (RBAC)

- **Built-in Roles (Phase 1–2)**:
  - `Owner`: Organization administration, billing, SSO/SCIM config, member management, destructive actions.
  - `Admin`: Full management of projects, environments, flags, segments, tokens, webhooks, and team membership.
  - `Writer`: Create/edit flags and segments (subject to environment protection rules and mandatory approvals).
  - `Reader`: Read-only access to flags, targeting rules, and audit logs.
- **Custom Policy Engine (Phase 3)**:
  - Resource-path allow/deny policy engine (`proj/{project}:env/{env}:flag/{flag}`) matching explicit actions (`createFlag`, `updateOn`, `updateRules`, `approveChange`, `rotateSdkKey`, `viewAuditLog`), evaluated in-process inside `crates/skotos-auth` with `deny` overriding `allow`.

## Privacy, PII & Integrity

1. **Private Attributes**: Contexts mark sensitive attributes globally (SDK config) or per-context (`_meta.privateAttributes`). They participate in local evaluation and are stripped inside every SDK before telemetry leaves the process.
2. **Client-Side Rule Secrecy**: Browser/mobile SDKs never receive targeting rules, email lists, or segment definitions — only evaluated flag outputs via `evalx`.
3. **Immutable Audit Trail**: Partitioned append-only `audit_logs` written inside the same ACID transaction as every mutation, capturing `actor_id`, `actor_type`, `ip`, `comment`, semantic instruction list, and before/after diffs.
4. **Signed Outbound Webhooks**: Every webhook payload is signed via `HMAC-SHA256` (`X-Skotos-Signature` + `X-Skotos-Timestamp`) to prevent tampering and replay attacks.
