import {
  Alert,
  AlertDialog,
  Badge,
  Button,
  Card,
  Checkbox,
  DataTable,
  Dialog,
  FormField,
  Heading,
  Icons,
  Input,
  PageHeader,
  Select,
  Separator,
  Skeleton,
  Stack,
  Text,
  useToast,
} from "@skotos/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import {
  canAdmin,
  formatErrorMessage,
  skotosApi,
  unwrapApiResult,
  type ApplicationKind,
  type BuiltInRole,
  type Environment,
  type ReleasePhase,
  type ReleasePipeline,
} from "../lib/api";

export interface PipelinesPageProps {
  projectKey: string;
  environments: Environment[];
  userRole: BuiltInRole;
}

export function PipelinesPage({ projectKey, environments, userRole }: PipelinesPageProps) {
  const qc = useQueryClient();
  const { toast } = useToast();
  const adminAllowed = canAdmin(userRole);

  // Release Pipeline create dialog state
  const [pipeOpen, setPipeOpen] = useState(false);
  const [pipeKey, setPipeKey] = useState("");
  const [pipeName, setPipeName] = useState("");
  const [pipeDesc, setPipeDesc] = useState("");
  const [phases, setPhases] = useState<ReleasePhase[]>([]);
  const [pipeErr, setPipeErr] = useState<string | null>(null);
  const [deletePipe, setDeletePipe] = useState<ReleasePipeline | null>(null);

  // Application create dialog state
  const [appOpen, setAppOpen] = useState(false);
  const [appKey, setAppKey] = useState("");
  const [appName, setAppName] = useState("");
  const [appKind, setAppKind] = useState<ApplicationKind>("browser");
  const [appDesc, setAppDesc] = useState("");
  const [appErr, setAppErr] = useState<string | null>(null);

  const pipelinesQuery = useQuery({
    queryKey: ["release-pipelines", projectKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/release-pipelines", {
          params: { path: { projectKey } },
        }),
      ),
  });

  const appsQuery = useQuery({
    queryKey: ["applications"],
    queryFn: async () => unwrapApiResult(await skotosApi.GET("/api/v1/applications")),
  });

  const createPipelineMutation = useMutation({
    mutationFn: async () =>
      unwrapApiResult(
        await skotosApi.POST("/api/v1/projects/{projectKey}/release-pipelines", {
          params: { path: { projectKey } },
          body: {
            key: pipeKey.trim(),
            name: pipeName.trim(),
            description: pipeDesc.trim(),
            phases,
          },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["release-pipelines", projectKey] });
      setPipeOpen(false);
      setPipeKey("");
      setPipeName("");
      setPipeDesc("");
      setPipeErr(null);
      toast({ title: "Release pipeline created", tone: "success" });
    },
    onError: (err) => setPipeErr(formatErrorMessage(err)),
  });

  const deletePipelineMutation = useMutation({
    mutationFn: async (pipelineKey: string) =>
      unwrapApiResult(
        await skotosApi.DELETE("/api/v1/projects/{projectKey}/release-pipelines/{pipelineKey}", {
          params: { path: { projectKey, pipelineKey } },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["release-pipelines", projectKey] });
      setDeletePipe(null);
      toast({ title: "Release pipeline deleted", tone: "success" });
    },
    onError: (err) =>
      toast({ title: "Delete failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const createAppMutation = useMutation({
    mutationFn: async () =>
      unwrapApiResult(
        await skotosApi.POST("/api/v1/applications", {
          body: {
            key: appKey.trim(),
            name: appName.trim(),
            kind: appKind,
            description: appDesc.trim(),
          },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["applications"] });
      setAppOpen(false);
      setAppKey("");
      setAppName("");
      setAppDesc("");
      setAppErr(null);
      toast({ title: "Application registered", tone: "success" });
    },
    onError: (err) => setAppErr(formatErrorMessage(err)),
  });

  const openNewPipeline = () => {
    setPipeErr(null);
    const defaultPhases: ReleasePhase[] =
      environments.length > 0
        ? environments.map((env, idx) => ({
            name: `Roll out to ${env.name}`,
            environmentKey: env.key,
            copyFromPreviousEnv: idx > 0,
            requireApproval: env.key === "production",
            instructions: idx === 0 ? [{ kind: "turnFlagOn" }] : [],
          }))
        : [
            {
              name: "Development",
              environmentKey: "development",
              copyFromPreviousEnv: false,
              requireApproval: false,
              instructions: [{ kind: "turnFlagOn" }],
            },
          ];
    setPhases(defaultPhases);
    setPipeOpen(true);
  };

  const pipelines = pipelinesQuery.data?.items ?? [];
  const apps = appsQuery.data?.items ?? [];

  return (
    <Stack gap={6}>
      <PageHeader
        title={<Heading level={1}>Release Pipelines &amp; Applications</Heading>}
        description={
          <Text tone="muted" size="sm">
            Standardize multi-environment feature promotions across {projectKey} and track
            connected client/server application versions.
          </Text>
        }
        actions={
          <Stack direction="horizontal" gap={2}>
            <Button
              variant="secondary"
              disabled={!adminAllowed}
              onClick={() => {
                setAppErr(null);
                setAppOpen(true);
              }}
            >
              <Icons.Plus size={14} />
              Register App
            </Button>
            <Button variant="primary" disabled={!adminAllowed} onClick={openNewPipeline}>
              <Icons.GitBranch size={14} />
              New Pipeline
            </Button>
          </Stack>
        }
      />

      {pipelinesQuery.isError && (
        <Alert tone="danger" title="Could not load release pipelines">
          {formatErrorMessage(pipelinesQuery.error)}
        </Alert>
      )}

      {pipelinesQuery.isPending ? (
        <Skeleton height={120} />
      ) : (
        <DataTable
          ariaLabel="Release pipelines"
          rows={pipelines}
          rowKey={(p) => p.id}
          emptyTitle="No release pipelines configured"
          emptyDescription="Create a standardized Dev → Staging → Production rollout pipeline for your team."
          columns={[
            {
              key: "name",
              header: "Pipeline",
              render: (p) => (
                <Stack gap={1}>
                  <Text bold>{p.name}</Text>
                  <Text size="xs" mono tone="muted">
                    {p.key}
                  </Text>
                </Stack>
              ),
            },
            {
              key: "phases",
              header: "Promotion Phases",
              render: (p) => (
                <Stack direction="horizontal" align="center" gap={2} style={{ flexWrap: "wrap" }}>
                  {p.phases.map((ph, idx) => (
                    <Badge key={`${ph.environmentKey}-${idx}`} tone={ph.requireApproval ? "warning" : "accent"}>
                      {idx + 1}. {ph.name} ({ph.environmentKey})
                    </Badge>
                  ))}
                </Stack>
              ),
            },
            {
              key: "actions",
              header: "Actions",
              width: "100px",
              render: (p) =>
                adminAllowed ? (
                  <Button
                    size="sm"
                    variant="ghost"
                    aria-label={`Delete pipeline ${p.name}`}
                    onClick={() => setDeletePipe(p)}
                  >
                    <Icons.Trash2 size={14} aria-hidden="true" />
                  </Button>
                ) : null,
            },
          ]}
        />
      )}

      <Separator />

      <Stack gap={3}>
        <Heading level={2}>Applications &amp; Versions</Heading>
        {appsQuery.isPending ? (
          <Skeleton height={100} />
        ) : (
          <DataTable
            ariaLabel="Registered applications"
            rows={apps}
            rowKey={(a) => a.id}
            emptyTitle="No applications registered"
            emptyDescription="Register mobile, browser, or backend apps to track version adoption and targeting."
            columns={[
              {
                key: "app",
                header: "Application",
                render: (a) => (
                  <Stack gap={1}>
                    <Stack direction="horizontal" align="center" gap={2}>
                      <Text bold>{a.name}</Text>
                      <Badge tone="neutral">{a.kind}</Badge>
                    </Stack>
                    <Text size="xs" mono tone="muted">
                      {a.key}
                    </Text>
                  </Stack>
                ),
              },
              {
                key: "versions",
                header: "Tracked Versions",
                render: (a) => (
                  <Stack direction="horizontal" gap={1} style={{ flexWrap: "wrap" }}>
                    {(a.versions ?? []).length === 0 ? (
                      <Text size="xs" tone="muted">
                        No versions reported yet
                      </Text>
                    ) : (
                      (a.versions ?? []).map((v) => (
                        <Badge key={v.id} tone={v.supported ? "success" : "warning"}>
                          {v.versionKey}
                        </Badge>
                      ))
                    )}
                  </Stack>
                ),
              },
              {
                key: "desc",
                header: "Description",
                render: (a) => (
                  <Text size="sm" tone="muted">
                    {a.description || "—"}
                  </Text>
                ),
              },
            ]}
          />
        )}
      </Stack>

      {/* Create Release Pipeline Dialog */}
      <Dialog
        open={pipeOpen}
        onOpenChange={setPipeOpen}
        wide
        title="Create release pipeline"
        description="Define ordered environment phases to safely promote feature flags."
        footer={
          <>
            <Button variant="secondary" onClick={() => setPipeOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={!pipeKey.trim() || !pipeName.trim() || phases.length === 0 || createPipelineMutation.isPending}
              onClick={() => createPipelineMutation.mutate()}
            >
              {createPipelineMutation.isPending ? "Creating..." : "Create pipeline"}
            </Button>
          </>
        }
      >
        <Stack gap={4}>
          {pipeErr && <Alert tone="danger">{pipeErr}</Alert>}
          <FormField label="Pipeline Key" htmlFor="pipe-key" required>
            <Input
              id="pipe-key"
              mono
              placeholder="standard-release"
              value={pipeKey}
              onChange={(e) => setPipeKey(e.target.value)}
            />
          </FormField>
          <FormField label="Pipeline Name" htmlFor="pipe-name" required>
            <Input
              id="pipe-name"
              placeholder="Standard Progressive Rollout"
              value={pipeName}
              onChange={(e) => setPipeName(e.target.value)}
            />
          </FormField>
          <FormField label="Description" htmlFor="pipe-desc">
            <Input
              id="pipe-desc"
              placeholder="Dev -> Production with mandatory approval gate"
              value={pipeDesc}
              onChange={(e) => setPipeDesc(e.target.value)}
            />
          </FormField>

          <Stack gap={2}>
            <Heading level={4}>Phases ({phases.length})</Heading>
            {phases.map((ph, idx) => (
              <Card key={idx} padded>
                <Stack gap={3}>
                  <Stack direction="horizontal" align="center" justify="space-between" gap={2}>
                    <Input
                      aria-label={`Phase ${idx + 1} name`}
                      value={ph.name}
                      onChange={(e) =>
                        setPhases((prev) =>
                          prev.map((item, i) => (i === idx ? { ...item, name: e.target.value } : item)),
                        )
                      }
                    />
                    <Select
                      ariaLabel={`Phase ${idx + 1} environment`}
                      value={ph.environmentKey}
                      options={(environments.length > 0
                        ? environments
                        : [{ key: ph.environmentKey, name: ph.environmentKey }]
                      ).map((e) => ({ value: e.key, label: e.name }))}
                      onValueChange={(val) =>
                        setPhases((prev) =>
                          prev.map((item, i) =>
                            i === idx ? { ...item, environmentKey: val } : item,
                          ),
                        )
                      }
                    />
                  </Stack>
                  <Stack direction="horizontal" gap={4}>
                    <Checkbox
                      checked={Boolean(ph.copyFromPreviousEnv)}
                      label="Copy targeting from previous env"
                      onCheckedChange={(checked) =>
                        setPhases((prev) =>
                          prev.map((item, i) =>
                            i === idx ? { ...item, copyFromPreviousEnv: checked } : item,
                          ),
                        )
                      }
                    />
                    <Checkbox
                      checked={Boolean(ph.requireApproval)}
                      label="Require approval gate"
                      onCheckedChange={(checked) =>
                        setPhases((prev) =>
                          prev.map((item, i) =>
                            i === idx ? { ...item, requireApproval: checked } : item,
                          ),
                        )
                      }
                    />
                  </Stack>
                </Stack>
              </Card>
            ))}
          </Stack>
        </Stack>
      </Dialog>

      {/* Register Application Dialog */}
      <Dialog
        open={appOpen}
        onOpenChange={setAppOpen}
        title="Register application"
        description="Track SDK application versions across mobile, browser, and backend services."
        footer={
          <>
            <Button variant="secondary" onClick={() => setAppOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={!appKey.trim() || !appName.trim() || createAppMutation.isPending}
              onClick={() => createAppMutation.mutate()}
            >
              {createAppMutation.isPending ? "Saving..." : "Register"}
            </Button>
          </>
        }
      >
        <Stack gap={4}>
          {appErr && <Alert tone="danger">{appErr}</Alert>}
          <FormField label="Application Key" htmlFor="app-key" required>
            <Input
              id="app-key"
              mono
              placeholder="checkout-ios"
              value={appKey}
              onChange={(e) => setAppKey(e.target.value)}
            />
          </FormField>
          <FormField label="Name" htmlFor="app-name" required>
            <Input
              id="app-name"
              placeholder="Checkout iOS App"
              value={appName}
              onChange={(e) => setAppName(e.target.value)}
            />
          </FormField>
          <FormField label="Platform Kind" htmlFor="app-kind">
            <Select
              id="app-kind"
              ariaLabel="Application platform kind"
              value={appKind}
              options={[
                { value: "browser", label: "Browser" },
                { value: "mobile", label: "Mobile" },
                { value: "server", label: "Server" },
              ]}
              onValueChange={(v) => setAppKind(v as ApplicationKind)}
            />
          </FormField>
          <FormField label="Description" htmlFor="app-desc">
            <Input
              id="app-desc"
              placeholder="Primary consumer storefront app"
              value={appDesc}
              onChange={(e) => setAppDesc(e.target.value)}
            />
          </FormField>
        </Stack>
      </Dialog>

      <AlertDialog
        open={deletePipe !== null}
        onOpenChange={(o) => !o && setDeletePipe(null)}
        title={`Delete pipeline '${deletePipe?.name ?? ""}'?`}
        description="Existing flags will keep their current environment targeting."
        confirmLabel="Delete pipeline"
        onConfirm={() => deletePipe && deletePipelineMutation.mutate(deletePipe.key)}
      />
    </Stack>
  );
}
