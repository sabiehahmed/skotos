"""Skotos Python Server-Side SDK (zero third-party runtime dependencies).

Provides bit-exact SHA-1 60-bit flag evaluation matching `crates/skotos-eval`,
streaming/polling Delivery Edge integration, 2/4/6-stage Migration Flag helpers
with consistency/latency telemetry, and an OpenFeature-compatible provider.
"""

from .client import MigrationHelper, MigrationResult, SkotosClient, SkotosConfig, TestData
from .evaluator import (
    BigSegmentSnapshot,
    EvalStore,
    InMemoryEvalStore,
    big_segment_key_hash,
    bool_variation,
    check_big_segment_snapshot,
    evaluate,
    get_context_kind,
    get_context_kinds,
    hash_to_bucket,
    number_variation,
    parse_attribute_ref,
    redact_context,
    string_variation,
    validate_context,
)
from .openfeature import OpenFeatureResolutionDetails, SkotosOpenFeatureProvider

__all__ = [
    "BigSegmentSnapshot",
    "EvalStore",
    "InMemoryEvalStore",
    "MigrationHelper",
    "MigrationResult",
    "OpenFeatureResolutionDetails",
    "SkotosClient",
    "SkotosConfig",
    "SkotosOpenFeatureProvider",
    "TestData",
    "big_segment_key_hash",
    "bool_variation",
    "check_big_segment_snapshot",
    "evaluate",
    "get_context_kind",
    "get_context_kinds",
    "hash_to_bucket",
    "number_variation",
    "parse_attribute_ref",
    "redact_context",
    "string_variation",
    "validate_context",
]
