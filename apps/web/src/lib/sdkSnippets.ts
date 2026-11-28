/*
 * Code generation for the SDK setup page.
 *
 * Every snippet here is derived from the real public API of the SDKs that live
 * under `sdks/` in this repository. When an SDK's constructor or variation
 * methods change, update the matching generator below.
 *
 * Credentials model (never put a secret where it does not belong):
 *   - server SDKs authenticate with the environment's secret SDK key (`sk_sdk_...`).
 *     The control API only ever returns its prefix, so snippets read it from the
 *     `SKOTOS_SDK_KEY` environment variable instead of inlining it.
 *   - the mobile SDK uses the mobile key, also only known by prefix.
 *   - browser / React SDKs use the public client-side ID, which is safe to inline.
 */

export type SdkId =
  | "node"
  | "go"
  | "python"
  | "java"
  | "rust"
  | "edge"
  | "browser"
  | "react"
  | "mobile";

export type SdkSide = "server" | "client" | "mobile";

export type InstallMode = "local" | "registry";

export type FlagValueKind = "boolean" | "string" | "number" | "json";

export interface SdkDefinition {
  id: SdkId;
  label: string;
  language: string;
  side: SdkSide;
  /** Package / module name as declared in the SDK's manifest. */
  packageName: string;
  /** Directory of the SDK inside the Skotos repository. */
  repoDir: string;
  /** File name for the init snippet header. */
  fileName: string;
  /** True when no package registry install exists yet (local build only). */
  localOnly?: boolean;
}

/** Version of the SDK packages in `sdks/*` (all SDKs are versioned together). */
export const SDK_PACKAGE_VERSION = "0.1.0";

/** Directory (relative to the repo root) that local JS tarballs are packed into. */
export const LOCAL_PACK_DIR = ".sdk-packs";

export const SDKS: SdkDefinition[] = [
  {
    id: "node",
    label: "Node.js",
    language: "TypeScript",
    side: "server",
    packageName: "@skotos/node-server-sdk",
    repoDir: "sdks/node-server",
    fileName: "skotos.ts",
  },
  {
    id: "go",
    label: "Go",
    language: "Go",
    side: "server",
    packageName: "github.com/sabiehahmed/skotos/sdks/go-server",
    repoDir: "sdks/go-server",
    fileName: "main.go",
  },
  {
    id: "python",
    label: "Python",
    language: "Python",
    side: "server",
    packageName: "skotos-server-sdk",
    repoDir: "sdks/python-server",
    fileName: "app.py",
  },
  {
    id: "java",
    label: "Java",
    language: "Java",
    side: "server",
    packageName: "dev.skotos.sdk",
    repoDir: "sdks/java-server",
    fileName: "App.java",
    localOnly: true,
  },
  {
    id: "rust",
    label: "Rust",
    language: "Rust",
    side: "server",
    packageName: "skotos-server-sdk",
    repoDir: "sdks/rust-server",
    fileName: "main.rs",
  },
  {
    id: "edge",
    label: "Edge workers",
    language: "TypeScript",
    side: "server",
    packageName: "@skotos/edge-sdk",
    repoDir: "sdks/edge",
    fileName: "worker.ts",
  },
  {
    id: "browser",
    label: "Browser",
    language: "TypeScript",
    side: "client",
    packageName: "@skotos/browser-sdk",
    repoDir: "sdks/browser",
    fileName: "skotos.ts",
  },
  {
    id: "react",
    label: "React",
    language: "TSX",
    side: "client",
    packageName: "@skotos/react-sdk",
    repoDir: "sdks/react",
    fileName: "main.tsx",
  },
  {
    id: "mobile",
    label: "React Native",
    language: "TypeScript",
    side: "mobile",
    packageName: "@skotos/mobile-sdk",
    repoDir: "sdks/mobile",
    fileName: "skotos.ts",
  },
];

export function getSdk(id: string): SdkDefinition {
  return SDKS.find((s) => s.id === id) ?? SDKS[0]!;
}

export interface SnippetInput {
  sdk: SdkDefinition;
  /** Absolute path of the Skotos checkout on the developer's machine (local installs). */
  repoPath: string;
  deliveryUrl: string;
  eventsUrl: string;
  clientSideId: string;
  /** Plaintext secret, only known right after a rotation in this browser session. */
  secretKey?: string;
  /** Prefix of the active secret (e.g. `sk_sdk_1a2b`) used as a hint when the plaintext is unknown. */
  secretKeyPrefix?: string;
  flagKey: string;
  flagKind: FlagValueKind;
  /** Sample fallback value for string / number flags. */
  sampleValue?: unknown;
  contextKind: string;
  contextKey: string;
}

