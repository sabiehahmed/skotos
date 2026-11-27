use serde_json::Value;
use skotos_domain::{
    Context, EvalFlag, EvalVariationOrRollout, EvaluationDetail, EvaluationErrorKind,
    EvaluationReason,
};

use crate::bucketing::evaluate_rollout;
use crate::operators::{eval_clause, InternalEvalError};
use crate::store::EvalStore;

/// Evaluates `flag_key` against `context`, returning an [`EvaluationDetail`] containing
/// the resolved JSON value, variation index, and structured [`EvaluationReason`].
pub fn evaluate<S: EvalStore>(
    store: &S,
    flag_key: &str,
    context: &Context,
    default_value: Value,
) -> EvaluationDetail {
    // Step 1: Validate context
    if context.validate().is_err() {
        return error_detail(default_value, EvaluationErrorKind::UserNotSpecified);
    }

    // Step 2: Lookup flag
    let Some(flag) = store.get_flag(flag_key) else {
        return error_detail(default_value, EvaluationErrorKind::FlagNotFound);
    };

    let mut prereq_stack = Vec::new();
    evaluate_flag_internal(store, flag, context, &default_value, &mut prereq_stack)
}

/// Evaluates a boolean flag; returns `default_value` with `ERROR(WRONG_TYPE)` if the
/// resolved variation is not a JSON boolean.
pub fn bool_variation<S: EvalStore>(
    store: &S,
    flag_key: &str,
    context: &Context,
    default_value: bool,
) -> EvaluationDetail {
    let default_json = Value::Bool(default_value);
    let detail = evaluate(store, flag_key, context, default_json.clone());
    if detail.variation_index.is_some() && !detail.value.is_boolean() {
        return error_detail(default_json, EvaluationErrorKind::WrongType);
    }
    detail
}

/// Evaluates a string flag; returns `default_value` with `ERROR(WRONG_TYPE)` on type mismatch.
pub fn string_variation<S: EvalStore>(
    store: &S,
    flag_key: &str,
    context: &Context,
    default_value: &str,
) -> EvaluationDetail {
    let default_json = Value::String(default_value.to_string());
    let detail = evaluate(store, flag_key, context, default_json.clone());
    if detail.variation_index.is_some() && !detail.value.is_string() {
        return error_detail(default_json, EvaluationErrorKind::WrongType);
    }
    detail
}

/// Evaluates a numeric flag; returns `default_value` with `ERROR(WRONG_TYPE)` on type mismatch.
pub fn number_variation<S: EvalStore>(
    store: &S,
    flag_key: &str,
    context: &Context,
    default_value: f64,
) -> EvaluationDetail {
    let default_json = serde_json::json!(default_value);
    let detail = evaluate(store, flag_key, context, default_json.clone());
    if detail.variation_index.is_some() && !detail.value.is_number() {
        return error_detail(default_json, EvaluationErrorKind::WrongType);
    }
    detail
}

/// Evaluates an arbitrary JSON flag (accepts any JSON value type).
pub fn json_variation<S: EvalStore>(
    store: &S,
    flag_key: &str,
    context: &Context,
    default_value: Value,
) -> EvaluationDetail {
    evaluate(store, flag_key, context, default_value)
}

