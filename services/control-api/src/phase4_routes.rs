//! Phase 4 Control Plane REST API Routes:
//! - Metrics & Metric Groups (Funnels)
//! - Mutual Exclusion Layers
//! - Full-Stack A/B/n & A/A Experiments (Frequentist & Bayesian + SRM)
//! - Guarded Rollouts & Manual/Auto Rollback
//! - Feature Monitoring & Engineering Insights (DORA + Flag Health)
//! - 2/4/6-Stage Migration Flags & Migration Telemetry
//! - Synced Segments (External Warehouse / CDP Push)
//! - Real-Time Data Export Sinks

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use skotos_auth::{Action, ResourceRef};
use skotos_domain::{
    DataExportFlushResult, DataExportSink, DataExportSinkKind, EngineeringInsights, Experiment,
    ExperimentMode, ExperimentResults, ExperimentStatus, ExperimentVariationWeight,
    FlagMonitoringSummary, FlagPatchInstruction, GuardedRollout, GuardedRolloutEvaluation,
    GuardedRolloutStatus, GuardrailMetricRule, MetricAggregation, MetricDefinition, MetricGroup,
    MetricGroupKind, MetricKind, MetricSuccessDirection, MigrationFlagConfig, MigrationStage,
    MigrationTelemetrySummary, MigrationTemplate, MutualExclusionLayer, StatisticalEngine,
    SyncedSegmentIntegration, SyncedSegmentProvider, VariationSufficientStats,
};
use skotos_fanout::evaluate_guarded_rollout;
use skotos_storage::AuditActor;
use skotos_telemetry::{
    check_srm, dispatch_http_sink, encode_zstd_ndjson, evaluate_metric_experiment,
};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::extract::{AppState, AuthPrincipal};
use crate::problem::ApiError;

