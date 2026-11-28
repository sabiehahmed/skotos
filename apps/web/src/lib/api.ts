import {
  createSkotosClient,
  SkotosApiError,
  unwrapApiResult,
  type components,
} from "@skotos/api-client";

export type AuthConfig = components["schemas"]["AuthConfigResponse"];
export type AuthMe = components["schemas"]["AuthMeResponse"];
export type Project = components["schemas"]["Project"];
export type Environment = components["schemas"]["Environment"];
export type EnvironmentKey = components["schemas"]["EnvironmentKey"];
export type FlagResponse = components["schemas"]["FlagResponse"];
export type FlagEnvConfig = components["schemas"]["FlagEnvConfig"];
export type FlagEnvRevision = components["schemas"]["FlagEnvRevision"];
export type FlagPatchInstruction = components["schemas"]["FlagPatchInstruction"];
export type PatchFlagRequest = components["schemas"]["PatchFlagRequest"];
export type SimulateFlagResponse = components["schemas"]["SimulateFlagResponse"];
export type AuditLogEntry = components["schemas"]["AuditLogEntry"];
export type ApiToken = components["schemas"]["ApiToken"];
export type Member = components["schemas"]["Member"];
export type BuiltInRole = components["schemas"]["BuiltInRole"];
export type FlagKind = components["schemas"]["FlagKind"];
export type Variation = components["schemas"]["Variation"];
export type ClauseOp = components["schemas"]["ClauseOp"];
export type Segment = components["schemas"]["Segment"];
export type ScheduledChange = components["schemas"]["ScheduledChange"];
export type FlagTrigger = components["schemas"]["FlagTrigger"];
export type TriggerAction = components["schemas"]["TriggerAction"];
export type Webhook = components["schemas"]["Webhook"];
export type WebhookDelivery = components["schemas"]["WebhookDelivery"];
export type WebhookIntegrationKind = components["schemas"]["WebhookIntegrationKind"];
export type FlagEvalUsage = components["schemas"]["FlagEvalUsage"];
export type CustomRole = components["schemas"]["CustomRole"];
export type PolicyEffect = components["schemas"]["PolicyEffect"];
export type PolicyStatement = components["schemas"]["PolicyStatement"];
export type Team = components["schemas"]["Team"];
export type ApprovalRequest = components["schemas"]["ApprovalRequest"];
export type ApprovalReview = components["schemas"]["ApprovalReview"];
export type ApprovalReviewKind = components["schemas"]["ApprovalReviewKind"];
export type ApprovalStatus = components["schemas"]["ApprovalStatus"];
export type EnvironmentApprovalSettings = components["schemas"]["EnvironmentApprovalSettings"];
export type ReleasePipeline = components["schemas"]["ReleasePipeline"];
export type ReleasePhase = components["schemas"]["ReleasePhase"];
export type FlagRelease = components["schemas"]["FlagRelease"];
export type FlagWorkflow = components["schemas"]["FlagWorkflow"];
export type WorkflowStage = components["schemas"]["WorkflowStage"];
export type SsoProtocol = components["schemas"]["SsoProtocol"];
export type SsoProviderConfig = components["schemas"]["SsoProviderConfig"];
export type AuditArchiveRecord = components["schemas"]["AuditArchiveRecord"];
export type FlagCodeRefSummary = components["schemas"]["FlagCodeRefSummary"];
export type CodeRefHunk = components["schemas"]["CodeRefHunk"];
export type Application = components["schemas"]["Application"];
export type ApplicationKind = components["schemas"]["ApplicationKind"];
export type LiveEvalDebugEvent = components["schemas"]["LiveEvalDebugEvent"];
export type MetricKind = components["schemas"]["MetricKind"];
export type MetricSuccessDirection = components["schemas"]["MetricSuccessDirection"];
export type MetricAggregation = components["schemas"]["MetricAggregation"];
export type MetricDefinition = components["schemas"]["MetricDefinition"];
export type MetricGroupKind = components["schemas"]["MetricGroupKind"];
export type MetricGroup = components["schemas"]["MetricGroup"];
export type MutualExclusionLayer = components["schemas"]["MutualExclusionLayer"];
export type ExperimentMode = components["schemas"]["ExperimentMode"];
export type StatisticalEngine = components["schemas"]["StatisticalEngine"];
export type ExperimentStatus = components["schemas"]["ExperimentStatus"];
export type Experiment = components["schemas"]["Experiment"];
export type ExperimentResults = components["schemas"]["ExperimentResults"];
export type GuardedRolloutStatus = components["schemas"]["GuardedRolloutStatus"];
export type GuardrailMetricRule = components["schemas"]["GuardrailMetricRule"];
export type GuardedRollout = components["schemas"]["GuardedRollout"];
export type GuardedRolloutEvaluation = components["schemas"]["GuardedRolloutEvaluation"];
export type FlagVariationMonitoring = components["schemas"]["FlagVariationMonitoring"];
export type FlagMonitoringSummary = components["schemas"]["FlagMonitoringSummary"];
export type EngineeringInsights = components["schemas"]["EngineeringInsights"];
export type MigrationStage = components["schemas"]["MigrationStage"];
export type MigrationTemplate = components["schemas"]["MigrationTemplate"];
export type MigrationFlagConfig = components["schemas"]["MigrationFlagConfig"];
export type MigrationTelemetrySummary = components["schemas"]["MigrationTelemetrySummary"];
export type SyncedSegmentProvider = components["schemas"]["SyncedSegmentProvider"];
export type SyncedSegmentIntegration = components["schemas"]["SyncedSegmentIntegration"];
export type DataExportSinkKind = components["schemas"]["DataExportSinkKind"];
export type DataExportSink = components["schemas"]["DataExportSink"];

export const API_BASE_URL =
  import.meta.env.VITE_API_BASE_URL ||
  (typeof window !== "undefined" && window.location?.origin && window.location.origin !== "null"
    ? window.location.origin
    : "http://localhost:3000");

export const skotosApi = createSkotosClient({ baseUrl: API_BASE_URL });

export { SkotosApiError, unwrapApiResult };

export function canWrite(role?: BuiltInRole): boolean {
  return role === "writer" || role === "admin" || role === "owner";
}

export function canAdmin(role?: BuiltInRole): boolean {
  return role === "admin" || role === "owner";
}

export function canOwner(role?: BuiltInRole): boolean {
  return role === "owner";
}

export function formatErrorMessage(err: unknown): string {
  if (err instanceof SkotosApiError) {
    if (err.errors && err.errors.length > 0) {
      return `${err.detail} (${err.errors.join(", ")})`;
    }
    return err.detail;
  }
  if (err instanceof Error) return err.message;
  return "An unexpected error occurred";
}
