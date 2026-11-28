use std::convert::Infallible;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use bytes::Bytes;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use futures::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use skotos_auth::{Action, ResourceRef};
use skotos_domain::{
    Application, ApplicationKind, ApplicationVersion, ApprovalRequest, ApprovalReviewKind,
    AuditArchiveRecord, BigSegmentStatus, CodeRefHunk, CustomRole, Environment,
    EnvironmentApprovalSettings, FlagCodeRefSummary, FlagPatchInstruction, FlagRelease,
    FlagWorkflow, PolicyStatement, ReleasePhase, ReleasePipeline, ScheduledSegmentChange,
    SegmentPatchInstruction, SsoProtocol, SsoProviderConfig, Team, WorkflowStage,
};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::dto::PatchFlagResponse;
use crate::extract::{AppState, AuthPrincipal};
use crate::problem::{ApiError, ProblemDetails};

// =============================================================================
// Phase 3 DTOs
// =============================================================================

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CustomRoleListResponse {
    pub items: Vec<CustomRole>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateCustomRoleRequest {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub statements: Vec<PolicyStatement>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCustomRoleRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub statements: Option<Vec<PolicyStatement>>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssignMemberRolesRequest {
    pub role_keys: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssignMemberRolesResponse {
    pub member_id: Uuid,
    pub role_keys: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TeamListResponse {
    pub items: Vec<Team>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateTeamRequest {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub custom_role_keys: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTeamRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub custom_role_keys: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalListQuery {
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalListResponse {
    pub items: Vec<ApprovalRequest>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateApprovalRequestDto {
    #[serde(default)]
    pub description: String,
    pub instructions: Vec<FlagPatchInstruction>,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewApprovalRequestDto {
    pub kind: ApprovalReviewKind,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplyApprovalResponse {
    pub approval: ApprovalRequest,
    pub patch: PatchFlagResponse,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SsoProviderListResponse {
    pub items: Vec<SsoProviderConfig>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateSsoProviderRequest {
    pub protocol: SsoProtocol,
    pub name: String,
    pub issuer_url: String,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub jwks_url: Option<String>,
    #[serde(default)]
    pub saml_metadata_url: Option<String>,
    #[serde(default)]
    pub scim_enabled: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportAuditArchiveRequest {
    #[serde(default)]
    pub from: Option<DateTime<Utc>>,
    #[serde(default)]
    pub to: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditArchiveListResponse {
    pub items: Vec<AuditArchiveRecord>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowListResponse {
    pub items: Vec<FlagWorkflow>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorkflowRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub stages: Vec<WorkflowStage>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdvanceWorkflowRequest {
    #[serde(default)]
    pub approve_gate: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReleasePipelineListResponse {
    pub items: Vec<ReleasePipeline>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateReleasePipelineRequest {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub phases: Vec<ReleasePhase>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AttachReleasePipelineRequest {
    pub pipeline_key: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationListResponse {
    pub items: Vec<Application>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateApplicationRequest {
    pub key: String,
    pub name: String,
    pub kind: ApplicationKind,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub maintainer_team_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddApplicationVersionRequest {
    pub version_key: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub supported: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BigSegmentMembersRequest {
    #[serde(default = "default_user_kind")]
    pub context_kind: String,
    #[serde(default)]
    pub add_included: Vec<String>,
    #[serde(default)]
    pub remove_included: Vec<String>,
    #[serde(default)]
    pub add_excluded: Vec<String>,
    #[serde(default)]
    pub remove_excluded: Vec<String>,
}

fn default_user_kind() -> String {
    "user".to_string()
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BigSegmentMembershipCheckResponse {
    pub segment_key: String,
    pub context_kind: String,
    pub context_key: String,
    pub membership: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledSegmentListResponse {
    pub items: Vec<ScheduledSegmentChange>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateScheduledSegmentRequest {
    pub execute_at: DateTime<Utc>,
    pub instructions: Vec<SegmentPatchInstruction>,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpsertCodeRefsRequest {
    pub repo_name: String,
    pub branch: String,
    #[serde(default)]
    pub commit_sha: String,
    #[serde(default)]
    pub hunks: Vec<CodeRefHunk>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CodeRefListResponse {
    pub items: Vec<FlagCodeRefSummary>,
}

// =============================================================================
// Phase 3 Router Mount
// =============================================================================

pub fn phase3_router() -> Router<AppState> {
    Router::new()
        // Custom RBAC Roles
        .route("/api/v1/roles", get(list_custom_roles).post(create_custom_role))
        .route(
            "/api/v1/roles/{roleKey}",
            get(get_custom_role)
                .patch(update_custom_role)
                .delete(delete_custom_role),
        )
        .route("/api/v1/members/{memberId}/roles", put(assign_member_roles))
        // Teams
        .route("/api/v1/teams", get(list_teams).post(create_team))
        .route(
            "/api/v1/teams/{teamKey}",
            axum::routing::patch(update_team).delete(delete_team),
        )
        .route(
            "/api/v1/teams/{teamKey}/members/{memberId}",
            post(add_team_member).delete(remove_team_member),
        )
        // Environment Approval Settings & Approval Workflows
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}/approval-settings",
            put(update_env_approval_settings),
        )
        .route("/api/v1/approvals", get(list_approvals))
        .route(
            "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/approvals",
            get(list_flag_approvals).post(create_flag_approval),
        )
        .route(
            "/api/v1/approvals/{approvalId}",
            get(get_approval).delete(cancel_approval),
        )
        .route(
            "/api/v1/approvals/{approvalId}/reviews",
            post(review_approval),
        )
        .route("/api/v1/approvals/{approvalId}/apply", post(apply_approval))
        // Enterprise SSO / OIDC / SAML & SCIM 2.0 Provisioning
        .route(
            "/api/v1/sso/providers",
            get(list_sso_providers).post(create_sso_provider),
        )
        .route("/api/v1/sso/providers/{providerId}", delete(delete_sso_provider))
        .route("/scim/v2/Users", get(scim_list_users).post(scim_create_user))
        .route("/scim/v2/Groups", get(scim_list_groups))
        // Audit Log R2/S3 Zstd Archival
        .route("/api/v1/audit-log/export", post(export_audit_archive))
        .route("/api/v1/audit-log/archives", get(list_audit_archives))
        // Multi-Step Flag Workflows
        .route(
            "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/workflows",
            get(list_workflows).post(create_workflow),
        )
        .route(
            "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/workflows/{workflowId}/advance",
            post(advance_workflow),
        )
        .route(
            "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/workflows/{workflowId}",
            delete(cancel_workflow),
        )
        // Release Pipelines & Flag Releases
        .route(
            "/api/v1/projects/{projectKey}/release-pipelines",
            get(list_release_pipelines).post(create_release_pipeline),
        )
        .route(
            "/api/v1/projects/{projectKey}/release-pipelines/{pipelineKey}",
            delete(delete_release_pipeline),
        )
        .route(
            "/api/v1/projects/{projectKey}/flags/{flagKey}/release",
            get(get_flag_release).post(attach_flag_release),
        )
        .route(
            "/api/v1/projects/{projectKey}/flags/{flagKey}/release/phases/{phaseIndex}/start",
            post(start_flag_release_phase),
        )
        // Applications & App Versions
        .route(
            "/api/v1/applications",
            get(list_applications).post(create_application),
        )
        .route(
            "/api/v1/applications/{appKey}/versions",
            post(add_application_version),
        )
        // Big Segments & Scheduled Segment Changes
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}/segments/{segmentKey}/members",
            post(mutate_big_segment_members),
        )
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}/segments/{segmentKey}/membership/{contextKey}",
            get(check_big_segment_membership),
        )
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}/segments/{segmentKey}/schedules",
            get(list_scheduled_segment_changes).post(create_scheduled_segment_change),
        )
        // Code References (skotos-code-refs CI scanner)
        .route(
            "/api/v1/projects/{projectKey}/code-refs",
            post(upsert_code_refs),
        )
        .route(
            "/api/v1/projects/{projectKey}/flags/{flagKey}/code-refs",
            get(list_flag_code_refs),
        )
        // Live Events Debugger (ephemeral NATS tap)
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}/live-events",
            get(stream_env_live_events),
        )
}

// =============================================================================
// Custom RBAC Roles Handlers
// =============================================================================

#[utoipa::path(
    get,
    path = "/api/v1/roles",
    tag = "governance",
    responses((status = 200, body = CustomRoleListResponse))
)]
pub async fn list_custom_roles(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
) -> Result<Json<CustomRoleListResponse>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::Read, &ResourceRef::org())?;
    let items = state.storage.list_custom_roles(principal.org_id).await?;
    Ok(Json(CustomRoleListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/roles",
    tag = "governance",
    request_body = CreateCustomRoleRequest,
    responses((status = 201, body = CustomRole), (status = 409, body = ProblemDetails))
)]
pub async fn create_custom_role(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Json(body): Json<CreateCustomRoleRequest>,
) -> Result<(StatusCode, Json<CustomRole>), ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageRoles, &ResourceRef::org())?;
    let role = state
        .storage
        .create_custom_role(
            principal.org_id,
            &body.key,
            &body.name,
            &body.description,
            &body.statements,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(role)))
}

#[utoipa::path(
    get,
    path = "/api/v1/roles/{roleKey}",
    tag = "governance",
    params(("roleKey" = String, Path, description = "Custom role key")),
    responses((status = 200, body = CustomRole))
)]
pub async fn get_custom_role(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(role_key): Path<String>,
) -> Result<Json<CustomRole>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::Read, &ResourceRef::org())?;
    let role = state
        .storage
        .get_custom_role(principal.org_id, &role_key)
        .await?;
    Ok(Json(role))
}

#[utoipa::path(
    patch,
    path = "/api/v1/roles/{roleKey}",
    tag = "governance",
    params(("roleKey" = String, Path, description = "Custom role key")),
    request_body = UpdateCustomRoleRequest,
    responses((status = 200, body = CustomRole))
)]
pub async fn update_custom_role(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(role_key): Path<String>,
    Json(body): Json<UpdateCustomRoleRequest>,
) -> Result<Json<CustomRole>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageRoles, &ResourceRef::org())?;
    let role = state
        .storage
        .update_custom_role(
            principal.org_id,
            &role_key,
            body.name.as_deref(),
            body.description.as_deref(),
            body.statements.as_deref(),
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(Json(role))
}

#[utoipa::path(
    delete,
    path = "/api/v1/roles/{roleKey}",
    tag = "governance",
    params(("roleKey" = String, Path, description = "Custom role key")),
    responses((status = 204))
)]
pub async fn delete_custom_role(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(role_key): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageRoles, &ResourceRef::org())?;
    state
        .storage
        .delete_custom_role(principal.org_id, &role_key, &principal.to_audit_actor())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    put,
    path = "/api/v1/members/{memberId}/roles",
    tag = "governance",
    params(("memberId" = Uuid, Path, description = "Member UUID")),
    request_body = AssignMemberRolesRequest,
    responses((status = 200, body = AssignMemberRolesResponse))
)]
pub async fn assign_member_roles(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(member_id): Path<Uuid>,
    Json(body): Json<AssignMemberRolesRequest>,
) -> Result<Json<AssignMemberRolesResponse>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageRoles, &ResourceRef::org())?;
    let role_keys = state
        .storage
        .assign_member_custom_roles(
            principal.org_id,
            member_id,
            &body.role_keys,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(Json(AssignMemberRolesResponse {
        member_id,
        role_keys,
    }))
}