// =============================================================================
// DTOs
// =============================================================================

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MetricListResponse {
    pub items: Vec<MetricDefinition>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateMetricRequest {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub kind: MetricKind,
    pub event_key: String,
    pub success_direction: MetricSuccessDirection,
    #[serde(default = "default_aggregation")]
    pub aggregation: MetricAggregation,
    #[serde(default)]
    pub unit: String,
    #[serde(default = "default_rand_unit")]
    pub randomization_unit: String,
}

fn default_aggregation() -> MetricAggregation {
    MetricAggregation::Average
}

fn default_rand_unit() -> String {
    "user".to_string()
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MetricGroupListResponse {
    pub items: Vec<MetricGroup>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateMetricGroupRequest {
    pub key: String,
    pub name: String,
    pub kind: MetricGroupKind,
    #[serde(default)]
    pub description: String,
    pub metric_keys: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LayerListResponse {
    pub items: Vec<MutualExclusionLayer>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateLayerRequest {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentListResponse {
    pub items: Vec<Experiment>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateExperimentRequest {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub hypothesis: String,
    pub flag_key: String,
    #[serde(default = "default_exp_mode")]
    pub mode: ExperimentMode,
    #[serde(default = "default_stat_engine")]
    pub engine: StatisticalEngine,
    #[serde(default = "default_rand_unit")]
    pub randomization_unit: String,
    #[serde(default)]
    pub control_variation_index: i32,
    pub primary_metric_key: String,
    #[serde(default)]
    pub secondary_metric_keys: Vec<String>,
    #[serde(default)]
    pub metric_group_key: Option<String>,
    #[serde(default)]
    pub layer_key: Option<String>,
    #[serde(default)]
    pub layer_reservation_buckets: Option<i32>,
    #[serde(default)]
    pub allocations: Vec<ExperimentVariationWeight>,
    #[serde(default = "default_alpha")]
    pub alpha: f64,
}

fn default_exp_mode() -> ExperimentMode {
    ExperimentMode::AbTest
}

fn default_stat_engine() -> StatisticalEngine {
    StatisticalEngine::Frequentist
}

fn default_alpha() -> f64 {
    0.05
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GuardedRolloutListResponse {
    pub items: Vec<GuardedRollout>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateGuardedRolloutRequest {
    pub flag_key: String,
    pub name: String,
    #[serde(default = "default_control_var")]
    pub control_variation_index: i32,
    #[serde(default)]
    pub treatment_variation_index: i32,
    #[serde(default = "default_traffic_pct")]
    pub current_traffic_pct: i32,
    #[serde(default = "default_true")]
    pub auto_rollback: bool,
    pub guardrails: Vec<GuardrailMetricRule>,
}

fn default_control_var() -> i32 {
    1
}

fn default_traffic_pct() -> i32 {
    20
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct LookbackQuery {
    #[serde(default = "default_days")]
    pub days: i64,
    #[serde(default = "default_hours")]
    pub hours: i64,
}

fn default_days() -> i64 {
    30
}

fn default_hours() -> i64 {
    24
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpsertMigrationConfigRequest {
    pub template: MigrationTemplate,
    pub current_stage: MigrationStage,
    #[serde(default = "default_check_ratio")]
    pub check_ratio: i32,
    #[serde(default = "default_read_order")]
    pub read_execution_order: String,
}

fn default_check_ratio() -> i32 {
    1
}

fn default_read_order() -> String {
    "parallel".to_string()
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncedSegmentListResponse {
    pub items: Vec<SyncedSegmentIntegration>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateSyncedSegmentRequest {
    pub segment_key: String,
    pub provider: SyncedSegmentProvider,
    pub external_cohort_id: String,
    #[serde(default = "default_rand_unit")]
    pub context_kind: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateSyncedSegmentResponse {
    pub integration: SyncedSegmentIntegration,
    pub plaintext_token: String,
    pub ingest_path: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncedSegmentIngestRequest {
    #[serde(default)]
    pub add_keys: Vec<String>,
    #[serde(default)]
    pub remove_keys: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncedSegmentIngestResponse {
    pub segment_key: String,
    pub included_count: i64,
    pub generation: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataExportSinkListResponse {
    pub items: Vec<DataExportSink>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDataExportSinkRequest {
    pub name: String,
    pub kind: DataExportSinkKind,
    pub destination: String,
    #[serde(default)]
    pub env_id: Option<Uuid>,
    #[serde(default)]
    pub config: Value,
}

// =============================================================================
// Router
// =============================================================================

pub fn phase4_router() -> Router<AppState> {
    Router::new()
        // Metrics & Metric Groups (Funnels)
        .route(
            "/api/v1/projects/{projectKey}/metrics",
            get(list_metrics).post(create_metric),
        )
        .route(
            "/api/v1/projects/{projectKey}/metrics/{metricKey}",
            delete(delete_metric),
        )
        .route(
            "/api/v1/projects/{projectKey}/metric-groups",
            get(list_metric_groups).post(create_metric_group),
        )
        .route(
            "/api/v1/projects/{projectKey}/metric-groups/{groupKey}",
            delete(delete_metric_group),
        )
        // Mutual Exclusion Layers
        .route(
            "/api/v1/projects/{projectKey}/layers",
            get(list_layers).post(create_layer),
        )
        .route(
            "/api/v1/projects/{projectKey}/layers/{layerKey}",
            delete(delete_layer),
        )
        // Experiments
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}/experiments",
            get(list_experiments).post(create_experiment),
        )
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}/experiments/{expKey}",
            get(get_experiment).delete(delete_experiment),
        )
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}/experiments/{expKey}/start",
            post(start_experiment),
        )
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}/experiments/{expKey}/stop",
            post(stop_experiment),
        )
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}/experiments/{expKey}/results",
            get(get_experiment_results),
        )
        // Guarded Rollouts
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}/guarded-rollouts",
            get(list_guarded_rollouts).post(create_guarded_rollout),
        )
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}/guarded-rollouts/{rolloutId}/evaluate",
            post(evaluate_guarded_rollout_handler),
        )
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}/guarded-rollouts/{rolloutId}/rollback",
            post(rollback_guarded_rollout),
        )
        // Feature Monitoring & Engineering Insights (DORA)
        .route(
            "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/monitoring",
            get(get_flag_monitoring),
        )
        .route(
            "/api/v1/projects/{projectKey}/insights",
            get(get_engineering_insights),
        )
        // 2/4/6-Stage Migration Flags
        .route(
            "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/migration",
            get(get_migration_config).put(upsert_migration_config),
        )
        .route(
            "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/migration/telemetry",
            get(get_migration_telemetry),
        )
        // Synced Segments (External Warehouse/CDP Push)
        .route(
            "/api/v1/projects/{projectKey}/environments/{envKey}/synced-segments",
            get(list_synced_segments).post(create_synced_segment),
        )
        .route(
            "/api/v1/synced-segments/{integrationId}",
            delete(delete_synced_segment),
        )
        .route(
            "/api/v1/synced-segments/ingest/{token}",
            post(ingest_synced_segment),
        )
        // Real-Time Data Export Sinks
        .route(
            "/api/v1/data-export/sinks",
            get(list_data_export_sinks).post(create_data_export_sink),
        )
        .route(
            "/api/v1/data-export/sinks/{sinkId}",
            delete(delete_data_export_sink),
        )
        .route(
            "/api/v1/data-export/sinks/{sinkId}/flush",
            post(flush_data_export_sink),
        )
}

// =============================================================================
// Handlers: Metrics, Metric Groups & Mutual Exclusion Layers
// =============================================================================

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/metrics",
    tag = "experiments",
    params(("projectKey" = String, Path, description = "Project key")),
    responses((status = 200, body = MetricListResponse))
)]
pub async fn list_metrics(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(project_key): Path<String>,
) -> Result<Json<MetricListResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::project(&project_key),
    )?;
    let items = state
        .storage
        .list_metrics(principal.org_id, &project_key)
        .await?;
    Ok(Json(MetricListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/metrics",
    tag = "experiments",
    params(("projectKey" = String, Path, description = "Project key")),
    request_body = CreateMetricRequest,
    responses((status = 201, body = MetricDefinition))
)]
pub async fn create_metric(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(project_key): Path<String>,
    Json(body): Json<CreateMetricRequest>,
) -> Result<(StatusCode, Json<MetricDefinition>), ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::CreateFlag,
        &ResourceRef::project(&project_key),
    )?;
    let metric = state
        .storage
        .create_metric(
            principal.org_id,
            &project_key,
            &body.key,
            &body.name,
            &body.description,
            body.kind,
            &body.event_key,
            body.success_direction,
            body.aggregation,
            &body.unit,
            &body.randomization_unit,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(metric)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/projects/{projectKey}/metrics/{metricKey}",
    tag = "experiments",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("metricKey" = String, Path, description = "Metric key"),
    ),
    responses((status = 204))
)]
pub async fn delete_metric(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, metric_key)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::ManageProject,
        &ResourceRef::project(&project_key),
    )?;
    state
        .storage
        .delete_metric(
            principal.org_id,
            &project_key,
            &metric_key,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/metric-groups",
    tag = "experiments",
    params(("projectKey" = String, Path, description = "Project key")),
    responses((status = 200, body = MetricGroupListResponse))
)]
pub async fn list_metric_groups(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(project_key): Path<String>,
) -> Result<Json<MetricGroupListResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::project(&project_key),
    )?;
    let items = state
        .storage
        .list_metric_groups(principal.org_id, &project_key)
        .await?;
    Ok(Json(MetricGroupListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/metric-groups",
    tag = "experiments",
    params(("projectKey" = String, Path, description = "Project key")),
    request_body = CreateMetricGroupRequest,
    responses((status = 201, body = MetricGroup))
)]
pub async fn create_metric_group(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(project_key): Path<String>,
    Json(body): Json<CreateMetricGroupRequest>,
) -> Result<(StatusCode, Json<MetricGroup>), ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::CreateFlag,
        &ResourceRef::project(&project_key),
    )?;
    let group = state
        .storage
        .create_metric_group(
            principal.org_id,
            &project_key,
            &body.key,
            &body.name,
            body.kind,
            &body.description,
            &body.metric_keys,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(group)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/projects/{projectKey}/metric-groups/{groupKey}",
    tag = "experiments",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("groupKey" = String, Path, description = "Metric group key"),
    ),
    responses((status = 204))
)]
pub async fn delete_metric_group(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, group_key)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::ManageProject,
        &ResourceRef::project(&project_key),
    )?;
    state
        .storage
        .delete_metric_group(
            principal.org_id,
            &project_key,
            &group_key,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/layers",
    tag = "experiments",
    params(("projectKey" = String, Path, description = "Project key")),
    responses((status = 200, body = LayerListResponse))
)]
pub async fn list_layers(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(project_key): Path<String>,
) -> Result<Json<LayerListResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::project(&project_key),
    )?;
    let items = state
        .storage
        .list_layers(principal.org_id, &project_key)
        .await?;
    Ok(Json(LayerListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/layers",
    tag = "experiments",
    params(("projectKey" = String, Path, description = "Project key")),
    request_body = CreateLayerRequest,
    responses((status = 201, body = MutualExclusionLayer))
)]
pub async fn create_layer(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(project_key): Path<String>,
    Json(body): Json<CreateLayerRequest>,
) -> Result<(StatusCode, Json<MutualExclusionLayer>), ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::ManageProject,
        &ResourceRef::project(&project_key),
    )?;
    let layer = state
        .storage
        .create_layer(
            principal.org_id,
            &project_key,
            &body.key,
            &body.name,
            &body.description,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(layer)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/projects/{projectKey}/layers/{layerKey}",
    tag = "experiments",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("layerKey" = String, Path, description = "Mutual exclusion layer key"),
    ),
    responses((status = 204))
)]
pub async fn delete_layer(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, layer_key)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::ManageProject,
        &ResourceRef::project(&project_key),
    )?;
    state
        .storage
        .delete_layer(
            principal.org_id,
            &project_key,
            &layer_key,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// =============================================================================
// Handlers: Experiments & Live Frequentist/Bayesian/SRM Results
// =============================================================================

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/experiments",
    tag = "experiments",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("envKey" = String, Path, description = "Environment key"),
    ),
    responses((status = 200, body = ExperimentListResponse))
)]
pub async fn list_experiments(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key)): Path<(String, String)>,
) -> Result<Json<ExperimentListResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::environment(&project_key, &env_key),
    )?;
    let items = state
        .storage
        .list_experiments(principal.org_id, &project_key, &env_key)
        .await?;
    Ok(Json(ExperimentListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/experiments",
    tag = "experiments",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("envKey" = String, Path, description = "Environment key"),
    ),
    request_body = CreateExperimentRequest,
    responses((status = 201, body = Experiment))
)]
pub async fn create_experiment(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key)): Path<(String, String)>,
    Json(body): Json<CreateExperimentRequest>,
) -> Result<(StatusCode, Json<Experiment>), ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::flag_in_env(&project_key, &env_key, &body.flag_key),
    )?;
    let exp = state
        .storage
        .create_experiment(
            principal.org_id,
            &project_key,
            &env_key,
            &body.key,
            &body.name,
            &body.hypothesis,
            &body.flag_key,
            body.mode,
            body.engine,
            &body.randomization_unit,
            body.control_variation_index,
            &body.primary_metric_key,
            &body.secondary_metric_keys,
            body.metric_group_key.as_deref(),
            body.layer_key.as_deref(),
            body.layer_reservation_buckets,
            &body.allocations,
            body.alpha,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(exp)))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/experiments/{expKey}",
    tag = "experiments",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("envKey" = String, Path, description = "Environment key"),
        ("expKey" = String, Path, description = "Experiment key"),
    ),
    responses((status = 200, body = Experiment))
)]
pub async fn get_experiment(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key, exp_key)): Path<(String, String, String)>,
) -> Result<Json<Experiment>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::environment(&project_key, &env_key),
    )?;
    let exp = state
        .storage
        .get_experiment(principal.org_id, &project_key, &env_key, &exp_key)
        .await?;
    Ok(Json(exp))
}

