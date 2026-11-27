import crypto from 'node:crypto';
import {
  CLAUSE_OPS,
  ROLLOUT_TOTAL_WEIGHT,
  type Clause,
  type ClauseOp,
  type Context,
  type ContextMeta,
  type FlagPatchInstruction,
  type FlagPatchRequest,
  type JsonValue,
  type MultiContext,
  type MultiContextKindEntry,
  type Rollout,
  type RolloutKind,
  type SingleContext,
  type Uuid,
  type WeightedVariation,
} from './types';

export const ALL_CLAUSE_OPS = CLAUSE_OPS;

const ONE_SHIFT_60 = 1n << 60n;
const TOTAL_WEIGHT_BIGINT = BigInt(ROLLOUT_TOTAL_WEIGHT);
const RESERVED_MULTI_KEYS = new Set(['kind', 'multi', '_meta']);

function isPlainRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

// ============================================================================
// RFC 6901 AttributeRef Parsing & Context Resolution
// ============================================================================

/**
 * Parses an attribute reference according to Skotos / RFC 6901 rules:
 * - Empty string -> invalid (null)
 * - No leading '/' -> literal top-level key (no ~0/~1 unescaping)
 * - Leading '/' -> RFC 6901 pointer: rejects '/', trailing '/', '//', and invalid '~' escapes;
 *   single-pass unescapes '~1' -> '/' and '~0' -> '~' ('~01' -> '~1').
 */
export function parseAttributeRef(raw: string): string[] | null {
  if (raw.length === 0) {
    return null;
  }
  if (!raw.startsWith('/')) {
    return [raw];
  }
  if (raw === '/' || raw.endsWith('/')) {
    return null;
  }

  const rawSegments = raw.slice(1).split('/');
  const components: string[] = [];

  for (const segment of rawSegments) {
    if (segment.length === 0) {
      return null;
    }
    let unescaped = '';
    for (let i = 0; i < segment.length; i++) {
      const ch = segment[i];
      if (ch === '~') {
        const next = segment[i + 1];
        if (next === '1') {
          unescaped += '/';
          i++;
        } else if (next === '0') {
          unescaped += '~';
          i++;
        } else {
          return null;
        }
      } else {
        unescaped += ch;
      }
    }
    components.push(unescaped);
  }

  return components;
}

/**
 * Resolves an attribute from a single-kind context or multi-context entry.
 * Always blocks '_meta' access (returns undefined).
 */
export function resolveContextAttribute(
  single: SingleContext | MultiContextKindEntry,
  ref: string,
  kind?: string,
): JsonValue | undefined {
  const components = parseAttributeRef(ref);
  if (!components || components.length === 0) {
    return undefined;
  }
  if (components[0] === '_meta') {
    return undefined;
  }

  if (components.length === 1) {
    const attr = components[0];
    if (attr === 'kind') {
      const resolvedKind =
        'kind' in single && typeof single.kind === 'string' ? single.kind : kind;
      return resolvedKind;
    }
    if (attr === 'anonymous') {
      return typeof single.anonymous === 'boolean' ? single.anonymous : false;
    }
    if (Object.prototype.hasOwnProperty.call(single, attr)) {
      return single[attr] as JsonValue | undefined;
    }
    return undefined;
  }

  let current: unknown = single;
  for (let i = 0; i < components.length; i++) {
    const key = components[i];
    if (!isPlainRecord(current) || !Object.prototype.hasOwnProperty.call(current, key)) {
      return undefined;
    }
    current = current[key];
  }

  return current as JsonValue | undefined;
}

// ============================================================================
// Exact SHA-1 60-Bit Integer Basis-Point Bucketing (0..=99_999)
// ============================================================================

export interface BucketHashInputParams {
  flagKey: string;
  salt: string;
  bucketValue: string;
  seed?: number | null;
  secondary?: string | null;
}

export interface Sha1BucketResult {
  input: string;
  sha1Hex: string;
  h: bigint;
  bucket: number;
}

export function buildBucketHashInput(params: BucketHashInputParams): string {
  if (params.seed !== undefined && params.seed !== null) {
    return `${params.seed}.${params.bucketValue}`;
  }
  if (typeof params.secondary === 'string' && params.secondary.length > 0) {
    return `${params.flagKey}.${params.salt}.${params.bucketValue}.${params.secondary}`;
  }
  return `${params.flagKey}.${params.salt}.${params.bucketValue}`;
}

