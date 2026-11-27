import {
  Alert,
  AlertDialog,
  Badge,
  Button,
  DataTable,
  Dialog,
  FormField,
  Heading,
  Icons,
  Input,
  PageHeader,
  Skeleton,
  Stack,
  TagInput,
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
  type Environment,
  type Segment,
} from "../lib/api";

export interface SegmentsPageProps {
  projectKey: string;
  activeEnv?: Environment;
  userRole: BuiltInRole;
}

export function SegmentsPage({ projectKey, activeEnv, userRole }: SegmentsPageProps) {
  const qc = useQueryClient();
  const { toast } = useToast();
  const envKey = activeEnv?.key ?? "";
  const writerAllowed = canWrite(userRole);
  const adminAllowed = canAdmin(userRole);

  const [createOpen, setCreateOpen] = useState(false);
  const [segName, setSegName] = useState("");
  const [segKey, setSegKey] = useState("");
  const [segIncluded, setSegIncluded] = useState<string[]>([]);
  const [segExcluded, setSegExcluded] = useState<string[]>([]);
  const [formErr, setFormErr] = useState<string | null>(null);

  const [editSeg, setEditSeg] = useState<Segment | null>(null);
  const [editName, setEditName] = useState("");
  const [editIncluded, setEditIncluded] = useState<string[]>([]);
  const [editExcluded, setEditExcluded] = useState<string[]>([]);
  const [deleteTarget, setDeleteTarget] = useState<Segment | null>(null);

  const segmentsQuery = useQuery({
    queryKey: ["segments", projectKey, envKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/environments/{envKey}/segments", {
          params: { path: { projectKey, envKey } },
        }),
      ),
    enabled: Boolean(projectKey && envKey),
  });

  const createMutation = useMutation({
    mutationFn: async () =>
      unwrapApiResult(
        await skotosApi.POST("/api/v1/projects/{projectKey}/environments/{envKey}/segments", {
          params: { path: { projectKey, envKey } },
          body: {
            key: segKey.trim(),
            name: segName.trim(),
            included: segIncluded,
            excluded: segExcluded,
          },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["segments", projectKey, envKey] });
      setCreateOpen(false);
      setSegName("");
      setSegKey("");
      setSegIncluded([]);
      setSegExcluded([]);
      setFormErr(null);
      toast({ title: "Segment created", tone: "success" });
    },
    onError: (err) => setFormErr(formatErrorMessage(err)),
  });

  const patchMutation = useMutation({
    mutationFn: async (seg: Segment) => {
      const prevInc = seg.included ?? [];
      const prevExc = seg.excluded ?? [];
      const addInc = editIncluded.filter((k) => !prevInc.includes(k));
      const remInc = prevInc.filter((k) => !editIncluded.includes(k));
      const addExc = editExcluded.filter((k) => !prevExc.includes(k));
      const remExc = prevExc.filter((k) => !editExcluded.includes(k));

      const instructions: Array<
        | { kind: "updateName"; name: string }
        | { kind: "addIncluded"; contextKind: string; values: string[] }
        | { kind: "removeIncluded"; contextKind: string; values: string[] }
        | { kind: "addExcluded"; contextKind: string; values: string[] }
        | { kind: "removeExcluded"; contextKind: string; values: string[] }
      > = [];

      if (editName.trim() !== seg.name) {
        instructions.push({ kind: "updateName", name: editName.trim() });
      }
      if (addInc.length > 0) {
        instructions.push({ kind: "addIncluded", contextKind: "user", values: addInc });
      }
      if (remInc.length > 0) {
        instructions.push({ kind: "removeIncluded", contextKind: "user", values: remInc });
      }
      if (addExc.length > 0) {
        instructions.push({ kind: "addExcluded", contextKind: "user", values: addExc });
      }
      if (remExc.length > 0) {
        instructions.push({ kind: "removeExcluded", contextKind: "user", values: remExc });
      }

      return unwrapApiResult(
        await skotosApi.PATCH(
          "/api/v1/projects/{projectKey}/environments/{envKey}/segments/{segmentKey}",
          {
            params: { path: { projectKey, envKey, segmentKey: seg.key } },
            body: {
              expectedVersion: seg.version,
              comment: `Updated segment ${seg.key}`,
              instructions,
            },
          },
        ),
      );
    },
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["segments", projectKey, envKey] });
      setEditSeg(null);
      toast({ title: "Segment updated", tone: "success" });
    },
    onError: (err) =>
      toast({ title: "Update failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const deleteMutation = useMutation({
    mutationFn: async (segmentKey: string) =>
      unwrapApiResult(
        await skotosApi.DELETE(
          "/api/v1/projects/{projectKey}/environments/{envKey}/segments/{segmentKey}",
          {
            params: { path: { projectKey, envKey, segmentKey } },
          },
        ),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["segments", projectKey, envKey] });
      setDeleteTarget(null);
      toast({ title: "Segment deleted", tone: "success" });
    },
    onError: (err) =>
      toast({ title: "Delete failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const rows = segmentsQuery.data?.items ?? [];

  return (
    <Stack gap={5}>
      <PageHeader
        title={<Heading level={1}>Segments</Heading>}
        description={
          <Text tone="muted" size="sm">
            Reusable user and context cohorts evaluated via segmentMatch rules in{" "}
            {activeEnv?.name ?? envKey}.
          </Text>
        }
        actions={
          <Button
            variant="primary"
            disabled={!writerAllowed || !envKey}
            onClick={() => {
              setFormErr(null);
              setCreateOpen(true);
            }}
          >
            <Icons.Plus size={15} />
            Create segment
          </Button>
        }
      />

      {segmentsQuery.isError && (
        <Alert tone="danger" title="Could not load segments">
          {formatErrorMessage(segmentsQuery.error)}
        </Alert>
      )}

      {segmentsQuery.isPending ? (
        <Stack gap={3}>
          <Skeleton height={54} />
          <Skeleton height={54} />
        </Stack>
      ) : (
        <DataTable
          ariaLabel="Segments list"
          rows={rows}
          rowKey={(s) => s.id}
          emptyTitle="No segments in this environment"
          emptyDescription="Create reusable cohorts like Beta Users or Enterprise Orgs to target across flags."
          columns={[
            {
              key: "segment",
              header: "Segment",
              render: (s) => (
                <Stack gap={1}>
                  <Text bold>{s.name}</Text>
                  <Stack direction="horizontal" gap={2} align="center">
                    <Text as="span" size="xs" mono tone="muted">
                      {s.key}
                    </Text>
                    <Badge tone="neutral">{s.kind}</Badge>
                  </Stack>
                </Stack>
              ),
            },
            {
              key: "members",
              header: "Included / Excluded",
              render: (s) => (
                <Stack direction="horizontal" gap={2}>
                  <Badge tone="success">+{(s.included ?? []).length} included</Badge>
                  <Badge tone="warning">-{(s.excluded ?? []).length} excluded</Badge>
                  <Badge tone="accent">{(s.rules ?? []).length} rules</Badge>
                </Stack>
              ),
            },
            {
              key: "version",
              header: "Version",
              width: "100px",
              render: (s) => (
                <Text as="span" size="xs" mono tone="muted">
                  v{s.version}
                </Text>
              ),
            },
            {
              key: "actions",
              header: "Actions",
              width: "160px",
              render: (s) => (
                <Stack direction="horizontal" gap={2}>
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={!writerAllowed}
                    onClick={() => {
                      setEditSeg(s);
                      setEditName(s.name);
                      setEditIncluded([...(s.included ?? [])]);
                      setEditExcluded([...(s.excluded ?? [])]);
                    }}
                  >
                    Edit
                  </Button>
                  {adminAllowed && (
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={`Delete segment ${s.key}`}
                      onClick={() => setDeleteTarget(s)}
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

      <Dialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        title="Create segment"
        description={`Create a reusable targeting segment in ${activeEnv?.name ?? envKey}.`}
        footer={
          <>
            <Button variant="secondary" onClick={() => setCreateOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={!segKey.trim() || !segName.trim() || createMutation.isPending}
              onClick={() => createMutation.mutate()}
            >
              {createMutation.isPending ? "Creating..." : "Create segment"}
            </Button>
          </>
        }
      >
        <Stack gap={4}>
          {formErr && <Alert tone="danger">{formErr}</Alert>}
          <FormField label="Name" htmlFor="seg-name" required>
            <Input
              id="seg-name"
              placeholder="Beta Users"
              value={segName}
              onChange={(e) => {
                setSegName(e.target.value);
                if (!segKey) {
                  setSegKey(
                    e.target.value
                      .toLowerCase()
                      .replace(/[^a-z0-9]+/g, "-")
                      .replace(/^-|-$/g, ""),
                  );
                }
              }}
            />
          </FormField>
          <FormField label="Key" htmlFor="seg-key" required>
            <Input
              id="seg-key"
              mono
              placeholder="beta-users"
              value={segKey}
              onChange={(e) => setSegKey(e.target.value)}
            />
          </FormField>
          <FormField label="Included user keys">
            <TagInput
              ariaLabel="Included user keys"
              values={segIncluded}
              onChange={setSegIncluded}
              placeholder="Add user key and press Enter..."
            />
          </FormField>
          <FormField label="Excluded user keys">
            <TagInput
              ariaLabel="Excluded user keys"
              values={segExcluded}
              onChange={setSegExcluded}
              placeholder="Add excluded user key..."
            />
          </FormField>
        </Stack>
      </Dialog>

      <Dialog
        open={editSeg !== null}
        onOpenChange={(o) => !o && setEditSeg(null)}
        title={`Edit segment '${editSeg?.key ?? ""}'`}
        description="Update included/excluded keys using semantic segment patches."
        footer={
          <>
            <Button variant="secondary" onClick={() => setEditSeg(null)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={!editSeg || patchMutation.isPending}
              onClick={() => editSeg && patchMutation.mutate(editSeg)}
            >
              {patchMutation.isPending ? "Saving..." : "Save segment"}
            </Button>
          </>
        }
      >
        <Stack gap={4}>
          <FormField label="Name" htmlFor="edit-seg-name">
            <Input
              id="edit-seg-name"
              value={editName}
              onChange={(e) => setEditName(e.target.value)}
            />
          </FormField>
          <FormField label="Included user keys">
            <TagInput
              ariaLabel="Edit included user keys"
              values={editIncluded}
              onChange={setEditIncluded}
            />
          </FormField>
          <FormField label="Excluded user keys">
            <TagInput
              ariaLabel="Edit excluded user keys"
              values={editExcluded}
              onChange={setEditExcluded}
            />
          </FormField>
        </Stack>
      </Dialog>

      <AlertDialog
        open={deleteTarget !== null}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
        title={`Delete segment '${deleteTarget?.key ?? ""}'?`}
        description="Flags referencing this segment will evaluate segmentMatch as false."
        confirmLabel="Delete segment"
        onConfirm={() => deleteTarget && deleteMutation.mutate(deleteTarget.key)}
      />
    </Stack>
  );
}
