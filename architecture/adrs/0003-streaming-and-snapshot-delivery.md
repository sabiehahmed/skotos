# ADR-0003: Real-Time Streaming Fanout, Snapshot Delivery & Client MAU Strategy

- **Status:** Accepted
- **Date:** 2026-11-27
- **Deciders:** Skotos Core Team

## Context and Problem Statement

LaunchDarkly's highest customer pricing friction comes from **Service Connections** (`~$10–$12/mo` per server replica) and **Client-Side MAU** (`~$8.33–$10/mo` per 1K MAU). Skotos must deliver `< 200 ms` p95 global flag update propagation, survive complete Control Plane outages without impacting customer apps, and handle reconnect storms — all at **pennies of marginal infrastructure cost**.

## Decision Drivers

- Sub-200ms end-to-end flag propagation from Control Plane commit to connected SDKs.
- Complete runtime decoupling: Delivery Edge must never query PostgreSQL on the hot read path.
- Minimal RAM per open SSE connection and minimal egress bandwidth cost.
- Easy clustering for Cloud SaaS and trivial single-container/Helm deployment for self-hosted VPCs.

## Considered Options

### Fanout Backbone & Snapshot Store

1. **NATS JetStream + Zero-Egress Object Storage (Cloudflare R2 / S3 / MinIO) + CDN**
2. Valkey/Redis Pub/Sub + Streams + S3/R2
3. PostgreSQL `LISTEN/NOTIFY` only
4. Kafka / Redpanda + Redis + S3/R2

### Client-Side Real-Time Streaming Model

1. **Ping-to-Re-evaluate (SSE sends tiny env-change `ping`; client fetches CDN/ETag-cached `evalx` payload)**
2. Direct per-context push (edge holds every connected browser/mobile context in RAM and re-evaluates on every change)

## Decision Outcome

### 1. NATS JetStream + Pre-Compressed R2/S3 Snapshots

Chosen option: **NATS JetStream + Cloudflare R2 (SaaS) / S3-compatible MinIO (self-hosted)**:

- **Outbox Publisher** (`control-api` worker):
  1. Consumes committed `outbox_events` from PostgreSQL.
  2. Rebuilds the full environment snapshot JSON (flags + standard segments), pre-compresses it (`gzip` and `zstd`), computes its `ETag` / revision hash, and uploads it atomically to R2/S3 (`snapshots/{env_id}/latest.json.gz` and client-side subset).
  3. Publishes the compact incremental event (`patch` / `delete` + `new_revision` + `client_side_affected` flag) onto NATS JetStream subject `skotos.env.{env_id}.events`.
- **Delivery Edge (`services/delivery-edge` in Rust/Tokio)**:
  - Maintains an in-memory LRU cache of active environment rulesets (both raw compressed bytes for instant `put` SSE handshakes and parsed `EnvironmentRuleset` structs for client-side `evalx`).
  - Opens **one NATS subscription per active `env_id`** regardless of how many SDK sockets connect to that node.
  - On SSE connect (`/stream/v1/all`), streams the pre-compressed `put` snapshot directly from memory/R2 (zero DB load, zero JSON re-serialization per client) and forwards subsequent `patch`/`delete` events.
  - On polling (`/sdk/v1/latest-all`), serves ETag-validated snapshots (`304 Not Modified` when unchanged), frontable by CDN for zero origin cost.

### 2. Client-Side Streaming: Ping-to-Re-evaluate

Chosen option: **Ping-to-Re-evaluate** for browser and mobile SDKs:

- Browser/mobile SSE connections (`/stream/v1/eval/{clientSideId}`) register only their `env_id` — **no per-user context is stored in edge RAM**.
- When any flag with `clientSideAvailability = true` changes in that environment, the Delivery Edge broadcasts a tiny SSE `ping` frame (`event: ping\ndata: {"rev":42}\n\n`).
- Connected client SDKs immediately re-fetch `GET /sdk/v1/evalx/{clientSideId}/contexts/{base64urlContext}?rev=42` (or `REPORT` for large/private contexts), which is evaluated in microseconds by stateless Rust edge workers and CDN-cached for common anonymous/public contexts.

### Positive Consequences

- **Ultra-low Service Connection COGS**: Rust/Tokio SSE nodes hold 100k–250k+ idle sockets per modest VM with minimal RAM; NATS JetStream runs in ~30–100 MB RAM with built-in leaf-node multi-region fanout.
- **Ultra-low Client-Side MAU COGS**: Ping-to-Re-evaluate eliminates per-user memory on edge nodes and keeps CDN cache hit rates high; R2 eliminates cloud egress charges on snapshot transfers.
- **Zero-downtime Control Plane isolation**: If PostgreSQL goes offline, Delivery Edge and R2 snapshots keep serving all existing and newly booting SDKs seamlessly.

### Negative Consequences / Tradeoffs

- Client-side flag updates require one extra HTTP RTT after the SSE `ping` (~20–60 ms added on browser clients).
  - _Mitigation_: Still well within `< 200 ms` total perceived update latency, and we can optimize high-churn enterprise apps via edge POP proximity and HTTP/2/3 multiplexing.
- Very large static segments (`100k+` keys) cannot be embedded in SSE snapshots.
  - _Mitigation_: Handled in Phase 3 via Big Segments (compressed Roaring Bitmaps / hashed sets in R2 + optional Redis/SQLite sync in Relay Proxy).

## Links & References

- [Flag Delivery Pipeline](../flag-delivery-pipeline.md)
- [Scalability & Reliability](../scalability-and-reliability.md)
