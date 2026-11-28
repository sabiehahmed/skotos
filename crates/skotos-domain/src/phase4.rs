//! Phase 4 Domain Models:
//! - ClickHouse Analytics & Real-Time Data Export Sinks
//! - Experimentation Core (Metrics, Metric Groups / Funnels, Mutual Exclusion Layers,
//!   A/B/n & A/A Experiments, Sufficient Statistics, Frequentist/Bayesian/SRM Results)
//! - Guarded Rollouts, Feature Monitoring & Engineering Insights (DORA + Flag Health)
//! - 2/4/6-Stage Migration Flags & Migration Telemetry
//! - Synced Segments (Amplitude, Hightouch, Fivetran, Census, RudderStack, Segment)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// =============================================================================
// Real-Time Data Export Sinks
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum DataExportSinkKind {
    S3Parquet,
    Kinesis,
    Pubsub,
    Snowflake,
    Segment,
    WebhookNdjson,
}

impl DataExportSinkKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::S3Parquet => "s3_parquet",
            Self::Kinesis => "kinesis",
            Self::Pubsub => "pubsub",
            Self::Snowflake => "snowflake",
            Self::Segment => "segment",
            Self::WebhookNdjson => "webhook_ndjson",
        }
    }

    pub fn parse_str(raw: &str) -> Option<Self> {
        match raw {
            "s3_parquet" => Some(Self::S3Parquet),
            "kinesis" => Some(Self::Kinesis),
            "pubsub" => Some(Self::Pubsub),
            "snowflake" => Some(Self::Snowflake),
            "segment" => Some(Self::Segment),
            "webhook_ndjson" => Some(Self::WebhookNdjson),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DataExportSink {
    pub id: Uuid,
    pub org_id: Uuid,
    pub env_id: Option<Uuid>,
    pub name: String,
    pub kind: DataExportSinkKind,
    pub destination: String,
    pub enabled: bool,
    #[serde(default)]
    pub config: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_exported_at: Option<DateTime<Utc>>,
    pub exported_event_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DataExportFlushResult {
    pub sink_id: Uuid,
    pub events_exported: i64,
    pub bytes_written: i64,
    pub location: String,
    pub flushed_at: DateTime<Utc>,
}

// =============================================================================
// Experimentation Core: Metrics, Metric Groups (Funnels), Mutual Exclusion Layers
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum MetricKind {
    Conversion,
    Numeric,
    Latency,
    ErrorRate,
}

impl MetricKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Conversion => "conversion",
            Self::Numeric => "numeric",
            Self::Latency => "latency",
            Self::ErrorRate => "error_rate",
        }
    }

    pub fn parse_str(raw: &str) -> Option<Self> {
        match raw {
            "conversion" => Some(Self::Conversion),
            "numeric" => Some(Self::Numeric),
            "latency" => Some(Self::Latency),
            "error_rate" => Some(Self::ErrorRate),
            _ => None,
        }
    }

    pub fn is_binary(&self) -> bool {
        matches!(self, Self::Conversion | Self::ErrorRate)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum MetricSuccessDirection {
    HigherIsBetter,
    LowerIsBetter,
}

impl MetricSuccessDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::HigherIsBetter => "higher_is_better",
            Self::LowerIsBetter => "lower_is_better",
        }
    }

    pub fn parse_str(raw: &str) -> Option<Self> {
        match raw {
            "higher_is_better" => Some(Self::HigherIsBetter),
            "lower_is_better" => Some(Self::LowerIsBetter),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum MetricAggregation {
    Average,
    Sum,
    Count,
    P95,
}

impl MetricAggregation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Average => "average",
            Self::Sum => "sum",
            Self::Count => "count",
            Self::P95 => "p95",
        }
    }

    pub fn parse_str(raw: &str) -> Option<Self> {
        match raw {
            "average" => Some(Self::Average),
            "sum" => Some(Self::Sum),
            "count" => Some(Self::Count),
            "p95" => Some(Self::P95),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MetricDefinition {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub kind: MetricKind,
    /// SDK custom event key tracked via `client.track(event_key, ctx, data, value)`.
    pub event_key: String,
    pub success_direction: MetricSuccessDirection,
    pub aggregation: MetricAggregation,
    #[serde(default)]
    pub unit: String,
    pub randomization_unit: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum MetricGroupKind {
    Funnel,
    Collection,
}

impl MetricGroupKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Funnel => "funnel",
            Self::Collection => "collection",
        }
    }

    pub fn parse_str(raw: &str) -> Option<Self> {
        match raw {
            "funnel" => Some(Self::Funnel),
            "collection" => Some(Self::Collection),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MetricGroup {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub key: String,
    pub name: String,
    pub kind: MetricGroupKind,
    #[serde(default)]
    pub description: String,
    /// Ordered metric keys (step order matters for `funnel` groups).
    pub metric_keys: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Non-overlapping traffic space (`0..100_000` basis points) isolating concurrent experiments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MutualExclusionLayer {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub salt: String,
    pub total_buckets: i32,
    pub allocated_buckets: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// =============================================================================
// Experiments & Statistical Results
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ExperimentMode {
    AbTest,
    AaTest,
}

impl ExperimentMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AbTest => "ab_test",
            Self::AaTest => "aa_test",
        }
    }

    pub fn parse_str(raw: &str) -> Option<Self> {
        match raw {
            "ab_test" => Some(Self::AbTest),
            "aa_test" => Some(Self::AaTest),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum StatisticalEngine {
    Frequentist,
    Bayesian,
}

impl StatisticalEngine {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Frequentist => "frequentist",
            Self::Bayesian => "bayesian",
        }
    }

    pub fn parse_str(raw: &str) -> Option<Self> {
        match raw {
            "frequentist" => Some(Self::Frequentist),
            "bayesian" => Some(Self::Bayesian),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum ExperimentStatus {
    Draft,
    Running,
    Stopped,
    Archived,
}

impl ExperimentStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Archived => "archived",
        }
    }

    pub fn parse_str(raw: &str) -> Option<Self> {
        match raw {
            "draft" => Some(Self::Draft),
            "running" => Some(Self::Running),
            "stopped" => Some(Self::Stopped),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ExperimentVariationWeight {
    pub variation_index: i32,
    pub weight: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct Experiment {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub env_id: Uuid,
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub hypothesis: String,
    pub flag_key: String,
    pub mode: ExperimentMode,
    pub engine: StatisticalEngine,
    pub status: ExperimentStatus,
    pub randomization_unit: String,
    pub control_variation_index: i32,
    pub primary_metric_key: String,
    #[serde(default)]
    pub secondary_metric_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_group_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_bucket_start: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_bucket_end: Option<i32>,
    /// Expected allocation weights in basis points summing to 100_000 (used for SRM).
    pub allocations: Vec<ExperimentVariationWeight>,
    pub alpha: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Compact sufficient statistics per variation (`n`, `conversions`, `sum`, `sum_sq`),
/// allowing Frequentist & Bayesian inference in O(1) without pulling raw rows from OLAP.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct VariationSufficientStats {
    pub variation_index: i32,
    pub exposures: i64,
    pub conversions: i64,
    pub sum: f64,
    pub sum_sq: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SrmCheckResult {
    pub chi_square: f64,
    pub p_value: f64,
    pub srm_detected: bool,
    pub expected_counts: Vec<f64>,
    pub observed_counts: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct VariationResult {
    pub variation_index: i32,
    pub is_control: bool,
    pub exposures: i64,
    pub conversions: i64,
    pub mean: f64,
    pub std_err: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_lift: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ci_lower: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ci_upper: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probability_to_beat_control: Option<f64>,
    pub statistically_significant: bool,
    pub winner: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MetricExperimentResult {
    pub metric_key: String,
    pub metric_name: String,
    pub kind: MetricKind,
    pub success_direction: MetricSuccessDirection,
    pub variations: Vec<VariationResult>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ExperimentResults {
    pub experiment_key: String,
    pub flag_key: String,
    pub mode: ExperimentMode,
    pub engine: StatisticalEngine,
    pub status: ExperimentStatus,
    pub total_exposures: i64,
    pub srm: SrmCheckResult,
    pub primary: MetricExperimentResult,
    #[serde(default)]
    pub secondary: Vec<MetricExperimentResult>,
    pub computed_at: DateTime<Utc>,
}

// =============================================================================
// Guarded Rollouts, Adaptive Triggers, Feature Monitoring & Engineering Insights
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum GuardedRolloutStatus {
    Monitoring,
    Completed,
    RolledBack,
    Paused,
}

impl GuardedRolloutStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Monitoring => "monitoring",
            Self::Completed => "completed",
            Self::RolledBack => "rolled_back",
            Self::Paused => "paused",
        }
    }

    pub fn parse_str(raw: &str) -> Option<Self> {
        match raw {
            "monitoring" => Some(Self::Monitoring),
            "completed" => Some(Self::Completed),
            "rolled_back" => Some(Self::RolledBack),
            "paused" => Some(Self::Paused),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GuardrailMetricRule {
    pub metric_key: String,
    /// Relative degradation percentage threshold (e.g. `10.0` = 10% worse than control triggers alert/rollback).
    pub max_regression_pct: f64,
    #[serde(default = "default_min_samples")]
    pub min_sample_size: i64,
}

fn default_min_samples() -> i64 {
    20
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GuardedRollout {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub env_id: Uuid,
    pub flag_key: String,
    pub name: String,
    pub status: GuardedRolloutStatus,
    pub control_variation_index: i32,
    pub treatment_variation_index: i32,
    pub current_traffic_pct: i32,
    pub auto_rollback: bool,
    pub guardrails: Vec<GuardrailMetricRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_evaluated_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_verdict: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GuardrailMetricCheck {
    pub metric_key: String,
    pub control_mean: f64,
    pub treatment_mean: f64,
    pub control_samples: i64,
    pub treatment_samples: i64,
    pub regression_pct: f64,
    pub p_value: Option<f64>,
    pub regressed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GuardedRolloutEvaluation {
    pub rollout: GuardedRollout,
    pub srm: SrmCheckResult,
    pub checks: Vec<GuardrailMetricCheck>,
    pub regression_detected: bool,
    pub auto_rolled_back: bool,
    pub evaluated_at: DateTime<Utc>,
}

/// Per-variation health metrics displayed directly on Flag Detail pages (Feature Monitoring).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct FlagVariationMonitoring {
    pub variation_index: i32,
    pub exposures: i64,
    pub error_count: i64,
    pub error_rate: f64,
    pub avg_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub conversion_count: i64,
    pub conversion_rate: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct FlagMonitoringSummary {
    pub flag_key: String,
    pub lookback_hours: i64,
    pub variations: Vec<FlagVariationMonitoring>,
    pub generated_at: DateTime<Utc>,
}

/// Engineering Insights: DORA velocity & stability metrics + Flag Health.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct EngineeringInsights {
    pub project_key: String,
    pub lookback_days: i64,
    pub flag_deployments_count: i64,
    pub deployments_per_day: f64,
    pub rollbacks_count: i64,
    pub change_failure_rate_pct: f64,
    pub mean_time_to_recovery_minutes: f64,
    pub total_flags: i64,
    pub temporary_flags: i64,
    pub stale_flags: i64,
    pub active_experiments: i64,
    pub active_guarded_rollouts: i64,
    pub rolled_back_guardrails: i64,
    pub computed_at: DateTime<Utc>,
}

// =============================================================================
// 2/4/6-Stage Migration Flags
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum MigrationStage {
    Off,
    Dualwrite,
    Shadow,
    Live,
    Rampdown,
    Complete,
}

impl MigrationStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Dualwrite => "dualwrite",
            Self::Shadow => "shadow",
            Self::Live => "live",
            Self::Rampdown => "rampdown",
            Self::Complete => "complete",
        }
    }

    pub fn parse_str(raw: &str) -> Option<Self> {
        match raw {
            "off" => Some(Self::Off),
            "dualwrite" => Some(Self::Dualwrite),
            "shadow" => Some(Self::Shadow),
            "live" => Some(Self::Live),
            "rampdown" => Some(Self::Rampdown),
            "complete" => Some(Self::Complete),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum MigrationTemplate {
    TwoStage,
    FourStage,
    SixStage,
}

impl MigrationTemplate {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TwoStage => "two_stage",
            Self::FourStage => "four_stage",
            Self::SixStage => "six_stage",
        }
    }

    pub fn parse_str(raw: &str) -> Option<Self> {
        match raw {
            "two_stage" => Some(Self::TwoStage),
            "four_stage" => Some(Self::FourStage),
            "six_stage" => Some(Self::SixStage),
            _ => None,
        }
    }

    pub fn allowed_stages(&self) -> &'static [MigrationStage] {
        match self {
            Self::TwoStage => &[MigrationStage::Off, MigrationStage::Complete],
            Self::FourStage => &[
                MigrationStage::Off,
                MigrationStage::Shadow,
                MigrationStage::Live,
                MigrationStage::Complete,
            ],
            Self::SixStage => &[
                MigrationStage::Off,
                MigrationStage::Dualwrite,
                MigrationStage::Shadow,
                MigrationStage::Live,
                MigrationStage::Rampdown,
                MigrationStage::Complete,
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MigrationFlagConfig {
    pub id: Uuid,
    pub org_id: Uuid,
    pub env_id: Uuid,
    pub flag_key: String,
    pub template: MigrationTemplate,
    pub current_stage: MigrationStage,
    /// Consistency check sampling ratio (`1` = every call, `10` = 1 in 10, `0` = disabled).
    pub check_ratio: i32,
    /// Read execution order: `"parallel"`, `"serial"`, or `"random"`.
    pub read_execution_order: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MigrationStageMetrics {
    pub stage: String,
    pub old_invocations: i64,
    pub new_invocations: i64,
    pub old_errors: i64,
    pub new_errors: i64,
    pub old_avg_latency_ms: f64,
    pub new_avg_latency_ms: f64,
    pub consistency_matches: i64,
    pub consistency_mismatches: i64,
    pub consistency_rate_pct: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MigrationTelemetrySummary {
    pub flag_key: String,
    pub stages: Vec<MigrationStageMetrics>,
    pub generated_at: DateTime<Utc>,
}

// =============================================================================
// Synced Segments (External Warehouse & CDP Reverse-ETL Push)
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum SyncedSegmentProvider {
    Amplitude,
    Hightouch,
    Fivetran,
    Census,
    Rudderstack,
    Segment,
    Webhook,
}

impl SyncedSegmentProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Amplitude => "amplitude",
            Self::Hightouch => "hightouch",
            Self::Fivetran => "fivetran",
            Self::Census => "census",
            Self::Rudderstack => "rudderstack",
            Self::Segment => "segment",
            Self::Webhook => "webhook",
        }
    }

    pub fn parse_str(raw: &str) -> Option<Self> {
        match raw {
            "amplitude" => Some(Self::Amplitude),
            "hightouch" => Some(Self::Hightouch),
            "fivetran" => Some(Self::Fivetran),
            "census" => Some(Self::Census),
            "rudderstack" => Some(Self::Rudderstack),
            "segment" => Some(Self::Segment),
            "webhook" => Some(Self::Webhook),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SyncedSegmentIntegration {
    pub id: Uuid,
    pub org_id: Uuid,
    pub env_id: Uuid,
    pub segment_key: String,
    pub provider: SyncedSegmentProvider,
    pub external_cohort_id: String,
    pub context_kind: String,
    pub token_prefix: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_synced_at: Option<DateTime<Utc>>,
    pub synced_member_count: i64,
    pub created_at: DateTime<Utc>,
}
