// ============================================================================
// Skotos Phase 1 Opaque-Box E2E Types & Canonical Domain/API Contracts
// Pure TypeScript types/constants — zero imports from product packages/crates
// and zero runtime dependency on @playwright/test.
// ============================================================================

export type Uuid = string;
export type IsoTimestamp = string;

export type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

export const BUILT_IN_ROLES = ['owner', 'admin', 'writer', 'reader'] as const;
export type BuiltInRole = (typeof BUILT_IN_ROLES)[number];

export const PLAN_TIERS = ['free', 'pro', 'team', 'enterprise'] as const;
export type PlanTier = (typeof PLAN_TIERS)[number];

export const FLAG_KINDS = ['boolean', 'string', 'number', 'json'] as const;
export type FlagKind = (typeof FLAG_KINDS)[number];

export const ENVIRONMENT_KEY_KINDS = ['sdk', 'mobile'] as const;
export type EnvironmentKeyKind = (typeof ENVIRONMENT_KEY_KINDS)[number];

export const ROLLOUT_KINDS = ['rollout', 'experiment'] as const;
export type RolloutKind = (typeof ROLLOUT_KINDS)[number];

export const SEGMENT_KINDS = ['standard', 'big', 'synced'] as const;
export type SegmentKind = (typeof SEGMENT_KINDS)[number];

export const ACTOR_TYPES = ['member', 'api_token', 'system'] as const;
export type ActorType = (typeof ACTOR_TYPES)[number];

export type AuthMode = 'self_hosted' | 'external' | 'external_oidc';

export const KEY_PREFIXES = {
  sdk: 'sk_sdk_',
  mobile: 'sk_mob_',
  clientSideId: 'sk_cid_',
  apiToken: 'sk_api_',
} as const;

export const ROLLOUT_TOTAL_WEIGHT = 100_000 as const;

// ---- Pagination & RFC 9457 Problem Details ----

export interface CursorPage<T> {
  items: T[];
  nextCursor: string | null;
  hasMore: boolean;
}

export type PaginatedResponse<T> = CursorPage<T>;

export interface ProblemFieldError {
  field: string;
  message: string;
}

export interface ProblemDetails {
  type: string;
  title: string;
  status: number;
  detail: string;
  instance: string;
  code: string;
  currentVersion?: number | null;
  errors?: ProblemFieldError[] | null;
}

export const PROBLEM_CODES = {
  BAD_REQUEST: 'BAD_REQUEST',
  UNAUTHORIZED: 'UNAUTHORIZED',
  FORBIDDEN: 'FORBIDDEN',
  NOT_FOUND: 'NOT_FOUND',
  VERSION_CONFLICT: 'VERSION_CONFLICT',
  ALREADY_EXISTS: 'ALREADY_EXISTS',
  VALIDATION_ERROR: 'VALIDATION_ERROR',
  COMMENT_REQUIRED: 'COMMENT_REQUIRED',
  VARIATION_IN_USE: 'VARIATION_IN_USE',
  PREREQUISITE_CYCLE: 'PREREQUISITE_CYCLE',
  FORBIDDEN_LAST_OWNER: 'FORBIDDEN_LAST_OWNER',
  INTERNAL_ERROR: 'INTERNAL_ERROR',
  SERVICE_UNAVAILABLE: 'SERVICE_UNAVAILABLE',
} as const;

export type ProblemCode = (typeof PROBLEM_CODES)[keyof typeof PROBLEM_CODES];

// ---- 15 Canonical Clause Operators ----

export const CLAUSE_OPS = [
  // Equality & Membership (1)
  'in',
  // String (4)
  'startsWith',
  'endsWith',
  'contains',
  'matches',
  // Numeric (4)
  'lessThan',
  'lessThanOrEqual',
  'greaterThan',
  'greaterThanOrEqual',
  // Date / Time (2)
  'before',
  'after',
  // Semantic Version (3)
  'semverEqual',
  'semverLessThan',
  'semverGreaterThan',
  // Segment (1)
  'segmentMatch',
] as const;

export type ClauseOp = (typeof CLAUSE_OPS)[number];

export interface Clause {
  id?: string | null;
  contextKind?: string;
  attribute: string;
  op: ClauseOp;
  values: JsonValue[];
  negate?: boolean;
}

export interface WeightedVariation {
  variationId: Uuid;
  weight: number;
  untracked?: boolean;
}

export interface Rollout {
  kind?: RolloutKind;
  contextKind?: string;
  bucketBy?: string;
  seed?: number | null;
  variations: WeightedVariation[];
}

export type VariationOrRollout =
  | { variationId: Uuid; rollout?: undefined | null }
  | { rollout: Rollout; variationId?: undefined | null };

