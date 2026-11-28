use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::Value;
use skotos_auth::{hash_password, Action, ResourceRef};
use skotos_domain::{
    apply_flag_patch, compile_eval_flag_with_flags, compile_eval_segment, BuiltInRole, FeatureFlag,
    Variation,
};
use skotos_eval::{evaluate, InMemoryEvalStore};
use skotos_storage::{AuditLogFilter, CreateFlagInput};
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use uuid::Uuid;

use crate::dto::*;
use crate::extract::{build_session_set_cookie, parse_if_match_version, AppState, AuthPrincipal};
use crate::problem::{ApiError, ProblemDetails};
use crate::SERVICE_NAME;

/// Builds the complete Axum HTTP router with CORS, body limits, tracing, and all Phase 1 routes.
pub fn build_router(state: AppState, cors_origins: &[String]) -> Router {
    let allowed_origins: Vec<HeaderValue> = cors_origins
        .iter()
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect();

    let cors = CorsLayer::new()
        .allow_origin(allowed_origins)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::ACCEPT,
            header::IF_MATCH,
        ])
        .allow_credentials(true);

    Router::new()
        // Probes
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        // Auth
        .route("/api/v1/auth/config", get(auth_config))
        .route("/api/v1/auth/login", post(auth_login))
        .route("/api/v1/auth/logout", post(auth_logout))
        .route("/api/v1/auth/me", get(auth_me))
        // Projects
        .route("/api/v1/projects", get(list_projects).post(create_project))
        .route(
            "/api/v1/projects/{projectKey}",
            get(get_project)
                .patch(update_project)
                .delete(delete_project),
        )
        // Environments & Keys
        .route(
            "/api/v1/projects/{projectKey}/environments",
            get(list_environments).post(create_environment),
        )
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}",
            get(get_environment)
                .patch(update_environment)
                .delete(delete_environment),
        )
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}/keys",
            get(list_environment_keys),
        )
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}/keys/rotate",
            post(rotate_environment_key),
        )
        // Feature Flags
        .route(
            "/api/v1/projects/{projectKey}/flags",
            get(list_flags).post(create_flag),
        )
        .route(
            "/api/v1/projects/{projectKey}/flags/{flagKey}",
            get(get_flag).patch(patch_flag).delete(delete_flag),
        )
        .route(
            "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/simulate",
            post(simulate_flag),
        )
        .route(
            "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/revisions",
            get(list_flag_revisions),
        )
        .route(
            "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/restore/{version}",
            post(restore_flag_revision),
        )
        // Audit Log
        .route("/api/v1/audit-log", get(list_audit_log))
        // API Tokens
        .route("/api/v1/tokens", get(list_tokens).post(create_token))
        .route("/api/v1/tokens/{tokenId}", delete(revoke_token))
        // Members
        .route("/api/v1/members", get(list_members).post(create_member))
        .route(
            "/api/v1/members/{memberId}",
            axum::routing::patch(update_member).delete(delete_member),
        )
        .merge(crate::phase2_routes::phase2_router())
        .merge(crate::phase3_routes::phase3_router())
        .merge(crate::phase4_routes::phase4_router())
        .layer(RequestBodyLimitLayer::new(2 * 1024 * 1024))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Complete OpenAPI 3.1 specification exported by `export-openapi`.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Skotos Control Plane API",
        version = "0.1.0",
        description = "Control Plane REST API for managing Skotos organizations, projects, environments, feature flags, targeting rules, segments, Big Segments, custom RBAC roles, teams, approval workflows, release pipelines, workflows, SSO/SCIM, code references, scheduling, triggers, webhooks, usage analytics, experimentation, guarded rollouts, migration flags, synced segments, data export sinks, simulations, revisions, audit logs, API tokens, and members."
    ),
    paths(
        healthz,
        readyz,
        auth_config,
        auth_login,
        auth_logout,
        auth_me,
        list_projects,
        create_project,
        get_project,
        update_project,
        delete_project,
        list_environments,
        create_environment,
        get_environment,
        update_environment,
        delete_environment,
        list_environment_keys,
        rotate_environment_key,
        list_flags,
        create_flag,
        get_flag,
        patch_flag,
        delete_flag,
        simulate_flag,
        list_flag_revisions,
        restore_flag_revision,
        list_audit_log,
        list_tokens,
        create_token,
        revoke_token,
        list_members,
        create_member,
        update_member,
        delete_member,
        crate::phase2_routes::list_segments,
        crate::phase2_routes::create_segment,
        crate::phase2_routes::get_segment,
        crate::phase2_routes::patch_segment,
        crate::phase2_routes::delete_segment,
        crate::phase2_routes::copy_flag_env,
        crate::phase2_routes::bulk_patch_flags,
        crate::phase2_routes::list_scheduled_changes,
        crate::phase2_routes::create_scheduled_change,
        crate::phase2_routes::cancel_scheduled_change,
        crate::phase2_routes::list_flag_triggers,
        crate::phase2_routes::create_flag_trigger,
        crate::phase2_routes::delete_flag_trigger,
        crate::phase2_routes::execute_flag_trigger,
        crate::phase2_routes::list_webhooks,
        crate::phase2_routes::create_webhook,
        crate::phase2_routes::update_webhook,
        crate::phase2_routes::delete_webhook,
        crate::phase2_routes::list_webhook_deliveries,
        crate::phase2_routes::get_environment_usage,
        crate::phase2_routes::get_flag_usage,
        crate::phase3_routes::list_custom_roles,
        crate::phase3_routes::create_custom_role,
        crate::phase3_routes::get_custom_role,
        crate::phase3_routes::update_custom_role,
        crate::phase3_routes::delete_custom_role,
        crate::phase3_routes::assign_member_roles,
        crate::phase3_routes::list_teams,
        crate::phase3_routes::create_team,
        crate::phase3_routes::update_team,
        crate::phase3_routes::delete_team,
        crate::phase3_routes::add_team_member,
        crate::phase3_routes::remove_team_member,
        crate::phase3_routes::update_env_approval_settings,
        crate::phase3_routes::list_approvals,
        crate::phase3_routes::list_flag_approvals,
        crate::phase3_routes::create_flag_approval,
        crate::phase3_routes::get_approval,
        crate::phase3_routes::review_approval,
        crate::phase3_routes::apply_approval,
        crate::phase3_routes::cancel_approval,
        crate::phase3_routes::list_sso_providers,
        crate::phase3_routes::create_sso_provider,
        crate::phase3_routes::delete_sso_provider,
        crate::phase3_routes::export_audit_archive,
        crate::phase3_routes::list_audit_archives,
        crate::phase3_routes::list_workflows,
        crate::phase3_routes::create_workflow,
        crate::phase3_routes::advance_workflow,
        crate::phase3_routes::cancel_workflow,
        crate::phase3_routes::list_release_pipelines,
        crate::phase3_routes::create_release_pipeline,
        crate::phase3_routes::delete_release_pipeline,
        crate::phase3_routes::get_flag_release,
        crate::phase3_routes::attach_flag_release,
        crate::phase3_routes::start_flag_release_phase,
        crate::phase3_routes::list_applications,
        crate::phase3_routes::create_application,
        crate::phase3_routes::add_application_version,
        crate::phase3_routes::mutate_big_segment_members,
        crate::phase3_routes::check_big_segment_membership,
        crate::phase3_routes::list_scheduled_segment_changes,
        crate::phase3_routes::create_scheduled_segment_change,
        crate::phase3_routes::upsert_code_refs,
        crate::phase3_routes::list_flag_code_refs,
        crate::phase4_routes::list_metrics,
        crate::phase4_routes::create_metric,
        crate::phase4_routes::delete_metric,
        crate::phase4_routes::list_metric_groups,
        crate::phase4_routes::create_metric_group,
        crate::phase4_routes::delete_metric_group,
        crate::phase4_routes::list_layers,
        crate::phase4_routes::create_layer,
        crate::phase4_routes::delete_layer,
        crate::phase4_routes::list_experiments,
        crate::phase4_routes::create_experiment,
        crate::phase4_routes::get_experiment,
        crate::phase4_routes::delete_experiment,
        crate::phase4_routes::start_experiment,
        crate::phase4_routes::stop_experiment,
        crate::phase4_routes::get_experiment_results,
        crate::phase4_routes::list_guarded_rollouts,
        crate::phase4_routes::create_guarded_rollout,
        crate::phase4_routes::evaluate_guarded_rollout_handler,
        crate::phase4_routes::rollback_guarded_rollout,
        crate::phase4_routes::get_flag_monitoring,
        crate::phase4_routes::get_engineering_insights,
        crate::phase4_routes::get_migration_config,
        crate::phase4_routes::upsert_migration_config,
        crate::phase4_routes::get_migration_telemetry,
        crate::phase4_routes::list_synced_segments,
        crate::phase4_routes::create_synced_segment,
        crate::phase4_routes::delete_synced_segment,
        crate::phase4_routes::ingest_synced_segment,
        crate::phase4_routes::list_data_export_sinks,
        crate::phase4_routes::create_data_export_sink,
        crate::phase4_routes::delete_data_export_sink,
        crate::phase4_routes::flush_data_export_sink
    ),
    components(schemas(
        ProblemDetails,
        HealthResponse,
        AuthConfigResponse,
        LoginRequest,
        AuthMeResponse,
        ProjectListResponse,
        CreateProjectRequest,
        CreateProjectResponse,
        UpdateProjectRequest,
        EnvironmentListResponse,
        CreateEnvironmentRequest,
        UpdateEnvironmentRequest,
        EnvironmentKeysResponse,
        RotateKeyRequest,
        RotateKeyResponse,
        FlagResponse,
        FlagListResponse,
        CreateVariationInput,
        CreateFlagRequest,
        PatchFlagRequest,
        PatchFlagResponse,
        SimulateFlagRequest,
        SimulateFlagResponse,
        FlagRevisionsResponse,
        RestoreRevisionRequest,
        AuditLogListResponse,
        ApiTokenListResponse,
        CreateApiTokenRequest,
        CreateApiTokenResponse,
        MemberListResponse,
        CreateMemberRequest,
        UpdateMemberRequest,
        crate::phase2_routes::SegmentListResponse,
        crate::phase2_routes::CreateSegmentRequest,
        crate::phase2_routes::PatchSegmentRequest,
        crate::phase2_routes::PatchSegmentResponse,
        crate::phase2_routes::CopyFlagEnvRequest,
        crate::phase2_routes::BulkPatchFlagsRequest,
        crate::phase2_routes::BulkPatchFlagsResponse,
        crate::phase2_routes::ScheduledChangeListResponse,
        crate::phase2_routes::CreateScheduledChangeRequest,
        crate::phase2_routes::FlagTriggerListResponse,
        crate::phase2_routes::CreateFlagTriggerRequest,
        crate::phase2_routes::CreateFlagTriggerResponse,
        crate::phase2_routes::ExecuteTriggerResponse,
        crate::phase2_routes::WebhookListResponse,
        crate::phase2_routes::CreateWebhookRequest,
        crate::phase2_routes::CreateWebhookResponse,
        crate::phase2_routes::UpdateWebhookRequest,
        crate::phase2_routes::WebhookDeliveryListResponse,
        crate::phase2_routes::EnvironmentUsageResponse,
        crate::phase3_routes::CustomRoleListResponse,
        crate::phase3_routes::CreateCustomRoleRequest,
        crate::phase3_routes::UpdateCustomRoleRequest,
        crate::phase3_routes::AssignMemberRolesRequest,
        crate::phase3_routes::AssignMemberRolesResponse,
        crate::phase3_routes::TeamListResponse,
        crate::phase3_routes::CreateTeamRequest,
        crate::phase3_routes::UpdateTeamRequest,
        crate::phase3_routes::ApprovalListResponse,
        crate::phase3_routes::CreateApprovalRequestDto,
        crate::phase3_routes::ReviewApprovalRequestDto,
        crate::phase3_routes::ApplyApprovalResponse,
        crate::phase3_routes::SsoProviderListResponse,
        crate::phase3_routes::CreateSsoProviderRequest,
        crate::phase3_routes::ExportAuditArchiveRequest,
        crate::phase3_routes::AuditArchiveListResponse,
        crate::phase3_routes::WorkflowListResponse,
        crate::phase3_routes::CreateWorkflowRequest,
        crate::phase3_routes::AdvanceWorkflowRequest,
        crate::phase3_routes::ReleasePipelineListResponse,
        crate::phase3_routes::CreateReleasePipelineRequest,
        crate::phase3_routes::AttachReleasePipelineRequest,
        crate::phase3_routes::ApplicationListResponse,
        crate::phase3_routes::CreateApplicationRequest,
        crate::phase3_routes::AddApplicationVersionRequest,
        crate::phase3_routes::BigSegmentMembersRequest,
        crate::phase3_routes::BigSegmentMembershipCheckResponse,
        crate::phase3_routes::ScheduledSegmentListResponse,
        crate::phase3_routes::CreateScheduledSegmentRequest,
        crate::phase3_routes::UpsertCodeRefsRequest,
        crate::phase3_routes::CodeRefListResponse,
        crate::phase4_routes::MetricListResponse,
        crate::phase4_routes::CreateMetricRequest,
        crate::phase4_routes::MetricGroupListResponse,
        crate::phase4_routes::CreateMetricGroupRequest,
        crate::phase4_routes::LayerListResponse,
        crate::phase4_routes::CreateLayerRequest,
        crate::phase4_routes::ExperimentListResponse,
        crate::phase4_routes::CreateExperimentRequest,
        crate::phase4_routes::GuardedRolloutListResponse,
        crate::phase4_routes::CreateGuardedRolloutRequest,
        crate::phase4_routes::UpsertMigrationConfigRequest,
        crate::phase4_routes::SyncedSegmentListResponse,
        crate::phase4_routes::CreateSyncedSegmentRequest,
        crate::phase4_routes::CreateSyncedSegmentResponse,
        crate::phase4_routes::SyncedSegmentIngestRequest,
        crate::phase4_routes::SyncedSegmentIngestResponse,
        crate::phase4_routes::DataExportSinkListResponse,
        crate::phase4_routes::CreateDataExportSinkRequest,
        skotos_domain::Organization,
        skotos_domain::PlanTier,
        skotos_domain::BuiltInRole,
        skotos_domain::Member,
        skotos_domain::ClientSideAvailability,
        skotos_domain::Project,
        skotos_domain::Environment,
        skotos_domain::EnvironmentApprovalSettings,
        skotos_domain::EnvironmentKeyKind,
        skotos_domain::EnvironmentKey,
        skotos_domain::ApiToken,
        skotos_domain::FlagKind,
        skotos_domain::Variation,
        skotos_domain::FeatureFlag,
        skotos_domain::FlagEnvConfig,
        skotos_domain::FlagEnvConfigBody,
        skotos_domain::FlagEnvRevision,
        skotos_domain::Prerequisite,
        skotos_domain::Target,
        skotos_domain::TargetingRule,
        skotos_domain::Clause,
        skotos_domain::ClauseOp,
        skotos_domain::VariationOrRollout,
        skotos_domain::Rollout,
        skotos_domain::RolloutKind,
        skotos_domain::WeightedVariation,
        skotos_domain::FlagPatchInstruction,
        skotos_domain::PatchSummaryItem,
        skotos_domain::Segment,
        skotos_domain::SegmentKind,
        skotos_domain::SegmentTarget,
        skotos_domain::SegmentRule,
        skotos_domain::SegmentPatchInstruction,
        skotos_domain::ScheduledChange,
        skotos_domain::ScheduledChangeStatus,
        skotos_domain::FlagTrigger,
        skotos_domain::TriggerAction,
        skotos_domain::Webhook,
        skotos_domain::WebhookIntegrationKind,
        skotos_domain::WebhookDelivery,
        skotos_domain::FlagEvalUsage,
        skotos_domain::VariationUsageCount,
        skotos_domain::PolicyEffect,
        skotos_domain::PolicyStatement,
        skotos_domain::CustomRole,
        skotos_domain::Team,
        skotos_domain::ApprovalStatus,
        skotos_domain::ApprovalReviewKind,
        skotos_domain::ApprovalReview,
        skotos_domain::ApprovalRequest,
        skotos_domain::SsoProtocol,
        skotos_domain::SsoProviderConfig,
        skotos_domain::AuditArchiveRecord,
        skotos_domain::WorkflowStatus,
        skotos_domain::WorkflowStageStatus,
        skotos_domain::WorkflowStage,
        skotos_domain::FlagWorkflow,
        skotos_domain::ReleasePhase,
        skotos_domain::ReleasePipeline,
        skotos_domain::FlagReleasePhaseState,
        skotos_domain::FlagRelease,
        skotos_domain::ApplicationKind,
        skotos_domain::ApplicationVersion,
        skotos_domain::Application,
        skotos_domain::BigSegmentStatus,
        skotos_domain::ScheduledSegmentChange,
        skotos_domain::CodeRefHunk,
        skotos_domain::FlagCodeRefSummary,
        skotos_domain::LiveEvalDebugEvent,
        skotos_domain::DataExportSinkKind,
        skotos_domain::DataExportSink,
        skotos_domain::DataExportFlushResult,
        skotos_domain::MetricKind,
        skotos_domain::MetricSuccessDirection,
        skotos_domain::MetricAggregation,
        skotos_domain::MetricDefinition,
        skotos_domain::MetricGroupKind,
        skotos_domain::MetricGroup,
        skotos_domain::MutualExclusionLayer,
        skotos_domain::ExperimentMode,
        skotos_domain::StatisticalEngine,
        skotos_domain::ExperimentStatus,
        skotos_domain::ExperimentVariationWeight,
        skotos_domain::Experiment,
        skotos_domain::VariationSufficientStats,
        skotos_domain::SrmCheckResult,
        skotos_domain::VariationResult,
        skotos_domain::MetricExperimentResult,
        skotos_domain::ExperimentResults,
        skotos_domain::GuardedRolloutStatus,
        skotos_domain::GuardrailMetricRule,
        skotos_domain::GuardedRollout,
        skotos_domain::GuardrailMetricCheck,
        skotos_domain::GuardedRolloutEvaluation,
        skotos_domain::FlagVariationMonitoring,
        skotos_domain::FlagMonitoringSummary,
        skotos_domain::EngineeringInsights,
        skotos_domain::MigrationStage,
        skotos_domain::MigrationTemplate,
        skotos_domain::MigrationFlagConfig,
        skotos_domain::MigrationStageMetrics,
        skotos_domain::MigrationTelemetrySummary,
        skotos_domain::SyncedSegmentProvider,
        skotos_domain::SyncedSegmentIntegration,
        skotos_domain::Context,
        skotos_domain::ContextMeta,
        skotos_domain::EvaluationDetail,
        skotos_domain::EvaluationReason,
        skotos_domain::EvaluationErrorKind,
        skotos_domain::AuditLogEntry
    )),
    tags(
        (name = "health", description = "Liveness and readiness probes"),
        (name = "auth", description = "Authentication and session management"),
        (name = "projects", description = "Project management"),
        (name = "environments", description = "Environment and SDK key management"),
        (name = "flags", description = "Feature flag management, semantic patching, simulation, migration flags, and version restore"),
        (name = "segments", description = "Rule-based, list-based, high-scale Big Segments, and Synced Segments"),
        (name = "automation", description = "Scheduled flag changes, multi-step workflows, and APM rollback triggers"),
        (name = "governance", description = "Custom RBAC roles, teams, and enterprise SSO/SCIM"),
        (name = "approvals", description = "Mandatory approval workflows and change review queue"),
        (name = "releases", description = "Release pipelines, flag releases, and application version tracking"),
        (name = "experiments", description = "Metrics, funnels, mutual exclusion layers, A/B & A/A experiments, and guarded rollouts"),
        (name = "analytics", description = "Feature monitoring, engineering insights (DORA), and real-time data export sinks"),
        (name = "devtools", description = "Code references tracking and developer tooling APIs"),
        (name = "webhooks", description = "HMAC-SHA256 signed outbound webhooks and integrations"),
        (name = "usage", description = "Flag evaluation summary counters and stale detection"),
        (name = "audit-log", description = "Immutable audit trail and R2/S3 Zstd archival"),
        (name = "tokens", description = "API access tokens"),
        (name = "members", description = "Organization members and RBAC roles")
    )
)]
pub struct ApiDoc;

