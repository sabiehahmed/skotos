# Security & Multi-Tenancy

## Credential & Key Isolation Model

Every environment issues distinct credentials with strict least-privilege separation:

| Credential           | Prefix (Example) | Audience                       | Capabilities                                                                                  | Storage                                                                                                                   |
| -------------------- | ---------------- | ------------------------------ | --------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| **SDK Key**          | `sk_sdk_...`     | Server-side SDKs & Relay Proxy | Read full raw flag & segment ruleset for one environment; submit events                       | Stored hashed (or encrypted with envelope KMS) + fast lookup index; supports zero-downtime key rotation with grace window |
| **Mobile Key**       | `sk_mob_...`     | Native mobile apps             | Request server-evaluated flag values for a context; submit events                             | Never returns raw targeting rules                                                                                         |
| **Client-Side ID**   | `sk_cid_...`     | Public browser JS              | Request server-evaluated values ONLY for flags explicitly opted into client-side availability | Public identifier (not a secret); strict CORS + rate limiting                                                             |
| **API Access Token** | `sk_api_...`     | CI/CD, CLI, Terraform          | Control Plane CRUD scoped to explicit RBAC role/policies                                      | Stored as cryptographic hash (SHA-256/Argon2); shown once at creation                                                     |

## Multi-Tenant Isolation

- Every Control Plane query is explicitly scoped by `org_id` (and verified against the authenticated principal's org membership).
- Delivery Plane caches and Pub/Sub channels are namespaced by `env_id` (UUID), never guessable slugs alone.
- Client-side endpoints enforce per-environment rate limits and payload size caps so one noisy tenant cannot degrade others.

## Authorization (RBAC)

- **Phase 1–2 Built-in Roles**:
  - `Owner`: Full organization, billing, SSO, member, and destructive controls.
  - `Admin`: Manage projects, environments, flags, tokens, and team access.
  - `Writer`: Create/edit flags and segments (subject to environment protection rules).
  - `Reader`: Read-only access to flags, targeting rules, and audit logs.
- **Phase 3+ Custom Policies (LaunchDarkly-style)**:
  - Resource ARN/path matcher (e.g., `proj/checkout:env/production:flag/*`) with explicit `allow` / `deny` actions (`updateOn`, `updateRules`, `createFlag`, `viewSdkKey`).

## Privacy & PII Protection

1. **Private Attributes**: Contexts may mark attributes as private globally (in SDK config) or per-context (`_meta.privateAttributes`).
   - Private attributes **are** used locally during rule evaluation.
   - Private attributes **are stripped** before sending `identify`, `feature`, or `debug` events over the network.
2. **Client-Side Rule Secrecy**: Browser SDKs never download targeting rules or segment lists, preventing exposure of beta customer emails, enterprise domain names, or unreleased feature logic.
3. **Audit Log Integrity**: Audit events are append-only, recorded transactionally with mutations, capturing actor ID, IP/user-agent, timestamp, comment, and exact before/after diffs.
4. **Outbound Webhooks**: Signed with `HMAC-SHA256` (`X-Skotos-Signature` + timestamp header) to prevent replay and tampering.
