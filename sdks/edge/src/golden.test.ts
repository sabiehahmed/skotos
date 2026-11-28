import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  bigSegmentKeyHash,
  boolVariation,
  evaluate,
  KvSnapshotProvider,
  numberVariation,
  redactContext,
  SkotosEdgeClient,
  SkotosOpenFeatureEdgeProvider,
  stringVariation,
  type EdgeKvStore,
  type EnvironmentSnapshot,
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

describe("@skotos/edge-sdk canonical golden evaluation suite & edge runtime features", () => {
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

  it("passes 100% of all 124 canonical golden vectors with pure Web-Standard SHA-1", () => {
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

    expect(total).toBeGreaterThanOrEqual(124);
  });

  it("evaluates Big Segments and loads snapshots from Cloudflare/Vercel KV + OpenFeature", async () => {
    const includedHash = bigSegmentKeyHash("user", "vip-edge-1");
    const excludedHash = bigSegmentKeyHash("user", "blocked-edge-2");

    const envSnapshot: EnvironmentSnapshot = {
      revision: 42,
      flags: {
        "vip-checkout": {
          key: "vip-checkout",
          version: 3,
          on: true,
          salt: "salt-edge",
          variations: [true, false],
          offVariation: 1,
          rules: [
            {
              id: "r-bigseg",
              clauses: [
                {
                  contextKind: "user",
                  attribute: "key",
                  op: "segmentMatch",
                  values: ["million-vips"],
                },
              ],
              variation: 0,
            },
          ],
          fallthrough: { variation: 1 },
        },
      },
      segments: {
        "million-vips": {
          key: "million-vips",
          unbounded: true,
          unboundedContextKind: "user",
          generation: 1,
        },
      },
    };

    const kvData = new Map<string, string>([
      ["skotos:env:prod", JSON.stringify(envSnapshot)],
      [
        "skotos:bigseg:million-vips",
        JSON.stringify({
          segmentKey: "million-vips",
          contextKind: "user",
          generation: 1,
          includedHashes: [includedHash.toString()],
          excludedHashes: [excludedHash.toString()],
        }),
      ],
    ]);

    const kv: EdgeKvStore = {
      get: async (k) => kvData.get(k) ?? null,
    };

    const client = await SkotosEdgeClient.init(
      new KvSnapshotProvider({
        kv,
        snapshotKey: "skotos:env:prod",
        bigSegmentPrefix: "skotos:bigseg:",
      }),
    );

    expect(client.isInitialized()).toBe(true);
    expect(client.getRevision()).toBe(42);
    expect(client.boolVariation("vip-checkout", { kind: "user", key: "vip-edge-1" }, false)).toBe(
      true,
    );
    expect(
      client.boolVariation("vip-checkout", { kind: "user", key: "blocked-edge-2" }, false),
    ).toBe(false);
    expect(client.boolVariation("vip-checkout", { kind: "user", key: "nobody" }, false)).toBe(
      false,
    );

    const provider = new SkotosOpenFeatureEdgeProvider(client);
    const res = await provider.resolveBooleanEvaluation("vip-checkout", false, {
      targetingKey: "vip-edge-1",
    });
    expect(res).toEqual({ value: true, variant: "0", reason: "TARGETING_MATCH" });
  });
});
