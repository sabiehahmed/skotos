use std::cmp::Ordering;

use chrono::DateTime;
use regex::Regex;
use semver::Version;
use serde_json::Value;
use skotos_domain::{AttributeRef, Clause, ClauseOp, Context};

use crate::segment::evaluate_segment;
use crate::store::EvalStore;

/// Internal error signaling a malformed flag or cyclic dependency during evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InternalEvalError {
    MalformedFlag,
}

/// Evaluates a single [`Clause`] against `context`, supporting all 15 canonical operators,
/// array-OR semantics, missing-attribute non-negation, and diamond-safe segment cycle checks.
pub fn eval_clause<S: EvalStore>(
    store: &S,
    clause: &Clause,
    context: &Context,
    segment_stack: &mut Vec<String>,
) -> Result<bool, InternalEvalError> {
    if clause.op == ClauseOp::SegmentMatch {
        let mut matched = false;
        for val in &clause.values {
            if let Some(seg_key) = val.as_str() {
                if let Some(segment) = store.get_segment(seg_key) {
                    if evaluate_segment(store, segment, context, segment_stack)? {
                        matched = true;
                        break;
                    }
                }
            }
        }
        return Ok(if clause.negate { !matched } else { matched });
    }

    // For all non-segment operators: missing contextKind, invalid AttributeRef,
    // missing/null attribute, or empty array [] returns false WITHOUT applying negate.
    let Some(single) = context.get_kind(&clause.context_kind) else {
        return Ok(false);
    };
    let Ok(attr_ref) = AttributeRef::parse(&clause.attribute) else {
        return Ok(false);
    };
    let Some(attr_val) = single.get_attribute(&attr_ref) else {
        return Ok(false);
    };
    if attr_val.is_null() {
        return Ok(false);
    }

    let raw_match = match &attr_val {
        Value::Array(items) => {
            if items.is_empty() {
                return Ok(false);
            }
            items.iter().any(|item| {
                !item.is_null()
                    && clause
                        .values
                        .iter()
                        .any(|cv| match_single_value(clause.op, item, cv))
            })
        }
        scalar => clause
            .values
            .iter()
            .any(|cv| match_single_value(clause.op, scalar, cv)),
    };

    Ok(if clause.negate { !raw_match } else { raw_match })
}

fn match_single_value(op: ClauseOp, actual: &Value, target: &Value) -> bool {
    match op {
        ClauseOp::In => json_equals(actual, target),
        ClauseOp::StartsWith => match (actual.as_str(), target.as_str()) {
            (Some(a), Some(t)) => a.starts_with(t),
            _ => false,
        },
        ClauseOp::EndsWith => match (actual.as_str(), target.as_str()) {
            (Some(a), Some(t)) => a.ends_with(t),
            _ => false,
        },
        ClauseOp::Contains => match (actual.as_str(), target.as_str()) {
            (Some(a), Some(t)) => a.contains(t),
            _ => false,
        },
        ClauseOp::Matches => match (actual.as_str(), target.as_str()) {
            (Some(a), Some(pattern)) => Regex::new(pattern).is_ok_and(|re| re.is_match(a)),
            _ => false,
        },
        ClauseOp::LessThan => compare_numbers(actual, target) == Some(Ordering::Less),
        ClauseOp::LessThanOrEqual => {
            matches!(
                compare_numbers(actual, target),
                Some(Ordering::Less | Ordering::Equal)
            )
        }
        ClauseOp::GreaterThan => compare_numbers(actual, target) == Some(Ordering::Greater),
        ClauseOp::GreaterThanOrEqual => {
            matches!(
                compare_numbers(actual, target),
                Some(Ordering::Greater | Ordering::Equal)
            )
        }
        ClauseOp::Before => compare_dates(actual, target) == Some(Ordering::Less),
        ClauseOp::After => compare_dates(actual, target) == Some(Ordering::Greater),
        ClauseOp::SemverEqual => compare_semver(actual, target) == Some(Ordering::Equal),
        ClauseOp::SemverLessThan => compare_semver(actual, target) == Some(Ordering::Less),
        ClauseOp::SemverGreaterThan => compare_semver(actual, target) == Some(Ordering::Greater),
        ClauseOp::SegmentMatch => false,
    }
}

