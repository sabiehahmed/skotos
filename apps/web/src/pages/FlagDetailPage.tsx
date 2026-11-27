import {
  Alert,
  AlertDialog,
  AuditTimeline,
  Badge,
  Breadcrumb,
  Button,
  Card,
  Checkbox,
  CodeBlock,
  Dialog,
  DiffViewer,
  FormField,
  Heading,
  Icons,
  Input,
  JsonEditor,
  KeyValueEditor,
  PageHeader,
  RolloutAllocator,
  RuleBuilder,
  Select,
  Separator,
  Skeleton,
  SplitPane,
  Stack,
  Switch,
  Tabs,
  TagInput,
  Text,
  Textarea,
  ToggleGroup,
  useToast,
  type DiffInstructionSummary,
  type KeyValuePair,
  type RolloutValue,
  type RuleBuilderRule,
} from "@skotos/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import {
  canAdmin,
  canWrite,
  formatErrorMessage,
  skotosApi,
  SkotosApiError,
  unwrapApiResult,
  type BuiltInRole,
  type ClauseOp,
  type Environment,
  type FlagEnvConfig,
  type FlagPatchInstruction,
  type FlagResponse,
  type SimulateFlagResponse,
} from "../lib/api";

export interface FlagDetailPageProps {
  projectKey: string;
  flagKey: string;
  activeEnv?: Environment;
  userRole: BuiltInRole;
  onBack: () => void;
}

function coerceClauseValue(raw: string): unknown {
  if (raw === "true") return true;
  if (raw === "false") return false;
  if (raw.trim() !== "" && !Number.isNaN(Number(raw)) && /^-?\d+(\.\d+)?$/.test(raw.trim())) {
    return Number(raw);
  }
  return raw;
}

function envConfigToRules(cfg?: FlagEnvConfig): RuleBuilderRule[] {
  if (!cfg) return [];
  return (cfg.config.rules ?? []).map((r) => ({
    id: r.id,
    description: r.description ?? undefined,
    clauses: (r.clauses ?? []).map((c) => ({
      contextKind: c.contextKind ?? "user",
      attribute: c.attribute,
      op: c.op,
      values: (c.values ?? []).map((v) => (typeof v === "string" ? v : JSON.stringify(v))),
      negate: c.negate ?? false,
    })),
    variationId: r.variationId ?? undefined,
    rollout: r.rollout
      ? {
          contextKind: r.rollout.contextKind ?? "user",
          bucketBy: r.rollout.bucketBy ?? undefined,
          variations: r.rollout.variations.map((v) => ({
            variationId: v.variationId,
            weight: v.weight,
          })),
        }
      : undefined,
  }));
}