#[utoipa::path(
    delete,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/experiments/{expKey}",
    tag = "experiments",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("envKey" = String, Path, description = "Environment key"),
        ("expKey" = String, Path, description = "Experiment key"),
    ),
    responses((status = 204))
)]
pub async fn delete_experiment(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key, exp_key)): Path<(String, String, String)>,
) -> Result<StatusCode, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::ManageProject,
        &ResourceRef::environment(&project_key, &env_key),
    )?;
    state
        .storage
        .delete_experiment(
            principal.org_id,
            &project_key,
            &env_key,
            &exp_key,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/experiments/{expKey}/start",
    tag = "experiments",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("envKey" = String, Path, description = "Environment key"),
        ("expKey" = String, Path, description = "Experiment key"),
    ),
    responses((status = 200, body = Experiment))
)]
pub async fn start_experiment(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key, exp_key)): Path<(String, String, String)>,
) -> Result<Json<Experiment>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::environment(&project_key, &env_key),
    )?;
    let exp = state
        .storage
        .transition_experiment_status(
            principal.org_id,
            &project_key,
            &env_key,
            &exp_key,
            ExperimentStatus::Running,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(Json(exp))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/experiments/{expKey}/stop",
    tag = "experiments",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("envKey" = String, Path, description = "Environment key"),
        ("expKey" = String, Path, description = "Experiment key"),
    ),
    responses((status = 200, body = Experiment))
)]
pub async fn stop_experiment(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key, exp_key)): Path<(String, String, String)>,
) -> Result<Json<Experiment>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::environment(&project_key, &env_key),
    )?;
    let exp = state
        .storage
        .transition_experiment_status(
            principal.org_id,
            &project_key,
            &env_key,
            &exp_key,
            ExperimentStatus::Stopped,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(Json(exp))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/experiments/{expKey}/results",
    tag = "experiments",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("envKey" = String, Path, description = "Environment key"),
        ("expKey" = String, Path, description = "Experiment key"),
    ),
    responses((status = 200, body = ExperimentResults))
)]
pub async fn get_experiment_results(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key, exp_key)): Path<(String, String, String)>,
) -> Result<Json<ExperimentResults>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::environment(&project_key, &env_key),
    )?;
    let exp = state
        .storage
        .get_experiment(principal.org_id, &project_key, &env_key, &exp_key)
        .await?;
    let primary_metric = state
        .storage
        .get_metric(principal.org_id, exp.project_id, &exp.primary_metric_key)
        .await?;

    let since = exp
        .started_at
        .map(|s| s - Duration::minutes(5))
        .unwrap_or_else(|| exp.created_at - Duration::minutes(5));
    let until = exp.stopped_at.unwrap_or_else(Utc::now);

    let raw_primary_stats = state
        .telemetry
        .query_sufficient_stats(
            principal.org_id,
            exp.env_id,
            &exp.flag_key,
            &primary_metric,
            since,
            until,
        )
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "telemetry_error",
                "Telemetry Query Failed",
                e.to_string(),
            )
        })?;

    // Ensure every configured allocation variation appears in results
    let mut padded_stats: Vec<VariationSufficientStats> = exp
        .allocations
        .iter()
        .map(|a| {
            raw_primary_stats
                .iter()
                .find(|s| s.variation_index == a.variation_index)
                .cloned()
                .unwrap_or_else(|| VariationSufficientStats {
                    variation_index: a.variation_index,
                    ..Default::default()
                })
        })
        .collect();
    if padded_stats.is_empty() {
        padded_stats = raw_primary_stats;
    }

    let observed_counts: Vec<i64> = padded_stats.iter().map(|s| s.exposures).collect();
    let expected_weights: Vec<i32> = if exp.allocations.len() == padded_stats.len() {
        exp.allocations.iter().map(|a| a.weight).collect()
    } else {
        vec![50_000; padded_stats.len()]
    };
    let srm = check_srm(&observed_counts, &expected_weights);
    let total_exposures = observed_counts.iter().sum();

    let primary = evaluate_metric_experiment(
        &primary_metric,
        &padded_stats,
        exp.control_variation_index,
        exp.engine,
        exp.alpha,
    );

    let mut secondary = Vec::new();
    for sec_key in &exp.secondary_metric_keys {
        if let Ok(sec_metric) = state
            .storage
            .get_metric(principal.org_id, exp.project_id, sec_key)
            .await
        {
            if let Ok(sec_stats) = state
                .telemetry
                .query_sufficient_stats(
                    principal.org_id,
                    exp.env_id,
                    &exp.flag_key,
                    &sec_metric,
                    since,
                    until,
                )
                .await
            {
                secondary.push(evaluate_metric_experiment(
                    &sec_metric,
                    &sec_stats,
                    exp.control_variation_index,
                    exp.engine,
                    exp.alpha,
                ));
            }
        }
    }

    Ok(Json(ExperimentResults {
        experiment_key: exp.key,
        flag_key: exp.flag_key,
        mode: exp.mode,
        engine: exp.engine,
        status: exp.status,
        total_exposures,
        srm,
        primary,
        secondary,
        computed_at: Utc::now(),
    }))
}