fn json_equals(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(na), Value::Number(nb)) => match (na.as_f64(), nb.as_f64()) {
            (Some(fa), Some(fb)) => fa == fb,
            _ => na == nb,
        },
        _ => a == b,
    }
}

fn compare_numbers(a: &Value, b: &Value) -> Option<Ordering> {
    let fa = a.as_f64()?;
    let fb = b.as_f64()?;
    fa.partial_cmp(&fb)
}

/// Parses an RFC 3339 timestamp string or numeric epoch milliseconds into epoch nanoseconds.
fn parse_timestamp_nanos(v: &Value) -> Option<i128> {
    match v {
        Value::String(s) => {
            let dt = DateTime::parse_from_rfc3339(s).ok()?;
            Some(
                i128::from(dt.timestamp()) * 1_000_000_000
                    + i128::from(dt.timestamp_subsec_nanos()),
            )
        }
        Value::Number(n) => {
            if let Some(ms) = n.as_i64() {
                Some(i128::from(ms) * 1_000_000)
            } else if let Some(ms) = n.as_f64() {
                if ms.is_finite() {
                    Some((ms * 1_000_000.0) as i128)
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

fn compare_dates(a: &Value, b: &Value) -> Option<Ordering> {
    let ta = parse_timestamp_nanos(a)?;
    let tb = parse_timestamp_nanos(b)?;
    Some(ta.cmp(&tb))
}

/// Parses SemVer 2.0.0 with LaunchDarkly 1- and 2-component shorthand (`"1"` -> `"1.0.0"`,
/// `"1.2"` -> `"1.2.0"`, `"1.2-rc.1"` -> `"1.2.0-rc.1"`). Rejects leading `"v"`/`"V"`.
pub fn parse_semver_relaxed(input: &str) -> Option<Version> {
    if input.is_empty() || input.starts_with('v') || input.starts_with('V') {
        return None;
    }
    if let Ok(v) = Version::parse(input) {
        return Some(v);
    }

    // Split off optional pre-release (`-`) or build metadata (`+`) suffix
    let suffix_idx = input.find(['-', '+']).unwrap_or(input.len());
    let (core, suffix) = input.split_at(suffix_idx);
    let parts: Vec<&str> = core.split('.').collect();
    let normalized = match parts.len() {
        1 => format!("{}.0.0{suffix}", parts[0]),
        2 => format!("{}.{}.0{suffix}", parts[0], parts[1]),
        _ => return None,
    };
    Version::parse(&normalized).ok()
}

fn compare_semver(a: &Value, b: &Value) -> Option<Ordering> {
    let va = parse_semver_relaxed(a.as_str()?)?;
    let vb = parse_semver_relaxed(b.as_str()?)?;
    // Per SemVer 2.0.0 section 10 & PROJECT.md, build metadata is ignored in precedence/equality
    Some(va.cmp_precedence(&vb))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_semver_shorthand_and_ignores_build_metadata() {
        let a = parse_semver_relaxed("1.2").unwrap();
        let b = parse_semver_relaxed("1.2.0+build.99").unwrap();
        assert_eq!(a.cmp_precedence(&b), Ordering::Equal);

        let pre = parse_semver_relaxed("2-beta.1").unwrap();
        let rel = parse_semver_relaxed("2").unwrap();
        assert_eq!(pre.cmp_precedence(&rel), Ordering::Less);

        assert!(parse_semver_relaxed("v1.0.0").is_none());
        assert!(parse_semver_relaxed("V2.0").is_none());
    }
}
