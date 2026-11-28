package dev.skotos.sdk;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/**
 * Verifies 124/124 canonical golden evaluation vectors plus Big Segment & OpenFeature parity.
 */
public final class GoldenSuiteTest {
    private static final String[] SUITES = {
        "basic.json",
        "operators.json",
        "rollouts.json",
        "multi-context.json",
        "prerequisites.json",
        "segments.json",
        "errors.json",
        "redaction.json"
    };

    @SuppressWarnings("unchecked")
    public static void main(String[] args) throws Exception {
        Path specDir = Path.of("spec/evaluation-tests").toAbsolutePath();
        int total = 0;
        int failures = 0;

        for (String file : SUITES) {
            String raw = Files.readString(specDir.resolve(file));
            Map<String, Object> suite = Json.parseObject(raw);
            String suiteName = String.valueOf(suite.get("suite"));
            String kind = suite.get("kind") instanceof String s ? s : null;
            Map<String, Map<String, Object>> baseFlags = toNestedMap(suite.get("flags"));
            Map<String, Map<String, Object>> baseSegments = toNestedMap(suite.get("segments"));

            List<Object> tests = (List<Object>) suite.get("tests");
            for (Object tcObj : tests) {
                total++;
                Map<String, Object> tc = (Map<String, Object>) tcObj;
                String name = String.valueOf(tc.get("name"));
                Object ctx = tc.get("context");
                Object expected = tc.get("expected");
                Object actual;

                if ("redaction".equals(kind)) {
                    Map<String, Object> cfg = (Map<String, Object>) tc.get("redactionConfig");
                    actual = Evaluator.redactContext((Map<String, Object>) ctx, cfg);
                } else {
                    Map<String, Map<String, Object>> flags = new HashMap<>(baseFlags);
                    flags.putAll(toNestedMap(tc.get("flags")));
                    Map<String, Map<String, Object>> segments = new HashMap<>(baseSegments);
                    segments.putAll(toNestedMap(tc.get("segments")));
                    Evaluator.InMemoryEvalStore store = new Evaluator.InMemoryEvalStore(flags, segments, null);

                    String flagKey = tc.get("flagKey") instanceof String s ? s : "";
                    String getter = tc.get("getter") instanceof String s ? s : null;
                    Object def = tc.get("defaultValue");

                    if ("bool".equals(getter)) {
                        actual = Evaluator.boolVariation(store, flagKey, ctx, Boolean.TRUE.equals(def));
                    } else if ("string".equals(getter)) {
                        actual = Evaluator.stringVariation(store, flagKey, ctx, def != null ? String.valueOf(def) : "");
                    } else if ("number".equals(getter)) {
                        actual = Evaluator.numberVariation(store, flagKey, ctx, def instanceof Number n ? n : 0);
                    } else {
                        actual = Evaluator.evaluate(store, flagKey, ctx, def);
                    }
                }

                if (!Json.deepEquals(actual, expected)) {
                    failures++;
                    System.err.println("FAIL [" + suiteName + "] " + name);
                    System.err.println("  expected: " + Json.stringify(expected));
                    System.err.println("  actual:   " + Json.stringify(actual));
                }
            }
        }

        if (total < 124) {
            throw new AssertionError("Expected >= 124 golden vectors, ran " + total);
        }
        if (failures > 0) {
            throw new AssertionError("Golden suite failed with " + failures + " / " + total + " failures");
        }

        // Verify Big Segment FNV-1a snapshot + SkotosClient + OpenFeature provider
        long vipHash = Evaluator.bigSegmentKeyHash("user", "vip-java-1");
        String snapJson = """
            {
              "revision": 7,
              "flags": {
                "vip-feature": {
                  "key": "vip-feature",
                  "version": 1,
                  "on": true,
                  "salt": "salt-java",
                  "variations": [true, false],
                  "offVariation": 1,
                  "rules": [
                    {
                      "id": "r1",
                      "clauses": [
                        {
                          "contextKind": "user",
                          "attribute": "key",
                          "op": "segmentMatch",
                          "values": ["java-vips"]
                        }
                      ],
                      "variation": 0
                    }
                  ],
                  "fallthrough": { "variation": 1 }
                }
              },
              "segments": {
                "java-vips": {
                  "key": "java-vips",
                  "unbounded": true,
                  "unboundedContextKind": "user",
                  "generation": 1
                }
              },
              "bigSegments": {
                "java-vips": {
                  "segmentKey": "java-vips",
                  "contextKind": "user",
                  "generation": 1,
                  "includedHashes": ["%s"],
                  "excludedHashes": []
                }
              }
            }
            """.formatted(Long.toUnsignedString(vipHash));

        SkotosClient client = SkotosClient.fromSnapshot(Json.parseObject(snapJson));
        if (!client.isInitialized() || client.getRevision() != 7) {
            throw new AssertionError("Client failed to initialize from snapshot");
        }
        if (!client.boolVariation("vip-feature", Map.of("kind", "user", "key", "vip-java-1"), false)) {
            throw new AssertionError("Expected Big Segment VIP match");
        }
        if (client.boolVariation("vip-feature", Map.of("kind", "user", "key", "other"), false)) {
            throw new AssertionError("Expected non-VIP to get false");
        }

        SkotosOpenFeatureProvider provider = new SkotosOpenFeatureProvider(client);
        var res = provider.resolveBooleanEvaluation("vip-feature", false, Map.of("targetingKey", "vip-java-1"));
        if (!Boolean.TRUE.equals(res.value()) || !"0".equals(res.variant()) || !"TARGETING_MATCH".equals(res.reason())) {
            throw new AssertionError("Unexpected OpenFeature resolution: " + res);
        }

        System.out.println("PASS: Java SDK verified " + total + "/" + total + " golden vectors + Big Segments + OpenFeature");
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Map<String, Object>> toNestedMap(Object obj) {
        Map<String, Map<String, Object>> out = new HashMap<>();
        if (obj instanceof Map<?, ?> m) {
            for (Map.Entry<?, ?> e : m.entrySet()) {
                out.put(String.valueOf(e.getKey()), (Map<String, Object>) e.getValue());
            }
        }
        return out;
    }
}