// =============================================================================
// Teams Handlers
// =============================================================================

#[utoipa::path(
    get,
    path = "/api/v1/teams",
    tag = "governance",
    responses((status = 200, body = TeamListResponse))
)]
pub async fn list_teams(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
) -> Result<Json<TeamListResponse>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::Read, &ResourceRef::org())?;
    let items = state.storage.list_teams(principal.org_id).await?;
    Ok(Json(TeamListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/teams",
    tag = "governance",
    request_body = CreateTeamRequest,
    responses((status = 201, body = Team))
)]
pub async fn create_team(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Json(body): Json<CreateTeamRequest>,
) -> Result<(StatusCode, Json<Team>), ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageTeams, &ResourceRef::org())?;
    let team = state
        .storage
        .create_team(
            principal.org_id,
            &body.key,
            &body.name,
            &body.description,
            &body.custom_role_keys,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(team)))
}

#[utoipa::path(
    patch,
    path = "/api/v1/teams/{teamKey}",
    tag = "governance",
    params(("teamKey" = String, Path, description = "Team key")),
    request_body = UpdateTeamRequest,
    responses((status = 200, body = Team))
)]
pub async fn update_team(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(team_key): Path<String>,
    Json(body): Json<UpdateTeamRequest>,
) -> Result<Json<Team>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageTeams, &ResourceRef::org())?;
    let team = state
        .storage
        .update_team_roles_and_meta(
            principal.org_id,
            &team_key,
            body.name.as_deref(),
            body.description.as_deref(),
            body.custom_role_keys.as_deref(),
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(Json(team))
}

