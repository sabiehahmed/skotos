export interface ClientContext {
  kind?: string;
  key?: string;
  name?: string;
  anonymous?: boolean;
  [attr: string]: unknown;
}

export interface ClientEvaluatedFlag {
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

export interface BrowserEvaluationDetail<T = unknown> {
  value: T;
  variationIndex?: number;
  reason?: ClientEvaluatedFlag["reason"];
}

export interface SkotosBrowserConfig {
  clientSideId: string;
  baseUrl: string;
  eventsUrl?: string;
  streaming?: boolean;
  usePostForEval?: boolean;
  withReasons?: boolean;
  bootstrap?: Record<string, ClientEvaluatedFlag> | "localStorage";
  flushIntervalMs?: number;
  fetchImpl?: typeof fetch;
  eventSourceFactory?: (url: string) => EventSourceLike;
}

export interface EventSourceLike {
  addEventListener(type: string, listener: (ev: { data: string }) => void): void;
  close(): void;
}

type ChangeCallback = (newValue: unknown, oldValue: unknown, flagKey: string) => void;

function base64UrlEncodeUtf8(str: string): string {
  const bytes = new TextEncoder().encode(str);
  let binary = "";
  for (const b of bytes) {
    binary += String.fromCharCode(b);
  }
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function cacheStorageKey(clientSideId: string, context: ClientContext): string {
  const key = typeof context.key === "string" ? context.key : "multi";
  return `skotos:${clientSideId}:${context.kind ?? "user"}:${key}`;
}

/**
 * Skotos Browser / Client-Side SDK implementing Ping-to-Re-evaluate streaming (ADR-0003).
 */
export class SkotosBrowserClient {
  private readonly cfg: SkotosBrowserConfig;
  private context: ClientContext;
  private flags: Record<string, ClientEvaluatedFlag> = {};
  private initialized = false;
  private lastRev = 0;
  private eventSource?: EventSourceLike;
  private flushTimer?: ReturnType<typeof setInterval>;
  private readonly listeners = new Map<string, Set<ChangeCallback>>();
  private readonly readyPromise: Promise<void>;
  private resolveReady!: () => void;

  private windowStartMs = Date.now();
  private counters = new Map<
    string,
    { flagKey: string; version?: number; variation?: number; value: unknown; defaultVal: unknown; count: number }
  >();

  constructor(config: SkotosBrowserConfig, initialContext: ClientContext) {
    this.cfg = config;
    this.context = initialContext;
    this.readyPromise = new Promise<void>((res) => {
      this.resolveReady = res;
    });

    // Optional bootstrap from memory or localStorage for instant first-paint
    if (config.bootstrap === "localStorage" && typeof globalThis.localStorage !== "undefined") {
      try {
        const saved = globalThis.localStorage.getItem(
          cacheStorageKey(config.clientSideId, initialContext),
        );
        if (saved) {
          this.flags = JSON.parse(saved) as Record<string, ClientEvaluatedFlag>;
          this.initialized = true;
          this.resolveReady();
        }
      } catch {
        // Ignore storage errors
      }
    } else if (config.bootstrap && typeof config.bootstrap === "object") {
      this.flags = { ...config.bootstrap };
      this.initialized = true;
      this.resolveReady();
    }

    void this.fetchEvaluations();

    if (config.streaming !== false) {
      this.connectPingStream();
    }

    if (config.eventsUrl) {
      this.flushTimer = setInterval(() => {
        void this.flush();
      }, config.flushIntervalMs ?? 5_000);
    }
  }

  isInitialized(): boolean {
    return this.initialized;
  }

  async waitForInitialization(timeoutMs = 5_000): Promise<boolean> {
    if (this.initialized) return true;
    await Promise.race([
      this.readyPromise,
      new Promise<void>((resolve) => setTimeout(resolve, timeoutMs)),
    ]);
    return this.initialized;
  }

  getContext(): ClientContext {
    return this.context;
  }

  async identify(nextContext: ClientContext): Promise<void> {
    this.context = nextContext;
    await this.fetchEvaluations(this.lastRev || undefined);
  }

  variation<T = unknown>(flagKey: string, defaultValue: T): T {
    return this.variationDetail(flagKey, defaultValue).value;
  }

  variationDetail<T = unknown>(flagKey: string, defaultValue: T): BrowserEvaluationDetail<T> {
    const entry = this.flags[flagKey];
    const value = entry !== undefined ? (entry.value as T) : defaultValue;
    const variationIndex = entry?.variationIndex;
    this.recordEval(flagKey, entry?.version, variationIndex, value, defaultValue);
    return {
      value,
      variationIndex,
      reason: entry?.reason,
    };
  }

  allFlags(): Record<string, unknown> {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(this.flags)) {
      out[k] = v.value;
    }
    return out;
  }

  on(event: "change" | `change:${string}`, cb: ChangeCallback): () => void {
    let set = this.listeners.get(event);
    if (!set) {
      set = new Set();
      this.listeners.set(event, set);
    }
    set.add(cb);
    return () => set?.delete(cb);
  }

  private emitDiff(prev: Record<string, ClientEvaluatedFlag>, next: Record<string, ClientEvaluatedFlag>) {
    const allKeys = new Set([...Object.keys(prev), ...Object.keys(next)]);
    for (const key of allKeys) {
      const oldVal = prev[key]?.value;
      const newVal = next[key]?.value;
      if (JSON.stringify(oldVal) !== JSON.stringify(newVal)) {
        this.listeners.get("change")?.forEach((fn) => fn(newVal, oldVal, key));
        this.listeners.get(`change:${key}`)?.forEach((fn) => fn(newVal, oldVal, key));
      }
    }
  }

  private async fetchEvaluations(rev?: number): Promise<void> {
    const fetchFn = this.cfg.fetchImpl ?? globalThis.fetch;
    if (!fetchFn) return;
    const base = this.cfg.baseUrl.replace(/\/$/, "");
    const cid = encodeURIComponent(this.cfg.clientSideId);
    const params = new URLSearchParams();
    if (rev !== undefined) params.set("rev", String(rev));
    if (this.cfg.withReasons) params.set("withReasons", "true");
    const qs = params.toString() ? `?${params.toString()}` : "";

    try {
      let res: Response;
      if (this.cfg.usePostForEval) {
        res = await fetchFn(`${base}/sdk/v1/evalx/${cid}/context${qs}`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify(this.context),
        });
      } else {
        const encoded = base64UrlEncodeUtf8(JSON.stringify(this.context));
        res = await fetchFn(`${base}/sdk/v1/evalx/${cid}/contexts/${encoded}${qs}`);
      }
      if (!res.ok) return;
      const next = (await res.json()) as Record<string, ClientEvaluatedFlag>;
      const prev = this.flags;
      this.flags = next;
      this.initialized = true;
      this.resolveReady();

      if (this.cfg.bootstrap === "localStorage" && typeof globalThis.localStorage !== "undefined") {
        try {
          globalThis.localStorage.setItem(
            cacheStorageKey(this.cfg.clientSideId, this.context),
            JSON.stringify(next),
          );
        } catch {
          // Ignore quota errors
        }
      }
      this.emitDiff(prev, next);
    } catch {
      // Resilient fallback: keep cached flags
    }
  }

