# ADR-0004: Multi-SDK Evaluation Engine & Golden Test Suite Strategy

- **Status:** Accepted
- **Date:** 2026-11-27
- **Deciders:** Skotos Core Team

## Context and Problem Statement

LaunchDarkly's core moat is broad, reliable SDK coverage where every server-side SDK evaluates flags locally in `< 1 ms` and produces **byte-for-byte identical targeting and rollout results** across every language. We need an SDK architecture that maximizes customer adoption (zero painful native build dependencies), keeps maintenance cost manageable, and guarantees 100% evaluation parity.

## Decision Drivers

- Frictionless installation for customers (`npm install`, `go get`, `cargo add`, `pip install`) with zero native compiler/CGO/WASM bundling pitfalls.
- Strict mathematical and semantic parity across all SDKs, the Delivery Edge, and the Relay Proxy.
- Low switching friction for teams migrating off LaunchDarkly (OpenFeature compliance).

## Considered Options

1. **Spec-driven pure idiomatic SDKs validated in CI by a shared Golden Evaluation Test Suite** (with `crates/skotos-eval` directly powering Rust backend services, Relay Proxy, and the Rust SDK)
2. Single Rust evaluation core compiled to C-ABI/FFI and WASM bindings wrapped by every SDK language

## Decision Outcome

Chosen option: **Pure idiomatic SDKs verified against a canonical Golden Evaluation Test Suite** (`spec/evaluation-tests/`):

### 1. Canonical Specification & Golden Test Suite

- Before writing any second SDK, we author a language-agnostic JSON fixture suite under `spec/evaluation-tests/` covering:
  - Flag ON/OFF, missing flags, wrong variation types, prerequisites & cycle detection.
  - Single contexts, multi-contexts, nested JSON pointer attribute refs, missing context kinds.
  - Every clause operator (`in`, string ops, RE2 regex `matches`, numeric, RFC 3339 date, semver, `segmentMatch`, negation, array attributes).
  - Deterministic SHA-1 percentage rollouts, custom `bucketBy`, custom seeds, sticky bucketing, and boundary bucket edges (`0`, `99999`, `100000`).
  - Exact `EvaluationReason` payloads and private attribute redaction fixtures.
- Every SDK repo/directory runs the **entire golden test suite in CI** as a mandatory merge gate.

### 2. Shared Rust Core Where Native

- `crates/skotos-eval` is the canonical reference implementation used natively by:
  - `services/control-api` (UI "Test Context" rule simulator)
  - `services/delivery-edge` (client-side `evalx` evaluation)
  - `services/relay-proxy` (local evaluation & client-side proxying)
  - `sdks/rust-server` (zero duplication)

### 3. Phased SDK & OpenFeature Rollout

- **Phase 2 (Initial Launch Suite)**:
  - `@skotos/node-server-sdk` (TypeScript/Node.js Server SDK) + `@skotos/openfeature-node-provider`
  - `@skotos/browser-sdk` + `@skotos/react-sdk` (Browser/React Client SDK) + `@skotos/openfeature-web-provider`
  - `skotos-go-sdk` (Go Server SDK) + OpenFeature Go provider
  - `skotos-server-sdk` (Rust Server SDK) + OpenFeature Rust provider
- **Phase 3–4 (Ecosystem Expansion)**:
  - Python, Java, .NET, iOS (Swift), Android (Kotlin), React Native, Flutter, and Edge SDKs (Cloudflare Workers, Vercel Edge, Fastly Compute).

### Positive Consequences

- Zero native binary / musl / glibc / WASM bundler headaches for Node.js, Go, Browser, and mobile developers.
- Pure-language evaluation engines are small (~800–1,200 LOC), easy to debug, and achieve sub-millisecond local evaluation.
- Day-one OpenFeature providers make migrating from LaunchDarkly a 10-minute provider swap.

### Negative Consequences / Tradeoffs

- Adding a new clause operator requires updating each idiomatic SDK implementation.
  - _Mitigation_: Operators evolve slowly; adding a new golden test fixture immediately flags any unimplemented SDK in CI.

## Links & References

- [Evaluation Engine Semantics](../../design/evaluation-engine.md)
- [SDK Architecture](../../design/sdk-architecture.md)