// =============================================================================
// Handlers: Guarded Rollouts, Feature Monitoring & Engineering Insights
// =============================================================================

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/guarded-rollouts",
    tag = "experiments",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("envKey" = String, Path, description = "Environment key"),
    ),
    responses((status = 200, body = GuardedRolloutListResponse))
)]
pub async fn list_guarded_rollouts(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key)): Path<(String, String)>,
) -> Result<Json<GuardedRolloutListResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::environment(&project_key, &env_key),
    )?;
    let items = state
        .storage
        .list_guarded_rollouts(principal.org_id, &project_key, &env_key)
        .await?;
    Ok(Json(GuardedRolloutListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/guarded-rollouts",
    tag = "experiments",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("envKey" = String, Path, description = "Environment key"),
    ),
    request_body = CreateGuardedRolloutRequest,
    responses((status = 201, body = GuardedRollout))
)]
pub async fn create_guarded_rollout(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key)): Path<(String, String)>,
    Json(body): Json<CreateGuardedRolloutRequest>,
) -> Result<(StatusCode, Json<GuardedRollout>), ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::flag_in_env(&project_key, &env_key, &body.flag_key),
    )?;
    let rollout = state
        .storage
        .create_guarded_rollout(
            principal.org_id,
            &project_key,
            &env_key,
            &body.flag_key,
            &body.name,
            body.control_variation_index,
            body.treatment_variation_index,
            body.current_traffic_pct,
            body.auto_rollback,
            &body.guardrails,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(rollout)))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/guarded-rollouts/{rolloutId}/evaluate",
    tag = "experiments",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("envKey" = String, Path, description = "Environment key"),
        ("rolloutId" = Uuid, Path, description = "Guarded rollout ID"),
    ),
    responses((status = 200, body = GuardedRolloutEvaluation))
)]
pub async fn evaluate_guarded_rollout_handler(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key, rollout_id)): Path<(String, String, Uuid)>,
) -> Result<Json<GuardedRolloutEvaluation>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::environment(&project_key, &env_key),
    )?;
    let rollout = state
        .storage
        .get_guarded_rollout(principal.org_id, rollout_id)
        .await?;
    let eval = evaluate_guarded_rollout(
        &state.storage,
        state.telemetry.as_ref(),
        &rollout,
        &project_key,
        &env_key,
    )
    .await
    .map_err(|e| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "guardrail_eval_error",
            "Guardrail Evaluation Failed",
            e.to_string(),
        )
    })?;
    Ok(Json(eval))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/guarded-rollouts/{rolloutId}/rollback",
    tag = "experiments",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("envKey" = String, Path, description = "Environment key"),
        ("rolloutId" = Uuid, Path, description = "Guarded rollout ID"),
    ),
    responses((status = 200, body = GuardedRollout))
)]
pub async fn rollback_guarded_rollout(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key, rollout_id)): Path<(String, String, Uuid)>,
) -> Result<Json<GuardedRollout>, ApiError> {
    let rollout = state
        .storage
        .get_guarded_rollout(principal.org_id, rollout_id)
        .await?;
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::flag_in_env(&project_key, &env_key, &rollout.flag_key),
    )?;
    let _ = state
        .storage
        .patch_flag(
            principal.org_id,
            &project_key,
            &rollout.flag_key,
            &env_key,
            None,
            Some("Manual rollback triggered from Guarded Rollout"),
            &[FlagPatchInstruction::TurnFlagOff],
            &principal.to_audit_actor(),
        )
        .await?;
    let updated = state
        .storage
        .update_guarded_rollout_verdict(
            principal.org_id,
            rollout_id,
            GuardedRolloutStatus::RolledBack,
            "Manually rolled back by operator",
        )
        .await?;
    Ok(Json(updated))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/monitoring",
    tag = "analytics",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("flagKey" = String, Path, description = "Flag key"),
        ("envKey" = String, Path, description = "Environment key"),
    ),
    responses((status = 200, body = FlagMonitoringSummary))
)]
pub async fn get_flag_monitoring(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, env_key)): Path<(String, String, String)>,
    Query(q): Query<LookbackQuery>,
) -> Result<Json<FlagMonitoringSummary>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::flag_in_env(&project_key, &env_key, &flag_key),
    )?;
    let env = state
        .storage
        .get_environment(principal.org_id, &project_key, &env_key)
        .await?;
    let summary = state
        .telemetry
        .query_flag_monitoring(principal.org_id, env.id, &flag_key, q.hours)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "monitoring_error",
                "Monitoring Query Failed",
                e.to_string(),
            )
        })?;
    Ok(Json(summary))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/insights",
    tag = "analytics",
    params(("projectKey" = String, Path, description = "Project key")),
    responses((status = 200, body = EngineeringInsights))
)]
pub async fn get_engineering_insights(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(project_key): Path<String>,
    Query(q): Query<LookbackQuery>,
) -> Result<Json<EngineeringInsights>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::project(&project_key),
    )?;
    let insights = state
        .storage
        .compute_engineering_insights(principal.org_id, &project_key, q.days)
        .await?;
    Ok(Json(insights))
}

