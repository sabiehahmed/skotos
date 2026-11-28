use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use skotos_auth::{Action, ResourceRef};
use skotos_domain::{
    FlagEnvConfig, FlagEvalUsage, FlagPatchInstruction, FlagTrigger, PatchSummaryItem,
    ScheduledChange, Segment, SegmentKind, SegmentPatchInstruction, SegmentRule, SegmentTarget,
    TriggerAction, Webhook, WebhookDelivery,
};
use skotos_storage::CreateSegmentInput;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::dto::PatchFlagResponse;
use crate::extract::{parse_if_match_version, AppState, AuthPrincipal};
use crate::problem::{ApiError, ProblemDetails};

// =============================================================================
// Phase 2 DTOs
// =============================================================================

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SegmentListResponse {
    pub items: Vec<Segment>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateSegmentRequest {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub kind: SegmentKind,
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

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchSegmentRequest {
    #[serde(default)]
    pub expected_version: Option<i64>,
    #[serde(default)]
    pub comment: Option<String>,
    pub instructions: Vec<SegmentPatchInstruction>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchSegmentResponse {
    pub segment: Segment,
    pub summaries: Vec<PatchSummaryItem>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CopyFlagEnvRequest {
    pub source_environment_key: String,
    pub target_environment_key: String,
    #[serde(default)]
    pub copy_on_state: bool,
    #[serde(default)]
    pub expected_version: Option<i64>,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BulkPatchFlagsRequest {
    pub flag_keys: Vec<String>,
    pub instructions: Vec<FlagPatchInstruction>,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BulkPatchFlagsResponse {
    pub items: Vec<PatchFlagResponse>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledChangeListResponse {
    pub items: Vec<ScheduledChange>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateScheduledChangeRequest {
    pub execute_at: DateTime<Utc>,
    pub instructions: Vec<FlagPatchInstruction>,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FlagTriggerListResponse {
    pub items: Vec<FlagTrigger>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateFlagTriggerRequest {
    pub action: TriggerAction,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateFlagTriggerResponse {
    pub trigger: FlagTrigger,
    pub plaintext_token: String,
    pub trigger_path: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteTriggerResponse {
    pub flag_key: String,
    pub environment_key: String,
    pub action: TriggerAction,
    pub new_version: i64,
    pub on: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WebhookListResponse {
    pub items: Vec<Webhook>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateWebhookRequest {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub integration_kind: skotos_domain::WebhookIntegrationKind,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub env_filter: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateWebhookResponse {
    pub webhook: Webhook,
    pub secret: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWebhookRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub events: Option<Vec<String>>,
    #[serde(default)]
    pub env_filter: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WebhookDeliveryListResponse {
    pub items: Vec<WebhookDelivery>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct UsageQuery {
    #[serde(default = "default_lookback_days")]
    pub days: i64,
}

fn default_lookback_days() -> i64 {
    30
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentUsageResponse {
    pub items: Vec<FlagEvalUsage>,
}

// =============================================================================
// Phase 2 Router Mount
// =============================================================================

pub fn phase2_router() -> Router<AppState> {
    Router::new()
        // Segments
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}/segments",
            get(list_segments).post(create_segment),
        )
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}/segments/{segmentKey}",
            get(get_segment).patch(patch_segment).delete(delete_segment),
        )
        // Env Copy & Bulk Targeting
        .route(
            "/api/v1/projects/{projectKey}/flags/{flagKey}/copy",
            post(copy_flag_env),
        )
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}/flags/bulk-patch",
            post(bulk_patch_flags),
        )
        // Flag Scheduling
        .route(
            "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/schedules",
            get(list_scheduled_changes).post(create_scheduled_change),
        )
        .route(
            "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/schedules/{scheduleId}",
            delete(cancel_scheduled_change),
        )
        // Flag Triggers
        .route(
            "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/triggers",
            get(list_flag_triggers).post(create_flag_trigger),
        )
        .route(
            "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/triggers/{triggerId}",
            delete(delete_flag_trigger),
        )
        .route("/api/v1/triggers/{token}", post(execute_flag_trigger))
        // Signed Outbound Webhooks
        .route("/api/v1/webhooks", get(list_webhooks).post(create_webhook))
        .route(
            "/api/v1/webhooks/{webhookId}",
            axum::routing::patch(update_webhook).delete(delete_webhook),
        )
        .route(
            "/api/v1/webhooks/{webhookId}/deliveries",
            get(list_webhook_deliveries),
        )
        // Flag Usage & Stale Detection
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}/usage",
            get(get_environment_usage),
        )
        .route(
            "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/usage",
            get(get_flag_usage),
        )
}

// =============================================================================
// Segment Handlers
// =============================================================================

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/segments",
    tag = "segments",
    params(("projectKey" = String, Path, description = "Project key"), ("envKey" = String, Path, description = "Environment key")),
    responses((status = 200, body = SegmentListResponse))
)]
pub async fn list_segments(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key)): Path<(String, String)>,
) -> Result<Json<SegmentListResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::project(&project_key),
    )?;
    let items = state
        .storage
        .list_segments(principal.org_id, &project_key, &env_key)
        .await?;
    Ok(Json(SegmentListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/segments",
    tag = "segments",
    params(("projectKey" = String, Path, description = "Project key"), ("envKey" = String, Path, description = "Environment key")),
    request_body = CreateSegmentRequest,
    responses((status = 201, body = Segment), (status = 409, body = ProblemDetails))
)]
pub async fn create_segment(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key)): Path<(String, String)>,
    Json(body): Json<CreateSegmentRequest>,
) -> Result<(StatusCode, Json<Segment>), ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::CreateFlag,
        &ResourceRef::project(&project_key),
    )?;
    let seg = state
        .storage
        .create_segment(
            principal.org_id,
            &project_key,
            &env_key,
            CreateSegmentInput {
                key: body.key,
                name: body.name,
                kind: body.kind,
                included: body.included,
                excluded: body.excluded,
                included_contexts: body.included_contexts,
                excluded_contexts: body.excluded_contexts,
                rules: body.rules,
            },
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(seg)))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/segments/{segmentKey}",
    tag = "segments",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("envKey" = String, Path, description = "Environment key"),
        ("segmentKey" = String, Path, description = "Segment key")
    ),
    responses((status = 200, body = Segment), (status = 404, body = ProblemDetails))
)]
pub async fn get_segment(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key, segment_key)): Path<(String, String, String)>,
) -> Result<Json<Segment>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::project(&project_key),
    )?;
    let seg = state
        .storage
        .get_segment(principal.org_id, &project_key, &env_key, &segment_key)
        .await?;
    Ok(Json(seg))
}

