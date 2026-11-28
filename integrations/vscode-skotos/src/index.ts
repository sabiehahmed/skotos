/**
 * Skotos VS Code / Cursor IDE Extension core module.
 *
 * Detects feature flag keys inside source documents, fetches flag metadata from
 * the Skotos Control Plane API, formats rich Markdown hover cards, and builds
 * CodeLens actions for instant inspection and dashboard navigation.
 */

export interface SkotosExtensionConfig {
  apiBaseUrl: string;
  projectKey: string;
  envKey: string;
  apiToken?: string;
  dashboardBaseUrl?: string;
}

export interface FlagSummaryItem {
  key: string;
  name: string;
  description?: string;
  kind: string;
  temporary?: boolean;
  archived?: boolean;
  tags?: string[];
  envOn?: boolean;
}

export interface FlagReferenceMatch {
  flagKey: string;
  line: number;
  startCol: number;
  endCol: number;
}

export interface CodeLensDescriptor {
  flagKey: string;
  line: number;
  title: string;
  command: string;
  arguments: unknown[];
}

const FLAG_CALL_PATTERN =
  /(?:boolVariation|stringVariation|numberVariation|jsonVariation|variation|bool_variation|string_variation|number_variation|json_variation|BoolVariation|StringVariation|NumberVariation|useFlag|resolveBooleanEvaluation|resolveStringEvaluation|resolveNumberEvaluation)\s*\(\s*["'`]([a-zA-Z0-9][a-zA-Z0-9._-]{0,127})["'`]/g;

/**
 * Scans source text for SDK flag evaluation calls or known flag keys.
 */
export function detectFlagReferences(
  sourceText: string,
  knownKeys?: ReadonlySet<string>,
): FlagReferenceMatch[] {
  const lines = sourceText.split(/\r?\n/);
  const matches: FlagReferenceMatch[] = [];

  for (let lineIdx = 0; lineIdx < lines.length; lineIdx++) {
    const line = lines[lineIdx] ?? "";
    const seenCols = new Set<number>();

    FLAG_CALL_PATTERN.lastIndex = 0;
    let m: RegExpExecArray | null;
    while ((m = FLAG_CALL_PATTERN.exec(line)) !== null) {
      const flagKey = m[1];
      if (!flagKey) continue;
      if (knownKeys && knownKeys.size > 0 && !knownKeys.has(flagKey)) continue;
      const startCol = m.index + m[0].indexOf(flagKey);
      seenCols.add(startCol);
      matches.push({
        flagKey,
        line: lineIdx,
        startCol,
        endCol: startCol + flagKey.length,
      });
    }

    if (knownKeys && knownKeys.size > 0) {
      for (const key of knownKeys) {
        let searchFrom = 0;
        while (searchFrom < line.length) {
          const idx = line.indexOf(`"${key}"`, searchFrom);
          const singleIdx = line.indexOf(`'${key}'`, searchFrom);
          const found =
            idx >= 0 && singleIdx >= 0 ? Math.min(idx, singleIdx) : Math.max(idx, singleIdx);
          if (found < 0) break;
          const startCol = found + 1;
          if (!seenCols.has(startCol)) {
            seenCols.add(startCol);
            matches.push({
              flagKey: key,
              line: lineIdx,
              startCol,
              endCol: startCol + key.length,
            });
          }
          searchFrom = found + key.length + 2;
        }
      }
    }
  }

  return matches;
}

/**
 * Formats a Markdown hover card for a Skotos feature flag in VS Code / Cursor.
 */
export function formatFlagHoverMarkdown(
  flag: FlagSummaryItem,
  cfg: Pick<SkotosExtensionConfig, "projectKey" | "envKey" | "dashboardBaseUrl">,
): string {
  const status = flag.archived
    ? "Archived"
    : flag.envOn === true
      ? `ON (${cfg.envKey})`
      : flag.envOn === false
        ? `OFF (${cfg.envKey})`
        : "Configured";
  const lines = [
    `### Skotos Flag: \`${flag.key}\``,
    `**${flag.name}** · \`${flag.kind}\` · Status: **${status}**`,
  ];
  if (flag.description) {
    lines.push("", flag.description);
  }
  if (flag.tags && flag.tags.length > 0) {
    lines.push("", `Tags: ${flag.tags.map((t) => `\`${t}\``).join(" ")}`);
  }
  if (cfg.dashboardBaseUrl) {
    const base = cfg.dashboardBaseUrl.replace(/\/+$/, "");
    lines.push(
      "",
      `[Open in Skotos Dashboard](${base}/projects/${encodeURIComponent(cfg.projectKey)}/flags/${encodeURIComponent(flag.key)})`,
    );
  }
  return lines.join("\n");
}

/**
 * Generates CodeLens descriptors for every detected flag reference in a file.
 */
export function buildCodeLenses(
  matches: readonly FlagReferenceMatch[],
  flagsByKey: ReadonlyMap<string, FlagSummaryItem>,
  cfg: Pick<SkotosExtensionConfig, "projectKey" | "envKey">,
): CodeLensDescriptor[] {
  return matches.map((m) => {
    const flag = flagsByKey.get(m.flagKey);
    const stateLabel =
      flag?.envOn === true ? "ON" : flag?.envOn === false ? "OFF" : flag ? flag.kind : "unknown";
    return {
      flagKey: m.flagKey,
      line: m.line,
      title: `Skotos [${cfg.envKey}]: ${m.flagKey} (${stateLabel})`,
      command: "skotos.openFlagInDashboard",
      arguments: [cfg.projectKey, m.flagKey],
    };
  });
}

/**
 * Fetches all flags in the configured Skotos project from the Control Plane REST API.
 */
export async function fetchProjectFlags(
  cfg: SkotosExtensionConfig,
  fetchImpl: typeof fetch = globalThis.fetch,
): Promise<Map<string, FlagSummaryItem>> {
  const base = cfg.apiBaseUrl.replace(/\/+$/, "");
  const headers: Record<string, string> = { Accept: "application/json" };
  if (cfg.apiToken) {
    headers.Authorization = `Bearer ${cfg.apiToken}`;
  }
  const resp = await fetchImpl(
    `${base}/api/v1/projects/${encodeURIComponent(cfg.projectKey)}/flags`,
    { headers },
  );
  if (!resp.ok) {
    throw new Error(`Failed to fetch Skotos flags: HTTP ${resp.status}`);
  }
  const body = (await resp.json()) as { items?: FlagSummaryItem[] };
  const map = new Map<string, FlagSummaryItem>();
  for (const item of body.items ?? []) {
    map.set(item.key, item);
  }
  return map;
}