// =============================================================================
// Health Probes
// =============================================================================

#[utoipa::path(
    get,
    path = "/healthz",
    tag = "health",
    responses((status = 200, description = "Service is alive", body = HealthResponse))
)]
async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        service: SERVICE_NAME.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

#[utoipa::path(
    get,
    path = "/readyz",
    tag = "health",
    responses(
        (status = 200, description = "Database is reachable", body = HealthResponse),
        (status = 503, description = "Database unavailable", body = ProblemDetails)
    )
)]
async fn readyz(State(state): State<AppState>) -> Result<Json<HealthResponse>, ApiError> {
    state.storage.ping().await.map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "not_ready",
            "Service Unavailable",
            "PostgreSQL readiness probe failed",
        )
    })?;
    Ok(Json(HealthResponse {
        status: "ready".to_string(),
        service: SERVICE_NAME.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }))
}

// =============================================================================
// Auth Handlers
// =============================================================================

#[utoipa::path(
    get,
    path = "/api/v1/auth/config",
    tag = "auth",
    responses((status = 200, description = "Active authentication mode configuration", body = AuthConfigResponse))
)]
async fn auth_config(State(state): State<AppState>) -> Json<AuthConfigResponse> {
    let cfg = state.identity_provider.public_config();
    Json(AuthConfigResponse {
        mode: cfg.mode.as_str().to_string(),
        oidc_issuer: cfg.oidc_issuer,
        oidc_client_id: cfg.oidc_client_id,
        oidc_audience: cfg.oidc_audience,
    })
}

