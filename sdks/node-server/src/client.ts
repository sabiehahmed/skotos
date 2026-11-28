import { randomUUID } from "node:crypto";
import { gzipSync } from "node:zlib";
import {
  boolVariation,
  evaluate,
  getContextKinds,
  numberVariation,
  redactContext,
  stringVariation,
} from "./evaluator";
import type {
  EvalFlag,
  EvalSegment,
  EvalStore,
  EvaluationDetail,
  RedactionConfig,
  SingleContext,
  SkotosContext,
} from "./types";

interface StoredEntry<T> {
  version: number;
  item?: T;
}

/**
 * Monotonically version-checked in-memory feature store (`put` replaces all;
 * `patch` / `delete` apply only when `incoming.version > stored.version`).
 */
export class VersionedFeatureStore implements EvalStore {
  private initialized = false;
  private flags = new Map<string, StoredEntry<EvalFlag>>();
  private segments = new Map<string, StoredEntry<EvalSegment>>();

  isInitialized(): boolean {
    return this.initialized;
  }

  put(flags: Record<string, EvalFlag>, segments: Record<string, EvalSegment>): string[] {
    const changed: string[] = [];
    for (const [k, f] of Object.entries(flags)) {
      const old = this.flags.get(k);
      if (!old || old.version !== f.version || !old.item) {
        changed.push(k);
      }
    }
    this.flags.clear();
    for (const [k, f] of Object.entries(flags)) {
      this.flags.set(k, { version: f.version, item: f });
    }
    this.segments.clear();
    for (const [k, s] of Object.entries(segments)) {
      this.segments.set(k, { version: s.version ?? 1, item: s });
    }
    this.initialized = true;
    return changed;
  }

  upsertFlag(flag: EvalFlag): boolean {
    const cur = this.flags.get(flag.key);
    if (cur && flag.version <= cur.version) return false;
    this.flags.set(flag.key, { version: flag.version, item: flag });
    this.initialized = true;
    return true;
  }

  deleteFlag(key: string, version: number): boolean {
    const cur = this.flags.get(key);
    if (cur && version <= cur.version) return false;
    this.flags.set(key, { version });
    return true;
  }

  upsertSegment(seg: EvalSegment): boolean {
    const ver = seg.version ?? 1;
    const cur = this.segments.get(seg.key);
    if (cur && ver <= cur.version) return false;
    this.segments.set(seg.key, { version: ver, item: seg });
    return true;
  }

  deleteSegment(key: string, version: number): boolean {
    const cur = this.segments.get(key);
    if (cur && version <= cur.version) return false;
    this.segments.set(key, { version });
    return true;
  }

  getFlag(key: string): EvalFlag | undefined {
    return this.flags.get(key)?.item;
  }

  getSegment(key: string): EvalSegment | undefined {
    return this.segments.get(key)?.item;
  }

  allFlagKeys(): string[] {
    const keys: string[] = [];
    for (const [k, v] of this.flags.entries()) {
      if (v.item) keys.push(k);
    }
    return keys;
  }
}

/**
 * Offline mock data source (`TestData`) for zero-network unit & integration tests.
 */
export class TestData {
  readonly store = new VersionedFeatureStore();

  constructor() {
    this.store.put({}, {});
  }

  setBoolFlag(key: string, value: boolean): this {
    const nextVer = (this.store.getFlag(key)?.version ?? 0) + 1;
    this.store.upsertFlag({
      key,
      version: nextVer,
      on: true,
      salt: "test-salt",
      variations: [true, false],
      offVariation: 1,
      fallthrough: { variation: value ? 0 : 1 },
    });
    return this;
  }

  setFlag(flag: EvalFlag): this {
    this.store.upsertFlag(flag);
    return this;
  }

  setSegment(segment: EvalSegment): this {
    this.store.upsertSegment(segment);
    return this;
  }
}

export interface SkotosNodeConfig {
  sdkKey: string;
  streamUrl: string;
  pollUrl?: string;
  eventsUrl?: string;
  offline?: boolean;
  usePolling?: boolean;
  pollIntervalMs?: number;
  flushIntervalMs?: number;
  redaction?: RedactionConfig;
  /**
   * Send a redacted copy of each context the first time it is evaluated in a window, so
   * the dashboard can suggest context kinds, attribute names and keys. Default true.
   */
  sendIndexEvents?: boolean;
  /** Max distinct contexts remembered between index windows. Default 1000. */
  contextKeysCapacity?: number;
  /** How often the remembered contexts reset, re-sending index events. Default 5 minutes. */
  contextKeysFlushIntervalMs?: number;
}

