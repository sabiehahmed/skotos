# API Contracts

Skotos separates its HTTP/streaming APIs into **three distinct surfaces** with different authentication models, scaling profiles, and availability requirements.

## 1. Control Plane API (`/api/v1/...`)

Used by the Management Dashboard, CLI, Terraform provider, and CI/CD automation.

- **Auth**: User session / JWT (dashboard) or scoped API Access Token (`Authorization: Bearer sk_api_...`).
- **Consistency**: Strongly consistent reads/writes against the primary store; optimistic concurrency via `If-Match` / `version` headers where appropriate.
- **Core Endpoints (Draft)**:
  - `GET/POST /api/v1/projects`
  - `GET/PATCH/DELETE /api/v1/projects/{projectKey}`
  - `GET/POST /api/v1/projects/{projectKey}/environments`
  - `POST /api/v1/projects/{projectKey}/environments/{envKey}/sdk-key/rotate`
  - `GET/POST /api/v1/projects/{projectKey}/flags`
  - `GET/PATCH/DELETE /api/v1/projects/{projectKey}/flags/{flagKey}` (supports semantic JSON Patch or structured instruction patches so audit diffs are human-readable)
  - `GET/POST/PATCH/DELETE /api/v1/projects/{projectKey}/environments/{envKey}/segments`
  - `GET /api/v1/audit-log` (cursor-paginated, filterable by project, env, flag, actor, time range)
  - `GET/POST/DELETE /api/v1/webhooks`

## 2. Flag Delivery Plane API

Strictly read-only, cache-friendly, and decoupled from Control Plane writes.

### A. Server-Side SDK Streaming (`GET /stream/v1/all`)

- **Auth**: `Authorization: <sdk-key>`
- **Protocol**: `text/event-stream` (Server-Sent Events)
- **Event Types**:
  - `put`: Full environment snapshot `{ "path": "/", "data": { "flags": {...}, "segments": {...} } }` sent immediately on connect.
  - `patch`: Single flag or segment update `{ "path": "/flags/{key}", "data": { ... } }` or `{ "path": "/segments/{key}", "data": { ... } }`.
  - `delete`: Tombstone `{ "path": "/flags/{key}", "version": 42 }` so SDKs never re-apply older state.
- **Heartbeats**: Periodic SSE comment (`: keepalive\n\n`) every 30s to keep proxies/load balancers alive and allow dead-connection detection.

### B. Server-Side SDK Polling (`GET /sdk/v1/latest-all`)

- **Auth**: `Authorization: <sdk-key>`
- **Caching**: Returns `ETag` and honors `If-None-Match` -> `304 Not Modified`.

### C. Client-Side / Mobile Evaluation

Client SDKs must **never** receive raw targeting rules (which may leak customer names, beta cohorts, or private rules). Instead, the Delivery Plane evaluates flags server-side for the given context:

- `GET /sdk/v1/evalx/{clientSideId}/contexts/{base64urlContext}` (cacheable at CDN/edge for public contexts)
- `REPORT /sdk/v1/evalx/{clientSideId}/context` (JSON body for large or sensitive contexts without putting attributes in URLs)
- `GET /stream/v1/eval/{clientSideId}/contexts/{base64urlContext}` (SSE stream emitting `ping` or `put`/`patch` of evaluated results when relevant flags change)
- Optional query param `?withReasons=true` to include evaluation reasons.

## 3. Event Ingestion API (`POST /events/v1/...`)

High-throughput, fire-and-forget asynchronous ingestion from SDKs.

- **Auth**: SDK Key, Mobile Key, or Client-Side ID.
- **Endpoints**:
  - `POST /events/v1/bulk` (server-side SDKs, `Content-Encoding: gzip`, `X-Skotos-Payload-ID` for idempotency)
  - `POST /events/v1/client/{clientSideId}` (browser/mobile SDKs, CORS-enabled)
- **Event Kinds**:
  - `summary`: Aggregated evaluation counts per `(flagKey, version, variation, value, contextKinds)` over a flush window (dramatically reduces ingestion volume).
  - `feature` / `debug`: Individual detailed evaluations (only emitted when flag tracking or live debugging is enabled).
  - `identify`: Context discovery for autocomplete/insights in the dashboard.
  - `custom`: Conversion/metric events for experimentation.
- **Response**: `202 Accepted` immediately after schema/auth check and enqueue.

## Open Questions for Planning

- Should Control Plane mutations use RFC 6902 JSON Patch, semantic instruction patches (e.g., `turnFlagOn`, `addRule`, `updateFallthrough`), or both? (Semantic instructions make audit logs and approval diffs dramatically cleaner.)
- Should we author formal OpenAPI 3.1 specs under `design/openapi/` before coding handlers?
