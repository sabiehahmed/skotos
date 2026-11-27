import { describe, expect, it } from "vitest";
import { getAppMetadata } from "./App";

describe("@skotos/web stub", () => {
  it("links @skotos/ui and @skotos/api-client workspace packages", () => {
    expect(getAppMetadata()).toEqual({
      appName: "@skotos/web",
      uiLibrary: "@skotos/ui",
      apiClient: "@skotos/api-client@0.0.1",
    });
  });
});
