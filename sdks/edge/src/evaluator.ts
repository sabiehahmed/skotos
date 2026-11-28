import { bigSegmentKeyHash, sha1Hex } from "./sha1";
import type {
  BigSegmentSnapshot,
  Clause,
  ClauseOp,
  EvalFlag,
  EvalRollout,
  EvalSegment,
  EvalStore,
  EvalVariationOrRollout,
  EvaluationDetail,
  EvaluationErrorKind,
  EvaluationReason,
  RedactionConfig,
  SingleContext,
  SkotosContext,
} from "./types";

const BUCKET_SCALE = 100_000n;
const TWO_POW_60 = 1n << 60n;
const VALID_KIND_RE = /^[a-zA-Z0-9._-]+$/;

/**
 * Exact SHA-1 60-bit integer basis-point bucketing (`0..=99_999`), matching `crates/skotos-eval`.
 */
export function hashToBucket(hashInput: string): number {
  const hex = sha1Hex(hashInput);
  const top60 = BigInt(`0x${hex.slice(0, 15)}`);
  return Number((top60 * BUCKET_SCALE) / TWO_POW_60);
}

/**
 * O(log N) binary search membership check against a sorted 64-bit FNV-1a Big Segment snapshot.
 */
export function checkBigSegmentSnapshot(
  snapshot: BigSegmentSnapshot,
  contextKind: string,
  contextKey: string,
): boolean | undefined {
  if (snapshot.contextKind !== contextKind) return undefined;
  const target = bigSegmentKeyHash(contextKind, contextKey);
  if (binarySearchBigInt(snapshot.includedHashes, target)) return true;
  if (binarySearchBigInt(snapshot.excludedHashes, target)) return false;
  return undefined;
}

function binarySearchBigInt(
  sorted: ReadonlyArray<bigint | number | string>,
  target: bigint,
): boolean {
  let lo = 0;
  let hi = sorted.length - 1;
  while (lo <= hi) {
    const mid = (lo + hi) >>> 1;
    const raw = sorted[mid];
    if (raw === undefined) return false;
    const cur = typeof raw === "bigint" ? raw : BigInt(raw);
    if (cur === target) return true;
    if (cur < target) lo = mid + 1;
    else hi = mid - 1;
  }
  return false;
}

export function parseAttributeRef(raw: string): string[] | null {
  if (!raw) return null;
  if (!raw.startsWith("/")) return [raw];
  if (raw === "/" || raw.endsWith("/")) return null;
  const parts = raw.slice(1).split("/");
  const out: string[] = [];
  for (const part of parts) {
    if (!part) return null;
    let unescaped = "";
    for (let i = 0; i < part.length; i++) {
      if (part[i] === "~") {
        if (i + 1 >= part.length) return null;
        const next = part[i + 1];
        if (next === "0") unescaped += "~";
        else if (next === "1") unescaped += "/";
        else return null;
        i++;
      } else {
        unescaped += part[i];
      }
    }
    out.push(unescaped);
  }
  return out;
}

function isValidKind(kind: unknown): kind is string {
  return (
    typeof kind === "string" &&
    kind.length > 0 &&
    kind !== "kind" &&
    kind !== "multi" &&
    VALID_KIND_RE.test(kind)
  );
}

export function validateContext(ctx: SkotosContext | null | undefined): boolean {
  if (!ctx || typeof ctx !== "object") return false;
  if (ctx.kind === "multi") {
    const entries = Object.entries(ctx).filter(([k]) => k !== "kind");
    if (entries.length === 0) return false;
    for (const [k, v] of entries) {
      if (!isValidKind(k)) return false;
      if (!v || typeof v !== "object") return false;
      const single = v as SingleContext;
      if (typeof single.key !== "string" || single.key.length === 0) return false;
      if (single.kind !== undefined && single.kind !== k) return false;
    }
    return true;
  }
  const single = ctx as SingleContext;
  const kind = single.kind ?? "user";
  return isValidKind(kind) && typeof single.key === "string" && single.key.length > 0;
}