#[utoipa::path(
    delete,
    path = "/api/v1/teams/{teamKey}",
    tag = "governance",
    params(("teamKey" = String, Path, description = "Team key")),
    responses((status = 204))
)]
pub async fn delete_team(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(team_key): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageTeams, &ResourceRef::org())?;
    state
        .storage
        .delete_team(principal.org_id, &team_key, &principal.to_audit_actor())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/v1/teams/{teamKey}/members/{memberId}",
    tag = "governance",
    params(("teamKey" = String, Path, description = "Team key"), ("memberId" = Uuid, Path, description = "Member UUID")),
    responses((status = 204))
)]
pub async fn add_team_member(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((team_key, member_id)): Path<(String, Uuid)>,
) -> Result<StatusCode, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageTeams, &ResourceRef::org())?;
    state
        .storage
        .add_team_member(
            principal.org_id,
            &team_key,
            member_id,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete,
    path = "/api/v1/teams/{teamKey}/members/{memberId}",
    tag = "governance",
    params(("teamKey" = String, Path, description = "Team key"), ("memberId" = Uuid, Path, description = "Member UUID")),
    responses((status = 204))
)]
pub async fn remove_team_member(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((team_key, member_id)): Path<(String, Uuid)>,
) -> Result<StatusCode, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageTeams, &ResourceRef::org())?;
    state
        .storage
        .remove_team_member(
            principal.org_id,
            &team_key,
            member_id,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// =============================================================================
// Approval Settings & Approval Workflows Handlers
// =============================================================================

#[utoipa::path(
    put,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/approval-settings",
    tag = "approvals",
    params(("projectKey" = String, Path, description = "Project key"), ("envKey" = String, Path, description = "Environment key")),
    request_body = EnvironmentApprovalSettings,
    responses((status = 200, body = Environment))
)]
pub async fn update_env_approval_settings(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key)): Path<(String, String)>,
    Json(body): Json<EnvironmentApprovalSettings>,
) -> Result<Json<Environment>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::ManageEnvironment,
        &ResourceRef::environment(&project_key, &env_key),
    )?;
    let env = state
        .storage
        .update_environment_approval_settings(
            principal.org_id,
            &project_key,
            &env_key,
            &body,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(Json(env))
}