// =============================================================================
// Handlers: 2/4/6-Stage Migration Flags
// =============================================================================

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/migration",
    tag = "flags",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("flagKey" = String, Path, description = "Flag key"),
        ("envKey" = String, Path, description = "Environment key"),
    ),
    responses((status = 200, body = Option<MigrationFlagConfig>))
)]
pub async fn get_migration_config(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, env_key)): Path<(String, String, String)>,
) -> Result<Json<Option<MigrationFlagConfig>>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::flag_in_env(&project_key, &env_key, &flag_key),
    )?;
    let cfg = state
        .storage
        .get_migration_config(principal.org_id, &project_key, &env_key, &flag_key)
        .await?;
    Ok(Json(cfg))
}

#[utoipa::path(
    put,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/migration",
    tag = "flags",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("flagKey" = String, Path, description = "Flag key"),
        ("envKey" = String, Path, description = "Environment key"),
    ),
    request_body = UpsertMigrationConfigRequest,
    responses((status = 200, body = MigrationFlagConfig))
)]
pub async fn upsert_migration_config(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, env_key)): Path<(String, String, String)>,
    Json(body): Json<UpsertMigrationConfigRequest>,
) -> Result<Json<MigrationFlagConfig>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::PatchFlag,
        &ResourceRef::flag_in_env(&project_key, &env_key, &flag_key),
    )?;
    let cfg = state
        .storage
        .upsert_migration_config(
            principal.org_id,
            &project_key,
            &env_key,
            &flag_key,
            body.template,
            body.current_stage,
            body.check_ratio,
            &body.read_execution_order,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(Json(cfg))
}

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/migration/telemetry",
    tag = "flags",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("flagKey" = String, Path, description = "Flag key"),
        ("envKey" = String, Path, description = "Environment key"),
    ),
    responses((status = 200, body = MigrationTelemetrySummary))
)]
pub async fn get_migration_telemetry(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, flag_key, env_key)): Path<(String, String, String)>,
    Query(q): Query<LookbackQuery>,
) -> Result<Json<MigrationTelemetrySummary>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::flag_in_env(&project_key, &env_key, &flag_key),
    )?;
    let env = state
        .storage
        .get_environment(principal.org_id, &project_key, &env_key)
        .await?;
    let summary = state
        .telemetry
        .query_migration_telemetry(principal.org_id, env.id, &flag_key, q.days)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "migration_telemetry_error",
                "Migration Telemetry Failed",
                e.to_string(),
            )
        })?;
    Ok(Json(summary))
}