export function computeBucketFromHashHex(sha1Hex: string): { h: bigint; bucket: number } {
  const normalized = sha1Hex.trim().toLowerCase();
  if (!/^[0-9a-f]{15,}$/.test(normalized)) {
    throw new Error(
      `Invalid hex hash for 60-bit bucketing: expected >= 15 hex chars, received "${sha1Hex}"`,
    );
  }
  const h = BigInt(`0x${normalized.slice(0, 15)}`);
  const bucket = Number((h * TOTAL_WEIGHT_BIGINT) / ONE_SHIFT_60);
  return { h, bucket };
}

export function computeSha1Bucket(params: BucketHashInputParams): Sha1BucketResult {
  const input = buildBucketHashInput(params);
  const sha1Hex = crypto.createHash('sha1').update(input, 'utf8').digest('hex');
  const { h, bucket } = computeBucketFromHashHex(sha1Hex);
  return { input, sha1Hex, h, bucket };
}

export function selectRolloutVariation(
  bucket: number,
  variations: readonly WeightedVariation[],
  usedFallback = false,
): { index: number; variation: WeightedVariation } {
  if (variations.length === 0) {
    throw new Error('Rollout variations list must not be empty');
  }

  if (usedFallback) {
    for (let index = 0; index < variations.length; index++) {
      if (variations[index].weight > 0) {
        return { index, variation: variations[index] };
      }
    }
    return { index: 0, variation: variations[0] };
  }

  let cumulative = 0;
  for (let index = 0; index < variations.length; index++) {
    cumulative += variations[index].weight;
    if (bucket < cumulative) {
      return { index, variation: variations[index] };
    }
  }

  const lastIndex = variations.length - 1;
  return { index: lastIndex, variation: variations[lastIndex] };
}

function extractBucketableValue(val: JsonValue | undefined): string | null {
  if (typeof val === 'string') {
    return val;
  }
  if (
    typeof val === 'number' &&
    Number.isFinite(val) &&
    Number.isInteger(val) &&
    Math.abs(val) <= Number.MAX_SAFE_INTEGER
  ) {
    return String(Object.is(val, -0) ? 0 : val);
  }
  return null;
}

function findSingleContextForKind(
  context: Context,
  targetKind: string,
): { single: SingleContext | MultiContextKindEntry; kind: string } | null {
  if (context.kind === 'multi') {
    if (RESERVED_MULTI_KEYS.has(targetKind)) {
      return null;
    }
    const candidate = (context as MultiContext)[targetKind];
    if (isPlainRecord(candidate) && typeof candidate.key === 'string') {
      return { single: candidate as MultiContextKindEntry, kind: targetKind };
    }
    return null;
  }
  if (context.kind === targetKind) {
    return { single: context as SingleContext, kind: context.kind };
  }
  return null;
}

export interface EvaluateRolloutParams {
  context: Context;
  rollout: Rollout;
  flagKey: string;
  salt: string;
}

export interface EvaluateRolloutResult {
  bucket: number;
  variationIndex: number;
  variationId: Uuid;
  inExperiment: boolean;
  usedFallback: boolean;
}

export function evaluateRolloutForContext(params: EvaluateRolloutParams): EvaluateRolloutResult {
  const { context, rollout, flagKey, salt } = params;
  const targetKind = rollout.contextKind ?? 'user';
  const bucketBy = rollout.bucketBy ?? 'key';
  const matched = findSingleContextForKind(context, targetKind);

  if (!matched) {
    const selected = selectRolloutVariation(0, rollout.variations, true);
    return {
      bucket: 0,
      variationIndex: selected.index,
      variationId: selected.variation.variationId,
      inExperiment: false,
      usedFallback: true,
    };
  }

  const rawAttr = resolveContextAttribute(matched.single, bucketBy, matched.kind);
  const bucketValue = extractBucketableValue(rawAttr);

  if (bucketValue === null) {
    const selected = selectRolloutVariation(0, rollout.variations, true);
    return {
      bucket: 0,
      variationIndex: selected.index,
      variationId: selected.variation.variationId,
      inExperiment: false,
      usedFallback: true,
    };
  }

  const secondary = matched.single._meta?.secondary ?? null;
  const { bucket } = computeSha1Bucket({
    flagKey,
    salt,
    bucketValue,
    seed: rollout.seed ?? null,
    secondary,
  });

  const selected = selectRolloutVariation(bucket, rollout.variations, false);
  const inExperiment = rollout.kind === 'experiment' && !selected.variation.untracked;

  return {
    bucket,
    variationIndex: selected.index,
    variationId: selected.variation.variationId,
    inExperiment,
    usedFallback: false,
  };
}

