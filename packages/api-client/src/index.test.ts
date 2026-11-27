import { describe, expect, it } from "vitest";
import { API_CLIENT_NAME, API_CLIENT_VERSION, createStubClientConfig } from "./index";

describe("@skotos/api-client stub", () => {
  it("exports metadata and normalizes trailing slashes in baseUrl", () => {
    expect(API_CLIENT_NAME).toBe("@skotos/api-client");
    expect(API_CLIENT_VERSION).toBe("0.0.1");
    expect(createStubClientConfig("http://localhost:8080///")).toEqual({
      baseUrl: "http://localhost:8080",
      credentials: "include",
    });
    expect(createStubClientConfig()).toEqual({
      baseUrl: "",
      credentials: "include",
    });
  });
});
