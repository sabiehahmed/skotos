import {
  Alert,
  AlertDialog,
  Badge,
  Button,
  Card,
  CodeBlock,
  FormField,
  Heading,
  Icons,
  Input,
  PageHeader,
  Select,
  Stack,
  StatusDot,
  Text,
  ToggleGroup,
  useToast,
} from "@skotos/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  API_BASE_URL,
  canAdmin,
  formatErrorMessage,
  skotosApi,
  unwrapApiResult,
  type BuiltInRole,
  type Environment,
  type EnvironmentKey,
  type FlagResponse,
} from "../lib/api";
import {
  SDKS,
  buildSnippets,
  defaultServiceUrls,
  getSdk,
  secretEnvName,
  type FlagValueKind,
  type InstallMode,
  type SdkDefinition,
  type SdkSide,
} from "../lib/sdkSnippets";
import styles from "./SdkSetupPage.module.css";

export interface SdkSetupPageProps {
  projectKey: string;
  environments: Environment[];
  activeEnv?: Environment;
  userRole: BuiltInRole;
  onEnvChange: (envKey: string) => void;
  onOpenFlag?: (flagKey: string) => void;
}

function clsx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}

const PREFS_KEY = "skotos:sdk-setup:v1";
const PLACEHOLDER_FLAG = "my-first-flag";
/** Activity newer than this counts as a live SDK connection. */
const LIVE_WINDOW_MS = 2 * 60_000;
const POLL_MS = 5_000;

interface SetupPrefs {
  sdk: string;
  installMode: InstallMode;
  repoPath: string;
  deliveryUrl: string;
  eventsUrl: string;
  contextKind: string;
  contextKey: string;
}

function loadPrefs(): Partial<SetupPrefs> {
  try {
    const raw = window.localStorage.getItem(PREFS_KEY);
    return raw ? (JSON.parse(raw) as Partial<SetupPrefs>) : {};
  } catch {
    return {};
  }
}

function savePrefs(prefs: SetupPrefs) {
  try {
    window.localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));
  } catch {
    // Storage may be unavailable (private mode); prefs are a convenience only.
  }
}

const SIDE_META: Record<SdkSide, { title: string; hint: string; icon: ReactNode }> = {
  server: {
    title: "Server-side",
    hint: "Secret SDK key · evaluates locally",
    icon: <Icons.HardDrives size={13} />,
  },
  client: {
    title: "Client-side",
    hint: "Public client-side ID",
    icon: <Icons.Desktop size={13} />,
  },
  mobile: {
    title: "Mobile",
    hint: "Mobile key",
    icon: <Icons.DeviceMobile size={13} />,
  },
};

/** Picks the newest non-expiring key of a kind (rotations leave the old one expiring). */
function activeKey(keys: EnvironmentKey[], kind: "sdk" | "mobile"): EnvironmentKey | undefined {
  const ofKind = keys.filter((k) => k.kind === kind);
  const live = ofKind.filter((k) => !k.expiresAt);
  const pool = live.length ? live : ofKind;
  return [...pool].sort((a, b) => b.createdAt.localeCompare(a.createdAt))[0];
}

function relativeTime(ts: number, now: number): string {
  const s = Math.max(0, Math.round((now - ts) / 1000));
  if (s < 5) return "just now";
  if (s < 60) return `${s}s ago`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  if (h < 48) return `${h}h ago`;
  return new Date(ts).toLocaleString();
}

function sampleValueFor(flag?: FlagResponse): unknown {
  if (!flag) return undefined;
  return flag.variations[0]?.value;
}

function Step({
  n,
  title,
  done,
  aside,
  children,
}: {
  n: number;
  title: string;
  done?: boolean;
  aside?: ReactNode;
  children: ReactNode;
}) {
  return (
    <Card padded>
      <div className={styles.step}>
        <span className={clsx(styles.stepNumber, done && styles.stepNumberDone)} aria-hidden="true">
          {done ? <Icons.Check size={12} weight="bold" /> : n}
        </span>
        <div className={styles.stepBody}>
          <Stack direction="horizontal" align="center" justify="space-between" gap={2}>
            <Heading level={3}>{title}</Heading>
            {aside}
          </Stack>
          {children}
        </div>
      </div>
    </Card>
  );
}