export interface SetupSnippets {
  install: string;
  installLabel: string;
  /** `.env` style lines, or undefined when the SDK needs no secret. */
  env?: string;
  init: string;
}

function trimSlash(s: string): string {
  return s.replace(/\/+$/, "");
}

function jsPackFile(packageName: string): string {
  // `pnpm pack` turns `@scope/name` into `scope-name-<version>.tgz`.
  return `${packageName.replace(/^@/, "").replace("/", "-")}-${SDK_PACKAGE_VERSION}.tgz`;
}

function shellPath(p: string): string {
  // Keep `~` expandable, quote everything else that needs it.
  return /[\s'"$]/.test(p) ? `"${p.replace(/"/g, '\\"')}"` : p;
}

function jsLocalInstall(repo: string, sdks: SdkDefinition[]): string {
  const r = shellPath(repo);
  const filters = sdks.map((s) => `--filter ${s.packageName}`).join(" ");
  const packs = sdks
    .map((s) => `(cd ${s.repoDir} && pnpm pack --pack-destination ../../${LOCAL_PACK_DIR})`)
    .join("\n");
  const tarballs = sdks.map((s) => `${r}/${LOCAL_PACK_DIR}/${jsPackFile(s.packageName)}`).join(" \\\n  ");
  return [
    "# 1. Build and pack the SDK in your Skotos checkout",
    `cd ${r}`,
    `pnpm install && pnpm ${filters} build`,
    packs,
    "",
    "# 2. Install the tarball in your app",
    "cd /path/to/your-app",
    `npm install ${tarballs}`,
  ].join("\n");
}

export function secretEnvName(side: SdkSide): string | undefined {
  if (side === "server") return "SKOTOS_SDK_KEY";
  if (side === "mobile") return "SKOTOS_MOBILE_KEY";
  return undefined;
}

function secretValue(input: SnippetInput): string {
  if (input.secretKey) return input.secretKey;
  const prefix = input.secretKeyPrefix ? `${input.secretKeyPrefix}...` : "";
  const kind = input.sdk.side === "mobile" ? "mobile key" : "SDK key";
  return prefix ? `<your ${kind}, starts with ${prefix}>` : `<your ${kind}>`;
}

export function buildInstall(input: SnippetInput, mode: InstallMode): { label: string; code: string } {
  const { sdk } = input;
  const repo = input.repoPath.trim() || "~/skotos";
  const r = shellPath(repo);
  const effective: InstallMode = sdk.localOnly ? "local" : mode;

  if (effective === "registry") {
    switch (sdk.id) {
      case "node":
      case "edge":
      case "browser":
      case "mobile":
        return { label: "Terminal", code: `npm install ${sdk.packageName}` };
      case "react":
        return { label: "Terminal", code: `npm install @skotos/react-sdk @skotos/browser-sdk` };
      case "go":
        return { label: "Terminal", code: `go get ${sdk.packageName}` };
      case "python":
        return { label: "Terminal", code: `pip install ${sdk.packageName}` };
      case "rust":
        return {
          label: "Terminal",
          code: `cargo add skotos-server-sdk --git https://github.com/sabiehahmed/skotos\ncargo add tokio --features full`,
        };
      case "java":
        break;
    }
  }

  switch (sdk.id) {
    case "node":
    case "edge":
    case "browser":
    case "mobile":
      return { label: "Terminal · local build", code: jsLocalInstall(repo, [sdk]) };
    case "react":
      return {
        label: "Terminal · local build",
        code: jsLocalInstall(repo, [getSdk("browser"), sdk]),
      };
    case "go":
      return {
        label: "Terminal · local build",
        code: [
          "# Point your module at the SDK in your Skotos checkout",
          `go mod edit -require=${sdk.packageName}@v0.0.0 \\`,
          `  -replace=${sdk.packageName}=${r}/${sdk.repoDir}`,
          "go mod tidy",
        ].join("\n"),
      };
    case "python":
      return {
        label: "Terminal · local build",
        code: [
          "# Editable install: SDK changes are picked up without reinstalling",
          `pip install -e ${r}/${sdk.repoDir}`,
        ].join("\n"),
      };
    case "rust":
      return {
        label: "Terminal · local build",
        code: [
          "# Path dependency on the SDK in your Skotos checkout",
          `cargo add skotos-server-sdk --path ${r}/${sdk.repoDir}`,
          "cargo add tokio --features full",
        ].join("\n"),
      };
    case "java":
      return {
        label: "Terminal · local build",
        code: [
          "# Compile the SDK (pure JDK 21, no dependencies) into a jar",
          `cd ${r}`,
          `javac --release 21 -d build/skotos-java ${sdk.repoDir}/src/main/java/dev/skotos/sdk/*.java`,
          "jar cf build/skotos-java-sdk.jar -C build/skotos-java .",
          "",
          "# Add the jar to your app's classpath, e.g.",
          `javac -cp ${r}/build/skotos-java-sdk.jar App.java`,
          `java -cp ${r}/build/skotos-java-sdk.jar:. App`,
        ].join("\n"),
      };
  }
}