export function FlagDetailPage({
  projectKey,
  flagKey,
  activeEnv,
  userRole,
  onBack,
}: FlagDetailPageProps) {
  const qc = useQueryClient();
  const { toast } = useToast();
  const writerAllowed = canWrite(userRole);
  const adminAllowed = canAdmin(userRole);

  const flagQuery = useQuery({
    queryKey: ["flag", projectKey, flagKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/flags/{flagKey}", {
          params: { path: { projectKey, flagKey } },
        }),
      ),
  });

  const envKey = activeEnv?.key ?? "";
  const envConfig = flagQuery.data?.environments[envKey];

  const revisionsQuery = useQuery({
    queryKey: ["flag-revisions", projectKey, flagKey, envKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET(
          "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/revisions",
          { params: { path: { projectKey, flagKey, envKey }, query: { limit: 50 } } },
        ),
      ),
    enabled: Boolean(envKey),
  });

  // Draft targeting state
  const [draftOn, setDraftOn] = useState(false);
  const [draftOffVarId, setDraftOffVarId] = useState<string>("");
  const [draftFallthroughMode, setDraftFallthroughMode] = useState<"variation" | "rollout">(
    "variation",
  );
  const [draftFallthroughVarId, setDraftFallthroughVarId] = useState<string>("");
  const [draftFallthroughRollout, setDraftFallthroughRollout] = useState<RolloutValue | null>(
    null,
  );
  const [draftTargets, setDraftTargets] = useState<Record<string, string[]>>({});
  const [draftRules, setDraftRules] = useState<RuleBuilderRule[]>([]);

  // Settings draft
  const [editName, setEditName] = useState("");
  const [editDesc, setEditDesc] = useState("");
  const [editTags, setEditTags] = useState<string[]>([]);
  const [editTemp, setEditTemp] = useState(true);
  const [editCsaEnv, setEditCsaEnv] = useState(false);
  const [editCsaMobile, setEditCsaMobile] = useState(false);

  // Add variation modal state
  const [newVarName, setNewVarName] = useState("");
  const [newVarValue, setNewVarValue] = useState("");

  // Save review modal & conflict handling
  const [reviewOpen, setReviewOpen] = useState(false);
  const [saveComment, setSaveComment] = useState("");
  const [saveError, setSaveError] = useState<string | null>(null);
  const [conflictVersion, setConflictVersion] = useState<number | null>(null);

  // Restore modal
  const [restoreVersion, setRestoreVersion] = useState<number | null>(null);
  const [restoreComment, setRestoreComment] = useState("");

  // Delete modal
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [deleteText, setDeleteText] = useState("");

  // Embedded Test Context Simulator state
  const [simMode, setSimMode] = useState<"form" | "json">("form");
  const [simKind, setSimKind] = useState("user");
  const [simKey, setSimKey] = useState("usr_demo_1");
  const [simAttrs, setSimAttrs] = useState<KeyValuePair[]>([
    { key: "email", value: "ada@skotos.dev" },
    { key: "plan", value: "enterprise" },
  ]);
  const [simJson, setSimJson] = useState(
    JSON.stringify(
      { kind: "user", key: "usr_demo_1", email: "ada@skotos.dev", plan: "enterprise" },
      null,
      2,
    ),
  );
  const [simUseDraft, setSimUseDraft] = useState(true);
  const [simResult, setSimResult] = useState<SimulateFlagResponse | null>(null);
  const [simError, setSimError] = useState<string | null>(null);

  // Sync draft state when flag or active environment loads/switches
  useEffect(() => {
    const flag = flagQuery.data;
    if (!flag) return;
    setEditName(flag.name);
    setEditDesc(flag.description ?? "");
    setEditTags(flag.tags ?? []);
    setEditTemp(flag.temporary ?? false);
    setEditCsaEnv(flag.clientSideAvailability?.usingEnvironmentId ?? false);
    setEditCsaMobile(flag.clientSideAvailability?.usingMobileKey ?? false);

    if (!envConfig) return;
    setDraftOn(envConfig.on);
    setDraftOffVarId(envConfig.config.offVariationId ?? flag.variations[1]?.id ?? "");
    if (envConfig.config.fallthrough.rollout) {
      setDraftFallthroughMode("rollout");
      setDraftFallthroughRollout({
        contextKind: envConfig.config.fallthrough.rollout.contextKind ?? "user",
        bucketBy: envConfig.config.fallthrough.rollout.bucketBy ?? undefined,
        variations: envConfig.config.fallthrough.rollout.variations.map((v) => ({
          variationId: v.variationId,
          weight: v.weight,
        })),
      });
      setDraftFallthroughVarId(flag.variations[0]?.id ?? "");
    } else {
      setDraftFallthroughMode("variation");
      setDraftFallthroughVarId(
        envConfig.config.fallthrough.variationId ?? flag.variations[0]?.id ?? "",
      );
      setDraftFallthroughRollout(null);
    }
    const targetMap: Record<string, string[]> = {};
    for (const t of envConfig.config.targets ?? []) {
      if ((t.contextKind ?? "user") === "user") {
        targetMap[t.variationId] = [...(t.values ?? [])];
      }
    }
    setDraftTargets(targetMap);
    setDraftRules(envConfigToRules(envConfig));
    setConflictVersion(null);
  }, [flagQuery.data, envConfig]);

  const variationOptions = useMemo(
    () =>
      (flagQuery.data?.variations ?? []).map((v, idx) => ({
        id: v.id,
        label: v.name ? `${v.name} (${JSON.stringify(v.value)})` : `Var ${idx}: ${JSON.stringify(v.value)}`,
      })),
    [flagQuery.data],
  );

  // Compile semantic FlagPatchInstruction[] diff from current envConfig -> draft state
  const { instructions: draftInstructions, summaries: draftSummaries } = useMemo<{
    instructions: FlagPatchInstruction[];
    summaries: DiffInstructionSummary[];
  }>(() => {
    if (!flagQuery.data || !envConfig) return { instructions: [], summaries: [] };
    const instrs: FlagPatchInstruction[] = [];
    const sums: DiffInstructionSummary[] = [];

    if (draftOn !== envConfig.on) {
      instrs.push({ kind: draftOn ? "turnFlagOn" : "turnFlagOff" });
      sums.push({
        kind: draftOn ? "turnFlagOn" : "turnFlagOff",
        summary: `Turn targeting ${draftOn ? "ON" : "OFF"}`,
      });
    }

    if ((draftOffVarId || null) !== (envConfig.config.offVariationId ?? null)) {
      instrs.push({ kind: "updateOffVariation", variationId: draftOffVarId || null });
      sums.push({ kind: "updateOffVariation", summary: "Update off variation" });
    }

    const existingTargets = envConfig.config.targets ?? [];
    const existingRules = envConfig.config.rules ?? [];

    // Compare individual user targets per variation
    for (const v of flagQuery.data.variations) {
      const prevVals =
        existingTargets.find((t) => (t.contextKind ?? "user") === "user" && t.variationId === v.id)
          ?.values ?? [];
      const nextVals = draftTargets[v.id] ?? [];
      const added = nextVals.filter((k) => !prevVals.includes(k));
      const removed = prevVals.filter((k) => !nextVals.includes(k));
      if (added.length > 0) {
        instrs.push({
          kind: "addTargets",
          contextKind: "user",
          variationId: v.id,
          values: added,
        });
        sums.push({
          kind: "addTargets",
          summary: `Add ${added.length} user target(s) to ${v.name ?? v.id}`,
        });
      }
      if (removed.length > 0) {
        instrs.push({
          kind: "removeTargets",
          contextKind: "user",
          variationId: v.id,
          values: removed,
        });
        sums.push({
          kind: "removeTargets",
          summary: `Remove ${removed.length} user target(s) from ${v.name ?? v.id}`,
        });
      }
    }

    // Compare targeting rules
    const existingRuleIds = new Set(existingRules.map((r) => r.id));
    const draftRuleIds = new Set(draftRules.map((r) => r.id));

    for (const oldRule of existingRules) {
      if (!draftRuleIds.has(oldRule.id)) {
        instrs.push({ kind: "removeRule", ruleId: oldRule.id });
        sums.push({ kind: "removeRule", summary: `Remove rule '${oldRule.description ?? oldRule.id}'` });
      }
    }

    const existingRuleMap = new Map(existingRules.map((r) => [r.id, r]));
    for (const rule of draftRules) {
      const apiClauses = rule.clauses.map((c) => ({
        contextKind: c.contextKind.trim() || "user",
        attribute: c.attribute.trim(),
        op: c.op as ClauseOp,
        values: c.values.map(coerceClauseValue),
        negate: c.negate,
      }));
      const vor = rule.rollout
        ? {
            rollout: {
              contextKind: rule.rollout.contextKind,
              bucketBy: rule.rollout.bucketBy ?? null,
              seed: null,
              kind: "rollout" as const,
              variations: rule.rollout.variations.map((v) => ({
                variationId: v.variationId,
                weight: v.weight,
                untracked: false,
              })),
            },
            variationId: null,
          }
        : {
            variationId: rule.variationId ?? flagQuery.data.variations[0]?.id ?? null,
            rollout: null,
          };

      if (!existingRuleIds.has(rule.id)) {
        instrs.push({
          kind: "addRule",
          id: rule.id,
          description: rule.description ?? null,
          clauses: apiClauses,
          trackEvents: false,
          ...vor,
        });
        sums.push({ kind: "addRule", summary: `Add targeting rule '${rule.description ?? rule.id}'` });
      } else {
        const prev = existingRuleMap.get(rule.id);
        const prevNormalized = prev ? envConfigToRules({ ...envConfig, config: { ...envConfig.config, rules: [prev] } })[0] : null;
        if (JSON.stringify(prevNormalized) !== JSON.stringify(rule)) {
          instrs.push({
            kind: "updateRule",
            ruleId: rule.id,
            description: rule.description ?? null,
            clauses: apiClauses,
            trackEvents: false,
            ...vor,
          });
          sums.push({ kind: "updateRule", summary: `Update targeting rule '${rule.description ?? rule.id}'` });
        }
      }
    }

    // Check rule order change if all remaining existing rules changed order
    const prevOrder = existingRules
      .map((r) => r.id)
      .filter((id) => draftRuleIds.has(id));
    const nextOrder = draftRules.map((r) => r.id);
    if (
      nextOrder.length > 1 &&
      nextOrder.length === prevOrder.length &&
      nextOrder.some((id, idx) => prevOrder[idx] !== id)
    ) {
      instrs.push({ kind: "reorderRules", ruleIds: nextOrder });
      sums.push({ kind: "reorderRules", summary: "Reorder targeting rules" });
    }

    // Compare fallthrough
    const prevIsRollout = Boolean(envConfig.config.fallthrough.rollout);
    if (draftFallthroughMode === "rollout" && draftFallthroughRollout) {
      const nextRollout = {
        contextKind: draftFallthroughRollout.contextKind,
        bucketBy: draftFallthroughRollout.bucketBy ?? null,
        seed: null,
        kind: "rollout" as const,
        variations: draftFallthroughRollout.variations.map((v) => ({
          variationId: v.variationId,
          weight: v.weight,
          untracked: false,
        })),
      };
      if (
        !prevIsRollout ||
        JSON.stringify(envConfig.config.fallthrough.rollout?.variations) !==
          JSON.stringify(nextRollout.variations)
      ) {
        instrs.push({
          kind: "updateFallthrough",
          rollout: nextRollout,
          variationId: null,
        });
        sums.push({ kind: "updateFallthrough", summary: "Update default fallthrough rollout" });
      }
    } else if (
      prevIsRollout ||
      draftFallthroughVarId !== (envConfig.config.fallthrough.variationId ?? "")
    ) {
      instrs.push({
        kind: "updateFallthrough",
        variationId: draftFallthroughVarId || null,
        rollout: null,
      });
      sums.push({ kind: "updateFallthrough", summary: "Update default fallthrough variation" });
    }

    return { instructions: instrs, summaries: sums };
  }, [
    flagQuery.data,
    envConfig,
    draftOn,
    draftOffVarId,
    draftTargets,
    draftRules,
    draftFallthroughMode,
    draftFallthroughVarId,
    draftFallthroughRollout,
  ]);

  const patchMutation = useMutation({
    mutationFn: async (args: { instructions: FlagPatchInstruction[]; comment?: string }) =>
      unwrapApiResult(
        await skotosApi.PATCH("/api/v1/projects/{projectKey}/flags/{flagKey}", {
          params: { path: { projectKey, flagKey } },
          body: {
            environmentKey: envKey,
            expectedVersion: envConfig?.version,
            comment: args.comment,
            instructions: args.instructions,
          },
        }),
      ),
    onSuccess: async () => {
      await Promise.all([
        qc.invalidateQueries({ queryKey: ["flag", projectKey, flagKey] }),
        qc.invalidateQueries({ queryKey: ["flags", projectKey] }),
        qc.invalidateQueries({ queryKey: ["flag-revisions", projectKey, flagKey, envKey] }),
      ]);
      setReviewOpen(false);
      setSaveComment("");
      setSaveError(null);
      setConflictVersion(null);
      toast({ title: "Flag changes saved", tone: "success" });
    },
    onError: (err) => {
      if (err instanceof SkotosApiError && err.status === 409 && err.currentVersion) {
        setConflictVersion(err.currentVersion);
      }
      setSaveError(formatErrorMessage(err));
    },
  });

  const simulateMutation = useMutation({
    mutationFn: async () => {
      let contextObj: Record<string, unknown>;
      if (simMode === "json") {
        contextObj = JSON.parse(simJson) as Record<string, unknown>;
      } else {
        contextObj = { kind: simKind.trim() || "user", key: simKey.trim() };
        for (const pair of simAttrs) {
          if (pair.key.trim()) {
            contextObj[pair.key.trim()] = coerceClauseValue(pair.value);
          }
        }
      }
      return unwrapApiResult(
        await skotosApi.POST(
          "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/simulate",
          {
            params: { path: { projectKey, flagKey, envKey } },
            body: {
              context: contextObj,
              draftInstructions:
                simUseDraft && draftInstructions.length > 0 ? draftInstructions : undefined,
            },
          },
        ),
      );
    },
    onSuccess: (res) => {
      setSimResult(res);
      setSimError(null);
    },
    onError: (err) => setSimError(formatErrorMessage(err)),
  });

  const restoreMutation = useMutation({
    mutationFn: async (args: { version: number; comment?: string }) =>
      unwrapApiResult(
        await skotosApi.POST(
          "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/restore/{version}",
          {
            params: { path: { projectKey, flagKey, envKey, version: args.version } },
            body: { comment: args.comment },
          },
        ),
      ),
    onSuccess: async (restored) => {
      await Promise.all([
        qc.invalidateQueries({ queryKey: ["flag", projectKey, flagKey] }),
        qc.invalidateQueries({ queryKey: ["flag-revisions", projectKey, flagKey, envKey] }),
      ]);
      setRestoreVersion(null);
      setRestoreComment("");
      toast({
        title: `Restored as version v${restored.version}`,
        tone: "success",
      });
    },
    onError: (err) => toast({ title: "Restore failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const deleteMutation = useMutation({
    mutationFn: async () =>
      unwrapApiResult(
        await skotosApi.DELETE("/api/v1/projects/{projectKey}/flags/{flagKey}", {
          params: { path: { projectKey, flagKey } },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["flags", projectKey] });
      toast({ title: `Deleted flag '${flagKey}'`, tone: "success" });
      onBack();
    },
    onError: (err) => toast({ title: "Delete failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  if (flagQuery.isPending) {
    return (
      <Stack gap={4}>
        <Skeleton height={48} />
        <Skeleton height={240} />
      </Stack>
    );
  }

  if (flagQuery.isError || !flagQuery.data) {
    return (
      <Alert tone="danger" title="Flag not found">
        {flagQuery.isError ? formatErrorMessage(flagQuery.error) : "Could not load flag."}
      </Alert>
    );
  }

  const flag: FlagResponse = flagQuery.data;

  const targetingTab = (
    <SplitPane
      primary={
        <Stack gap={4}>
          {conflictVersion && (
            <Alert tone="warning" title="Version conflict detected">
              <Stack direction="horizontal" align="center" justify="space-between" gap={3}>
                <span>
                  Another user updated this flag to v{conflictVersion}. Reload to fetch the latest
                  rules before saving.
                </span>
                <Button size="sm" variant="secondary" onClick={() => flagQuery.refetch()}>
                  Reload latest
                </Button>
              </Stack>
            </Alert>
          )}

          <Card
            header={
              <Stack direction="horizontal" align="center" justify="space-between" style={{ width: "100%" }}>
                <Stack direction="horizontal" align="center" gap={3}>
                  <Switch
                    checked={draftOn}
                    disabled={!writerAllowed || flag.archived}
                    onCheckedChange={setDraftOn}
                    label={draftOn ? "Targeting is ON" : "Targeting is OFF"}
                    data-testid="detail-targeting-switch"
                  />
                  <Badge tone={draftOn ? "success" : "neutral"}>
                    v{envConfig?.version ?? 1}
                  </Badge>
                </Stack>
                <Button
                  variant="primary"
                  disabled={!writerAllowed || draftInstructions.length === 0 || flag.archived}
                  onClick={() => {
                    setSaveError(null);
                    setReviewOpen(true);
                  }}
                >
                  Review &amp; Save ({draftInstructions.length})
                </Button>
              </Stack>
            }
          >
            <Stack gap={5}>
              {/* Individual User Targets */}
              <Stack gap={3}>
                <Heading level={4}>Individual Context Targets (kind: user)</Heading>
                {flag.variations.map((v, idx) => (
                  <FormField
                    key={v.id}
                    label={`Serve ${v.name ?? `Variation ${idx}`} (${JSON.stringify(v.value)}) to user keys`}
                  >
                    <TagInput
                      ariaLabel={`Target user keys for ${v.name ?? idx}`}
                      values={draftTargets[v.id] ?? []}
                      disabled={!writerAllowed || flag.archived}
                      placeholder="Add user key and press Enter..."
                      onChange={(vals) =>
                        setDraftTargets((prev) => ({ ...prev, [v.id]: vals }))
                      }
                    />
                  </FormField>
                ))}
              </Stack>

              <Separator />

              {/* Targeting Rules */}
              <Stack gap={3}>
                <Heading level={4}>Targeting Rules</Heading>
                <RuleBuilder
                  rules={draftRules}
                  variations={variationOptions}
                  disabled={!writerAllowed || flag.archived}
                  onChange={setDraftRules}
                />
              </Stack>

              <Separator />

              {/* Default Fallthrough */}
              <Stack gap={3}>
                <Heading level={4}>Default Rule (Fallthrough)</Heading>
                <Stack direction="horizontal" gap={3} align="center">
                  <Select
                    ariaLabel="Default rule mode"
                    value={draftFallthroughMode}
                    disabled={!writerAllowed || flag.archived}
                    options={[
                      { value: "variation", label: "Serve single variation" },
                      { value: "rollout", label: "Percentage rollout" },
                    ]}
                    onValueChange={(m) => {
                      const mode = m as "variation" | "rollout";
                      setDraftFallthroughMode(mode);
                      if (mode === "rollout" && !draftFallthroughRollout) {
                        const even = Math.floor(100_000 / Math.max(1, flag.variations.length));
                        const rem = 100_000 - even * flag.variations.length;
                        setDraftFallthroughRollout({
                          contextKind: "user",
                          variations: flag.variations.map((v, i) => ({
                            variationId: v.id,
                            weight: even + (i === 0 ? rem : 0),
                          })),
                        });
                      }
                    }}
                  />
                  {draftFallthroughMode === "variation" && (
                    <Select
                      ariaLabel="Fallthrough variation"
                      value={draftFallthroughVarId}
                      disabled={!writerAllowed || flag.archived}
                      options={variationOptions.map((o) => ({ value: o.id, label: o.label }))}
                      onValueChange={setDraftFallthroughVarId}
                    />
                  )}
                </Stack>
                {draftFallthroughMode === "rollout" && draftFallthroughRollout && (
                  <RolloutAllocator
                    value={draftFallthroughRollout}
                    variations={variationOptions}
                    disabled={!writerAllowed || flag.archived}
                    onChange={setDraftFallthroughRollout}
                  />
                )}
              </Stack>

              <Separator />

              {/* Off Variation */}
              <FormField label="When targeting is OFF, serve" htmlFor="off-var-select">
                <Select
                  id="off-var-select"
                  value={draftOffVarId}
                  disabled={!writerAllowed || flag.archived}
                  options={variationOptions.map((o) => ({ value: o.id, label: o.label }))}
                  onValueChange={setDraftOffVarId}
                />
              </FormField>
            </Stack>
          </Card>
        </Stack>
      }
      secondary={
        <Card
          header={
            <Stack direction="horizontal" align="center" justify="space-between" style={{ width: "100%" }}>
              <Stack direction="horizontal" align="center" gap={2}>
                <Icons.Play size={15} />
                <Heading level={4}>Test Context Simulator</Heading>
              </Stack>
              <ToggleGroup
                ariaLabel="Simulator input mode"
                value={simMode}
                onValueChange={(v) => setSimMode(v as "form" | "json")}
                options={[
                  { value: "form", label: "Form" },
                  { value: "json", label: "JSON" },
                ]}
              />
            </Stack>
          }
        >
          <Stack gap={4}>
            {simMode === "form" ? (
              <Stack gap={3}>
                <FormField label="Context Kind" htmlFor="sim-kind">
                  <Input
                    id="sim-kind"
                    mono
                    value={simKind}
                    onChange={(e) => setSimKind(e.target.value)}
                  />
                </FormField>
                <FormField label="Context Key" htmlFor="sim-key">
                  <Input
                    id="sim-key"
                    mono
                    value={simKey}
                    onChange={(e) => setSimKey(e.target.value)}
                  />
                </FormField>
                <FormField label="Attributes">
                  <KeyValueEditor pairs={simAttrs} onChange={setSimAttrs} />
                </FormField>
              </Stack>
            ) : (
              <JsonEditor value={simJson} onChange={setSimJson} ariaLabel="Simulator context JSON" />
            )}

            <Checkbox
              checked={simUseDraft}
              onCheckedChange={setSimUseDraft}
              label={`Evaluate unsaved draft (${draftInstructions.length} pending)`}
            />

            <Button
              variant="primary"
              disabled={!envKey || simulateMutation.isPending}
              onClick={() => simulateMutation.mutate()}
            >
              <Icons.Play size={14} />
              {simulateMutation.isPending ? "Evaluating..." : "Evaluate Context"}
            </Button>

            {simError && <Alert tone="danger">{simError}</Alert>}

            {simResult && (
              <Card padded>
                <Stack gap={2}>
                  <Stack direction="horizontal" align="center" justify="space-between">
                    <Text size="xs" tone="muted" bold>
                      EVALUATION RESULT
                    </Text>
                    <Badge tone={simResult.usedDraft ? "warning" : "accent"}>
                      {simResult.usedDraft ? "Draft" : "Saved"}
                    </Badge>
                  </Stack>
                  <CodeBlock
                    label={simResult.variationName ?? `Variation #${simResult.variationIndex ?? "default"}`}
                    code={JSON.stringify(simResult.value, null, 2)}
                  />
                  <Stack direction="horizontal" align="center" gap={2}>
                    <Text as="span" size="xs" tone="muted">
                      Reason:
                    </Text>
                    <Badge tone="success">{simResult.reason.kind}</Badge>
                  </Stack>
                </Stack>
              </Card>
            )}
          </Stack>
        </Card>
      }
    />
  );

  const variationsTab = (
    <Card padded>
      <Stack gap={4}>
        <Heading level={3}>Flag Variations ({flag.kind})</Heading>
        {flag.variations.map((v, idx) => (
          <Card key={v.id} padded>
            <Stack direction="horizontal" align="center" justify="space-between">
              <Stack gap={1}>
                <Text bold>
                  {v.name ?? `Variation ${idx}`}
                </Text>
                <Text size="xs" mono tone="muted">
                  Value: {JSON.stringify(v.value)} | ID: {v.id}
                </Text>
              </Stack>
              {writerAllowed && flag.kind !== "boolean" && flag.variations.length > 2 && (
                <Button
                  size="sm"
                  variant="danger"
                  onClick={() =>
                    patchMutation.mutate({
                      instructions: [{ kind: "removeVariation", variationId: v.id }],
                      comment: `Removed variation ${v.name ?? v.id}`,
                    })
                  }
                >
                  Remove
                </Button>
              )}
            </Stack>
          </Card>
        ))}

        {writerAllowed && flag.kind !== "boolean" && (
          <>
            <Separator />
            <Heading level={4}>Add Variation</Heading>
            <Stack direction="horizontal" gap={3} align="flex-end">
              <FormField label="Name" htmlFor="add-var-name">
                <Input
                  id="add-var-name"
                  value={newVarName}
                  onChange={(e) => setNewVarName(e.target.value)}
                />
              </FormField>
              <FormField label="Value" htmlFor="add-var-val">
                <Input
                  id="add-var-val"
                  mono
                  value={newVarValue}
                  onChange={(e) => setNewVarValue(e.target.value)}
                />
              </FormField>
              <Button
                variant="primary"
                disabled={!newVarValue.trim() || patchMutation.isPending}
                onClick={() => {
                  const parsedValue =
                    flag.kind === "number"
                      ? Number(newVarValue)
                      : flag.kind === "json"
                        ? JSON.parse(newVarValue)
                        : newVarValue;
                  patchMutation.mutate({
                    instructions: [
                      {
                        kind: "addVariation",
                        value: parsedValue,
                        name: newVarName.trim() || null,
                      },
                    ],
                    comment: `Added variation ${newVarName || newVarValue}`,
                  });
                  setNewVarName("");
                  setNewVarValue("");
                }}
              >
                Add variation
              </Button>
            </Stack>
          </>
        )}
      </Stack>
    </Card>
  );

  const settingsTab = (
    <Stack gap={4}>
      <Card padded>
        <Stack gap={4}>
          <Heading level={3}>Flag Settings</Heading>
          <FormField label="Name" htmlFor="edit-flag-name">
            <Input
              id="edit-flag-name"
              disabled={!writerAllowed}
              value={editName}
              onChange={(e) => setEditName(e.target.value)}
            />
          </FormField>
          <FormField label="Description" htmlFor="edit-flag-desc">
            <Textarea
              id="edit-flag-desc"
              disabled={!writerAllowed}
              value={editDesc}
              onChange={(e) => setEditDesc(e.target.value)}
            />
          </FormField>
          <FormField label="Tags">
            <TagInput values={editTags} onChange={setEditTags} disabled={!writerAllowed} />
          </FormField>
          <Switch
            checked={editTemp}
            disabled={!writerAllowed}
            onCheckedChange={setEditTemp}
            label="Temporary release flag"
          />
          <Switch
            checked={editCsaEnv}
            disabled={!writerAllowed}
            onCheckedChange={setEditCsaEnv}
            label="Available to Client-Side SDKs (using Client-Side ID)"
          />
          <Switch
            checked={editCsaMobile}
            disabled={!writerAllowed}
            onCheckedChange={setEditCsaMobile}
            label="Available to Mobile SDKs (using Mobile Key)"
          />
          {writerAllowed && (
            <Stack direction="horizontal" gap={2}>
              <Button
                variant="primary"
                disabled={patchMutation.isPending}
                onClick={() =>
                  patchMutation.mutate({
                    instructions: [
                      { kind: "updateName", name: editName },
                      { kind: "updateDescription", description: editDesc },
                      { kind: "updateTags", tags: editTags },
                      { kind: "updateTemporary", temporary: editTemp },
                      {
                        kind: "updateClientSideAvailability",
                        usingEnvironmentId: editCsaEnv,
                        usingMobileKey: editCsaMobile,
                      },
                    ],
                    comment: "Updated flag metadata & availability",
                  })
                }
              >
                Save settings
              </Button>
              <Button
                variant="secondary"
                onClick={() =>
                  patchMutation.mutate({
                    instructions: [{ kind: flag.archived ? "restoreFlag" : "archiveFlag" }],
                    comment: flag.archived ? "Restored flag" : "Archived flag",
                  })
                }
              >
                {flag.archived ? "Restore flag" : "Archive flag"}
              </Button>
            </Stack>
          )}
        </Stack>
      </Card>

      {adminAllowed && (
        <Card padded>
          <Stack direction="horizontal" align="center" justify="space-between">
            <Stack gap={1}>
              <Heading level={4}>Danger Zone</Heading>
              <Text size="sm" tone="muted">
                Permanently delete this flag and all environment rules.
              </Text>
            </Stack>
            <Button
              variant="danger"
              onClick={() => {
                setDeleteText("");
                setDeleteOpen(true);
              }}
            >
              Delete flag
            </Button>
          </Stack>
        </Card>
      )}
    </Stack>
  );

  const historyTab = (
    <AuditTimeline
      emptyTitle="No revision history yet"
      entries={(revisionsQuery.data?.items ?? []).map((rev) => ({
        id: rev.id,
        action: `v${rev.version}`,
        resourceType: rev.on ? "ON" : "OFF",
        resourceKey: flag.key,
        actor: rev.actorId ?? "system",
        timestamp: new Date(rev.createdAt).toLocaleString(),
        comment: rev.comment,
        after: rev.config,
        actionSlot:
          writerAllowed && envConfig && rev.version !== envConfig.version ? (
            <Button
              size="sm"
              variant="secondary"
              onClick={() => {
                setRestoreComment("");
                setRestoreVersion(rev.version);
              }}
            >
              <Icons.RotateCcw size={13} />
              Restore v{rev.version}
            </Button>
          ) : undefined,
      }))}
    />
  );

  return (
    <Stack gap={5}>
      <PageHeader
        breadcrumbs={
          <Breadcrumb
            items={[
              { label: projectKey, onClick: onBack },
              { label: "Flags", onClick: onBack },
              { label: flag.key },
            ]}
          />
        }
        title={
          <Stack direction="horizontal" align="center" gap={3}>
            <Heading level={1}>{flag.name}</Heading>
            <Badge tone="neutral">{flag.kind}</Badge>
            {flag.archived && <Badge tone="warning">Archived</Badge>}
          </Stack>
        }
        description={
          <Text size="sm" mono tone="muted">
            {flag.key}
          </Text>
        }
      />

      <Tabs
        items={[
          { value: "targeting", label: "Targeting", content: targetingTab },
          { value: "variations", label: "Variations", content: variationsTab },
          { value: "settings", label: "Settings", content: settingsTab },
          { value: "history", label: "History", content: historyTab },
        ]}
      />

      {/* Change Review Dialog */}
      <Dialog
        open={reviewOpen}
        onOpenChange={setReviewOpen}
        wide
        title={`Review changes in ${activeEnv?.name ?? envKey}`}
        description="Verify the semantic patch instructions before publishing."
        footer={
          <>
            <Button variant="secondary" onClick={() => setReviewOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={
                patchMutation.isPending ||
                Boolean(activeEnv?.requireComments && !saveComment.trim())
              }
              onClick={() =>
                patchMutation.mutate({
                  instructions: draftInstructions,
                  comment: saveComment.trim() || undefined,
                })
              }
            >
              {patchMutation.isPending ? "Saving..." : "Save changes"}
            </Button>
          </>
        }
      >
        <Stack gap={4}>
          {saveError && <Alert tone="danger">{saveError}</Alert>}
          <DiffViewer summaries={draftSummaries} />
          <FormField
            label="Change comment"
            htmlFor="save-patch-comment"
            required={activeEnv?.requireComments}
            hint={
              activeEnv?.requireComments
                ? "A change comment is required in this protected environment."
                : "Optional audit log explanation"
            }
          >
            <Textarea
              id="save-patch-comment"
              placeholder="Describe what changed and why..."
              value={saveComment}
              onChange={(e) => setSaveComment(e.target.value)}
            />
          </FormField>
        </Stack>
      </Dialog>

      {/* Restore Revision Dialog */}
      <Dialog
        open={restoreVersion !== null}
        onOpenChange={(open) => {
          if (!open) setRestoreVersion(null);
        }}
        title={`Restore version v${restoreVersion ?? ""}?`}
        description="Restoring writes a new monotonic version so complete audit history is preserved."
        footer={
          <>
            <Button variant="secondary" onClick={() => setRestoreVersion(null)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={
                restoreMutation.isPending ||
                Boolean(activeEnv?.requireComments && !restoreComment.trim())
              }
              onClick={() => {
                if (restoreVersion !== null) {
                  restoreMutation.mutate({
                    version: restoreVersion,
                    comment: restoreComment.trim() || undefined,
                  });
                }
              }}
            >
              Restore
            </Button>
          </>
        }
      >
        <FormField
          label="Restore comment"
          htmlFor="restore-rev-comment"
          required={activeEnv?.requireComments}
        >
          <Textarea
            id="restore-rev-comment"
            placeholder="Reason for rollback..."
            value={restoreComment}
            onChange={(e) => setRestoreComment(e.target.value)}
          />
        </FormField>
      </Dialog>

      {/* Delete Confirmation AlertDialog */}
      <AlertDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title={`Delete flag '${flag.key}'?`}
        description="This action permanently deletes the flag across all environments."
        confirmLabel="Delete flag"
        confirmDisabled={deleteMutation.isPending || deleteText.trim() !== flag.key}
        onConfirm={() => deleteMutation.mutate()}
      >
        <FormField label={`Type '${flag.key}' to confirm`} htmlFor="detail-delete-confirm">
          <Input
            id="detail-delete-confirm"
            mono
            value={deleteText}
            onChange={(e) => setDeleteText(e.target.value)}
          />
        </FormField>
      </AlertDialog>
    </Stack>
  );
}
