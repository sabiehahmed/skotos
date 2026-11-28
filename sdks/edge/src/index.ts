import {
  boolVariation,
  checkBigSegmentSnapshot,
  evaluate,
  getContextKinds,
  numberVariation,
  stringVariation,
} from "./evaluator.js";
import type {
  BigSegmentSnapshot,
  EnvironmentSnapshot,
  EvalFlag,
  EvalSegment,
  EvalStore,
  EvaluationDetail,
  SkotosContext,
} from "./types.js";

export * from "./types.js";
export {
  boolVariation,
  checkBigSegmentSnapshot,
  evaluate,
  getContextKind,
  getContextKinds,
  hashToBucket,
  numberVariation,
  parseAttributeRef,
  redactContext,
  stringVariation,
  validateContext,
} from "./evaluator.js";
export { bigSegmentKeyHash, sha1Hex } from "./sha1.js";

/**
 * Minimal KV interface compatible with Cloudflare Workers KV, Vercel Edge Config / KV,
 * Deno KV, Upstash Redis, or in-memory Maps.
 */
export interface EdgeKvStore {
  get(key: string): Promise<string | null>;
}

/**
 * Pluggable Edge snapshot source (KV, R2/HTTP, or static/in-memory).
 */
export interface EdgeSnapshotProvider {
  loadSnapshot(): Promise<EnvironmentSnapshot | null>;
}

export interface HttpSnapshotProviderOptions {
  /** Base URL of Skotos delivery-edge or Relay Proxy (e.g. `https://edge.skotos.dev`). */
  baseUrl: string;
  /** Server SDK key (`sdk-...`). */
  sdkKey: string;
  /** Custom `fetch` implementation (defaults to global Web `fetch`). */
  fetchFn?: typeof fetch;
}

/**
 * Fetches environment snapshots over standard Web `fetch` with `If-None-Match` ETag caching.
 */
export class HttpSnapshotProvider implements EdgeSnapshotProvider {
  private readonly baseUrl: string;
  private readonly sdkKey: string;
  private readonly fetchFn: typeof fetch;
  private etag: string | null = null;
  private cached: EnvironmentSnapshot | null = null;

  constructor(opts: HttpSnapshotProviderOptions) {
    this.baseUrl = opts.baseUrl.replace(/\/+$/, "");
    this.sdkKey = opts.sdkKey;
    this.fetchFn = opts.fetchFn ?? ((input, init) => fetch(input, init));
  }

  async loadSnapshot(): Promise<EnvironmentSnapshot | null> {
    const headers: Record<string, string> = {
      Authorization: this.sdkKey,
      Accept: "application/json",
    };
    if (this.etag) {
      headers["If-None-Match"] = this.etag;
    }

    const resp = await this.fetchFn(`${this.baseUrl}/sdk/v1/latest-all`, {
      method: "GET",
      headers,
    });
    if (resp.status === 304 && this.cached) {
      return this.cached;
    }
    if (!resp.ok) {
      throw new Error(`Skotos Edge HTTP snapshot fetch failed: ${resp.status}`);
    }
    const nextEtag = resp.headers.get("etag");
    if (nextEtag) this.etag = nextEtag;
    const snap = (await resp.json()) as EnvironmentSnapshot;
    this.cached = snap;
    return snap;
  }
}

export interface KvSnapshotProviderOptions {
  kv: EdgeKvStore;
  /** Key in KV storing the JSON `EnvironmentSnapshot`. */
  snapshotKey: string;
  /** Optional prefix for Big Segment snapshots (e.g. `skotos:bigseg:`). */
  bigSegmentPrefix?: string;
}

/**
 * Loads environment and Big Segment snapshots from Cloudflare KV / Vercel Edge Config / Deno KV.
 */
export class KvSnapshotProvider implements EdgeSnapshotProvider {
  private readonly kv: EdgeKvStore;
  private readonly snapshotKey: string;
  private readonly bigSegmentPrefix?: string;

  constructor(opts: KvSnapshotProviderOptions) {
    this.kv = opts.kv;
    this.snapshotKey = opts.snapshotKey;
    this.bigSegmentPrefix = opts.bigSegmentPrefix;
  }

