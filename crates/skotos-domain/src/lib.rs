//! Core Skotos domain entities, multi-context model, validation, evaluation
//! compilation, and semantic flag patch engine.

pub mod context;
pub mod entities;
pub mod error;
pub mod eval_model;
pub mod patch;
pub mod phase3;
pub mod phase4;
pub mod segment_patch;

pub use context::{validate_context_kind, AttributeRef, Context, ContextMeta, SingleContext};
pub use entities::{
    default_context_kind, validate_hex_color, validate_key, ApiToken, AuditLogEntry, BuiltInRole,
    Clause, ClauseOp, ClientSideAvailability, ContextAttributeCatalogEntry, ContextCatalog,
    ContextKindCatalogEntry, Environment, EnvironmentAuthRecord, EnvironmentKey,
    EnvironmentKeyHashRecord, EnvironmentKeyKind, FeatureFlag, FlagEnvConfig, FlagEnvConfigBody,
    FlagEnvRevision, FlagEvalUsage, FlagKind, FlagTrigger, Member, Organization, OutboxEvent,
    PlanTier, Prerequisite, Project, Rollout, RolloutKind, ScheduledChange, ScheduledChangeStatus,
    Segment, SegmentKind, SegmentRule, SegmentTarget, Target, TargetingRule, TriggerAction,
    Variation, VariationOrRollout, VariationUsageCount, Webhook, WebhookDelivery,
    WeightedVariation,
};
pub use error::DomainError;
pub use eval_model::{
    compile_eval_flag, compile_eval_flag_with_flags, compile_eval_segment, EvalFlag,
    EvalPrerequisite, EvalRollout, EvalRule, EvalSegment, EvalTarget, EvalVariationOrRollout,
    EvalWeightedVariation, EvaluationDetail, EvaluationErrorKind, EvaluationReason,
};
pub use patch::{
    apply_flag_patch, detect_prerequisite_cycle, validate_clause, validate_flag_env_config,
    validate_rollout, validate_variation_or_rollout, variation_in_use_locations,
    FlagPatchInstruction, PatchResult, PatchSummaryItem, ROLLOUT_TOTAL_WEIGHT,
};
pub use phase3::{
    big_segment_key_hash, Application, ApplicationKind, ApplicationVersion, ApprovalRequest,
    ApprovalReview, ApprovalReviewKind, ApprovalStatus, AuditArchiveRecord, BigSegmentSnapshot,
    BigSegmentStatus, CodeRefHunk, CustomRole, EnvironmentApprovalSettings, FlagCodeRefSummary,
    FlagRelease, FlagReleasePhaseState, FlagWorkflow, LiveEvalDebugEvent, PolicyEffect,
    PolicyStatement, ReleasePhase, ReleasePipeline, ScheduledSegmentChange, SsoProtocol,
    SsoProviderConfig, Team, WebhookIntegrationKind, WorkflowStage, WorkflowStageStatus,
    WorkflowStatus,
};
pub use phase4::{
    DataExportFlushResult, DataExportSink, DataExportSinkKind, EngineeringInsights, Experiment,
    ExperimentMode, ExperimentResults, ExperimentStatus, ExperimentVariationWeight,
    FlagMonitoringSummary, FlagVariationMonitoring, GuardedRollout, GuardedRolloutEvaluation,
    GuardedRolloutStatus, GuardrailMetricCheck, GuardrailMetricRule, MetricAggregation,
    MetricDefinition, MetricExperimentResult, MetricGroup, MetricGroupKind, MetricKind,
    MetricSuccessDirection, MigrationFlagConfig, MigrationStage, MigrationStageMetrics,
    MigrationTelemetrySummary, MigrationTemplate, MutualExclusionLayer, SrmCheckResult,
    StatisticalEngine, SyncedSegmentIntegration, SyncedSegmentProvider, VariationResult,
    VariationSufficientStats,
};
pub use segment_patch::{
    apply_segment_patch, validate_segment, SegmentPatchInstruction, SegmentPatchResult,
};

/// Canonical crate identifier for workspace wiring verification.
pub const CRATE_NAME: &str = "skotos-domain";

/// Returns the semantic version of the `skotos-domain` crate.
pub fn crate_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::{crate_version, CRATE_NAME};

    #[test]
    fn exposes_crate_metadata() {
        assert_eq!(CRATE_NAME, "skotos-domain");
        assert!(!crate_version().is_empty());
    }
}