export function getContextKind(ctx: SkotosContext, kind: string): SingleContext | undefined {
  if (ctx.kind === "multi") {
    const nested = (ctx as Record<string, unknown>)[kind];
    if (nested && typeof nested === "object") {
      return { kind, ...(nested as SingleContext) };
    }
    return undefined;
  }
  const single = ctx as SingleContext;
  const resolvedKind = single.kind ?? "user";
  return resolvedKind === kind ? { ...single, kind: resolvedKind } : undefined;
}

export function getContextKinds(ctx: SkotosContext): Array<[string, SingleContext]> {
  if (ctx.kind === "multi") {
    const out: Array<[string, SingleContext]> = [];
    for (const [k, v] of Object.entries(ctx)) {
      if (k === "kind" || !v || typeof v !== "object") continue;
      out.push([k, { kind: k, ...(v as SingleContext) }]);
    }
    return out;
  }
  const single = ctx as SingleContext;
  const kind = single.kind ?? "user";
  return [[kind, { ...single, kind }]];
}

function getAttribute(single: SingleContext, components: string[]): unknown {
  const first = components[0];
  if (first === undefined) return undefined;
  if (first === "_meta") return undefined;
  if (first === "kind") return components.length === 1 ? (single.kind ?? "user") : undefined;
  if (first === "key") return components.length === 1 ? single.key : undefined;
  if (first === "anonymous") {
    return components.length === 1 ? Boolean(single.anonymous) : undefined;
  }

  let cur: unknown = (single as Record<string, unknown>)[first];
  if (cur === undefined) return undefined;
  for (let i = 1; i < components.length; i++) {
    const seg = components[i];
    if (seg === undefined || cur === null || typeof cur !== "object" || Array.isArray(cur)) {
      return undefined;
    }
    cur = (cur as Record<string, unknown>)[seg];
    if (cur === undefined) return undefined;
  }
  return cur;
}

function buildHashInput(
  key: string,
  salt: string,
  bucketValue: string,
  secondary?: string,
  seed?: number | null,
): string {
  if (typeof seed === "number") {
    return `${seed}.${bucketValue}`;
  }
  if (secondary) {
    return `${key}.${salt}.${bucketValue}.${secondary}`;
  }
  return `${key}.${salt}.${bucketValue}`;
}

function extractBucketValue(
  ctx: SkotosContext,
  contextKind: string,
  bucketBy?: string | null,
): { value: string; secondary?: string } | null {
  const single = getContextKind(ctx, contextKind || "user");
  if (!single) return null;
  const parts = parseAttributeRef(bucketBy || "key");
  if (!parts) return null;
  const val = getAttribute(single, parts);
  let bucketStr: string;
  if (typeof val === "string") {
    bucketStr = val;
  } else if (typeof val === "number" && Number.isInteger(val)) {
    bucketStr = String(val);
  } else {
    return null;
  }
  const sec = single._meta?.secondary || undefined;
  return { value: bucketStr, secondary: sec };
}

function evaluateRollout(
  flagKey: string,
  salt: string,
  rollout: EvalRollout,
  ctx: SkotosContext,
): { variationIndex: number; inExperiment: boolean } | null {
  if (!rollout.variations || rollout.variations.length === 0) return null;
  const isExperiment = rollout.kind === "experiment";
  const effectiveBucketBy = isExperiment ? undefined : rollout.bucketBy;
  const extracted = extractBucketValue(ctx, rollout.contextKind ?? "user", effectiveBucketBy);

  let bucket = 0;
  let validContext = false;
  if (extracted) {
    const input = buildHashInput(
      flagKey,
      salt,
      extracted.value,
      isExperiment ? undefined : extracted.secondary,
      rollout.seed,
    );
    bucket = hashToBucket(input);
    validContext = true;
  }

  let cumulative = 0;
  for (const wv of rollout.variations) {
    cumulative += wv.weight;
    if (bucket < cumulative) {
      return {
        variationIndex: wv.variation,
        inExperiment: isExperiment && validContext && !wv.untracked,
      };
    }
  }
  const fallback =
    cumulative === 0 ? rollout.variations[0] : rollout.variations[rollout.variations.length - 1];
  if (!fallback) return null;
  return {
    variationIndex: fallback.variation,
    inExperiment: isExperiment && validContext && !fallback.untracked,
  };
}

function deepJsonEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (typeof a === "number" && typeof b === "number") return a === b;
  if (a === null || b === null || typeof a !== "object" || typeof b !== "object") return false;
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) return false;
    return a.every((item, idx) => deepJsonEqual(item, b[idx]));
  }
  const aObj = a as Record<string, unknown>;
  const bObj = b as Record<string, unknown>;
  const aKeys = Object.keys(aObj);
  const bKeys = Object.keys(bObj);
  if (aKeys.length !== bKeys.length) return false;
  return aKeys.every(
    (k) => Object.prototype.hasOwnProperty.call(bObj, k) && deepJsonEqual(aObj[k], bObj[k]),
  );
}

const RFC3339_RE =
  /^(\d{4})-(\d{2})-(\d{2})[Tt](\d{2}):(\d{2}):(\d{2})(?:\.(\d+))?(?:[Zz]|([+-])(\d{2}):(\d{2}))$/;

function parseTimestampNanos(v: unknown): bigint | null {
  if (typeof v === "string") {
    const m = RFC3339_RE.exec(v);
    if (!m) return null;
    const ms = Date.parse(v);
    if (!Number.isFinite(ms)) return null;
    let subNanos = 0n;
    if (m[7]) {
      const padded = m[7].slice(0, 9).padEnd(9, "0");
      subNanos = BigInt(padded) % 1_000_000n;
    }
    return BigInt(Math.trunc(ms)) * 1_000_000n + subNanos;
  }
  if (typeof v === "number" && Number.isFinite(v)) {
    return BigInt(Math.trunc(v * 1_000_000));
  }
  return null;
}

interface ParsedSemVer {
  major: bigint;
  minor: bigint;
  patch: bigint;
  prerelease: string[];
}

function validSemVerIdents(parts: string[], noLeadingZeroNumeric: boolean): boolean {
  for (const p of parts) {
    if (!p || !/^[0-9A-Za-z-]+$/.test(p)) return false;
    if (noLeadingZeroNumeric && /^[0-9]+$/.test(p) && p.length > 1 && p.startsWith("0")) {
      return false;
    }
  }
  return true;
}

function parseSemVerRelaxed(input: string): ParsedSemVer | null {
  if (!input || input.startsWith("v") || input.startsWith("V")) return null;
  let rest = input;
  const plusIdx = rest.indexOf("+");
  if (plusIdx >= 0) {
    const build = rest.slice(plusIdx + 1);
    if (!build || !validSemVerIdents(build.split("."), false)) return null;
    rest = rest.slice(0, plusIdx);
  }
  let prerelease: string[] = [];
  const dashIdx = rest.indexOf("-");
  if (dashIdx >= 0) {
    const preStr = rest.slice(dashIdx + 1);
    if (!preStr) return null;
    prerelease = preStr.split(".");
    if (!validSemVerIdents(prerelease, true)) return null;
    rest = rest.slice(0, dashIdx);
  }
  const core = rest.split(".");
  if (core.length < 1 || core.length > 3) return null;
  const nums: [bigint, bigint, bigint] = [0n, 0n, 0n];
  for (let i = 0; i < core.length; i++) {
    const part = core[i];
    if (part === undefined || !/^[0-9]+$/.test(part) || (part.length > 1 && part.startsWith("0"))) {
      return null;
    }
    nums[i] = BigInt(part);
  }
  return { major: nums[0], minor: nums[1], patch: nums[2], prerelease };
}

