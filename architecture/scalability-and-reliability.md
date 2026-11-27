# Scalability & Reliability

Feature flag infrastructure sits on the critical path of customer applications. Skotos must be engineered so that **a Skotos outage never causes a customer outage**.

## Blast-Radius Isolation & Failure Modes

| Failure Scenario                                        | Expected Behavior                                                                                                                     | Mitigation                                                                                            |
| ------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| **Control Plane API / Primary DB down**                 | Delivery Plane continues serving existing cached snapshots and client evaluations; connected SDKs stay healthy.                       | Delivery Plane reads exclusively from Snapshot Store, never directly from Primary DB on the hot path. |
| **Pub/Sub fanout broker down**                          | New edits queue in the DB outbox; existing SSE connections remain open; polling SDKs still serve cached snapshots.                    | Outbox retries with backoff; full snapshot reconciliation on broker recovery.                         |
| **Delivery Edge node crash**                            | Connected SDKs keep evaluating locally from in-memory store and transparently reconnect with jitter to healthy edge nodes.            | Stateless edge nodes behind L4/L7 load balancer; pre-gzipped snapshot cache prevents DB load spikes.  |
| **Event Ingestion / OLAP store down**                   | Event ingestion returns `503` or sheds load; SDKs buffer up to cap and drop telemetry silently without affecting `variation()` calls. | Strictly isolated service & bounded queues; zero shared thread/connection pools with Delivery Plane.  |
| **Customer app cold-starts during total Skotos outage** | App falls back to Relay Proxy, Redis persistent store, file bootstrap, or code-level default values within `initTimeout`.             | Non-blocking SDK init contract + optional Relay Proxy in customer VPC.                                |

## Scaling Dimensions

1. **Concurrent SSE Connections**: Horizontally scale stateless Streaming Edge nodes; each node handles tens of thousands of idle SSE connections (low per-connection memory footprint, tuned OS file descriptors and TCP keepalives).
2. **Client-Side Evaluation RPS**: Cache immutable/ETag-keyed evaluated payloads at edge/CDN for common anonymous contexts; horizontally scale stateless eval workers holding in-memory environment rulesets.
3. **Rule Payload Size**: Keep standard environments under a few hundred KB by separating large static targeting lists (`100k+` keys) into **Big Segments** evaluated via fast key-value lookups rather than streamed over SSE.
4. **Telemetry Ingestion Volume**: Rely on SDK-side **summary aggregation** (collapsing 1,000,000 evaluations of `flag-a` over 5s into a single counter row) before HTTP ingestion.

## Observability & SLOs (Design Targets)

- **Key Metrics**:
  - `propagation_latency_ms` (Control Plane commit timestamp -> Delivery Edge emit timestamp)
  - `active_sse_connections` (by environment & edge node)
  - `snapshot_cache_hit_ratio`
  - `outbox_lag_seconds`
  - `event_ingest_dropped_total`
- **Synthetic Canary**: Continuous synthetic probe toggles a dedicated canary flag every minute and measures end-to-end propagation latency to a canary SDK instance.