export function findContextKeyForBucketRange(params: {
  flagKey: string;
  salt: string;
  minBucket: number;
  maxBucket: number;
  prefix?: string;
  seed?: number | null;
  secondary?: string | null;
  maxAttempts?: number;
}): { key: string; bucket: number } {
  const {
    flagKey,
    salt,
    minBucket,
    maxBucket,
    prefix = 'e2e-bucket-key',
    seed = null,
    secondary = null,
    maxAttempts = 100_000,
  } = params;

  if (minBucket < 0 || maxBucket >= ROLLOUT_TOTAL_WEIGHT || minBucket > maxBucket) {
    throw new Error(
      `Invalid bucket range [${minBucket}, ${maxBucket}]: must satisfy 0 <= minBucket <= maxBucket <= ${ROLLOUT_TOTAL_WEIGHT - 1}`,
    );
  }

  for (let i = 0; i < maxAttempts; i++) {
    const key = `${prefix}-${i}`;
    const { bucket } = computeSha1Bucket({
      flagKey,
      salt,
      bucketValue: key,
      seed,
      secondary,
    });
    if (bucket >= minBucket && bucket <= maxBucket) {
      return { key, bucket };
    }
  }

  throw new Error(
    `Failed to find a context key in bucket range [${minBucket}, ${maxBucket}] within ${maxAttempts} attempts`,
  );
}

// ============================================================================
// Canonical Context, Clause & Rollout Builders
// ============================================================================

export interface SingleContextOptions {
  kind?: string;
  name?: string;
  anonymous?: boolean;
  privateAttributes?: string[];
  secondary?: string;
}

export function singleContext(
  key: string,
  attributes: Record<string, JsonValue> = {},
  options: SingleContextOptions = {},
): SingleContext {
  const ctx: SingleContext = {
    kind: options.kind ?? 'user',
    key,
  };
  if (options.name !== undefined) {
    ctx.name = options.name;
  }
  if (options.anonymous !== undefined) {
    ctx.anonymous = options.anonymous;
  }
  for (const [attrKey, attrVal] of Object.entries(attributes)) {
    ctx[attrKey] = attrVal;
  }

  const meta: ContextMeta = {};
  if (options.privateAttributes !== undefined) {
    meta.privateAttributes = [...options.privateAttributes];
  }
  if (options.secondary !== undefined) {
    meta.secondary = options.secondary;
  }
  if (Object.keys(meta).length > 0) {
    ctx._meta = meta;
  }

  return ctx;
}

export interface MultiContextEntryInput {
  key: string;
  name?: string;
  anonymous?: boolean;
  privateAttributes?: string[];
  secondary?: string;
  attributes?: Record<string, JsonValue>;
}

export function multiContext(entries: Record<string, MultiContextEntryInput>): MultiContext {
  const result: MultiContext = { kind: 'multi' };
  for (const [contextKind, spec] of Object.entries(entries)) {
    if (RESERVED_MULTI_KEYS.has(contextKind)) {
      throw new Error(`Invalid nested context kind "${contextKind}" inside multi-context`);
    }
    const entry: MultiContextKindEntry = {
      key: spec.key,
    };
    if (spec.name !== undefined) {
      entry.name = spec.name;
    }
    if (spec.anonymous !== undefined) {
      entry.anonymous = spec.anonymous;
    }
    if (spec.attributes) {
      for (const [attrKey, attrVal] of Object.entries(spec.attributes)) {
        if (attrKey === 'kind') continue;
        entry[attrKey] = attrVal;
      }
    }
    const meta: ContextMeta = {};
    if (spec.privateAttributes !== undefined) {
      meta.privateAttributes = [...spec.privateAttributes];
    }
    if (spec.secondary !== undefined) {
      meta.secondary = spec.secondary;
    }
    if (Object.keys(meta).length > 0) {
      entry._meta = meta;
    }
    result[contextKind] = entry;
  }
  return result;
}

export function buildClause(
  attribute: string,
  op: ClauseOp,
  values: JsonValue[],
  options: { contextKind?: string; negate?: boolean; id?: string } = {},
): Clause {
  const clause: Clause = {
    attribute,
    op,
    values: [...values],
  };
  if (options.id !== undefined) {
    clause.id = options.id;
  }
  if (options.contextKind !== undefined) {
    clause.contextKind = options.contextKind;
  }
  if (options.negate !== undefined) {
    clause.negate = options.negate;
  }
  return clause;
}