type ChangeListener = (flagKey: string) => void;

export class SkotosNodeClient {
  private readonly store: VersionedFeatureStore;
  private readonly cfg: SkotosNodeConfig;
  private readonly abortController = new AbortController();
  private readonly readyResolvers: Array<() => void> = [];
  private readonly listeners = new Map<string, Set<ChangeListener>>();
  private flushTimer?: ReturnType<typeof setInterval>;

  private windowStartMs = Date.now();
  private summary = new Map<
    string,
    {
      flagKey: string;
      version?: number;
      variation?: number;
      value: unknown;
      defaultVal: unknown;
      count: number;
    }
  >();
  private flagContextKinds = new Map<string, Set<string>>();
  private rawEvents: unknown[] = [];
  private indexedContexts = new Set<string>();
  private indexWindowStartMs = Date.now();

  constructor(config: SkotosNodeConfig, existingStore?: VersionedFeatureStore) {
    this.cfg = config;
    this.store = existingStore ?? new VersionedFeatureStore();

    if (!config.offline) {
      if (config.usePolling) {
        void this.pollLoop();
      } else {
        void this.streamLoop();
      }
      if (config.eventsUrl) {
        const interval = config.flushIntervalMs ?? 5_000;
        this.flushTimer = setInterval(() => {
          void this.flush();
        }, interval);
        this.flushTimer.unref?.();
      }
    }
  }

  static withTestData(testData: TestData): SkotosNodeClient {
    return new SkotosNodeClient(
      { sdkKey: "offline", streamUrl: "", offline: true },
      testData.store,
    );
  }

  isInitialized(): boolean {
    return Boolean(this.cfg.offline) || this.store.isInitialized();
  }

  async waitForInitialization(timeoutMs = 5_000): Promise<boolean> {
    if (this.isInitialized()) return true;
    await new Promise<void>((resolve) => {
      const timer = setTimeout(() => {
        resolve();
      }, timeoutMs);
      this.readyResolvers.push(() => {
        clearTimeout(timer);
        resolve();
      });
    });
    return this.isInitialized();
  }

  on(event: "change" | `change:${string}`, listener: ChangeListener): () => void {
    let set = this.listeners.get(event);
    if (!set) {
      set = new Set();
      this.listeners.set(event, set);
    }
    set.add(listener);
    return () => set?.delete(listener);
  }

  private emitChanges(keys: string[]) {
    const globalListeners = this.listeners.get("change");
    for (const key of keys) {
      globalListeners?.forEach((fn) => fn(key));
      this.listeners.get(`change:${key}`)?.forEach((fn) => fn(key));
    }
  }

  private markReady() {
    while (this.readyResolvers.length > 0) {
      this.readyResolvers.shift()?.();
    }
  }

  boolVariation(flagKey: string, ctx: SkotosContext, defaultValue: boolean): boolean {
    return this.boolVariationDetail(flagKey, ctx, defaultValue).value;
  }

  boolVariationDetail(
    flagKey: string,
    ctx: SkotosContext,
    defaultValue: boolean,
  ): EvaluationDetail<boolean> {
    if (!this.isInitialized()) {
      return { value: defaultValue, reason: { kind: "ERROR", errorKind: "CLIENT_NOT_READY" } };
    }
    const detail = boolVariation(this.store, flagKey, ctx, defaultValue);
    this.recordEval(flagKey, ctx, defaultValue, detail);
    return detail;
  }

  stringVariation(flagKey: string, ctx: SkotosContext, defaultValue: string): string {
    return this.stringVariationDetail(flagKey, ctx, defaultValue).value;
  }

  stringVariationDetail(
    flagKey: string,
    ctx: SkotosContext,
    defaultValue: string,
  ): EvaluationDetail<string> {
    if (!this.isInitialized()) {
      return { value: defaultValue, reason: { kind: "ERROR", errorKind: "CLIENT_NOT_READY" } };
    }
    const detail = stringVariation(this.store, flagKey, ctx, defaultValue);
    this.recordEval(flagKey, ctx, defaultValue, detail);
    return detail;
  }

  numberVariation(flagKey: string, ctx: SkotosContext, defaultValue: number): number {
    return this.numberVariationDetail(flagKey, ctx, defaultValue).value;
  }

