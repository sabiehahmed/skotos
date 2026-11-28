use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::error::DomainError;

/// Validates a resource slug/key (projects, environments, flags, segments).
/// Must be 1..=128 ASCII chars matching `^[a-zA-Z0-9][a-zA-Z0-9._-]*$`.
pub fn validate_key(key: &str) -> Result<(), DomainError> {
    if key.is_empty() || key.len() > 128 {
        return Err(DomainError::InvalidKey {
            key: key.to_string(),
            reason: "key must be between 1 and 128 characters".to_string(),
        });
    }
    let bytes = key.as_bytes();
    if !bytes[0].is_ascii_alphanumeric() {
        return Err(DomainError::InvalidKey {
            key: key.to_string(),
            reason: "key must start with an ASCII letter or digit".to_string(),
        });
    }
    if !bytes
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(DomainError::InvalidKey {
            key: key.to_string(),
            reason: "key may only contain letters, digits, '.', '_', and '-'".to_string(),
        });
    }
    Ok(())
}

/// Validates a hex color string (`#RRGGBB`).
pub fn validate_hex_color(color: &str) -> Result<(), DomainError> {
    let bytes = color.as_bytes();
    if bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(DomainError::Validation(format!(
            "invalid hex color '{color}': expected #RRGGBB"
        )))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum PlanTier {
    Free,
    Pro,
    Team,
    Enterprise,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum BuiltInRole {
    Reader,
    Writer,
    Admin,
    Owner,
}

impl BuiltInRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Reader => "reader",
            Self::Writer => "writer",
            Self::Admin => "admin",
            Self::Owner => "owner",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct Organization {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_auth_org_id: Option<String>,
    pub plan_tier: PlanTier,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub id: Uuid,
    pub org_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_subject_id: Option<String>,
    pub email: String,
    pub display_name: String,
    pub role: BuiltInRole,
    /// Keys of custom RBAC roles assigned directly to this member (not via teams).
    #[serde(default)]
    pub custom_role_keys: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ClientSideAvailability {
    #[serde(default)]
    pub using_environment_id: bool,
    #[serde(default)]
    pub using_mobile_key: bool,
}

impl ClientSideAvailability {
    pub fn is_default(&self) -> bool {
        !self.using_environment_id && !self.using_mobile_key
    }

    pub fn any_enabled(&self) -> bool {
        self.using_environment_id || self.using_mobile_key
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: Uuid,
    pub org_id: Uuid,
    pub key: String,
    pub name: String,
    pub default_client_side_availability: ClientSideAvailability,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub id: Uuid,
    pub project_id: Uuid,
    pub key: String,
    pub name: String,
    pub color: String,
    pub client_side_id: String,
    pub require_comments: bool,
    pub confirm_changes: bool,
    #[serde(default)]
    pub approval_settings: crate::phase3::EnvironmentApprovalSettings,
    pub revision: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum EnvironmentKeyKind {
    Sdk,
    Mobile,
}

impl EnvironmentKeyKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sdk => "sdk",
            Self::Mobile => "mobile",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentKey {
    pub id: Uuid,
    pub env_id: Uuid,
    pub kind: EnvironmentKeyKind,
    pub key_prefix: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ApiToken {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub key_prefix: String,
    pub role: BuiltInRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum FlagKind {
    Boolean,
    String,
    Number,
    Json,
}

impl FlagKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Boolean => "boolean",
            Self::String => "string",
            Self::Number => "number",
            Self::Json => "json",
        }
    }

    pub fn matches_value(&self, value: &Value) -> bool {
        match self {
            Self::Boolean => value.is_boolean(),
            Self::String => value.is_string(),
            Self::Number => value.is_number(),
            Self::Json => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct Variation {
    pub id: Uuid,
    pub value: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct FeatureFlag {
    pub id: Uuid,
    pub project_id: Uuid,
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub kind: FlagKind,
    pub variations: Vec<Variation>,
    pub temporary: bool,
    pub client_side_availability: ClientSideAvailability,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maintainer_id: Option<Uuid>,
    pub archived: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl FeatureFlag {
    pub fn variation_index_by_id(&self, id: Uuid) -> Option<usize> {
        self.variations.iter().position(|v| v.id == id)
    }

    pub fn validate_variations(&self) -> Result<(), DomainError> {
        if self.variations.len() < 2 {
            return Err(DomainError::InvalidVariation(
                "flag must have at least 2 variations".to_string(),
            ));
        }
        if self.kind == FlagKind::Boolean && self.variations.len() != 2 {
            return Err(DomainError::InvalidVariation(
                "boolean flag must have exactly 2 variations".to_string(),
            ));
        }
        let mut seen = std::collections::HashSet::with_capacity(self.variations.len());
        for var in &self.variations {
            if !seen.insert(var.id) {
                return Err(DomainError::DuplicateVariationId(var.id));
            }
            if !self.kind.matches_value(&var.value) {
                return Err(DomainError::InvalidVariation(format!(
                    "variation '{}' value does not match flag kind '{}'",
                    var.id,
                    self.kind.as_str()
                )));
            }
        }
        Ok(())
    }
}

/// Exact 15 canonical clause operators locked in PROJECT.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum ClauseOp {
    In,
    StartsWith,
    EndsWith,
    Contains,
    Matches,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
    Before,
    After,
    SemverEqual,
    SemverLessThan,
    SemverGreaterThan,
    SegmentMatch,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct Clause {
    #[serde(default = "default_context_kind")]
    pub context_kind: String,
    pub attribute: String,
    pub op: ClauseOp,
    #[serde(default)]
    pub values: Vec<Value>,
    #[serde(default)]
    pub negate: bool,
}

pub fn default_context_kind() -> String {
    "user".to_string()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum RolloutKind {
    #[default]
    Rollout,
    Experiment,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct WeightedVariation {
    pub variation_id: Uuid,
    /// Integer basis points in `0..=100_000` (0.001% precision).
    pub weight: u32,
    #[serde(default)]
    pub untracked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct Rollout {
    #[serde(default = "default_context_kind")]
    pub context_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(default)]
    pub kind: RolloutKind,
    pub variations: Vec<WeightedVariation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct VariationOrRollout {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variation_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollout: Option<Rollout>,
}

impl VariationOrRollout {
    pub fn variation(id: Uuid) -> Self {
        Self {
            variation_id: Some(id),
            rollout: None,
        }
    }

    pub fn rollout(rollout: Rollout) -> Self {
        Self {
            variation_id: None,
            rollout: Some(rollout),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct Prerequisite {
    pub flag_key: String,
    pub variation_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct Target {
    #[serde(default = "default_context_kind")]
    pub context_kind: String,
    #[serde(default)]
    pub values: Vec<String>,
    pub variation_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct TargetingRule {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub clauses: Vec<Clause>,
    #[serde(flatten)]
    pub variation_or_rollout: VariationOrRollout,
    #[serde(default)]
    pub track_events: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct FlagEnvConfigBody {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off_variation_id: Option<Uuid>,
    #[serde(default)]
    pub prerequisites: Vec<Prerequisite>,
    #[serde(default)]
    pub targets: Vec<Target>,
    #[serde(default)]
    pub rules: Vec<TargetingRule>,
    pub fallthrough: VariationOrRollout,
    #[serde(default)]
    pub track_events: bool,
    #[serde(default)]
    pub track_events_fallthrough: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug_events_until_date: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct FlagEnvConfig {
    pub flag_id: Uuid,
    pub env_id: Uuid,
    pub version: i64,
    pub on: bool,
    pub salt: String,
    pub config: FlagEnvConfigBody,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct FlagEnvRevision {
    pub id: Uuid,
    pub flag_id: Uuid,
    pub env_id: Uuid,
    pub version: i64,
    pub on: bool,
    pub config: FlagEnvConfigBody,
    pub instructions: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum SegmentKind {
    #[default]
    Standard,
    Big,
    Synced,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SegmentTarget {
    #[serde(default = "default_context_kind")]
    pub context_kind: String,
    #[serde(default)]
    pub values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SegmentRule {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub clauses: Vec<Clause>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollout_context_kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub id: Uuid,
    pub env_id: Uuid,
    pub key: String,
    pub name: String,
    pub kind: SegmentKind,
    pub version: i64,
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
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AuditLogEntry {
    pub id: Uuid,
    pub org_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_id: Option<Uuid>,
    pub resource_type: String,
    pub resource_key: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<Uuid>,
    pub actor_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct OutboxEvent {
    pub id: Uuid,
    pub org_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_id: Option<Uuid>,
    pub event_type: String,
    pub payload: Value,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum ScheduledChangeStatus {
    Pending,
    Executed,
    Failed,
    Canceled,
}

impl ScheduledChangeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Executed => "executed",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ScheduledChange {
    pub id: Uuid,
    pub org_id: Uuid,
    pub flag_id: Uuid,
    pub env_id: Uuid,
    pub execute_at: DateTime<Utc>,
    pub instructions: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<Uuid>,
    pub status: ScheduledChangeStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum TriggerAction {
    TurnFlagOn,
    TurnFlagOff,
}

impl TriggerAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TurnFlagOn => "turnFlagOn",
            Self::TurnFlagOff => "turnFlagOff",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct FlagTrigger {
    pub id: Uuid,
    pub flag_id: Uuid,
    pub env_id: Uuid,
    pub action: TriggerAction,
    pub token_prefix: String,
    #[serde(default)]
    pub description: String,
    pub enabled: bool,
    pub trigger_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_triggered_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct Webhook {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub url: String,
    pub enabled: bool,
    #[serde(default)]
    pub integration_kind: crate::phase3::WebhookIntegrationKind,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub env_filter: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct WebhookDelivery {
    pub id: Uuid,
    pub webhook_id: Uuid,
    pub event_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_code: Option<i32>,
    pub attempt: i32,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub delivered_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct VariationUsageCount {
    pub variation_index: i32,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct FlagEvalUsage {
    pub flag_key: String,
    pub total_evaluations: i64,
    #[serde(default)]
    pub variations: Vec<VariationUsageCount>,
    #[serde(default)]
    pub context_kinds: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_evaluated_at: Option<DateTime<Utc>>,
}

/// Auth registry record exported to R2/S3 and NATS so `delivery-edge` and `event-ingest`
/// authenticate SDK/Mobile keys and Client-Side IDs with zero PostgreSQL calls on the hot path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentAuthRecord {
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub env_id: Uuid,
    pub env_key: String,
    pub client_side_id: String,
    pub keys: Vec<EnvironmentKeyHashRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentKeyHashRecord {
    pub key_id: Uuid,
    pub kind: EnvironmentKeyKind,
    pub key_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}