#[utoipa::path(
    post,
    path = "/api/v1/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login succeeded; sets HttpOnly skotos_session cookie", body = AuthMeResponse),
        (status = 400, description = "Unsupported in external auth mode", body = ProblemDetails),
        (status = 401, description = "Invalid credentials", body = ProblemDetails)
    )
)]
async fn auth_login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    let (principal, cookie_val) = state
        .identity_provider
        .login_with_password(&req.email, &req.password)
        .await?;
    let org = state.storage.get_organization(principal.org_id).await?;
    let body = AuthMeResponse {
        organization: org,
        actor_type: principal.actor_type.as_str().to_string(),
        member_id: principal.member_id,
        api_token_id: principal.api_token_id,
        email: principal.email,
        display_name: principal.display_name,
        role: principal.role,
    };
    let set_cookie = build_session_set_cookie(&cookie_val, state.cookie_secure, 7 * 24 * 3600);
    let mut response = Json(body).into_response();
    if let Ok(val) = HeaderValue::from_str(&set_cookie) {
        response.headers_mut().insert(header::SET_COOKIE, val);
    }
    Ok(response)
}

#[utoipa::path(
    post,
    path = "/api/v1/auth/logout",
    tag = "auth",
    responses((status = 204, description = "Session cleared"))
)]
async fn auth_logout(State(state): State<AppState>) -> Response {
    let clear_cookie = build_session_set_cookie("", state.cookie_secure, 0);
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Ok(val) = HeaderValue::from_str(&clear_cookie) {
        response.headers_mut().insert(header::SET_COOKIE, val);
    }
    response
}

