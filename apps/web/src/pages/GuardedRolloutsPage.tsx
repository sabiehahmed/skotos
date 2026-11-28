import {
  Alert,
  Badge,
  Button,
  Card,
  DataTable,
  Dialog,
  EmptyState,
  FormField,
  Heading,
  Icons,
  Input,
  PageHeader,
  Select,
  Separator,
  Skeleton,
  Stack,
  Switch,
  Text,
  useToast,
} from "@skotos/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import {
  canWrite,
  formatErrorMessage,
  skotosApi,
  unwrapApiResult,
  type BuiltInRole,
  type Environment,
  type GuardedRollout,
  type GuardedRolloutEvaluation,
} from "../lib/api";

export interface GuardedRolloutsPageProps {
  projectKey: string;
  activeEnv?: Environment;
  userRole: BuiltInRole;
}

export function GuardedRolloutsPage({
  projectKey,
  activeEnv,
  userRole,
}: GuardedRolloutsPageProps) {
  const qc = useQueryClient();
  const { toast } = useToast();
  const writerAllowed = canWrite(userRole);
  const envKey = activeEnv?.key ?? "";

  const flagsQuery = useQuery({
    queryKey: ["flags", projectKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/flags", {
          params: { path: { projectKey } },
        }),
      ),
  });

  const metricsQuery = useQuery({
    queryKey: ["metrics", projectKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/metrics", {
          params: { path: { projectKey } },
        }),
      ),
  });

  const rolloutsQuery = useQuery({
    queryKey: ["guarded-rollouts", projectKey, envKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET(
          "/api/v1/projects/{projectKey}/environments/{envKey}/guarded-rollouts",
          { params: { path: { projectKey, envKey } } },
        ),
      ),
    enabled: Boolean(envKey),
  });

  const [createOpen, setCreateOpen] = useState(false);
  const [rName, setRName] = useState("");
  const [rFlagKey, setRFlagKey] = useState("");
  const [rMetricKey, setRMetricKey] = useState("");
  const [rTrafficPct, setRTrafficPct] = useState("20");
  const [rMaxRegPct, setRMaxRegPct] = useState("5");
  const [rAutoRollback, setRAutoRollback] = useState(true);
  const [lastEval, setLastEval] = useState<GuardedRolloutEvaluation | null>(null);

  const createMutation = useMutation({
    mutationFn: async () => {
      const resolvedFlag = rFlagKey || flagsQuery.data?.items[0]?.key || "";
      const resolvedMetric = rMetricKey || metricsQuery.data?.items[0]?.key || "";
      return unwrapApiResult(
        await skotosApi.POST(
          "/api/v1/projects/{projectKey}/environments/{envKey}/guarded-rollouts",
          {
            params: { path: { projectKey, envKey } },
            body: {
              flagKey: resolvedFlag,
              name: rName.trim(),
              controlVariationIndex: 1,
              treatmentVariationIndex: 0,
              currentTrafficPct: Number(rTrafficPct) || 20,
              autoRollback: rAutoRollback,
              guardrails: [
                {
                  metricKey: resolvedMetric,
                  maxRegressionPct: Number(rMaxRegPct) || 5,
                  minSampleSize: 20,
                  alpha: 0.05,
                },
              ],
            },
          },
        ),
      );
    },
    onSuccess: () => {
      toast({ title: "Guarded rollout started", tone: "success" });
      setCreateOpen(false);
      setRName("");
      void qc.invalidateQueries({ queryKey: ["guarded-rollouts", projectKey, envKey] });
    },
    onError: (err) =>
      toast({ title: "Failed to start guarded rollout", description: formatErrorMessage(err), tone: "danger" }),
  });

  const evaluateMutation = useMutation({
    mutationFn: async (rolloutId: string) =>
      unwrapApiResult(
        await skotosApi.POST(
          "/api/v1/projects/{projectKey}/environments/{envKey}/guarded-rollouts/{rolloutId}/evaluate",
          { params: { path: { projectKey, envKey, rolloutId } } },
        ),
      ),
    onSuccess: (res) => {
      setLastEval(res);
      toast({
        title: res.autoRolledBack
          ? "Regression detected — flag automatically rolled back!"
          : res.regressionDetected
            ? "Regression alert triggered"
            : "Guardrails healthy",
        tone: res.regressionDetected ? "danger" : "success",
      });
      void qc.invalidateQueries({ queryKey: ["guarded-rollouts", projectKey, envKey] });
      void qc.invalidateQueries({ queryKey: ["flags", projectKey] });
    },
    onError: (err) =>
      toast({ title: "Evaluation failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const rollbackMutation = useMutation({
    mutationFn: async (rolloutId: string) =>
      unwrapApiResult(
        await skotosApi.POST(
          "/api/v1/projects/{projectKey}/environments/{envKey}/guarded-rollouts/{rolloutId}/rollback",
          { params: { path: { projectKey, envKey, rolloutId } } },
        ),
      ),
    onSuccess: () => {
      toast({ title: "Flag rolled back to OFF", tone: "success" });
      void qc.invalidateQueries({ queryKey: ["guarded-rollouts", projectKey, envKey] });
      void qc.invalidateQueries({ queryKey: ["flags", projectKey] });
    },
    onError: (err) =>
      toast({ title: "Rollback failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const flags = flagsQuery.data?.items ?? [];
  const metrics = metricsQuery.data?.items ?? [];
  const rollouts = rolloutsQuery.data?.items ?? [];

  return (
    <Stack gap={5}>
      <PageHeader
        title={<Heading level={1}>Guarded Rollouts</Heading>}
        description={
          <Text tone="muted">
            Automated metric-guarded progressive delivery with instant regression rollback in{" "}
            {activeEnv?.name ?? envKey}.
          </Text>
        }
        actions={
          writerAllowed ? (
            <Button
              variant="primary"
              disabled={!envKey || flags.length === 0 || metrics.length === 0}
              onClick={() => {
                setRFlagKey(flags[0]?.key ?? "");
                setRMetricKey(metrics[0]?.key ?? "");
                setCreateOpen(true);
              }}
            >
              <Icons.Shield size={14} />
              Start Guarded Rollout
            </Button>
          ) : undefined
        }
      />

      {lastEval && (
        <Card padded>
          <Stack gap={3}>
            <Stack direction="horizontal" justify="space-between" align="center">
              <Heading level={4}>Latest Evaluation: {lastEval.rollout.name}</Heading>
              <Badge tone={lastEval.regressionDetected ? "danger" : "success"}>
                {lastEval.autoRolledBack
                  ? "AUTO-ROLLED BACK"
                  : lastEval.regressionDetected
                    ? "REGRESSION DETECTED"
                    : "HEALTHY"}
              </Badge>
            </Stack>
            {lastEval.srm.srmDetected && (
              <Alert tone="warning" title="SRM Warning">
                Traffic ratio skew detected (χ² = {lastEval.srm.chiSquare.toFixed(2)}).
              </Alert>
            )}
            <DataTable
              rows={lastEval.checks}
              rowKey={(c) => c.metricKey}
              columns={[
                { key: "metric", header: "Guardrail Metric", render: (c) => <Text bold mono>{c.metricKey}</Text> },
                {
                  key: "control",
                  header: "Control",
                  render: (c) => <Text mono>{c.controlMean.toFixed(4)} (n={c.controlSamples})</Text>,
                },
                {
                  key: "treatment",
                  header: "Treatment",
                  render: (c) => <Text mono>{c.treatmentMean.toFixed(4)} (n={c.treatmentSamples})</Text>,
                },
                {
                  key: "change",
                  header: "Regression Δ",
                  render: (c) => <Text mono>{c.regressionPct.toFixed(2)}%</Text>,
                },
                {
                  key: "status",
                  header: "Verdict",
                  render: (c) => (
                    <Badge tone={c.regressed ? "danger" : "success"}>
                      {c.regressed
                        ? `Regressed${c.pValue != null ? ` (p=${c.pValue.toFixed(4)})` : ""}`
                        : "Healthy"}
                    </Badge>
                  ),
                },
              ]}
            />
          </Stack>
        </Card>
      )}

      {rolloutsQuery.isPending ? (
        <Skeleton height={160} />
      ) : rollouts.length === 0 ? (
        <EmptyState
          title="No guarded rollouts active"
          description="Attach error-rate, latency, or conversion guardrails to any flag to trigger automated rollback on statistically significant regressions."
        />
      ) : (
        <DataTable<GuardedRollout>
          rows={rollouts}
          rowKey={(r) => r.id}
          columns={[
            {
              key: "name",
              header: "Rollout",
              render: (r) => (
                <Stack gap={1}>
                  <Text bold>{r.name}</Text>
                  <Text size="xs" mono tone="muted">
                    Flag: {r.flagKey} · {r.currentTrafficPct}% traffic
                  </Text>
                </Stack>
              ),
            },
            {
              key: "guardrails",
              header: "Guardrails",
              render: (r) => (
                <Stack direction="horizontal" gap={2} style={{ flexWrap: "wrap" }}>
                  {(r.guardrails ?? []).map((g) => (
                    <Badge key={g.metricKey} tone="neutral">
                      {g.metricKey} (≤ {g.maxRegressionPct}%)
                    </Badge>
                  ))}
                  <Badge tone={r.autoRollback ? "accent" : "neutral"}>
                    {r.autoRollback ? "Auto-Rollback ON" : "Alert Only"}
                  </Badge>
                </Stack>
              ),
            },
            {
              key: "status",
              header: "Status",
              render: (r) => (
                <Stack gap={1}>
                  <Badge
                    tone={
                      r.status === "rolled_back"
                        ? "danger"
                        : r.status === "monitoring"
                          ? "success"
                          : "neutral"
                    }
                  >
                    {r.status}
                  </Badge>
                  {r.lastVerdict && (
                    <Text size="xs" tone="muted">
                      {r.lastVerdict}
                    </Text>
                  )}
                </Stack>
              ),
            },
            {
              key: "actions",
              header: "Actions",
              render: (r) =>
                writerAllowed ? (
                  <Stack direction="horizontal" gap={2}>
                    <Button
                      size="sm"
                      variant="secondary"
                      disabled={evaluateMutation.isPending}
                      onClick={() => evaluateMutation.mutate(r.id)}
                    >
                      <Icons.RefreshCw size={13} />
                      Evaluate Now
                    </Button>
                    {r.status === "monitoring" && (
                      <Button
                        size="sm"
                        variant="danger"
                        disabled={rollbackMutation.isPending}
                        onClick={() => rollbackMutation.mutate(r.id)}
                      >
                        <Icons.RotateCcw size={13} />
                        Rollback
                      </Button>
                    )}
                  </Stack>
                ) : null,
            },
          ]}
        />
      )}

      <Dialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        title="Start Guarded Rollout"
        description="Monitor live telemetry and automatically turn the flag OFF if a guardrail metric regresses."
      >
        <Stack gap={3}>
          <FormField label="Rollout Name" htmlFor="gr-name">
            <Input id="gr-name" value={rName} onChange={(e) => setRName(e.target.value)} placeholder="Checkout v2 Canary 20%" />
          </FormField>
          <Stack direction="horizontal" gap={3} style={{ flexWrap: "wrap" }}>
            <FormField label="Feature Flag" htmlFor="gr-flag">
              <Select
                id="gr-flag"
                value={rFlagKey || flags[0]?.key || ""}
                onValueChange={setRFlagKey}
                options={flags.map((f) => ({ value: f.key, label: `${f.name} (${f.key})` }))}
              />
            </FormField>
            <FormField label="Guardrail Metric" htmlFor="gr-metric">
              <Select
                id="gr-metric"
                value={rMetricKey || metrics[0]?.key || ""}
                onValueChange={setRMetricKey}
                options={metrics.map((m) => ({ value: m.key, label: `${m.name} (${m.kind})` }))}
              />
            </FormField>
          </Stack>
          <Stack direction="horizontal" gap={3} style={{ flexWrap: "wrap" }}>
            <FormField label="Traffic %" htmlFor="gr-traffic">
              <Input id="gr-traffic" type="number" value={rTrafficPct} onChange={(e) => setRTrafficPct(e.target.value)} />
            </FormField>
            <FormField label="Max Regression Tolerance (%)" htmlFor="gr-max-reg">
              <Input id="gr-max-reg" type="number" value={rMaxRegPct} onChange={(e) => setRMaxRegPct(e.target.value)} />
            </FormField>
          </Stack>
          <Switch
            checked={rAutoRollback}
            onCheckedChange={setRAutoRollback}
            label="Automatically turn flag OFF on statistically significant regression"
          />
          <Separator />
          <Stack direction="horizontal" justify="flex-end" gap={2}>
            <Button variant="secondary" onClick={() => setCreateOpen(false)}>Cancel</Button>
            <Button
              variant="primary"
              disabled={!rName.trim() || createMutation.isPending}
              onClick={() => createMutation.mutate()}
            >
              {createMutation.isPending ? "Starting..." : "Start Guarded Rollout"}
            </Button>
          </Stack>
        </Stack>
      </Dialog>
    </Stack>
  );
}