function compareSemVer(a: unknown, b: unknown): number | null {
  if (typeof a !== "string" || typeof b !== "string") return null;
  const va = parseSemVerRelaxed(a);
  const vb = parseSemVerRelaxed(b);
  if (!va || !vb) return null;
  if (va.major !== vb.major) return va.major < vb.major ? -1 : 1;
  if (va.minor !== vb.minor) return va.minor < vb.minor ? -1 : 1;
  if (va.patch !== vb.patch) return va.patch < vb.patch ? -1 : 1;
  if (va.prerelease.length === 0 && vb.prerelease.length === 0) return 0;
  if (va.prerelease.length === 0) return 1;
  if (vb.prerelease.length === 0) return -1;
  const minLen = Math.min(va.prerelease.length, vb.prerelease.length);
  for (let i = 0; i < minLen; i++) {
    const pa = va.prerelease[i];
    const pb = vb.prerelease[i];
    if (pa === undefined || pb === undefined || pa === pb) continue;
    const aNum = /^[0-9]+$/.test(pa);
    const bNum = /^[0-9]+$/.test(pb);
    if (aNum && bNum) return BigInt(pa) < BigInt(pb) ? -1 : 1;
    if (aNum) return -1;
    if (bNum) return 1;
    return pa < pb ? -1 : 1;
  }
  if (va.prerelease.length === vb.prerelease.length) return 0;
  return va.prerelease.length < vb.prerelease.length ? -1 : 1;
}

function matchSingleValue(op: ClauseOp, actual: unknown, target: unknown): boolean {
  switch (op) {
    case "in":
      return deepJsonEqual(actual, target);
    case "startsWith":
      return typeof actual === "string" && typeof target === "string" && actual.startsWith(target);
    case "endsWith":
      return typeof actual === "string" && typeof target === "string" && actual.endsWith(target);
    case "contains":
      return typeof actual === "string" && typeof target === "string" && actual.includes(target);
    case "matches":
      if (typeof actual !== "string" || typeof target !== "string") return false;
      try {
        return new RegExp(target).test(actual);
      } catch {
        return false;
      }
    case "lessThan":
      return typeof actual === "number" && typeof target === "number" && actual < target;
    case "lessThanOrEqual":
      return typeof actual === "number" && typeof target === "number" && actual <= target;
    case "greaterThan":
      return typeof actual === "number" && typeof target === "number" && actual > target;
    case "greaterThanOrEqual":
      return typeof actual === "number" && typeof target === "number" && actual >= target;
    case "before": {
      const ta = parseTimestampNanos(actual);
      const tb = parseTimestampNanos(target);
      return ta !== null && tb !== null && ta < tb;
    }
    case "after": {
      const ta = parseTimestampNanos(actual);
      const tb = parseTimestampNanos(target);
      return ta !== null && tb !== null && ta > tb;
    }
    case "semverEqual":
      return compareSemVer(actual, target) === 0;
    case "semverLessThan":
      return compareSemVer(actual, target) === -1;
    case "semverGreaterThan":
      return compareSemVer(actual, target) === 1;
    default:
      return false;
  }
}

class MalformedFlagError extends Error {}

function matchesKeyList(ctx: SkotosContext, kind: string, keys?: string[]): boolean {
  if (!keys || keys.length === 0) return false;
  const sc = getContextKind(ctx, kind || "user");
  if (!sc) return false;
  return keys.includes(sc.key);
}