#[utoipa::path(
    get,
    path = "/api/v1/auth/me",
    tag = "auth",
    responses(
        (status = 200, description = "Authenticated principal and organization", body = AuthMeResponse),
        (status = 401, description = "Unauthenticated", body = ProblemDetails)
    )
)]
async fn auth_me(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
) -> Result<Json<AuthMeResponse>, ApiError> {
    let org = state.storage.get_organization(principal.org_id).await?;
    Ok(Json(AuthMeResponse {
        organization: org,
        actor_type: principal.actor_type.as_str().to_string(),
        member_id: principal.member_id,
        api_token_id: principal.api_token_id,
        email: principal.email,
        display_name: principal.display_name,
        role: principal.role,
    }))
}

// =============================================================================
// Projects Handlers
// =============================================================================

#[utoipa::path(
    get,
    path = "/api/v1/projects",
    tag = "projects",
    responses((status = 200, body = ProjectListResponse), (status = 401, body = ProblemDetails))
)]
async fn list_projects(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
) -> Result<Json<ProjectListResponse>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::Read, &ResourceRef::org())?;
    let items = state.storage.list_projects(principal.org_id).await?;
    Ok(Json(ProjectListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects",
    tag = "projects",
    request_body = CreateProjectRequest,
    responses(
        (status = 201, body = CreateProjectResponse),
        (status = 400, body = ProblemDetails),
        (status = 403, body = ProblemDetails),
        (status = 409, body = ProblemDetails)
    )
)]
async fn create_project(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Json(req): Json<CreateProjectRequest>,
) -> Result<(StatusCode, Json<CreateProjectResponse>), ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageProject, &ResourceRef::org())?;
    let (project, environments) = state
        .storage
        .create_project(
            principal.org_id,
            &req.key,
            &req.name,
            req.default_client_side_availability,
            req.seed_default_environments,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(CreateProjectResponse {
            project,
            environments,
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}",
    tag = "projects",
    params(("projectKey" = String, Path, description = "Project key")),
    responses((status = 200, body = skotos_domain::Project), (status = 404, body = ProblemDetails))
)]
async fn get_project(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(project_key): Path<String>,
) -> Result<Json<skotos_domain::Project>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::project(&project_key),
    )?;
    let project = state
        .storage
        .get_project(principal.org_id, &project_key)
        .await?;
    Ok(Json(project))
}