  numberVariationDetail(
    flagKey: string,
    ctx: SkotosContext,
    defaultValue: number,
  ): EvaluationDetail<number> {
    if (!this.isInitialized()) {
      return { value: defaultValue, reason: { kind: "ERROR", errorKind: "CLIENT_NOT_READY" } };
    }
    const detail = numberVariation(this.store, flagKey, ctx, defaultValue);
    this.recordEval(flagKey, ctx, defaultValue, detail);
    return detail;
  }

  jsonVariation<T = unknown>(flagKey: string, ctx: SkotosContext, defaultValue: T): T {
    return this.jsonVariationDetail(flagKey, ctx, defaultValue).value;
  }

  jsonVariationDetail<T = unknown>(
    flagKey: string,
    ctx: SkotosContext,
    defaultValue: T,
  ): EvaluationDetail<T> {
    if (!this.isInitialized()) {
      return { value: defaultValue, reason: { kind: "ERROR", errorKind: "CLIENT_NOT_READY" } };
    }
    const detail = evaluate(this.store, flagKey, ctx, defaultValue) as EvaluationDetail<T>;
    this.recordEval(flagKey, ctx, defaultValue, detail);
    return detail;
  }

  identify(ctx: SkotosContext): void {
    if (this.rawEvents.length < 5_000) {
      this.rawEvents.push({
        kind: "identify",
        creationDate: Date.now(),
        context: redactContext(ctx, this.cfg.redaction),
      });
    }
  }

  track(eventKey: string, ctx: SkotosContext, data?: unknown, metricValue?: number): void {
    if (this.rawEvents.length < 5_000) {
      const contextKeys: Record<string, string> = {};
      for (const [k, sc] of getContextKinds(ctx)) {
        contextKeys[k] = sc.key;
      }
      this.rawEvents.push({
        kind: "custom",
        creationDate: Date.now(),
        key: eventKey,
        contextKeys,
        data,
        metricValue,
      });
    }
  }

  private recordEval(
    flagKey: string,
    ctx: SkotosContext,
    defaultVal: unknown,
    detail: EvaluationDetail,
  ) {
    const flag = this.store.getFlag(flagKey);
    const version = flag?.version;
    const mapKey = `${flagKey}|${version ?? "u"}|${detail.variationIndex ?? "d"}`;
    const cur = this.summary.get(mapKey);
    if (cur) {
      cur.count += 1;
    } else {
      this.summary.set(mapKey, {
        flagKey,
        version,
        variation: detail.variationIndex,
        value: detail.value,
        defaultVal,
        count: 1,
      });
    }
    let kinds = this.flagContextKinds.get(flagKey);
    if (!kinds) {
      kinds = new Set();
      this.flagContextKinds.set(flagKey, kinds);
    }
    for (const [k] of getContextKinds(ctx)) {
      kinds.add(k);
    }
    this.maybeIndexContext(ctx);
  }

  /** Queues one `index` event per distinct context per window (LaunchDarkly-style). */
  private maybeIndexContext(ctx: SkotosContext) {
    if (this.cfg.sendIndexEvents === false) return;
    const now = Date.now();
    if (now - this.indexWindowStartMs >= (this.cfg.contextKeysFlushIntervalMs ?? 300_000)) {
      this.indexedContexts.clear();
      this.indexWindowStartMs = now;
    }
    const id = getContextKinds(ctx)
      .map(([k, sc]) => `${k}:${sc.key}`)
      .sort()
      .join("|");
    if (!id || this.indexedContexts.has(id)) return;
    if (this.indexedContexts.size >= (this.cfg.contextKeysCapacity ?? 1_000)) {
      const oldest = this.indexedContexts.values().next().value;
      if (oldest !== undefined) this.indexedContexts.delete(oldest);
    }
    this.indexedContexts.add(id);
    if (this.rawEvents.length < 5_000) {
      this.rawEvents.push({
        kind: "index",
        creationDate: now,
        context: redactContext(ctx, this.cfg.redaction),
      });
    }
  }

