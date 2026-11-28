import {
  Alert,
  Badge,
  Button,
  Card,
  DataTable,
  Heading,
  Icons,
  Input,
  PageHeader,
  Stack,
  StatusDot,
  Text,
} from "@skotos/ui";
import { useEffect, useMemo, useRef, useState } from "react";
import { API_BASE_URL, type Environment, type LiveEvalDebugEvent } from "../lib/api";

export interface LiveDebuggerPageProps {
  projectKey: string;
  activeEnv?: Environment;
}

interface DebugEventRow extends LiveEvalDebugEvent {
  rowId: string;
}

export function LiveDebuggerPage({ projectKey, activeEnv }: LiveDebuggerPageProps) {
  const envKey = activeEnv?.key ?? "";
  const [paused, setPaused] = useState(false);
  const [filterFlag, setFilterFlag] = useState("");
  const [status, setStatus] = useState<"connecting" | "connected" | "disconnected">("disconnected");
  const [events, setEvents] = useState<DebugEventRow[]>([]);
  const pausedRef = useRef(paused);
  pausedRef.current = paused;

  useEffect(() => {
    if (!envKey || typeof EventSource === "undefined") {
      setStatus("disconnected");
      return;
    }

    const url = `${API_BASE_URL.replace(/\/+$/, "")}/api/v1/projects/${encodeURIComponent(
      projectKey,
    )}/environments/${encodeURIComponent(envKey)}/live-events`;

    setStatus("connecting");
    const es = new EventSource(url, { withCredentials: true });

    es.onopen = () => setStatus("connected");
    es.onerror = () => setStatus("disconnected");
    es.addEventListener("debug", (ev) => {
      if (pausedRef.current) return;
      try {
        const parsed = JSON.parse((ev as MessageEvent<string>).data) as LiveEvalDebugEvent;
        setEvents((prev) => [
          { ...parsed, rowId: `${parsed.timestamp}-${Math.random().toString(36).slice(2, 8)}` },
          ...prev.slice(0, 199),
        ]);
      } catch {
        // Ignore non-JSON keepalive frames
      }
    });

    return () => {
      es.close();
      setStatus("disconnected");
    };
  }, [projectKey, envKey]);

  const filtered = useMemo(() => {
    const q = filterFlag.trim().toLowerCase();
    if (!q) return events;
    return events.filter((e) => (e.flagKey ?? "").toLowerCase().includes(q));
  }, [events, filterFlag]);

  return (
    <Stack gap={5}>
      <PageHeader
        title={<Heading level={1}>Live Events Debugger</Heading>}
        description={
          <Text tone="muted" size="sm">
            Zero-storage real-time NATS evaluation tail for{" "}
            <Text as="span" mono>
              {projectKey} / {envKey || "—"}
            </Text>
            . Events stream ephemerally with zero warehouse write cost.
          </Text>
        }
        actions={
          <Stack direction="horizontal" align="center" gap={2}>
            <StatusDot
              tone={
                status === "connected"
                  ? "success"
                  : status === "connecting"
                    ? "warning"
                    : "neutral"
              }
              label={`Stream ${status}`}
            />
            <Badge
              tone={
                status === "connected"
                  ? "success"
                  : status === "connecting"
                    ? "warning"
                    : "neutral"
              }
            >
              {status === "connected"
                ? "Live SSE Connected"
                : status === "connecting"
                  ? "Connecting..."
                  : "Standby"}
            </Badge>
            <Button variant="secondary" size="sm" onClick={() => setPaused((p) => !p)}>
              {paused ? <Icons.Play size={14} /> : <Icons.Clock size={14} />}
              {paused ? "Resume" : "Pause"}
            </Button>
            <Button variant="ghost" size="sm" onClick={() => setEvents([])}>
              <Icons.RefreshCw size={14} />
              Clear
            </Button>
          </Stack>
        }
      />

      {!envKey && (
        <Alert tone="info" title="Select an environment">
          Pick an environment from the top bar to tap its ephemeral NATS evaluation stream.
        </Alert>
      )}

      <Card padded>
        <Stack direction="horizontal" align="center" justify="space-between" gap={3}>
          <Input
            aria-label="Filter live events by flag key"
            placeholder="Filter by flag key..."
            value={filterFlag}
            onChange={(e) => setFilterFlag(e.target.value)}
          />
          <Badge tone="neutral">{filtered.length} captured</Badge>
        </Stack>
      </Card>

      <DataTable
        ariaLabel="Live evaluation debug events"
        rows={filtered}
        rowKey={(r) => r.rowId}
        emptyTitle="Waiting for live SDK evaluations"
        emptyDescription="SDK evaluation summaries and debug events sent to event-ingest appear here in real time."
        columns={[
          {
            key: "timestamp",
            header: "Time",
            width: "160px",
            render: (r) => (
              <Text size="xs" mono tone="muted">
                {new Date(r.timestamp).toLocaleTimeString()}
              </Text>
            ),
          },
          {
            key: "kind",
            header: "Kind",
            width: "110px",
            render: (r) => <Badge tone="accent">{r.kind}</Badge>,
          },
          {
            key: "flag",
            header: "Flag Key",
            render: (r) => (
              <Text bold mono>
                {r.flagKey ?? "—"}
              </Text>
            ),
          },
          {
            key: "variation",
            header: "Variation",
            width: "110px",
            render: (r) => (
              <Badge tone="neutral">
                {r.variationIndex !== undefined && r.variationIndex !== null
                  ? `#${r.variationIndex}`
                  : "default"}
              </Badge>
            ),
          },
          {
            key: "contexts",
            header: "Context Kinds",
            render: (r) => (
              <Stack direction="horizontal" gap={1}>
                {(r.contextKinds ?? []).map((k) => (
                  <Badge key={k} tone="neutral">
                    {k}
                  </Badge>
                ))}
              </Stack>
            ),
          },
          {
            key: "count",
            header: "Count",
            width: "90px",
            render: (r) => <Text mono>{r.count ?? 1}</Text>,
          },
        ]}
      />
    </Stack>
  );
}