#[utoipa::path(
    patch,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/segments/{segmentKey}",
    tag = "segments",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("envKey" = String, Path, description = "Environment key"),
        ("segmentKey" = String, Path, description = "Segment key")
    ),
    request_body = PatchSegmentRequest,
    responses((status = 200, body = PatchSegmentResponse), (status = 409, body = ProblemDetails))
)]
pub async fn patch_segment(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key, segment_key)): Path<(String, String, String)>,
    headers: HeaderMap,
    Json(body): Json<PatchSegmentRequest>,
) -> Result<Json<PatchSegmentResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::project(&project_key),
    )?;
    let header_ver = parse_if_match_version(&headers)?;
    let expected = body.expected_version.or(header_ver);

    let outcome = state
        .storage
        .patch_segment(
            principal.org_id,
            &project_key,
            &env_key,
            &segment_key,
            expected,
            body.comment.as_deref(),
            &body.instructions,
            &principal.to_audit_actor(),
        )
        .await?;

    Ok(Json(PatchSegmentResponse {
        segment: outcome.segment,
        summaries: outcome.summaries,
    }))
}

#[utoipa::path(
    delete,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/segments/{segmentKey}",
    tag = "segments",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("envKey" = String, Path, description = "Environment key"),
        ("segmentKey" = String, Path, description = "Segment key")
    ),
    responses((status = 204), (status = 404, body = ProblemDetails))
)]
pub async fn delete_segment(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key, segment_key)): Path<(String, String, String)>,
) -> Result<StatusCode, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::DeleteFlag,
        &ResourceRef::project(&project_key),
    )?;
    state
        .storage
        .delete_segment(
            principal.org_id,
            &project_key,
            &env_key,
            &segment_key,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// =============================================================================
// Environment Copy & Bulk Patch Handlers
// =============================================================================

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/copy",
    tag = "flags",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("flagKey" = String, Path, description = "Flag key")
    ),
    request_body = CopyFlagEnvRequest,
    responses((status = 200, body = FlagEnvConfig), (status = 409, body = ProblemDetails))
)]
pub async fn copy_flag_env(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key)): Path<(String, String)>,
    Json(body): Json<CopyFlagEnvRequest>,
) -> Result<Json<FlagEnvConfig>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::flag(&project_key, &flag_key),
    )?;
    let cfg = state
        .storage
        .copy_flag_env_config(
            principal.org_id,
            &project_key,
            &flag_key,
            &body.source_environment_key,
            &body.target_environment_key,
            body.copy_on_state,
            body.expected_version,
            body.comment.as_deref(),
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(Json(cfg))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/flags/bulk-patch",
    tag = "flags",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("envKey" = String, Path, description = "Environment key")
    ),
    request_body = BulkPatchFlagsRequest,
    responses((status = 200, body = BulkPatchFlagsResponse))
)]
pub async fn bulk_patch_flags(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key)): Path<(String, String)>,
    Json(body): Json<BulkPatchFlagsRequest>,
) -> Result<Json<BulkPatchFlagsResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::project(&project_key),
    )?;
    let outcomes = state
        .storage
        .bulk_patch_flags(
            principal.org_id,
            &project_key,
            &env_key,
            &body.flag_keys,
            &body.instructions,
            body.comment.as_deref(),
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(Json(BulkPatchFlagsResponse {
        items: outcomes
            .into_iter()
            .map(|o| PatchFlagResponse {
                flag: o.flag,
                env_config: o.env_config,
                summaries: o.summaries,
            })
            .collect(),
    }))
}