#[utoipa::path(
    patch,
    path = "/api/v1/projects/{projectKey}",
    tag = "projects",
    params(("projectKey" = String, Path, description = "Project key")),
    request_body = UpdateProjectRequest,
    responses((status = 200, body = skotos_domain::Project), (status = 403, body = ProblemDetails), (status = 404, body = ProblemDetails))
)]
async fn update_project(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(project_key): Path<String>,
    Json(req): Json<UpdateProjectRequest>,
) -> Result<Json<skotos_domain::Project>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::ManageProject,
        &ResourceRef::project(&project_key),
    )?;
    let updated = state
        .storage
        .update_project(
            principal.org_id,
            &project_key,
            req.name.as_deref(),
            req.default_client_side_availability,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(Json(updated))
}

#[utoipa::path(
    delete,
    path = "/api/v1/projects/{projectKey}",
    tag = "projects",
    params(("projectKey" = String, Path, description = "Project key")),
    responses((status = 204), (status = 403, body = ProblemDetails), (status = 404, body = ProblemDetails))
)]
async fn delete_project(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(project_key): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::ManageProject,
        &ResourceRef::project(&project_key),
    )?;
    state
        .storage
        .delete_project(principal.org_id, &project_key, &principal.to_audit_actor())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// =============================================================================
// Environments & Keys Handlers
// =============================================================================

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/environments",
    tag = "environments",
    params(("projectKey" = String, Path, description = "Project key")),
    responses((status = 200, body = EnvironmentListResponse), (status = 404, body = ProblemDetails))
)]
async fn list_environments(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(project_key): Path<String>,
) -> Result<Json<EnvironmentListResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::project(&project_key),
    )?;
    let items = state
        .storage
        .list_environments(principal.org_id, &project_key)
        .await?;
    Ok(Json(EnvironmentListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/environments",
    tag = "environments",
    params(("projectKey" = String, Path, description = "Project key")),
    request_body = CreateEnvironmentRequest,
    responses((status = 201, body = skotos_domain::Environment), (status = 400, body = ProblemDetails), (status = 403, body = ProblemDetails), (status = 409, body = ProblemDetails))
)]
async fn create_environment(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(project_key): Path<String>,
    Json(req): Json<CreateEnvironmentRequest>,
) -> Result<(StatusCode, Json<skotos_domain::Environment>), ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::ManageEnvironment,
        &ResourceRef::project(&project_key),
    )?;
    let env = state
        .storage
        .create_environment(
            principal.org_id,
            &project_key,
            &req.key,
            &req.name,
            &req.color,
            req.require_comments,
            req.confirm_changes,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(env)))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}",
    tag = "environments",
    params(("projectKey" = String, Path), ("envKey" = String, Path)),
    responses((status = 200, body = skotos_domain::Environment), (status = 404, body = ProblemDetails))
)]
async fn get_environment(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key)): Path<(String, String)>,
) -> Result<Json<skotos_domain::Environment>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::project(&project_key),
    )?;
    let env = state
        .storage
        .get_environment(principal.org_id, &project_key, &env_key)
        .await?;
    Ok(Json(env))
}

#[utoipa::path(
    patch,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}",
    tag = "environments",
    params(("projectKey" = String, Path), ("envKey" = String, Path)),
    request_body = UpdateEnvironmentRequest,
    responses((status = 200, body = skotos_domain::Environment), (status = 403, body = ProblemDetails), (status = 404, body = ProblemDetails))
)]
async fn update_environment(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key)): Path<(String, String)>,
    Json(req): Json<UpdateEnvironmentRequest>,
) -> Result<Json<skotos_domain::Environment>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::ManageEnvironment,
        &ResourceRef::project(&project_key),
    )?;
    let env = state
        .storage
        .update_environment(
            principal.org_id,
            &project_key,
            &env_key,
            req.name.as_deref(),
            req.color.as_deref(),
            req.require_comments,
            req.confirm_changes,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(Json(env))
}

