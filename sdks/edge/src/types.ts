export interface ContextMeta {
  privateAttributes?: string[];
  secondary?: string;
  redactedAttributes?: string[];
}

export interface SingleContext {
  kind?: string;
  key: string;
  name?: string;
  anonymous?: boolean;
  _meta?: ContextMeta;
  [attr: string]: unknown;
}

export interface MultiContext {
  kind: "multi";
  [contextKind: string]: string | SingleContext | undefined;
}

export type SkotosContext = SingleContext | MultiContext;

export type ClauseOp =
  | "in"
  | "startsWith"
  | "endsWith"
  | "contains"
  | "matches"
  | "lessThan"
  | "lessThanOrEqual"
  | "greaterThan"
  | "greaterThanOrEqual"
  | "before"
  | "after"
  | "semverEqual"
  | "semverLessThan"
  | "semverGreaterThan"
  | "segmentMatch";

export interface Clause {
  contextKind?: string;
  attribute: string;
  op: ClauseOp;
  values: unknown[];
  negate?: boolean;
}

export interface EvalWeightedVariation {
  variation: number;
  weight: number;
  untracked?: boolean;
}

export interface EvalRollout {
  contextKind?: string;
  bucketBy?: string | null;
  seed?: number | null;
  kind?: "rollout" | "experiment";
  variations: EvalWeightedVariation[];
}

export interface EvalVariationOrRollout {
  variation?: number | null;
  rollout?: EvalRollout | null;
}

export interface EvalPrerequisite {
  key: string;
  variation: number;
}

export interface EvalTarget {
  contextKind?: string;
  values: string[];
  variation: number;
}

export interface EvalRule extends EvalVariationOrRollout {
  id: string;
  clauses: Clause[];
}

export interface EvalFlag {
  key: string;
  version: number;
  on: boolean;
  salt: string;
  variations: unknown[];
  offVariation?: number | null;
  prerequisites?: EvalPrerequisite[];
  targets?: EvalTarget[];
  rules?: EvalRule[];
  fallthrough: EvalVariationOrRollout;
  clientSideAvailability?: {
    usingEnvironmentId?: boolean;
    usingMobileKey?: boolean;
  };
  trackEvents?: boolean;
}

export interface SegmentTarget {
  contextKind?: string;
  values: string[];
}

export interface SegmentRule {
  id?: string;
  clauses: Clause[];
  weight?: number | null;
  bucketBy?: string | null;
  rolloutContextKind?: string | null;
}

export interface EvalSegment {
  key: string;
  version?: number;
  salt?: string;
  included?: string[];
  excluded?: string[];
  includedContexts?: SegmentTarget[];
  excludedContexts?: SegmentTarget[];
  rules?: SegmentRule[];
  unbounded?: boolean;
  unboundedContextKind?: string;
  generation?: number;
}

export interface BigSegmentSnapshot {
  envId?: string;
  segmentKey: string;
  contextKind: string;
  generation: number;
  /** Sorted FNV-1a 64-bit hashes (encoded as numbers, bigints, or decimal strings). */
  includedHashes: Array<bigint | number | string>;
  /** Sorted FNV-1a 64-bit hashes (encoded as numbers, bigints, or decimal strings). */
  excludedHashes: Array<bigint | number | string>;
}

export interface EnvironmentSnapshot {
  environmentId?: string;
  revision?: number;
  generatedAt?: string;
  flags: Record<string, EvalFlag>;
  segments?: Record<string, EvalSegment>;
  bigSegments?: Record<string, BigSegmentSnapshot>;
}

export type EvaluationErrorKind =
  | "CLIENT_NOT_READY"
  | "FLAG_NOT_FOUND"
  | "MALFORMED_FLAG"
  | "USER_NOT_SPECIFIED"
  | "WRONG_TYPE"
  | "EXCEPTION";

export type EvaluationReason =
  | { kind: "OFF" }
  | { kind: "TARGET_MATCH" }
  | { kind: "FALLTHROUGH"; inExperiment?: boolean }
  | { kind: "RULE_MATCH"; ruleIndex: number; ruleId: string; inExperiment?: boolean }
  | { kind: "PREREQUISITE_FAILED"; prerequisiteKey: string }
  | { kind: "ERROR"; errorKind: EvaluationErrorKind };

export interface EvaluationDetail<T = unknown> {
  value: T;
  variationIndex?: number;
  reason: EvaluationReason;
}

export interface RedactionConfig {
  allAttributesPrivate?: boolean;
  globalPrivateAttributes?: string[];
}

export interface EvalStore {
  getFlag(key: string): EvalFlag | undefined;
  getSegment(key: string): EvalSegment | undefined;
  /**
   * Optional Big Segment membership lookup (`true` = included, `false` = excluded,
   * `undefined` = not present in Big Segment hash blocks).
   */
  bigSegmentMembership?(
    segmentKey: string,
    contextKind: string,
    contextKey: string,
  ): boolean | undefined;
}