export function buildRollout(
  variations: WeightedVariation[],
  options: {
    kind?: RolloutKind;
    contextKind?: string;
    bucketBy?: string;
    seed?: number | null;
  } = {},
): Rollout {
  const rollout: Rollout = {
    variations: variations.map((v) => ({
      variationId: v.variationId,
      weight: v.weight,
      ...(v.untracked !== undefined ? { untracked: v.untracked } : {}),
    })),
  };
  if (options.kind !== undefined) {
    rollout.kind = options.kind;
  }
  if (options.contextKind !== undefined) {
    rollout.contextKind = options.contextKind;
  }
  if (options.bucketBy !== undefined) {
    rollout.bucketBy = options.bucketBy;
  }
  if (options.seed !== undefined) {
    rollout.seed = options.seed;
  }
  return rollout;
}

export function validateRolloutWeights(variations: readonly WeightedVariation[]): {
  valid: boolean;
  sum: number;
  delta: number;
} {
  let sum = 0;
  let allWeightsValid = variations.length > 0;
  for (const item of variations) {
    if (!Number.isInteger(item.weight) || item.weight < 0 || item.weight > ROLLOUT_TOTAL_WEIGHT) {
      allWeightsValid = false;
    }
    sum += item.weight;
  }
  const delta = sum - ROLLOUT_TOTAL_WEIGHT;
  return {
    valid: allWeightsValid && sum === ROLLOUT_TOTAL_WEIGHT,
    sum,
    delta,
  };
}

// ============================================================================
// All 23 Semantic FlagPatchInstruction Builders & Patch Request Envelope
// ============================================================================

