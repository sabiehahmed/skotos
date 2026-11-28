use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::entities::ScheduledChangeStatus;
use crate::patch::FlagPatchInstruction;
use crate::segment_patch::SegmentPatchInstruction;

// =============================================================================
// Custom RBAC Resource-Path Allow/Deny Policy Engine
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum PolicyEffect {
    Allow,
    Deny,
}

/// Declarative resource-path RBAC statement (LaunchDarkly-compatible grammar).
///
/// Resource paths follow `proj/{project}:env/{env}:flag/{flag}`,
/// `proj/{project}:env/{env}:segment/{segment}`, `team/{team}`, `role/{role}`,
/// `webhook/{webhook}`, `member/*`, or `*`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PolicyStatement {
    pub effect: PolicyEffect,
    #[serde(default)]
    pub resources: Vec<String>,
    #[serde(default)]
    pub not_resources: Vec<String>,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub not_actions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CustomRole {
    pub id: Uuid,
    pub org_id: Uuid,
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub statements: Vec<PolicyStatement>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// =============================================================================
// Teams
// =============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct Team {
    pub id: Uuid,
    pub org_id: Uuid,
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub custom_role_keys: Vec<String>,
    #[serde(default)]
    pub member_ids: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// =============================================================================
// Mandatory Approval Workflows
// =============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentApprovalSettings {
    #[serde(default)]
    pub required: bool,
    #[serde(default = "default_min_approvals")]
    pub min_approvals: i32,
    #[serde(default)]
    pub allow_self_approval: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_approval_webhook_url: Option<String>,
}

fn default_min_approvals() -> i32 {
    1
}