  async flush(): Promise<number> {
    const endDate = Date.now();
    const startDate = this.windowStartMs;
    this.windowStartMs = endDate;
    const events: unknown[] = [...this.rawEvents];
    this.rawEvents = [];

    if (this.summary.size > 0) {
      const features: Record<
        string,
        { default: unknown; contextKinds: string[]; counters: unknown[] }
      > = {};
      for (const item of this.summary.values()) {
        let bucket = features[item.flagKey];
        if (!bucket) {
          bucket = {
            default: item.defaultVal,
            contextKinds: Array.from(this.flagContextKinds.get(item.flagKey) ?? []),
            counters: [],
          };
          features[item.flagKey] = bucket;
        }
        bucket.counters.push({
          variation: item.variation,
          version: item.version,
          value: item.value,
          unknown: item.version === undefined,
          count: item.count,
        });
      }
      events.push({ kind: "summary", startDate, endDate, features });
      this.summary.clear();
      this.flagContextKinds.clear();
    }

    if (events.length === 0 || !this.cfg.eventsUrl) return events.length;
    const gz = gzipSync(Buffer.from(JSON.stringify(events), "utf8"));
    const url = `${this.cfg.eventsUrl.replace(/\/$/, "")}/events/v1/bulk`;
    await fetch(url, {
      method: "POST",
      headers: {
        authorization: this.cfg.sdkKey,
        "content-type": "application/json",
        "content-encoding": "gzip",
        "x-skotos-payload-id": randomUUID(),
      },
      body: gz,
    });
    return events.length;
  }

  async close(): Promise<void> {
    this.abortController.abort();
    if (this.flushTimer) clearInterval(this.flushTimer);
    await this.flush();
  }

  private async streamLoop(): Promise<void> {
    const url = `${this.cfg.streamUrl.replace(/\/$/, "")}/stream/v1/all`;
    let backoffMs = 250;
    while (!this.abortController.signal.aborted) {
      try {
        const res = await fetch(url, {
          headers: {
            authorization: this.cfg.sdkKey,
            accept: "text/event-stream",
          },
          signal: this.abortController.signal,
        });
        if (res.status === 401 || res.status === 403) return;
        if (res.ok && res.body) {
          backoffMs = 250;
          const reader = res.body.getReader();
          const decoder = new TextDecoder();
          let buf = "";
          let evName = "";
          let evData = "";
          for (;;) {
            const { value, done } = await reader.read();
            if (done) break;
            buf += decoder.decode(value, { stream: true });
            let nl: number;
            while ((nl = buf.indexOf("\n")) >= 0) {
              const line = buf.slice(0, nl).replace(/\r$/, "");
              buf = buf.slice(nl + 1);
              if (line === "") {
                if (evData) this.applySSE(evName, evData);
                evName = "";
                evData = "";
              } else if (line.startsWith("event:")) {
                evName = line.slice(6).trim();
              } else if (line.startsWith("data:")) {
                evData += (evData ? "\n" : "") + line.slice(5).trimStart();
              }
            }
          }
        }
      } catch {
        // Reconnect with exponential backoff
      }
      if (this.abortController.signal.aborted) break;
      await new Promise((r) => setTimeout(r, backoffMs));
      backoffMs = Math.min(backoffMs * 2, 30_000);
    }
  }

  private applySSE(eventName: string, rawData: string) {
    try {
      const parsed = JSON.parse(rawData) as Record<string, unknown>;
      if (eventName === "put") {
        const data = (parsed.data ?? {}) as {
          flags?: Record<string, EvalFlag>;
          segments?: Record<string, EvalSegment>;
        };
        const changed = this.store.put(data.flags ?? {}, data.segments ?? {});
        this.markReady();
        this.emitChanges(changed);
      } else if (eventName === "patch") {
        const path = String(parsed.path ?? "");
        if (path.startsWith("/flags/")) {
          const flag = parsed.data as EvalFlag;
          if (this.store.upsertFlag(flag)) this.emitChanges([flag.key]);
        } else if (path.startsWith("/segments/")) {
          this.store.upsertSegment(parsed.data as EvalSegment);
        }
      } else if (eventName === "delete") {
        const path = String(parsed.path ?? "");
        const ver = Number(parsed.version ?? 0);
        if (path.startsWith("/flags/")) {
          const key = path.slice("/flags/".length);
          if (this.store.deleteFlag(key, ver)) this.emitChanges([key]);
        } else if (path.startsWith("/segments/")) {
          this.store.deleteSegment(path.slice("/segments/".length), ver);
        }
      }
    } catch {
      // Ignore malformed SSE payload
    }
  }

