use std::collections::HashMap;

use proptest::prelude::*;
use serde_json::json;
use skotos_domain::{
    detect_prerequisite_cycle, Context, EvalFlag, EvalPrerequisite, EvalRollout,
    EvalVariationOrRollout, EvalWeightedVariation, EvaluationErrorKind, EvaluationReason,
    RolloutKind, SingleContext,
};
use skotos_eval::{evaluate, hash_to_bucket, InMemoryEvalStore};

proptest! {
    #[test]
    fn bucketing_is_deterministic_and_within_basis_points(
        key in "[a-zA-Z0-9._-]{1,40}",
        salt in "[a-zA-Z0-9._-]{1,20}",
        user in "[a-zA-Z0-9._-]{1,40}"
    ) {
        let input = format!("{key}.{salt}.{user}");
        let b1 = hash_to_bucket(&input);
        let b2 = hash_to_bucket(&input);
        prop_assert_eq!(b1, b2);
        prop_assert!(b1 < 100_000);
    }

    #[test]
    fn rollout_never_panics_and_always_picks_valid_variation(
        w0 in 0u32..=100_000u32,
        user_key in "[a-zA-Z0-9._-]{1,32}"
    ) {
        let w1 = 100_000 - w0;
        let mut store = InMemoryEvalStore::new();
        store.insert_flag(EvalFlag {
            key: "prop-flag".to_string(),
            version: 1,
            on: true,
            salt: "prop-salt".to_string(),
            variations: vec![json!("a"), json!("b")],
            off_variation: Some(0),
            prerequisites: vec![],
            targets: vec![],
            rules: vec![],
            fallthrough: EvalVariationOrRollout {
                variation: None,
                rollout: Some(EvalRollout {
                    context_kind: "user".to_string(),
                    bucket_by: None,
                    seed: None,
                    kind: RolloutKind::Rollout,
                    variations: vec![
                        EvalWeightedVariation { variation: 0, weight: w0, untracked: false },
                        EvalWeightedVariation { variation: 1, weight: w1, untracked: false },
                    ],
                }),
            },
            client_side_availability: Default::default(),
            track_events: false,
            track_events_fallthrough: false,
        });

        let ctx = Context::single(SingleContext::new("user", user_key));
        let detail = evaluate(&store, "prop-flag", &ctx, json!("fallback"));
        prop_assert!(matches!(detail.variation_index, Some(0 | 1)));
        prop_assert_eq!(detail.reason, EvaluationReason::Fallthrough { in_experiment: false });
    }

    #[test]
    fn arbitrary_prerequisite_graphs_never_stack_overflow(
        edges in prop::collection::vec((0usize..6, 0usize..6), 0..14)
    ) {
        let mut store = InMemoryEvalStore::new();
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();

        for i in 0..6 {
            let key = format!("f{i}");
            let mut prereqs = Vec::new();
            let mut deps = Vec::new();
            for &(from, to) in &edges {
                if from == i && !deps.contains(&format!("f{to}")) {
                    deps.push(format!("f{to}"));
                    prereqs.push(EvalPrerequisite {
                        key: format!("f{to}"),
                        variation: 0,
                    });
                }
            }
            adj.insert(key.clone(), deps);
            store.insert_flag(EvalFlag {
                key,
                version: 1,
                on: true,
                salt: "s".to_string(),
                variations: vec![json!(true), json!(false)],
                off_variation: Some(1),
                prerequisites: prereqs,
                targets: vec![],
                rules: vec![],
                fallthrough: EvalVariationOrRollout {
                    variation: Some(0),
                    rollout: None,
                },
                client_side_availability: Default::default(),
                track_events: false,
                track_events_fallthrough: false,
            });
        }

        let ctx = Context::single(SingleContext::new("user", "u1"));
        let detail = evaluate(&store, "f0", &ctx, json!(false));
        let has_cycle = detect_prerequisite_cycle("f0", &adj).is_err();
        if has_cycle {
            prop_assert_eq!(
                detail.reason,
                EvaluationReason::Error {
                    error_kind: EvaluationErrorKind::MalformedFlag
                }
            );
        } else {
            prop_assert_eq!(detail.value, json!(true));
        }
    }
}

#[test]
fn rollout_distribution_is_statistically_uniform_over_10k_users() {
    let mut store = InMemoryEvalStore::new();
    store.insert_flag(EvalFlag {
        key: "dist-flag".to_string(),
        version: 1,
        on: true,
        salt: "dist-salt".to_string(),
        variations: vec![json!("a"), json!("b")],
        off_variation: Some(0),
        prerequisites: vec![],
        targets: vec![],
        rules: vec![],
        fallthrough: EvalVariationOrRollout {
            variation: None,
            rollout: Some(EvalRollout {
                context_kind: "user".to_string(),
                bucket_by: None,
                seed: None,
                kind: RolloutKind::Rollout,
                variations: vec![
                    EvalWeightedVariation {
                        variation: 0,
                        weight: 50_000,
                        untracked: false,
                    },
                    EvalWeightedVariation {
                        variation: 1,
                        weight: 50_000,
                        untracked: false,
                    },
                ],
            }),
        },
        client_side_availability: Default::default(),
        track_events: false,
        track_events_fallthrough: false,
    });

    let mut count_a = 0usize;
    for i in 0..10_000 {
        let ctx = Context::single(SingleContext::new("user", format!("user-{i}")));
        let detail = evaluate(&store, "dist-flag", &ctx, json!("fallback"));
        if detail.variation_index == Some(0) {
            count_a += 1;
        }
    }
    // Expect 50% ± 2.5% over 10,000 samples (well within 5 sigma)
    assert!(
        (4_750..=5_250).contains(&count_a),
        "expected ~5000 in bucket A, got {count_a}"
    );
}