// =============================================================================
// Handlers: Synced Segments (External Warehouse & CDP Reverse-ETL Push)
// =============================================================================

#[utoipa::path(
    get,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/synced-segments",
    tag = "segments",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("envKey" = String, Path, description = "Environment key"),
    ),
    responses((status = 200, body = SyncedSegmentListResponse))
)]
pub async fn list_synced_segments(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key)): Path<(String, String)>,
) -> Result<Json<SyncedSegmentListResponse>, ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::Read,
        &ResourceRef::environment(&project_key, &env_key),
    )?;
    let items = state
        .storage
        .list_synced_segments(principal.org_id, &project_key, &env_key)
        .await?;
    Ok(Json(SyncedSegmentListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/projects/{projectKey}/environments/{envKey}/synced-segments",
    tag = "segments",
    params(
        ("projectKey" = String, Path, description = "Project key"),
        ("envKey" = String, Path, description = "Environment key"),
    ),
    request_body = CreateSyncedSegmentRequest,
    responses((status = 201, body = CreateSyncedSegmentResponse))
)]
pub async fn create_synced_segment(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path((project_key, env_key)): Path<(String, String)>,
    Json(body): Json<CreateSyncedSegmentRequest>,
) -> Result<(StatusCode, Json<CreateSyncedSegmentResponse>), ApiError> {
    state.authorizer.authorize(
        &principal,
        Action::ManageProject,
        &ResourceRef::environment(&project_key, &env_key),
    )?;
    let created = state
        .storage
        .create_synced_segment(
            principal.org_id,
            &project_key,
            &env_key,
            &body.segment_key,
            body.provider,
            &body.external_cohort_id,
            &body.context_kind,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(CreateSyncedSegmentResponse {
            integration: created.integration,
            plaintext_token: created.plaintext_token,
            ingest_path: created.ingest_path,
        }),
    ))
}

