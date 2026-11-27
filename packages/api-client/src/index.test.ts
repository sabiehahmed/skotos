import { describe, expect, it } from "vitest";
import { createSkotosClient, SkotosApiError, unwrapApiResult } from "./index";

describe("@skotos/api-client", () => {
  it("creates a typed openapi-fetch client with include credentials by default", () => {
    const client = createSkotosClient({ baseUrl: "http://localhost:8080" });
    expect(typeof client.GET).toBe("function");
    expect(typeof client.POST).toBe("function");
    expect(typeof client.PATCH).toBe("function");
    expect(typeof client.DELETE).toBe("function");
  });

  it("wraps RFC 9457 ProblemDetails into SkotosApiError with currentVersion", () => {
    const err = new SkotosApiError(409, {
      code: "version_conflict",
      detail: "expected version 1, current is 2",
      currentVersion: 2,
    });
    expect(err).toBeInstanceOf(Error);
    expect(err.status).toBe(409);
    expect(err.code).toBe("version_conflict");
    expect(err.currentVersion).toBe(2);
  });

  it("unwrapApiResult returns data on 200 and throws SkotosApiError on error", () => {
    const okResp = new Response(null, { status: 200 });
    expect(unwrapApiResult({ data: { ok: true }, response: okResp })).toEqual({ ok: true });

    const errResp = new Response(null, { status: 403 });
    expect(() =>
      unwrapApiResult({
        error: { code: "forbidden", detail: "Access denied" },
        response: errResp,
      }),
    ).toThrow(SkotosApiError);
  });
});