#[utoipa::path(
    get,
    path = "/api/v1/approvals",
    tag = "approvals",
    params(ApprovalListQuery),
    responses((status = 200, body = ApprovalListResponse))
)]
pub async fn list_approvals(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Query(query): Query<ApprovalListQuery>,
) -> Result<Json<ApprovalListResponse>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::Read, &ResourceRef::org())?;
    let items = state
        .storage
        .list_approval_requests(principal.org_id, query.status.as_deref())
        .await?;
    Ok(Json(ApprovalListResponse { items }))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/approvals",
    tag = "approvals",
    params(("projectKey" = String, Path, description = "Project key"), ("flagKey" = String, Path, description = "Flag key"), ("envKey" = String, Path, description = "Environment key")),
    responses((status = 200, body = ApprovalListResponse))
)]
pub async fn list_flag_approvals(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, env_key)): Path<(String, String, String)>,
) -> Result<Json<ApprovalListResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::flag_in_env(&project_key, &env_key, &flag_key),
    )?;
    let items = state
        .storage
        .list_flag_approval_requests(principal.org_id, &project_key, &flag_key, &env_key)
        .await?;
    Ok(Json(ApprovalListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/approvals",
    tag = "approvals",
    params(("projectKey" = String, Path, description = "Project key"), ("flagKey" = String, Path, description = "Flag key"), ("envKey" = String, Path, description = "Environment key")),
    request_body = CreateApprovalRequestDto,
    responses((status = 201, body = ApprovalRequest))
)]
pub async fn create_flag_approval(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, env_key)): Path<(String, String, String)>,
    Json(body): Json<CreateApprovalRequestDto>,
) -> Result<(StatusCode, Json<ApprovalRequest>), ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::flag_in_env(&project_key, &env_key, &flag_key),
    )?;
    let req = state
        .storage
        .create_approval_request(
            principal.org_id,
            &project_key,
            &flag_key,
            &env_key,
            &body.description,
            &body.instructions,
            body.comment.as_deref(),
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(req)))
}

#[utoipa::path(
    get,
    path = "/api/v1/approvals/{approvalId}",
    tag = "approvals",
    params(("approvalId" = Uuid, Path, description = "Approval request UUID")),
    responses((status = 200, body = ApprovalRequest))
)]
pub async fn get_approval(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(approval_id): Path<Uuid>,
) -> Result<Json<ApprovalRequest>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::Read, &ResourceRef::org())?;
    let req = state
        .storage
        .get_approval_request(principal.org_id, approval_id)
        .await?;
    Ok(Json(req))
}

#[utoipa::path(
    post,
    path = "/api/v1/approvals/{approvalId}/reviews",
    tag = "approvals",
    params(("approvalId" = Uuid, Path, description = "Approval request UUID")),
    request_body = ReviewApprovalRequestDto,
    responses((status = 200, body = ApprovalRequest), (status = 422, body = ProblemDetails))
)]
pub async fn review_approval(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(approval_id): Path<Uuid>,
    Json(body): Json<ReviewApprovalRequestDto>,
) -> Result<Json<ApprovalRequest>, ApiError> {
    let existing = state
        .storage
        .get_approval_request(principal.org_id, approval_id)
        .await?;
    state.authorizer.authorize(
        &principal,
        Action::ApproveChange,
        &ResourceRef::flag_in_env(&existing.project_key, &existing.env_key, &existing.flag_key),
    )?;
    let reviewer_id = principal.member_id.ok_or_else(|| {
        ApiError::bad_request(
            "member_required",
            "approval reviews must be submitted by an authenticated organization member",
        )
    })?;
    let updated = state
        .storage
        .review_approval_request(
            principal.org_id,
            approval_id,
            reviewer_id,
            principal.email.as_deref(),
            body.kind,
            body.comment.as_deref(),
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(Json(updated))
}

#[utoipa::path(
    post,
    path = "/api/v1/approvals/{approvalId}/apply",
    tag = "approvals",
    params(("approvalId" = Uuid, Path, description = "Approval request UUID")),
    responses((status = 200, body = ApplyApprovalResponse), (status = 422, body = ProblemDetails))
)]
pub async fn apply_approval(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(approval_id): Path<Uuid>,
) -> Result<Json<ApplyApprovalResponse>, ApiError> {
    let existing = state
        .storage
        .get_approval_request(principal.org_id, approval_id)
        .await?;
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::flag_in_env(&existing.project_key, &existing.env_key, &existing.flag_key),
    )?;
    let (approval, outcome) = state
        .storage
        .apply_approval_request(principal.org_id, approval_id, &principal.to_audit_actor())
        .await?;
    Ok(Json(ApplyApprovalResponse {
        approval,
        patch: PatchFlagResponse {
            flag: outcome.flag,
            env_config: outcome.env_config,
            summaries: outcome.summaries,
        },
    }))
}