function jsString(s: string): string {
  return JSON.stringify(s);
}

function fallback(input: SnippetInput, lang: "js" | "go" | "py" | "java" | "rust"): string {
  const v = input.sampleValue;
  switch (input.flagKind) {
    case "boolean":
      return lang === "py" ? "False" : "false";
    case "string": {
      const s = typeof v === "string" ? v : "default";
      return lang === "rust" ? JSON.stringify(s) : JSON.stringify(s);
    }
    case "number": {
      const n = typeof v === "number" ? v : 0;
      if (lang === "go" || lang === "rust" || lang === "java") {
        return Number.isInteger(n) ? `${n}.0` : `${n}`;
      }
      return `${n}`;
    }
    case "json":
      if (lang === "py") return "{}";
      if (lang === "go") return "map[string]any{}";
      if (lang === "rust") return "serde_json::json!({})";
      return "{}";
  }
}

function nodeInit(i: SnippetInput): string {
  const method =
    i.flagKind === "boolean"
      ? "boolVariation"
      : i.flagKind === "string"
        ? "stringVariation"
        : i.flagKind === "number"
          ? "numberVariation"
          : "jsonVariation";
  return `import { SkotosNodeClient } from "@skotos/node-server-sdk";

const client = new SkotosNodeClient({
  sdkKey: process.env.SKOTOS_SDK_KEY!,
  streamUrl: ${jsString(trimSlash(i.deliveryUrl))},
  eventsUrl: ${jsString(trimSlash(i.eventsUrl))},
});

await client.waitForInitialization(5_000);

const context = { kind: ${jsString(i.contextKind)}, key: ${jsString(i.contextKey)} };
const value = client.${method}(${jsString(i.flagKey)}, context, ${fallback(i, "js")});
console.log(${jsString(`${i.flagKey} =`)}, value);

// Flush evaluation events before the process exits.
await client.close();
`;
}

function edgeInit(i: SnippetInput): string {
  const method =
    i.flagKind === "boolean"
      ? "boolVariation"
      : i.flagKind === "string"
        ? "stringVariation"
        : i.flagKind === "number"
          ? "numberVariation"
          : null;
  const evalLine = method
    ? `const value = client.${method}(${jsString(i.flagKey)}, context, ${fallback(i, "js")});`
    : `const value = client.allFlagsState(context)[${jsString(i.flagKey)}] ?? {};`;
  return `import { HttpSnapshotProvider, SkotosEdgeClient } from "@skotos/edge-sdk";

export default {
  async fetch(request: Request, env: { SKOTOS_SDK_KEY: string }): Promise<Response> {
    const provider = new HttpSnapshotProvider({
      baseUrl: ${jsString(trimSlash(i.deliveryUrl))},
      sdkKey: env.SKOTOS_SDK_KEY,
    });
    const client = await SkotosEdgeClient.init(provider, {
      sdkKey: env.SKOTOS_SDK_KEY,
      eventsUrl: ${jsString(trimSlash(i.eventsUrl))},
    });

    const context = { kind: ${jsString(i.contextKind)}, key: ${jsString(i.contextKey)} };
    ${evalLine}
    return Response.json({ ${jsString(i.flagKey)}: value });
  },
};
`;
}

function goInit(i: SnippetInput): string {
  const call =
    i.flagKind === "boolean"
      ? `client.BoolVariation(${jsString(i.flagKey)}, ctx, false)`
      : i.flagKind === "string"
        ? `client.StringVariation(${jsString(i.flagKey)}, ctx, ${fallback(i, "go")})`
        : i.flagKind === "number"
          ? `client.Float64Variation(${jsString(i.flagKey)}, ctx, ${fallback(i, "go")})`
          : `client.JSONVariationDetail(${jsString(i.flagKey)}, ctx, ${fallback(i, "go")}).Value`;
  return `package main

import (
	"log"
	"os"
	"time"

	skotos "github.com/sabiehahmed/skotos/sdks/go-server"
)

func main() {
	client := skotos.NewClient(skotos.Config{
		SDKKey:    os.Getenv("SKOTOS_SDK_KEY"),
		StreamURL: ${jsString(trimSlash(i.deliveryUrl))},
		EventsURL: ${jsString(trimSlash(i.eventsUrl))},
	})
	defer client.Close()

	if !client.WaitForInitialization(5 * time.Second) {
		log.Println("skotos: not initialized yet, serving fallback values")
	}

	ctx := skotos.NewContext(${jsString(i.contextKind)}, ${jsString(i.contextKey)})
	value := ${call}
	log.Printf("${i.flagKey} = %v", value)
}
`;
}

