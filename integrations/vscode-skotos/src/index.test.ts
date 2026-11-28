import { describe, expect, it, vi } from "vitest";
import {
  buildCodeLenses,
  detectFlagReferences,
  fetchProjectFlags,
  formatFlagHoverMarkdown,
  type FlagSummaryItem,
} from "./index";

describe("@skotos/vscode-extension flag detection, hover, and CodeLens", () => {
  it("detects flag calls across TypeScript, Python, and Go snippets and formats Hover/CodeLens", () => {
    const code = [
      `const enabled = client.boolVariation("new-checkout", ctx, false);`,
      `stage = py_client.string_variation("db-migration", ctx, "off")`,
      `if useFlag("ai-assistant", false) { render(); }`,
    ].join("\n");

    const matches = detectFlagReferences(code);
    expect(matches.map((m) => [m.flagKey, m.line])).toEqual([
      ["new-checkout", 0],
      ["db-migration", 1],
      ["ai-assistant", 2],
    ]);

    const flags = new Map<string, FlagSummaryItem>([
      [
        "new-checkout",
        {
          key: "new-checkout",
          name: "New Checkout",
          description: "Streamlined one-click checkout",
          kind: "boolean",
          tags: ["checkout", "experiment"],
          envOn: true,
        },
      ],
    ]);

    const hover = formatFlagHoverMarkdown(flags.get("new-checkout")!, {
      projectKey: "default",
      envKey: "production",
      dashboardBaseUrl: "http://localhost:5173",
    });
    expect(hover).toContain("`new-checkout`");
    expect(hover).toContain("ON (production)");
    expect(hover).toContain("http://localhost:5173/projects/default/flags/new-checkout");

    const lenses = buildCodeLenses(matches, flags, {
      projectKey: "default",
      envKey: "production",
    });
    expect(lenses[0]).toEqual({
      flagKey: "new-checkout",
      line: 0,
      title: "Skotos [production]: new-checkout (ON)",
      command: "skotos.openFlagInDashboard",
      arguments: ["default", "new-checkout"],
    });
  });

  it("fetches project flags from Control Plane API", async () => {
    const fetchMock = vi.fn(async () =>
      new Response(
        JSON.stringify({
          items: [{ key: "dark-mode", name: "Dark Mode", kind: "boolean" }],
        }),
        { status: 200, headers: { "Content-Type": "application/json" } },
      ),
    );

    const map = await fetchProjectFlags(
      { apiBaseUrl: "http://localhost:8080", projectKey: "default", envKey: "prod", apiToken: "sk_1" },
      fetchMock as unknown as typeof fetch,
    );
    expect(map.get("dark-mode")?.name).toBe("Dark Mode");
  });
});