#[utoipa::path(
    delete,
    path = "/api/v1/synced-segments/{integrationId}",
    tag = "segments",
    params(("integrationId" = Uuid, Path, description = "Synced segment integration ID")),
    responses((status = 204))
)]
pub async fn delete_synced_segment(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(integration_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageProject, &ResourceRef::org())?;
    state
        .storage
        .delete_synced_segment(
            principal.org_id,
            integration_id,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// High-throughput unauthenticated CDP / Reverse-ETL sync endpoint (`POST /api/v1/synced-segments/ingest/{token}`).
/// Authenticates via one-time hashed sync token (`sk_sync_...`), mutates Big Segment membership,
/// and triggers OutboxWorker Zstd hash-block sync to R2/S3 & NATS!
#[utoipa::path(
    post,
    path = "/api/v1/synced-segments/ingest/{token}",
    tag = "segments",
    params(("token" = String, Path, description = "Secret synced segment push token")),
    request_body = SyncedSegmentIngestRequest,
    responses((status = 200, body = SyncedSegmentIngestResponse))
)]
pub async fn ingest_synced_segment(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Json(body): Json<SyncedSegmentIngestRequest>,
) -> Result<Json<SyncedSegmentIngestResponse>, ApiError> {
    let resolved = state
        .storage
        .resolve_synced_segment_by_token(&token)
        .await?;
    let actor = AuditActor {
        actor_id: None,
        actor_type: format!("synced_segment:{}", resolved.integration.provider.as_str()),
        actor_email: None,
    };
    let status = state
        .storage
        .mutate_big_segment_members(
            resolved.integration.org_id,
            &resolved.project_key,
            &resolved.env_key,
            &resolved.integration.segment_key,
            &resolved.integration.context_kind,
            &body.add_keys,
            &body.remove_keys,
            &[],
            &[],
            &actor,
        )
        .await?;
    state
        .storage
        .mark_synced_segment_ingested(resolved.integration.id, status.included_count)
        .await?;
    Ok(Json(SyncedSegmentIngestResponse {
        segment_key: status.segment_key,
        included_count: status.included_count,
        generation: status.generation,
    }))
}