export type Fallthrough = VariationOrRollout;

export interface TargetingRule {
  id: string;
  description?: string | null;
  clauses: Clause[];
  variationId?: Uuid | null;
  rollout?: Rollout | null;
  trackEvents?: boolean;
}

export type Rule = TargetingRule;

export interface Target {
  contextKind: string;
  variationId: Uuid;
  values: string[];
}

export interface Prerequisite {
  key: string;
  variationId: Uuid;
}

export interface Variation {
  id: Uuid;
  value: JsonValue;
  name?: string | null;
  description?: string | null;
}

export interface ClientSideAvailability {
  usingEnvironmentId: boolean;
  usingMobileKey: boolean;
}

export interface Flag {
  id: Uuid;
  projectId: Uuid;
  key: string;
  name: string;
  description: string;
  kind: FlagKind;
  variations: Variation[];
  temporary: boolean;
  clientSideAvailability: ClientSideAvailability;
  tags: string[];
  maintainerId: Uuid | null;
  archived: boolean;
  version: number;
  createdAt: IsoTimestamp;
  updatedAt: IsoTimestamp;
}

export type FeatureFlag = Flag;

export interface FlagEnvConfig {
  flagId?: Uuid;
  envId?: Uuid;
  environmentKey?: string;
  version: number;
  on: boolean;
  salt: string;
  offVariation: Uuid | null;
  prerequisites: Prerequisite[];
  targets: Target[];
  rules: TargetingRule[];
  fallthrough: Fallthrough;
  trackEvents: boolean;
  trackEventsFallthrough: boolean;
  debugEventsUntilDate?: number | null;
  updatedAt: IsoTimestamp;
}

export interface FlagEnvSummary {
  environmentKey?: string;
  envId?: Uuid;
  version: number;
  on: boolean;
  updatedAt?: IsoTimestamp;
}

export interface FlagDetail extends Flag {
  environments: Record<string, FlagEnvConfig>;
}

export interface FlagListItem extends Flag {
  environments?: Record<string, FlagEnvSummary>;
}

// ---- All 23 Semantic FlagPatchInstruction Variants ----

export const FLAG_PATCH_INSTRUCTION_KINDS = [
  // Env-scoped (12)
  'turnFlagOn',
  'turnFlagOff',
  'updateOffVariation',
  'addTargets',
  'removeTargets',
  'addRule',
  'updateRule',
  'removeRule',
  'reorderRules',
  'updateFallthrough',
  'addPrerequisite',
  'removePrerequisite',
  // Flag-scoped core (6)
  'addVariation',
  'updateVariation',
  'removeVariation',
  'updateClientSideAvailability',
  'archiveFlag',
  'restoreFlag',
  // Flag-scoped metadata (5)
  'updateName',
  'updateDescription',
  'updateTags',
  'updateMaintainer',
  'updateTemporary',
] as const;

export type FlagPatchInstructionKind = (typeof FLAG_PATCH_INSTRUCTION_KINDS)[number];

export type FlagPatchInstruction =
  | { kind: 'turnFlagOn' }
  | { kind: 'turnFlagOff' }
  | { kind: 'updateOffVariation'; variationId: Uuid | null }
  | { kind: 'addTargets'; contextKind?: string; variationId: Uuid; values: string[] }
  | { kind: 'removeTargets'; contextKind?: string; variationId: Uuid; values: string[] }
  | {
      kind: 'addRule';
      ruleId?: string;
      description?: string;
      clauses: Clause[];
      variationId?: Uuid;
      rollout?: Rollout;
      beforeRuleId?: string;
      trackEvents?: boolean;
    }
  | {
      kind: 'updateRule';
      ruleId: string;
      description?: string;
      clauses?: Clause[];
      variationId?: Uuid;
      rollout?: Rollout;
      trackEvents?: boolean;
    }
  | { kind: 'removeRule'; ruleId: string }
  | { kind: 'reorderRules'; ruleIds: string[] }
  | { kind: 'updateFallthrough'; variationId?: Uuid; rollout?: Rollout }
  | { kind: 'addPrerequisite'; key: string; variationId: Uuid }
  | { kind: 'removePrerequisite'; key: string }
  | {
      kind: 'addVariation';
      variationId?: Uuid;
      value: JsonValue;
      name?: string;
      description?: string;
    }
  | {
      kind: 'updateVariation';
      variationId: Uuid;
      value?: JsonValue;
      name?: string;
      description?: string;
    }
  | { kind: 'removeVariation'; variationId: Uuid }
  | {
      kind: 'updateClientSideAvailability';
      usingEnvironmentId: boolean;
      usingMobileKey: boolean;
    }
  | { kind: 'archiveFlag' }
  | { kind: 'restoreFlag' }
  | { kind: 'updateName'; name: string }
  | { kind: 'updateDescription'; description: string }
  | { kind: 'updateTags'; tags: string[] }
  | { kind: 'updateMaintainer'; maintainerId: Uuid | null }
  | { kind: 'updateTemporary'; temporary: boolean };

