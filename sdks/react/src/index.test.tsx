import { SkotosBrowserClient, type EventSourceLike } from "@skotos/browser-sdk";
import { act, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SkotosProvider, useFlag } from "./index";

function BannerProbe() {
  const enabled = useFlag("promo-banner", false);
  return <div data-testid="probe">{enabled ? "ON" : "OFF"}</div>;
}

describe("@skotos/react-sdk", () => {
  it("reactively re-renders components when a live SSE ping updates a flag", async () => {
    let enabled = false;
    let pingListener: ((ev: { data: string }) => void) | undefined;

    const mockFetch: typeof fetch = vi.fn(async () => {
      return new Response(
        JSON.stringify({
          "promo-banner": {
            value: enabled,
            variationIndex: enabled ? 0 : 1,
            version: enabled ? 2 : 1,
            flagVersion: enabled ? 2 : 1,
          },
        }),
        { status: 200 },
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
        clientSideId: "sk_cid_react",
        baseUrl: "http://edge.local",
        fetchImpl: mockFetch,
        eventSourceFactory: () => mockES,
      },
      { kind: "user", key: "u1" },
    );
    await client.waitForInitialization(2_000);

    render(
      <SkotosProvider client={client}>
        <BannerProbe />
      </SkotosProvider>,
    );
    expect(screen.getByTestId("probe").textContent).toBe("OFF");

    enabled = true;
    await act(async () => {
      pingListener?.({ data: JSON.stringify({ rev: 2 }) });
      await new Promise((r) => setTimeout(r, 20));
    });

    expect(screen.getByTestId("probe").textContent).toBe("ON");
    await client.close();
  });
});