// =============================================================================
// Handlers: Real-Time Data Export Sinks
// =============================================================================

#[utoipa::path(
    get,
    path = "/api/v1/data-export/sinks",
    tag = "analytics",
    responses((status = 200, body = DataExportSinkListResponse))
)]
pub async fn list_data_export_sinks(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
) -> Result<Json<DataExportSinkListResponse>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::Read, &ResourceRef::org())?;
    let items = state
        .storage
        .list_data_export_sinks(principal.org_id)
        .await?;
    Ok(Json(DataExportSinkListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/api/v1/data-export/sinks",
    tag = "analytics",
    request_body = CreateDataExportSinkRequest,
    responses((status = 201, body = DataExportSink))
)]
pub async fn create_data_export_sink(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Json(body): Json<CreateDataExportSinkRequest>,
) -> Result<(StatusCode, Json<DataExportSink>), ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageProject, &ResourceRef::org())?;
    let sink = state
        .storage
        .create_data_export_sink(
            principal.org_id,
            body.env_id,
            &body.name,
            body.kind,
            &body.destination,
            body.config,
            &principal.to_audit_actor(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(sink)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/data-export/sinks/{sinkId}",
    tag = "analytics",
    params(("sinkId" = Uuid, Path, description = "Data export sink ID")),
    responses((status = 204))
)]
pub async fn delete_data_export_sink(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(sink_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageProject, &ResourceRef::org())?;
    state
        .storage
        .delete_data_export_sink(principal.org_id, sink_id, &principal.to_audit_actor())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/v1/data-export/sinks/{sinkId}/flush",
    tag = "analytics",
    params(("sinkId" = Uuid, Path, description = "Data export sink ID")),
    responses((status = 200, body = DataExportFlushResult))
)]
pub async fn flush_data_export_sink(
    State(state): State<AppState>,
    AuthPrincipal(principal): AuthPrincipal,
    Path(sink_id): Path<Uuid>,
) -> Result<Json<DataExportFlushResult>, ApiError> {
    state
        .authorizer
        .authorize(&principal, Action::ManageProject, &ResourceRef::org())?;
    let sink = state
        .storage
        .get_data_export_sink(principal.org_id, sink_id)
        .await?;
    let events = state
        .storage
        .load_recent_export_events(principal.org_id, sink.env_id, 1_000)
        .await?;
    let count = events.len() as i64;

    let (location, bytes_written) = match sink.kind {
        DataExportSinkKind::S3Parquet => {
            let compressed = encode_zstd_ndjson(&events).map_err(|e| {
                ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "export_compress_error",
                    "Data Export Compression Failed",
                    e.to_string(),
                )
            })?;
            let store = state
                .snapshots
                .clone()
                .unwrap_or_else(skotos_fanout::SnapshotStore::in_memory);
            store
                .put_data_export_batch(principal.org_id, sink.id, Uuid::now_v7(), compressed)
                .await
                .map_err(|e| {
                    ApiError::new(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "export_s3_error",
                        "Data Export S3 Upload Failed",
                        e.to_string(),
                    )
                })?
        }
        _ => {
            let http = reqwest::Client::new();
            let len = dispatch_http_sink(&http, &sink, &events)
                .await
                .map_err(|e| {
                    ApiError::new(
                        StatusCode::BAD_GATEWAY,
                        "export_http_error",
                        "Data Export HTTP Push Failed",
                        e.to_string(),
                    )
                })?;
            (sink.destination.clone(), len as i64)
        }
    };

    state
        .storage
        .record_data_export_flush(principal.org_id, sink.id, count)
        .await?;

    Ok(Json(DataExportFlushResult {
        sink_id: sink.id,
        events_exported: count,
        bytes_written,
        location,
        flushed_at: Utc::now(),
    }))
}
