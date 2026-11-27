use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use skotos_domain::{
    ApiToken, AuditLogEntry, BuiltInRole, ClientSideAvailability, Context, Environment,
    EnvironmentKey, EnvironmentKeyKind, EvaluationReason, FeatureFlag, FlagEnvConfig,
    FlagEnvRevision, FlagKind, FlagPatchInstruction, Member, Organization, PatchSummaryItem,
    Project,
};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

// =============================================================================
// Health & Auth DTOs
// =============================================================================

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: String,
    pub service: String,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfigResponse {
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oidc_issuer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oidc_client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oidc_audience: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthMeResponse {
    pub organization: Organization,
    pub actor_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_token_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    pub display_name: String,
    pub role: BuiltInRole,
}

// =============================================================================
// Projects DTOs
// =============================================================================

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectListResponse {
    pub items: Vec<Project>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectRequest {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub default_client_side_availability: ClientSideAvailability,
    #[serde(default = "default_true")]
    pub seed_default_environments: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectResponse {
    pub project: Project,
    pub environments: Vec<Environment>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProjectRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub default_client_side_availability: Option<ClientSideAvailability>,
}

// =============================================================================
// Environments & Keys DTOs
// =============================================================================

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentListResponse {
    pub items: Vec<Environment>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateEnvironmentRequest {
    pub key: String,
    pub name: String,
    #[serde(default = "default_env_color")]
    pub color: String,
    #[serde(default)]
    pub require_comments: bool,
    #[serde(default)]
    pub confirm_changes: bool,
}

fn default_env_color() -> String {
    "#3B82F6".to_string()
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEnvironmentRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub require_comments: Option<bool>,
    #[serde(default)]
    pub confirm_changes: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentKeysResponse {
    pub client_side_id: String,
    pub keys: Vec<EnvironmentKey>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RotateKeyRequest {
    pub kind: EnvironmentKeyKind,
    #[serde(default)]
    pub grace_period_seconds: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RotateKeyResponse {
    pub key: EnvironmentKey,
    pub plaintext_key: String,
}

// =============================================================================
// Feature Flags, Patching, Simulation & Revisions DTOs
// =============================================================================

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FlagResponse {
    #[serde(flatten)]
    pub flag: FeatureFlag,
    pub environments: HashMap<String, FlagEnvConfig>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FlagListResponse {
    pub items: Vec<FlagResponse>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListFlagsQuery {
    #[serde(default)]
    pub include_archived: bool,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateVariationInput {
    #[serde(default)]
    pub id: Option<Uuid>,
    pub value: Value,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateFlagRequest {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub kind: FlagKind,
    pub variations: Vec<CreateVariationInput>,
    #[serde(default = "default_true")]
    pub temporary: bool,
    #[serde(default)]
    pub client_side_availability: Option<ClientSideAvailability>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub maintainer_id: Option<Uuid>,
    #[serde(default)]
    pub default_on_variation_index: Option<usize>,
    #[serde(default)]
    pub default_off_variation_index: Option<usize>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchFlagRequest {
    pub environment_key: String,
    #[serde(default)]
    pub expected_version: Option<i64>,
    #[serde(default)]
    pub comment: Option<String>,
    pub instructions: Vec<FlagPatchInstruction>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchFlagResponse {
    pub flag: FeatureFlag,
    pub env_config: FlagEnvConfig,
    pub summaries: Vec<PatchSummaryItem>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SimulateFlagRequest {
    pub context: Context,
    #[serde(default)]
    pub draft_instructions: Option<Vec<FlagPatchInstruction>>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SimulateFlagResponse {
    pub flag_key: String,
    pub environment_key: String,
    pub value: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variation_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variation_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variation_name: Option<String>,
    pub reason: EvaluationReason,
    pub used_draft: bool,
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListRevisionsQuery {
    #[serde(default = "default_revisions_limit")]
    pub limit: i64,
}

fn default_revisions_limit() -> i64 {
    50
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FlagRevisionsResponse {
    pub items: Vec<FlagEnvRevision>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RestoreRevisionRequest {
    #[serde(default)]
    pub comment: Option<String>,
}

// =============================================================================
// Audit Log, API Tokens & Members DTOs
// =============================================================================

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogQuery {
    #[serde(default)]
    pub project_key: Option<String>,
    #[serde(default)]
    pub env_key: Option<String>,
    #[serde(default)]
    pub resource_type: Option<String>,
    #[serde(default)]
    pub resource_key: Option<String>,
    #[serde(default)]
    pub actor_id: Option<Uuid>,
    #[serde(default = "default_audit_limit")]
    pub limit: i64,
    #[serde(default)]
    pub cursor: Option<String>,
}

fn default_audit_limit() -> i64 {
    50
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogListResponse {
    pub items: Vec<AuditLogEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiTokenListResponse {
    pub items: Vec<ApiToken>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiTokenRequest {
    pub name: String,
    pub role: BuiltInRole,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiTokenResponse {
    pub token: ApiToken,
    pub plaintext_token: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemberListResponse {
    pub items: Vec<Member>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateMemberRequest {
    pub email: String,
    pub display_name: String,
    pub role: BuiltInRole,
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMemberRequest {
    pub role: BuiltInRole,
}