#[utoipa::path(
    delete,
    path = "/api/v1/approvals/{approvalId}",
    tag = "approvals",
    params(("approvalId" = Uuid, Path, description = "Approval request UUID")),
    responses((status = 204))
)]
pub async fn cancel_approval(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(approval_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::PatchFlag, &ResourceRef::org())?;
    state
        .storage
        .cancel_approval_request(principal.org_id, approval_id, &principal.to_audit_actor())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// =============================================================================
// Enterprise SSO, SCIM 2.0 & Audit Archival Handlers
// =============================================================================

#[utoipa::path(
    get,
    path = "/api/v1/sso/providers",
    tag = "governance",
    responses((status = 200, body = SsoProviderListResponse))
)]
pub async fn list_sso_providers(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
) -> Result<Json<SsoProviderListResponse>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::Read, &ResourceRef::org())?;
    let items = state.storage.list_sso_providers(principal.org_id).await?;
    Ok(Json(SsoProviderListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/sso/providers",
    tag = "governance",
    request_body = CreateSsoProviderRequest,
    responses((status = 201, body = SsoProviderConfig))
)]
pub async fn create_sso_provider(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Json(body): Json<CreateSsoProviderRequest>,
) -> Result<(StatusCode, Json<SsoProviderConfig>), ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageSso, &ResourceRef::org())?;
    let cfg = state
        .storage
        .create_sso_provider(
            principal.org_id,
            body.protocol,
            &body.name,
            &body.issuer_url,
            body.client_id.as_deref(),
            body.jwks_url.as_deref(),
            body.saml_metadata_url.as_deref(),
            body.scim_enabled,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(cfg)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/sso/providers/{providerId}",
    tag = "governance",
    params(("providerId" = Uuid, Path, description = "SSO provider UUID")),
    responses((status = 204))
)]
pub async fn delete_sso_provider(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(provider_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageSso, &ResourceRef::org())?;
    state
        .storage
        .delete_sso_provider(principal.org_id, provider_id, &principal.to_audit_actor())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// SCIM 2.0 `GET /scim/v2/Users` (RFC 7644 directory sync read).
pub async fn scim_list_users(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
) -> Result<Json<Value>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageMembers, &ResourceRef::org())?;
    let members = state.storage.list_members(principal.org_id).await?;
    let resources: Vec<Value> = members
        .into_iter()
        .map(|m| {
            json!({
                "schemas": ["urn:ietf:params:scim:schemas:core:2.0:User"],
                "id": m.id,
                "externalId": m.external_subject_id,
                "userName": m.email,
                "displayName": m.display_name,
                "active": true,
            })
        })
        .collect();
    Ok(Json(json!({
        "schemas": ["urn:ietf:params:scim:api:messages:2.0:ListResponse"],
        "totalResults": resources.len(),
        "Resources": resources,
    })))
}

/// SCIM 2.0 `POST /scim/v2/Users` (RFC 7644 user provisioning).
pub async fn scim_create_user(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageMembers, &ResourceRef::org())?;
    let email = body
        .get("userName")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad_request("scim_invalid", "SCIM userName (email) required"))?;
    let display_name = body
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or(email);
    let external_id = body.get("externalId").and_then(Value::as_str);

    let member = state
        .storage
        .scim_upsert_member(
            principal.org_id,
            email,
            display_name,
            external_id,
            &principal.to_audit_actor(),
        )
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:User"],
            "id": member.id,
            "externalId": member.external_subject_id,
            "userName": member.email,
            "displayName": member.display_name,
            "active": true,
        })),
    ))
}

/// SCIM 2.0 `GET /scim/v2/Groups` (RFC 7644 teams directory sync).
pub async fn scim_list_groups(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
) -> Result<Json<Value>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageTeams, &ResourceRef::org())?;
    let teams = state.storage.list_teams(principal.org_id).await?;
    let resources: Vec<Value> = teams
        .into_iter()
        .map(|t| {
            json!({
                "schemas": ["urn:ietf:params:scim:schemas:core:2.0:Group"],
                "id": t.id,
                "displayName": t.name,
                "externalId": t.key,
                "members": t.member_ids.into_iter().map(|id| json!({ "value": id })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(Json(json!({
        "schemas": ["urn:ietf:params:scim:api:messages:2.0:ListResponse"],
        "totalResults": resources.len(),
        "Resources": resources,
    })))
}

#[utoipa::path(
    post,
    path = "/api/v1/audit-log/export",
    tag = "audit-log",
    request_body = ExportAuditArchiveRequest,
    responses((status = 201, body = AuditArchiveRecord))
)]
pub async fn export_audit_archive(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Json(body): Json<ExportAuditArchiveRequest>,
) -> Result<(StatusCode, Json<AuditArchiveRecord>), ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageProject, &ResourceRef::org())?;
    let to_ts = body.to.unwrap_or_else(Utc::now);
    let from_ts = body
        .from
        .unwrap_or_else(|| to_ts - ChronoDuration::days(30));

    let entries = state
        .storage
        .list_audit_logs_in_range(principal.org_id, from_ts, to_ts, 50_000)
        .await?;
    let archive_id = Uuid::now_v7();
    let store = state
        .snapshots
        .clone()
        .unwrap_or_else(skotos_fanout::SnapshotStore::in_memory);
    let (object_key, compressed_bytes, sha256) = store
        .put_audit_archive(principal.org_id, archive_id, &entries)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "archive_error",
                "Audit Archive Failed",
                e.to_string(),
            )
        })?;

    let record = state
        .storage
        .record_audit_archive(
            principal.org_id,
            &object_key,
            "ndjson.zst",
            entries.len() as i64,
            compressed_bytes,
            &sha256,
            from_ts,
            to_ts,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(record)))
}

