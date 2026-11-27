use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;
use skotos_domain::{Context, EvalFlag, EvalSegment, EvaluationDetail};
use skotos_eval::{
    bool_variation, evaluate, json_variation, number_variation, redact_context, string_variation,
    InMemoryEvalStore, RedactionConfig,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoldenSuiteFile {
    suite: String,
    #[serde(default = "default_kind")]
    kind: String,
    #[serde(default)]
    flags: HashMap<String, EvalFlag>,
    #[serde(default)]
    segments: HashMap<String, EvalSegment>,
    tests: Vec<GoldenTestCase>,
}

fn default_kind() -> String {
    "evaluation".to_string()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoldenTestCase {
    name: String,
    #[serde(default)]
    flag_key: Option<String>,
    #[serde(default)]
    getter: Option<String>,
    #[serde(default)]
    default_value: Option<Value>,
    context: Context,
    #[serde(default)]
    flags: HashMap<String, EvalFlag>,
    #[serde(default)]
    segments: HashMap<String, EvalSegment>,
    #[serde(default)]
    redaction_config: Option<GoldenRedactionConfig>,
    expected: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoldenRedactionConfig {
    #[serde(default)]
    all_attributes_private: bool,
    #[serde(default)]
    global_private_attributes: Vec<String>,
}

#[test]
fn executes_all_golden_evaluation_suites() {
    let spec_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/evaluation-tests");

    let expected_suites = [
        "basic.json",
        "operators.json",
        "rollouts.json",
        "multi-context.json",
        "prerequisites.json",
        "segments.json",
        "errors.json",
        "redaction.json",
    ];

    let mut total_vectors = 0usize;

    for filename in expected_suites {
        let path = spec_dir.join(filename);
        let raw = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        let suite: GoldenSuiteFile = serde_json::from_str(&raw)
            .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()));

        for case in suite.tests {
            total_vectors += 1;

            if suite.kind == "redaction" {
                let cfg = case
                    .redaction_config
                    .map(|c| RedactionConfig {
                        all_attributes_private: c.all_attributes_private,
                        global_private_attributes: c.global_private_attributes,
                    })
                    .unwrap_or_default();
                let actual = redact_context(&case.context, &cfg);
                assert_eq!(
                    actual, case.expected,
                    "[suite={}] test '{}' failed",
                    suite.suite, case.name
                );
                continue;
            }

            let mut store = InMemoryEvalStore::new();
            for (k, f) in &suite.flags {
                store.flags.insert(k.clone(), f.clone());
            }
            for (k, s) in &suite.segments {
                store.segments.insert(k.clone(), s.clone());
            }
            for (k, f) in &case.flags {
                store.flags.insert(k.clone(), f.clone());
            }
            for (k, s) in &case.segments {
                store.segments.insert(k.clone(), s.clone());
            }

            let flag_key = case.flag_key.as_deref().unwrap_or_else(|| {
                panic!(
                    "[suite={}] test '{}' missing flagKey",
                    suite.suite, case.name
                )
            });
            let default_val = case.default_value.clone().unwrap_or(Value::Null);

            let actual: EvaluationDetail = match case.getter.as_deref().unwrap_or("any") {
                "bool" => bool_variation(
                    &store,
                    flag_key,
                    &case.context,
                    default_val.as_bool().unwrap_or(false),
                ),
                "string" => string_variation(
                    &store,
                    flag_key,
                    &case.context,
                    default_val.as_str().unwrap_or(""),
                ),
                "number" => number_variation(
                    &store,
                    flag_key,
                    &case.context,
                    default_val.as_f64().unwrap_or(0.0),
                ),
                "json" => json_variation(&store, flag_key, &case.context, default_val),
                _ => evaluate(&store, flag_key, &case.context, default_val),
            };

            let expected: EvaluationDetail = serde_json::from_value(case.expected.clone())
                .unwrap_or_else(|e| {
                    panic!(
                        "[suite={}] test '{}' invalid expected EvaluationDetail: {e}",
                        suite.suite, case.name
                    )
                });

            assert_eq!(
                actual.variation_index, expected.variation_index,
                "[suite={}] test '{}' variationIndex mismatch",
                suite.suite, case.name
            );
            assert_eq!(
                actual.reason, expected.reason,
                "[suite={}] test '{}' reason mismatch",
                suite.suite, case.name
            );
            assert!(
                json_values_equal(&actual.value, &expected.value),
                "[suite={}] test '{}' value mismatch: actual={:?}, expected={:?}",
                suite.suite,
                case.name,
                actual.value,
                expected.value
            );
        }
    }

    assert!(
        total_vectors >= 115,
        "expected at least 115 golden vectors per PROJECT.md, found {total_vectors}"
    );
}

fn json_values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(na), Value::Number(nb)) => match (na.as_f64(), nb.as_f64()) {
            (Some(fa), Some(fb)) => fa == fb,
            _ => na == nb,
        },
        (Value::Array(la), Value::Array(lb)) => {
            la.len() == lb.len()
                && la
                    .iter()
                    .zip(lb.iter())
                    .all(|(x, y)| json_values_equal(x, y))
        }
        (Value::Object(ma), Value::Object(mb)) => {
            ma.len() == mb.len()
                && ma
                    .iter()
                    .all(|(k, va)| mb.get(k).is_some_and(|vb| json_values_equal(va, vb)))
        }
        _ => a == b,
    }
}