function evaluateSegment(
  store: EvalStore,
  segment: EvalSegment,
  ctx: SkotosContext,
  segStack: string[],
): boolean {
  if (segStack.includes(segment.key)) {
    throw new MalformedFlagError("segment cycle detected");
  }

  // 0. Unbounded Big Segment hash-block lookup (takes precedence over dynamic rules)
  if (segment.unbounded && store.bigSegmentMembership) {
    const kind = segment.unboundedContextKind ?? "user";
    const sc = getContextKind(ctx, kind);
    if (sc) {
      const decision = store.bigSegmentMembership(segment.key, kind, sc.key);
      if (decision !== undefined) return decision;
    }
  }

  if (matchesKeyList(ctx, "user", segment.included)) return true;
  for (const t of segment.includedContexts ?? []) {
    if (matchesKeyList(ctx, t.contextKind ?? "user", t.values)) return true;
  }
  if (matchesKeyList(ctx, "user", segment.excluded)) return false;
  for (const t of segment.excludedContexts ?? []) {
    if (matchesKeyList(ctx, t.contextKind ?? "user", t.values)) return false;
  }

  segStack.push(segment.key);
  try {
    for (const rule of segment.rules ?? []) {
      const allMatch = rule.clauses.every((c) => evalClause(store, c, ctx, segStack));
      if (!allMatch) continue;
      if (typeof rule.weight === "number") {
        if (rule.weight === 0) continue;
        if (rule.weight >= 100_000) return true;
        const extracted = extractBucketValue(
          ctx,
          rule.rolloutContextKind ?? "user",
          rule.bucketBy,
        );
        if (extracted) {
          const input = buildHashInput(
            segment.key,
            segment.salt ?? "",
            extracted.value,
            extracted.secondary,
            null,
          );
          if (hashToBucket(input) < rule.weight) return true;
        }
      } else {
        return true;
      }
    }
    return false;
  } finally {
    segStack.pop();
  }
}

function evalClause(
  store: EvalStore,
  clause: Clause,
  ctx: SkotosContext,
  segStack: string[],
): boolean {
  if (clause.op === "segmentMatch") {
    let matched = false;
    for (const val of clause.values) {
      if (typeof val !== "string") continue;
      const seg = store.getSegment(val);
      if (seg && evaluateSegment(store, seg, ctx, segStack)) {
        matched = true;
        break;
      }
    }
    return clause.negate ? !matched : matched;
  }

  const single = getContextKind(ctx, clause.contextKind ?? "user");
  if (!single) return false;
  const parts = parseAttributeRef(clause.attribute);
  if (!parts) return false;
  const attrVal = getAttribute(single, parts);
  if (attrVal === undefined || attrVal === null) return false;

  let rawMatch: boolean;
  if (Array.isArray(attrVal)) {
    if (attrVal.length === 0) return false;
    rawMatch = attrVal.some(
      (item) =>
        item !== null &&
        item !== undefined &&
        clause.values.some((cv) => matchSingleValue(clause.op, item, cv)),
    );
  } else {
    rawMatch = clause.values.some((cv) => matchSingleValue(clause.op, attrVal, cv));
  }
  return clause.negate ? !rawMatch : rawMatch;
}

function errorDetail<T>(defaultValue: T, errorKind: EvaluationErrorKind): EvaluationDetail<T> {
  return {
    value: defaultValue,
    reason: { kind: "ERROR", errorKind },
  };
}

function resolveVariation(
  flag: EvalFlag,
  index: number,
  defaultValue: unknown,
  reason: EvaluationReason,
): EvaluationDetail {
  if (index < 0 || index >= flag.variations.length) {
    return errorDetail(defaultValue, "MALFORMED_FLAG");
  }
  return {
    value: flag.variations[index],
    variationIndex: index,
    reason,
  };
}

function resolveOffVariation(
  flag: EvalFlag,
  defaultValue: unknown,
  reason: EvaluationReason,
): EvaluationDetail {
  if (flag.offVariation === undefined || flag.offVariation === null) {
    return { value: defaultValue, reason };
  }
  return resolveVariation(flag, flag.offVariation, defaultValue, reason);
}