// ---- Canonical Context, EvaluationDetail & EvaluationReason ----

export interface ContextMeta {
  privateAttributes?: string[];
  secondary?: string;
  redactedAttributes?: string[];
}

export interface SingleContext {
  kind: string;
  key: string;
  name?: string;
  anonymous?: boolean;
  _meta?: ContextMeta;
  [attribute: string]: JsonValue | ContextMeta | undefined;
}

export interface MultiContextKindEntry {
  key: string;
  name?: string;
  anonymous?: boolean;
  _meta?: ContextMeta;
  [attribute: string]: JsonValue | ContextMeta | undefined;
}

export interface MultiContext {
  kind: 'multi';
  [contextKind: string]: 'multi' | MultiContextKindEntry;
}

export type Context = SingleContext | MultiContext;

export const EVALUATION_REASON_KINDS = [
  'OFF',
  'TARGET_MATCH',
  'RULE_MATCH',
  'PREREQUISITE_FAILED',
  'FALLTHROUGH',
  'ERROR',
] as const;

export type EvaluationReasonKind = (typeof EVALUATION_REASON_KINDS)[number];

export const EVALUATION_ERROR_KINDS = [
  'FLAG_NOT_FOUND',
  'WRONG_TYPE',
  'MALFORMED_FLAG',
  'INVALID_CONTEXT',
  'CLIENT_NOT_READY',
  'EXCEPTION',
] as const;

export type EvaluationErrorKind = (typeof EVALUATION_ERROR_KINDS)[number];

export type EvaluationReason =
  | { kind: 'OFF' }
  | { kind: 'TARGET_MATCH' }
  | { kind: 'RULE_MATCH'; ruleIndex: number; ruleId: string; inExperiment?: boolean }
  | { kind: 'PREREQUISITE_FAILED'; prerequisiteKey: string }
  | { kind: 'FALLTHROUGH'; inExperiment?: boolean }
  | { kind: 'ERROR'; errorKind: EvaluationErrorKind };

export interface EvaluationDetail<T extends JsonValue = JsonValue> {
  value: T;
  variationIndex: number | null;
  variationId?: Uuid | null;
  variationName?: string | null;
  reason: EvaluationReason;
}

// ---- Phase 1 REST DTOs ----

export interface HealthzResponse {
  status: string;
}

export interface ReadyzResponse {
  status: string;
  checks: {
    postgres: string;
    [check: string]: string;
  };
}

export interface Organization {
  id: Uuid;
  slug: string;
  name: string;
  externalAuthOrgId?: string | null;
  planTier: PlanTier;
  createdAt: IsoTimestamp;
}

export interface Member {
  id: Uuid;
  orgId: Uuid;
  externalSubjectId?: string | null;
  email: string;
  displayName: string;
  role: BuiltInRole;
  createdAt: IsoTimestamp;
  updatedAt?: IsoTimestamp;
}

export interface AuthenticatedPrincipalDto {
  orgId: Uuid;
  actorType: ActorType;
  memberId?: Uuid | null;
  apiTokenId?: Uuid | null;
  email?: string | null;
  displayName?: string | null;
  role: BuiltInRole;
  teamIds?: Uuid[];
  roleIds?: Uuid[];
  tokenScopes?: string[];
}

export interface AuthConfigResponse {
  mode: AuthMode;
  externalLoginUrl?: string | null;
}

export interface LoginRequest {
  email: string;
  password: string;
}

export interface LoginResponse {
  member: Member;
  organization: Organization;
}

export interface AuthMeResponse {
  principal: AuthenticatedPrincipalDto;
  member?: Member | null;
  organization: Organization;
}

export interface Project {
  id: Uuid;
  orgId: Uuid;
  key: string;
  name: string;
  defaultClientSideAvailability: ClientSideAvailability;
  createdAt: IsoTimestamp;
}

export interface ProjectDetail extends Project {
  environments?: EnvironmentSummary[];
}

export interface CreateProjectRequest {
  key: string;
  name: string;
  defaultClientSideAvailability?: ClientSideAvailability;
  environments?: CreateEnvironmentRequest[];
}

export interface UpdateProjectRequest {
  name?: string;
  defaultClientSideAvailability?: ClientSideAvailability;
}