export const patchInstructions = {
  // ---- Env-scoped (12) ----
  turnFlagOn(): Extract<FlagPatchInstruction, { kind: 'turnFlagOn' }> {
    return { kind: 'turnFlagOn' };
  },

  turnFlagOff(): Extract<FlagPatchInstruction, { kind: 'turnFlagOff' }> {
    return { kind: 'turnFlagOff' };
  },

  updateOffVariation(
    variationId: Uuid | null,
  ): Extract<FlagPatchInstruction, { kind: 'updateOffVariation' }> {
    return { kind: 'updateOffVariation', variationId };
  },

  addTargets(
    variationId: Uuid,
    values: string[],
    contextKind?: string,
  ): Extract<FlagPatchInstruction, { kind: 'addTargets' }> {
    return {
      kind: 'addTargets',
      variationId,
      values: [...values],
      ...(contextKind !== undefined ? { contextKind } : {}),
    };
  },

  removeTargets(
    variationId: Uuid,
    values: string[],
    contextKind?: string,
  ): Extract<FlagPatchInstruction, { kind: 'removeTargets' }> {
    return {
      kind: 'removeTargets',
      variationId,
      values: [...values],
      ...(contextKind !== undefined ? { contextKind } : {}),
    };
  },

  addRule(
    params: Omit<Extract<FlagPatchInstruction, { kind: 'addRule' }>, 'kind'>,
  ): Extract<FlagPatchInstruction, { kind: 'addRule' }> {
    const instruction: Extract<FlagPatchInstruction, { kind: 'addRule' }> = {
      kind: 'addRule',
      clauses: [...params.clauses],
    };
    if (params.ruleId !== undefined) instruction.ruleId = params.ruleId;
    if (params.description !== undefined) instruction.description = params.description;
    if (params.variationId !== undefined) instruction.variationId = params.variationId;
    if (params.rollout !== undefined) instruction.rollout = params.rollout;
    if (params.beforeRuleId !== undefined) instruction.beforeRuleId = params.beforeRuleId;
    if (params.trackEvents !== undefined) instruction.trackEvents = params.trackEvents;
    return instruction;
  },

  updateRule(
    params: Omit<Extract<FlagPatchInstruction, { kind: 'updateRule' }>, 'kind'>,
  ): Extract<FlagPatchInstruction, { kind: 'updateRule' }> {
    const instruction: Extract<FlagPatchInstruction, { kind: 'updateRule' }> = {
      kind: 'updateRule',
      ruleId: params.ruleId,
    };
    if (params.description !== undefined) instruction.description = params.description;
    if (params.clauses !== undefined) instruction.clauses = [...params.clauses];
    if (params.variationId !== undefined) instruction.variationId = params.variationId;
    if (params.rollout !== undefined) instruction.rollout = params.rollout;
    if (params.trackEvents !== undefined) instruction.trackEvents = params.trackEvents;
    return instruction;
  },

  removeRule(ruleId: string): Extract<FlagPatchInstruction, { kind: 'removeRule' }> {
    return { kind: 'removeRule', ruleId };
  },

  reorderRules(ruleIds: string[]): Extract<FlagPatchInstruction, { kind: 'reorderRules' }> {
    return { kind: 'reorderRules', ruleIds: [...ruleIds] };
  },

  updateFallthrough(
    target: { variationId: Uuid } | { rollout: Rollout },
  ): Extract<FlagPatchInstruction, { kind: 'updateFallthrough' }> {
    if ('rollout' in target) {
      return { kind: 'updateFallthrough', rollout: target.rollout };
    }
    return { kind: 'updateFallthrough', variationId: target.variationId };
  },

  addPrerequisite(
    key: string,
    variationId: Uuid,
  ): Extract<FlagPatchInstruction, { kind: 'addPrerequisite' }> {
    return { kind: 'addPrerequisite', key, variationId };
  },

  removePrerequisite(key: string): Extract<FlagPatchInstruction, { kind: 'removePrerequisite' }> {
    return { kind: 'removePrerequisite', key };
  },

  // ---- Flag-scoped core (6) ----
  addVariation(
    value: JsonValue,
    options: { variationId?: Uuid; name?: string; description?: string } = {},
  ): Extract<FlagPatchInstruction, { kind: 'addVariation' }> {
    const instruction: Extract<FlagPatchInstruction, { kind: 'addVariation' }> = {
      kind: 'addVariation',
      value,
    };
    if (options.variationId !== undefined) instruction.variationId = options.variationId;
    if (options.name !== undefined) instruction.name = options.name;
    if (options.description !== undefined) instruction.description = options.description;
    return instruction;
  },

  updateVariation(
    variationId: Uuid,
    updates: { value?: JsonValue; name?: string; description?: string },
  ): Extract<FlagPatchInstruction, { kind: 'updateVariation' }> {
    const instruction: Extract<FlagPatchInstruction, { kind: 'updateVariation' }> = {
      kind: 'updateVariation',
      variationId,
    };
    if (updates.value !== undefined) instruction.value = updates.value;
    if (updates.name !== undefined) instruction.name = updates.name;
    if (updates.description !== undefined) instruction.description = updates.description;
    return instruction;
  },

  removeVariation(variationId: Uuid): Extract<FlagPatchInstruction, { kind: 'removeVariation' }> {
    return { kind: 'removeVariation', variationId };
  },

  updateClientSideAvailability(
    usingEnvironmentId: boolean,
    usingMobileKey: boolean,
  ): Extract<FlagPatchInstruction, { kind: 'updateClientSideAvailability' }> {
    return {
      kind: 'updateClientSideAvailability',
      usingEnvironmentId,
      usingMobileKey,
    };
  },

  archiveFlag(): Extract<FlagPatchInstruction, { kind: 'archiveFlag' }> {
    return { kind: 'archiveFlag' };
  },

  restoreFlag(): Extract<FlagPatchInstruction, { kind: 'restoreFlag' }> {
    return { kind: 'restoreFlag' };
  },

  // ---- Flag-scoped metadata (5) ----
  updateName(name: string): Extract<FlagPatchInstruction, { kind: 'updateName' }> {
    return { kind: 'updateName', name };
  },

  updateDescription(
    description: string,
  ): Extract<FlagPatchInstruction, { kind: 'updateDescription' }> {
    return { kind: 'updateDescription', description };
  },

  updateTags(tags: string[]): Extract<FlagPatchInstruction, { kind: 'updateTags' }> {
    return { kind: 'updateTags', tags: [...tags] };
  },

  updateMaintainer(
    maintainerId: Uuid | null,
  ): Extract<FlagPatchInstruction, { kind: 'updateMaintainer' }> {
    return { kind: 'updateMaintainer', maintainerId };
  },

  updateTemporary(temporary: boolean): Extract<FlagPatchInstruction, { kind: 'updateTemporary' }> {
    return { kind: 'updateTemporary', temporary };
  },
} as const;

export function buildFlagPatchRequest(params: {
  instructions: FlagPatchInstruction[];
  environmentKey?: string;
  expectedVersion?: number;
  comment?: string | null;
}): FlagPatchRequest {
  const payload: FlagPatchRequest = {
    instructions: [...params.instructions],
  };
  if (params.environmentKey !== undefined) {
    payload.environmentKey = params.environmentKey;
  }
  if (params.expectedVersion !== undefined) {
    payload.expectedVersion = params.expectedVersion;
  }
  if (params.comment !== undefined) {
    payload.comment = params.comment;
  }
  return payload;
}
