import { describe, expect, it } from "vitest";
import { SDKS, buildSnippets, getSdk, type SnippetInput } from "./sdkSnippets";

function input(overrides: Partial<SnippetInput> = {}): SnippetInput {
  return {
    sdk: getSdk("node"),
    repoPath: "~/code/skotos",
    deliveryUrl: "http://localhost:8081/",
    eventsUrl: "http://localhost:8082",
    clientSideId: "sk_cid_dev123",
    secretKeyPrefix: "sdk-ab12",
    flagKey: "express-checkout",
    flagKind: "boolean",
    contextKind: "user",
    contextKey: "user-123",
    ...overrides,
  };
}

describe("SDK setup snippets", () => {
  it("covers every SDK shipped in sdks/", () => {
    expect(SDKS.map((s) => s.repoDir).sort()).toEqual(
      [
        "sdks/browser",
        "sdks/edge",
        "sdks/go-server",
        "sdks/java-server",
        "sdks/mobile",
        "sdks/node-server",
        "sdks/python-server",
        "sdks/react",
        "sdks/rust-server",
      ].sort(),
    );
  });

  it("never inlines a server secret: it reads SKOTOS_SDK_KEY and hints the prefix", () => {
    for (const sdk of SDKS.filter((s) => s.side === "server")) {
      const out = buildSnippets(input({ sdk }), "local");
      expect(out.init).toContain("SKOTOS_SDK_KEY");
      expect(out.init).not.toContain("sk_cid_dev123");
      expect(out.env).toBe("SKOTOS_SDK_KEY=<your SDK key, starts with sdk-ab12...>");
    }
  });

  it("puts a freshly generated secret only into the .env line", () => {
    const out = buildSnippets(input({ secretKey: "sdk-ab12-full-secret" }), "local");
    expect(out.env).toBe("SKOTOS_SDK_KEY=sdk-ab12-full-secret");
    expect(out.init).not.toContain("sdk-ab12-full-secret");
  });

  it("inlines the public client-side ID for browser SDKs and needs no secret", () => {
    for (const id of ["browser", "react"]) {
      const out = buildSnippets(input({ sdk: getSdk(id) }), "local");
      expect(out.init).toContain('clientSideId: "sk_cid_dev123"');
      expect(out.env).toBeUndefined();
    }
  });

  it("builds local tarball installs with pnpm pack (react pulls in browser-sdk)", () => {
    const out = buildSnippets(input({ sdk: getSdk("react") }), "local");
    expect(out.install).toContain("cd ~/code/skotos");
    expect(out.install).toContain("(cd sdks/browser && pnpm pack --pack-destination ../../.sdk-packs)");
    expect(out.install).toContain("(cd sdks/react && pnpm pack --pack-destination ../../.sdk-packs)");
    expect(out.install).toContain("~/code/skotos/.sdk-packs/skotos-browser-sdk-0.1.0.tgz");
    expect(out.install).toContain("~/code/skotos/.sdk-packs/skotos-react-sdk-0.1.0.tgz");
  });

  it("uses language-native local installs for Go, Python and Rust", () => {
    expect(buildSnippets(input({ sdk: getSdk("go") }), "local").install).toContain(
      "-replace=github.com/sabiehahmed/skotos/sdks/go-server=~/code/skotos/sdks/go-server",
    );
    expect(buildSnippets(input({ sdk: getSdk("python") }), "local").install).toContain(
      "pip install -e ~/code/skotos/sdks/python-server",
    );
    expect(buildSnippets(input({ sdk: getSdk("rust") }), "local").install).toContain(
      "cargo add skotos-server-sdk --path ~/code/skotos/sdks/rust-server",
    );
  });

  it("forces a local build for Java, which has no published artifact", () => {
    const out = buildSnippets(input({ sdk: getSdk("java") }), "registry");
    expect(out.install).toContain("javac --release 21");
  });

  it("uses the typed variation method for the flag kind and trims trailing slashes", () => {
    const str = buildSnippets(
      input({ flagKind: "string", sampleValue: "blue", sdk: getSdk("python") }),
      "registry",
    );
    expect(str.init).toContain('client.string_variation("express-checkout", context, "blue")');
    expect(str.init).toContain('base_url="http://localhost:8081"');
    expect(str.install).toBe("pip install skotos-server-sdk");

    const num = buildSnippets(input({ flagKind: "number", sampleValue: 3, sdk: getSdk("go") }), "local");
    expect(num.init).toContain('client.Float64Variation("express-checkout", ctx, 3.0)');
  });
});
