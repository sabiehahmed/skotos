import { describe, expect, it } from "vitest";
import { SkotosShellStub, UI_LIBRARY_NAME } from "./index";

describe("@skotos/ui stub", () => {
  it("exports library metadata and stub component", () => {
    expect(UI_LIBRARY_NAME).toBe("@skotos/ui");
    expect(typeof SkotosShellStub).toBe("function");
  });
});
