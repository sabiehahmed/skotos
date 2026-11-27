import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  boolVariation,
  evaluate,
  numberVariation,
  redactContext,
  SkotosNodeClient,
  SkotosOpenFeatureNodeProvider,
  stringVariation,
  TestData,
  type EvalFlag,
  type EvalSegment,
  type EvalStore,
  type RedactionConfig,
  type SkotosContext,
} from "./index";

interface GoldenSuite {
  suite: string;
  kind?: string;
  flags?: Record<string, EvalFlag>;
  segments?: Record<string, EvalSegment>;
  tests: Array<{
    name: string;
    flagKey?: string;
    getter?: "bool" | "string" | "number" | "json";
    defaultValue?: unknown;
    context: SkotosContext;
    flags?: Record<string, EvalFlag>;
    segments?: Record<string, EvalSegment>;
    redactionConfig?: RedactionConfig;
    expected: unknown;
  }>;
}

const here = path.dirname(fileURLToPath(import.meta.url));
const specDir = path.resolve(here, "../../../spec/evaluation-tests");

describe("@skotos/node-server-sdk canonical golden evaluation suite", () => {
  const suites = [
    "basic.json",
    "operators.json",
    "rollouts.json",
    "multi-context.json",
    "prerequisites.json",
    "segments.json",
    "errors.json",
    "redaction.json",
  ];

  it("passes 100% of all 124 canonical golden vectors", () => {
    let total = 0;

    for (const file of suites) {
      const suite = JSON.parse(
        fs.readFileSync(path.join(specDir, file), "utf8"),
      ) as GoldenSuite;

      for (const tc of suite.tests) {
        total++;
        if (suite.kind === "redaction") {
          const actual = redactContext(tc.context, tc.redactionConfig ?? {});
          expect(actual, `[${suite.suite}] ${tc.name}`).toEqual(tc.expected);
          continue;
        }

        const flags: Record<string, EvalFlag> = {
          ...(suite.flags ?? {}),
          ...(tc.flags ?? {}),
        };
        const segments: Record<string, EvalSegment> = {
          ...(suite.segments ?? {}),
          ...(tc.segments ?? {}),
        };
        const store: EvalStore = {
          getFlag: (k) => flags[k],
          getSegment: (k) => segments[k],
        };

        const flagKey = tc.flagKey ?? "";
        let actual: unknown;
        if (tc.getter === "bool") {
          actual = boolVariation(store, flagKey, tc.context, Boolean(tc.defaultValue));
        } else if (tc.getter === "string") {
          actual = stringVariation(store, flagKey, tc.context, String(tc.defaultValue ?? ""));
        } else if (tc.getter === "number") {
          actual = numberVariation(store, flagKey, tc.context, Number(tc.defaultValue ?? 0));
        } else {
          actual = evaluate(store, flagKey, tc.context, tc.defaultValue);
        }

        expect(JSON.parse(JSON.stringify(actual)), `[${suite.suite}] ${tc.name}`).toEqual(
          tc.expected,
        );
      }
    }

    expect(total).toBeGreaterThanOrEqual(120);
  });

  it("supports offline TestData and OpenFeature Node provider", async () => {
    const td = new TestData().setBoolFlag("ai-copilot", true);
    const client = SkotosNodeClient.withTestData(td);
    expect(client.isInitialized()).toBe(true);
    expect(client.boolVariation("ai-copilot", { kind: "user", key: "u1" }, false)).toBe(true);

    const provider = new SkotosOpenFeatureNodeProvider(client);
    const res = await provider.resolveBooleanEvaluation("ai-copilot", false, {
      targetingKey: "u1",
    });
    expect(res).toEqual({ value: true, variant: "0", reason: "DEFAULT" });

    const missing = await provider.resolveBooleanEvaluation("nope", false, {
      targetingKey: "u1",
    });
    expect(missing.errorCode).toBe("FLAG_NOT_FOUND");
    await client.close();
  });
});
