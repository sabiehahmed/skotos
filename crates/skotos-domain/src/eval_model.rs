use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::entities::{
    default_context_kind, Clause, ClientSideAvailability, FeatureFlag, FlagEnvConfig, RolloutKind,
    Segment, SegmentRule, SegmentTarget, VariationOrRollout,
};
use crate::error::DomainError;

/// Canonical compiled flag representation consumed by `crates/skotos-eval`
/// and `spec/evaluation-tests/` fixtures (`variationId` UUIDs compiled to `usize` indices).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalFlag {
    pub key: String,
    pub version: i64,
    pub on: bool,
    pub salt: String,
    pub variations: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off_variation: Option<usize>,
    #[serde(default)]
    pub prerequisites: Vec<EvalPrerequisite>,
    #[serde(default)]
    pub targets: Vec<EvalTarget>,
    #[serde(default)]
    pub rules: Vec<EvalRule>,
    pub fallthrough: EvalVariationOrRollout,
    #[serde(default, skip_serializing_if = "ClientSideAvailability::is_default")]
    pub client_side_availability: ClientSideAvailability,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub track_events: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub track_events_fallthrough: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalPrerequisite {
    pub key: String,
    pub variation: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalTarget {
    #[serde(default = "default_context_kind")]
    pub context_kind: String,
    #[serde(default)]
    pub values: Vec<String>,
    pub variation: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalRule {
    pub id: String,
    #[serde(default)]
    pub clauses: Vec<Clause>,
    #[serde(flatten)]
    pub variation_or_rollout: EvalVariationOrRollout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalVariationOrRollout {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variation: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollout: Option<EvalRollout>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalRollout {
    #[serde(default = "default_context_kind")]
    pub context_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(default)]
    pub kind: RolloutKind,
    pub variations: Vec<EvalWeightedVariation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalWeightedVariation {
    pub variation: usize,
    pub weight: u32,
    #[serde(default)]
    pub untracked: bool,
}

/// Canonical compiled segment representation consumed by `crates/skotos-eval`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalSegment {
    pub key: String,
    #[serde(default)]
    pub version: i64,
    #[serde(default)]
    pub salt: String,
    #[serde(default)]
    pub included: Vec<String>,
    #[serde(default)]
    pub excluded: Vec<String>,
    #[serde(default)]
    pub included_contexts: Vec<SegmentTarget>,
    #[serde(default)]
    pub excluded_contexts: Vec<SegmentTarget>,
    #[serde(default)]
    pub rules: Vec<SegmentRule>,
}

/// Detailed result returned by the evaluation engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct EvaluationDetail {
    pub value: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variation_index: Option<usize>,
    pub reason: EvaluationReason,
}

/// Structured explanation of why a variation was served.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvaluationReason {
    Off,
    #[serde(rename_all = "camelCase")]
    Fallthrough {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        in_experiment: bool,
    },
    TargetMatch,
    #[serde(rename_all = "camelCase")]
    RuleMatch {
        rule_index: usize,
        rule_id: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        in_experiment: bool,
    },
    #[serde(rename_all = "camelCase")]
    PrerequisiteFailed {
        prerequisite_key: String,
    },
    #[serde(rename_all = "camelCase")]
    Error {
        error_kind: EvaluationErrorKind,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvaluationErrorKind {
    ClientNotReady,
    FlagNotFound,
    MalformedFlag,
    UserNotSpecified,
    WrongType,
    Exception,
}

fn compile_variation_or_rollout(
    flag: &FeatureFlag,
    vor: &VariationOrRollout,
) -> Result<EvalVariationOrRollout, DomainError> {
    match (&vor.variation_id, &vor.rollout) {
        (Some(var_id), None) => {
            let idx = flag
                .variation_index_by_id(*var_id)
                .ok_or(DomainError::VariationNotFound(*var_id))?;
            Ok(EvalVariationOrRollout {
                variation: Some(idx),
                rollout: None,
            })
        }
        (None, Some(rollout)) => {
            let mut weighted = Vec::with_capacity(rollout.variations.len());
            for wv in &rollout.variations {
                let idx = flag
                    .variation_index_by_id(wv.variation_id)
                    .ok_or(DomainError::VariationNotFound(wv.variation_id))?;
                weighted.push(EvalWeightedVariation {
                    variation: idx,
                    weight: wv.weight,
                    untracked: wv.untracked,
                });
            }
            Ok(EvalVariationOrRollout {
                variation: None,
                rollout: Some(EvalRollout {
                    context_kind: rollout.context_kind.clone(),
                    bucket_by: rollout.bucket_by.clone(),
                    seed: rollout.seed,
                    kind: rollout.kind,
                    variations: weighted,
                }),
            })
        }
        _ => Err(DomainError::Validation(
            "VariationOrRollout must specify exactly one of variationId or rollout".to_string(),
        )),
    }
}

/// Compiles a Control Plane `FeatureFlag` + `FlagEnvConfig` into a zero-UUID `EvalFlag`.
/// When the flag has prerequisites, pass `other_flags` keyed by `flag.key` via
/// [`compile_eval_flag_with_flags`].
pub fn compile_eval_flag(
    flag: &FeatureFlag,
    env_config: &FlagEnvConfig,
) -> Result<EvalFlag, DomainError> {
    compile_eval_flag_with_flags(flag, env_config, &HashMap::new())
}

/// Compiles a Control Plane `FeatureFlag` + `FlagEnvConfig` into an `EvalFlag`,
/// resolving prerequisite `variationId` UUIDs against `other_flags`.
pub fn compile_eval_flag_with_flags(
    flag: &FeatureFlag,
    env_config: &FlagEnvConfig,
    other_flags: &HashMap<String, &FeatureFlag>,
) -> Result<EvalFlag, DomainError> {
    let off_variation = match env_config.config.off_variation_id {
        Some(id) => Some(
            flag.variation_index_by_id(id)
                .ok_or(DomainError::VariationNotFound(id))?,
        ),
        None => None,
    };

    let mut prerequisites = Vec::with_capacity(env_config.config.prerequisites.len());
    for prereq in &env_config.config.prerequisites {
        let target_flag = if prereq.flag_key == flag.key {
            flag
        } else {
            other_flags
                .get(&prereq.flag_key)
                .copied()
                .ok_or_else(|| DomainError::PrerequisiteFlagNotFound(prereq.flag_key.clone()))?
        };
        let var_idx = target_flag
            .variation_index_by_id(prereq.variation_id)
            .ok_or(DomainError::VariationNotFound(prereq.variation_id))?;
        prerequisites.push(EvalPrerequisite {
            key: prereq.flag_key.clone(),
            variation: var_idx,
        });
    }

    let mut targets = Vec::with_capacity(env_config.config.targets.len());
    for t in &env_config.config.targets {
        let var_idx = flag
            .variation_index_by_id(t.variation_id)
            .ok_or(DomainError::VariationNotFound(t.variation_id))?;
        targets.push(EvalTarget {
            context_kind: t.context_kind.clone(),
            values: t.values.clone(),
            variation: var_idx,
        });
    }

    let mut rules = Vec::with_capacity(env_config.config.rules.len());
    for r in &env_config.config.rules {
        rules.push(EvalRule {
            id: r.id.clone(),
            clauses: r.clauses.clone(),
            variation_or_rollout: compile_variation_or_rollout(flag, &r.variation_or_rollout)?,
        });
    }

    let fallthrough = compile_variation_or_rollout(flag, &env_config.config.fallthrough)?;

    Ok(EvalFlag {
        key: flag.key.clone(),
        version: env_config.version,
        on: env_config.on,
        salt: env_config.salt.clone(),
        variations: flag.variations.iter().map(|v| v.value.clone()).collect(),
        off_variation,
        prerequisites,
        targets,
        rules,
        fallthrough,
        client_side_availability: flag.client_side_availability,
        track_events: env_config.config.track_events,
        track_events_fallthrough: env_config.config.track_events_fallthrough,
    })
}

/// Compiles a Control Plane `Segment` into an `EvalSegment`.
pub fn compile_eval_segment(segment: &Segment) -> EvalSegment {
    EvalSegment {
        key: segment.key.clone(),
        version: segment.version,
        salt: segment.salt.clone(),
        included: segment.included.clone(),
        excluded: segment.excluded.clone(),
        included_contexts: segment.included_contexts.clone(),
        excluded_contexts: segment.excluded_contexts.clone(),
        rules: segment.rules.clone(),
    }
}
