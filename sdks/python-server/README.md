# Skotos Python Server SDK

Pure Python 3.11+ server-side SDK for Skotos with zero external runtime dependencies.

- 124/124 canonical golden evaluation vector parity with `crates/skotos-eval`
- Big Segment 64-bit FNV-1a snapshot lookup
- 2/4/6-stage `MigrationHelper` with consistency and latency telemetry
- OpenFeature-compatible `SkotosOpenFeatureProvider`

## Running tests

```bash
python3 -m unittest discover -s sdks/python-server/tests -v
```
