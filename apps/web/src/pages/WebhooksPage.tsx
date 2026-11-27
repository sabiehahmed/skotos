import {
  Alert,
  AlertDialog,
  Badge,
  Button,
  CodeBlock,
  DataTable,
  Dialog,
  FormField,
  Heading,
  Icons,
  Input,
  PageHeader,
  Skeleton,
  Stack,
  Switch,
  TagInput,
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
  type BuiltInRole,
  type Webhook,
} from "../lib/api";

export interface WebhooksPageProps {
  userRole: BuiltInRole;
}

export function WebhooksPage({ userRole }: WebhooksPageProps) {
  const qc = useQueryClient();
  const { toast } = useToast();
  const adminAllowed = canAdmin(userRole);

  const [createOpen, setCreateOpen] = useState(false);
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [events, setEvents] = useState<string[]>(["flag.updated", "flag.deleted"]);
  const [envFilter, setEnvFilter] = useState<string[]>([]);
  const [createdSecret, setCreatedSecret] = useState<string | null>(null);
  const [formErr, setFormErr] = useState<string | null>(null);

  const [deliveriesWebhook, setDeliveriesWebhook] = useState<Webhook | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Webhook | null>(null);

  const webhooksQuery = useQuery({
    queryKey: ["webhooks"],
    queryFn: async () => unwrapApiResult(await skotosApi.GET("/api/v1/webhooks")),
  });

  const deliveriesQuery = useQuery({
    queryKey: ["webhook-deliveries", deliveriesWebhook?.id],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/webhooks/{webhookId}/deliveries", {
          params: { path: { webhookId: deliveriesWebhook!.id } },
        }),
      ),
    enabled: Boolean(deliveriesWebhook),
  });

  const createMutation = useMutation({
    mutationFn: async () =>
      unwrapApiResult(
        await skotosApi.POST("/api/v1/webhooks", {
          body: {
            name: name.trim(),
            url: url.trim(),
            events,
            envFilter,
          },
        }),
      ),
    onSuccess: async (res) => {
      await qc.invalidateQueries({ queryKey: ["webhooks"] });
      setCreatedSecret(res.secret);
      setName("");
      setUrl("");
      setFormErr(null);
      toast({ title: "Webhook created", tone: "success" });
    },
    onError: (err) => setFormErr(formatErrorMessage(err)),
  });

  const toggleMutation = useMutation({
    mutationFn: async (args: { id: string; enabled: boolean }) =>
      unwrapApiResult(
        await skotosApi.PATCH("/api/v1/webhooks/{webhookId}", {
          params: { path: { webhookId: args.id } },
          body: { enabled: args.enabled },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["webhooks"] });
    },
    onError: (err) =>
      toast({ title: "Update failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const deleteMutation = useMutation({
    mutationFn: async (id: string) =>
      unwrapApiResult(
        await skotosApi.DELETE("/api/v1/webhooks/{webhookId}", {
          params: { path: { webhookId: id } },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["webhooks"] });
      setDeleteTarget(null);
      toast({ title: "Webhook deleted", tone: "success" });
    },
    onError: (err) =>
      toast({ title: "Delete failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const rows = webhooksQuery.data?.items ?? [];

  return (
    <Stack gap={5}>
      <PageHeader
        title={<Heading level={1}>Webhooks</Heading>}
        description={
          <Text tone="muted" size="sm">
            Receive HMAC-SHA256 signed HTTP POST notifications (X-Skotos-Signature) on flag and
            segment changes.
          </Text>
        }
        actions={
          <Button
            variant="primary"
            disabled={!adminAllowed}
            onClick={() => {
              setCreatedSecret(null);
              setFormErr(null);
              setCreateOpen(true);
            }}
          >
            <Icons.Plus size={15} />
            Add webhook
          </Button>
        }
      />

      {webhooksQuery.isError && (
        <Alert tone="danger" title="Could not load webhooks">
          {formatErrorMessage(webhooksQuery.error)}
        </Alert>
      )}

      {webhooksQuery.isPending ? (
        <Skeleton height={120} />
      ) : (
        <DataTable
          ariaLabel="Outbound webhooks"
          rows={rows}
          rowKey={(w) => w.id}
          emptyTitle="No webhooks configured"
          emptyDescription="Connect Slack, Datadog, or custom CI pipelines to Skotos flag updates."
          columns={[
            {
              key: "name",
              header: "Endpoint",
              render: (w) => (
                <Stack gap={1}>
                  <Text bold>{w.name}</Text>
                  <Text size="xs" mono tone="muted">
                    {w.url}
                  </Text>
                </Stack>
              ),
            },
            {
              key: "events",
              header: "Events",
              render: (w) => (
                <Stack direction="horizontal" gap={1}>
                  {(w.events ?? []).length === 0 ? (
                    <Badge tone="neutral">All events</Badge>
                  ) : (
                    (w.events ?? []).map((ev) => (
                      <Badge key={ev} tone="accent">
                        {ev}
                      </Badge>
                    ))
                  )}
                </Stack>
              ),
            },
            {
              key: "enabled",
              header: "Status",
              width: "120px",
              render: (w) => (
                <Switch
                  checked={w.enabled}
                  disabled={!adminAllowed}
                  ariaLabel={`Toggle webhook ${w.name}`}
                  label={w.enabled ? "Active" : "Paused"}
                  onCheckedChange={(enabled) => toggleMutation.mutate({ id: w.id, enabled })}
                />
              ),
            },
            {
              key: "actions",
              header: "Actions",
              width: "180px",
              render: (w) => (
                <Stack direction="horizontal" gap={2}>
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => setDeliveriesWebhook(w)}
                  >
                    Deliveries
                  </Button>
                  {adminAllowed && (
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={`Delete webhook ${w.name}`}
                      onClick={() => setDeleteTarget(w)}
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
        title="Add signed webhook"
        description="Skotos signs every webhook request with HMAC-SHA256 in the X-Skotos-Signature header."
        footer={
          createdSecret ? (
            <Button variant="primary" onClick={() => setCreateOpen(false)}>
              Done
            </Button>
          ) : (
            <>
              <Button variant="secondary" onClick={() => setCreateOpen(false)}>
                Cancel
              </Button>
              <Button
                variant="primary"
                disabled={!name.trim() || !url.trim() || createMutation.isPending}
                onClick={() => createMutation.mutate()}
              >
                {createMutation.isPending ? "Creating..." : "Create webhook"}
              </Button>
            </>
          )
        }
      >
        {createdSecret ? (
          <Stack gap={3}>
            <Alert tone="success" title="Save your signing secret">
              This HMAC signing secret is shown once. Verify incoming requests using
              X-Skotos-Signature.
            </Alert>
            <CodeBlock label="Signing Secret" code={createdSecret} />
          </Stack>
        ) : (
          <Stack gap={4}>
            {formErr && <Alert tone="danger">{formErr}</Alert>}
            <FormField label="Name" htmlFor="wh-name" required>
              <Input
                id="wh-name"
                placeholder="Production Slack Alerts"
                value={name}
                onChange={(e) => setName(e.target.value)}
              />
            </FormField>
            <FormField label="Target URL" htmlFor="wh-url" required>
              <Input
                id="wh-url"
                mono
                placeholder="https://hooks.example.com/skotos"
                value={url}
                onChange={(e) => setUrl(e.target.value)}
              />
            </FormField>
            <FormField label="Event filter (leave empty for all)">
              <TagInput ariaLabel="Webhook events" values={events} onChange={setEvents} />
            </FormField>
            <FormField label="Environment keys filter (optional)">
              <TagInput
                ariaLabel="Webhook environment keys"
                values={envFilter}
                onChange={setEnvFilter}
                placeholder="e.g. production"
              />
            </FormField>
          </Stack>
        )}
      </Dialog>

      <Dialog
        open={deliveriesWebhook !== null}
        onOpenChange={(o) => !o && setDeliveriesWebhook(null)}
        wide
        title={`Recent deliveries — ${deliveriesWebhook?.name ?? ""}`}
        description="Latest signed HTTP POST attempts recorded by the Skotos outbox worker."
        footer={
          <Button variant="secondary" onClick={() => setDeliveriesWebhook(null)}>
            Close
          </Button>
        }
      >
        {deliveriesQuery.isPending ? (
          <Skeleton height={80} />
        ) : (
          <DataTable
            ariaLabel="Webhook delivery history"
            rows={deliveriesQuery.data?.items ?? []}
            rowKey={(d) => d.id}
            emptyTitle="No deliveries yet"
            emptyDescription="Deliveries will appear here once matching flag or segment events fire."
            columns={[
              { key: "event", header: "Event", render: (d) => <Badge tone="accent">{d.eventType}</Badge> },
              {
                key: "status",
                header: "Result",
                render: (d) => (
                  <Badge tone={d.success ? "success" : "danger"}>
                    {d.statusCode ? `HTTP ${d.statusCode}` : d.errorMessage ?? "Error"}
                  </Badge>
                ),
              },
              { key: "attempt", header: "Attempt", render: (d) => <Text size="xs">#{d.attempt}</Text> },
              {
                key: "time",
                header: "Delivered At",
                render: (d) => (
                  <Text size="xs" tone="muted">
                    {new Date(d.deliveredAt).toLocaleString()}
                  </Text>
                ),
              },
            ]}
          />
        )}
      </Dialog>

      <AlertDialog
        open={deleteTarget !== null}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
        title={`Delete webhook '${deleteTarget?.name ?? ""}'?`}
        description="Skotos will immediately stop sending event notifications to this endpoint."
        confirmLabel="Delete webhook"
        onConfirm={() => deleteTarget && deleteMutation.mutate(deleteTarget.id)}
      />
    </Stack>
  );
}