fn evaluate_flag_internal<S: EvalStore>(
    store: &S,
    flag: &EvalFlag,
    context: &Context,
    default_value: &Value,
    prereq_stack: &mut Vec<String>,
) -> EvaluationDetail {
    // Step 3: Kill switch
    if !flag.on {
        return resolve_off_variation(flag, default_value, EvaluationReason::Off);
    }

    // Step 4: Prerequisites with diamond-safe push/pop cycle guard
    if prereq_stack.iter().any(|k| k == &flag.key) {
        return error_detail(default_value.clone(), EvaluationErrorKind::MalformedFlag);
    }
    prereq_stack.push(flag.key.clone());

    for prereq in &flag.prerequisites {
        if prereq_stack.iter().any(|k| k == &prereq.key) {
            prereq_stack.pop();
            return error_detail(default_value.clone(), EvaluationErrorKind::MalformedFlag);
        }

        let Some(prereq_flag) = store.get_flag(&prereq.key) else {
            prereq_stack.pop();
            return resolve_off_variation(
                flag,
                default_value,
                EvaluationReason::PrerequisiteFailed {
                    prerequisite_key: prereq.key.clone(),
                },
            );
        };

        if !prereq_flag.on {
            prereq_stack.pop();
            return resolve_off_variation(
                flag,
                default_value,
                EvaluationReason::PrerequisiteFailed {
                    prerequisite_key: prereq.key.clone(),
                },
            );
        }

        let prereq_detail =
            evaluate_flag_internal(store, prereq_flag, context, &Value::Null, prereq_stack);
        if matches!(
            prereq_detail.reason,
            EvaluationReason::Error {
                error_kind: EvaluationErrorKind::MalformedFlag
            }
        ) {
            prereq_stack.pop();
            return error_detail(default_value.clone(), EvaluationErrorKind::MalformedFlag);
        }

        if prereq_detail.variation_index != Some(prereq.variation) {
            prereq_stack.pop();
            return resolve_off_variation(
                flag,
                default_value,
                EvaluationReason::PrerequisiteFailed {
                    prerequisite_key: prereq.key.clone(),
                },
            );
        }
    }

    prereq_stack.pop();

    // Step 5: Individual targets
    for target in &flag.targets {
        if let Some(single) = context.get_kind(&target.context_kind) {
            if target.values.iter().any(|k| k == &single.key) {
                return resolve_variation_index(
                    flag,
                    target.variation,
                    default_value,
                    EvaluationReason::TargetMatch,
                );
            }
        }
    }

    // Step 6: Ordered targeting rules
    let mut segment_stack = Vec::new();
    for (rule_index, rule) in flag.rules.iter().enumerate() {
        let mut all_match = true;
        for clause in &rule.clauses {
            match eval_clause(store, clause, context, &mut segment_stack) {
                Ok(true) => {}
                Ok(false) => {
                    all_match = false;
                    break;
                }
                Err(InternalEvalError::MalformedFlag) => {
                    return error_detail(default_value.clone(), EvaluationErrorKind::MalformedFlag);
                }
            }
        }
        if all_match {
            return resolve_variation_or_rollout(
                flag,
                &rule.variation_or_rollout,
                context,
                default_value,
                |in_experiment| EvaluationReason::RuleMatch {
                    rule_index,
                    rule_id: rule.id.clone(),
                    in_experiment,
                },
            );
        }
    }

    // Step 7: Fallthrough
    resolve_variation_or_rollout(
        flag,
        &flag.fallthrough,
        context,
        default_value,
        |in_experiment| EvaluationReason::Fallthrough { in_experiment },
    )
}

fn resolve_off_variation(
    flag: &EvalFlag,
    default_value: &Value,
    reason: EvaluationReason,
) -> EvaluationDetail {
    match flag.off_variation {
        Some(idx) => resolve_variation_index(flag, idx, default_value, reason),
        None => EvaluationDetail {
            value: default_value.clone(),
            variation_index: None,
            reason,
        },
    }
}

fn resolve_variation_index(
    flag: &EvalFlag,
    index: usize,
    default_value: &Value,
    reason: EvaluationReason,
) -> EvaluationDetail {
    match flag.variations.get(index) {
        Some(val) => EvaluationDetail {
            value: val.clone(),
            variation_index: Some(index),
            reason,
        },
        None => error_detail(default_value.clone(), EvaluationErrorKind::MalformedFlag),
    }
}

fn resolve_variation_or_rollout<F>(
    flag: &EvalFlag,
    vor: &EvalVariationOrRollout,
    context: &Context,
    default_value: &Value,
    make_reason: F,
) -> EvaluationDetail
where
    F: FnOnce(bool) -> EvaluationReason,
{
    if let Some(idx) = vor.variation {
        return resolve_variation_index(flag, idx, default_value, make_reason(false));
    }
    if let Some(ref rollout) = vor.rollout {
        let Some(res) = evaluate_rollout(&flag.key, &flag.salt, rollout, context) else {
            return error_detail(default_value.clone(), EvaluationErrorKind::MalformedFlag);
        };
        return resolve_variation_index(
            flag,
            res.variation_index,
            default_value,
            make_reason(res.in_experiment),
        );
    }
    error_detail(default_value.clone(), EvaluationErrorKind::MalformedFlag)
}

fn error_detail(default_value: Value, error_kind: EvaluationErrorKind) -> EvaluationDetail {
    EvaluationDetail {
        value: default_value,
        variation_index: None,
        reason: EvaluationReason::Error { error_kind },
    }
}