function resolveVariationOrRollout(
  flag: EvalFlag,
  vor: EvalVariationOrRollout,
  ctx: SkotosContext,
  defaultValue: unknown,
  buildReason: (inExperiment: boolean) => EvaluationReason,
): EvaluationDetail {
  if (typeof vor.variation === "number") {
    return resolveVariation(flag, vor.variation, defaultValue, buildReason(false));
  }
  if (vor.rollout) {
    const res = evaluateRollout(flag.key, flag.salt, vor.rollout, ctx);
    if (!res) return errorDetail(defaultValue, "MALFORMED_FLAG");
    return resolveVariation(flag, res.variationIndex, defaultValue, buildReason(res.inExperiment));
  }
  return errorDetail(defaultValue, "MALFORMED_FLAG");
}

function evaluateFlagInternal(
  store: EvalStore,
  flag: EvalFlag,
  ctx: SkotosContext,
  defaultValue: unknown,
  prereqStack: string[],
): EvaluationDetail {
  if (!flag.on) {
    return resolveOffVariation(flag, defaultValue, { kind: "OFF" });
  }
  if (prereqStack.includes(flag.key)) {
    return errorDetail(defaultValue, "MALFORMED_FLAG");
  }
  prereqStack.push(flag.key);
  try {
    for (const pre of flag.prerequisites ?? []) {
      if (prereqStack.includes(pre.key)) {
        return errorDetail(defaultValue, "MALFORMED_FLAG");
      }
      const preFlag = store.getFlag(pre.key);
      if (!preFlag || !preFlag.on) {
        return resolveOffVariation(flag, defaultValue, {
          kind: "PREREQUISITE_FAILED",
          prerequisiteKey: pre.key,
        });
      }
      const preDetail = evaluateFlagInternal(store, preFlag, ctx, null, prereqStack);
      if (
        preDetail.reason.kind === "ERROR" &&
        preDetail.reason.errorKind === "MALFORMED_FLAG"
      ) {
        return errorDetail(defaultValue, "MALFORMED_FLAG");
      }
      if (preDetail.variationIndex !== pre.variation) {
        return resolveOffVariation(flag, defaultValue, {
          kind: "PREREQUISITE_FAILED",
          prerequisiteKey: pre.key,
        });
      }
    }

    for (const target of flag.targets ?? []) {
      const sc = getContextKind(ctx, target.contextKind ?? "user");
      if (sc && target.values.includes(sc.key)) {
        return resolveVariation(flag, target.variation, defaultValue, { kind: "TARGET_MATCH" });
      }
    }

    const rules = flag.rules ?? [];
    for (let i = 0; i < rules.length; i++) {
      const rule = rules[i];
      if (!rule) continue;
      try {
        const segStack: string[] = [];
        const matched = rule.clauses.every((c) => evalClause(store, c, ctx, segStack));
        if (matched) {
          return resolveVariationOrRollout(flag, rule, ctx, defaultValue, (inExp) =>
            inExp
              ? { kind: "RULE_MATCH", ruleIndex: i, ruleId: rule.id, inExperiment: true }
              : { kind: "RULE_MATCH", ruleIndex: i, ruleId: rule.id },
          );
        }
      } catch (err) {
        if (err instanceof MalformedFlagError) {
          return errorDetail(defaultValue, "MALFORMED_FLAG");
        }
        throw err;
      }
    }

    return resolveVariationOrRollout(flag, flag.fallthrough, ctx, defaultValue, (inExp) =>
      inExp ? { kind: "FALLTHROUGH", inExperiment: true } : { kind: "FALLTHROUGH" },
    );
  } finally {
    prereqStack.pop();
  }
}

export function evaluate(
  store: EvalStore,
  flagKey: string,
  ctx: SkotosContext,
  defaultValue: unknown,
): EvaluationDetail {
  if (!validateContext(ctx)) {
    return errorDetail(defaultValue, "USER_NOT_SPECIFIED");
  }
  const flag = store.getFlag(flagKey);
  if (!flag) {
    return errorDetail(defaultValue, "FLAG_NOT_FOUND");
  }
  return evaluateFlagInternal(store, flag, ctx, defaultValue, []);
}

