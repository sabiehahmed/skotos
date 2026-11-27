use serde_json::Value;
use sha1::{Digest, Sha1};
use skotos_domain::{AttributeRef, Context, EvalRollout, RolloutKind};

/// Exact basis-point bucket range (`0..=99_999`, i.e., `100_000` buckets = 0.001% precision).
pub const BUCKET_SCALE: u128 = 100_000;

/// Result of resolving a rollout against a context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RolloutResult {
    pub variation_index: usize,
    pub in_experiment: bool,
}

/// Computes the deterministic SHA-1 60-bit integer basis-point bucket in `0..=99_999`.
///
/// Formula locked in `PROJECT.md`:
/// - Take the first 15 hex chars (top 60 bits, big-endian) of `SHA-1(hash_input)` as `h: u64`.
/// - `bucket = ((h as u128 * 100_000u128) / (1u128 << 60)) as u64`.
pub fn hash_to_bucket(hash_input: &str) -> u64 {
    let digest = Sha1::digest(hash_input.as_bytes());
    let first_eight: [u8; 8] = [
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
    ];
    let top_60_bits = u64::from_be_bytes(first_eight) >> 4;
    ((u128::from(top_60_bits) * BUCKET_SCALE) / (1u128 << 60)) as u64
}

/// Builds the canonical hash input string.
pub fn build_hash_input(
    key: &str,
    salt: &str,
    bucket_value: &str,
    secondary: Option<&str>,
    seed: Option<i64>,
) -> String {
    if let Some(s) = seed {
        return format!("{s}.{bucket_value}");
    }
    match secondary {
        Some(sec) if !sec.is_empty() => format!("{key}.{salt}.{bucket_value}.{sec}"),
        _ => format!("{key}.{salt}.{bucket_value}"),
    }
}

/// Extracts a hashable bucketing value (`String` or integer `Number`) from the context.
/// Returns `None` if `context_kind` is missing, `bucket_by` is invalid, or the attribute
/// is missing / non-string / non-integer.
pub fn extract_bucket_value(
    context: &Context,
    context_kind: &str,
    bucket_by: Option<&str>,
) -> Option<(String, Option<String>)> {
    let single = context.get_kind(context_kind)?;
    let attr_str = bucket_by.unwrap_or("key");
    let attr_ref = AttributeRef::parse(attr_str).ok()?;
    let val = single.get_attribute(&attr_ref)?;
    let bucket_str = match val {
        Value::String(s) => s,
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else {
                n.as_u64()?.to_string()
            }
        }
        _ => return None,
    };
    let secondary = single.meta.secondary.clone().filter(|s| !s.is_empty());
    Some((bucket_str, secondary))
}

/// Evaluates an [`EvalRollout`] for a flag and returns the selected variation index and
/// `in_experiment` flag. Returns `None` only if `rollout.variations` is empty (malformed).
pub fn evaluate_rollout(
    flag_key: &str,
    salt: &str,
    rollout: &EvalRollout,
    context: &Context,
) -> Option<RolloutResult> {
    if rollout.variations.is_empty() {
        return None;
    }

    let is_experiment = rollout.kind == RolloutKind::Experiment;
    // Experiments always bucket by "key" (ignore custom bucket_by) unless specified otherwise
    let effective_bucket_by = if is_experiment {
        None
    } else {
        rollout.bucket_by.as_deref()
    };

    let (bucket, valid_context) =
        match extract_bucket_value(context, &rollout.context_kind, effective_bucket_by) {
            Some((bucket_value, secondary)) => {
                let effective_secondary = if is_experiment {
                    None
                } else {
                    secondary.as_deref()
                };
                let input = build_hash_input(
                    flag_key,
                    salt,
                    &bucket_value,
                    effective_secondary,
                    rollout.seed,
                );
                (hash_to_bucket(&input), true)
            }
            None => (0u64, false),
        };

    // Walk cumulative weights; select first variation where bucket < cumulative_weight.
    let mut cumulative: u64 = 0;
    for wv in &rollout.variations {
        cumulative += u64::from(wv.weight);
        if bucket < cumulative {
            return Some(RolloutResult {
                variation_index: wv.variation,
                in_experiment: is_experiment && valid_context && !wv.untracked,
            });
        }
    }

    // Fallback when sum < 100_000 or all weights are 0:
    // if context was missing and all weights 0, select first variation; else last variation.
    let fallback = if cumulative == 0 {
        &rollout.variations[0]
    } else {
        rollout.variations.last()?
    };
    Some(RolloutResult {
        variation_index: fallback.variation,
        in_experiment: is_experiment && valid_context && !fallback.untracked,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_to_bucket_matches_hex_prefix_spec() {
        let input = "my-flag.my-salt.user-123";
        let digest = Sha1::digest(input.as_bytes());
        let hex_str = hex::encode(digest);
        let h_from_hex = u64::from_str_radix(&hex_str[..15], 16).unwrap();
        let expected = ((u128::from(h_from_hex) * 100_000u128) / (1u128 << 60)) as u64;
        assert_eq!(hash_to_bucket(input), expected);
        assert!(expected < 100_000);
    }
}