export function SdkSetupPage({
  projectKey,
  environments,
  activeEnv,
  userRole,
  onEnvChange,
  onOpenFlag,
}: SdkSetupPageProps) {
  const qc = useQueryClient();
  const { toast } = useToast();
  const admin = canAdmin(userRole);
  const envKey = activeEnv?.key ?? "";

  const initialPrefs = useMemo(() => {
    const defaults = defaultServiceUrls(
      typeof window !== "undefined" ? window.location : undefined,
    );
    const saved = loadPrefs();
    return {
      sdk: saved.sdk && SDKS.some((s) => s.id === saved.sdk) ? saved.sdk : "node",
      installMode: saved.installMode ?? "local",
      repoPath: saved.repoPath ?? "~/skotos",
      deliveryUrl: saved.deliveryUrl || defaults.deliveryUrl,
      eventsUrl: saved.eventsUrl || defaults.eventsUrl,
      contextKind: saved.contextKind || "user",
      contextKey: saved.contextKey || "user-123",
    } satisfies SetupPrefs;
  }, []);

  const [sdkId, setSdkId] = useState(initialPrefs.sdk);
  const [installMode, setInstallMode] = useState<InstallMode>(initialPrefs.installMode);
  const [repoPath, setRepoPath] = useState(initialPrefs.repoPath);
  const [deliveryUrl, setDeliveryUrl] = useState(initialPrefs.deliveryUrl);
  const [eventsUrl, setEventsUrl] = useState(initialPrefs.eventsUrl);
  const [contextKind, setContextKind] = useState(initialPrefs.contextKind);
  const [contextKey, setContextKey] = useState(initialPrefs.contextKey);
  const [flagKey, setFlagKey] = useState<string>("");
  const [showUrls, setShowUrls] = useState(false);
  const [rotateOpen, setRotateOpen] = useState(false);
  /** Plaintext keys revealed by a rotation in this session, per env + kind. Never persisted. */
  const [revealed, setRevealed] = useState<Record<string, string>>({});

  const sdk: SdkDefinition = getSdk(sdkId);
  const keyKind: "sdk" | "mobile" | null =
    sdk.side === "server" ? "sdk" : sdk.side === "mobile" ? "mobile" : null;

  useEffect(() => {
    savePrefs({ sdk: sdkId, installMode, repoPath, deliveryUrl, eventsUrl, contextKind, contextKey });
  }, [sdkId, installMode, repoPath, deliveryUrl, eventsUrl, contextKind, contextKey]);

  const keysQuery = useQuery({
    queryKey: ["env-keys", projectKey, envKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/environments/{envKey}/keys", {
          params: { path: { projectKey, envKey } },
        }),
      ),
    enabled: Boolean(envKey),
  });

  const flagsQuery = useQuery({
    queryKey: ["flags", projectKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/flags", {
          params: { path: { projectKey } },
        }),
      ),
  });

  const flags = useMemo(
    () => (flagsQuery.data?.items ?? []).filter((f) => !f.archived),
    [flagsQuery.data],
  );

  // Default to a flag the chosen SDK can actually receive.
  useEffect(() => {
    if (!flags.length) return;
    if (flagKey && flags.some((f) => f.key === flagKey)) return;
    const preferred =
      sdk.side === "client"
        ? flags.find((f) => f.clientSideAvailability?.usingEnvironmentId)
        : sdk.side === "mobile"
          ? flags.find((f) => f.clientSideAvailability?.usingMobileKey)
          : undefined;
    const pick = preferred ?? flags.find((f) => f.kind === "boolean") ?? flags[0];
    if (pick) setFlagKey(pick.key);
  }, [flags, flagKey, sdk.side]);

  const selectedFlag = flags.find((f) => f.key === flagKey);
  const effectiveFlagKey = selectedFlag?.key ?? PLACEHOLDER_FLAG;
  const flagKind: FlagValueKind = selectedFlag?.kind ?? "boolean";

  const keyInfo = keyKind ? activeKey(keysQuery.data?.keys ?? [], keyKind) : undefined;
  const revealedSecret = keyKind ? revealed[`${envKey}:${keyKind}`] : undefined;
  const clientSideId = keysQuery.data?.clientSideId ?? activeEnv?.clientSideId ?? "<client-side-id>";

  const snippets = useMemo(
    () =>
      buildSnippets(
        {
          sdk,
          repoPath,
          deliveryUrl,
          eventsUrl,
          clientSideId,
          secretKey: revealedSecret,
          secretKeyPrefix: keyInfo?.keyPrefix,
          flagKey: effectiveFlagKey,
          flagKind,
          sampleValue: sampleValueFor(selectedFlag),
          contextKind: contextKind.trim() || "user",
          contextKey: contextKey.trim() || "user-123",
        },
        installMode,
      ),
    [
      sdk,
      repoPath,
      deliveryUrl,
      eventsUrl,
      clientSideId,
      revealedSecret,
      keyInfo?.keyPrefix,
      effectiveFlagKey,
      flagKind,
      selectedFlag,
      contextKind,
      contextKey,
      installMode,
    ],
  );

  const rotateMutation = useMutation({
    mutationFn: async (kind: "sdk" | "mobile") =>
      unwrapApiResult(
        await skotosApi.POST("/api/v1/projects/{projectKey}/environments/{envKey}/keys/rotate", {
          params: { path: { projectKey, envKey } },
          body: { kind, gracePeriodSeconds: 3600 },
        }),
      ),
    onSuccess: async (res, kind) => {
      setRevealed((prev) => ({ ...prev, [`${envKey}:${kind}`]: res.plaintextKey }));
      setRotateOpen(false);
      await qc.invalidateQueries({ queryKey: ["env-keys", projectKey, envKey] });
      toast({
        title: "New key generated",
        description: "It is filled into the snippets below. Copy it now: it is shown only once.",
        tone: "success",
      });
    },
    onError: (err) => toast({ title: "Error", description: formatErrorMessage(err), tone: "danger" }),
  });

  // ---------------------------------------------------------------------------
  // Connection check: SDK telemetry lands in event-ingest, which feeds the
  // evaluation usage rollups, the context catalog and the live debug stream.
  // ---------------------------------------------------------------------------
  const openedAt = useRef(Date.now());
  const [now, setNow] = useState(Date.now());
  const [liveEventAt, setLiveEventAt] = useState<number | null>(null);
  const [liveFlags, setLiveFlags] = useState<string[]>([]);

  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(t);
  }, []);

  useEffect(() => {
    setLiveEventAt(null);
    setLiveFlags([]);
    openedAt.current = Date.now();
  }, [envKey]);

  const usageQuery = useQuery({
    queryKey: ["sdk-setup-usage", projectKey, envKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/environments/{envKey}/usage", {
          params: { path: { projectKey, envKey }, query: { days: 7 } },
        }),
      ),
    enabled: Boolean(envKey),
    refetchInterval: POLL_MS,
  });

  const catalogQuery = useQuery({
    queryKey: ["sdk-setup-catalog", projectKey, envKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET(
          "/api/v1/projects/{projectKey}/environments/{envKey}/context-catalog",
          { params: { path: { projectKey, envKey } } },
        ),
      ),
    enabled: Boolean(envKey),
    refetchInterval: POLL_MS,
  });

  useEffect(() => {
    if (!envKey || typeof EventSource === "undefined") return;
    const url = `${API_BASE_URL.replace(/\/+$/, "")}/api/v1/projects/${encodeURIComponent(
      projectKey,
    )}/environments/${encodeURIComponent(envKey)}/live-events`;
    const es = new EventSource(url, { withCredentials: true });
    es.addEventListener("debug", (ev) => {
      try {
        const parsed = JSON.parse((ev as MessageEvent<string>).data) as { flagKey?: string };
        setLiveEventAt(Date.now());
        if (parsed.flagKey) {
          const fk = parsed.flagKey;
          setLiveFlags((prev) => (prev.includes(fk) ? prev : [fk, ...prev].slice(0, 5)));
        }
      } catch {
        // ignore keepalives / malformed frames
      }
    });
    return () => es.close();
  }, [projectKey, envKey]);

  const usageItems = usageQuery.data?.items ?? [];
  const catalogKinds = catalogQuery.data?.kinds ?? [];
  const timestamps = [
    ...usageItems.map((u) => (u.lastEvaluatedAt ? Date.parse(u.lastEvaluatedAt) : NaN)),
    ...catalogKinds.map((k) => Date.parse(k.lastSeenAt)),
    liveEventAt ?? NaN,
  ].filter((t) => Number.isFinite(t));
  const lastActivity = timestamps.length ? Math.max(...timestamps) : null;
  const flagUsage = usageItems.find((u) => u.flagKey === effectiveFlagKey);
  const seenKinds = catalogKinds.map((k) => k.kind);

  const connection: "live" | "idle" | "waiting" =
    lastActivity === null
      ? "waiting"
      : now - lastActivity <= LIVE_WINDOW_MS || lastActivity >= openedAt.current
        ? "live"
        : "idle";

  const flagUnavailable =
    selectedFlag &&
    ((sdk.side === "client" && !selectedFlag.clientSideAvailability?.usingEnvironmentId) ||
      (sdk.side === "mobile" && !selectedFlag.clientSideAvailability?.usingMobileKey));

  const sdkGroups: SdkSide[] = ["server", "client", "mobile"];
  const secretName = secretEnvName(sdk.side);
  const registryAvailable = !sdk.localOnly;

  return (
    <Stack gap={5}>
      <PageHeader
        title={<Heading level={1}>SDK setup</Heading>}
        description={
          <Text size="sm" tone="muted">
            Connect an application to{" "}
            <Text as="span" size="sm" mono>
              {projectKey}
            </Text>{" "}
            in a few commands. Snippets are filled in for this deployment.
          </Text>
        }
      />

      <div className={styles.layout}>
        <Stack gap={4}>
          {/* Choose SDK */}
          <Card padded>
            <Stack gap={4}>
              <Stack direction="horizontal" align="center" justify="space-between" gap={3}>
                <Heading level={3}>Choose your SDK</Heading>
                <div style={{ minWidth: 200 }}>
                  <Select
                    ariaLabel="Environment"
                    value={envKey}
                    onValueChange={onEnvChange}
                    options={environments.map((e) => ({ value: e.key, label: e.name }))}
                    placeholder="Environment"
                  />
                </div>
              </Stack>
              <div className={styles.sdkGroups}>
                {sdkGroups.map((side) => (
                  <Stack key={side} gap={2}>
                    <Stack direction="horizontal" align="center" gap={2}>
                      <span className={styles.sectionLabel}>{SIDE_META[side].title}</span>
                      <Text as="span" size="xs" tone="muted">
                        {SIDE_META[side].hint}
                      </Text>
                    </Stack>
                    <div className={styles.sdkGrid} role="radiogroup" aria-label={SIDE_META[side].title}>
                      {SDKS.filter((s) => s.side === side).map((s) => (
                        <Button
                          key={s.id}
                          variant="ghost"
                          role="radio"
                          aria-checked={s.id === sdk.id}
                          className={clsx(styles.sdkTile, s.id === sdk.id && styles.sdkTileActive)}
                          onClick={() => setSdkId(s.id)}
                        >
                          <span className={styles.sdkTileName}>
                            {SIDE_META[side].icon}
                            {s.label}
                          </span>
                          <span className={styles.sdkTileMeta}>{s.language}</span>
                        </Button>
                      ))}
                    </div>
                  </Stack>
                ))}
              </div>
            </Stack>
          </Card>

          {/* 1. Install */}
          <Step
            n={1}
            title="Install the SDK"
            aside={
              registryAvailable ? (
                <ToggleGroup
                  ariaLabel="Install source"
                  value={installMode}
                  onValueChange={(v) => setInstallMode(v as InstallMode)}
                  options={[
                    { value: "local", label: "Local build" },
                    { value: "registry", label: "Package registry" },
                  ]}
                />
              ) : (
                <Badge>Local build only</Badge>
              )
            }
          >
            {(installMode === "local" || !registryAvailable) && (
              <FormField
                label="Skotos checkout path"
                htmlFor="sdk-repo-path"
                hint="Where the Skotos repository lives on your machine. Installs straight from it, no publishing needed."
              >
                <Input
                  id="sdk-repo-path"
                  mono
                  value={repoPath}
                  onChange={(e) => setRepoPath(e.target.value)}
                  placeholder="~/skotos"
                />
              </FormField>
            )}
            {installMode === "registry" && registryAvailable && (
              <Text size="sm" tone="muted">
                Use this once {sdk.packageName} is published to your registry.
              </Text>
            )}
            <CodeBlock label={snippets.installLabel} code={snippets.install} />
          </Step>

          {/* 2. Credentials */}
          <Step
            n={2}
            title={sdk.side === "client" ? "Use the client-side ID" : "Add your key"}
            done={sdk.side === "client" || Boolean(revealedSecret)}
          >
            {sdk.side === "client" ? (
              <>
                <Text size="sm" tone="muted">
                  Browser SDKs authenticate with the environment&apos;s public client-side ID.
                  It is already in the snippet and is safe to ship in your bundle. Only flags
                  marked &ldquo;Available to Client-Side SDKs&rdquo; are sent to browsers.
                </Text>
                <CodeBlock label="Client-side ID · public" code={clientSideId} />
              </>
            ) : (
              <>
                <Text size="sm" tone="muted">
                  {sdk.side === "server"
                    ? "Server SDKs use the secret SDK key. Keep it on the server: never ship it to browsers or commit it."
                    : "The mobile SDK uses the mobile key. Load it from app config instead of committing it."}{" "}
                  Skotos stores only a hash, so the full key is shown once, when it is generated.
                </Text>
                <div className={styles.kvList}>
                  <div className={styles.kvRow}>
                    <span className={styles.kvKey}>Active key</span>
                    <span className={styles.kvValue}>
                      {keyInfo ? `${keyInfo.keyPrefix}…` : keysQuery.isLoading ? "…" : "none"}
                    </span>
                  </div>
                  <div className={styles.kvRow}>
                    <span className={styles.kvKey}>Variable</span>
                    <span className={styles.kvValue}>{secretName}</span>
                  </div>
                </div>
                {revealedSecret && (
                  <Alert tone="warning" title="Copy this key now (shown once)">
                    The previous key keeps working for 1 hour, then stops.
                  </Alert>
                )}
                {snippets.env && <CodeBlock label=".env" code={snippets.env} />}
                {admin && !revealedSecret && (
                  <Stack direction="horizontal" align="center" gap={2}>
                    <Button
                      size="sm"
                      variant="secondary"
                      onClick={() => setRotateOpen(true)}
                      disabled={!envKey}
                    >
                      <Icons.KeyRound size={13} />
                      Generate new {keyKind === "mobile" ? "mobile key" : "SDK key"}
                    </Button>
                    <Text as="span" size="xs" tone="muted">
                      Have the key already? Paste it into {secretName}.
                    </Text>
                  </Stack>
                )}
              </>
            )}
          </Step>

          {/* 3. Initialize & evaluate */}
          <Step n={3} title="Initialize and evaluate a flag">
            <div className={styles.fieldRow}>
              <FormField label="Flag" htmlFor="sdk-flag">
                <Select
                  id="sdk-flag"
                  ariaLabel="Flag"
                  value={selectedFlag?.key}
                  onValueChange={setFlagKey}
                  placeholder={flags.length ? "Choose a flag" : `No flags yet (${PLACEHOLDER_FLAG})`}
                  disabled={!flags.length}
                  options={flags.map((f) => ({ value: f.key, label: `${f.key} · ${f.kind}` }))}
                />
              </FormField>
              <div className={styles.fieldRow}>
                <FormField label="Context kind" htmlFor="sdk-ctx-kind">
                  <Input
                    id="sdk-ctx-kind"
                    mono
                    value={contextKind}
                    onChange={(e) => setContextKind(e.target.value)}
                  />
                </FormField>
                <FormField label="Context key" htmlFor="sdk-ctx-key">
                  <Input
                    id="sdk-ctx-key"
                    mono
                    value={contextKey}
                    onChange={(e) => setContextKey(e.target.value)}
                  />
                </FormField>
              </div>
            </div>
            {flagUnavailable && (
              <Alert
                tone="warning"
                title={`${selectedFlag.key} is not available to ${sdk.side === "mobile" ? "mobile" : "client-side"} SDKs`}
              >
                <Stack gap={2}>
                  <Text size="sm">
                    {sdk.label} will get the fallback value until you turn on &ldquo;Available to{" "}
                    {sdk.side === "mobile" ? "Mobile SDKs" : "Client-Side SDKs"}&rdquo; in the
                    flag&apos;s settings.
                  </Text>
                  {onOpenFlag && (
                    <div>
                      <Button size="sm" variant="secondary" onClick={() => onOpenFlag(selectedFlag.key)}>
                        Open flag settings
                        <Icons.ArrowUpRight size={13} />
                      </Button>
                    </div>
                  )}
                </Stack>
              </Alert>
            )}
            <CodeBlock label={sdk.fileName} code={snippets.init} />
          </Step>

          {/* 4. Verify */}
          <Step n={4} title="Run your app and verify" done={connection === "live"}>
            <ConnectionStatus
              state={connection}
              lastActivity={lastActivity}
              now={now}
              loading={usageQuery.isLoading || catalogQuery.isLoading}
            />
            {connection !== "waiting" && (
              <div className={styles.kvList}>
                <div className={styles.kvRow}>
                  <span className={styles.kvKey}>{effectiveFlagKey}</span>
                  <span className={styles.kvValue}>
                    {flagUsage
                      ? `${flagUsage.totalEvaluations.toLocaleString()} ${flagUsage.totalEvaluations === 1 ? "evaluation" : "evaluations"} · 7d`
                      : "not evaluated yet"}
                  </span>
                </div>
                <div className={styles.kvRow}>
                  <span className={styles.kvKey}>Context kinds seen</span>
                  <span className={styles.kvValue}>{seenKinds.length ? seenKinds.join(", ") : "none"}</span>
                </div>
                {liveFlags.length > 0 && (
                  <div className={styles.kvRow}>
                    <span className={styles.kvKey}>Live this session</span>
                    <span className={styles.kvValue}>{liveFlags.join(", ")}</span>
                  </div>
                )}
              </div>
            )}
            <Text size="xs" tone="muted">
              SDKs send evaluation summaries every few seconds. Server SDKs in short scripts
              must flush or close before exiting.
            </Text>
          </Step>
        </Stack>

        {/* Aside: connection details */}
        <div className={styles.aside}>
          <Card padded>
            <Stack gap={3}>
              <span className={styles.sectionLabel}>Connection details</span>
              <div className={styles.kvList}>
                <div className={styles.kvRow}>
                  <span className={styles.kvKey}>Project</span>
                  <span className={styles.kvValue}>{projectKey}</span>
                </div>
                <div className={styles.kvRow}>
                  <span className={styles.kvKey}>Environment</span>
                  <span className={styles.kvValue}>
                    <Stack direction="horizontal" align="center" gap={1} style={{ display: "inline-flex" }}>
                      {activeEnv && <StatusDot color={activeEnv.color} />}
                      {activeEnv?.key ?? "—"}
                    </Stack>
                  </span>
                </div>
                <div className={styles.kvRow}>
                  <span className={styles.kvKey}>SDK</span>
                  <span className={styles.kvValue}>{sdk.label}</span>
                </div>
                <div className={styles.kvRow}>
                  <span className={styles.kvKey}>Delivery</span>
                  <span className={styles.kvValue} title={deliveryUrl}>
                    {deliveryUrl}
                  </span>
                </div>
                <div className={styles.kvRow}>
                  <span className={styles.kvKey}>Events</span>
                  <span className={styles.kvValue} title={eventsUrl}>
                    {eventsUrl}
                  </span>
                </div>
              </div>
              <Button size="sm" variant="ghost" onClick={() => setShowUrls((v) => !v)}>
                <Icons.SlidersHorizontal size={13} />
                {showUrls ? "Hide service URLs" : "Edit service URLs"}
              </Button>
              {showUrls && (
                <Stack gap={3}>
                  <FormField
                    label="Delivery edge URL"
                    htmlFor="sdk-delivery-url"
                    hint="Where SDKs fetch and stream flags (delivery-edge or relay proxy)."
                  >
                    <Input
                      id="sdk-delivery-url"
                      mono
                      value={deliveryUrl}
                      onChange={(e) => setDeliveryUrl(e.target.value)}
                    />
                  </FormField>
                  <FormField
                    label="Event ingest URL"
                    htmlFor="sdk-events-url"
                    hint="Where SDKs send evaluation events."
                  >
                    <Input
                      id="sdk-events-url"
                      mono
                      value={eventsUrl}
                      onChange={(e) => setEventsUrl(e.target.value)}
                    />
                  </FormField>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => {
                      const d = defaultServiceUrls(window.location);
                      setDeliveryUrl(d.deliveryUrl);
                      setEventsUrl(d.eventsUrl);
                    }}
                  >
                    <Icons.RotateCcw size={13} />
                    Reset to defaults
                  </Button>
                </Stack>
              )}
            </Stack>
          </Card>
          <Card padded>
            <Stack gap={2}>
              <span className={styles.sectionLabel}>Status</span>
              <Stack direction="horizontal" align="center" gap={2}>
                <span
                  className={clsx(
                    styles.pulse,
                    connection === "live" && styles.pulseLive,
                    connection === "idle" && styles.pulseIdle,
                    connection === "waiting" && styles.pulseWaiting,
                  )}
                  aria-hidden="true"
                />
                <Text size="sm">
                  {connection === "live"
                    ? "SDK connected"
                    : connection === "idle"
                      ? "No recent SDK activity"
                      : "Waiting for first SDK event"}
                </Text>
              </Stack>
            </Stack>
          </Card>
        </div>
      </div>

      <AlertDialog
        open={rotateOpen}
        onOpenChange={setRotateOpen}
        title={`Generate a new ${keyKind === "mobile" ? "mobile key" : "SDK key"} for ${activeEnv?.name ?? envKey}?`}
        description="The new key is shown once and filled into the snippets. Apps using the current key keep working for 1 hour, then they must switch to the new key."
        confirmLabel="Generate key"
        confirmDisabled={rotateMutation.isPending || !keyKind}
        onConfirm={() => keyKind && rotateMutation.mutate(keyKind)}
      />
    </Stack>
  );
}

function ConnectionStatus({
  state,
  lastActivity,
  now,
  loading,
}: {
  state: "live" | "idle" | "waiting";
  lastActivity: number | null;
  now: number;
  loading: boolean;
}) {
  return (
    <div className={styles.status} role="status" aria-live="polite">
      <span
        className={clsx(
          styles.pulse,
          state === "live" && styles.pulseLive,
          state === "idle" && styles.pulseIdle,
          state === "waiting" && styles.pulseWaiting,
        )}
        aria-hidden="true"
      />
      <Stack gap={1}>
        <Text size="sm" bold>
          {state === "live"
            ? "Receiving events from your SDK"
            : state === "idle"
              ? "An SDK connected before, but not recently"
              : loading
                ? "Checking for SDK activity…"
                : "Waiting for the first SDK event…"}
        </Text>
        <Text size="xs" tone="muted" mono>
          {lastActivity !== null
            ? `Last event ${relativeTime(lastActivity, now)}`
            : "Run the snippet above. This updates automatically."}
        </Text>
      </Stack>
    </div>
  );
}