  private connectPingStream(): void {
    const base = this.cfg.baseUrl.replace(/\/$/, "");
    const cid = encodeURIComponent(this.cfg.clientSideId);
    const url = `${base}/stream/v1/eval/${cid}`;

    const factory =
      this.cfg.eventSourceFactory ??
      (typeof globalThis.EventSource !== "undefined"
        ? (u: string) => new globalThis.EventSource(u)
        : undefined);
    if (!factory) return;

    this.eventSource = factory(url);
    this.eventSource.addEventListener("ping", (ev) => {
      try {
        const parsed = JSON.parse(ev.data) as { rev?: number };
        if (typeof parsed.rev === "number" && parsed.rev > this.lastRev) {
          this.lastRev = parsed.rev;
          void this.fetchEvaluations(parsed.rev);
        }
      } catch {
        // Ignore malformed ping
      }
    });
  }

  private recordEval(
    flagKey: string,
    version: number | undefined,
    variation: number | undefined,
    value: unknown,
    defaultVal: unknown,
  ) {
    const key = `${flagKey}|${version ?? "u"}|${variation ?? "d"}`;
    const cur = this.counters.get(key);
    if (cur) {
      cur.count += 1;
    } else {
      this.counters.set(key, { flagKey, version, variation, value, defaultVal, count: 1 });
    }
  }

