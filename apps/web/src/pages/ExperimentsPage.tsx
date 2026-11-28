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
  Tabs,
  Text,
  Textarea,
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
  type Experiment,
  type ExperimentMode,
  type ExperimentResults,
  type MetricDefinition,
  type MetricGroup,
  type MetricGroupKind,
  type MetricKind,
  type MetricSuccessDirection,
  type MutualExclusionLayer,
  type StatisticalEngine,
} from "../lib/api";

export interface ExperimentsPageProps {
  projectKey: string;
  activeEnv?: Environment;
  userRole: BuiltInRole;
}

export function ExperimentsPage({ projectKey, activeEnv, userRole }: ExperimentsPageProps) {
  const qc = useQueryClient();
  const { toast } = useToast();
  const writerAllowed = canWrite(userRole);
  const envKey = activeEnv?.key ?? "";

  // Queries
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

  const groupsQuery = useQuery({
    queryKey: ["metric-groups", projectKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/metric-groups", {
          params: { path: { projectKey } },
        }),
      ),
  });

  const layersQuery = useQuery({
    queryKey: ["layers", projectKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/layers", {
          params: { path: { projectKey } },
        }),
      ),
  });

  const experimentsQuery = useQuery({
    queryKey: ["experiments", projectKey, envKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/environments/{envKey}/experiments", {
          params: { path: { projectKey, envKey } },
        }),
      ),
    enabled: Boolean(envKey),
  });

  // Selected experiment for statistical results dialog
  const [resultsExpKey, setResultsExpKey] = useState<string | null>(null);
  const resultsQuery = useQuery({
    queryKey: ["experiment-results", projectKey, envKey, resultsExpKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET(
          "/api/v1/projects/{projectKey}/environments/{envKey}/experiments/{expKey}/results",
          { params: { path: { projectKey, envKey, expKey: resultsExpKey! } } },
        ),
      ),
    enabled: Boolean(envKey && resultsExpKey),
  });

  // Create Metric dialog state
  const [metricOpen, setMetricOpen] = useState(false);
  const [mKey, setMKey] = useState("");
  const [mName, setMName] = useState("");
  const [mKind, setMKind] = useState<MetricKind>("conversion");
  const [mEventKey, setMEventKey] = useState("");
  const [mDirection, setMDirection] = useState<MetricSuccessDirection>("higher_is_better");
  const [mUnit, setMUnit] = useState("");

  // Create Metric Group dialog state
  const [groupOpen, setGroupOpen] = useState(false);
  const [gKey, setGKey] = useState("");
  const [gName, setGName] = useState("");
  const [gKind, setGKind] = useState<MetricGroupKind>("funnel");
  const [gMetricKeys, setGMetricKeys] = useState("");

  // Create Layer dialog state
  const [layerOpen, setLayerOpen] = useState(false);
  const [lKey, setLKey] = useState("");
  const [lName, setLName] = useState("");
  const [lDesc, setLDesc] = useState("");

  // Create Experiment dialog state
  const [expOpen, setExpOpen] = useState(false);
  const [eKey, setEKey] = useState("");
  const [eName, setEName] = useState("");
  const [eHyp, setEHyp] = useState("");
  const [eFlagKey, setEFlagKey] = useState("");
  const [eMetricKey, setEMetricKey] = useState("");
  const [eMode, setEMode] = useState<ExperimentMode>("ab_test");
  const [eEngine, setEEngine] = useState<StatisticalEngine>("frequentist");
  const [eLayerKey, setELayerKey] = useState("__none__");

  const createMetricMutation = useMutation({
    mutationFn: async () =>
      unwrapApiResult(
        await skotosApi.POST("/api/v1/projects/{projectKey}/metrics", {
          params: { path: { projectKey } },
          body: {
            key: mKey.trim(),
            name: mName.trim(),
            description: "",
            kind: mKind,
            eventKey: mEventKey.trim(),
            successDirection: mDirection,
            aggregation: mKind === "conversion" ? "average" : "average",
            unit: mUnit.trim(),
            randomizationUnit: "user",
          },
        }),
      ),
    onSuccess: () => {
      toast({ title: "Metric created", tone: "success" });
      setMetricOpen(false);
      setMKey("");
      setMName("");
      setMEventKey("");
      setMUnit("");
      void qc.invalidateQueries({ queryKey: ["metrics", projectKey] });
    },
    onError: (err) => toast({ title: "Failed to create metric", description: formatErrorMessage(err), tone: "danger" }),
  });

  const deleteMetricMutation = useMutation({
    mutationFn: async (metricKey: string) =>
      unwrapApiResult(
        await skotosApi.DELETE("/api/v1/projects/{projectKey}/metrics/{metricKey}", {
          params: { path: { projectKey, metricKey } },
        }),
      ),
    onSuccess: () => {
      toast({ title: "Metric deleted", tone: "success" });
      void qc.invalidateQueries({ queryKey: ["metrics", projectKey] });
    },
    onError: (err) => toast({ title: "Delete failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const createGroupMutation = useMutation({
    mutationFn: async () =>
      unwrapApiResult(
        await skotosApi.POST("/api/v1/projects/{projectKey}/metric-groups", {
          params: { path: { projectKey } },
          body: {
            key: gKey.trim(),
            name: gName.trim(),
            kind: gKind,
            description: "",
            metricKeys: gMetricKeys
              .split(",")
              .map((s) => s.trim())
              .filter(Boolean),
          },
        }),
      ),
    onSuccess: () => {
      toast({ title: "Metric group created", tone: "success" });
      setGroupOpen(false);
      setGKey("");
      setGName("");
      setGMetricKeys("");
      void qc.invalidateQueries({ queryKey: ["metric-groups", projectKey] });
    },
    onError: (err) => toast({ title: "Create group failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const createLayerMutation = useMutation({
    mutationFn: async () =>
      unwrapApiResult(
        await skotosApi.POST("/api/v1/projects/{projectKey}/layers", {
          params: { path: { projectKey } },
          body: { key: lKey.trim(), name: lName.trim(), description: lDesc.trim() },
        }),
      ),
    onSuccess: () => {
      toast({ title: "Mutual exclusion layer created", tone: "success" });
      setLayerOpen(false);
      setLKey("");
      setLName("");
      setLDesc("");
      void qc.invalidateQueries({ queryKey: ["layers", projectKey] });
    },
    onError: (err) => toast({ title: "Create layer failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const createExperimentMutation = useMutation({
    mutationFn: async () => {
      const resolvedFlag = eFlagKey || flagsQuery.data?.items[0]?.key || "";
      const resolvedMetric = eMetricKey || metricsQuery.data?.items[0]?.key || "";
      return unwrapApiResult(
        await skotosApi.POST("/api/v1/projects/{projectKey}/environments/{envKey}/experiments", {
          params: { path: { projectKey, envKey } },
          body: {
            key: eKey.trim(),
            name: eName.trim(),
            hypothesis: eHyp.trim(),
            flagKey: resolvedFlag,
            mode: eMode,
            engine: eEngine,
            randomizationUnit: "user",
            controlVariationIndex: 0,
            primaryMetricKey: resolvedMetric,
            secondaryMetricKeys: [],
            layerKey: eLayerKey && eLayerKey !== "__none__" ? eLayerKey : null,
            layerReservationBuckets: eLayerKey && eLayerKey !== "__none__" ? 20_000 : null,
            allocations: [
              { variationIndex: 0, weight: 50_000 },
              { variationIndex: 1, weight: 50_000 },
            ],
            alpha: 0.05,
          },
        }),
      );
    },
    onSuccess: () => {
      toast({ title: "Experiment created", tone: "success" });
      setExpOpen(false);
      setEKey("");
      setEName("");
      setEHyp("");
      void qc.invalidateQueries({ queryKey: ["experiments", projectKey, envKey] });
      void qc.invalidateQueries({ queryKey: ["layers", projectKey] });
    },
    onError: (err) =>
      toast({ title: "Failed to create experiment", description: formatErrorMessage(err), tone: "danger" }),
  });

  const startExperimentMutation = useMutation({
    mutationFn: async (expKey: string) =>
      unwrapApiResult(
        await skotosApi.POST(
          "/api/v1/projects/{projectKey}/environments/{envKey}/experiments/{expKey}/start",
          { params: { path: { projectKey, envKey, expKey } } },
        ),
      ),
    onSuccess: () => {
      toast({ title: "Experiment started", tone: "success" });
      void qc.invalidateQueries({ queryKey: ["experiments", projectKey, envKey] });
    },
    onError: (err) => toast({ title: "Start failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const stopExperimentMutation = useMutation({
    mutationFn: async (expKey: string) =>
      unwrapApiResult(
        await skotosApi.POST(
          "/api/v1/projects/{projectKey}/environments/{envKey}/experiments/{expKey}/stop",
          { params: { path: { projectKey, envKey, expKey } } },
        ),
      ),
    onSuccess: () => {
      toast({ title: "Experiment stopped", tone: "success" });
      void qc.invalidateQueries({ queryKey: ["experiments", projectKey, envKey] });
    },
    onError: (err) => toast({ title: "Stop failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const flags = flagsQuery.data?.items ?? [];
  const metrics = metricsQuery.data?.items ?? [];
  const groups = groupsQuery.data?.items ?? [];
  const layers = layersQuery.data?.items ?? [];
  const experiments = experimentsQuery.data?.items ?? [];

  const renderResultsBody = (res: ExperimentResults) => (
    <Stack gap={4}>
      <Stack direction="horizontal" align="center" gap={2} style={{ flexWrap: "wrap" }}>
        <Badge tone="accent">{res.engine}</Badge>
        <Badge tone="neutral">{res.mode}</Badge>
        <Badge tone={res.status === "running" ? "success" : "neutral"}>{res.status}</Badge>
        <Badge tone="neutral">{res.totalExposures.toLocaleString()} exposures</Badge>
      </Stack>

      {res.srm.srmDetected ? (
        <Alert tone="danger" title="Sample Ratio Mismatch (SRM) Detected">
          Chi-square = {res.srm.chiSquare.toFixed(2)}, p = {res.srm.pValue.toExponential(2)}. Traffic
          assignment deviates significantly from expected weights.
        </Alert>
      ) : (
        <Alert tone="success" title="SRM Check Balanced">
          Traffic allocation matches configured weights (χ² = {res.srm.chiSquare.toFixed(2)}, p ={" "}
          {res.srm.pValue.toFixed(3)}).
        </Alert>
      )}

      <Stack gap={2}>
        <Heading level={4}>
          Primary Metric: {res.primary.metricName} ({res.primary.kind})
        </Heading>
        <DataTable
          rows={res.primary.variations}
          rowKey={(v) => String(v.variationIndex)}
          columns={[
            {
              key: "variation",
              header: "Variation",
              render: (v) => (
                <Stack direction="horizontal" align="center" gap={2}>
                  <Text bold>Variation #{v.variationIndex}</Text>
                  {v.isControl && <Badge tone="neutral">Control</Badge>}
                  {v.winner && <Badge tone="success">Winner</Badge>}
                </Stack>
              ),
            },
            {
              key: "exposures",
              header: "Exposures",
              render: (v) => <Text mono>{v.exposures.toLocaleString()}</Text>,
            },
            {
              key: "mean",
              header: "Mean / Rate",
              render: (v) => (
                <Text mono>
                  {res.primary.kind === "conversion" || res.primary.kind === "error_rate"
                    ? `${(v.mean * 100).toFixed(2)}%`
                    : v.mean.toFixed(3)}
                </Text>
              ),
            },
            {
              key: "lift",
              header: "Relative Lift",
              render: (v) =>
                v.relativeLift != null ? (
                  <Badge tone={v.relativeLift >= 0 ? "success" : "danger"}>
                    {v.relativeLift >= 0 ? "+" : ""}
                    {(v.relativeLift * 100).toFixed(2)}%
                  </Badge>
                ) : (
                  <Text tone="muted">—</Text>
                ),
            },
            {
              key: "stats",
              header: res.engine === "bayesian" ? "P(Beats Control)" : "p-value",
              render: (v) =>
                v.isControl ? (
                  <Text tone="muted">Baseline</Text>
                ) : res.engine === "bayesian" ? (
                  <Text mono bold>
                    {v.probabilityToBeatControl != null
                      ? `${(v.probabilityToBeatControl * 100).toFixed(1)}%`
                      : "—"}
                  </Text>
                ) : (
                  <Text mono bold>
                    {v.pValue != null ? v.pValue.toFixed(4) : "—"}
                  </Text>
                ),
            },
          ]}
        />
      </Stack>
    </Stack>
  );

  const experimentsTab = (
    <Stack gap={4}>
      <Stack direction="horizontal" justify="space-between" align="center">
        <Text size="sm" tone="muted">
          A/B/n and A/A experiments evaluated in {activeEnv?.name ?? envKey} using Frequentist
          (Welch&apos;s t / 2-proportion z) or Bayesian inference with automatic SRM detection.
        </Text>
        {writerAllowed && (
          <Button
            variant="primary"
            disabled={!envKey || flags.length === 0 || metrics.length === 0}
            onClick={() => {
              setEFlagKey(flags[0]?.key ?? "");
              setEMetricKey(metrics[0]?.key ?? "");
              setExpOpen(true);
            }}
          >
            <Icons.Plus size={14} />
            Create Experiment
          </Button>
        )}
      </Stack>

      {experimentsQuery.isPending ? (
        <Skeleton height={160} />
      ) : experiments.length === 0 ? (
        <EmptyState
          title="No experiments yet"
          description="Define a metric first, then launch an A/B or A/A experiment linked to any feature flag."
        />
      ) : (
        <DataTable<Experiment>
          rows={experiments}
          rowKey={(row) => row.id}
          columns={[
            {
              key: "name",
              header: "Experiment",
              render: (row) => (
                <Stack gap={1}>
                  <Text bold>{row.name}</Text>
                  <Text size="xs" mono tone="muted">
                    {row.key} · flag: {row.flagKey}
                  </Text>
                </Stack>
              ),
            },
            {
              key: "config",
              header: "Engine & Metric",
              render: (row) => (
                <Stack direction="horizontal" gap={2} style={{ flexWrap: "wrap" }}>
                  <Badge tone="accent">{row.engine}</Badge>
                  <Badge tone="neutral">{row.mode}</Badge>
                  <Badge tone="neutral">{row.primaryMetricKey}</Badge>
                  {row.layerKey && <Badge tone="warning">Layer: {row.layerKey}</Badge>}
                </Stack>
              ),
            },
            {
              key: "status",
              header: "Status",
              render: (row) => (
                <Badge
                  tone={
                    row.status === "running"
                      ? "success"
                      : row.status === "stopped"
                        ? "warning"
                        : "neutral"
                  }
                >
                  {row.status}
                </Badge>
              ),
            },
            {
              key: "actions",
              header: "Actions",
              render: (row) => (
                <Stack direction="horizontal" gap={2}>
                  {writerAllowed && row.status === "draft" && (
                    <Button
                      size="sm"
                      variant="primary"
                      onClick={() => startExperimentMutation.mutate(row.key)}
                    >
                      <Icons.Play size={13} />
                      Start
                    </Button>
                  )}
                  {writerAllowed && row.status === "running" && (
                    <Button
                      size="sm"
                      variant="secondary"
                      onClick={() => stopExperimentMutation.mutate(row.key)}
                    >
                      Stop
                    </Button>
                  )}
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => setResultsExpKey(row.key)}
                  >
                    Results
                  </Button>
                </Stack>
              ),
            },
          ]}
        />
      )}
    </Stack>
  );

  const metricsTab = (
    <Stack gap={5}>
      <Card padded>
        <Stack gap={4}>
          <Stack direction="horizontal" justify="space-between" align="center">
            <Stack gap={1}>
              <Heading level={3}>Metrics</Heading>
              <Text size="sm" tone="muted">
                Reusable conversion, numeric, latency, and error-rate metrics bound to SDK custom
                event keys.
              </Text>
            </Stack>
            {writerAllowed && (
              <Button variant="primary" onClick={() => setMetricOpen(true)}>
                <Icons.Plus size={14} />
                Create Metric
              </Button>
            )}
          </Stack>

          {metricsQuery.isPending ? (
            <Skeleton height={120} />
          ) : metrics.length === 0 ? (
            <EmptyState
              title="No metrics defined"
              description="Create conversion, revenue, latency, or error-rate metrics to power experiments and guarded rollouts."
            />
          ) : (
            <DataTable<MetricDefinition>
              rows={metrics}
              rowKey={(m) => m.id}
              columns={[
                {
                  key: "name",
                  header: "Metric",
                  render: (m) => (
                    <Stack gap={1}>
                      <Text bold>{m.name}</Text>
                      <Text size="xs" mono tone="muted">
                        {m.key} (event: {m.eventKey})
                      </Text>
                    </Stack>
                  ),
                },
                {
                  key: "kind",
                  header: "Type & Direction",
                  render: (m) => (
                    <Stack direction="horizontal" gap={2}>
                      <Badge tone="accent">{m.kind}</Badge>
                      <Badge tone="neutral">{m.successDirection}</Badge>
                      {m.unit && <Badge tone="neutral">{m.unit}</Badge>}
                    </Stack>
                  ),
                },
                {
                  key: "actions",
                  header: "Actions",
                  render: (m) =>
                    writerAllowed ? (
                      <Button
                        size="sm"
                        variant="danger"
                        onClick={() => deleteMetricMutation.mutate(m.key)}
                      >
                        <Icons.Trash2 size={13} />
                        Delete
                      </Button>
                    ) : null,
                },
              ]}
            />
          )}
        </Stack>
      </Card>

      <Card padded>
        <Stack gap={4}>
          <Stack direction="horizontal" justify="space-between" align="center">
            <Stack gap={1}>
              <Heading level={3}>Metric Groups &amp; Funnels</Heading>
              <Text size="sm" tone="muted">
                Ordered multi-step funnels and curated metric collections attached to experiments.
              </Text>
            </Stack>
            {writerAllowed && (
              <Button variant="secondary" onClick={() => setGroupOpen(true)}>
                <Icons.Plus size={14} />
                Create Metric Group
              </Button>
            )}
          </Stack>

          {groups.length === 0 ? (
            <Text size="sm" tone="muted">
              No funnels or metric groups yet.
            </Text>
          ) : (
            <DataTable<MetricGroup>
              rows={groups}
              rowKey={(g) => g.id}
              columns={[
                {
                  key: "name",
                  header: "Group",
                  render: (g) => (
                    <Stack gap={1}>
                      <Text bold>{g.name}</Text>
                      <Text size="xs" mono tone="muted">
                        {g.key}
                      </Text>
                    </Stack>
                  ),
                },
                {
                  key: "kind",
                  header: "Kind",
                  render: (g) => <Badge tone="accent">{g.kind}</Badge>,
                },
                {
                  key: "steps",
                  header: "Metrics",
                  render: (g) => (
                    <Text size="xs" mono>
                      {(g.metricKeys ?? []).join(" → ")}
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

  const layersTab = (
    <Card padded>
      <Stack gap={4}>
        <Stack direction="horizontal" justify="space-between" align="center">
          <Stack gap={1}>
            <Heading level={3}>Mutual Exclusion Layers</Heading>
            <Text size="sm" tone="muted">
              Prevent interaction effects between overlapping experiments by reserving non-overlapping
              SHA-1 bucket ranges across 100,000 basis points.
            </Text>
          </Stack>
          {writerAllowed && (
            <Button variant="primary" onClick={() => setLayerOpen(true)}>
              <Icons.Plus size={14} />
              Create Layer
            </Button>
          )}
        </Stack>

        {layers.length === 0 ? (
          <EmptyState
            title="No mutual exclusion layers"
            description="Create a layer to isolate traffic between concurrent experiments on the same user journey."
          />
        ) : (
          <DataTable<MutualExclusionLayer>
            rows={layers}
            rowKey={(l) => l.id}
            columns={[
              {
                key: "name",
                header: "Layer",
                render: (l) => (
                  <Stack gap={1}>
                    <Text bold>{l.name}</Text>
                    <Text size="xs" mono tone="muted">
                      {l.key}
                    </Text>
                  </Stack>
                ),
              },
              {
                key: "utilization",
                header: "Bucket Utilization",
                render: (l) => {
                  const pct = Math.round((l.allocatedBuckets / Math.max(1, l.totalBuckets)) * 100);
                  return (
                    <Stack direction="horizontal" align="center" gap={2}>
                      <Badge tone={pct >= 90 ? "warning" : "accent"}>{pct}% allocated</Badge>
                      <Text size="xs" mono tone="muted">
                        {l.allocatedBuckets.toLocaleString()} / {l.totalBuckets.toLocaleString()}
                      </Text>
                    </Stack>
                  );
                },
              },
              {
                key: "desc",
                header: "Description",
                render: (l) => <Text size="sm" tone="muted">{l.description || "—"}</Text>,
              },
            ]}
          />
        )}
      </Stack>
    </Card>
  );

  return (
    <Stack gap={5}>
      <PageHeader
        title={<Heading level={1}>Experimentation</Heading>}
        description={
          <Text tone="muted">
            A/B &amp; A/A testing, reusable metrics, funnels, and mutual exclusion layers for{" "}
            {projectKey}.
          </Text>
        }
      />

      <Tabs
        defaultValue="experiments"
        items={[
          { value: "experiments", label: `Experiments (${experiments.length})`, content: experimentsTab },
          { value: "metrics", label: `Metrics & Funnels (${metrics.length})`, content: metricsTab },
          { value: "layers", label: `Exclusion Layers (${layers.length})`, content: layersTab },
        ]}
      />

      {/* Create Metric Dialog */}
      <Dialog
        open={metricOpen}
        onOpenChange={setMetricOpen}
        title="Create Metric"
        description="Bind a reusable experiment or guardrail metric to an SDK custom event key."
      >
        <Stack gap={3}>
          <FormField label="Metric Key" htmlFor="metric-key">
            <Input id="metric-key" mono value={mKey} onChange={(e) => setMKey(e.target.value)} placeholder="checkout-conversion" />
          </FormField>
          <FormField label="Name" htmlFor="metric-name">
            <Input id="metric-name" value={mName} onChange={(e) => setMName(e.target.value)} placeholder="Checkout Conversion" />
          </FormField>
          <FormField label="SDK Event Key" htmlFor="metric-event-key">
            <Input id="metric-event-key" mono value={mEventKey} onChange={(e) => setMEventKey(e.target.value)} placeholder="checkout_completed" />
          </FormField>
          <Stack direction="horizontal" gap={3} style={{ flexWrap: "wrap" }}>
            <FormField label="Kind" htmlFor="metric-kind">
              <Select
                id="metric-kind"
                value={mKind}
                onValueChange={(v) => setMKind(v as MetricKind)}
                options={[
                  { value: "conversion", label: "Conversion (Binary)" },
                  { value: "numeric", label: "Numeric (Revenue / Count)" },
                  { value: "latency", label: "Latency (ms)" },
                  { value: "error_rate", label: "Error Rate" },
                ]}
              />
            </FormField>
            <FormField label="Success Direction" htmlFor="metric-dir">
              <Select
                id="metric-dir"
                value={mDirection}
                onValueChange={(v) => setMDirection(v as MetricSuccessDirection)}
                options={[
                  { value: "higher_is_better", label: "Higher is better" },
                  { value: "lower_is_better", label: "Lower is better" },
                ]}
              />
            </FormField>
          </Stack>
          <FormField label="Unit (optional)" htmlFor="metric-unit">
            <Input id="metric-unit" value={mUnit} onChange={(e) => setMUnit(e.target.value)} placeholder="USD, ms, %" />
          </FormField>
          <Separator />
          <Stack direction="horizontal" justify="flex-end" gap={2}>
            <Button variant="secondary" onClick={() => setMetricOpen(false)}>Cancel</Button>
            <Button
              variant="primary"
              disabled={!mKey.trim() || !mName.trim() || !mEventKey.trim() || createMetricMutation.isPending}
              onClick={() => createMetricMutation.mutate()}
            >
              {createMetricMutation.isPending ? "Creating..." : "Create Metric"}
            </Button>
          </Stack>
        </Stack>
      </Dialog>

      {/* Create Metric Group Dialog */}
      <Dialog
        open={groupOpen}
        onOpenChange={setGroupOpen}
        title="Create Metric Group / Funnel"
        description="Group ordered funnel steps or a collection of metrics."
      >
        <Stack gap={3}>
          <FormField label="Group Key" htmlFor="group-key">
            <Input id="group-key" mono value={gKey} onChange={(e) => setGKey(e.target.value)} placeholder="purchase-funnel" />
          </FormField>
          <FormField label="Name" htmlFor="group-name">
            <Input id="group-name" value={gName} onChange={(e) => setGName(e.target.value)} placeholder="Purchase Funnel" />
          </FormField>
          <FormField label="Kind" htmlFor="group-kind">
            <Select
              id="group-kind"
              value={gKind}
              onValueChange={(v) => setGKind(v as MetricGroupKind)}
              options={[
                { value: "funnel", label: "Ordered Funnel" },
                { value: "collection", label: "Metric Collection" },
              ]}
            />
          </FormField>
          <FormField label="Metric Keys (comma-separated)" htmlFor="group-metrics">
            <Input
              id="group-metrics"
              mono
              value={gMetricKeys}
              onChange={(e) => setGMetricKeys(e.target.value)}
              placeholder={metrics.map((m) => m.key).join(", ") || "add-to-cart, checkout-conversion"}
            />
          </FormField>
          <Stack direction="horizontal" justify="flex-end" gap={2}>
            <Button variant="secondary" onClick={() => setGroupOpen(false)}>Cancel</Button>
            <Button
              variant="primary"
              disabled={!gKey.trim() || !gName.trim() || !gMetricKeys.trim() || createGroupMutation.isPending}
              onClick={() => createGroupMutation.mutate()}
            >
              Create Group
            </Button>
          </Stack>
        </Stack>
      </Dialog>

      {/* Create Layer Dialog */}
      <Dialog
        open={layerOpen}
        onOpenChange={setLayerOpen}
        title="Create Mutual Exclusion Layer"
        description="Reserve 100,000 non-overlapping SHA-1 traffic buckets for mutually exclusive experiments."
      >
        <Stack gap={3}>
          <FormField label="Layer Key" htmlFor="layer-key">
            <Input id="layer-key" mono value={lKey} onChange={(e) => setLKey(e.target.value)} placeholder="checkout-surface" />
          </FormField>
          <FormField label="Name" htmlFor="layer-name">
            <Input id="layer-name" value={lName} onChange={(e) => setLName(e.target.value)} placeholder="Checkout Surface" />
          </FormField>
          <FormField label="Description" htmlFor="layer-desc">
            <Textarea id="layer-desc" value={lDesc} onChange={(e) => setLDesc(e.target.value)} />
          </FormField>
          <Stack direction="horizontal" justify="flex-end" gap={2}>
            <Button variant="secondary" onClick={() => setLayerOpen(false)}>Cancel</Button>
            <Button
              variant="primary"
              disabled={!lKey.trim() || !lName.trim() || createLayerMutation.isPending}
              onClick={() => createLayerMutation.mutate()}
            >
              Create Layer
            </Button>
          </Stack>
        </Stack>
      </Dialog>

      {/* Create Experiment Dialog */}
      <Dialog
        open={expOpen}
        onOpenChange={setExpOpen}
        title="Create Experiment"
        description={`Attach a statistical experiment to a flag in ${activeEnv?.name ?? envKey}.`}
      >
        <Stack gap={3}>
          <FormField label="Experiment Key" htmlFor="exp-key">
            <Input id="exp-key" mono value={eKey} onChange={(e) => setEKey(e.target.value)} placeholder="exp-one-click-checkout" />
          </FormField>
          <FormField label="Name" htmlFor="exp-name">
            <Input id="exp-name" value={eName} onChange={(e) => setEName(e.target.value)} placeholder="One-Click Checkout Lift" />
          </FormField>
          <FormField label="Hypothesis" htmlFor="exp-hyp">
            <Textarea id="exp-hyp" value={eHyp} onChange={(e) => setEHyp(e.target.value)} placeholder="Reducing checkout steps increases conversion by >= 3%" />
          </FormField>
          <Stack direction="horizontal" gap={3} style={{ flexWrap: "wrap" }}>
            <FormField label="Feature Flag" htmlFor="exp-flag">
              <Select
                id="exp-flag"
                value={eFlagKey || flags[0]?.key || ""}
                onValueChange={setEFlagKey}
                options={flags.map((f) => ({ value: f.key, label: `${f.name} (${f.key})` }))}
              />
            </FormField>
            <FormField label="Primary Metric" htmlFor="exp-metric">
              <Select
                id="exp-metric"
                value={eMetricKey || metrics[0]?.key || ""}
                onValueChange={setEMetricKey}
                options={metrics.map((m) => ({ value: m.key, label: m.name }))}
              />
            </FormField>
          </Stack>
          <Stack direction="horizontal" gap={3} style={{ flexWrap: "wrap" }}>
            <FormField label="Mode" htmlFor="exp-mode">
              <Select
                id="exp-mode"
                value={eMode}
                onValueChange={(v) => setEMode(v as ExperimentMode)}
                options={[
                  { value: "ab_test", label: "A/B/n Test" },
                  { value: "aa_test", label: "A/A Validation Test" },
                ]}
              />
            </FormField>
            <FormField label="Statistical Engine" htmlFor="exp-engine">
              <Select
                id="exp-engine"
                value={eEngine}
                onValueChange={(v) => setEEngine(v as StatisticalEngine)}
                options={[
                  { value: "frequentist", label: "Frequentist (Welch / Z-Test)" },
                  { value: "bayesian", label: "Bayesian (P(Beat Control))" },
                ]}
              />
            </FormField>
          </Stack>
          {layers.length > 0 && (
            <FormField label="Mutual Exclusion Layer (optional)" htmlFor="exp-layer">
              <Select
                id="exp-layer"
                value={eLayerKey}
                onValueChange={setELayerKey}
                options={[
                  { value: "__none__", label: "None (Full Traffic)" },
                  ...layers.map((l) => ({ value: l.key, label: `${l.name} (${l.key})` })),
                ]}
              />
            </FormField>
          )}
          <Stack direction="horizontal" justify="flex-end" gap={2}>
            <Button variant="secondary" onClick={() => setExpOpen(false)}>Cancel</Button>
            <Button
              variant="primary"
              disabled={!eKey.trim() || !eName.trim() || createExperimentMutation.isPending}
              onClick={() => createExperimentMutation.mutate()}
            >
              {createExperimentMutation.isPending ? "Creating..." : "Create Experiment"}
            </Button>
          </Stack>
        </Stack>
      </Dialog>

      {/* Experiment Statistical Results Dialog */}
      <Dialog
        open={Boolean(resultsExpKey)}
        onOpenChange={(open) => {
          if (!open) setResultsExpKey(null);
        }}
        title={`Experiment Results: ${resultsExpKey ?? ""}`}
        description="Live Frequentist & Bayesian inference over sufficient statistics."
      >
        {resultsQuery.isPending ? (
          <Skeleton height={160} />
        ) : resultsQuery.error ? (
          <Alert tone="danger">{formatErrorMessage(resultsQuery.error)}</Alert>
        ) : resultsQuery.data ? (
          renderResultsBody(resultsQuery.data)
        ) : null}
      </Dialog>
    </Stack>
  );
}