export interface EnvironmentKey {
  id: Uuid;
  envId: Uuid;
  kind: EnvironmentKeyKind;
  keyPrefix: string;
  expiresAt: IsoTimestamp | null;
  createdAt: IsoTimestamp;
}

export interface Environment {
  id: Uuid;
  projectId: Uuid;
  key: string;
  name: string;
  color: string;
  clientSideId: string;
  requireComments: boolean;
  confirmChanges: boolean;
  revision: number;
  createdAt: IsoTimestamp;
}

export interface EnvironmentSummary extends Environment {
  keys?: EnvironmentKey[];
}

export interface EnvironmentDetail extends Environment {
  keys: EnvironmentKey[];
}

export interface CreateEnvironmentRequest {
  key: string;
  name: string;
  color?: string;
  requireComments?: boolean;
  confirmChanges?: boolean;
}

export interface UpdateEnvironmentRequest {
  name?: string;
  color?: string;
  requireComments?: boolean;
  confirmChanges?: boolean;
}

export interface CreatedEnvironmentResponse extends Environment {
  sdkKey: string;
  mobileKey: string;
  keys?: EnvironmentKey[];
}

export interface EnvironmentKeysResponse {
  clientSideId?: string;
  items: EnvironmentKey[];
}

export interface RotateEnvironmentKeyRequest {
  kind: EnvironmentKeyKind;
  gracePeriodSeconds?: number;
}

export interface RotateEnvironmentKeyResponse {
  key: EnvironmentKey;
  plaintextKey: string;
  previousKeysExpireAt?: IsoTimestamp | null;
}

export interface CreateVariationInput {
  id?: Uuid;
  value: JsonValue;
  name?: string;
  description?: string;
}

export interface CreateFlagRequest {
  key: string;
  name: string;
  description?: string;
  kind: FlagKind;
  variations: CreateVariationInput[];
  temporary?: boolean;
  clientSideAvailability?: ClientSideAvailability;
  tags?: string[];
  maintainerId?: Uuid | null;
  defaults?: {
    onVariationIndex: number;
    offVariationIndex: number;
  };
}

export interface FlagPatchRequest {
  environmentKey?: string;
  expectedVersion?: number;
  comment?: string | null;
  instructions: FlagPatchInstruction[];
}

export interface SimulateFlagRequest {
  context: Context;
  defaultValue?: JsonValue;
  draftInstructions?: FlagPatchInstruction[];
}

export interface SimulateFlagResponse<T extends JsonValue = JsonValue> extends EvaluationDetail<T> {
  flagVersion: number;
  usedDraft: boolean;
}

export interface FlagEnvRevision {
  id: Uuid;
  flagId: Uuid;
  envId: Uuid;
  version: number;
  on: boolean;
  config: Omit<
    FlagEnvConfig,
    'flagId' | 'envId' | 'environmentKey' | 'version' | 'on' | 'salt' | 'updatedAt'
  >;
  instructions: FlagPatchInstruction[];
  actorId: Uuid | null;
  comment: string | null;
  createdAt: IsoTimestamp;
}

export interface RestoreFlagRevisionRequest {
  expectedVersion?: number;
  comment?: string | null;
}

export interface AuditLogEntry {
  id: Uuid;
  orgId: Uuid;
  projectId: Uuid | null;
  envId: Uuid | null;
  projectKey?: string | null;
  envKey?: string | null;
  resourceType: string;
  resourceKey: string;
  action: string;
  actorId: Uuid | null;
  actorType: ActorType;
  actorEmail?: string | null;
  actorDisplayName?: string | null;
  ip?: string | null;
  comment: string | null;
  instructions: FlagPatchInstruction[];
  before: JsonValue | null;
  after: JsonValue | null;
  createdAt: IsoTimestamp;
}

export interface ApiToken {
  id: Uuid;
  orgId: Uuid;
  creatorMemberId: Uuid | null;
  name: string;
  role: BuiltInRole;
  tokenPrefix: string;
  lastUsedAt: IsoTimestamp | null;
  expiresAt: IsoTimestamp | null;
  revokedAt: IsoTimestamp | null;
  createdAt: IsoTimestamp;
}

export interface CreateApiTokenRequest {
  name: string;
  role: BuiltInRole;
  expiresAt?: IsoTimestamp | null;
}

export interface CreateApiTokenResponse {
  token: ApiToken;
  plaintextToken: string;
}

export interface CreateMemberRequest {
  email: string;
  displayName: string;
  role: BuiltInRole;
  password?: string;
}

export interface UpdateMemberRequest {
  displayName?: string;
  role?: BuiltInRole;
}