  async flush(): Promise<number> {
    if (this.counters.size === 0 || !this.cfg.eventsUrl) return 0;
    const fetchFn = this.cfg.fetchImpl ?? globalThis.fetch;
    if (!fetchFn) return 0;

    const endDate = Date.now();
    const startDate = this.windowStartMs;
    this.windowStartMs = endDate;

    const features: Record<
      string,
      { default: unknown; contextKinds: string[]; counters: unknown[] }
    > = {};
    const ctxKind = typeof this.context.kind === "string" ? this.context.kind : "user";
    for (const c of this.counters.values()) {
      let bucket = features[c.flagKey];
      if (!bucket) {
        bucket = { default: c.defaultVal, contextKinds: [ctxKind], counters: [] };
        features[c.flagKey] = bucket;
      }
      bucket.counters.push({
        variation: c.variation,
        version: c.version,
        value: c.value,
        unknown: c.version === undefined,
        count: c.count,
      });
    }
    const count = this.counters.size;
    this.counters.clear();

    const base = this.cfg.eventsUrl.replace(/\/$/, "");
    const cid = encodeURIComponent(this.cfg.clientSideId);
    await fetchFn(`${base}/events/v1/client/${cid}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify([{ kind: "summary", startDate, endDate, features }]),
    });
    return count;
  }

  async close(): Promise<void> {
    this.eventSource?.close();
    if (this.flushTimer) clearInterval(this.flushTimer);
    await this.flush();
  }
}

/**
 * Official OpenFeature Web Provider wrapping [`SkotosBrowserClient`] (ADR-0004).
 */
export class SkotosOpenFeatureWebProvider {
  readonly metadata = { name: "Skotos Browser OpenFeature Provider" } as const;

  constructor(private readonly client: SkotosBrowserClient) {}

  async onContextChange(
    _oldContext: Record<string, unknown>,
    newContext: Record<string, unknown>,
  ): Promise<void> {
    const kind = typeof newContext.kind === "string" ? newContext.kind : "user";
    const key =
      typeof newContext.targetingKey === "string"
        ? newContext.targetingKey
        : typeof newContext.key === "string"
          ? newContext.key
          : "anonymous";
    await this.client.identify({ ...newContext, kind, key });
  }

  resolveBooleanEvaluation(flagKey: string, defaultValue: boolean) {
    const d = this.client.variationDetail(flagKey, defaultValue);
    return {
      value: typeof d.value === "boolean" ? d.value : defaultValue,
      variant: d.variationIndex !== undefined ? String(d.variationIndex) : undefined,
      reason: d.reason?.kind ?? "DEFAULT",
    };
  }

  resolveStringEvaluation(flagKey: string, defaultValue: string) {
    const d = this.client.variationDetail(flagKey, defaultValue);
    return {
      value: typeof d.value === "string" ? d.value : defaultValue,
      variant: d.variationIndex !== undefined ? String(d.variationIndex) : undefined,
      reason: d.reason?.kind ?? "DEFAULT",
    };
  }

  resolveNumberEvaluation(flagKey: string, defaultValue: number) {
    const d = this.client.variationDetail(flagKey, defaultValue);
    return {
      value: typeof d.value === "number" ? d.value : defaultValue,
      variant: d.variationIndex !== undefined ? String(d.variationIndex) : undefined,
      reason: d.reason?.kind ?? "DEFAULT",
    };
  }

  resolveObjectEvaluation<T = Record<string, unknown>>(flagKey: string, defaultValue: T) {
    const d = this.client.variationDetail(flagKey, defaultValue);
    return {
      value: d.value,
      variant: d.variationIndex !== undefined ? String(d.variationIndex) : undefined,
      reason: d.reason?.kind ?? "DEFAULT",
    };
  }
}