#[utoipa::path(
    get,
    path = "/api/v1/audit-log/archives",
    tag = "audit-log",
    responses((status = 200, body = AuditArchiveListResponse))
)]
pub async fn list_audit_archives(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
) -> Result<Json<AuditArchiveListResponse>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::Read, &ResourceRef::org())?;
    let items = state.storage.list_audit_archives(principal.org_id).await?;
    Ok(Json(AuditArchiveListResponse { items }))
}

// =============================================================================
// Multi-Step Flag Workflows, Release Pipelines & Applications Handlers
// =============================================================================

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/workflows",
    tag = "automation",
    params(("projectKey" = String, Path, description = "Project key"), ("flagKey" = String, Path, description = "Flag key"), ("envKey" = String, Path, description = "Environment key")),
    responses((status = 200, body = WorkflowListResponse))
)]
pub async fn list_workflows(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, env_key)): Path<(String, String, String)>,
) -> Result<Json<WorkflowListResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::flag_in_env(&project_key, &env_key, &flag_key),
    )?;
    let items = state
        .storage
        .list_flag_workflows(principal.org_id, &project_key, &flag_key, &env_key)
        .await?;
    Ok(Json(WorkflowListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/workflows",
    tag = "automation",
    params(("projectKey" = String, Path, description = "Project key"), ("flagKey" = String, Path, description = "Flag key"), ("envKey" = String, Path, description = "Environment key")),
    request_body = CreateWorkflowRequest,
    responses((status = 201, body = FlagWorkflow))
)]
pub async fn create_workflow(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, env_key)): Path<(String, String, String)>,
    Json(body): Json<CreateWorkflowRequest>,
) -> Result<(StatusCode, Json<FlagWorkflow>), ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::flag_in_env(&project_key, &env_key, &flag_key),
    )?;
    let wf = state
        .storage
        .create_flag_workflow(
            principal.org_id,
            &project_key,
            &flag_key,
            &env_key,
            &body.name,
            &body.description,
            body.stages,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(wf)))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/workflows/{workflowId}/advance",
    tag = "automation",
    params(("projectKey" = String, Path, description = "Project key"), ("flagKey" = String, Path, description = "Flag key"), ("envKey" = String, Path, description = "Environment key"), ("workflowId" = Uuid, Path, description = "Workflow UUID")),
    request_body = AdvanceWorkflowRequest,
    responses((status = 200, body = FlagWorkflow))
)]
pub async fn advance_workflow(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, env_key, workflow_id)): Path<(String, String, String, Uuid)>,
    Json(body): Json<AdvanceWorkflowRequest>,
) -> Result<Json<FlagWorkflow>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::flag_in_env(&project_key, &env_key, &flag_key),
    )?;
    let wf = state
        .storage
        .advance_flag_workflow(
            principal.org_id,
            workflow_id,
            body.approve_gate,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(Json(wf))
}

#[utoipa::path(
    delete,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/workflows/{workflowId}",
    tag = "automation",
    params(("projectKey" = String, Path, description = "Project key"), ("flagKey" = String, Path, description = "Flag key"), ("envKey" = String, Path, description = "Environment key"), ("workflowId" = Uuid, Path, description = "Workflow UUID")),
    responses((status = 204))
)]
pub async fn cancel_workflow(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, env_key, workflow_id)): Path<(String, String, String, Uuid)>,
) -> Result<StatusCode, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::flag_in_env(&project_key, &env_key, &flag_key),
    )?;
    state
        .storage
        .cancel_flag_workflow(principal.org_id, workflow_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/release-pipelines",
    tag = "releases",
    params(("projectKey" = String, Path, description = "Project key")),
    responses((status = 200, body = ReleasePipelineListResponse))
)]
pub async fn list_release_pipelines(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(project_key): Path<String>,
) -> Result<Json<ReleasePipelineListResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::project(&project_key),
    )?;
    let items = state
        .storage
        .list_release_pipelines(principal.org_id, &project_key)
        .await?;
    Ok(Json(ReleasePipelineListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/release-pipelines",
    tag = "releases",
    params(("projectKey" = String, Path, description = "Project key")),
    request_body = CreateReleasePipelineRequest,
    responses((status = 201, body = ReleasePipeline))
)]
pub async fn create_release_pipeline(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(project_key): Path<String>,
    Json(body): Json<CreateReleasePipelineRequest>,
) -> Result<(StatusCode, Json<ReleasePipeline>), ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::ManagePipelines,
        &ResourceRef::project(&project_key),
    )?;
    let pipeline = state
        .storage
        .create_release_pipeline(
            principal.org_id,
            &project_key,
            &body.key,
            &body.name,
            &body.description,
            &body.phases,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(pipeline)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/projects/{projectKey}/release-pipelines/{pipelineKey}",
    tag = "releases",
    params(("projectKey" = String, Path, description = "Project key"), ("pipelineKey" = String, Path, description = "Pipeline key")),
    responses((status = 204))
)]
pub async fn delete_release_pipeline(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, pipeline_key)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::ManagePipelines,
        &ResourceRef::project(&project_key),
    )?;
    state
        .storage
        .delete_release_pipeline(
            principal.org_id,
            &project_key,
            &pipeline_key,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/release",
    tag = "releases",
    params(("projectKey" = String, Path, description = "Project key"), ("flagKey" = String, Path, description = "Flag key")),
    responses((status = 200, body = Option<FlagRelease>))
)]
pub async fn get_flag_release(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key)): Path<(String, String)>,
) -> Result<Json<Option<FlagRelease>>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::flag(&project_key, &flag_key),
    )?;
    let release = state
        .storage
        .get_flag_release(principal.org_id, &project_key, &flag_key)
        .await?;
    Ok(Json(release))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/release",
    tag = "releases",
    params(("projectKey" = String, Path, description = "Project key"), ("flagKey" = String, Path, description = "Flag key")),
    request_body = AttachReleasePipelineRequest,
    responses((status = 201, body = FlagRelease))
)]
pub async fn attach_flag_release(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key)): Path<(String, String)>,
    Json(body): Json<AttachReleasePipelineRequest>,
) -> Result<(StatusCode, Json<FlagRelease>), ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::flag(&project_key, &flag_key),
    )?;
    let release = state
        .storage
        .attach_flag_release(
            principal.org_id,
            &project_key,
            &flag_key,
            &body.pipeline_key,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(release)))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/release/phases/{phaseIndex}/start",
    tag = "releases",
    params(("projectKey" = String, Path, description = "Project key"), ("flagKey" = String, Path, description = "Flag key"), ("phaseIndex" = usize, Path, description = "Zero-based phase index")),
    responses((status = 200, body = FlagRelease))
)]
pub async fn start_flag_release_phase(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, phase_index)): Path<(String, String, usize)>,
) -> Result<Json<FlagRelease>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::flag(&project_key, &flag_key),
    )?;
    let release = state
        .storage
        .start_flag_release_phase(
            principal.org_id,
            &project_key,
            &flag_key,
            phase_index,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(Json(release))
}

