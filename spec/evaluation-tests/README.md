# Skotos Golden Evaluation Test Suite

Language-agnostic conformance suite validating deterministic flag evaluation, SHA-1 rollout bucketing, clause operators, segment precedence, prerequisite cycle guards, typed getters, and private attribute redaction across every Skotos engine implementation (`crates/skotos-eval`, Node/TS SDK, Browser/React SDK, Go SDK, and future SDKs).

## Suite Files

| File | Coverage |
| --- | --- |
| `basic.json` | Kill switch (`OFF`), `offVariation`, individual targets, rule order, fallthrough, boolean/string/number/JSON flags |
| `operators.json` | All 15 canonical operators, SemVer shorthand & build-metadata rules, RFC 3339/epoch ms dates, RE2 regex, array-OR, missing-attr non-negation, RFC 6901 JSON Pointers |
| `rollouts.json` | Exact 60-bit integer basis-point SHA-1 bucketing, 1-basis-point boundaries, `bucketBy`, `seed`, `_meta.secondary`, Experiment `inExperiment` & `untracked`, missing/unhashable fallback |
| `multi-context.json` | Single & multi-kind contexts (`user`, `organization`, `device`, custom), cross-kind AND clauses |
| `prerequisites.json` | Passing/failing/off/missing prerequisites, deep chains, diamond DAGs, self/2-node/3-node cycles |
| `segments.json` | `included` > `excluded` > `rules` precedence, non-user context targets, segment rollouts, missing segment negation, diamond DAGs, cyclic segments |
| `errors.json` | `FLAG_NOT_FOUND`, `USER_NOT_SPECIFIED`, `WRONG_TYPE`, `MALFORMED_FLAG` |
| `redaction.json` | Global & per-context private attributes, nested JSON Pointer redaction, `allAttributesPrivate`, protected `kind`/`key`/`anonymous` fields |
