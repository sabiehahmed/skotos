import { describe, expect, it, vi } from "vitest";
import {
  SkotosBrowserClient,
  SkotosOpenFeatureWebProvider,
  type EventSourceLike,
} from "./index";

describe("@skotos/browser-sdk", () => {
  it("hydrates via evalx, reacts to Ping-to-Re-evaluate SSE pings, flushes summaries, and works with OpenFeature", async () => {
    let rev = 1;
    let pingListener: ((ev: { data: string }) => void) | undefined;
    const postedEvents: unknown[] = [];

    const mockFetch: typeof fetch = vi.fn(async (input, init) => {
      const url = String(input);
      if (url.includes("/events/v1/client/")) {
        postedEvents.push(JSON.parse(String(init?.body ?? "[]")));
        return new Response(JSON.stringify({ accepted: true }), { status: 202 });
      }
      return new Response(
        JSON.stringify({
          "new-ui": {
            value: rev >= 2,
            variationIndex: rev >= 2 ? 0 : 1,
            version: rev,
            flagVersion: rev,
            reason: { kind: "FALLTHROUGH" },
          },
        }),
        { status: 200, headers: { "content-type": "application/json" } },
      );
    });

    const mockES: EventSourceLike = {
      addEventListener: (type, cb) => {
        if (type === "ping") pingListener = cb;
      },
      close: vi.fn(),
    };

    const client = new SkotosBrowserClient(
      {
        clientSideId: "sk_cid_demo",
        baseUrl: "http://edge.local",
        eventsUrl: "http://events.local",
        fetchImpl: mockFetch,
        eventSourceFactory: () => mockES,
      },
      { kind: "user", key: "u1" },
    );

    expect(await client.waitForInitialization(2_000)).toBe(true);
    expect(client.variation("new-ui", false)).toBe(false);

    const changes: Array<[unknown, unknown]> = [];
    client.on("change:new-ui", (next, prev) => changes.push([next, prev]));

    // Simulate Ping-to-Re-evaluate SSE ping rev=2 from delivery-edge
    rev = 2;
    pingListener?.({ data: JSON.stringify({ rev: 2 }) });
    await vi.waitFor(() => {
      expect(client.variation("new-ui", false)).toBe(true);
    });
    expect(changes).toEqual([[true, false]]);

    const provider = new SkotosOpenFeatureWebProvider(client);
    expect(provider.resolveBooleanEvaluation("new-ui", false).value).toBe(true);

    await client.flush();
    expect(postedEvents).toHaveLength(1);
    await client.close();
  });
});
