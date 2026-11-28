/**
 * Skotos Mobile Client SDK (`@skotos/mobile-sdk`) for React Native, iOS, and Android.
 *
 * Offline-first mobile architecture:
 * - Persists last-known evaluations per context via an async `MobileStorageAdapter`
 *   (AsyncStorage / MMKV / Keychain / SharedPreferences) for instant cold-start reads.
 * - Uses Mobile Keys (`mob-...`) against `delivery-edge` / `relay-proxy` `/sdk/v1/evalx/{mobileKey}/context`.
 * - Supports Ping-to-Re-evaluate SSE refresh, foreground lifecycle refresh, custom metric tracking,
 *   and an OpenFeature-compatible mobile provider.
 */

export interface MobileContext {
  kind?: string;
  key?: string;
  name?: string;
  anonymous?: boolean;
  appVersion?: string;
  osName?: string;
  osVersion?: string;
  [attr: string]: unknown;
}

export interface MobileEvaluatedFlag {
  value: unknown;
  variationIndex?: number;
  version: number;
  flagVersion: number;
  reason?: {
    kind: string;
    ruleIndex?: number;
    ruleId?: string;
    prerequisiteKey?: string;
    inExperiment?: boolean;
    errorKind?: string;
  };
}

export interface MobileEvaluationDetail<T = unknown> {
  value: T;
  variationIndex?: number;
  reason?: MobileEvaluatedFlag["reason"];
}

/**
 * Pluggable persistent storage adapter (compatible with React Native AsyncStorage,
 * MMKV, Expo SecureStore, or in-memory test stores).
 */
export interface MobileStorageAdapter {
  getItem(key: string): Promise<string | null>;
  setItem(key: string, value: string): Promise<void>;
  removeItem?(key: string): Promise<void>;
}

export class InMemoryMobileStorage implements MobileStorageAdapter {
  private readonly store = new Map<string, string>();
  async getItem(key: string): Promise<string | null> {
    return this.store.get(key) ?? null;
  }
  async setItem(key: string, value: string): Promise<void> {
    this.store.set(key, value);
  }
  async removeItem(key: string): Promise<void> {
    this.store.delete(key);
  }
}

export interface MobileEventSourceLike {
  addEventListener(type: string, listener: (ev: { data: string }) => void): void;
  close(): void;
}

export interface SkotosMobileConfig {
  /** Mobile SDK Key (`mob-...`). */
  mobileKey: string;
  /** Delivery Edge or Relay Proxy base URL. */
  baseUrl: string;
  /** Optional Event Ingest URL (`https://events.skotos.dev`). */
  eventsUrl?: string;
  /** Optional persistent storage adapter for offline-first cold start. */
  storage?: MobileStorageAdapter;
  /** Include evaluation reasons from delivery edge. */
  withReasons?: boolean;
  /** Disable network calls and serve strictly from cache/defaults. */
  offline?: boolean;
  /** Enable ping-to-re-evaluate streaming when EventSource is available. */
  streaming?: boolean;
  /** Custom fetch implementation. */
  fetchImpl?: typeof fetch;
  /** Custom EventSource factory (for React Native SSE polyfills). */
  eventSourceFactory?: (url: string) => MobileEventSourceLike;
}

type ChangeListener = (newValue: unknown, oldValue: unknown, flagKey: string) => void;

function cacheKey(mobileKey: string, context: MobileContext): string {
  const ctxKey = typeof context.key === "string" && context.key ? context.key : "multi";
  return `skotos:mobile:${mobileKey}:${context.kind ?? "user"}:${ctxKey}`;
}

export class SkotosMobileClient {
  private readonly cfg: SkotosMobileConfig;
  private context: MobileContext;
  private flags: Record<string, MobileEvaluatedFlag> = {};
  private initialized = false;
  private fromCache = false;
  private lastRev = 0;
  private eventSource?: MobileEventSourceLike;
  private readonly listeners = new Map<string, Set<ChangeListener>>();
  private readonly customEvents: Array<Record<string, unknown>> = [];
  private readonly summaryCounters = new Map<
    string,
    { flagKey: string; version?: number; variation?: number; value: unknown; count: number }
  >();
  private windowStartMs = Date.now();

  constructor(config: SkotosMobileConfig, initialContext: MobileContext) {
    this.cfg = config;
    this.context = initialContext;
  }

  /**
   * Initializes the mobile client: hydrates from persistent storage first for instant
   * offline-first startup, then fetches fresh evaluations from Delivery Edge / Relay Proxy.
   */
  async init(): Promise<boolean> {
    await this.hydrateFromStorage();
    if (!this.cfg.offline) {
      await this.refresh();
      if (this.cfg.streaming) {
        this.connectPingStream();
      }
    }
    return this.initialized;
  }

  isInitialized(): boolean {
    return this.initialized;
  }

  isLoadedFromCache(): boolean {
    return this.fromCache;
  }

  getContext(): MobileContext {
    return this.context;
  }