export function boolVariation(
  store: EvalStore,
  flagKey: string,
  ctx: SkotosContext,
  defaultValue: boolean,
): EvaluationDetail<boolean> {
  const d = evaluate(store, flagKey, ctx, defaultValue);
  if (d.variationIndex !== undefined && typeof d.value !== "boolean") {
    return errorDetail(defaultValue, "WRONG_TYPE");
  }
  return d as EvaluationDetail<boolean>;
}

export function stringVariation(
  store: EvalStore,
  flagKey: string,
  ctx: SkotosContext,
  defaultValue: string,
): EvaluationDetail<string> {
  const d = evaluate(store, flagKey, ctx, defaultValue);
  if (d.variationIndex !== undefined && typeof d.value !== "string") {
    return errorDetail(defaultValue, "WRONG_TYPE");
  }
  return d as EvaluationDetail<string>;
}

export function numberVariation(
  store: EvalStore,
  flagKey: string,
  ctx: SkotosContext,
  defaultValue: number,
): EvaluationDetail<number> {
  const d = evaluate(store, flagKey, ctx, defaultValue);
  if (d.variationIndex !== undefined && typeof d.value !== "number") {
    return errorDetail(defaultValue, "WRONG_TYPE");
  }
  return d as EvaluationDetail<number>;
}

function removeNestedPath(obj: Record<string, unknown>, parts: string[]): boolean {
  const head = parts[0];
  if (head === undefined) return false;
  if (parts.length === 1) {
    if (Object.prototype.hasOwnProperty.call(obj, head)) {
      delete obj[head];
      return true;
    }
    return false;
  }
  const child = obj[head];
  if (!child || typeof child !== "object" || Array.isArray(child)) return false;
  return removeNestedPath(child as Record<string, unknown>, parts.slice(1));
}

function redactSingle(
  single: SingleContext,
  cfg: RedactionConfig,
  includeKind: boolean,
): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  if (includeKind) out.kind = single.kind ?? "user";
  out.key = single.key;
  if (single.anonymous) out.anonymous = true;

  const redacted = new Set<string>();
  if (cfg.allAttributesPrivate) {
    for (const [k, v] of Object.entries(single)) {
      if (k === "kind" || k === "key" || k === "anonymous" || k === "_meta") continue;
      if (v !== undefined) redacted.add(k);
    }
  } else {
    for (const [k, v] of Object.entries(single)) {
      if (k === "kind" || k === "key" || k === "anonymous" || k === "_meta") continue;
      if (v !== undefined) out[k] = structuredClone(v);
    }
    const refs = [
      ...(cfg.globalPrivateAttributes ?? []),
      ...(single._meta?.privateAttributes ?? []),
    ];
    for (const rawRef of refs) {
      const parts = parseAttributeRef(rawRef);
      const first = parts?.[0];
      if (!parts || first === undefined) continue;
      if (first === "kind" || first === "key" || first === "anonymous" || first === "_meta") {
        continue;
      }
      if (removeNestedPath(out, parts)) {
        redacted.add(rawRef.startsWith("/") ? rawRef : first);
      }
    }
  }

  if (redacted.size > 0) {
    out._meta = { redactedAttributes: Array.from(redacted).sort() };
  }
  return out;
}

export function redactContext(
  ctx: SkotosContext,
  cfg: RedactionConfig = {},
): Record<string, unknown> {
  if (ctx.kind === "multi") {
    const out: Record<string, unknown> = { kind: "multi" };
    for (const [k, v] of Object.entries(ctx)) {
      if (k === "kind" || !v || typeof v !== "object") continue;
      out[k] = redactSingle({ kind: k, ...(v as SingleContext) }, cfg, false);
    }
    return out;
  }
  return redactSingle(ctx as SingleContext, cfg, true);
}