  async loadSnapshot(): Promise<EnvironmentSnapshot | null> {
    const raw = await this.kv.get(this.snapshotKey);
    if (!raw) return null;
    const snap = JSON.parse(raw) as EnvironmentSnapshot;
    if (this.bigSegmentPrefix && snap.segments) {
      const bigSegments: Record<string, BigSegmentSnapshot> = { ...(snap.bigSegments ?? {}) };
      for (const seg of Object.values(snap.segments)) {
        if (!seg.unbounded) continue;
        const bsRaw = await this.kv.get(`${this.bigSegmentPrefix}${seg.key}`);
        if (bsRaw) {
          bigSegments[seg.key] = JSON.parse(bsRaw) as BigSegmentSnapshot;
        }
      }
      snap.bigSegments = bigSegments;
    }
    return snap;
  }
}

export interface EdgeClientOptions {
  /** Optional telemetry ingestion base URL (`https://events.skotos.dev`). */
  eventsUrl?: string;
  /** Optional SDK key for telemetry submission (`sdk-...`). */
  sdkKey?: string;
  /** Custom `fetch` implementation for event flushing. */
  fetchFn?: typeof fetch;
  /** Disable telemetry buffering when set to `true`. */
  sendEvents?: boolean;
}

interface PendingSummaryCounter {
  variationIndex?: number;
  version: number;
  value: unknown;
  count: number;
  contextKinds: Set<string>;
}

/**
 * Pure Web-Standard Skotos Edge SDK client for Cloudflare Workers, Vercel Edge,
 * Deno Deploy, and Fastly Compute. Evaluates flags synchronously in-isolate with
 * zero Node.js built-ins and optional `waitUntil`-friendly summary event flushing.
 */
export class SkotosEdgeClient {
  private flags = new Map<string, EvalFlag>();
  private segments = new Map<string, EvalSegment>();
  private bigSegments = new Map<string, BigSegmentSnapshot>();
  private initialized = false;
  private revision = 0;
  private readonly provider?: EdgeSnapshotProvider;
  private readonly opts: EdgeClientOptions;
  private summaryStartMs = Date.now();
  private summaryCounters = new Map<string, PendingSummaryCounter>();

  private readonly store: EvalStore = {
    getFlag: (key) => this.flags.get(key),
    getSegment: (key) => this.segments.get(key),
    bigSegmentMembership: (segmentKey, contextKind, contextKey) => {
      const bs = this.bigSegments.get(segmentKey);
      if (!bs) return undefined;
      return checkBigSegmentSnapshot(bs, contextKind, contextKey);
    },
  };

  constructor(provider?: EdgeSnapshotProvider, opts: EdgeClientOptions = {}) {
    this.provider = provider;
    this.opts = opts;
  }

  /**
   * Initializes a client asynchronously from a KV, HTTP, or custom snapshot provider.
   */
  static async init(
    provider: EdgeSnapshotProvider,
    opts: EdgeClientOptions = {},
  ): Promise<SkotosEdgeClient> {
    const client = new SkotosEdgeClient(provider, opts);
    await client.refresh();
    return client;
  }

  /**
   * Creates an already-initialized client synchronously from an in-memory snapshot.
   */
  static fromSnapshot(
    snapshot: EnvironmentSnapshot,
    opts: EdgeClientOptions = {},
  ): SkotosEdgeClient {
    const client = new SkotosEdgeClient(undefined, opts);
    client.applySnapshot(snapshot);
    return client;
  }

  isInitialized(): boolean {
    return this.initialized;
  }

  getRevision(): number {
    return this.revision;
  }

  applySnapshot(snapshot: EnvironmentSnapshot): void {
    this.flags.clear();
    for (const [k, v] of Object.entries(snapshot.flags ?? {})) {
      this.flags.set(k, v);
    }
    this.segments.clear();
    for (const [k, v] of Object.entries(snapshot.segments ?? {})) {
      this.segments.set(k, v);
    }
    this.bigSegments.clear();
    for (const [k, v] of Object.entries(snapshot.bigSegments ?? {})) {
      this.bigSegments.set(k, v);
    }
    this.revision = snapshot.revision ?? this.revision;
    this.initialized = true;
  }

  async refresh(): Promise<boolean> {
    if (!this.provider) return false;
    const snap = await this.provider.loadSnapshot();
    if (!snap) return false;
    this.applySnapshot(snap);
    return true;
  }