  async identify(nextContext: MobileContext): Promise<void> {
    this.context = nextContext;
    await this.hydrateFromStorage();
    if (!this.cfg.offline) {
      await this.refresh(this.lastRev || undefined);
    }
  }

  /**
   * Call when the mobile app returns to foreground to pull latest flag state.
   */
  async onForeground(): Promise<void> {
    if (!this.cfg.offline) {
      await this.refresh(this.lastRev || undefined);
    }
  }

  boolVariation(flagKey: string, defaultValue: boolean): boolean {
    const val = this.variationDetail<boolean>(flagKey, defaultValue).value;
    return typeof val === "boolean" ? val : defaultValue;
  }

  stringVariation(flagKey: string, defaultValue: string): string {
    const val = this.variationDetail<string>(flagKey, defaultValue).value;
    return typeof val === "string" ? val : defaultValue;
  }

  numberVariation(flagKey: string, defaultValue: number): number {
    const val = this.variationDetail<number>(flagKey, defaultValue).value;
    return typeof val === "number" ? val : defaultValue;
  }

  variation<T = unknown>(flagKey: string, defaultValue: T): T {
    return this.variationDetail<T>(flagKey, defaultValue).value;
  }

  variationDetail<T = unknown>(flagKey: string, defaultValue: T): MobileEvaluationDetail<T> {
    const entry = this.flags[flagKey];
    const value = entry !== undefined ? (entry.value as T) : defaultValue;
    const variationIndex = entry?.variationIndex;
    this.recordSummary(flagKey, entry?.version, variationIndex, value);
    return { value, variationIndex, reason: entry?.reason };
  }

  allFlags(): Record<string, unknown> {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(this.flags)) {
      out[k] = v.value;
    }
    return out;
  }

  track(eventKey: string, metricValue?: number, data?: unknown): void {
    const ctxKind = typeof this.context.kind === "string" && this.context.kind ? this.context.kind : "user";
    const ctxKey = typeof this.context.key === "string" ? this.context.key : "";
    const ev: Record<string, unknown> = {
      kind: "custom",
      creationDate: Date.now(),
      key: eventKey,
      contextKeys: ctxKey ? { [ctxKind]: ctxKey } : {},
      context: this.context,
    };
    if (typeof metricValue === "number") ev.metricValue = metricValue;
    if (data !== undefined) ev.data = data;
    this.customEvents.push(ev);
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

  async refresh(rev?: number): Promise<boolean> {
    if (this.cfg.offline) return false;
    const fetchFn = this.cfg.fetchImpl ?? globalThis.fetch;
    if (!fetchFn) return false;
    const base = this.cfg.baseUrl.replace(/\/+$/, "");
    const key = encodeURIComponent(this.cfg.mobileKey);
    const params = new URLSearchParams();
    if (rev !== undefined) params.set("rev", String(rev));
    if (this.cfg.withReasons) params.set("withReasons", "true");
    const qs = params.toString() ? `?${params.toString()}` : "";

    try {
      const resp = await fetchFn(`${base}/sdk/v1/evalx/${key}/context${qs}`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: this.cfg.mobileKey,
        },
        body: JSON.stringify(this.context),
      });
      if (!resp.ok) return false;
      const next = (await resp.json()) as Record<string, MobileEvaluatedFlag>;
      const prev = this.flags;
      this.flags = next;
      this.initialized = true;
      this.fromCache = false;
      await this.persistToStorage(next);
      this.emitDiff(prev, next);
      return true;
    } catch {
      return false;
    }
  }

  async flush(): Promise<number> {
    const payload: Array<Record<string, unknown>> = [...this.customEvents];
    this.customEvents.length = 0;

    if (this.summaryCounters.size > 0) {
      const features: Record<
        string,
        {
          contextKinds: string[];
          counters: Array<{ version: number; variation?: number; value: unknown; count: number }>;
        }
      > = {};
      const ctxKind = typeof this.context.kind === "string" ? this.context.kind : "user";
      for (const c of this.summaryCounters.values()) {
        let f = features[c.flagKey];
        if (!f) {
          f = { contextKinds: [ctxKind], counters: [] };
          features[c.flagKey] = f;
        }
        f.counters.push({
          version: c.version ?? 0,
          variation: c.variation,
          value: c.value,
          count: c.count,
        });
      }
      const endDate = Date.now();
      payload.push({
        kind: "summary",
        startDate: this.windowStartMs,
        endDate,
        features,
      });
      this.summaryCounters.clear();
      this.windowStartMs = endDate;
    }

    if (payload.length === 0 || this.cfg.offline || !this.cfg.eventsUrl) {
      return payload.length;
    }
    const fetchFn = this.cfg.fetchImpl ?? globalThis.fetch;
    if (!fetchFn) return payload.length;
    const base = this.cfg.eventsUrl.replace(/\/+$/, "");
    await fetchFn(`${base}/events/v1/bulk`, {
      method: "POST",
      headers: {
        Authorization: this.cfg.mobileKey,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(payload),
    });
    return payload.length;
  }

  close(): void {
    this.eventSource?.close();
    this.eventSource = undefined;
  }

  private async hydrateFromStorage(): Promise<void> {
    if (!this.cfg.storage) return;
    try {
      const raw = await this.cfg.storage.getItem(cacheKey(this.cfg.mobileKey, this.context));
      if (raw) {
        this.flags = JSON.parse(raw) as Record<string, MobileEvaluatedFlag>;
        this.initialized = true;
        this.fromCache = true;
      }
    } catch {
      // Ignore corrupt cache
    }
  }

  private async persistToStorage(next: Record<string, MobileEvaluatedFlag>): Promise<void> {
    if (!this.cfg.storage) return;
    try {
      await this.cfg.storage.setItem(cacheKey(this.cfg.mobileKey, this.context), JSON.stringify(next));
    } catch {
      // Ignore quota errors
    }
  }

  private connectPingStream(): void {
    const factory = this.cfg.eventSourceFactory;
    if (!factory) return;
    const base = this.cfg.baseUrl.replace(/\/+$/, "");
    const key = encodeURIComponent(this.cfg.mobileKey);
    this.eventSource = factory(`${base}/stream/v1/eval/${key}`);
    this.eventSource.addEventListener("ping", (ev) => {
      try {
        const parsed = JSON.parse(ev.data) as { rev?: number };
        if (typeof parsed.rev === "number") this.lastRev = parsed.rev;
      } catch {
        // Ignore non-JSON pings
      }
      void this.refresh(this.lastRev || undefined);
    });
  }

  private recordSummary(
    flagKey: string,
    version: number | undefined,
    variation: number | undefined,
    value: unknown,
  ): void {
    const k = `${flagKey}::${variation ?? -1}::${version ?? 0}`;
    const existing = this.summaryCounters.get(k);
    if (existing) {
      existing.count += 1;
    } else {
      this.summaryCounters.set(k, { flagKey, version, variation, value, count: 1 });
    }
  }

  private emitDiff(
    prev: Record<string, MobileEvaluatedFlag>,
    next: Record<string, MobileEvaluatedFlag>,
  ): void {
    const keys = new Set([...Object.keys(prev), ...Object.keys(next)]);
    for (const k of keys) {
      const oldVal = prev[k]?.value;
      const newVal = next[k]?.value;
      if (JSON.stringify(oldVal) !== JSON.stringify(newVal)) {
        this.listeners.get("change")?.forEach((cb) => cb(newVal, oldVal, k));
        this.listeners.get(`change:${k}`)?.forEach((cb) => cb(newVal, oldVal, k));
      }
    }
  }
}

