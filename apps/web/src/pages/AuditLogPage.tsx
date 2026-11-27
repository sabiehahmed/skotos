import {
  Alert,
  AuditTimeline,
  Button,
  Card,
  Heading,
  Input,
  PageHeader,
  Pagination,
  Select,
  Skeleton,
  Stack,
  Text,
} from "@skotos/ui";
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { formatErrorMessage, skotosApi, unwrapApiResult } from "../lib/api";

export function AuditLogPage() {
  const [resourceType, setResourceType] = useState("all");
  const [resourceKey, setResourceKey] = useState("");
  const [cursorStack, setCursorStack] = useState<(string | undefined)[]>([undefined]);
  const currentCursor = cursorStack[cursorStack.length - 1];

  const auditQuery = useQuery({
    queryKey: ["audit-log", resourceType, resourceKey, currentCursor],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/audit-log", {
          params: {
            query: {
              resourceType: resourceType === "all" ? undefined : resourceType,
              resourceKey: resourceKey.trim() || undefined,
              cursor: currentCursor,
              limit: 25,
            },
          },
        }),
      ),
  });

  const entries = (auditQuery.data?.items ?? []).map((item) => ({
    id: item.id,
    action: item.action,
    resourceType: item.resourceType,
    resourceKey: item.resourceKey,
    actor: item.actorEmail ?? item.actorType,
    timestamp: new Date(item.createdAt).toLocaleString(),
    comment: item.comment,
    before: item.before,
    after: item.after,
  }));

  return (
    <Stack gap={5}>
      <PageHeader
        title={<Heading level={1}>Audit Log</Heading>}
        description={
          <Text tone="muted" size="sm">
            Immutable transactional trail of every control-plane mutation.
          </Text>
        }
      />

      <Card padded>
        <Stack direction="horizontal" gap={3} align="center">
          <Select
            ariaLabel="Filter by resource type"
            value={resourceType}
            onValueChange={(v) => {
              setResourceType(v);
              setCursorStack([undefined]);
            }}
            options={[
              { value: "all", label: "All resources" },
              { value: "flag", label: "Flags" },
              { value: "environment", label: "Environments" },
              { value: "environmentKey", label: "Environment Keys" },
              { value: "project", label: "Projects" },
              { value: "apiToken", label: "API Tokens" },
              { value: "member", label: "Members" },
            ]}
          />
          <Input
            aria-label="Filter by resource key"
            placeholder="Filter by key (e.g. checkout-v2)..."
            value={resourceKey}
            onChange={(e) => {
              setResourceKey(e.target.value);
              setCursorStack([undefined]);
            }}
            style={{ maxWidth: 280 }}
          />
          <Button variant="secondary" onClick={() => auditQuery.refetch()}>
            Refresh
          </Button>
        </Stack>
      </Card>

      {auditQuery.isError && (
        <Alert tone="danger" title="Failed to load audit log">
          {formatErrorMessage(auditQuery.error)}
        </Alert>
      )}

      {auditQuery.isPending ? (
        <Stack gap={3}>
          <Skeleton height={72} />
          <Skeleton height={72} />
        </Stack>
      ) : (
        <>
          <AuditTimeline entries={entries} />
          <Pagination
            hasPrev={cursorStack.length > 1}
            hasNext={Boolean(auditQuery.data?.hasMore && auditQuery.data?.nextCursor)}
            summary={`Page ${cursorStack.length} (${entries.length} entries)`}
            onPrev={() => setCursorStack((prev) => prev.slice(0, -1))}
            onNext={() => {
              const next = auditQuery.data?.nextCursor;
              if (next) setCursorStack((prev) => [...prev, next]);
            }}
          />
        </>
      )}
    </Stack>
  );
}
