# API Contracts (Finalized Specification)

Skotos splits its HTTP/streaming surface into **three independent APIs** with automated **OpenAPI 3.1** generation via Rust `utoipa` (`services/control-api` -> `spec/openapi/control-api.json` -> `packages/api-client`).

## 1. Control Plane API (`services/control-api`, `/api/v1/...`)

- **Auth**: Verified via `crates/skotos-auth` (`Authorization: Bearer <jwt-or-api-token>` or `HttpOnly` session cookie).
- **Optimistic Concurrency**: `If-Match` / `expectedVersion` prevents silent lost updates when two operators edit a flag simultaneously.
- **Core Resources**:
  - `GET/POST /api/v1/projects`, `GET/PATCH/DELETE /api/v1/projects/{projectKey}`
  - `GET/POST /api/v1/projects/{projectKey}/environments`
  - `POST /api/v1/projects/{projectKey}/environments/{envKey}/keys/rotate`
  - `GET/POST /api/v1/projects/{projectKey}/flags`
  - `GET/PATCH/DELETE /api/v1/projects/{projectKey}/flags/{flagKey}`
  - `POST /api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/simulate` (runs `crates/skotos-eval` against saved or unsaved draft rules for the UI "Test Context" panel)
  - `GET /api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/revisions` & `POST .../restore/{version}`
  - `GET/POST/PATCH/DELETE /api/v1/projects/{projectKey}/environments/{envKey}/segments`
  - `GET /api/v1/audit-log` (cursor-paginated)
  - `GET/POST/DELETE /api/v1/webhooks`

### Semantic Instruction Patch Protocol

Flag and segment mutations use **Semantic Instruction Patches** ([ADR-0001](../architecture/adrs/0001-tech-stack-and-monorepo.md)) rather than raw full-document overwrites. Every patch produces clean human-readable audit logs, safe approvals, and composable scheduled changes:

```json
{
  "environmentKey": "production",
  "expectedVersion": 13,
  "comment": "Ramp checkout rollout to 25% for US enterprise orgs",
  "instructions": [
    { "kind": "turnFlagOn" },
    {
      "kind": "addRule",
      "clauses": [
        {
          "contextKind": "organization",
          "attribute": "region",
          "op": "in",
          "values": ["us-east", "us-west"],
          "negate": false
        }
      ],
      "rollout": {
        "contextKind": "user",
        "bucketBy": "key",
        "variations": [
          { "variationId": "var_on", "weight": 25000 },
          { "variationId": "var_off", "weight": 75000 }
        ]
      }
    }
  ]
}
```

Core instruction kinds (`FlagPatchInstruction` enum in `crates/skotos-domain`):

- `turnFlagOn`, `turnFlagOff`, `updateOffVariation`
- `addTargets`, `removeTargets`
- `addRule`, `updateRule`, `removeRule`, `reorderRules`
- `updateFallthrough`
- `addPrerequisite`, `removePrerequisite`
- `addVariation`, `updateVariation`, `removeVariation`
- `updateClientSideAvailability`, `archiveFlag`, `restoreFlag`

## 2. Flag Delivery Plane API (`services/delivery-edge`)

Strictly read-only, DB-free on the hot path, and isolated from Control Plane outages.

### A. Server-Side SDK Streaming (`GET /stream/v1/all`)

- **Auth**: `Authorization: <sdk-key>`
- **Protocol**: `text/event-stream` (SSE)
- **Events**:
  - `put`: Full pre-compressed environment snapshot `{ "path": "/", "data": { "flags": {...}, "segments": {...} } }`.
  - `patch`: Single flag/segment delta `{ "path": "/flags/{key}", "data": { ... } }`.
  - `delete`: Monotonic tombstone `{ "path": "/flags/{key}", "version": 42 }`.
  - Heartbeat comment `: keepalive\n\n` every 30s.

### B. Server-Side SDK Polling (`GET /sdk/v1/latest-all`)

- **Auth**: `Authorization: <sdk-key>`
- **Caching**: Strong `ETag: "rev-{revision}"` + `If-None-Match` -> `304 Not Modified`; backed by R2/CDN pre-compressed snapshots.

### C. Client-Side & Mobile Evaluation (Ping-to-Re-evaluate)

- `GET /stream/v1/eval/{clientSideId}`: Lightweight SSE stream emitting `event: ping\ndata: {"rev":42}\n\n` only when a `clientSideAvailability = true` flag changes.
- `GET /sdk/v1/evalx/{clientSideId}/contexts/{base64urlContext}?rev=42&withReasons=false`: CDN-cacheable evaluated flag map.
- `REPORT /sdk/v1/evalx/{clientSideId}/context`: Evaluates large or sensitive contexts in the request body without exposing attributes in URLs.

## 3. Event Ingestion API (`services/event-ingest`)

- `POST /events/v1/bulk` (Server SDKs, gzipped batches, `X-Skotos-Payload-ID` idempotency, returns `202 Accepted`).
- `POST /events/v1/client/{clientSideId}` (Browser/Mobile SDKs, CORS-enabled, returns `202 Accepted`).
- `POST /otlp/v1/traces`, `/otlp/v1/logs`, `/otlp/v1/metrics` (Phase 4–5 OpenTelemetry ingestion).
- Event kinds: `summary` (5s local SDK rollups), `feature`, `debug`, `identify`, `custom`.
