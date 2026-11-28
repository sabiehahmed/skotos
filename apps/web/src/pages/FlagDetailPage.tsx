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
  DataTable,
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
  type MigrationStage,
  type MigrationTemplate,
  type SimulateFlagResponse,
  type TriggerAction,
} from "../lib/api";

export interface FlagDetailPageProps {
  projectKey: string;
  flagKey: string;
  activeEnv?: Environment;
  environments?: Environment[];
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
  environments = [],
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

  const usageQuery = useQuery({
    queryKey: ["flag-usage", projectKey, flagKey, envKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET(
          "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/usage",
          { params: { path: { projectKey, flagKey, envKey }, query: { days: 30 } } },
        ),
      ),
    enabled: Boolean(envKey),
  });

  const schedulesQuery = useQuery({
    queryKey: ["flag-schedules", projectKey, flagKey, envKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET(
          "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/schedules",
          { params: { path: { projectKey, flagKey, envKey } } },
        ),
      ),
    enabled: Boolean(envKey),
  });

  const triggersQuery = useQuery({
    queryKey: ["flag-triggers", projectKey, flagKey, envKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET(
          "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/triggers",
          { params: { path: { projectKey, flagKey, envKey } } },
        ),
      ),
    enabled: Boolean(envKey),
  });