#[utoipa::path(
    delete,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}",
    tag = "environments",
    params(("projectKey" = String, Path), ("envKey" = String, Path)),
    responses((status = 204), (status = 403, body = ProblemDetails), (status = 404, body = ProblemDetails))
)]
async fn delete_environment(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::ManageEnvironment,
        &ResourceRef::project(&project_key),
    )?;
    state
        .storage
        .delete_environment(
            principal.org_id,
            &project_key,
            &env_key,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/keys",
    tag = "environments",
    params(("projectKey" = String, Path), ("envKey" = String, Path)),
    responses((status = 200, body = EnvironmentKeysResponse), (status = 404, body = ProblemDetails))
)]
async fn list_environment_keys(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key)): Path<(String, String)>,
) -> Result<Json<EnvironmentKeysResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::project(&project_key),
    )?;
    let env = state
        .storage
        .get_environment(principal.org_id, &project_key, &env_key)
        .await?;
    let keys = state
        .storage
        .list_environment_keys(principal.org_id, &project_key, &env_key)
        .await?;
    Ok(Json(EnvironmentKeysResponse {
        client_side_id: env.client_side_id,
        keys,
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/keys/rotate",
    tag = "environments",
    params(("projectKey" = String, Path), ("envKey" = String, Path)),
    request_body = RotateKeyRequest,
    responses((status = 200, body = RotateKeyResponse), (status = 403, body = ProblemDetails), (status = 404, body = ProblemDetails))
)]
async fn rotate_environment_key(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key)): Path<(String, String)>,
    Json(req): Json<RotateKeyRequest>,
) -> Result<Json<RotateKeyResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::RotateEnvironmentKey,
        &ResourceRef::project(&project_key),
    )?;
    let rotated = state
        .storage
        .rotate_environment_key(
            principal.org_id,
            &project_key,
            &env_key,
            req.kind,
            req.grace_period_seconds,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(Json(RotateKeyResponse {
        key: rotated.key,
        plaintext_key: rotated.plaintext_key,
    }))
}

// =============================================================================
// Feature Flags, Semantic Patch, Simulate & Revisions Handlers
// =============================================================================

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/flags",
    tag = "flags",
    params(("projectKey" = String, Path), ListFlagsQuery),
    responses((status = 200, body = FlagListResponse), (status = 404, body = ProblemDetails))
)]
async fn list_flags(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(project_key): Path<String>,
    Query(query): Query<ListFlagsQuery>,
) -> Result<Json<FlagListResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::project(&project_key),
    )?;
    let rows = state
        .storage
        .list_flags(
            principal.org_id,
            &project_key,
            query.include_archived,
            query.search.as_deref(),
            query.tag.as_deref(),
        )
        .await?;
    let items = rows
        .into_iter()
        .map(|f| FlagResponse {
            flag: f.flag,
            environments: f.environments,
        })
        .collect();
    Ok(Json(FlagListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/flags",
    tag = "flags",
    params(("projectKey" = String, Path)),
    request_body = CreateFlagRequest,
    responses((status = 201, body = FlagResponse), (status = 400, body = ProblemDetails), (status = 403, body = ProblemDetails), (status = 409, body = ProblemDetails))
)]
async fn create_flag(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(project_key): Path<String>,
    Json(req): Json<CreateFlagRequest>,
) -> Result<(StatusCode, Json<FlagResponse>), ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::CreateFlag,
        &ResourceRef::project(&project_key),
    )?;
    let variations: Vec<Variation> = req
        .variations
        .into_iter()
        .map(|v| Variation {
            id: v.id.unwrap_or_else(Uuid::new_v4),
            value: v.value,
            name: v.name,
            description: v.description,
        })
        .collect();

    let default_on_variation_id = req
        .default_on_variation_index
        .and_then(|i| variations.get(i).map(|v| v.id));
    let default_off_variation_id = req
        .default_off_variation_index
        .and_then(|i| variations.get(i).map(|v| v.id));

    let created = state
        .storage
        .create_flag(
            principal.org_id,
            &project_key,
            CreateFlagInput {
                key: req.key,
                name: req.name,
                description: req.description,
                kind: req.kind,
                variations,
                temporary: req.temporary,
                client_side_availability: req.client_side_availability,
                tags: req.tags,
                maintainer_id: req.maintainer_id.or(principal.member_id),
                default_on_variation_id,
                default_off_variation_id,
            },
            &principal.to_audit_actor(),
        )
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(FlagResponse {
            flag: created.flag,
            environments: created.environments,
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}",
    tag = "flags",
    params(("projectKey" = String, Path), ("flagKey" = String, Path)),
    responses((status = 200, body = FlagResponse), (status = 404, body = ProblemDetails))
)]
async fn get_flag(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key)): Path<(String, String)>,
) -> Result<Json<FlagResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::flag(&project_key, &flag_key),
    )?;
    let found = state
        .storage
        .get_flag(principal.org_id, &project_key, &flag_key)
        .await?;
    Ok(Json(FlagResponse {
        flag: found.flag,
        environments: found.environments,
    }))
}

#[utoipa::path(
    patch,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}",
    tag = "flags",
    params(("projectKey" = String, Path), ("flagKey" = String, Path)),
    request_body = PatchFlagRequest,
    responses(
        (status = 200, body = PatchFlagResponse),
        (status = 400, body = ProblemDetails),
        (status = 403, body = ProblemDetails),
        (status = 404, body = ProblemDetails),
        (status = 409, body = ProblemDetails)
    )
)]
async fn patch_flag(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key)): Path<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<PatchFlagRequest>,
) -> Result<Json<PatchFlagResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::flag_in_env(&project_key, &req.environment_key, &flag_key),
    )?;

    // Phase 3 Mandatory Approval Workflow guard for protected environments
    let target_env = state
        .storage
        .get_environment(principal.org_id, &project_key, &req.environment_key)
        .await?;
    if target_env.approval_settings.required {
        let owner_bypass = principal.role == skotos_domain::BuiltInRole::Owner
            && headers
                .get("x-skotos-bypass-approval")
                .and_then(|v| v.to_str().ok())
                == Some("true");
        if !owner_bypass {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "approval_required",
                "Approval Workflow Required",
                format!(
                    "Environment '{}' requires {} reviewer approval(s); submit an approval request via POST /api/v1/projects/{}/flags/{}/environments/{}/approvals",
                    req.environment_key,
                    target_env.approval_settings.min_approvals,
                    project_key,
                    flag_key,
                    req.environment_key
                ),
            ));
        }
    }

    let if_match_version = parse_if_match_version(&headers)?;
    let expected_version = match (req.expected_version, if_match_version) {
        (Some(body_v), Some(hdr_v)) if body_v != hdr_v => {
            return Err(ApiError::bad_request(
                "version_header_mismatch",
                format!("expectedVersion ({body_v}) disagrees with If-Match header ({hdr_v})"),
            ));
        }
        (Some(v), _) | (None, Some(v)) => Some(v),
        (None, None) => None,
    };

    let outcome = state
        .storage
        .patch_flag(
            principal.org_id,
            &project_key,
            &flag_key,
            &req.environment_key,
            expected_version,
            req.comment.as_deref(),
            &req.instructions,
            &principal.to_audit_actor(),
        )
        .await?;

    Ok(Json(PatchFlagResponse {
        flag: outcome.flag,
        env_config: outcome.env_config,
        summaries: outcome.summaries,
    }))
}