// =============================================================================
// Flag Scheduling Handlers
// =============================================================================

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/schedules",
    tag = "automation",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("flagKey" = String, Path, description = "Flag key"),
        ("envKey" = String, Path, description = "Environment key")
    ),
    responses((status = 200, body = ScheduledChangeListResponse))
)]
pub async fn list_scheduled_changes(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, env_key)): Path<(String, String, String)>,
) -> Result<Json<ScheduledChangeListResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::flag(&project_key, &flag_key),
    )?;
    let items = state
        .storage
        .list_scheduled_changes(principal.org_id, &project_key, &flag_key, &env_key)
        .await?;
    Ok(Json(ScheduledChangeListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/schedules",
    tag = "automation",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("flagKey" = String, Path, description = "Flag key"),
        ("envKey" = String, Path, description = "Environment key")
    ),
    request_body = CreateScheduledChangeRequest,
    responses((status = 201, body = ScheduledChange))
)]
pub async fn create_scheduled_change(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, env_key)): Path<(String, String, String)>,
    Json(body): Json<CreateScheduledChangeRequest>,
) -> Result<(StatusCode, Json<ScheduledChange>), ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::flag(&project_key, &flag_key),
    )?;
    let sched = state
        .storage
        .create_scheduled_change(
            principal.org_id,
            &project_key,
            &flag_key,
            &env_key,
            body.execute_at,
            &body.instructions,
            body.comment.as_deref(),
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(sched)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/schedules/{scheduleId}",
    tag = "automation",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("flagKey" = String, Path, description = "Flag key"),
        ("envKey" = String, Path, description = "Environment key"),
        ("scheduleId" = Uuid, Path, description = "Scheduled change ID")
    ),
    responses((status = 204), (status = 404, body = ProblemDetails))
)]
pub async fn cancel_scheduled_change(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, _env_key, schedule_id)): Path<(String, String, String, Uuid)>,
) -> Result<StatusCode, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::flag(&project_key, &flag_key),
    )?;
    state
        .storage
        .cancel_scheduled_change(principal.org_id, schedule_id, &principal.to_audit_actor())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// =============================================================================