export interface OpenFeatureMobileDetails<T> {
  value: T;
  variant?: string;
  reason?: string;
}

/**
 * OpenFeature-compatible mobile provider backed by `SkotosMobileClient`.
 */
export class SkotosOpenFeatureMobileProvider {
  readonly metadata = { name: "skotos-mobile-provider" } as const;
  private readonly client: SkotosMobileClient;

  constructor(client: SkotosMobileClient) {
    this.client = client;
  }

  async onContextChange(nextContext: Record<string, unknown>): Promise<void> {
    const key =
      typeof nextContext.targetingKey === "string" && nextContext.targetingKey
        ? nextContext.targetingKey
        : typeof nextContext.key === "string"
          ? nextContext.key
          : "anonymous";
    const kind = typeof nextContext.kind === "string" ? nextContext.kind : "user";
    await this.client.identify({ ...nextContext, kind, key });
  }

  resolveBooleanEvaluation(flagKey: string, defaultValue: boolean): OpenFeatureMobileDetails<boolean> {
    const d = this.client.variationDetail<boolean>(flagKey, defaultValue);
    return {
      value: typeof d.value === "boolean" ? d.value : defaultValue,
      variant: d.variationIndex !== undefined ? String(d.variationIndex) : undefined,
      reason: d.reason?.kind ?? "DEFAULT",
    };
  }

  resolveStringEvaluation(flagKey: string, defaultValue: string): OpenFeatureMobileDetails<string> {
    const d = this.client.variationDetail<string>(flagKey, defaultValue);
    return {
      value: typeof d.value === "string" ? d.value : defaultValue,
      variant: d.variationIndex !== undefined ? String(d.variationIndex) : undefined,
      reason: d.reason?.kind ?? "DEFAULT",
    };
  }

  resolveNumberEvaluation(flagKey: string, defaultValue: number): OpenFeatureMobileDetails<number> {
    const d = this.client.variationDetail<number>(flagKey, defaultValue);
    return {
      value: typeof d.value === "number" ? d.value : defaultValue,
      variant: d.variationIndex !== undefined ? String(d.variationIndex) : undefined,
      reason: d.reason?.kind ?? "DEFAULT",
    };
  }
}