#[utoipa::path(
    get,
    path = "/api/v1/applications",
    tag = "releases",
    responses((status = 200, body = ApplicationListResponse))
)]
pub async fn list_applications(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
) -> Result<Json<ApplicationListResponse>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::Read, &ResourceRef::org())?;
    let items = state.storage.list_applications(principal.org_id).await?;
    Ok(Json(ApplicationListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/applications",
    tag = "releases",
    request_body = CreateApplicationRequest,
    responses((status = 201, body = Application))
)]
pub async fn create_application(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Json(body): Json<CreateApplicationRequest>,
) -> Result<(StatusCode, Json<Application>), ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::PatchFlag, &ResourceRef::org())?;
    let app = state
        .storage
        .create_application(
            principal.org_id,
            &body.key,
            &body.name,
            body.kind,
            &body.description,
            body.maintainer_team_id,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(app)))
}

#[utoipa::path(
    post,
    path = "/api/v1/applications/{appKey}/versions",
    tag = "releases",
    params(("appKey" = String, Path, description = "Application key")),
    request_body = AddApplicationVersionRequest,
    responses((status = 201, body = ApplicationVersion))
)]
pub async fn add_application_version(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(app_key): Path<String>,
    Json(body): Json<AddApplicationVersionRequest>,
) -> Result<(StatusCode, Json<ApplicationVersion>), ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::PatchFlag, &ResourceRef::org())?;
    let ver = state
        .storage
        .add_application_version(
            principal.org_id,
            &app_key,
            &body.version_key,
            &body.name,
            body.supported,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(ver)))
}