// Flag Trigger Handlers
// =============================================================================

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/triggers",
    tag = "automation",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("flagKey" = String, Path, description = "Flag key"),
        ("envKey" = String, Path, description = "Environment key")
    ),
    responses((status = 200, body = FlagTriggerListResponse))
)]
pub async fn list_flag_triggers(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, env_key)): Path<(String, String, String)>,
) -> Result<Json<FlagTriggerListResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::flag(&project_key, &flag_key),
    )?;
    let items = state
        .storage
        .list_flag_triggers(principal.org_id, &project_key, &flag_key, &env_key)
        .await?;
    Ok(Json(FlagTriggerListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/triggers",
    tag = "automation",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("flagKey" = String, Path, description = "Flag key"),
        ("envKey" = String, Path, description = "Environment key")
    ),
    request_body = CreateFlagTriggerRequest,
    responses((status = 201, body = CreateFlagTriggerResponse))
)]
pub async fn create_flag_trigger(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, env_key)): Path<(String, String, String)>,
    Json(body): Json<CreateFlagTriggerRequest>,
) -> Result<(StatusCode, Json<CreateFlagTriggerResponse>), ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::flag(&project_key, &flag_key),
    )?;
    let created = state
        .storage
        .create_flag_trigger(
            principal.org_id,
            &project_key,
            &flag_key,
            &env_key,
            body.action,
            &body.description,
            &principal.to_audit_actor(),
        )
        .await?;
    let trigger_path = format!("/api/v1/triggers/{}", created.plaintext_token);
    Ok((
        StatusCode::CREATED,
        Json(CreateFlagTriggerResponse {
            trigger: created.trigger,
            plaintext_token: created.plaintext_token,
            trigger_path,
        }),
    ))
}

#[utoipa::path(
    delete,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/triggers/{triggerId}",
    tag = "automation",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("flagKey" = String, Path, description = "Flag key"),
        ("envKey" = String, Path, description = "Environment key"),
        ("triggerId" = Uuid, Path, description = "Trigger ID")
    ),
    responses((status = 204), (status = 404, body = ProblemDetails))
)]
pub async fn delete_flag_trigger(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, _env_key, trigger_id)): Path<(String, String, String, Uuid)>,
) -> Result<StatusCode, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::flag(&project_key, &flag_key),
    )?;
    state
        .storage
        .delete_flag_trigger(principal.org_id, trigger_id, &principal.to_audit_actor())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/v1/triggers/{token}",
    tag = "automation",
    params(("token" = String, Path, description = "Secret flag trigger token")),
    responses((status = 200, body = ExecuteTriggerResponse), (status = 404, body = ProblemDetails))
)]
pub async fn execute_flag_trigger(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<ExecuteTriggerResponse>, ApiError> {
    let outcome = state.storage.execute_flag_trigger_by_token(&token).await?;
    Ok(Json(ExecuteTriggerResponse {
        flag_key: outcome.flag_key,
        environment_key: outcome.env_key,
        action: outcome.action,
        new_version: outcome.new_version,
        on: outcome.on,
    }))
}

// =============================================================================
// Signed Outbound Webhook Handlers
// =============================================================================

#[utoipa::path(
    get,
    path = "/api/v1/webhooks",
    tag = "webhooks",
    responses((status = 200, body = WebhookListResponse))
)]
pub async fn list_webhooks(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
) -> Result<Json<WebhookListResponse>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::Read, &ResourceRef::org())?;
    let items = state.storage.list_webhooks(principal.org_id).await?;
    Ok(Json(WebhookListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/webhooks",
    tag = "webhooks",
    request_body = CreateWebhookRequest,
    responses((status = 201, body = CreateWebhookResponse))
)]
pub async fn create_webhook(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Json(body): Json<CreateWebhookRequest>,
) -> Result<(StatusCode, Json<CreateWebhookResponse>), ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageEnvironment, &ResourceRef::org())?;
    let mut created = state
        .storage
        .create_webhook(
            principal.org_id,
            &body.name,
            &body.url,
            body.secret.as_deref(),
            body.events,
            body.env_filter,
            &principal.to_audit_actor(),
        )
        .await?;
    if body.integration_kind != skotos_domain::WebhookIntegrationKind::Generic {
        state
            .storage
            .set_webhook_integration_kind(
                principal.org_id,
                created.webhook.id,
                body.integration_kind,
            )
            .await?;
        created.webhook.integration_kind = body.integration_kind;
    }
    Ok((
        StatusCode::CREATED,
        Json(CreateWebhookResponse {
            webhook: created.webhook,
            secret: created.secret,
        }),
    ))
}

