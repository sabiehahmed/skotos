use skotos_domain::{Context, EvalSegment};

use crate::bucketing::{build_hash_input, extract_bucket_value, hash_to_bucket};
use crate::operators::{eval_clause, InternalEvalError};
use crate::store::EvalStore;

/// Evaluates whether `context` belongs to `segment` following locked precedence:
/// 1. `included` / `included_contexts` -> `true`
/// 2. `excluded` / `excluded_contexts` -> `false`
/// 3. `rules[]` (clauses ANDed + optional rollout weight) -> `true` on first match
///
/// Uses a push/pop `segment_stack` so diamond segment DAGs succeed while true cycles
/// return `Err(InternalEvalError::MalformedFlag)`.
pub fn evaluate_segment<S: EvalStore>(
    store: &S,
    segment: &EvalSegment,
    context: &Context,
    segment_stack: &mut Vec<String>,
) -> Result<bool, InternalEvalError> {
    if segment_stack.iter().any(|k| k == &segment.key) {
        return Err(InternalEvalError::MalformedFlag);
    }

    // 1. Explicit inclusions take top precedence
    if matches_key_list(context, "user", &segment.included) {
        return Ok(true);
    }
    for target in &segment.included_contexts {
        if matches_key_list(context, &target.context_kind, &target.values) {
            return Ok(true);
        }
    }

    // 2. Explicit exclusions take precedence over dynamic rules
    if matches_key_list(context, "user", &segment.excluded) {
        return Ok(false);
    }
    for target in &segment.excluded_contexts {
        if matches_key_list(context, &target.context_kind, &target.values) {
            return Ok(false);
        }
    }

    // 3. Dynamic rules with diamond-safe cycle stack
    segment_stack.push(segment.key.clone());
    let mut matched = false;
    for rule in &segment.rules {
        let mut all_clauses_match = true;
        for clause in &rule.clauses {
            match eval_clause(store, clause, context, segment_stack) {
                Ok(true) => {}
                Ok(false) => {
                    all_clauses_match = false;
                    break;
                }
                Err(e) => {
                    segment_stack.pop();
                    return Err(e);
                }
            }
        }

        if all_clauses_match {
            if let Some(weight) = rule.weight {
                if weight == 0 {
                    continue;
                }
                if weight >= 100_000 {
                    matched = true;
                    break;
                }
                let rollout_kind = rule.rollout_context_kind.as_deref().unwrap_or("user");
                if let Some((bucket_val, secondary)) =
                    extract_bucket_value(context, rollout_kind, rule.bucket_by.as_deref())
                {
                    let input = build_hash_input(
                        &segment.key,
                        &segment.salt,
                        &bucket_val,
                        secondary.as_deref(),
                        None,
                    );
                    if hash_to_bucket(&input) < u64::from(weight) {
                        matched = true;
                        break;
                    }
                }
            } else {
                matched = true;
                break;
            }
        }
    }
    segment_stack.pop();

    Ok(matched)
}

fn matches_key_list(context: &Context, kind: &str, keys: &[String]) -> bool {
    if keys.is_empty() {
        return false;
    }
    let Some(single) = context.get_kind(kind) else {
        return false;
    };
    keys.iter().any(|k| k == &single.key)
}