#[utoipa::path(
    delete,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}",
    tag = "flags",
    params(("projectKey" = String, Path), ("flagKey" = String, Path)),
    responses((status = 204), (status = 403, body = ProblemDetails), (status = 404, body = ProblemDetails))
)]
async fn delete_flag(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::DeleteFlag,
        &ResourceRef::flag(&project_key, &flag_key),
    )?;
    state
        .storage
        .delete_flag(
            principal.org_id,
            &project_key,
            &flag_key,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/simulate",
    tag = "flags",
    params(("projectKey" = String, Path), ("flagKey" = String, Path), ("envKey" = String, Path)),
    request_body = SimulateFlagRequest,
    responses((status = 200, body = SimulateFlagResponse), (status = 400, body = ProblemDetails), (status = 404, body = ProblemDetails))
)]
async fn simulate_flag(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, env_key)): Path<(String, String, String)>,
    Json(req): Json<SimulateFlagRequest>,
) -> Result<Json<SimulateFlagResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::SimulateFlag,
        &ResourceRef::flag(&project_key, &flag_key),
    )?;

    let (_env, mut flags_with_cfg, segments) = state
        .storage
        .load_environment_eval_snapshot(principal.org_id, &project_key, &env_key)
        .await?;

    let target_pos = flags_with_cfg
        .iter()
        .position(|(f, _)| f.key == flag_key)
        .ok_or_else(|| ApiError::not_found(format!("flag '{flag_key}'")))?;

    let used_draft = match &req.draft_instructions {
        Some(instrs) if !instrs.is_empty() => {
            let (ref base_flag, ref base_cfg) = flags_with_cfg[target_pos];
            let patched = apply_flag_patch(base_flag, base_cfg, instrs)?;
            flags_with_cfg[target_pos] = (patched.flag, patched.env_config);
            true
        }
        _ => false,
    };

    let flag_map: HashMap<String, &FeatureFlag> = flags_with_cfg
        .iter()
        .map(|(f, _)| (f.key.clone(), f))
        .collect();

    let mut store = InMemoryEvalStore::new();
    for (f, cfg) in &flags_with_cfg {
        if let Ok(compiled) = compile_eval_flag_with_flags(f, cfg, &flag_map) {
            store.insert_flag(compiled);
        }
    }
    for seg in &segments {
        store.insert_segment(compile_eval_segment(seg));
    }

    let detail = evaluate(&store, &flag_key, &req.context, Value::Null);
    let target_flag = &flags_with_cfg[target_pos].0;
    let matched_var = detail
        .variation_index
        .and_then(|idx| target_flag.variations.get(idx));

    Ok(Json(SimulateFlagResponse {
        flag_key,
        environment_key: env_key,
        value: detail.value,
        variation_index: detail.variation_index,
        variation_id: matched_var.map(|v| v.id),
        variation_name: matched_var.and_then(|v| v.name.clone()),
        reason: detail.reason,
        used_draft,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/revisions",
    tag = "flags",
    params(("projectKey" = String, Path), ("flagKey" = String, Path), ("envKey" = String, Path), ListRevisionsQuery),
    responses((status = 200, body = FlagRevisionsResponse), (status = 404, body = ProblemDetails))
)]
async fn list_flag_revisions(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, env_key)): Path<(String, String, String)>,
    Query(query): Query<ListRevisionsQuery>,
) -> Result<Json<FlagRevisionsResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::flag(&project_key, &flag_key),
    )?;
    let items = state
        .storage
        .list_flag_revisions(
            principal.org_id,
            &project_key,
            &flag_key,
            &env_key,
            query.limit,
        )
        .await?;
    Ok(Json(FlagRevisionsResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/restore/{version}",
    tag = "flags",
    params(("projectKey" = String, Path), ("flagKey" = String, Path), ("envKey" = String, Path), ("version" = i64, Path)),
    request_body(content = Option<RestoreRevisionRequest>),
    responses((status = 200, body = skotos_domain::FlagEnvConfig), (status = 400, body = ProblemDetails), (status = 403, body = ProblemDetails), (status = 404, body = ProblemDetails))
)]
async fn restore_flag_revision(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, env_key, version)): Path<(String, String, String, i64)>,
    body: Option<Json<RestoreRevisionRequest>>,
) -> Result<Json<skotos_domain::FlagEnvConfig>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::RestoreFlagRevision,
        &ResourceRef::flag(&project_key, &flag_key),
    )?;
    let comment = body.and_then(|Json(r)| r.comment);
    let restored = state
        .storage
        .restore_flag_revision(
            principal.org_id,
            &project_key,
            &flag_key,
            &env_key,
            version,
            comment.as_deref(),
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(Json(restored))
}

// =============================================================================
// Audit Log, API Tokens & Members Handlers
// =============================================================================