function pyInit(i: SnippetInput): string {
  const method =
    i.flagKind === "boolean"
      ? "bool_variation"
      : i.flagKind === "string"
        ? "string_variation"
        : i.flagKind === "number"
          ? "number_variation"
          : "json_variation";
  return `import os

from skotos_sdk import SkotosClient, SkotosConfig

client = SkotosClient(
    SkotosConfig(
        sdk_key=os.environ["SKOTOS_SDK_KEY"],
        base_url=${jsString(trimSlash(i.deliveryUrl))},
        events_url=${jsString(trimSlash(i.eventsUrl))},
    )
)

context = {"kind": ${jsString(i.contextKind)}, "key": ${jsString(i.contextKey)}}
value = client.${method}(${jsString(i.flagKey)}, context, ${fallback(i, "py")})
print(${jsString(`${i.flagKey} =`)}, value)

# Send evaluation events to Skotos.
client.flush()
`;
}

function javaInit(i: SnippetInput): string {
  const call =
    i.flagKind === "boolean"
      ? `boolean value = client.boolVariation(${jsString(i.flagKey)}, context, false);`
      : i.flagKind === "number"
        ? `double value = client.numberVariation(${jsString(i.flagKey)}, context, ${fallback(i, "java")});`
        : i.flagKind === "string"
          ? `String value = client.stringVariation(${jsString(i.flagKey)}, context, ${fallback(i, "java")});`
          : `// JSON flags are not exposed by the Java SDK yet; read a string variation instead.\n        String value = client.stringVariation(${jsString(i.flagKey)}, context, "{}");`;
  return `import dev.skotos.sdk.SkotosClient;
import java.util.Map;

public class App {
    public static void main(String[] args) throws Exception {
        SkotosClient client = new SkotosClient(new SkotosClient.Config(
            System.getenv("SKOTOS_SDK_KEY"),
            ${jsString(trimSlash(i.deliveryUrl))},
            ${jsString(trimSlash(i.eventsUrl))},
            false, // offline
            true   // sendEvents
        ));

        Map<String, Object> context = Map.of("kind", ${jsString(i.contextKind)}, "key", ${jsString(i.contextKey)});
        ${call}
        System.out.println(${jsString(`${i.flagKey} = `)} + value);

        // Send evaluation events to Skotos.
        client.flush();
    }
}
`;
}

function rustInit(i: SnippetInput): string {
  const call =
    i.flagKind === "boolean"
      ? `client.bool_variation(${jsString(i.flagKey)}, &context, false)`
      : i.flagKind === "string"
        ? `client.string_variation(${jsString(i.flagKey)}, &context, ${fallback(i, "rust")})`
        : i.flagKind === "number"
          ? `client.f64_variation(${jsString(i.flagKey)}, &context, ${fallback(i, "rust")})`
          : `client.json_variation(${jsString(i.flagKey)}, &context, ${fallback(i, "rust")})`;
  return `use std::time::Duration;

use skotos_server_sdk::{Context, SingleContext, SkotosClient, SkotosConfig};

#[tokio::main]
async fn main() {
    let sdk_key = std::env::var("SKOTOS_SDK_KEY").expect("SKOTOS_SDK_KEY is not set");
    let config = SkotosConfig::new(sdk_key, ${jsString(trimSlash(i.deliveryUrl))})
        .with_events_url(${jsString(trimSlash(i.eventsUrl))});
    let client = SkotosClient::new(config);

    if !client.wait_for_initialization(Duration::from_secs(5)).await {
        eprintln!("skotos: not initialized yet, serving fallback values");
    }

    let context = Context::single(SingleContext::new(${jsString(i.contextKind)}, ${jsString(i.contextKey)}));
    let value = ${call};
    println!("${i.flagKey} = {value:?}");
}
`;
}