#[utoipa::path(
    patch,
    path = "/api/v1/webhooks/{webhookId}",
    tag = "webhooks",
    params(("webhookId" = Uuid, Path, description = "Webhook ID")),
    request_body = UpdateWebhookRequest,
    responses((status = 200, body = Webhook), (status = 404, body = ProblemDetails))
)]
pub async fn update_webhook(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(webhook_id): Path<Uuid>,
    Json(body): Json<UpdateWebhookRequest>,
) -> Result<Json<Webhook>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageEnvironment, &ResourceRef::org())?;
    let wh = state
        .storage
        .update_webhook(
            principal.org_id,
            webhook_id,
            body.name.as_deref(),
            body.url.as_deref(),
            body.enabled,
            body.events,
            body.env_filter,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(Json(wh))
}

#[utoipa::path(
    delete,
    path = "/api/v1/webhooks/{webhookId}",
    tag = "webhooks",
    params(("webhookId" = Uuid, Path, description = "Webhook ID")),
    responses((status = 204), (status = 404, body = ProblemDetails))
)]
pub async fn delete_webhook(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(webhook_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageEnvironment, &ResourceRef::org())?;
    state
        .storage
        .delete_webhook(principal.org_id, webhook_id, &principal.to_audit_actor())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/api/v1/webhooks/{webhookId}/deliveries",
    tag = "webhooks",
    params(("webhookId" = Uuid, Path, description = "Webhook ID")),
    responses((status = 200, body = WebhookDeliveryListResponse))
)]
pub async fn list_webhook_deliveries(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(webhook_id): Path<Uuid>,
) -> Result<Json<WebhookDeliveryListResponse>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::Read, &ResourceRef::org())?;
    let items = state
        .storage
        .list_webhook_deliveries(principal.org_id, webhook_id, 50)
        .await?;
    Ok(Json(WebhookDeliveryListResponse { items }))
}

// =============================================================================
// Flag Usage & Stale Detection Handlers (TelemetryStore)
// =============================================================================

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/usage",
    tag = "usage",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("envKey" = String, Path, description = "Environment key"),
        UsageQuery
    ),
    responses((status = 200, body = EnvironmentUsageResponse))
)]
pub async fn get_environment_usage(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key)): Path<(String, String)>,
    Query(query): Query<UsageQuery>,
) -> Result<Json<EnvironmentUsageResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::project(&project_key),
    )?;
    let env = state
        .storage
        .get_environment(principal.org_id, &project_key, &env_key)
        .await?;
    let items = state
        .telemetry
        .query_environment_usage(principal.org_id, env.id, query.days)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "telemetry_error",
                "Telemetry Query Failed",
                e.to_string(),
            )
        })?;
    Ok(Json(EnvironmentUsageResponse { items }))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/usage",
    tag = "usage",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("flagKey" = String, Path, description = "Flag key"),
        ("envKey" = String, Path, description = "Environment key"),
        UsageQuery
    ),
    responses((status = 200, body = FlagEvalUsage))
)]
pub async fn get_flag_usage(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, env_key)): Path<(String, String, String)>,
    Query(query): Query<UsageQuery>,
) -> Result<Json<FlagEvalUsage>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::flag(&project_key, &flag_key),
    )?;
    let env = state
        .storage
        .get_environment(principal.org_id, &project_key, &env_key)
        .await?;
    let usage = state
        .telemetry
        .query_flag_usage(principal.org_id, env.id, &flag_key, query.days)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "telemetry_error",
                "Telemetry Query Failed",
                e.to_string(),
            )
        })?;
    Ok(Json(usage))
}
