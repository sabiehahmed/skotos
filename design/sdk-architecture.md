# SDK Architecture

Skotos SDKs are designed around two core principles: **zero-latency local reads** (on servers) and **graceful resilience** (application code must never crash or block indefinitely if Skotos is unreachable).

## Server-Side vs Client-Side SDKs

| Aspect                    | Server-Side SDK (Node, Go, Python, Java, Rust)           | Client-Side / Mobile SDK (Browser, React, iOS, Android)                 |
| ------------------------- | -------------------------------------------------------- | ----------------------------------------------------------------------- |
| **Credential**            | Secret `SDK Key`                                         | Public `Client-Side ID` or `Mobile Key`                                 |
| **Context cardinality**   | Evaluates for millions of distinct contexts per process  | Bound to one active context at a time (`identify()` switches context)   |
| **Data received**         | Full environment ruleset (flags + segments)              | Pre-evaluated flag values only (for flags marked client-side available) |
| **Where evaluation runs** | Locally in-process (`< 1 ms`)                            | On Skotos Delivery Plane / Relay Proxy; cached locally in SDK           |
| **Persistence**           | In-memory map (default) or external Redis/DynamoDB store | In-memory + optional `localStorage` / SQLite bootstrap cache            |

## Server-Side SDK Internal Components

1. **Data Source (Streaming / Polling)**
   - Connects via SSE (`/stream/v1/all`) with jittered exponential backoff on disconnects.
   - Falls back to polling if configured; respects `Retry-After` and terminal auth errors (`401`/`403` stop retrying to prevent storms).
2. **Feature Store**
   - Thread-safe version-checked store (`put` replaces all, `patch` / `delete` apply only if `incoming.version > existing.version`).
   - Pluggable interface supporting in-memory (default), Redis, or file-based offline fixtures.
3. **Evaluator**
   - Pure, allocation-light implementation of [`evaluation-engine.md`](evaluation-engine.md).
   - Validates against the shared golden evaluation test suite.
4. **Event Processor**
   - Bounded in-memory ring buffer; drops oldest/overflow events rather than growing memory unboundedly.
   - Summarizes flag evaluations locally into compact `summary` buckets flushed every `N` seconds (default 5s) with gzip compression.
   - Enforces **private attribute redaction** before serialization.

## Reliability & Developer Ergonomics

- **Non-blocking initialization**: `waitForInitialization(timeout)` resolves on initial `put` or timeout; never hangs app boot forever.
- **Safe fallbacks**: Every evaluation call requires a code-level `defaultValue` returned if uninitialized, flag missing, or type mismatch occurs.
- **Offline & Test Data Modes**: First-class in-memory mock data source (`TestData`) so customer unit/integration tests never hit the network.
- **Flag Change Listeners**: Pub/sub hooks (`on('change:flag-key', callback)`) for reacting to live config changes.

## Open Questions for Planning

- Which exact SDKs will we build in Phase 2? (Recommended starter pair: **TypeScript/Node.js Server SDK** + **TypeScript/React Browser SDK**, plus **Go Server SDK** if the backend is in Go.)
- Should we adopt **OpenFeature** provider compatibility alongside native Skotos SDK APIs?