function browserInit(i: SnippetInput): string {
  return `import { SkotosBrowserClient } from "@skotos/browser-sdk";

// The client-side ID is public and safe to ship in browser bundles.
const client = new SkotosBrowserClient(
  {
    clientSideId: ${jsString(i.clientSideId)},
    baseUrl: ${jsString(trimSlash(i.deliveryUrl))},
    eventsUrl: ${jsString(trimSlash(i.eventsUrl))},
  },
  { kind: ${jsString(i.contextKind)}, key: ${jsString(i.contextKey)} },
);

await client.waitForInitialization(5_000);

const value = client.variation(${jsString(i.flagKey)}, ${fallback(i, "js")});
console.log(${jsString(`${i.flagKey} =`)}, value);

// Re-render when the flag changes.
client.on(${jsString(`change:${i.flagKey}`)}, () => {
  console.log(${jsString(`${i.flagKey} changed to`)}, client.variation(${jsString(i.flagKey)}, ${fallback(i, "js")}));
});
`;
}

function reactInit(i: SnippetInput): string {
  return `import { SkotosBrowserClient } from "@skotos/browser-sdk";
import { SkotosProvider, useFlag } from "@skotos/react-sdk";
import { createRoot } from "react-dom/client";

// The client-side ID is public and safe to ship in browser bundles.
const client = new SkotosBrowserClient(
  {
    clientSideId: ${jsString(i.clientSideId)},
    baseUrl: ${jsString(trimSlash(i.deliveryUrl))},
    eventsUrl: ${jsString(trimSlash(i.eventsUrl))},
  },
  { kind: ${jsString(i.contextKind)}, key: ${jsString(i.contextKey)} },
);

function Feature() {
  // Re-renders automatically when the flag changes.
  const value = useFlag(${jsString(i.flagKey)}, ${fallback(i, "js")});
  return <pre>${i.flagKey} = {JSON.stringify(value)}</pre>;
}

await client.waitForInitialization(5_000);

createRoot(document.getElementById("root")!).render(
  <SkotosProvider client={client}>
    <Feature />
  </SkotosProvider>,
);
`;
}

function mobileInit(i: SnippetInput): string {
  const method =
    i.flagKind === "boolean"
      ? "boolVariation"
      : i.flagKind === "string"
        ? "stringVariation"
        : i.flagKind === "number"
          ? "numberVariation"
          : "variation";
  return `import { SkotosMobileClient } from "@skotos/mobile-sdk";

// Load the mobile key from your app config (e.g. react-native-config); do not commit it.
const client = new SkotosMobileClient(
  {
    mobileKey: Config.SKOTOS_MOBILE_KEY,
    baseUrl: ${jsString(trimSlash(i.deliveryUrl))},
    eventsUrl: ${jsString(trimSlash(i.eventsUrl))},
  },
  { kind: ${jsString(i.contextKind)}, key: ${jsString(i.contextKey)} },
);

await client.init();

const value = client.${method}(${jsString(i.flagKey)}, ${fallback(i, "js")});
console.log(${jsString(`${i.flagKey} =`)}, value);

// Send evaluation events to Skotos.
await client.flush();
`;
}

export function buildInit(input: SnippetInput): string {
  switch (input.sdk.id) {
    case "node":
      return nodeInit(input);
    case "edge":
      return edgeInit(input);
    case "go":
      return goInit(input);
    case "python":
      return pyInit(input);
    case "java":
      return javaInit(input);
    case "rust":
      return rustInit(input);
    case "browser":
      return browserInit(input);
    case "react":
      return reactInit(input);
    case "mobile":
      return mobileInit(input);
  }
}

export function buildEnv(input: SnippetInput): string | undefined {
  const name = secretEnvName(input.sdk.side);
  if (!name) return undefined;
  return `${name}=${secretValue(input)}`;
}

export function buildSnippets(input: SnippetInput, mode: InstallMode): SetupSnippets {
  const install = buildInstall(input, mode);
  return {
    install: install.code,
    installLabel: install.label,
    env: buildEnv(input),
    init: buildInit(input),
  };
}

/** Default public URLs for this deployment's delivery edge and event ingest services. */
export function defaultServiceUrls(loc?: { protocol: string; hostname: string }): {
  deliveryUrl: string;
  eventsUrl: string;
} {
  const env = (import.meta as { env?: Record<string, string | undefined> }).env ?? {};
  const protocol = loc?.protocol ?? "http:";
  const host = loc?.hostname || "localhost";
  return {
    deliveryUrl: env.VITE_SKOTOS_DELIVERY_URL || `${protocol}//${host}:8081`,
    eventsUrl: env.VITE_SKOTOS_EVENTS_URL || `${protocol}//${host}:8082`,
  };
}