  private async pollLoop(): Promise<void> {
    const base = (this.cfg.pollUrl || this.cfg.streamUrl).replace(/\/$/, "");
    const url = `${base}/sdk/v1/latest-all`;
    let etag: string | undefined;
    const interval = this.cfg.pollIntervalMs ?? 30_000;

    while (!this.abortController.signal.aborted) {
      try {
        const headers: Record<string, string> = { authorization: this.cfg.sdkKey };
        if (etag) headers["if-none-match"] = etag;
        const res = await fetch(url, { headers, signal: this.abortController.signal });
        if (res.status === 401 || res.status === 403) return;
        if (res.status === 200) {
          etag = res.headers.get("etag") ?? undefined;
          const snap = (await res.json()) as {
            flags?: Record<string, EvalFlag>;
            segments?: Record<string, EvalSegment>;
          };
          const changed = this.store.put(snap.flags ?? {}, snap.segments ?? {});
          this.markReady();
          this.emitChanges(changed);
        }
      } catch {
        // Retry next tick
      }
      if (this.abortController.signal.aborted) break;
      await new Promise((r) => setTimeout(r, interval));
    }
  }
}

// =============================================================================
// Official OpenFeature Node Provider (ADR-0004)
// =============================================================================

export interface OpenFeatureResolutionDetails<T> {
  value: T;
  variant?: string;
  reason?: string;
  errorCode?: string;
}

export class SkotosOpenFeatureNodeProvider {
  readonly metadata = { name: "Skotos Node Server OpenFeature Provider" } as const;

  constructor(private readonly client: SkotosNodeClient) {}

  async resolveBooleanEvaluation(
    flagKey: string,
    defaultValue: boolean,
    context: Record<string, unknown>,
  ): Promise<OpenFeatureResolutionDetails<boolean>> {
    return toOFDetails(
      this.client.boolVariationDetail(flagKey, toSkotosCtx(context), defaultValue),
    );
  }

  async resolveStringEvaluation(
    flagKey: string,
    defaultValue: string,
    context: Record<string, unknown>,
  ): Promise<OpenFeatureResolutionDetails<string>> {
    return toOFDetails(
      this.client.stringVariationDetail(flagKey, toSkotosCtx(context), defaultValue),
    );
  }

  async resolveNumberEvaluation(
    flagKey: string,
    defaultValue: number,
    context: Record<string, unknown>,
  ): Promise<OpenFeatureResolutionDetails<number>> {
    return toOFDetails(
      this.client.numberVariationDetail(flagKey, toSkotosCtx(context), defaultValue),
    );
  }

  async resolveObjectEvaluation<T = Record<string, unknown>>(
    flagKey: string,
    defaultValue: T,
    context: Record<string, unknown>,
  ): Promise<OpenFeatureResolutionDetails<T>> {
    return toOFDetails(
      this.client.jsonVariationDetail(flagKey, toSkotosCtx(context), defaultValue),
    );
  }
}

function toSkotosCtx(ofCtx: Record<string, unknown>): SingleContext {
  const kind = typeof ofCtx.kind === "string" && ofCtx.kind ? ofCtx.kind : "user";
  const key =
    typeof ofCtx.targetingKey === "string"
      ? ofCtx.targetingKey
      : typeof ofCtx.key === "string"
        ? ofCtx.key
        : "";
  const rest: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(ofCtx)) {
    if (k === "kind" || k === "key" || k === "targetingKey") continue;
    rest[k] = v;
  }
  return { kind, key, ...rest };
}

function toOFDetails<T>(d: EvaluationDetail<T>): OpenFeatureResolutionDetails<T> {
  const variant = d.variationIndex !== undefined ? String(d.variationIndex) : undefined;
  switch (d.reason.kind) {
    case "OFF":
    case "PREREQUISITE_FAILED":
      return { value: d.value, variant, reason: "DISABLED" };
    case "TARGET_MATCH":
    case "RULE_MATCH":
      return { value: d.value, variant, reason: "TARGETING_MATCH" };
    case "FALLTHROUGH":
      return { value: d.value, variant, reason: "DEFAULT" };
    case "ERROR": {
      const codeMap: Record<string, string> = {
        CLIENT_NOT_READY: "PROVIDER_NOT_READY",
        FLAG_NOT_FOUND: "FLAG_NOT_FOUND",
        MALFORMED_FLAG: "PARSE_ERROR",
        USER_NOT_SPECIFIED: "TARGETING_KEY_MISSING",
        WRONG_TYPE: "TYPE_MISMATCH",
      };
      return {
        value: d.value,
        variant,
        reason: "ERROR",
        errorCode: codeMap[d.reason.errorKind] ?? "GENERAL",
      };
    }
  }
}