#[utoipa::path(
    get,
    path = "/api/v1/audit-log",
    tag = "audit-log",
    params(AuditLogQuery),
    responses((status = 200, body = AuditLogListResponse), (status = 401, body = ProblemDetails))
)]
async fn list_audit_log(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Query(query): Query<AuditLogQuery>,
) -> Result<Json<AuditLogListResponse>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::Read, &ResourceRef::org())?;

    let project_id = match query.project_key.as_deref() {
        Some(pk) if !pk.is_empty() => {
            Some(state.storage.get_project(principal.org_id, pk).await?.id)
        }
        _ => None,
    };
    let env_id = match (query.project_key.as_deref(), query.env_key.as_deref()) {
        (Some(pk), Some(ek)) if !pk.is_empty() && !ek.is_empty() => Some(
            state
                .storage
                .get_environment(principal.org_id, pk, ek)
                .await?
                .id,
        ),
        _ => None,
    };

    let page = state
        .storage
        .list_audit_logs(
            principal.org_id,
            AuditLogFilter {
                project_id,
                env_id,
                resource_type: query.resource_type,
                resource_key: query.resource_key,
                actor_id: query.actor_id,
                limit: query.limit,
                cursor: query.cursor,
            },
        )
        .await?;

    Ok(Json(AuditLogListResponse {
        items: page.items,
        next_cursor: page.next_cursor,
        has_more: page.has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/tokens",
    tag = "tokens",
    responses((status = 200, body = ApiTokenListResponse), (status = 403, body = ProblemDetails))
)]
async fn list_tokens(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
) -> Result<Json<ApiTokenListResponse>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageApiTokens, &ResourceRef::org())?;
    let items = state.storage.list_api_tokens(principal.org_id).await?;
    Ok(Json(ApiTokenListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/tokens",
    tag = "tokens",
    request_body = CreateApiTokenRequest,
    responses((status = 201, body = CreateApiTokenResponse), (status = 403, body = ProblemDetails))
)]
async fn create_token(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Json(req): Json<CreateApiTokenRequest>,
) -> Result<(StatusCode, Json<CreateApiTokenResponse>), ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageApiTokens, &ResourceRef::org())?;
    if req.role == BuiltInRole::Owner && principal.role != BuiltInRole::Owner {
        return Err(ApiError::forbidden(
            "only owners can issue owner-scoped API tokens",
        ));
    }
    let created = state
        .storage
        .create_api_token(
            principal.org_id,
            &req.name,
            req.role,
            req.expires_at,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(CreateApiTokenResponse {
            token: created.token,
            plaintext_token: created.plaintext_token,
        }),
    ))
}

#[utoipa::path(
    delete,
    path = "/api/v1/tokens/{tokenId}",
    tag = "tokens",
    params(("tokenId" = Uuid, Path, description = "API token UUID")),
    responses((status = 204), (status = 403, body = ProblemDetails), (status = 404, body = ProblemDetails))
)]
async fn revoke_token(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(token_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageApiTokens, &ResourceRef::org())?;
    state
        .storage
        .revoke_api_token(principal.org_id, token_id, &principal.to_audit_actor())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/api/v1/members",
    tag = "members",
    responses((status = 200, body = MemberListResponse), (status = 401, body = ProblemDetails))
)]
async fn list_members(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
) -> Result<Json<MemberListResponse>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::Read, &ResourceRef::org())?;
    let items = state.storage.list_members(principal.org_id).await?;
    Ok(Json(MemberListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/members",
    tag = "members",
    request_body = CreateMemberRequest,
    responses((status = 201, body = skotos_domain::Member), (status = 400, body = ProblemDetails), (status = 403, body = ProblemDetails), (status = 409, body = ProblemDetails))
)]
async fn create_member(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Json(req): Json<CreateMemberRequest>,
) -> Result<(StatusCode, Json<skotos_domain::Member>), ApiError> {
    let required_action = if req.role == BuiltInRole::Owner {
        Action::ManageOwnerMembers
    } else {
        Action::ManageMembers
    };
    state
        .authorizer
        .authorize(&principal, required_action, &ResourceRef::org())?;

    let password_hash = match req.password.as_deref() {
        Some(pwd) if !pwd.is_empty() => Some(hash_password(pwd)?),
        _ => None,
    };

    let created = state
        .storage
        .create_member(
            principal.org_id,
            &req.email,
            &req.display_name,
            req.role,
            password_hash.as_deref(),
            None,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

#[utoipa::path(
    patch,
    path = "/api/v1/members/{memberId}",
    tag = "members",
    params(("memberId" = Uuid, Path, description = "Member UUID")),
    request_body = UpdateMemberRequest,
    responses((status = 200, body = skotos_domain::Member), (status = 403, body = ProblemDetails), (status = 404, body = ProblemDetails), (status = 409, body = ProblemDetails))
)]
async fn update_member(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(member_id): Path<Uuid>,
    Json(req): Json<UpdateMemberRequest>,
) -> Result<Json<skotos_domain::Member>, ApiError> {
    let existing = state
        .storage
        .get_member(principal.org_id, member_id)
        .await?;
    let touches_owner = existing.role == BuiltInRole::Owner || req.role == BuiltInRole::Owner;
    let action = if touches_owner {
        Action::ManageOwnerMembers
    } else {
        Action::ManageMembers
    };
    state
        .authorizer
        .authorize(&principal, action, &ResourceRef::org())?;

    let updated = state
        .storage
        .update_member_role(
            principal.org_id,
            member_id,
            req.role,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(Json(updated))
}

#[utoipa::path(
    delete,
    path = "/api/v1/members/{memberId}",
    tag = "members",
    params(("memberId" = Uuid, Path, description = "Member UUID")),
    responses((status = 204), (status = 403, body = ProblemDetails), (status = 404, body = ProblemDetails), (status = 409, body = ProblemDetails))
)]
async fn delete_member(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(member_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let existing = state
        .storage
        .get_member(principal.org_id, member_id)
        .await?;
    let action = if existing.role == BuiltInRole::Owner {
        Action::ManageOwnerMembers
    } else {
        Action::ManageMembers
    };
    state
        .authorizer
        .authorize(&principal, action, &ResourceRef::org())?;

    state
        .storage
        .delete_member(principal.org_id, member_id, &principal.to_audit_actor())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
