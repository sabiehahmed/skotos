import {
  Alert,
  AlertDialog,
  Badge,
  Button,
  Card,
  DataTable,
  Dialog,
  DiffViewer,
  FormField,
  Heading,
  Icons,
  Input,
  PageHeader,
  Select,
  Skeleton,
  Stack,
  Switch,
  TagInput,
  Text,
  Textarea,
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
  type Environment,
  type FlagKind,
  type FlagResponse,
} from "../lib/api";

export interface FlagsListPageProps {
  projectKey: string;
  activeEnv?: Environment;
  userRole: BuiltInRole;
  onOpenFlag: (flagKey: string) => void;
}

export function FlagsListPage({
  projectKey,
  activeEnv,
  userRole,
  onOpenFlag,
}: FlagsListPageProps) {
  const qc = useQueryClient();
  const { toast } = useToast();
  const [search, setSearch] = useState("");
  const [includeArchived, setIncludeArchived] = useState(false);

  // Create flag modal state
  const [createOpen, setCreateOpen] = useState(false);
  const [newKey, setNewKey] = useState("");
  const [newName, setNewName] = useState("");
  const [newDesc, setNewDesc] = useState("");
  const [newKind, setNewKind] = useState<FlagKind>("boolean");
  const [newTags, setNewTags] = useState<string[]>([]);
  const [newTemporary, setNewTemporary] = useState(true);
  const [createError, setCreateError] = useState<string | null>(null);

  // Protected inline toggle confirmation dialog
  const [pendingToggle, setPendingToggle] = useState<{
    flag: FlagResponse;
    nextOn: boolean;
    version: number;
  } | null>(null);
  const [toggleComment, setToggleComment] = useState("");
  const [toggleError, setToggleError] = useState<string | null>(null);

  // Delete confirmation dialog
  const [deleteTarget, setDeleteTarget] = useState<FlagResponse | null>(null);
  const [deleteConfirmText, setDeleteConfirmText] = useState("");

  const flagsQuery = useQuery({
    queryKey: ["flags", projectKey, search, includeArchived],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/flags", {
          params: {
            path: { projectKey },
            query: {
              includeArchived,
              search: search.trim() || undefined,
            },
          },
        }),
      ),
    enabled: Boolean(projectKey),
  });

  const createFlagMutation = useMutation({
    mutationFn: async () => {
      const variations =
        newKind === "boolean"
          ? [
              { value: true, name: "Enabled" },
              { value: false, name: "Disabled" },
            ]
          : newKind === "string"
            ? [
                { value: "variant-a", name: "Variant A" },
                { value: "control", name: "Control" },
              ]
            : newKind === "number"
              ? [
                  { value: 1, name: "High" },
                  { value: 0, name: "Default" },
                ]
              : [
                  { value: { enabled: true }, name: "Enabled Config" },
                  { value: { enabled: false }, name: "Default Config" },
                ];

      return unwrapApiResult(
        await skotosApi.POST("/api/v1/projects/{projectKey}/flags", {
          params: { path: { projectKey } },
          body: {
            key: newKey.trim(),
            name: newName.trim(),
            description: newDesc.trim(),
            kind: newKind,
            variations,
            temporary: newTemporary,
            tags: newTags,
          },
        }),
      );
    },
    onSuccess: async (created) => {
      await qc.invalidateQueries({ queryKey: ["flags", projectKey] });
      setCreateOpen(false);
      setNewKey("");
      setNewName("");
      setNewDesc("");
      setNewTags([]);
      setCreateError(null);
      toast({ title: `Created flag '${created.key}'`, tone: "success" });
    },
    onError: (err) => setCreateError(formatErrorMessage(err)),
  });

  const toggleMutation = useMutation({
    mutationFn: async (args: {
      flagKey: string;
      envKey: string;
      nextOn: boolean;
      version: number;
      comment?: string;
    }) =>
      unwrapApiResult(
        await skotosApi.PATCH("/api/v1/projects/{projectKey}/flags/{flagKey}", {
          params: { path: { projectKey, flagKey: args.flagKey } },
          body: {
            environmentKey: args.envKey,
            expectedVersion: args.version,
            comment: args.comment,
            instructions: [{ kind: args.nextOn ? "turnFlagOn" : "turnFlagOff" }],
          },
        }),
      ),
    onSuccess: async (res) => {
      await qc.invalidateQueries({ queryKey: ["flags", projectKey] });
      setPendingToggle(null);
      setToggleComment("");
      setToggleError(null);
      toast({
        title: `${res.flag.key} is now ${res.envConfig.on ? "ON" : "OFF"}`,
        tone: "success",
      });
    },
    onError: (err) => {
      const msg = formatErrorMessage(err);
      setToggleError(msg);
      toast({ title: "Toggle failed", description: msg, tone: "danger" });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: async (flagKey: string) =>
      unwrapApiResult(
        await skotosApi.DELETE("/api/v1/projects/{projectKey}/flags/{flagKey}", {
          params: { path: { projectKey, flagKey } },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["flags", projectKey] });
      setDeleteTarget(null);
      setDeleteConfirmText("");
      toast({ title: "Flag deleted", tone: "success" });
    },
    onError: (err) => toast({ title: "Delete failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const handleInlineToggle = (flag: FlagResponse, nextOn: boolean) => {
    if (!activeEnv) return;
    const cfg = flag.environments[activeEnv.key];
    const version = cfg?.version ?? 1;
    if (activeEnv.confirmChanges || activeEnv.requireComments) {
      setToggleComment("");
      setToggleError(null);
      setPendingToggle({ flag, nextOn, version });
    } else {
      toggleMutation.mutate({
        flagKey: flag.key,
        envKey: activeEnv.key,
        nextOn,
        version,
      });
    }
  };

  const rows = flagsQuery.data?.items ?? [];
  const writerAllowed = canWrite(userRole);
  const adminAllowed = canAdmin(userRole);

  return (
    <Stack gap={5}>
      <PageHeader
        title={<Heading level={1}>Feature Flags</Heading>}
        description={
          <Text tone="muted" size="sm">
            Manage targeting rules, progressive rollouts, and instant kill switches.
          </Text>
        }
        actions={
          <Button
            variant="primary"
            disabled={!writerAllowed}
            onClick={() => {
              setCreateError(null);
              setCreateOpen(true);
            }}
          >
            <Icons.Plus size={15} />
            Create flag
          </Button>
        }
      />

      <Card padded>
        <Stack direction="horizontal" gap={4} align="center" justify="space-between">
          <Input
            aria-label="Search flags"
            placeholder="Search flags by key or name..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            style={{ maxWidth: 340 }}
          />
          <Switch
            checked={includeArchived}
            onCheckedChange={setIncludeArchived}
            label="Show archived"
          />
        </Stack>
      </Card>

      {flagsQuery.isError && (
        <Alert tone="danger" title="Could not load flags">
          {formatErrorMessage(flagsQuery.error)}
        </Alert>
      )}

      {flagsQuery.isPending ? (
        <Stack gap={3}>
          <Skeleton height={54} />
          <Skeleton height={54} />
        </Stack>
      ) : (
        <DataTable
          ariaLabel="Feature flags list"
          rows={rows}
          rowKey={(f) => f.id}
          emptyTitle="No feature flags yet"
          emptyDescription="Create your first feature flag to decouple deploy from release."
          emptyAction={
            writerAllowed ? (
              <Button variant="primary" onClick={() => setCreateOpen(true)}>
                Create flag
              </Button>
            ) : undefined
          }
          columns={[
            {
              key: "flag",
              header: "Flag",
              render: (f) => (
                <Stack gap={1}>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => onOpenFlag(f.key)}
                    style={{ justifyContent: "flex-start", paddingLeft: 0 }}
                  >
                    <Text as="span" bold>
                      {f.name}
                    </Text>
                  </Button>
                  <Stack direction="horizontal" gap={2} align="center">
                    <Text as="span" size="xs" mono tone="muted">
                      {f.key}
                    </Text>
                    <Badge tone="neutral">{f.kind}</Badge>
                    {f.archived && <Badge tone="warning">Archived</Badge>}
                    {(f.tags ?? []).map((t) => (
                      <Badge key={t} tone="accent">
                        {t}
                      </Badge>
                    ))}
                  </Stack>
                </Stack>
              ),
            },
            {
              key: "targeting",
              header: activeEnv ? `Targeting (${activeEnv.name})` : "Targeting",
              width: "200px",
              render: (f) => {
                const envCfg = activeEnv ? f.environments[activeEnv.key] : undefined;
                const isOn = envCfg?.on ?? false;
                return (
                  <Switch
                    checked={isOn}
                    disabled={!writerAllowed || !activeEnv || f.archived}
                    ariaLabel={`Toggle ${f.key} targeting`}
                    data-testid={`flag-toggle-${f.key}`}
                    label={isOn ? "ON" : "OFF"}
                    onCheckedChange={(next) => handleInlineToggle(f, next)}
                  />
                );
              },
            },
            {
              key: "version",
              header: "Version",
              width: "110px",
              render: (f) => {
                const envCfg = activeEnv ? f.environments[activeEnv.key] : undefined;
                return (
                  <Text as="span" size="xs" mono tone="muted">
                    v{envCfg?.version ?? 1}
                  </Text>
                );
              },
            },
            {
              key: "actions",
              header: "Actions",
              width: "180px",
              render: (f) => (
                <Stack direction="horizontal" gap={2}>
                  <Button size="sm" variant="secondary" onClick={() => onOpenFlag(f.key)}>
                    Edit
                  </Button>
                  {adminAllowed && (
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={`Delete flag ${f.key}`}
                      onClick={() => {
                        setDeleteConfirmText("");
                        setDeleteTarget(f);
                      }}
                    >
                      <Icons.Trash2 size={14} aria-hidden="true" />
                    </Button>
                  )}
                </Stack>
              ),
            },
          ]}
        />
      )}

      {/* Quick Create Flag Dialog */}
      <Dialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        title="Create Feature Flag"
        description={`Creates a new flag across all environments in project '${projectKey}'.`}
        footer={
          <>
            <Button variant="secondary" onClick={() => setCreateOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={createFlagMutation.isPending || !newKey.trim() || !newName.trim()}
              onClick={() => createFlagMutation.mutate()}
            >
              {createFlagMutation.isPending ? "Creating..." : "Create flag"}
            </Button>
          </>
        }
      >
        <Stack gap={4}>
          {createError && <Alert tone="danger">{createError}</Alert>}
          <FormField label="Name" htmlFor="new-flag-name" required>
            <Input
              id="new-flag-name"
              placeholder="New Checkout Flow"
              value={newName}
              onChange={(e) => {
                const val = e.target.value;
                setNewName(val);
                if (!newKey) {
                  setNewKey(
                    val
                      .toLowerCase()
                      .replace(/[^a-z0-9]+/g, "-")
                      .replace(/^-|-$/g, ""),
                  );
                }
              }}
            />
          </FormField>
          <FormField
            label="Key"
            htmlFor="new-flag-key"
            required
            hint="Used in SDK calls; letters, digits, '.', '_', '-'"
          >
            <Input
              id="new-flag-key"
              mono
              placeholder="new-checkout-flow"
              value={newKey}
              onChange={(e) => setNewKey(e.target.value)}
            />
          </FormField>
          <FormField label="Kind" htmlFor="new-flag-kind">
            <Select
              id="new-flag-kind"
              value={newKind}
              onValueChange={(v) => setNewKind(v as FlagKind)}
              options={[
                { value: "boolean", label: "Boolean (true / false)" },
                { value: "string", label: "String (multivariate)" },
                { value: "number", label: "Number (multivariate)" },
                { value: "json", label: "JSON (multivariate)" },
              ]}
            />
          </FormField>
          <FormField label="Description" htmlFor="new-flag-desc">
            <Textarea
              id="new-flag-desc"
              placeholder="What does this flag control?"
              value={newDesc}
              onChange={(e) => setNewDesc(e.target.value)}
            />
          </FormField>
          <FormField label="Tags">
            <TagInput values={newTags} onChange={setNewTags} ariaLabel="Flag tags" />
          </FormField>
          <Switch
            checked={newTemporary}
            onCheckedChange={setNewTemporary}
            label="Temporary release flag"
          />
        </Stack>
      </Dialog>

      {/* Protected Environment Change Review Dialog */}
      <Dialog
        open={Boolean(pendingToggle)}
        onOpenChange={(open) => {
          if (!open) setPendingToggle(null);
        }}
        title="Review Protected Environment Change"
        description={
          activeEnv
            ? `Confirm changes to '${pendingToggle?.flag.key}' in ${activeEnv.name}.`
            : undefined
        }
        footer={
          <>
            <Button variant="secondary" onClick={() => setPendingToggle(null)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={
                toggleMutation.isPending ||
                Boolean(activeEnv?.requireComments && !toggleComment.trim())
              }
              onClick={() => {
                if (pendingToggle && activeEnv) {
                  toggleMutation.mutate({
                    flagKey: pendingToggle.flag.key,
                    envKey: activeEnv.key,
                    nextOn: pendingToggle.nextOn,
                    version: pendingToggle.version,
                    comment: toggleComment.trim() || undefined,
                  });
                }
              }}
            >
              Confirm change
            </Button>
          </>
        }
      >
        <Stack gap={4}>
          {toggleError && <Alert tone="danger">{toggleError}</Alert>}
          {pendingToggle && (
            <DiffViewer
              summaries={[
                {
                  kind: pendingToggle.nextOn ? "turnFlagOn" : "turnFlagOff",
                  summary: `Turn targeting ${pendingToggle.nextOn ? "ON" : "OFF"} for '${pendingToggle.flag.key}'`,
                },
              ]}
            />
          )}
          <FormField
            label="Change comment"
            htmlFor="toggle-comment"
            required={activeEnv?.requireComments}
            hint={
              activeEnv?.requireComments
                ? "Required for this protected environment"
                : "Optional audit log note"
            }
          >
            <Textarea
              id="toggle-comment"
              placeholder="Explain why you are toggling this flag..."
              value={toggleComment}
              onChange={(e) => setToggleComment(e.target.value)}
            />
          </FormField>
        </Stack>
      </Dialog>

      {/* Typed-key Delete AlertDialog */}
      <AlertDialog
        open={Boolean(deleteTarget)}
        onOpenChange={(open) => {
          if (!open) setDeleteTarget(null);
        }}
        title={`Delete flag '${deleteTarget?.key ?? ""}'?`}
        description="This permanently deletes the flag across all environments and cannot be undone."
        confirmLabel="Delete flag"
        confirmDisabled={
          deleteMutation.isPending || deleteConfirmText.trim() !== (deleteTarget?.key ?? "")
        }
        onConfirm={() => {
          if (deleteTarget) deleteMutation.mutate(deleteTarget.key);
        }}
      >
        <FormField
          label={`Type '${deleteTarget?.key ?? ""}' to confirm`}
          htmlFor="confirm-delete-flag"
        >
          <Input
            id="confirm-delete-flag"
            mono
            value={deleteConfirmText}
            onChange={(e) => setDeleteConfirmText(e.target.value)}
          />
        </FormField>
      </AlertDialog>
    </Stack>
  );
}