  const codeRefsQuery = useQuery({
    queryKey: ["flag-code-refs", projectKey, flagKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/flags/{flagKey}/code-refs", {
          params: { path: { projectKey, flagKey } },
        }),
      ),
  });

  const workflowsQuery = useQuery({
    queryKey: ["flag-workflows", projectKey, flagKey, envKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET(
          "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/workflows",
          { params: { path: { projectKey, flagKey, envKey } } },
        ),
      ),
    enabled: Boolean(envKey),
  });

  const monitoringQuery = useQuery({
    queryKey: ["flag-monitoring", projectKey, flagKey, envKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET(
          "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/monitoring",
          { params: { path: { projectKey, flagKey, envKey } } },
        ),
      ),
    enabled: Boolean(envKey),
  });

  const migrationConfigQuery = useQuery({
    queryKey: ["flag-migration-config", projectKey, flagKey, envKey],
    queryFn: async () => {
      const res = await skotosApi.GET(
        "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/migration",
        { params: { path: { projectKey, flagKey, envKey } } },
      );
      if (res.response.status === 404) return null;
      return unwrapApiResult(res);
    },
    enabled: Boolean(envKey),
  });

  const migrationTelemetryQuery = useQuery({
    queryKey: ["flag-migration-telemetry", projectKey, flagKey, envKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET(
          "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/migration/telemetry",
          { params: { path: { projectKey, flagKey, envKey } } },
        ),
      ),
    enabled: Boolean(envKey),
  });

  const [migTemplate, setMigTemplate] = useState<MigrationTemplate>("six_stage");
  const [migStage, setMigStage] = useState<MigrationStage>("off");
  const [migCheckRatio, setMigCheckRatio] = useState("1");
  const [migReadOrder, setMigReadOrder] = useState("parallel");

  useEffect(() => {
    if (migrationConfigQuery.data) {
      setMigTemplate(migrationConfigQuery.data.template);
      setMigStage(migrationConfigQuery.data.currentStage);
      setMigCheckRatio(String(migrationConfigQuery.data.checkRatio));
      setMigReadOrder(migrationConfigQuery.data.readExecutionOrder);
    }
  }, [migrationConfigQuery.data]);

  const saveMigrationMutation = useMutation({
    mutationFn: async () =>
      unwrapApiResult(
        await skotosApi.PUT(
          "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/migration",
          {
            params: { path: { projectKey, flagKey, envKey } },
            body: {
              template: migTemplate,
              currentStage: migStage,
              checkRatio: Number(migCheckRatio) || 1,
              readExecutionOrder: migReadOrder,
            },
          },
        ),
      ),
    onSuccess: (cfg) => {
      toast({ title: `Migration stage set to ${cfg.currentStage}`, tone: "success" });
      void qc.invalidateQueries({ queryKey: ["flag-migration-config", projectKey, flagKey, envKey] });
      void qc.invalidateQueries({ queryKey: ["flag-migration-telemetry", projectKey, flagKey, envKey] });
    },
    onError: (err) =>
      toast({ title: "Failed to save migration stage", description: formatErrorMessage(err), tone: "danger" }),
  });

  const [workflowName, setWorkflowName] = useState("Progressive Rollout");

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

  // Phase 2 Automation state (Copy Env, Schedules, Triggers)
  const copyTargetEnvs = useMemo(
    () => environments.filter((e) => e.key !== envKey),
    [environments, envKey],
  );
  const [copyTargetEnvKey, setCopyTargetEnvKey] = useState("");
  const [copyOnState, setCopyOnState] = useState(false);
  const [copyComment, setCopyComment] = useState("");

  const [schedAction, setSchedAction] = useState<"turnFlagOn" | "turnFlagOff">("turnFlagOn");
  const [schedAt, setSchedAt] = useState(() => {
    const dt = new Date(Date.now() + 60 * 60 * 1000);
    return dt.toISOString().slice(0, 16);
  });
  const [schedComment, setSchedComment] = useState("");

  const [triggerAction, setTriggerAction] = useState<TriggerAction>("turnFlagOff");
  const [triggerDesc, setTriggerDesc] = useState("");
  const [createdTriggerPath, setCreatedTriggerPath] = useState<string | null>(null);

  useEffect(() => {
    if (copyTargetEnvs.length > 0 && !copyTargetEnvs.some((e) => e.key === copyTargetEnvKey)) {
      setCopyTargetEnvKey(copyTargetEnvs[0]?.key ?? "");
    }
  }, [copyTargetEnvs, copyTargetEnvKey]);

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

  const copyEnvMutation = useMutation({
    mutationFn: async () =>
      unwrapApiResult(
        await skotosApi.POST("/api/v1/projects/{projectKey}/flags/{flagKey}/copy", {
          params: { path: { projectKey, flagKey } },
          body: {
            sourceEnvironmentKey: envKey,
            targetEnvironmentKey: copyTargetEnvKey,
            copyOnState,
            comment: copyComment.trim() || undefined,
          },
        }),
      ),
    onSuccess: async () => {
      await Promise.all([
        qc.invalidateQueries({ queryKey: ["flag", projectKey, flagKey] }),
        qc.invalidateQueries({ queryKey: ["flags", projectKey] }),
      ]);
      setCopyComment("");
      toast({
        title: `Copied targeting from ${envKey} to ${copyTargetEnvKey}`,
        tone: "success",
      });
    },
    onError: (err) =>
      toast({ title: "Copy targeting failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const createScheduleMutation = useMutation({
    mutationFn: async () =>
      unwrapApiResult(
        await skotosApi.POST(
          "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/schedules",
          {
            params: { path: { projectKey, flagKey, envKey } },
            body: {
              executeAt: new Date(schedAt).toISOString(),
              instructions: [{ kind: schedAction }],
              comment: schedComment.trim() || undefined,
            },
          },
        ),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["flag-schedules", projectKey, flagKey, envKey] });
      setSchedComment("");
      toast({ title: "Scheduled flag change created", tone: "success" });
    },
    onError: (err) =>
      toast({ title: "Schedule failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const cancelScheduleMutation = useMutation({
    mutationFn: async (scheduleId: string) =>
      unwrapApiResult(
        await skotosApi.DELETE(
          "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/schedules/{scheduleId}",
          { params: { path: { projectKey, flagKey, envKey, scheduleId } } },
        ),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["flag-schedules", projectKey, flagKey, envKey] });
      toast({ title: "Scheduled change canceled", tone: "success" });
    },
    onError: (err) =>
      toast({ title: "Cancel failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const createTriggerMutation = useMutation({
    mutationFn: async () =>
      unwrapApiResult(
        await skotosApi.POST(
          "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/triggers",
          {
            params: { path: { projectKey, flagKey, envKey } },
            body: {
              action: triggerAction,
              description: triggerDesc.trim(),
            },
          },
        ),
      ),
    onSuccess: async (created) => {
      await qc.invalidateQueries({ queryKey: ["flag-triggers", projectKey, flagKey, envKey] });
      setCreatedTriggerPath(created.triggerPath);
      setTriggerDesc("");
      toast({ title: "Flag trigger URL created", tone: "success" });
    },
    onError: (err) =>
      toast({ title: "Trigger creation failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const deleteTriggerMutation = useMutation({
    mutationFn: async (triggerId: string) =>
      unwrapApiResult(
        await skotosApi.DELETE(
          "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/triggers/{triggerId}",
          { params: { path: { projectKey, flagKey, envKey, triggerId } } },
        ),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["flag-triggers", projectKey, flagKey, envKey] });
      toast({ title: "Flag trigger deleted", tone: "success" });
    },
    onError: (err) =>
      toast({ title: "Delete trigger failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const submitApprovalMutation = useMutation({
    mutationFn: async (body: { instructions: FlagPatchInstruction[]; comment?: string }) =>
      unwrapApiResult(
        await skotosApi.POST(
          "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/approvals",
          {
            params: { path: { projectKey, flagKey, envKey } },
            body: {
              description: body.comment || `Change request for ${flagKey} in ${envKey}`,
              comment: body.comment || null,
              instructions: body.instructions,
            },
          },
        ),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["approvals"] });
      setReviewOpen(false);
      setSaveComment("");
      setSaveError(null);
      toast({
        title: "Approval request submitted",
        description: "Reviewers can now approve and apply these changes from the Approvals queue.",
        tone: "success",
      });
    },
    onError: (err) => setSaveError(formatErrorMessage(err)),
  });

  const createWorkflowMutation = useMutation({
    mutationFn: async () =>
      unwrapApiResult(
        await skotosApi.POST(
          "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/workflows",
          {
            params: { path: { projectKey, flagKey, envKey } },
            body: {
              name: workflowName.trim() || "Progressive Rollout",
              description: "Multi-stage automated rollout workflow",
              stages: [
                {
                  name: "Stage 1 — Enable Targeting",
                  waitSeconds: 0,
                  instructions: [{ kind: "turnFlagOn" }],
                },
                {
                  name: "Stage 2 — Wait & Verify",
                  waitSeconds: 3600,
                  instructions: [{ kind: "turnFlagOn" }],
                },
              ],
            },
          },
        ),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["flag-workflows", projectKey, flagKey, envKey] });
      toast({ title: "Flag workflow created", tone: "success" });
    },
    onError: (err) =>
      toast({ title: "Workflow creation failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const advanceWorkflowMutation = useMutation({
    mutationFn: async (workflowId: string) =>
      unwrapApiResult(
        await skotosApi.POST(
          "/api/v1/projects/{projectKey}/flags/{flagKey}/environments/{envKey}/workflows/{workflowId}/advance",
          {
            params: { path: { projectKey, flagKey, envKey, workflowId } },
            body: { approveGate: true },
          },
        ),
      ),
    onSuccess: async () => {
      await Promise.all([
        qc.invalidateQueries({ queryKey: ["flag-workflows", projectKey, flagKey, envKey] }),
        qc.invalidateQueries({ queryKey: ["flag", projectKey, flagKey] }),
      ]);
      toast({ title: "Workflow stage executed", tone: "success" });
    },
    onError: (err) =>
      toast({ title: "Workflow advance failed", description: formatErrorMessage(err), tone: "danger" }),
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

  const usage = usageQuery.data;
  const schedules = schedulesQuery.data?.items ?? [];
  const triggers = triggersQuery.data?.items ?? [];

  const automationTab = (
    <Stack gap={5}>
      {/* Evaluation Usage & Stale Detection */}
      <Card padded>
        <Stack gap={4}>
          <Stack direction="horizontal" align="center" justify="space-between">
            <Stack gap={1}>
              <Heading level={3}>Evaluation Usage (Last 30 Days)</Heading>
              <Text size="sm" tone="muted">
                Aggregated from 5-second SDK evaluation summaries in {activeEnv?.name ?? envKey}.
              </Text>
            </Stack>
            {usageQuery.isPending ? (
              <Badge tone="neutral">Loading...</Badge>
            ) : (usage?.totalEvaluations ?? 0) === 0 ? (
              <Badge tone="warning">Stale · 0 evaluations (30d)</Badge>
            ) : (
              <Badge tone="success">
                {usage?.totalEvaluations.toLocaleString()} evaluations
              </Badge>
            )}
          </Stack>

          {usage && (
            <Stack gap={3}>
              <Stack direction="horizontal" align="center" gap={3} style={{ flexWrap: "wrap" }}>
                <Text size="sm" tone="muted">
                  Last evaluated:{" "}
                  {usage.lastEvaluatedAt
                    ? new Date(usage.lastEvaluatedAt).toLocaleString()
                    : "Never"}
                </Text>
                {(usage.contextKinds ?? []).length > 0 && (
                  <Stack direction="horizontal" align="center" gap={1}>
                    <Text size="xs" tone="muted">
                      Context kinds:
                    </Text>
                    {(usage.contextKinds ?? []).map((kind) => (
                      <Badge key={kind} tone="neutral">
                        {kind}
                      </Badge>
                    ))}
                  </Stack>
                )}
              </Stack>

              {(usage.variations ?? []).length > 0 && (
                <Stack direction="horizontal" gap={2} style={{ flexWrap: "wrap" }}>
                  {(usage.variations ?? []).map((v) => {
                    const matched = flag.variations[v.variationIndex];
                    const label = matched?.name ?? `Variation #${v.variationIndex}`;
                    return (
                      <Badge key={v.variationIndex} tone="accent">
                        {label}: {v.count.toLocaleString()}
                      </Badge>
                    );
                  })}
                </Stack>
              )}
            </Stack>
          )}
        </Stack>
      </Card>

      {/* Copy Targeting to Another Environment */}
      <Card padded>
        <Stack gap={4}>
          <Stack gap={1}>
            <Heading level={3}>Copy Targeting to Another Environment</Heading>
            <Text size="sm" tone="muted">
              Promote individual targets, rules, prerequisites, and fallthrough from{" "}
              {activeEnv?.name ?? envKey} into a target environment.
            </Text>
          </Stack>
          {copyTargetEnvs.length === 0 ? (
            <Alert tone="info">
              Create another environment in Settings to copy targeting rules across environments.
            </Alert>
          ) : (
            <Stack gap={3}>
              <Stack direction="horizontal" gap={3} align="flex-end" style={{ flexWrap: "wrap" }}>
                <FormField label="Target Environment" htmlFor="copy-target-env">
                  <Select
                    id="copy-target-env"
                    value={copyTargetEnvKey}
                    disabled={!writerAllowed}
                    options={copyTargetEnvs.map((e) => ({ value: e.key, label: e.name }))}
                    onValueChange={setCopyTargetEnvKey}
                  />
                </FormField>
                <FormField label="Audit Comment" htmlFor="copy-env-comment">
                  <Input
                    id="copy-env-comment"
                    placeholder="Promote verified targeting rules..."
                    disabled={!writerAllowed}
                    value={copyComment}
                    onChange={(e) => setCopyComment(e.target.value)}
                  />
                </FormField>
                <Button
                  variant="primary"
                  disabled={!writerAllowed || !copyTargetEnvKey || copyEnvMutation.isPending}
                  onClick={() => copyEnvMutation.mutate()}
                >
                  <Icons.Copy size={14} />
                  {copyEnvMutation.isPending ? "Copying..." : "Copy Targeting"}
                </Button>
              </Stack>
              <Switch
                checked={copyOnState}
                disabled={!writerAllowed}
                onCheckedChange={setCopyOnState}
                label="Also copy ON/OFF targeting switch state"
              />
            </Stack>
          )}
        </Stack>
      </Card>

      {/* Scheduled Flag Changes */}
      <Card padded>
        <Stack gap={4}>
          <Stack gap={1}>
            <Heading level={3}>Scheduled Changes</Heading>
            <Text size="sm" tone="muted">
              Schedule an automated targeting toggle executed by the background scheduler worker.
            </Text>
          </Stack>

          {writerAllowed && (
            <Stack direction="horizontal" gap={3} align="flex-end" style={{ flexWrap: "wrap" }}>
              <FormField label="Action" htmlFor="sched-action">
                <Select
                  id="sched-action"
                  value={schedAction}
                  options={[
                    { value: "turnFlagOn", label: "Turn Targeting ON" },
                    { value: "turnFlagOff", label: "Turn Targeting OFF" },
                  ]}
                  onValueChange={(val) => setSchedAction(val as "turnFlagOn" | "turnFlagOff")}
                />
              </FormField>
              <FormField label="Execute At (UTC)" htmlFor="sched-at">
                <Input
                  id="sched-at"
                  type="datetime-local"
                  value={schedAt}
                  onChange={(e) => setSchedAt(e.target.value)}
                />
              </FormField>
              <FormField label="Comment" htmlFor="sched-comment">
                <Input
                  id="sched-comment"
                  placeholder="Scheduled launch window..."
                  value={schedComment}
                  onChange={(e) => setSchedComment(e.target.value)}
                />
              </FormField>
              <Button
                variant="primary"
                disabled={!schedAt || createScheduleMutation.isPending}
                onClick={() => createScheduleMutation.mutate()}
              >
                <Icons.Clock size={14} />
                {createScheduleMutation.isPending ? "Scheduling..." : "Schedule Change"}
              </Button>
            </Stack>
          )}

          <Separator />

          {schedules.length === 0 ? (
            <Text size="sm" tone="muted">
              No scheduled changes in this environment.
            </Text>
          ) : (
            <Stack gap={2}>
              {schedules.map((sc) => (
                <Card key={sc.id} padded>
                  <Stack direction="horizontal" align="center" justify="space-between" gap={3}>
                    <Stack gap={1}>
                      <Stack direction="horizontal" align="center" gap={2}>
                        <Badge
                          tone={
                            sc.status === "executed"
                              ? "success"
                              : sc.status === "failed"
                                ? "danger"
                                : sc.status === "pending"
                                  ? "accent"
                                  : "neutral"
                          }
                        >
                          {sc.status}
                        </Badge>
                        <Text size="sm" bold>
                          {new Date(sc.executeAt).toLocaleString()}
                        </Text>
                      </Stack>
                      <Text size="xs" tone="muted">
                        {sc.comment || JSON.stringify(sc.instructions)}
                      </Text>
                    </Stack>
                    {writerAllowed && sc.status === "pending" && (
                      <Button
                        size="sm"
                        variant="secondary"
                        disabled={cancelScheduleMutation.isPending}
                        onClick={() => cancelScheduleMutation.mutate(sc.id)}
                      >
                        Cancel
                      </Button>
                    )}
                  </Stack>
                </Card>
              ))}
            </Stack>
          )}
        </Stack>
      </Card>

      {/* Flag Triggers (APM / Monitoring Webhook Rollbacks) */}
      <Card padded>
        <Stack gap={4}>
          <Stack gap={1}>
            <Heading level={3}>Flag Triggers (APM Kill-Switch URLs)</Heading>
            <Text size="sm" tone="muted">
              Generate unauthenticated POST trigger URLs for Datadog, PagerDuty, or Prometheus
              alerts to toggle targeting immediately.
            </Text>
          </Stack>

          {createdTriggerPath && (
            <Alert tone="warning" title="Copy your one-time trigger URL now">
              <Stack gap={2}>
                <Text size="sm">
                  The plaintext trigger token is only shown once. Configure your monitoring alert to
                  send an HTTP POST request to:
                </Text>
                <CodeBlock label="Trigger Endpoint Path" code={`POST ${createdTriggerPath}`} />
              </Stack>
            </Alert>
          )}

          {writerAllowed && (
            <Stack direction="horizontal" gap={3} align="flex-end" style={{ flexWrap: "wrap" }}>
              <FormField label="Trigger Action" htmlFor="trigger-action">
                <Select
                  id="trigger-action"
                  value={triggerAction}
                  options={[
                    { value: "turnFlagOff", label: "Turn Targeting OFF (Kill-switch)" },
                    { value: "turnFlagOn", label: "Turn Targeting ON" },
                  ]}
                  onValueChange={(val) => setTriggerAction(val as TriggerAction)}
                />
              </FormField>
              <FormField label="Description" htmlFor="trigger-desc">
                <Input
                  id="trigger-desc"
                  placeholder="Datadog p99 error-rate rollback..."
                  value={triggerDesc}
                  onChange={(e) => setTriggerDesc(e.target.value)}
                />
              </FormField>
              <Button
                variant="primary"
                disabled={createTriggerMutation.isPending}
                onClick={() => createTriggerMutation.mutate()}
              >
                <Icons.Plus size={14} />
                {createTriggerMutation.isPending ? "Creating..." : "Create Trigger URL"}
              </Button>
            </Stack>
          )}

          <Separator />

          {triggers.length === 0 ? (
            <Text size="sm" tone="muted">
              No flag triggers configured in this environment.
            </Text>
          ) : (
            <Stack gap={2}>
              {triggers.map((tr) => (
                <Card key={tr.id} padded>
                  <Stack direction="horizontal" align="center" justify="space-between" gap={3}>
                    <Stack gap={1}>
                      <Stack direction="horizontal" align="center" gap={2}>
                        <Badge tone={tr.action === "turnFlagOff" ? "danger" : "success"}>
                          {tr.action}
                        </Badge>
                        <Text size="sm" mono>
                          {tr.tokenPrefix}…
                        </Text>
                        <Badge tone="neutral">Fired {tr.triggerCount}×</Badge>
                      </Stack>
                      <Text size="xs" tone="muted">
                        {tr.description || "No description"} ·{" "}
                        {tr.lastTriggeredAt
                          ? `Last fired ${new Date(tr.lastTriggeredAt).toLocaleString()}`
                          : "Never fired"}
                      </Text>
                    </Stack>
                    {writerAllowed && (
                      <Button
                        size="sm"
                        variant="danger"
                        disabled={deleteTriggerMutation.isPending}
                        onClick={() => deleteTriggerMutation.mutate(tr.id)}
                      >
                        <Icons.Trash2 size={14} aria-hidden="true" />
                        Revoke
                      </Button>
                    )}
                  </Stack>
                </Card>
              ))}
            </Stack>
          )}
        </Stack>
      </Card>

      {/* Phase 3: Multi-Step Flag Workflows */}
      <Card padded>
        <Stack gap={4}>
          <Stack gap={1}>
            <Heading level={3}>Multi-Step Rollout Workflows</Heading>
            <Text size="sm" tone="muted">
              Chain timed rollout stages and approval gates driven automatically by the Skotos
              scheduler worker.
            </Text>
          </Stack>
          {writerAllowed && (
            <Stack direction="horizontal" gap={3} align="flex-end" style={{ flexWrap: "wrap" }}>
              <FormField label="Workflow Name" htmlFor="wf-name">
                <Input
                  id="wf-name"
                  value={workflowName}
                  onChange={(e) => setWorkflowName(e.target.value)}
                />
              </FormField>
              <Button
                variant="secondary"
                disabled={createWorkflowMutation.isPending}
                onClick={() => createWorkflowMutation.mutate()}
              >
                <Icons.GitBranch size={14} />
                {createWorkflowMutation.isPending ? "Creating..." : "Create Workflow"}
              </Button>
            </Stack>
          )}
          {(workflowsQuery.data?.items ?? []).length === 0 ? (
            <Text size="sm" tone="muted">
              No workflows configured for this flag in {envKey}.
            </Text>
          ) : (
            <Stack gap={2}>
              {(workflowsQuery.data?.items ?? []).map((wf) => (
                <Card key={wf.id} padded>
                  <Stack direction="horizontal" align="center" justify="space-between" gap={3}>
                    <Stack gap={1}>
                      <Stack direction="horizontal" align="center" gap={2}>
                        <Text bold>{wf.name}</Text>
                        <Badge tone={wf.status === "completed" ? "success" : "accent"}>
                          {wf.status}
                        </Badge>
                        <Badge tone="neutral">
                          Stage {Math.min(wf.currentStageIndex + 1, wf.stages.length)} /{" "}
                          {wf.stages.length}
                        </Badge>
                      </Stack>
                      <Text size="xs" tone="muted">
                        Next run: {new Date(wf.nextRunAt).toLocaleString()}
                      </Text>
                    </Stack>
                    {writerAllowed && wf.status === "active" && (
                      <Button
                        size="sm"
                        variant="secondary"
                        disabled={advanceWorkflowMutation.isPending}
                        onClick={() => advanceWorkflowMutation.mutate(wf.id)}
                      >
                        <Icons.Play size={14} />
                        Advance Stage
                      </Button>
                    )}
                  </Stack>
                </Card>
              ))}
            </Stack>
          )}
        </Stack>
      </Card>

      {/* Phase 3: Code References & Stale Flag Extinction Tracking */}
      <Card padded>
        <Stack gap={3}>
          <Stack direction="horizontal" align="center" justify="space-between">
            <Stack gap={1}>
              <Heading level={3}>Code References (CI Scanner)</Heading>
              <Text size="sm" tone="muted">
                Source-file occurrences reported by{" "}
                <Text as="span" mono>
                  skotos-code-refs
                </Text>{" "}
                in CI pipelines.
              </Text>
            </Stack>
            {(codeRefsQuery.data?.items ?? []).length > 0 && (
              <Badge
                tone={
                  (codeRefsQuery.data?.items ?? []).every((i) => i.extinct) ? "warning" : "success"
                }
              >
                {(codeRefsQuery.data?.items ?? []).every((i) => i.extinct)
                  ? "Extinct (Safe to Archive)"
                  : `${(codeRefsQuery.data?.items ?? []).reduce((sum, i) => sum + i.referenceCount, 0)} active reference(s)`}
              </Badge>
            )}
          </Stack>
          {codeRefsQuery.isPending ? (
            <Skeleton height={60} />
          ) : (codeRefsQuery.data?.items ?? []).length === 0 ? (
            <Text size="sm" tone="muted">
              No code references uploaded yet. Run{" "}
              <Text as="span" mono>
                skotos-code-refs
              </Text>{" "}
              in GitHub Actions to track file:line locations.
            </Text>
          ) : (
            <Stack gap={3}>
              {(codeRefsQuery.data?.items ?? []).map((repoRef) => (
                <Stack key={`${repoRef.repoName}-${repoRef.branch}`} gap={2}>
                  <Text size="xs" tone="muted" mono>
                    {repoRef.repoName} @ {repoRef.branch} ({repoRef.commitSha.slice(0, 8)})
                  </Text>
                  {repoRef.hunks.map((hunk, idx) => (
                    <Card key={`${hunk.filePath}-${hunk.lineNumber}-${idx}`} padded>
                      <Stack gap={1}>
                        <Text size="xs" bold mono>
                          {hunk.filePath}:{hunk.lineNumber}
                        </Text>
                        <CodeBlock code={hunk.linePreview} />
                      </Stack>
                    </Card>
                  ))}
                </Stack>
              ))}
            </Stack>
          )}
        </Stack>
      </Card>

      {/* Phase 4: Feature Monitoring (Per-Variation Error Rate & Latency) */}
      <Card padded>
        <Stack gap={3}>
          <Stack direction="horizontal" align="center" justify="space-between">
            <Stack gap={1}>
              <Heading level={3}>Feature Monitoring (Last 24h)</Heading>
              <Text size="sm" tone="muted">
                Per-variation exposures, error rate, p95 latency, and conversions for regression
                monitoring in {activeEnv?.name ?? envKey}.
              </Text>
            </Stack>
            <Badge tone="accent">
              {(monitoringQuery.data?.variations ?? []).reduce((s, v) => s + v.exposures, 0).toLocaleString()}{" "}
              exposures
            </Badge>
          </Stack>
          {monitoringQuery.isPending ? (
            <Skeleton height={80} />
          ) : (monitoringQuery.data?.variations ?? []).length === 0 ? (
            <Text size="sm" tone="muted">
              No per-variation exposure/telemetry samples recorded in the last 24 hours.
            </Text>
          ) : (
            <DataTable
              rows={monitoringQuery.data?.variations ?? []}
              rowKey={(row) => String(row.variationIndex)}
              columns={[
                {
                  key: "var",
                  header: "Variation",
                  render: (row) => (
                    <Text bold>
                      {flag.variations[row.variationIndex]?.name ?? `Variation #${row.variationIndex}`}
                    </Text>
                  ),
                },
                {
                  key: "exp",
                  header: "Exposures",
                  render: (row) => <Text mono>{row.exposures.toLocaleString()}</Text>,
                },
                {
                  key: "err",
                  header: "Error Rate",
                  render: (row) => (
                    <Badge tone={row.errorRate > 0.05 ? "danger" : "success"}>
                      {(row.errorRate * 100).toFixed(2)}%
                    </Badge>
                  ),
                },
                {
                  key: "lat",
                  header: "p95 Latency",
                  render: (row) => (
                    <Text mono>{row.p95LatencyMs != null ? `${row.p95LatencyMs.toFixed(1)} ms` : "—"}</Text>
                  ),
                },
                {
                  key: "conv",
                  header: "Conversions",
                  render: (row) => <Text mono>{row.conversionCount.toLocaleString()}</Text>,
                },
              ]}
            />
          )}
        </Stack>
      </Card>

      {/* Phase 4: 2/4/6-Stage Migration Flags & Consistency Telemetry */}
      <Card padded>
        <Stack gap={4}>
          <Stack direction="horizontal" align="center" justify="space-between">
            <Stack gap={1}>
              <Heading level={3}>2 / 4 / 6-Stage Migration Flag</Heading>
              <Text size="sm" tone="muted">
                Orchestrate zero-downtime database or service migrations with dual-write, shadow
                reads, consistency checks, and latency telemetry.
              </Text>
            </Stack>
            {migrationConfigQuery.data && (
              <Badge tone="accent">
                Stage: {migrationConfigQuery.data.currentStage} ({migrationConfigQuery.data.template})
              </Badge>
            )}
          </Stack>

          {writerAllowed && (
            <Stack direction="horizontal" gap={3} align="flex-end" style={{ flexWrap: "wrap" }}>
              <FormField label="Template" htmlFor="mig-template">
                <Select
                  id="mig-template"
                  value={migTemplate}
                  onValueChange={(v) => {
                    const tpl = v as MigrationTemplate;
                    setMigTemplate(tpl);
                    if (tpl === "two_stage" && migStage !== "off" && migStage !== "complete") {
                      setMigStage("off");
                    }
                  }}
                  options={[
                    { value: "two_stage", label: "2-Stage (off → complete)" },
                    { value: "four_stage", label: "4-Stage (off → shadow → live → complete)" },
                    { value: "six_stage", label: "6-Stage (off → dualwrite → shadow → live → rampdown → complete)" },
                  ]}
                />
              </FormField>
              <FormField label="Current Stage" htmlFor="mig-stage">
                <Select
                  id="mig-stage"
                  value={migStage}
                  onValueChange={(v) => setMigStage(v as MigrationStage)}
                  options={
                    migTemplate === "two_stage"
                      ? [
                          { value: "off", label: "1. off (Old only)" },
                          { value: "complete", label: "2. complete (New only)" },
                        ]
                      : migTemplate === "four_stage"
                        ? [
                            { value: "off", label: "1. off" },
                            { value: "shadow", label: "2. shadow (Dual read, serve old)" },
                            { value: "live", label: "3. live (Dual read, serve new)" },
                            { value: "complete", label: "4. complete" },
                          ]
                        : [
                            { value: "off", label: "1. off" },
                            { value: "dualwrite", label: "2. dualwrite" },
                            { value: "shadow", label: "3. shadow" },
                            { value: "live", label: "4. live" },
                            { value: "rampdown", label: "5. rampdown" },
                            { value: "complete", label: "6. complete" },
                          ]
                  }
                />
              </FormField>
              <FormField label="Consistency Check Ratio" htmlFor="mig-ratio">
                <Input
                  id="mig-ratio"
                  type="number"
                  value={migCheckRatio}
                  onChange={(e) => setMigCheckRatio(e.target.value)}
                />
              </FormField>
              <FormField label="Read Order" htmlFor="mig-order">
                <Select
                  id="mig-order"
                  value={migReadOrder}
                  onValueChange={setMigReadOrder}
                  options={[
                    { value: "parallel", label: "Parallel" },
                    { value: "serial", label: "Serial" },
                    { value: "random", label: "Random" },
                  ]}
                />
              </FormField>
              <Button
                variant="primary"
                disabled={saveMigrationMutation.isPending}
                onClick={() => saveMigrationMutation.mutate()}
              >
                {saveMigrationMutation.isPending ? "Saving..." : "Save Migration Stage"}
              </Button>
            </Stack>
          )}

          {(migrationTelemetryQuery.data?.stages ?? []).length > 0 && (
            <DataTable
              rows={migrationTelemetryQuery.data?.stages ?? []}
              rowKey={(s) => s.stage}
              columns={[
                { key: "stage", header: "Stage", render: (s) => <Badge tone="accent">{s.stage}</Badge> },
                {
                  key: "ops",
                  header: "Old / New Calls",
                  render: (s) => (
                    <Text mono>
                      {s.oldInvocations.toLocaleString()} / {s.newInvocations.toLocaleString()}
                    </Text>
                  ),
                },
                {
                  key: "consistency",
                  header: "Consistency",
                  render: (s) => (
                    <Badge tone={s.consistencyRatePct >= 99 ? "success" : "warning"}>
                      {s.consistencyRatePct.toFixed(1)}% ({s.consistencyMatches}/
                      {s.consistencyMatches + s.consistencyMismatches})
                    </Badge>
                  ),
                },
                {
                  key: "latency",
                  header: "Old / New Avg Latency",
                  render: (s) => (
                    <Text mono>
                      {s.oldAvgLatencyMs.toFixed(1)}ms / {s.newAvgLatencyMs.toFixed(1)}ms
                    </Text>
                  ),
                },
                {
                  key: "errors",
                  header: "Old / New Errors",
                  render: (s) => (
                    <Text mono>
                      {s.oldErrors} / {s.newErrors}
                    </Text>
                  ),
                },
              ]}
            />
          )}
        </Stack>
      </Card>
    </Stack>
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
          { value: "automation", label: "Usage & Automation", content: automationTab },
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
              variant="secondary"
              disabled={submitApprovalMutation.isPending}
              onClick={() =>
                submitApprovalMutation.mutate({
                  instructions: draftInstructions,
                  comment: saveComment.trim() || undefined,
                })
              }
            >
              <Icons.CheckCircle2 size={14} />
              {submitApprovalMutation.isPending ? "Submitting..." : "Request Approval"}
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
          {activeEnv?.approvalSettings?.required && (
            <Alert tone="warning" title="Peer approval required in this environment">
              This environment requires {activeEnv.approvalSettings.minApprovals ?? 1} approval(s).
              Use Request Approval to queue changes for peer review.
            </Alert>
          )}
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
