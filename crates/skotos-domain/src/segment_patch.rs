use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::context::{validate_context_kind, AttributeRef};
use crate::entities::{Clause, Segment, SegmentRule, SegmentTarget};
use crate::error::DomainError;
use crate::patch::{validate_clause, PatchSummaryItem, ROLLOUT_TOTAL_WEIGHT};

/// Semantic instruction patches for segment mutations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SegmentPatchInstruction {
    UpdateName {
        name: String,
    },
    #[serde(rename_all = "camelCase")]
    AddIncluded {
        #[serde(default = "default_user_kind")]
        context_kind: String,
        values: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    RemoveIncluded {
        #[serde(default = "default_user_kind")]
        context_kind: String,
        values: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    AddExcluded {
        #[serde(default = "default_user_kind")]
        context_kind: String,
        values: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    RemoveExcluded {
        #[serde(default = "default_user_kind")]
        context_kind: String,
        values: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    AddRule {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        clauses: Vec<Clause>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weight: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bucket_by: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rollout_context_kind: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    UpdateRule {
        rule_id: String,
        clauses: Vec<Clause>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weight: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bucket_by: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rollout_context_kind: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    RemoveRule {
        rule_id: String,
    },
}

fn default_user_kind() -> String {
    "user".to_string()
}

#[derive(Debug, Clone)]
pub struct SegmentPatchResult {
    pub segment: Segment,
    pub summaries: Vec<PatchSummaryItem>,
}

/// Validates segment rules, clauses, context kinds, and rollout weights.
pub fn validate_segment(segment: &Segment) -> Result<(), DomainError> {
    if segment.name.trim().is_empty() {
        return Err(DomainError::Validation(
            "segment name cannot be empty".to_string(),
        ));
    }
    for target in segment
        .included_contexts
        .iter()
        .chain(segment.excluded_contexts.iter())
    {
        validate_context_kind(&target.context_kind)?;
    }
    for rule in &segment.rules {
        for clause in &rule.clauses {
            validate_clause(clause)?;
            if clause.op == crate::entities::ClauseOp::SegmentMatch {
                return Err(DomainError::Validation(
                    "segment rules cannot nest segmentMatch clauses in Phase 2".to_string(),
                ));
            }
        }
        if let Some(w) = rule.weight {
            if w > ROLLOUT_TOTAL_WEIGHT {
                return Err(DomainError::Validation(format!(
                    "segment rule weight {w} exceeds {ROLLOUT_TOTAL_WEIGHT}"
                )));
            }
        }
        if let Some(kind) = &rule.rollout_context_kind {
            validate_context_kind(kind)?;
        }
        if let Some(bucket_by) = &rule.bucket_by {
            AttributeRef::parse(bucket_by)?;
        }
    }
    Ok(())
}

fn add_context_values(
    legacy_user_list: &mut Vec<String>,
    context_lists: &mut Vec<SegmentTarget>,
    context_kind: &str,
    values: &[String],
) -> Result<(), DomainError> {
    validate_context_kind(context_kind)?;
    if context_kind == "user" {
        for val in values {
            if !val.is_empty() && !legacy_user_list.contains(val) {
                legacy_user_list.push(val.clone());
            }
        }
    } else if let Some(existing) = context_lists
        .iter_mut()
        .find(|t| t.context_kind == context_kind)
    {
        for val in values {
            if !val.is_empty() && !existing.values.contains(val) {
                existing.values.push(val.clone());
            }
        }
    } else {
        let mut deduped = Vec::new();
        for val in values {
            if !val.is_empty() && !deduped.contains(val) {
                deduped.push(val.clone());
            }
        }
        if !deduped.is_empty() {
            context_lists.push(SegmentTarget {
                context_kind: context_kind.to_string(),
                values: deduped,
            });
        }
    }
    Ok(())
}

fn remove_context_values(
    legacy_user_list: &mut Vec<String>,
    context_lists: &mut Vec<SegmentTarget>,
    context_kind: &str,
    values: &[String],
) -> Result<(), DomainError> {
    validate_context_kind(context_kind)?;
    if context_kind == "user" {
        legacy_user_list.retain(|v| !values.contains(v));
    }
    if let Some(existing) = context_lists
        .iter_mut()
        .find(|t| t.context_kind == context_kind)
    {
        existing.values.retain(|v| !values.contains(v));
    }
    context_lists.retain(|t| !t.values.is_empty());
    Ok(())
}

/// Applies semantic `SegmentPatchInstruction`s deterministically and validates the result.
pub fn apply_segment_patch(
    segment: &Segment,
    instructions: &[SegmentPatchInstruction],
) -> Result<SegmentPatchResult, DomainError> {
    if instructions.is_empty() {
        return Err(DomainError::Validation(
            "segment patch must contain at least one instruction".to_string(),
        ));
    }

    let mut next = segment.clone();
    let mut summaries = Vec::with_capacity(instructions.len());

    for instr in instructions {
        match instr {
            SegmentPatchInstruction::UpdateName { name } => {
                if name.trim().is_empty() {
                    return Err(DomainError::Validation(
                        "segment name cannot be empty".to_string(),
                    ));
                }
                next.name = name.trim().to_string();
                summaries.push(PatchSummaryItem {
                    kind: "updateName".to_string(),
                    summary: format!("Rename segment to '{}'", next.name),
                });
            }
            SegmentPatchInstruction::AddIncluded {
                context_kind,
                values,
            } => {
                add_context_values(
                    &mut next.included,
                    &mut next.included_contexts,
                    context_kind,
                    values,
                )?;
                summaries.push(PatchSummaryItem {
                    kind: "addIncluded".to_string(),
                    summary: format!("Include {} {} key(s)", values.len(), context_kind),
                });
            }
            SegmentPatchInstruction::RemoveIncluded {
                context_kind,
                values,
            } => {
                remove_context_values(
                    &mut next.included,
                    &mut next.included_contexts,
                    context_kind,
                    values,
                )?;
                summaries.push(PatchSummaryItem {
                    kind: "removeIncluded".to_string(),
                    summary: format!("Remove {} included {} key(s)", values.len(), context_kind),
                });
            }
            SegmentPatchInstruction::AddExcluded {
                context_kind,
                values,
            } => {
                add_context_values(
                    &mut next.excluded,
                    &mut next.excluded_contexts,
                    context_kind,
                    values,
                )?;
                summaries.push(PatchSummaryItem {
                    kind: "addExcluded".to_string(),
                    summary: format!("Exclude {} {} key(s)", values.len(), context_kind),
                });
            }
            SegmentPatchInstruction::RemoveExcluded {
                context_kind,
                values,
            } => {
                remove_context_values(
                    &mut next.excluded,
                    &mut next.excluded_contexts,
                    context_kind,
                    values,
                )?;
                summaries.push(PatchSummaryItem {
                    kind: "removeExcluded".to_string(),
                    summary: format!("Remove {} excluded {} key(s)", values.len(), context_kind),
                });
            }
            SegmentPatchInstruction::AddRule {
                id,
                clauses,
                weight,
                bucket_by,
                rollout_context_kind,
            } => {
                let rule_id = id
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("srule_{}", Uuid::now_v7()));
                if next.rules.iter().any(|r| r.id == rule_id) {
                    return Err(DomainError::Validation(format!(
                        "segment rule '{rule_id}' already exists"
                    )));
                }
                next.rules.push(SegmentRule {
                    id: rule_id.clone(),
                    clauses: clauses.clone(),
                    weight: *weight,
                    bucket_by: bucket_by.clone(),
                    rollout_context_kind: rollout_context_kind.clone(),
                });
                summaries.push(PatchSummaryItem {
                    kind: "addRule".to_string(),
                    summary: format!("Add segment rule '{rule_id}'"),
                });
            }
            SegmentPatchInstruction::UpdateRule {
                rule_id,
                clauses,
                weight,
                bucket_by,
                rollout_context_kind,
            } => {
                let rule = next
                    .rules
                    .iter_mut()
                    .find(|r| r.id == *rule_id)
                    .ok_or_else(|| DomainError::RuleNotFound(rule_id.clone()))?;
                rule.clauses = clauses.clone();
                rule.weight = *weight;
                rule.bucket_by = bucket_by.clone();
                rule.rollout_context_kind = rollout_context_kind.clone();
                summaries.push(PatchSummaryItem {
                    kind: "updateRule".to_string(),
                    summary: format!("Update segment rule '{rule_id}'"),
                });
            }
            SegmentPatchInstruction::RemoveRule { rule_id } => {
                let before = next.rules.len();
                next.rules.retain(|r| r.id != *rule_id);
                if next.rules.len() == before {
                    return Err(DomainError::RuleNotFound(rule_id.clone()));
                }
                summaries.push(PatchSummaryItem {
                    kind: "removeRule".to_string(),
                    summary: format!("Remove segment rule '{rule_id}'"),
                });
            }
        }
    }

    validate_segment(&next)?;
    Ok(SegmentPatchResult {
        segment: next,
        summaries,
    })
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;

    use super::*;
    use crate::entities::{ClauseOp, SegmentKind};

    #[test]
    fn patches_segment_targets_and_rules_validating_weights() {
        let seg = Segment {
            id: Uuid::now_v7(),
            env_id: Uuid::now_v7(),
            key: "beta-users".to_string(),
            name: "Beta Users".to_string(),
            kind: SegmentKind::Standard,
            version: 1,
            salt: "seg-salt".to_string(),
            included: vec![],
            excluded: vec![],
            included_contexts: vec![],
            excluded_contexts: vec![],
            rules: vec![],
            updated_at: Utc::now(),
        };

        let res = apply_segment_patch(
            &seg,
            &[
                SegmentPatchInstruction::AddIncluded {
                    context_kind: "user".to_string(),
                    values: vec!["u1".to_string(), "u2".to_string()],
                },
                SegmentPatchInstruction::AddExcluded {
                    context_kind: "organization".to_string(),
                    values: vec!["org_blocked".to_string()],
                },
                SegmentPatchInstruction::AddRule {
                    id: Some("r1".to_string()),
                    clauses: vec![Clause {
                        context_kind: "user".to_string(),
                        attribute: "email".to_string(),
                        op: ClauseOp::EndsWith,
                        values: vec![json!("@skotos.dev")],
                        negate: false,
                    }],
                    weight: Some(50_000),
                    bucket_by: Some("key".to_string()),
                    rollout_context_kind: Some("user".to_string()),
                },
            ],
        )
        .expect("valid segment patch");

        assert_eq!(res.segment.included, vec!["u1", "u2"]);
        assert_eq!(res.segment.excluded_contexts.len(), 1);
        assert_eq!(res.segment.rules.len(), 1);

        // Weight > 100_000 must fail
        let bad = apply_segment_patch(
            &res.segment,
            &[SegmentPatchInstruction::UpdateRule {
                rule_id: "r1".to_string(),
                clauses: vec![],
                weight: Some(100_001),
                bucket_by: None,
                rollout_context_kind: None,
            }],
        );
        assert!(bad.is_err());
    }
}
