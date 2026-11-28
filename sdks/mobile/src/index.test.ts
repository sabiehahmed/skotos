import { describe, expect, it, vi } from "vitest";
import {
  InMemoryMobileStorage,
  SkotosMobileClient,
  SkotosOpenFeatureMobileProvider,
  type MobileEvaluatedFlag,
} from "./index";

describe("@skotos/mobile-sdk offline-first client & OpenFeature provider", () => {
  it("hydrates from persistent storage offline, refreshes online, and flushes events", async () => {
    const storage = new InMemoryMobileStorage();
    const initialFlags: Record<string, MobileEvaluatedFlag> = {
      "dark-mode": {
        value: true,
        variationIndex: 0,
        version: 4,
        flagVersion: 4,
        reason: { kind: "TARGET_MATCH" },
      },
    };
    const postedEvents: unknown[] = [];
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (url.includes("/sdk/v1/evalx/mob-test/context")) {
        return new Response(JSON.stringify(initialFlags), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.includes("/events/v1/bulk")) {
        postedEvents.push(JSON.parse(String(init?.body ?? "[]")));
        return new Response(null, { status: 202 });
      }
      return new Response(null, { status: 404 });
    });

    const client = new SkotosMobileClient(
      {
        mobileKey: "mob-test",
        baseUrl: "https://edge.skotos.local",
        eventsUrl: "https://events.skotos.local",
        storage,
        withReasons: true,
        fetchImpl: fetchMock as unknown as typeof fetch,
      },
      { kind: "user", key: "ios-user-1", osName: "iOS", appVersion: "2.4.0" },
    );

    const ready = await client.init();
    expect(ready).toBe(true);
    expect(client.isLoadedFromCache()).toBe(false);
    expect(client.boolVariation("dark-mode", false)).toBe(true);
    client.track("checkout_completed", 49.99);
    const flushed = await client.flush();
    expect(flushed).toBe(2);
    expect(postedEvents).toHaveLength(1);

    // Cold-start offline client hydrates immediately from persistent mobile storage
    const offlineClient = new SkotosMobileClient(
      {
        mobileKey: "mob-test",
        baseUrl: "https://edge.skotos.local",
        storage,
        offline: true,
      },
      { kind: "user", key: "ios-user-1" },
    );
    await offlineClient.init();
    expect(offlineClient.isInitialized()).toBe(true);
    expect(offlineClient.isLoadedFromCache()).toBe(true);
    expect(offlineClient.boolVariation("dark-mode", false)).toBe(true);

    const provider = new SkotosOpenFeatureMobileProvider(offlineClient);
    expect(provider.resolveBooleanEvaluation("dark-mode", false)).toEqual({
      value: true,
      variant: "0",
      reason: "TARGET_MATCH",
    });
  });
});