  boolVariationDetail(
    flagKey: string,
    context: SkotosContext,
    defaultValue: boolean,
  ): EvaluationDetail<boolean> {
    if (!this.initialized) {
      return { value: defaultValue, reason: { kind: "ERROR", errorKind: "CLIENT_NOT_READY" } };
    }
    const detail = boolVariation(this.store, flagKey, context, defaultValue);
    this.recordSummary(flagKey, context, detail);
    return detail;
  }

  boolVariation(flagKey: string, context: SkotosContext, defaultValue: boolean): boolean {
    return this.boolVariationDetail(flagKey, context, defaultValue).value;
  }

  stringVariationDetail(
    flagKey: string,
    context: SkotosContext,
    defaultValue: string,
  ): EvaluationDetail<string> {
    if (!this.initialized) {
      return { value: defaultValue, reason: { kind: "ERROR", errorKind: "CLIENT_NOT_READY" } };
    }
    const detail = stringVariation(this.store, flagKey, context, defaultValue);
    this.recordSummary(flagKey, context, detail);
    return detail;
  }

  stringVariation(flagKey: string, context: SkotosContext, defaultValue: string): string {
    return this.stringVariationDetail(flagKey, context, defaultValue).value;
  }

  numberVariationDetail(
    flagKey: string,
    context: SkotosContext,
    defaultValue: number,
  ): EvaluationDetail<number> {
    if (!this.initialized) {
      return { value: defaultValue, reason: { kind: "ERROR", errorKind: "CLIENT_NOT_READY" } };
    }
    const detail = numberVariation(this.store, flagKey, context, defaultValue);
    this.recordSummary(flagKey, context, detail);
    return detail;
  }

  numberVariation(flagKey: string, context: SkotosContext, defaultValue: number): number {
    return this.numberVariationDetail(flagKey, context, defaultValue).value;
  }

  jsonVariationDetail<T = unknown>(
    flagKey: string,
    context: SkotosContext,
    defaultValue: T,
  ): EvaluationDetail<T> {
    if (!this.initialized) {
      return { value: defaultValue, reason: { kind: "ERROR", errorKind: "CLIENT_NOT_READY" } };
    }
    const detail = evaluate(this.store, flagKey, context, defaultValue) as EvaluationDetail<T>;
    this.recordSummary(flagKey, context, detail);
    return detail;
  }

  jsonVariation<T = unknown>(flagKey: string, context: SkotosContext, defaultValue: T): T {
    return this.jsonVariationDetail(flagKey, context, defaultValue).value;
  }

  allFlagsState(context: SkotosContext): Record<string, unknown> {
    const out: Record<string, unknown> = {};
    for (const key of this.flags.keys()) {
      out[key] = evaluate(this.store, key, context, null).value;
    }
    return out;
  }

  private recordSummary(flagKey: string, context: SkotosContext, detail: EvaluationDetail): void {
    if (this.opts.sendEvents === false || !this.opts.eventsUrl || !this.opts.sdkKey) return;
    const flag = this.flags.get(flagKey);
    const version = flag?.version ?? 0;
    const bucketKey = `${flagKey}::${detail.variationIndex ?? -1}::${version}`;
    let entry = this.summaryCounters.get(bucketKey);
    if (!entry) {
      entry = {
        variationIndex: detail.variationIndex,
        version,
        value: detail.value,
        count: 0,
        contextKinds: new Set<string>(),
      };
      this.summaryCounters.set(bucketKey, entry);
    }
    entry.count += 1;
    for (const [kind] of getContextKinds(context)) {
      entry.contextKinds.add(kind);
    }
  }

