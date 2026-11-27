use std::collections::{HashMap, HashSet};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::context::{validate_context_kind, AttributeRef};
use crate::entities::{
    Clause, ClauseOp, ClientSideAvailability, FeatureFlag, FlagEnvConfig, FlagEnvConfigBody,
    Prerequisite, Rollout, Target, TargetingRule, Variation, VariationOrRollout,
};
use crate::error::DomainError;

/// Maximum rollout weight total (100.000% in basis points).
pub const ROLLOUT_TOTAL_WEIGHT: u32 = 100_000;

/// Canonical 23-variant semantic instruction patch enum for feature flag mutations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FlagPatchInstruction {
    // ── Environment targeting & lifecycle (15) ──────────────────────────
    TurnFlagOn,
    TurnFlagOff,
    #[serde(rename_all = "camelCase")]
    UpdateOffVariation {
        variation_id: Option<Uuid>,
    },
    #[serde(rename_all = "camelCase")]
    AddTargets {
        context_kind: String,
        variation_id: Uuid,
        values: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    RemoveTargets {
        context_kind: String,
        variation_id: Uuid,
        values: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    AddRule {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        clauses: Vec<Clause>,
        #[serde(flatten)]
        variation_or_rollout: VariationOrRollout,
        #[serde(default)]
        track_events: bool,
    },
    #[serde(rename_all = "camelCase")]
    UpdateRule {
        rule_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        clauses: Vec<Clause>,
        #[serde(flatten)]
        variation_or_rollout: VariationOrRollout,
        #[serde(default)]
        track_events: bool,
    },
    #[serde(rename_all = "camelCase")]
    RemoveRule {
        rule_id: String,
    },
    #[serde(rename_all = "camelCase")]
    ReorderRules {
        rule_ids: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    UpdateFallthrough {
        #[serde(flatten)]
        variation_or_rollout: VariationOrRollout,
    },
    #[serde(rename_all = "camelCase")]
    AddPrerequisite {
        flag_key: String,
        variation_id: Uuid,
    },
    #[serde(rename_all = "camelCase")]
    RemovePrerequisite {
        flag_key: String,
    },
    ArchiveFlag,
    RestoreFlag,
    #[serde(rename_all = "camelCase")]
    UpdateClientSideAvailability {
        using_environment_id: bool,
        using_mobile_key: bool,
    },

    // ── Project-level variations (3) ────────────────────────────────────
    #[serde(rename_all = "camelCase")]
    AddVariation {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<Uuid>,
        value: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    UpdateVariation {
        variation_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    RemoveVariation {
        variation_id: Uuid,
    },

    // ── Project-level metadata (5) ──────────────────────────────────────
    #[serde(rename_all = "camelCase")]
    UpdateName {
        name: String,
    },
    #[serde(rename_all = "camelCase")]
    UpdateDescription {
        description: String,
    },
    #[serde(rename_all = "camelCase")]
    UpdateTags {
        tags: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    UpdateMaintainer {
        maintainer_id: Option<Uuid>,
    },
    #[serde(rename_all = "camelCase")]
    UpdateTemporary {
        temporary: bool,
    },
}

/// Human-readable description of a single semantic patch instruction for UI diffs and audit logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PatchSummaryItem {
    pub kind: String,
    pub summary: String,
}

/// Result of applying a batch of semantic instructions to a flag and one environment config.
#[derive(Debug, Clone, PartialEq)]
pub struct PatchResult {
    pub flag: FeatureFlag,
    pub env_config: FlagEnvConfig,
    pub env_config_changed: bool,
    pub flag_changed: bool,
    pub summaries: Vec<PatchSummaryItem>,
}

/// Validates a rollout's weights, context kind, and variation IDs.
pub fn validate_rollout(flag: &FeatureFlag, rollout: &Rollout) -> Result<(), DomainError> {
    validate_context_kind(&rollout.context_kind)?;
    if let Some(ref bucket_by) = rollout.bucket_by {
        AttributeRef::parse(bucket_by)?;
    }
    if rollout.variations.is_empty() {
        return Err(DomainError::InvalidRolloutWeights(
            "rollout must include at least one variation".to_string(),
        ));
    }
    let mut sum: u64 = 0;
    for wv in &rollout.variations {
        if flag.variation_index_by_id(wv.variation_id).is_none() {
            return Err(DomainError::VariationNotFound(wv.variation_id));
        }
        if wv.weight > ROLLOUT_TOTAL_WEIGHT {
            return Err(DomainError::InvalidRolloutWeights(format!(
                "weight {} exceeds maximum {}",
                wv.weight, ROLLOUT_TOTAL_WEIGHT
            )));
        }
        sum += u64::from(wv.weight);
    }
    if sum != u64::from(ROLLOUT_TOTAL_WEIGHT) {
        return Err(DomainError::InvalidRolloutWeights(format!(
            "rollout weights must sum to {ROLLOUT_TOTAL_WEIGHT} (got {sum})"
        )));
    }
    Ok(())
}

/// Validates a `VariationOrRollout` against the flag's variations.
pub fn validate_variation_or_rollout(
    flag: &FeatureFlag,
    vor: &VariationOrRollout,
) -> Result<(), DomainError> {
    match (&vor.variation_id, &vor.rollout) {
        (Some(id), None) => {
            if flag.variation_index_by_id(*id).is_none() {
                return Err(DomainError::VariationNotFound(*id));
            }
            Ok(())
        }
        (None, Some(rollout)) => validate_rollout(flag, rollout),
        _ => Err(DomainError::Validation(
            "must specify exactly one of variationId or rollout".to_string(),
        )),
    }
}

/// Validates a single targeting clause (context kind, attribute ref, linear-time RE2 regex).
pub fn validate_clause(clause: &Clause) -> Result<(), DomainError> {
    if clause.op == ClauseOp::SegmentMatch {
        if clause.values.is_empty() {
            return Err(DomainError::InvalidClause(
                "segmentMatch clause requires at least one segment key".to_string(),
            ));
        }
        for v in &clause.values {
            if !v.is_string() {
                return Err(DomainError::InvalidClause(
                    "segmentMatch values must be segment key strings".to_string(),
                ));
            }
        }
        return Ok(());
    }

    validate_context_kind(&clause.context_kind)?;
    AttributeRef::parse(&clause.attribute)?;

    if clause.op == ClauseOp::Matches {
        for v in &clause.values {
            if let Some(pattern) = v.as_str() {
                Regex::new(pattern).map_err(|e| DomainError::InvalidRegex {
                    pattern: pattern.to_string(),
                    reason: e.to_string(),
                })?;
            }
        }
    }
    Ok(())
}

/// Validates an entire `FlagEnvConfigBody` against its owning `FeatureFlag`.
pub fn validate_flag_env_config(
    flag: &FeatureFlag,
    body: &FlagEnvConfigBody,
) -> Result<(), DomainError> {
    flag.validate_variations()?;

    if let Some(off_id) = body.off_variation_id {
        if flag.variation_index_by_id(off_id).is_none() {
            return Err(DomainError::VariationNotFound(off_id));
        }
    }

    validate_variation_or_rollout(flag, &body.fallthrough)?;

    for prereq in &body.prerequisites {
        if prereq.flag_key == flag.key {
            return Err(DomainError::SelfPrerequisite(flag.key.clone()));
        }
    }

    for target in &body.targets {
        validate_context_kind(&target.context_kind)?;
        if flag.variation_index_by_id(target.variation_id).is_none() {
            return Err(DomainError::VariationNotFound(target.variation_id));
        }
    }

    let mut seen_rule_ids = HashSet::with_capacity(body.rules.len());
    for rule in &body.rules {
        if rule.id.is_empty() {
            return Err(DomainError::InvalidPatch(
                "targeting rule id cannot be empty".to_string(),
            ));
        }
        if !seen_rule_ids.insert(rule.id.as_str()) {
            return Err(DomainError::InvalidPatch(format!(
                "duplicate targeting rule id '{}'",
                rule.id
            )));
        }
        for clause in &rule.clauses {
            validate_clause(clause)?;
        }
        validate_variation_or_rollout(flag, &rule.variation_or_rollout)?;
    }

    Ok(())
}

/// Checks if a variation is still referenced within an environment config body.
pub fn variation_in_use_locations(
    env_label: &str,
    body: &FlagEnvConfigBody,
    variation_id: Uuid,
) -> Vec<String> {
    let mut refs = Vec::new();
    if body.off_variation_id == Some(variation_id) {
        refs.push(format!("{env_label}:offVariation"));
    }
    if body.fallthrough.variation_id == Some(variation_id) {
        refs.push(format!("{env_label}:fallthrough"));
    }
    if let Some(ref rollout) = body.fallthrough.rollout {
        if rollout
            .variations
            .iter()
            .any(|v| v.variation_id == variation_id)
        {
            refs.push(format!("{env_label}:fallthrough.rollout"));
        }
    }
    for t in &body.targets {
        if t.variation_id == variation_id && !t.values.is_empty() {
            refs.push(format!("{env_label}:target({})", t.context_kind));
        }
    }
    for (idx, r) in body.rules.iter().enumerate() {
        if r.variation_or_rollout.variation_id == Some(variation_id) {
            refs.push(format!("{env_label}:rule[{idx}]({})", r.id));
        }
        if let Some(ref rollout) = r.variation_or_rollout.rollout {
            if rollout
                .variations
                .iter()
                .any(|v| v.variation_id == variation_id)
            {
                refs.push(format!("{env_label}:rule[{idx}]({}).rollout", r.id));
            }
        }
    }
    refs
}

/// Detects cycles in a flag prerequisite dependency graph starting from `start_flag`.
/// Uses a push/pop path stack so diamond DAGs (`A -> B`, `A -> C`, `B -> D`, `C -> D`)
/// are accepted while any cycle returns `DomainError::PrerequisiteCycle`.
pub fn detect_prerequisite_cycle(
    start_flag: &str,
    adjacency: &HashMap<String, Vec<String>>,
) -> Result<(), DomainError> {
    let mut stack: Vec<String> = Vec::new();
    let mut visited_ok: HashSet<String> = HashSet::new();
    dfs_cycle(start_flag, adjacency, &mut stack, &mut visited_ok)
}

fn dfs_cycle(
    node: &str,
    adjacency: &HashMap<String, Vec<String>>,
    stack: &mut Vec<String>,
    visited_ok: &mut HashSet<String>,
) -> Result<(), DomainError> {
    if let Some(pos) = stack.iter().position(|n| n == node) {
        let mut cycle = stack[pos..].to_vec();
        cycle.push(node.to_string());
        return Err(DomainError::PrerequisiteCycle { path: cycle });
    }
    if visited_ok.contains(node) {
        return Ok(());
    }
    stack.push(node.to_string());
    if let Some(deps) = adjacency.get(node) {
        for dep in deps {
            dfs_cycle(dep, adjacency, stack, visited_ok)?;
        }
    }
    stack.pop();
    visited_ok.insert(node.to_string());
    Ok(())
}

/// Applies a slice of semantic instructions deterministically to a cloned flag and environment config.
pub fn apply_flag_patch(
    flag: &FeatureFlag,
    env_config: &FlagEnvConfig,
    instructions: &[FlagPatchInstruction],
) -> Result<PatchResult, DomainError> {
    if instructions.is_empty() {
        return Err(DomainError::EmptyInstructions);
    }

    let mut next_flag = flag.clone();
    let mut next_env = env_config.clone();
    let mut env_changed = false;
    let mut flag_changed = false;
    let mut summaries = Vec::with_capacity(instructions.len());

    for instr in instructions {
        match instr {
            FlagPatchInstruction::TurnFlagOn => {
                next_env.on = true;
                env_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "turnFlagOn".to_string(),
                    summary: "Turned targeting ON".to_string(),
                });
            }
            FlagPatchInstruction::TurnFlagOff => {
                next_env.on = false;
                env_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "turnFlagOff".to_string(),
                    summary: "Turned targeting OFF".to_string(),
                });
            }
            FlagPatchInstruction::UpdateOffVariation { variation_id } => {
                if let Some(id) = variation_id {
                    if next_flag.variation_index_by_id(*id).is_none() {
                        return Err(DomainError::VariationNotFound(*id));
                    }
                }
                next_env.config.off_variation_id = *variation_id;
                env_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "updateOffVariation".to_string(),
                    summary: format!("Updated off variation to {variation_id:?}"),
                });
            }
            FlagPatchInstruction::AddTargets {
                context_kind,
                variation_id,
                values,
            } => {
                validate_context_kind(context_kind)?;
                if next_flag.variation_index_by_id(*variation_id).is_none() {
                    return Err(DomainError::VariationNotFound(*variation_id));
                }
                // Remove added keys from any other variation of the same contextKind to preserve uniqueness
                for t in &mut next_env.config.targets {
                    if t.context_kind == *context_kind && t.variation_id != *variation_id {
                        t.values.retain(|v| !values.contains(v));
                    }
                }
                if let Some(existing) =
                    next_env.config.targets.iter_mut().find(|t| {
                        t.context_kind == *context_kind && t.variation_id == *variation_id
                    })
                {
                    for val in values {
                        if !existing.values.contains(val) {
                            existing.values.push(val.clone());
                        }
                    }
                } else {
                    let mut deduped = Vec::new();
                    for val in values {
                        if !deduped.contains(val) {
                            deduped.push(val.clone());
                        }
                    }
                    next_env.config.targets.push(Target {
                        context_kind: context_kind.clone(),
                        values: deduped,
                        variation_id: *variation_id,
                    });
                }
                next_env.config.targets.retain(|t| !t.values.is_empty());
                env_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "addTargets".to_string(),
                    summary: format!(
                        "Added {} {context_kind} target(s) to variation {variation_id}",
                        values.len()
                    ),
                });
            }
            FlagPatchInstruction::RemoveTargets {
                context_kind,
                variation_id,
                values,
            } => {
                validate_context_kind(context_kind)?;
                if let Some(existing) =
                    next_env.config.targets.iter_mut().find(|t| {
                        t.context_kind == *context_kind && t.variation_id == *variation_id
                    })
                {
                    existing.values.retain(|v| !values.contains(v));
                }
                next_env.config.targets.retain(|t| !t.values.is_empty());
                env_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "removeTargets".to_string(),
                    summary: format!(
                        "Removed {} {context_kind} target(s) from variation {variation_id}",
                        values.len()
                    ),
                });
            }
            FlagPatchInstruction::AddRule {
                id,
                description,
                clauses,
                variation_or_rollout,
                track_events,
            } => {
                for clause in clauses {
                    validate_clause(clause)?;
                }
                validate_variation_or_rollout(&next_flag, variation_or_rollout)?;
                let rule_id = id
                    .clone()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| Uuid::new_v4().to_string());
                if next_env.config.rules.iter().any(|r| r.id == rule_id) {
                    return Err(DomainError::InvalidPatch(format!(
                        "rule with id '{rule_id}' already exists"
                    )));
                }
                next_env.config.rules.push(TargetingRule {
                    id: rule_id.clone(),
                    description: description.clone(),
                    clauses: clauses.clone(),
                    variation_or_rollout: variation_or_rollout.clone(),
                    track_events: *track_events,
                });
                env_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "addRule".to_string(),
                    summary: format!("Added targeting rule '{rule_id}'"),
                });
            }
            FlagPatchInstruction::UpdateRule {
                rule_id,
                description,
                clauses,
                variation_or_rollout,
                track_events,
            } => {
                for clause in clauses {
                    validate_clause(clause)?;
                }
                validate_variation_or_rollout(&next_flag, variation_or_rollout)?;
                let rule = next_env
                    .config
                    .rules
                    .iter_mut()
                    .find(|r| r.id == *rule_id)
                    .ok_or_else(|| DomainError::RuleNotFound(rule_id.clone()))?;
                rule.description = description.clone();
                rule.clauses = clauses.clone();
                rule.variation_or_rollout = variation_or_rollout.clone();
                rule.track_events = *track_events;
                env_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "updateRule".to_string(),
                    summary: format!("Updated targeting rule '{rule_id}'"),
                });
            }
            FlagPatchInstruction::RemoveRule { rule_id } => {
                let before = next_env.config.rules.len();
                next_env.config.rules.retain(|r| r.id != *rule_id);
                if next_env.config.rules.len() == before {
                    return Err(DomainError::RuleNotFound(rule_id.clone()));
                }
                env_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "removeRule".to_string(),
                    summary: format!("Removed targeting rule '{rule_id}'"),
                });
            }
            FlagPatchInstruction::ReorderRules { rule_ids } => {
                if rule_ids.len() != next_env.config.rules.len() {
                    return Err(DomainError::InvalidPatch(
                        "reorderRules must include every existing rule id exactly once".to_string(),
                    ));
                }
                let mut by_id: HashMap<String, TargetingRule> = next_env
                    .config
                    .rules
                    .drain(..)
                    .map(|r| (r.id.clone(), r))
                    .collect();
                let mut reordered = Vec::with_capacity(rule_ids.len());
                for rid in rule_ids {
                    let rule = by_id
                        .remove(rid)
                        .ok_or_else(|| DomainError::RuleNotFound(rid.clone()))?;
                    reordered.push(rule);
                }
                next_env.config.rules = reordered;
                env_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "reorderRules".to_string(),
                    summary: format!("Reordered {} targeting rules", rule_ids.len()),
                });
            }
            FlagPatchInstruction::UpdateFallthrough {
                variation_or_rollout,
            } => {
                validate_variation_or_rollout(&next_flag, variation_or_rollout)?;
                next_env.config.fallthrough = variation_or_rollout.clone();
                env_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "updateFallthrough".to_string(),
                    summary: "Updated default fallthrough rule".to_string(),
                });
            }
            FlagPatchInstruction::AddPrerequisite {
                flag_key,
                variation_id,
            } => {
                if *flag_key == next_flag.key {
                    return Err(DomainError::SelfPrerequisite(flag_key.clone()));
                }
                if let Some(existing) = next_env
                    .config
                    .prerequisites
                    .iter_mut()
                    .find(|p| p.flag_key == *flag_key)
                {
                    existing.variation_id = *variation_id;
                } else {
                    next_env.config.prerequisites.push(Prerequisite {
                        flag_key: flag_key.clone(),
                        variation_id: *variation_id,
                    });
                }
                env_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "addPrerequisite".to_string(),
                    summary: format!("Added prerequisite on '{flag_key}'"),
                });
            }
            FlagPatchInstruction::RemovePrerequisite { flag_key } => {
                next_env
                    .config
                    .prerequisites
                    .retain(|p| p.flag_key != *flag_key);
                env_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "removePrerequisite".to_string(),
                    summary: format!("Removed prerequisite '{flag_key}'"),
                });
            }
            FlagPatchInstruction::ArchiveFlag => {
                next_flag.archived = true;
                flag_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "archiveFlag".to_string(),
                    summary: "Archived flag".to_string(),
                });
            }
            FlagPatchInstruction::RestoreFlag => {
                next_flag.archived = false;
                flag_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "restoreFlag".to_string(),
                    summary: "Restored archived flag".to_string(),
                });
            }
            FlagPatchInstruction::UpdateClientSideAvailability {
                using_environment_id,
                using_mobile_key,
            } => {
                next_flag.client_side_availability = ClientSideAvailability {
                    using_environment_id: *using_environment_id,
                    using_mobile_key: *using_mobile_key,
                };
                flag_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "updateClientSideAvailability".to_string(),
                    summary: format!(
                        "Updated client-side availability (envId={using_environment_id}, mobileKey={using_mobile_key})"
                    ),
                });
            }
            FlagPatchInstruction::AddVariation {
                id,
                value,
                name,
                description,
            } => {
                if !next_flag.kind.matches_value(value) {
                    return Err(DomainError::InvalidVariation(format!(
                        "value does not match flag kind '{}'",
                        next_flag.kind.as_str()
                    )));
                }
                let var_id = id.unwrap_or_else(Uuid::new_v4);
                if next_flag.variation_index_by_id(var_id).is_some() {
                    return Err(DomainError::DuplicateVariationId(var_id));
                }
                next_flag.variations.push(Variation {
                    id: var_id,
                    value: value.clone(),
                    name: name.clone(),
                    description: description.clone(),
                });
                flag_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "addVariation".to_string(),
                    summary: format!("Added variation '{var_id}'"),
                });
            }
            FlagPatchInstruction::UpdateVariation {
                variation_id,
                value,
                name,
                description,
            } => {
                let kind = next_flag.kind;
                let var = next_flag
                    .variations
                    .iter_mut()
                    .find(|v| v.id == *variation_id)
                    .ok_or(DomainError::VariationNotFound(*variation_id))?;
                if let Some(new_val) = value {
                    if !kind.matches_value(new_val) {
                        return Err(DomainError::InvalidVariation(format!(
                            "value does not match flag kind '{}'",
                            kind.as_str()
                        )));
                    }
                    var.value = new_val.clone();
                }
                if name.is_some() {
                    var.name = name.clone();
                }
                if description.is_some() {
                    var.description = description.clone();
                }
                flag_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "updateVariation".to_string(),
                    summary: format!("Updated variation '{variation_id}'"),
                });
            }
            FlagPatchInstruction::RemoveVariation { variation_id } => {
                if next_flag.variation_index_by_id(*variation_id).is_none() {
                    return Err(DomainError::VariationNotFound(*variation_id));
                }
                let refs =
                    variation_in_use_locations("environment", &next_env.config, *variation_id);
                if !refs.is_empty() {
                    return Err(DomainError::VariationInUse {
                        variation_id: *variation_id,
                        references: refs,
                    });
                }
                next_flag.variations.retain(|v| v.id != *variation_id);
                next_flag.validate_variations()?;
                flag_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "removeVariation".to_string(),
                    summary: format!("Removed variation '{variation_id}'"),
                });
            }
            FlagPatchInstruction::UpdateName { name } => {
                if name.trim().is_empty() {
                    return Err(DomainError::Validation(
                        "flag name cannot be empty".to_string(),
                    ));
                }
                next_flag.name = name.clone();
                flag_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "updateName".to_string(),
                    summary: format!("Updated flag name to '{name}'"),
                });
            }
            FlagPatchInstruction::UpdateDescription { description } => {
                next_flag.description = description.clone();
                flag_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "updateDescription".to_string(),
                    summary: "Updated flag description".to_string(),
                });
            }
            FlagPatchInstruction::UpdateTags { tags } => {
                next_flag.tags = tags.clone();
                flag_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "updateTags".to_string(),
                    summary: format!("Updated tags ({} total)", tags.len()),
                });
            }
            FlagPatchInstruction::UpdateMaintainer { maintainer_id } => {
                next_flag.maintainer_id = *maintainer_id;
                flag_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "updateMaintainer".to_string(),
                    summary: format!("Updated maintainer to {maintainer_id:?}"),
                });
            }
            FlagPatchInstruction::UpdateTemporary { temporary } => {
                next_flag.temporary = *temporary;
                flag_changed = true;
                summaries.push(PatchSummaryItem {
                    kind: "updateTemporary".to_string(),
                    summary: format!("Marked flag temporary={temporary}"),
                });
            }
        }
    }

    validate_flag_env_config(&next_flag, &next_env.config)?;

    Ok(PatchResult {
        flag: next_flag,
        env_config: next_env,
        env_config_changed: env_changed,
        flag_changed,
        summaries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{FlagKind, WeightedVariation};
    use chrono::Utc;
    use serde_json::json;

    fn sample_flag_and_env() -> (FeatureFlag, FlagEnvConfig, Uuid, Uuid) {
        let on_id = Uuid::new_v4();
        let off_id = Uuid::new_v4();
        let now = Utc::now();
        let flag = FeatureFlag {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            key: "checkout-v2".to_string(),
            name: "Checkout V2".to_string(),
            description: String::new(),
            kind: FlagKind::Boolean,
            variations: vec![
                Variation {
                    id: on_id,
                    value: json!(true),
                    name: Some("On".to_string()),
                    description: None,
                },
                Variation {
                    id: off_id,
                    value: json!(false),
                    name: Some("Off".to_string()),
                    description: None,
                },
            ],
            temporary: true,
            client_side_availability: ClientSideAvailability::default(),
            tags: vec![],
            maintainer_id: None,
            archived: false,
            created_at: now,
            updated_at: now,
        };
        let env_cfg = FlagEnvConfig {
            flag_id: flag.id,
            env_id: Uuid::new_v4(),
            version: 1,
            on: false,
            salt: "salt-1".to_string(),
            config: FlagEnvConfigBody {
                off_variation_id: Some(off_id),
                prerequisites: vec![],
                targets: vec![],
                rules: vec![],
                fallthrough: VariationOrRollout::variation(on_id),
                track_events: false,
                track_events_fallthrough: false,
                debug_events_until_date: None,
            },
            updated_at: now,
        };
        (flag, env_cfg, on_id, off_id)
    }

    #[test]
    fn applies_semantic_instructions_and_validates_invariants() {
        let (flag, env_cfg, on_id, off_id) = sample_flag_and_env();
        let result = apply_flag_patch(
            &flag,
            &env_cfg,
            &[
                FlagPatchInstruction::TurnFlagOn,
                FlagPatchInstruction::AddTargets {
                    context_kind: "user".to_string(),
                    variation_id: on_id,
                    values: vec!["u1".to_string(), "u2".to_string()],
                },
                FlagPatchInstruction::AddRule {
                    id: Some("r1".to_string()),
                    description: Some("Rollout 50%".to_string()),
                    clauses: vec![Clause {
                        context_kind: "user".to_string(),
                        attribute: "plan".to_string(),
                        op: ClauseOp::In,
                        values: vec![json!("pro")],
                        negate: false,
                    }],
                    variation_or_rollout: VariationOrRollout::rollout(Rollout {
                        context_kind: "user".to_string(),
                        bucket_by: None,
                        seed: None,
                        kind: crate::entities::RolloutKind::Rollout,
                        variations: vec![
                            WeightedVariation {
                                variation_id: on_id,
                                weight: 50_000,
                                untracked: false,
                            },
                            WeightedVariation {
                                variation_id: off_id,
                                weight: 50_000,
                                untracked: false,
                            },
                        ],
                    }),
                    track_events: false,
                },
            ],
        )
        .unwrap();

        assert!(result.env_config.on);
        assert_eq!(result.env_config.config.targets.len(), 1);
        assert_eq!(result.env_config.config.rules.len(), 1);
        assert_eq!(result.summaries.len(), 3);
    }

    #[test]
    fn diamond_dag_passes_and_cycle_fails() {
        let mut dag = HashMap::new();
        dag.insert("a".to_string(), vec!["b".to_string(), "c".to_string()]);
        dag.insert("b".to_string(), vec!["d".to_string()]);
        dag.insert("c".to_string(), vec!["d".to_string()]);
        dag.insert("d".to_string(), vec![]);
        assert!(detect_prerequisite_cycle("a", &dag).is_ok());

        dag.insert("d".to_string(), vec!["a".to_string()]);
        assert!(matches!(
            detect_prerequisite_cycle("a", &dag),
            Err(DomainError::PrerequisiteCycle { .. })
        ));
    }

    #[test]
    fn rejects_invalid_regex_and_variation_in_use() {
        let (flag, env_cfg, on_id, _) = sample_flag_and_env();
        let bad_regex = apply_flag_patch(
            &flag,
            &env_cfg,
            &[FlagPatchInstruction::AddRule {
                id: Some("bad".to_string()),
                description: None,
                clauses: vec![Clause {
                    context_kind: "user".to_string(),
                    attribute: "email".to_string(),
                    op: ClauseOp::Matches,
                    values: vec![json!("[unclosed")],
                    negate: false,
                }],
                variation_or_rollout: VariationOrRollout::variation(on_id),
                track_events: false,
            }],
        );
        assert!(matches!(bad_regex, Err(DomainError::InvalidRegex { .. })));

        let in_use = apply_flag_patch(
            &flag,
            &env_cfg,
            &[FlagPatchInstruction::RemoveVariation {
                variation_id: on_id,
            }],
        );
        assert!(matches!(in_use, Err(DomainError::VariationInUse { .. })));
    }
}
