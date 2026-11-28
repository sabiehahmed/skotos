"""Canonical 124/124 golden evaluation test suite + client, migration & OpenFeature tests."""

from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

SDK_ROOT = Path(__file__).resolve().parent.parent
if str(SDK_ROOT) not in sys.path:
    sys.path.insert(0, str(SDK_ROOT))

from skotos_sdk import (  # noqa: E402
    BigSegmentSnapshot,
    InMemoryEvalStore,
    MigrationHelper,
    SkotosClient,
    SkotosConfig,
    SkotosOpenFeatureProvider,
    TestData,
    big_segment_key_hash,
    bool_variation,
    evaluate,
    number_variation,
    redact_context,
    string_variation,
)

SPEC_DIR = SDK_ROOT.parent.parent / "spec" / "evaluation-tests"
SUITES = [
    "basic.json",
    "operators.json",
    "rollouts.json",
    "multi-context.json",
    "prerequisites.json",
    "segments.json",
    "errors.json",
    "redaction.json",
]


class PythonSdkGoldenSuiteTests(unittest.TestCase):
    def test_124_canonical_golden_vectors(self) -> None:
        total = 0
        for filename in SUITES:
            suite = json.loads((SPEC_DIR / filename).read_text(encoding="utf-8"))
            suite_name = suite["suite"]
            suite_kind = suite.get("kind")
            base_flags = suite.get("flags") or {}
            base_segments = suite.get("segments") or {}

            for tc in suite["tests"]:
                total += 1
                ctx = tc["context"]
                expected = tc["expected"]

                if suite_kind == "redaction":
                    actual = redact_context(ctx, tc.get("redactionConfig") or {})
                    self.assertEqual(
                        actual,
                        expected,
                        f"[{suite_name}] {tc['name']} mismatch",
                    )
                    continue

                flags = {**base_flags, **(tc.get("flags") or {})}
                segments = {**base_segments, **(tc.get("segments") or {})}
                store = InMemoryEvalStore(flags=flags, segments=segments)
                flag_key = tc.get("flagKey") or ""
                getter = tc.get("getter")
                default_value = tc.get("defaultValue")

                if getter == "bool":
                    actual = bool_variation(store, flag_key, ctx, bool(default_value))
                elif getter == "string":
                    actual = string_variation(store, flag_key, ctx, str(default_value or ""))
                elif getter == "number":
                    actual = number_variation(
                        store, flag_key, ctx, default_value if default_value is not None else 0
                    )
                else:
                    actual = evaluate(store, flag_key, ctx, default_value)

                self.assertEqual(
                    json.loads(json.dumps(actual)),
                    expected,
                    f"[{suite_name}] {tc['name']} mismatch",
                )

        self.assertGreaterEqual(total, 124)

    def test_big_segment_openfeature_and_migration_helper(self) -> None:
        vip_hash = big_segment_key_hash("user", "vip-py-1")
        td = (
            TestData()
            .update_flag(
                {
                    "key": "vip-flag",
                    "version": 2,
                    "on": True,
                    "salt": "salt-py",
                    "variations": [True, False],
                    "offVariation": 1,
                    "rules": [
                        {
                            "id": "r-big",
                            "clauses": [
                                {
                                    "contextKind": "user",
                                    "attribute": "key",
                                    "op": "segmentMatch",
                                    "values": ["warehouse-vips"],
                                }
                            ],
                            "variation": 0,
                        }
                    ],
                    "fallthrough": {"variation": 1},
                }
            )
            .update_segment(
                {
                    "key": "warehouse-vips",
                    "unbounded": True,
                    "unboundedContextKind": "user",
                    "generation": 1,
                }
            )
            .update_big_segment(
                BigSegmentSnapshot(
                    segment_key="warehouse-vips",
                    context_kind="user",
                    generation=1,
                    included_hashes=[vip_hash],
                )
            )
            .flag_string("db-migration", "shadow")
        )

        client = SkotosClient(SkotosConfig(offline=True), test_data=td)
        self.assertTrue(client.is_initialized())
        self.assertTrue(client.bool_variation("vip-flag", {"kind": "user", "key": "vip-py-1"}, False))
        self.assertFalse(client.bool_variation("vip-flag", {"kind": "user", "key": "other"}, False))

        provider = SkotosOpenFeatureProvider(client)
        res = provider.resolve_boolean_details("vip-flag", False, {"targetingKey": "vip-py-1"})
        self.assertTrue(res.value)
        self.assertEqual(res.variant, "0")
        self.assertEqual(res.reason, "TARGETING_MATCH")

        # Shadow-stage migration read executes both old and new and records consistency telemetry
        helper: MigrationHelper[dict[str, int]] = MigrationHelper(
            client=client,
            flag_key="db-migration",
            read_old=lambda: {"balance": 42},
            read_new=lambda: {"balance": 42},
        )
        m_res = helper.read({"kind": "user", "key": "u-1"})
        self.assertEqual(m_res.stage, "shadow")
        self.assertEqual(m_res.origin, "old")
        self.assertTrue(m_res.consistent)
        self.assertEqual(m_res.value, {"balance": 42})

        events = client.pending_events()
        self.assertEqual(len(events), 1)
        self.assertEqual(events[0]["kind"], "migration_op")
        self.assertEqual(events[0]["stage"], "shadow")
        self.assertTrue(events[0]["consistent"])


if __name__ == "__main__":
    unittest.main()
