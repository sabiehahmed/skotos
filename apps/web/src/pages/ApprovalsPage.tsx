import {
  Alert,
  Badge,
  Button,
  Card,
  DataTable,
  Dialog,
  DiffViewer,
  FormField,
  Heading,
  Icons,
  PageHeader,
  Select,
  Skeleton,
  Stack,
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
  type ApprovalRequest,
  type ApprovalReviewKind,
  type ApprovalStatus,
  type BuiltInRole,
} from "../lib/api";

export interface ApprovalsPageProps {
  userRole: BuiltInRole;
}

function statusTone(status: ApprovalStatus): "neutral" | "accent" | "success" | "warning" | "danger" {
  switch (status) {
    case "pending":
      return "warning";
    case "approved":
      return "accent";
    case "applied":
      return "success";
    case "rejected":
      return "danger";
    default:
      return "neutral";
  }
}

export function ApprovalsPage({ userRole }: ApprovalsPageProps) {
  const qc = useQueryClient();
  const { toast } = useToast();
  const writerAllowed = canWrite(userRole);

  const [statusFilter, setStatusFilter] = useState<string>("all");
  const [selected, setSelected] = useState<ApprovalRequest | null>(null);
  const [reviewComment, setReviewComment] = useState("");
  const [actionErr, setActionErr] = useState<string | null>(null);

  const approvalsQuery = useQuery({
    queryKey: ["approvals", statusFilter],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/approvals", {
          params: {
            query: statusFilter === "all" ? {} : { status: statusFilter },
          },
        }),
      ),
  });

  const reviewMutation = useMutation({
    mutationFn: async (args: { id: string; kind: ApprovalReviewKind; comment?: string }) =>
      unwrapApiResult(
        await skotosApi.POST("/api/v1/approvals/{approvalId}/reviews", {
          params: { path: { approvalId: args.id } },
          body: { kind: args.kind, comment: args.comment || null },
        }),
      ),
    onSuccess: async (updated) => {
      await qc.invalidateQueries({ queryKey: ["approvals"] });
      setSelected(updated);
      setReviewComment("");
      setActionErr(null);
      toast({ title: `Review recorded (${updated.status})`, tone: "success" });
    },
    onError: (err) => setActionErr(formatErrorMessage(err)),
  });

  const applyMutation = useMutation({
    mutationFn: async (id: string) =>
      unwrapApiResult(
        await skotosApi.POST("/api/v1/approvals/{approvalId}/apply", {
          params: { path: { approvalId: id } },
        }),
      ),
    onSuccess: async (res) => {
      await qc.invalidateQueries({ queryKey: ["approvals"] });
      await qc.invalidateQueries({ queryKey: ["flags"] });
      setSelected(res.approval);
      setActionErr(null);
      toast({
        title: "Approval applied",
        description: `Flag updated to v${res.patch.envConfig.version}`,
        tone: "success",
      });
    },
    onError: (err) => setActionErr(formatErrorMessage(err)),
  });

  const cancelMutation = useMutation({
    mutationFn: async (id: string) =>
      unwrapApiResult(
        await skotosApi.DELETE("/api/v1/approvals/{approvalId}", {
          params: { path: { approvalId: id } },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["approvals"] });
      setSelected(null);
      toast({ title: "Approval canceled", tone: "default" });
    },
    onError: (err) => setActionErr(formatErrorMessage(err)),
  });

  const rows = approvalsQuery.data?.items ?? [];

  return (
    <Stack gap={5}>
      <PageHeader
        title={<Heading level={1}>Approval Requests</Heading>}
        description={
          <Text tone="muted" size="sm">
            Review, approve, or apply governed flag changes before they ship to protected
            environments.
          </Text>
        }
        actions={
          <Select
            ariaLabel="Filter approvals by status"
            value={statusFilter}
            onValueChange={setStatusFilter}
            options={[
              { value: "all", label: "All statuses" },
              { value: "pending", label: "Pending" },
              { value: "approved", label: "Approved" },
              { value: "applied", label: "Applied" },
              { value: "rejected", label: "Rejected" },
              { value: "canceled", label: "Canceled" },
            ]}
          />
        }
      />

      {approvalsQuery.isError && (
        <Alert tone="danger" title="Could not load approval requests">
          {formatErrorMessage(approvalsQuery.error)}
        </Alert>
      )}

      {approvalsQuery.isPending ? (
        <Skeleton height={140} />
      ) : (
        <DataTable
          ariaLabel="Approval requests queue"
          rows={rows}
          rowKey={(a) => a.id}
          emptyTitle="No approval requests"
          emptyDescription="Flag changes submitted for review in protected environments will appear here."
          columns={[
            {
              key: "target",
              header: "Flag & Environment",
              render: (a) => (
                <Stack gap={1}>
                  <Text bold mono>
                    {a.flagKey}
                  </Text>
                  <Text size="xs" tone="muted">
                    {a.projectKey} / {a.envKey}
                  </Text>
                </Stack>
              ),
            },
            {
              key: "description",
              header: "Summary",
              render: (a) => (
                <Stack gap={1}>
                  <Text size="sm">{a.description || a.comment || "Flag change request"}</Text>
                  <Text size="xs" tone="muted">
                    Requested by {a.requestedByEmail ?? "member"} · {a.instructions.length}{" "}
                    instruction(s)
                  </Text>
                </Stack>
              ),
            },
            {
              key: "progress",
              header: "Approvals",
              width: "120px",
              render: (a) => (
                <Badge tone={a.approvalCount >= a.minApprovals ? "success" : "neutral"}>
                  {a.approvalCount} / {a.minApprovals}
                </Badge>
              ),
            },
            {
              key: "status",
              header: "Status",
              width: "110px",
              render: (a) => <Badge tone={statusTone(a.status)}>{a.status}</Badge>,
            },
            {
              key: "actions",
              header: "Actions",
              width: "140px",
              render: (a) => (
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() => {
                    setActionErr(null);
                    setReviewComment("");
                    setSelected(a);
                  }}
                >
                  <Icons.Eye size={14} />
                  Review
                </Button>
              ),
            },
          ]}
        />
      )}

      <Dialog
        open={selected !== null}
        onOpenChange={(open) => !open && setSelected(null)}
        wide
        title={selected ? `Approval — ${selected.flagKey} (${selected.envKey})` : "Approval"}
        description={selected?.description || "Review semantic patch instructions before applying."}
        footer={
          selected && (
            <Stack
              direction="horizontal"
              align="center"
              justify="space-between"
              style={{ width: "100%", flexWrap: "wrap" }}
              gap={2}
            >
              <Stack direction="horizontal" gap={2}>
                {writerAllowed &&
                  (selected.status === "pending" || selected.status === "approved") && (
                    <Button
                      variant="ghost"
                      disabled={cancelMutation.isPending}
                      onClick={() => cancelMutation.mutate(selected.id)}
                    >
                      Cancel Request
                    </Button>
                  )}
              </Stack>
              <Stack direction="horizontal" gap={2}>
                <Button variant="secondary" onClick={() => setSelected(null)}>
                  Close
                </Button>
                {writerAllowed &&
                  (selected.status === "pending" || selected.status === "approved") && (
                    <>
                      <Button
                        variant="danger"
                        disabled={reviewMutation.isPending}
                        onClick={() =>
                          reviewMutation.mutate({
                            id: selected.id,
                            kind: "decline",
                            comment: reviewComment.trim() || undefined,
                          })
                        }
                      >
                        <Icons.XCircle size={14} />
                        Decline
                      </Button>
                      <Button
                        variant="secondary"
                        disabled={reviewMutation.isPending}
                        onClick={() =>
                          reviewMutation.mutate({
                            id: selected.id,
                            kind: "approve",
                            comment: reviewComment.trim() || undefined,
                          })
                        }
                      >
                        <Icons.CheckCircle2 size={14} />
                        Approve
                      </Button>
                    </>
                  )}
                {writerAllowed && selected.status === "approved" && (
                  <Button
                    variant="primary"
                    disabled={applyMutation.isPending}
                    onClick={() => applyMutation.mutate(selected.id)}
                  >
                    <Icons.Play size={14} />
                    {applyMutation.isPending ? "Applying..." : "Apply Changes"}
                  </Button>
                )}
              </Stack>
            </Stack>
          )
        }
      >
        {selected && (
          <Stack gap={4}>
            {actionErr && <Alert tone="danger">{actionErr}</Alert>}

            <Stack direction="horizontal" align="center" gap={2}>
              <Badge tone={statusTone(selected.status)}>{selected.status}</Badge>
              <Badge tone="neutral">
                {selected.approvalCount} / {selected.minApprovals} approvals
              </Badge>
              <Text size="xs" tone="muted">
                {selected.projectKey} / {selected.envKey}
              </Text>
            </Stack>

            <DiffViewer
              summaries={selected.instructions.map((ins) => ({
                kind: ins.kind,
                summary: JSON.stringify(ins),
              }))}
            />

            {(selected.reviews ?? []).length > 0 && (
              <Card padded>
                <Stack gap={2}>
                  <Heading level={4}>Review History</Heading>
                  {(selected.reviews ?? []).map((rev) => (
                    <Stack
                      key={rev.id}
                      direction="horizontal"
                      align="center"
                      justify="space-between"
                    >
                      <Stack direction="horizontal" align="center" gap={2}>
                        <Badge tone={rev.kind === "approve" ? "success" : "danger"}>
                          {rev.kind}
                        </Badge>
                        <Text size="sm">{rev.reviewerEmail ?? rev.reviewerId}</Text>
                        {rev.comment && (
                          <Text size="xs" tone="muted">
                            — {rev.comment}
                          </Text>
                        )}
                      </Stack>
                      <Text size="xs" tone="muted">
                        {new Date(rev.createdAt).toLocaleString()}
                      </Text>
                    </Stack>
                  ))}
                </Stack>
              </Card>
            )}

            {writerAllowed &&
              (selected.status === "pending" || selected.status === "approved") && (
                <FormField label="Review comment (optional)" htmlFor="approval-review-comment">
                  <Textarea
                    id="approval-review-comment"
                    placeholder="Add review context..."
                    value={reviewComment}
                    onChange={(e) => setReviewComment(e.target.value)}
                  />
                </FormField>
              )}
          </Stack>
        )}
      </Dialog>
    </Stack>
  );
}