// =============================================================================
// Big Segments, Scheduled Segment Changes, Code Refs & Live Events Debugger
// =============================================================================

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/segments/{segmentKey}/members",
    tag = "segments",
    params(("projectKey" = String, Path, description = "Project key"), ("envKey" = String, Path, description = "Environment key"), ("segmentKey" = String, Path, description = "Segment key")),
    request_body = BigSegmentMembersRequest,
    responses((status = 200, body = BigSegmentStatus))
)]
pub async fn mutate_big_segment_members(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key, segment_key)): Path<(String, String, String)>,
    Json(body): Json<BigSegmentMembersRequest>,
) -> Result<Json<BigSegmentStatus>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::segment(&project_key, &env_key, &segment_key),
    )?;
    let status = state
        .storage
        .mutate_big_segment_members(
            principal.org_id,
            &project_key,
            &env_key,
            &segment_key,
            &body.context_kind,
            &body.add_included,
            &body.remove_included,
            &body.add_excluded,
            &body.remove_excluded,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(Json(status))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/segments/{segmentKey}/membership/{contextKey}",
    tag = "segments",
    params(("projectKey" = String, Path, description = "Project key"), ("envKey" = String, Path, description = "Environment key"), ("segmentKey" = String, Path, description = "Segment key"), ("contextKey" = String, Path, description = "Context key")),
    responses((status = 200, body = BigSegmentMembershipCheckResponse))
)]
pub async fn check_big_segment_membership(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key, segment_key, context_key)): Path<(String, String, String, String)>,
) -> Result<Json<BigSegmentMembershipCheckResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::segment(&project_key, &env_key, &segment_key),
    )?;
    let membership = state
        .storage
        .check_big_segment_member(
            principal.org_id,
            &project_key,
            &env_key,
            &segment_key,
            "user",
            &context_key,
        )
        .await?;
    Ok(Json(BigSegmentMembershipCheckResponse {
        segment_key,
        context_kind: "user".to_string(),
        context_key,
        membership,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/segments/{segmentKey}/schedules",
    tag = "segments",
    params(("projectKey" = String, Path, description = "Project key"), ("envKey" = String, Path, description = "Environment key"), ("segmentKey" = String, Path, description = "Segment key")),
    responses((status = 200, body = ScheduledSegmentListResponse))
)]
pub async fn list_scheduled_segment_changes(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key, segment_key)): Path<(String, String, String)>,
) -> Result<Json<ScheduledSegmentListResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::segment(&project_key, &env_key, &segment_key),
    )?;
    let items = state
        .storage
        .list_scheduled_segment_changes(principal.org_id, &project_key, &env_key, &segment_key)
        .await?;
    Ok(Json(ScheduledSegmentListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/segments/{segmentKey}/schedules",
    tag = "segments",
    params(("projectKey" = String, Path, description = "Project key"), ("envKey" = String, Path, description = "Environment key"), ("segmentKey" = String, Path, description = "Segment key")),
    request_body = CreateScheduledSegmentRequest,
    responses((status = 201, body = ScheduledSegmentChange))
)]
pub async fn create_scheduled_segment_change(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key, segment_key)): Path<(String, String, String)>,
    Json(body): Json<CreateScheduledSegmentRequest>,
) -> Result<(StatusCode, Json<ScheduledSegmentChange>), ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::segment(&project_key, &env_key, &segment_key),
    )?;
    let sched = state
        .storage
        .create_scheduled_segment_change(
            principal.org_id,
            &project_key,
            &env_key,
            &segment_key,
            body.execute_at,
            &body.instructions,
            body.comment.as_deref(),
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(sched)))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/code-refs",
    tag = "devtools",
    params(("projectKey" = String, Path, description = "Project key")),
    request_body = UpsertCodeRefsRequest,
    responses((status = 200, body = CodeRefListResponse))
)]
pub async fn upsert_code_refs(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(project_key): Path<String>,
    Json(body): Json<UpsertCodeRefsRequest>,
) -> Result<Json<CodeRefListResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::project(&project_key),
    )?;
    let items = state
        .storage
        .upsert_code_refs(
            principal.org_id,
            &project_key,
            &body.repo_name,
            &body.branch,
            &body.commit_sha,
            &body.hunks,
        )
        .await?;
    Ok(Json(CodeRefListResponse { items }))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/code-refs",
    tag = "devtools",
    params(("projectKey" = String, Path, description = "Project key"), ("flagKey" = String, Path, description = "Flag key")),
    responses((status = 200, body = CodeRefListResponse))
)]
pub async fn list_flag_code_refs(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key)): Path<(String, String)>,
) -> Result<Json<CodeRefListResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::flag(&project_key, &flag_key),
    )?;
    let items = state
        .storage
        .list_flag_code_refs(principal.org_id, &project_key, &flag_key)
        .await?;
    Ok(Json(CodeRefListResponse { items }))
}

/// Phase 3 Live Events Debugger SSE endpoint for Dashboard operators (`GET .../live-events`).
pub async fn stream_env_live_events(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::ViewLiveEvents,
        &ResourceRef::environment(&project_key, &env_key),
    )?;
    let env = state
        .storage
        .get_environment(principal.org_id, &project_key, &env_key)
        .await?;

    let initial = Bytes::from(format!(
        "event: connected\ndata: {{\"envId\":\"{}\",\"envKey\":\"{}\"}}\n\n",
        env.id, env.key
    ));
    let keepalive = tokio::time::interval(Duration::from_secs(15));

    if let Some(nats) = &state.nats {
        if let Ok(sub) = nats.subscribe_debug_events(env.id).await {
            let body_stream = stream::unfold(
                (Some(initial), sub, keepalive),
                |(mut init, mut sub, mut ka)| async move {
                    if let Some(first) = init.take() {
                        let _ = ka.tick().await;
                        return Some((Ok::<Bytes, Infallible>(first), (init, sub, ka)));
                    }
                    tokio::select! {
                        maybe_msg = sub.next() => {
                            let msg = maybe_msg?;
                            let text = String::from_utf8_lossy(&msg.payload);
                            let frame = Bytes::from(format!("event: debug\ndata: {text}\n\n"));
                            Some((Ok(frame), (init, sub, ka)))
                        }
                        _ = ka.tick() => {
                            Some((Ok(Bytes::from_static(b": keepalive\n\n")), (init, sub, ka)))
                        }
                    }
                },
            );
            return Ok(Response::builder()
                .status(StatusCode::OK)
                .header(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("text/event-stream; charset=utf-8"),
                )
                .header(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("no-cache, no-transform"),
                )
                .header("x-accel-buffering", HeaderValue::from_static("no"))
                .body(Body::from_stream(body_stream))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()));
        }
    }

    // Fallback when NATS is offline: still emit connected event + periodic keepalive
    let body_stream = stream::unfold(
        (Some(initial), keepalive),
        |(mut init, mut ka)| async move {
            if let Some(first) = init.take() {
                let _ = ka.tick().await;
                return Some((Ok::<Bytes, Infallible>(first), (init, ka)));
            }
            let _ = ka.tick().await;
            Some((Ok(Bytes::from_static(b": keepalive\n\n")), (init, ka)))
        },
    );
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream; charset=utf-8"),
        )
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache, no-transform"),
        )
        .body(Body::from_stream(body_stream))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()))
}