impl Default for EnvironmentApprovalSettings {
    fn default() -> Self {
        Self {
            required: false,
            min_approvals: 1,
            allow_self_approval: false,
            external_approval_webhook_url: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Applied,
    Canceled,
}

impl ApprovalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Applied => "applied",
            Self::Canceled => "canceled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum ApprovalReviewKind {
    Approve,
    Decline,
}

impl ApprovalReviewKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Decline => "decline",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ApprovalReview {
    pub id: Uuid,
    pub approval_id: Uuid,
    pub reviewer_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer_email: Option<String>,
    pub kind: ApprovalReviewKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_key: String,
    pub env_key: String,
    pub flag_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_by: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_by_email: Option<String>,
    pub description: String,
    pub instructions: Vec<FlagPatchInstruction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    pub status: ApprovalStatus,
    pub min_approvals: i32,
    pub approval_count: i32,
    #[serde(default)]
    pub reviews: Vec<ApprovalReview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_version: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// =============================================================================
// Enterprise SSO / OIDC / SAML Config & Audit Archive Metadata
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum SsoProtocol {
    Oidc,
    Saml,
}

impl SsoProtocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Oidc => "oidc",
            Self::Saml => "saml",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SsoProviderConfig {
    pub id: Uuid,
    pub org_id: Uuid,
    pub protocol: SsoProtocol,
    pub name: String,
    pub issuer_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jwks_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saml_metadata_url: Option<String>,
    pub enabled: bool,
    pub scim_enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AuditArchiveRecord {
    pub id: Uuid,
    pub org_id: Uuid,
    pub object_key: String,
    pub format: String,
    pub record_count: i64,
    pub compressed_bytes: i64,
    pub sha256: String,
    pub from_timestamp: DateTime<Utc>,
    pub to_timestamp: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

// =============================================================================
// Release Automation: Multi-Step Flag Workflows, Pipelines & Applications
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum WorkflowStatus {
    Active,
    WaitingApproval,
    Completed,
    Failed,
    Canceled,
}

impl WorkflowStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::WaitingApproval => "waiting_approval",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum WorkflowStageStatus {
    Pending,
    WaitingApproval,
    Executed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct WorkflowStage {
    pub name: String,
    #[serde(default)]
    pub wait_seconds: i64,
    #[serde(default)]
    pub require_approval: bool,
    pub instructions: Vec<FlagPatchInstruction>,
    #[serde(default = "default_stage_status")]
    pub status: WorkflowStageStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

fn default_stage_status() -> WorkflowStageStatus {
    WorkflowStageStatus::Pending
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct FlagWorkflow {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_key: String,
    pub env_key: String,
    pub flag_key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub status: WorkflowStatus,
    pub current_stage_index: i32,
    pub next_run_at: DateTime<Utc>,
    pub stages: Vec<WorkflowStage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ReleasePhase {
    pub name: String,
    pub environment_key: String,
    #[serde(default)]
    pub instructions: Vec<FlagPatchInstruction>,
    #[serde(default)]
    pub copy_from_previous_env: bool,
    #[serde(default)]
    pub require_approval: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ReleasePipeline {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub phases: Vec<ReleasePhase>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct FlagReleasePhaseState {
    pub phase_index: i32,
    pub name: String,
    pub environment_key: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct FlagRelease {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_key: String,
    pub flag_key: String,
    pub pipeline_key: String,
    pub pipeline_name: String,
    pub status: String,
    pub current_phase_index: i32,
    pub phases: Vec<FlagReleasePhaseState>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum ApplicationKind {
    Browser,
    Mobile,
    Server,
}

impl ApplicationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Browser => "browser",
            Self::Mobile => "mobile",
            Self::Server => "server",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ApplicationVersion {
    pub id: Uuid,
    pub version_key: String,
    pub name: String,
    pub supported: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct Application {
    pub id: Uuid,
    pub org_id: Uuid,
    pub key: String,
    pub name: String,
    pub kind: ApplicationKind,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maintainer_team_id: Option<Uuid>,
    #[serde(default)]
    pub versions: Vec<ApplicationVersion>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// =============================================================================
// Big Segments (Compressed Hash-Block Membership) & Scheduled Segment Changes
// =============================================================================

/// Compact sorted 64-bit hash membership snapshot stored Zstd-compressed in R2/S3
/// for high-scale Big Segments (millions of keys, O(log N) lookup, ~8 bytes/member).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BigSegmentSnapshot {
    pub env_id: Uuid,
    pub segment_key: String,
    pub context_kind: String,
    pub generation: i64,
    /// Sorted FNV-1a 64-bit hashes of included context keys.
    pub included_hashes: Vec<u64>,
    /// Sorted FNV-1a 64-bit hashes of excluded context keys.
    pub excluded_hashes: Vec<u64>,
}

/// Deterministic 64-bit FNV-1a hash for compact Big Segment membership storage.
pub fn big_segment_key_hash(context_kind: &str, context_key: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in context_kind
        .as_bytes()
        .iter()
        .chain(std::iter::once(&b':'))
        .chain(context_key.as_bytes())
    {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

impl BigSegmentSnapshot {
    /// Returns `Some(true)` if explicitly included, `Some(false)` if explicitly excluded,
    /// or `None` if not present in the Big Segment membership sets.
    pub fn check_membership(&self, context_kind: &str, context_key: &str) -> Option<bool> {
        if self.context_kind != context_kind {
            return None;
        }
        let h = big_segment_key_hash(context_kind, context_key);
        if self.included_hashes.binary_search(&h).is_ok() {
            return Some(true);
        }
        if self.excluded_hashes.binary_search(&h).is_ok() {
            return Some(false);
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct BigSegmentStatus {
    pub segment_key: String,
    pub generation: i64,
    pub included_count: i64,
    pub excluded_count: i64,
    pub synced_to_object_store: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ScheduledSegmentChange {
    pub id: Uuid,
    pub org_id: Uuid,
    pub segment_id: Uuid,
    pub env_id: Uuid,
    pub execute_at: DateTime<Utc>,
    pub instructions: Vec<SegmentPatchInstruction>,
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

// =============================================================================
// Developer Ecosystem: Code References, Integrations & Live Debugger Events
// =============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CodeRefHunk {
    pub flag_key: String,
    pub file_path: String,
    pub line_number: i32,
    pub line_preview: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct FlagCodeRefSummary {
    pub flag_key: String,
    pub repo_name: String,
    pub branch: String,
    pub commit_sha: String,
    pub reference_count: i64,
    pub extinct: bool,
    pub hunks: Vec<CodeRefHunk>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum WebhookIntegrationKind {
    #[default]
    Generic,
    Slack,
    Teams,
    Datadog,
    PagerDuty,
    Jira,
    Linear,
}

impl WebhookIntegrationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::Slack => "slack",
            Self::Teams => "teams",
            Self::Datadog => "datadog",
            Self::PagerDuty => "pagerduty",
            Self::Jira => "jira",
            Self::Linear => "linear",
        }
    }

    pub fn from_db(s: &str) -> Self {
        match s {
            "slack" => Self::Slack,
            "teams" => Self::Teams,
            "datadog" => Self::Datadog,
            "pagerduty" => Self::PagerDuty,
            "jira" => Self::Jira,
            "linear" => Self::Linear,
            _ => Self::Generic,
        }
    }
}

/// Ephemeral event published onto `skotos.debug.{env_id}` for the Live Events Debugger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LiveEvalDebugEvent {
    pub timestamp: DateTime<Utc>,
    pub env_id: Uuid,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flag_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variation_index: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(default)]
    pub context_kinds: Vec<String>,
    #[serde(default)]
    pub count: i64,
}
