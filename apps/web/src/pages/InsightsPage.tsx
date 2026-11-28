import {
  Alert,
  Badge,
  Button,
  Card,
  CodeBlock,
  DataTable,
  Dialog,
  EmptyState,
  FormField,
  Grid,
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
  useToast,
} from "@skotos/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import {
  canAdmin,
  canWrite,
  formatErrorMessage,
  skotosApi,
  unwrapApiResult,
  type BuiltInRole,
  type DataExportSink,
  type DataExportSinkKind,
  type Environment,
  type SyncedSegmentIntegration,
  type SyncedSegmentProvider,
} from "../lib/api";

export interface InsightsPageProps {
  projectKey: string;
  activeEnv?: Environment;
  userRole: BuiltInRole;
}

export function InsightsPage({ projectKey, activeEnv, userRole }: InsightsPageProps) {
  const qc = useQueryClient();
  const { toast } = useToast();
  const writerAllowed = canWrite(userRole);
  const adminAllowed = canAdmin(userRole);
  const envKey = activeEnv?.key ?? "";

  const insightsQuery = useQuery({
    queryKey: ["insights", projectKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/insights", {
          params: { path: { projectKey } },
        }),
      ),
  });

  const sinksQuery = useQuery({
    queryKey: ["data-export-sinks"],
    queryFn: async () => unwrapApiResult(await skotosApi.GET("/api/v1/data-export/sinks")),
  });

  const segmentsQuery = useQuery({
    queryKey: ["segments", projectKey, envKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/environments/{envKey}/segments", {
          params: { path: { projectKey, envKey } },
        }),
      ),
    enabled: Boolean(envKey),
  });

  const syncedQuery = useQuery({
    queryKey: ["synced-segments", projectKey, envKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/environments/{envKey}/synced-segments", {
          params: { path: { projectKey, envKey } },
        }),
      ),
    enabled: Boolean(envKey),
  });

  // Data Export Sink dialog
  const [sinkOpen, setSinkOpen] = useState(false);
  const [sinkName, setSinkName] = useState("");
  const [sinkKind, setSinkKind] = useState<DataExportSinkKind>("s3_parquet");
  const [sinkDest, setSinkDest] = useState("s3://skotos-exports/events");

  // Synced Segment dialog
  const [syncOpen, setSyncOpen] = useState(false);
  const [syncSegKey, setSyncSegKey] = useState("");
  const [syncProvider, setSyncProvider] = useState<SyncedSegmentProvider>("hightouch");
  const [syncCohortId, setSyncCohortId] = useState("");
  const [createdSyncPath, setCreatedSyncPath] = useState<string | null>(null);

  const createSinkMutation = useMutation({
    mutationFn: async () =>
      unwrapApiResult(
        await skotosApi.POST("/api/v1/data-export/sinks", {
          body: {
            name: sinkName.trim(),
            kind: sinkKind,
            destination: sinkDest.trim(),
            envId: activeEnv?.id ?? null,
            config: {},
          },
        }),
      ),
    onSuccess: () => {
      toast({ title: "Data export sink created", tone: "success" });
      setSinkOpen(false);
      setSinkName("");
      void qc.invalidateQueries({ queryKey: ["data-export-sinks"] });
    },
    onError: (err) =>
      toast({ title: "Failed to create sink", description: formatErrorMessage(err), tone: "danger" }),
  });

  const flushSinkMutation = useMutation({
    mutationFn: async (sinkId: string) =>
      unwrapApiResult(
        await skotosApi.POST("/api/v1/data-export/sinks/{sinkId}/flush", {
          params: { path: { sinkId } },
        }),
      ),
    onSuccess: (res) => {
      toast({
        title: `Exported ${res.eventsExported} event(s)`,
        description: res.location,
        tone: "success",
      });
      void qc.invalidateQueries({ queryKey: ["data-export-sinks"] });
    },
    onError: (err) =>
      toast({ title: "Export flush failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const deleteSinkMutation = useMutation({
    mutationFn: async (sinkId: string) =>
      unwrapApiResult(
        await skotosApi.DELETE("/api/v1/data-export/sinks/{sinkId}", {
          params: { path: { sinkId } },
        }),
      ),
    onSuccess: () => {
      toast({ title: "Sink deleted", tone: "success" });
      void qc.invalidateQueries({ queryKey: ["data-export-sinks"] });
    },
    onError: (err) => toast({ title: "Delete failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const createSyncedMutation = useMutation({
    mutationFn: async () => {
      const segKey = syncSegKey || segmentsQuery.data?.items[0]?.key || "";
      return unwrapApiResult(
        await skotosApi.POST("/api/v1/projects/{projectKey}/environments/{envKey}/synced-segments", {
          params: { path: { projectKey, envKey } },
          body: {
            segmentKey: segKey,
            provider: syncProvider,
            externalCohortId: syncCohortId.trim(),
            contextKind: "user",
          },
        }),
      );
    },
    onSuccess: (res) => {
      setCreatedSyncPath(res.ingestPath);
      toast({ title: "Synced segment integration created", tone: "success" });
      setSyncOpen(false);
      setSyncCohortId("");
      void qc.invalidateQueries({ queryKey: ["synced-segments", projectKey, envKey] });
    },
    onError: (err) =>
      toast({ title: "Create integration failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const deleteSyncedMutation = useMutation({
    mutationFn: async (integrationId: string) =>
      unwrapApiResult(
        await skotosApi.DELETE("/api/v1/synced-segments/{integrationId}", {
          params: { path: { integrationId } },
        }),
      ),
    onSuccess: () => {
      toast({ title: "Synced segment integration removed", tone: "success" });
      void qc.invalidateQueries({ queryKey: ["synced-segments", projectKey, envKey] });
    },
    onError: (err) => toast({ title: "Delete failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const insights = insightsQuery.data;
  const sinks = sinksQuery.data?.items ?? [];
  const segments = segmentsQuery.data?.items ?? [];
  const synced = syncedQuery.data?.items ?? [];

  const doraTab = insightsQuery.isPending ? (
    <Skeleton height={220} />
  ) : !insights ? (
    <EmptyState title="Insights unavailable" description="Could not compute DORA metrics." />
  ) : (
    <Stack gap={5}>
      <Grid columns={4} gap={4}>
        <Card padded>
          <Stack gap={1}>
            <Text size="xs" tone="muted" bold>DEPLOYMENT FREQUENCY (30D)</Text>
            <Heading level={2}>{insights.deploymentsPerDay.toFixed(2)} / day</Heading>
            <Text size="xs" tone="muted">{insights.flagDeploymentsCount} flag releases</Text>
          </Stack>
        </Card>
        <Card padded>
          <Stack gap={1}>
            <Text size="xs" tone="muted" bold>CHANGE FAILURE RATE</Text>
            <Heading level={2}>{insights.changeFailureRatePct.toFixed(1)}%</Heading>
            <Text size="xs" tone="muted">{insights.rollbacksCount} rollbacks / reverts</Text>
          </Stack>
        </Card>
        <Card padded>
          <Stack gap={1}>
            <Text size="xs" tone="muted" bold>MEAN TIME TO RECOVERY</Text>
            <Heading level={2}>{insights.meanTimeToRecoveryMinutes.toFixed(1)}m</Heading>
            <Text size="xs" tone="muted">Median incident rollback speed</Text>
          </Stack>
        </Card>
        <Card padded>
          <Stack gap={1}>
            <Text size="xs" tone="muted" bold>FLAG HEALTH</Text>
            <Heading level={2}>{insights.totalFlags} flags</Heading>
            <Text size="xs" tone="muted">
              {insights.staleFlags} stale · {insights.temporaryFlags} temporary
            </Text>
          </Stack>
        </Card>
      </Grid>

      <Card padded>
        <Stack direction="horizontal" align="center" gap={3} style={{ flexWrap: "wrap" }}>
          <Badge tone="accent">{insights.activeExperiments} running experiment(s)</Badge>
          <Badge tone="success">{insights.activeGuardedRollouts} active guarded rollout(s)</Badge>
          <Badge tone={insights.rolledBackGuardrails > 0 ? "warning" : "neutral"}>
            {insights.rolledBackGuardrails} guardrail auto-rollback(s)
          </Badge>
        </Stack>
      </Card>
    </Stack>
  );

  const exportTab = (
    <Card padded>
      <Stack gap={4}>
        <Stack direction="horizontal" justify="space-between" align="center">
          <Stack gap={1}>
            <Heading level={3}>Real-Time Data Export Sinks</Heading>
            <Text size="sm" tone="muted">
              Stream Zstd-compressed NDJSON evaluation and experiment batches to S3/R2, Kinesis,
              Pub/Sub, Snowflake, Segment, or custom webhooks.
            </Text>
          </Stack>
          {adminAllowed && (
            <Button variant="primary" onClick={() => setSinkOpen(true)}>
              <Icons.Plus size={14} />
              Add Export Sink
            </Button>
          )}
        </Stack>

        {sinks.length === 0 ? (
          <EmptyState
            title="No data export sinks configured"
            description="Connect S3/R2 object storage, Snowflake, Kinesis, or Segment for real-time telemetry warehouse sync."
          />
        ) : (
          <DataTable<DataExportSink>
            rows={sinks}
            rowKey={(s) => s.id}
            columns={[
              {
                key: "name",
                header: "Sink",
                render: (s) => (
                  <Stack gap={1}>
                    <Text bold>{s.name}</Text>
                    <Text size="xs" mono tone="muted">{s.destination}</Text>
                  </Stack>
                ),
              },
              {
                key: "kind",
                header: "Destination Type",
                render: (s) => <Badge tone="accent">{s.kind}</Badge>,
              },
              {
                key: "count",
                header: "Exported Events",
                render: (s) => (
                  <Text mono>{s.exportedEventCount.toLocaleString()}</Text>
                ),
              },
              {
                key: "actions",
                header: "Actions",
                render: (s) =>
                  adminAllowed ? (
                    <Stack direction="horizontal" gap={2}>
                      <Button
                        size="sm"
                        variant="secondary"
                        disabled={flushSinkMutation.isPending}
                        onClick={() => flushSinkMutation.mutate(s.id)}
                      >
                        <Icons.RefreshCw size={13} />
                        Flush Now
                      </Button>
                      <Button
                        size="sm"
                        variant="danger"
                        onClick={() => deleteSinkMutation.mutate(s.id)}
                      >
                        <Icons.Trash2 size={13} />
                        Delete
                      </Button>
                    </Stack>
                  ) : null,
              },
            ]}
          />
        )}
      </Stack>
    </Card>
  );

  const syncedTab = (
    <Card padded>
      <Stack gap={4}>
        <Stack direction="horizontal" justify="space-between" align="center">
          <Stack gap={1}>
            <Heading level={3}>Synced Segments (Reverse-ETL &amp; CDP Push)</Heading>
            <Text size="sm" tone="muted">
              Accept high-volume cohort pushes from Amplitude, Hightouch, Fivetran, Census,
              RudderStack, or Segment directly into Skotos Big Segments.
            </Text>
          </Stack>
          {writerAllowed && (
            <Button
              variant="primary"
              disabled={!envKey || segments.length === 0}
              onClick={() => {
                setSyncSegKey(segments[0]?.key ?? "");
                setSyncOpen(true);
              }}
            >
              <Icons.Plus size={14} />
              Connect CDP / Warehouse
            </Button>
          )}
        </Stack>

        {createdSyncPath && (
          <Alert tone="warning" title="Copy your one-time Synced Segment push URL">
            <Stack gap={2}>
              <Text size="sm">
                Configure your reverse-ETL connector to POST JSON add/remove key batches to:
              </Text>
              <CodeBlock label="Synced Segment Push Endpoint" code={`POST ${createdSyncPath}`} />
            </Stack>
          </Alert>
        )}

        {synced.length === 0 ? (
          <EmptyState
            title="No synced segment integrations"
            description="Create a Big Segment first, then connect Hightouch, Census, Amplitude, or Segment to push cohorts."
          />
        ) : (
          <DataTable<SyncedSegmentIntegration>
            rows={synced}
            rowKey={(s) => s.id}
            columns={[
              {
                key: "seg",
                header: "Segment & Cohort",
                render: (s) => (
                  <Stack gap={1}>
                    <Text bold mono>{s.segmentKey}</Text>
                    <Text size="xs" tone="muted">
                      External cohort: {s.externalCohortId} ({s.contextKind})
                    </Text>
                  </Stack>
                ),
              },
              {
                key: "provider",
                header: "Provider",
                render: (s) => <Badge tone="accent">{s.provider}</Badge>,
              },
              {
                key: "members",
                header: "Synced Keys",
                render: (s) => <Text mono>{s.syncedMemberCount.toLocaleString()}</Text>,
              },
              {
                key: "actions",
                header: "Actions",
                render: (s) =>
                  writerAllowed ? (
                    <Button
                      size="sm"
                      variant="danger"
                      onClick={() => deleteSyncedMutation.mutate(s.id)}
                    >
                      <Icons.Trash2 size={13} />
                      Revoke
                    </Button>
                  ) : null,
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
        title={<Heading level={1}>Engineering Insights &amp; Data Export</Heading>}
        description={
          <Text tone="muted">
            DORA delivery velocity, flag health, warehouse data export sinks, and CDP synced
            segments for {projectKey}.
          </Text>
        }
      />

      <Tabs
        defaultValue="dora"
        items={[
          { value: "dora", label: "DORA & Flag Health", content: doraTab },
          { value: "exports", label: `Data Export Sinks (${sinks.length})`, content: exportTab },
          { value: "synced", label: `Synced Segments (${synced.length})`, content: syncedTab },
        ]}
      />

      {/* Create Data Export Sink Dialog */}
      <Dialog
        open={sinkOpen}
        onOpenChange={setSinkOpen}
        title="Add Data Export Sink"
        description="Configure a real-time destination for evaluation, exposure, and custom event telemetry."
      >
        <Stack gap={3}>
          <FormField label="Sink Name" htmlFor="sink-name">
            <Input id="sink-name" value={sinkName} onChange={(e) => setSinkName(e.target.value)} placeholder="Production R2 Data Lake" />
          </FormField>
          <FormField label="Kind" htmlFor="sink-kind">
            <Select
              id="sink-kind"
              value={sinkKind}
              onValueChange={(v) => setSinkKind(v as DataExportSinkKind)}
              options={[
                { value: "s3_parquet", label: "S3 / R2 Zstd Archive" },
                { value: "snowflake", label: "Snowflake Ingest Webhook" },
                { value: "kinesis", label: "AWS Kinesis Firehose" },
                { value: "pubsub", label: "Google Cloud Pub/Sub" },
                { value: "segment", label: "Segment HTTP API" },
                { value: "webhook_ndjson", label: "Custom NDJSON Webhook" },
              ]}
            />
          </FormField>
          <FormField label="Destination URI / Endpoint" htmlFor="sink-dest">
            <Input id="sink-dest" mono value={sinkDest} onChange={(e) => setSinkDest(e.target.value)} />
          </FormField>
          <Separator />
          <Stack direction="horizontal" justify="flex-end" gap={2}>
            <Button variant="secondary" onClick={() => setSinkOpen(false)}>Cancel</Button>
            <Button
              variant="primary"
              disabled={!sinkName.trim() || !sinkDest.trim() || createSinkMutation.isPending}
              onClick={() => createSinkMutation.mutate()}
            >
              Create Sink
            </Button>
          </Stack>
        </Stack>
      </Dialog>

      {/* Connect Synced Segment Dialog */}
      <Dialog
        open={syncOpen}
        onOpenChange={setSyncOpen}
        title="Connect Synced Segment"
        description={`Generate a push ingestion token for an external CDP or reverse-ETL cohort in ${envKey}.`}
      >
        <Stack gap={3}>
          <FormField label="Target Segment" htmlFor="sync-seg">
            <Select
              id="sync-seg"
              value={syncSegKey || segments[0]?.key || ""}
              onValueChange={setSyncSegKey}
              options={segments.map((s) => ({ value: s.key, label: `${s.name} (${s.key})` }))}
            />
          </FormField>
          <FormField label="Provider" htmlFor="sync-provider">
            <Select
              id="sync-provider"
              value={syncProvider}
              onValueChange={(v) => setSyncProvider(v as SyncedSegmentProvider)}
              options={[
                { value: "hightouch", label: "Hightouch" },
                { value: "amplitude", label: "Amplitude" },
                { value: "fivetran", label: "Fivetran" },
                { value: "census", label: "Census" },
                { value: "rudderstack", label: "RudderStack" },
                { value: "segment", label: "Twilio Segment" },
                { value: "webhook", label: "Custom Webhook Push" },
              ]}
            />
          </FormField>
          <FormField label="External Cohort ID" htmlFor="sync-cohort">
            <Input id="sync-cohort" mono value={syncCohortId} onChange={(e) => setSyncCohortId(e.target.value)} placeholder="cohort_high_ltv_us" />
          </FormField>
          <Separator />
          <Stack direction="horizontal" justify="flex-end" gap={2}>
            <Button variant="secondary" onClick={() => setSyncOpen(false)}>Cancel</Button>
            <Button
              variant="primary"
              disabled={!syncCohortId.trim() || createSyncedMutation.isPending}
              onClick={() => createSyncedMutation.mutate()}
            >
              Generate Push Token
            </Button>
          </Stack>
        </Stack>
      </Dialog>
    </Stack>
  );
}