  /**
   * Flushes aggregated summary counters to `services/event-ingest`. Designed to be
   * passed to `ctx.waitUntil(client.flushEvents())` in Cloudflare Workers / Vercel Edge.
   */
  async flushEvents(): Promise<void> {
    if (!this.opts.eventsUrl || !this.opts.sdkKey || this.summaryCounters.size === 0) return;

    const features: Record<
      string,
      {
        contextKinds: string[];
        counters: Array<{
          variation?: number;
          version: number;
          value: unknown;
          count: number;
        }>;
      }
    > = {};

    for (const [bucketKey, counter] of this.summaryCounters.entries()) {
      const flagKey = bucketKey.split("::")[0]!;
      let feat = features[flagKey];
      if (!feat) {
        feat = { contextKinds: [], counters: [] };
        features[flagKey] = feat;
      }
      for (const k of counter.contextKinds) {
        if (!feat.contextKinds.includes(k)) feat.contextKinds.push(k);
      }
      feat.counters.push({
        variation: counter.variationIndex,
        version: counter.version,
        value: counter.value,
        count: counter.count,
      });
    }

    const startDate = this.summaryStartMs;
    const endDate = Date.now();
    this.summaryCounters.clear();
    this.summaryStartMs = endDate;

    const fetchFn = this.opts.fetchFn ?? ((input, init) => fetch(input, init));
    const base = this.opts.eventsUrl.replace(/\/+$/, "");
    await fetchFn(`${base}/events/v1/bulk`, {
      method: "POST",
      headers: {
        Authorization: this.opts.sdkKey,
        "Content-Type": "application/json",
      },
      body: JSON.stringify([{ kind: "summary", startDate, endDate, features }]),
    });
  }
}

export interface OpenFeatureResolutionDetails<T> {
  value: T;
  variant?: string;
  reason?: string;
  errorCode?: string;
}

/**
 * OpenFeature-compatible provider backed by `SkotosEdgeClient`.
 */
export class SkotosOpenFeatureEdgeProvider {
  readonly metadata = { name: "skotos-edge-provider" } as const;
  private readonly client: SkotosEdgeClient;

  constructor(client: SkotosEdgeClient) {
    this.client = client;
  }

  private toContext(evalCtx?: Record<string, unknown>): SkotosContext {
    const key =
      typeof evalCtx?.targetingKey === "string" && evalCtx.targetingKey.length > 0
        ? evalCtx.targetingKey
        : "anonymous";
    const kind =
      typeof evalCtx?.kind === "string" && evalCtx.kind.length > 0 ? evalCtx.kind : "user";
    return { ...(evalCtx ?? {}), kind, key };
  }

  private mapDetail<T>(detail: EvaluationDetail<T>): OpenFeatureResolutionDetails<T> {
    if (detail.reason.kind === "ERROR") {
      return {
        value: detail.value,
        reason: "ERROR",
        errorCode:
          detail.reason.errorKind === "FLAG_NOT_FOUND"
            ? "FLAG_NOT_FOUND"
            : detail.reason.errorKind === "WRONG_TYPE"
              ? "TYPE_MISMATCH"
              : "GENERAL",
      };
    }
    return {
      value: detail.value,
      variant: detail.variationIndex !== undefined ? String(detail.variationIndex) : undefined,
      reason:
        detail.reason.kind === "TARGET_MATCH" || detail.reason.kind === "RULE_MATCH"
          ? "TARGETING_MATCH"
          : detail.reason.kind === "OFF"
            ? "DISABLED"
            : "DEFAULT",
    };
  }

  async resolveBooleanEvaluation(
    flagKey: string,
    defaultValue: boolean,
    context?: Record<string, unknown>,
  ): Promise<OpenFeatureResolutionDetails<boolean>> {
    return this.mapDetail(
      this.client.boolVariationDetail(flagKey, this.toContext(context), defaultValue),
    );
  }

  async resolveStringEvaluation(
    flagKey: string,
    defaultValue: string,
    context?: Record<string, unknown>,
  ): Promise<OpenFeatureResolutionDetails<string>> {
    return this.mapDetail(
      this.client.stringVariationDetail(flagKey, this.toContext(context), defaultValue),
    );
  }

  async resolveNumberEvaluation(
    flagKey: string,
    defaultValue: number,
    context?: Record<string, unknown>,
  ): Promise<OpenFeatureResolutionDetails<number>> {
    return this.mapDetail(
      this.client.numberVariationDetail(flagKey, this.toContext(context), defaultValue),
    );
  }

  async resolveObjectEvaluation<T>(
    flagKey: string,
    defaultValue: T,
    context?: Record<string, unknown>,
  ): Promise<OpenFeatureResolutionDetails<T>> {
    return this.mapDetail(
      this.client.jsonVariationDetail(flagKey, this.toContext(context), defaultValue),
    );
  }
}
