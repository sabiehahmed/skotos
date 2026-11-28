import {
  AppShell,
  Badge,
  Button,
  CommandPalette,
  EnvironmentBanner,
  Heading,
  IconButton,
  Icons,
  Select,
  Separator,
  Sidebar,
  Spinner,
  Stack,
  Text,
  ThemeProvider,
  ToastProvider,
  TooltipProvider,
  TopBar,
  useTheme,
} from "@skotos/ui";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import {
  createBrowserHistory,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
  useLocation,
  useNavigate,
  useParams,
  useSearch,
} from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";
import { skotosApi, unwrapApiResult, type AuthMe } from "./lib/api";
import { useAuthMe, useLogoutMutation } from "./lib/auth";
import { ApprovalsPage } from "./pages/ApprovalsPage";
import { AuditLogPage } from "./pages/AuditLogPage";
import { FlagDetailPage } from "./pages/FlagDetailPage";
import { FlagsListPage } from "./pages/FlagsListPage";
import { RolesSettingsPage, TeamsSsoSettingsPage } from "./pages/GovernanceSettingsPage";
import { LiveDebuggerPage } from "./pages/LiveDebuggerPage";
import { LoginPage } from "./pages/LoginPage";
import { PipelinesPage } from "./pages/PipelinesPage";
import { SegmentsPage } from "./pages/SegmentsPage";
import {
  EnvironmentsSettingsPage,
  MembersSettingsPage,
  ProjectsSettingsPage,
  TokensSettingsPage,
} from "./pages/SettingsPages";
import { WebhooksPage } from "./pages/WebhooksPage";

export const WEB_APP_NAME = "@skotos/web";

interface EnvSearch {
  env?: string;
}

function ThemeToggleButton() {
  const { resolvedTheme, toggleTheme } = useTheme();
  return (
    <IconButton
      label={resolvedTheme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
      onClick={toggleTheme}
    >
      {resolvedTheme === "dark" ? <Icons.Sun size={16} /> : <Icons.Moon size={16} />}
    </IconButton>
  );
}

function AuthenticatedShell({ me }: { me: AuthMe }) {
  const navigate = useNavigate();
  const location = useLocation();
  const search = useSearch({ strict: false }) as EnvSearch;
  const params = useParams({ strict: false }) as { projectKey?: string };
  const logout = useLogoutMutation();
  const [cmdOpen, setCmdOpen] = useState(false);

  const projectsQuery = useQuery({
    queryKey: ["projects"],
    queryFn: async () => unwrapApiResult(await skotosApi.GET("/api/v1/projects")),
  });

  const projects = useMemo(() => projectsQuery.data?.items ?? [], [projectsQuery.data]);
  const activeProjectKey = params.projectKey ?? projects[0]?.key ?? "default";

  const envsQuery = useQuery({
    queryKey: ["environments", activeProjectKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/environments", {
          params: { path: { projectKey: activeProjectKey } },
        }),
      ),
    enabled: Boolean(activeProjectKey) && projects.length > 0,
  });

  const environments = useMemo(() => envsQuery.data?.items ?? [], [envsQuery.data]);
  const activeEnv =
    environments.find((e) => e.key === search.env) ?? environments[0] ?? undefined;

  useEffect(() => {
    if (location.pathname === "/" && projects[0]) {
      void navigate({
        to: "/projects/$projectKey/flags",
        params: { projectKey: projects[0].key },
        search: activeEnv ? { env: activeEnv.key } : {},
        replace: true,
      });
    }
  }, [location.pathname, projects, activeEnv, navigate]);

  const setActiveProject = (projectKey: string) => {
    void navigate({
      to: "/projects/$projectKey/flags",
      params: { projectKey },
      search: {},
    });
  };

  const setActiveEnvKey = (envKey: string) => {
    void navigate({
      to: location.pathname,
      search: { env: envKey } as never,
      replace: true,
    });
  };

  const isPath = (prefix: string) => location.pathname.startsWith(prefix);

  return (
    <>
      <AppShell
        envColor={activeEnv?.color}
        sidebar={
          <Sidebar
            header={
              <Stack direction="horizontal" align="center" gap={2}>
                <Icons.Flag size={18} />
                <Heading level={4}>Skotos</Heading>
                <Badge tone="neutral">{me.organization.slug}</Badge>
              </Stack>
            }
            footer={
              <Stack gap={2}>
                <Stack direction="horizontal" align="center" justify="space-between">
                  <Stack gap={1}>
                    <Text size="xs" bold>
                      {me.displayName}
                    </Text>
                    <Text size="xs" tone="muted">
                      {me.role}
                    </Text>
                  </Stack>
                  <IconButton
                    label="Sign out"
                    onClick={async () => {
                      await logout.mutateAsync();
                      void navigate({ to: "/login" });
                    }}
                  >
                    <Icons.LogOut size={15} />
                  </IconButton>
                </Stack>
              </Stack>
            }
          >
            <Button
              variant={location.pathname.includes("/flags") ? "secondary" : "ghost"}
              style={{ justifyContent: "flex-start" }}
              onClick={() =>
                void navigate({
                  to: "/projects/$projectKey/flags",
                  params: { projectKey: activeProjectKey },
                  search: activeEnv ? { env: activeEnv.key } : {},
                })
              }
            >
              <Icons.Flag size={15} />
              Feature Flags
            </Button>
            <Button
              variant={location.pathname.includes("/segments") ? "secondary" : "ghost"}
              style={{ justifyContent: "flex-start" }}
              onClick={() =>
                void navigate({
                  to: "/projects/$projectKey/segments",
                  params: { projectKey: activeProjectKey },
                  search: activeEnv ? { env: activeEnv.key } : {},
                })
              }
            >
              <Icons.Filter size={15} />
              Segments
            </Button>
            <Button
              variant={location.pathname.includes("/pipelines") ? "secondary" : "ghost"}
              style={{ justifyContent: "flex-start" }}
              onClick={() =>
                void navigate({
                  to: "/projects/$projectKey/pipelines",
                  params: { projectKey: activeProjectKey },
                  search: activeEnv ? { env: activeEnv.key } : {},
                })
              }
            >
              <Icons.GitBranch size={15} />
              Release Pipelines
            </Button>
            <Button
              variant={isPath("/approvals") ? "secondary" : "ghost"}
              style={{ justifyContent: "flex-start" }}
              onClick={() => void navigate({ to: "/approvals" })}
            >
              <Icons.CheckCircle2 size={15} />
              Approvals
            </Button>
            <Button
              variant={location.pathname.includes("/debugger") ? "secondary" : "ghost"}
              style={{ justifyContent: "flex-start" }}
              onClick={() =>
                void navigate({
                  to: "/projects/$projectKey/debugger",
                  params: { projectKey: activeProjectKey },
                  search: activeEnv ? { env: activeEnv.key } : {},
                })
              }
            >
              <Icons.Terminal size={15} />
              Live Debugger
            </Button>
            <Button
              variant={isPath("/audit-log") ? "secondary" : "ghost"}
              style={{ justifyContent: "flex-start" }}
              onClick={() => void navigate({ to: "/audit-log" })}
            >
              <Icons.History size={15} />
              Audit Log
            </Button>
            <Separator />
            <Text size="xs" tone="muted" bold style={{ padding: "4px 10px" }}>
              SETTINGS
            </Text>
            <Button
              variant={isPath("/settings/projects") ? "secondary" : "ghost"}
              style={{ justifyContent: "flex-start" }}
              onClick={() => void navigate({ to: "/settings/projects" })}
            >
              <Icons.FolderKanban size={15} />
              Projects
            </Button>
            <Button
              variant={isPath("/settings/environments") ? "secondary" : "ghost"}
              style={{ justifyContent: "flex-start" }}
              onClick={() =>
                void navigate({
                  to: "/settings/environments",
                  search: activeEnv ? { env: activeEnv.key } : {},
                })
              }
            >
              <Icons.Layers size={15} />
              Environments &amp; Keys
            </Button>
            <Button
              variant={isPath("/settings/webhooks") ? "secondary" : "ghost"}
              style={{ justifyContent: "flex-start" }}
              onClick={() => void navigate({ to: "/settings/webhooks" })}
            >
              <Icons.Code2 size={15} />
              Webhooks
            </Button>
            <Button
              variant={isPath("/settings/tokens") ? "secondary" : "ghost"}
              style={{ justifyContent: "flex-start" }}
              onClick={() => void navigate({ to: "/settings/tokens" })}
            >
              <Icons.KeyRound size={15} />
              API Tokens
            </Button>
            <Button
              variant={isPath("/settings/roles") ? "secondary" : "ghost"}
              style={{ justifyContent: "flex-start" }}
              onClick={() => void navigate({ to: "/settings/roles" })}
            >
              <Icons.Shield size={15} />
              Custom Roles
            </Button>
            <Button
              variant={isPath("/settings/teams") ? "secondary" : "ghost"}
              style={{ justifyContent: "flex-start" }}
              onClick={() => void navigate({ to: "/settings/teams" })}
            >
              <Icons.SlidersHorizontal size={15} />
              Teams &amp; SSO
            </Button>
            <Button
              variant={isPath("/settings/members") ? "secondary" : "ghost"}
              style={{ justifyContent: "flex-start" }}
              onClick={() => void navigate({ to: "/settings/members" })}
            >
              <Icons.Users size={15} />
              Team Members
            </Button>
          </Sidebar>
        }
        topBar={
          <TopBar
            left={
              <Stack direction="horizontal" align="center" gap={3}>
                {projects.length > 0 && (
                  <Select
                    ariaLabel="Active project"
                    value={activeProjectKey}
                    onValueChange={setActiveProject}
                    options={projects.map((p) => ({ value: p.key, label: p.name }))}
                  />
                )}
                {environments.length > 0 && activeEnv && (
                  <Select
                    ariaLabel="Active environment"
                    value={activeEnv.key}
                    onValueChange={setActiveEnvKey}
                    options={environments.map((e) => ({ value: e.key, label: e.name }))}
                  />
                )}
              </Stack>
            }
            right={
              <Stack direction="horizontal" align="center" gap={2}>
                <Button size="sm" variant="secondary" onClick={() => setCmdOpen(true)}>
                  <Icons.Command size={13} />
                  Quick Jump (⌘K)
                </Button>
                <ThemeToggleButton />
              </Stack>
            }
          />
        }
        banner={
          activeEnv ? (
            <EnvironmentBanner
              environmentName={activeEnv.name}
              environmentKey={activeEnv.key}
              color={activeEnv.color}
              protectedEnv={activeEnv.confirmChanges || activeEnv.requireComments}
            />
          ) : undefined
        }
      >
        <Outlet />
      </AppShell>

      <CommandPalette
        open={cmdOpen}
        onOpenChange={setCmdOpen}
        items={[
          {
            id: "flags",
            label: "Go to Feature Flags",
            hint: activeProjectKey,
            onSelect: () =>
              void navigate({
                to: "/projects/$projectKey/flags",
                params: { projectKey: activeProjectKey },
                search: activeEnv ? { env: activeEnv.key } : {},
              }),
          },
          {
            id: "segments",
            label: "Go to Segments",
            hint: activeProjectKey,
            onSelect: () =>
              void navigate({
                to: "/projects/$projectKey/segments",
                params: { projectKey: activeProjectKey },
                search: activeEnv ? { env: activeEnv.key } : {},
              }),
          },
          {
            id: "pipelines",
            label: "Go to Release Pipelines",
            hint: activeProjectKey,
            onSelect: () =>
              void navigate({
                to: "/projects/$projectKey/pipelines",
                params: { projectKey: activeProjectKey },
                search: activeEnv ? { env: activeEnv.key } : {},
              }),
          },
          {
            id: "approvals",
            label: "Review Approval Requests",
            onSelect: () => void navigate({ to: "/approvals" }),
          },
          {
            id: "debugger",
            label: "Open Live Events Debugger",
            hint: activeEnv?.key,
            onSelect: () =>
              void navigate({
                to: "/projects/$projectKey/debugger",
                params: { projectKey: activeProjectKey },
                search: activeEnv ? { env: activeEnv.key } : {},
              }),
          },
          {
            id: "audit",
            label: "Go to Audit Log",
            onSelect: () => void navigate({ to: "/audit-log" }),
          },
          {
            id: "envs",
            label: "Manage Environments & SDK Keys",
            onSelect: () => void navigate({ to: "/settings/environments" }),
          },
          {
            id: "webhooks",
            label: "Manage Outbound Webhooks",
            onSelect: () => void navigate({ to: "/settings/webhooks" }),
          },
          {
            id: "roles",
            label: "Manage Custom RBAC Roles",
            onSelect: () => void navigate({ to: "/settings/roles" }),
          },
          {
            id: "teams",
            label: "Manage Teams, SSO & Compliance",
            onSelect: () => void navigate({ to: "/settings/teams" }),
          },
          {
            id: "tokens",
            label: "Manage API Access Tokens",
            onSelect: () => void navigate({ to: "/settings/tokens" }),
          },
          {
            id: "members",
            label: "Manage Team Members",
            onSelect: () => void navigate({ to: "/settings/members" }),
          },
        ]}
      />
    </>
  );
}

function RootLayout() {
  const location = useLocation();
  const navigate = useNavigate();
  const meQuery = useAuthMe();

  if (location.pathname === "/login" || location.pathname === "/auth/callback") {
    return (
      <LoginPage
        onLoggedIn={() => {
          void navigate({ to: "/" });
        }}
      />
    );
  }

  if (meQuery.isPending) {
    return (
      <div
        style={{
          minHeight: "100vh",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <Spinner size={28} label="Loading workspace" />
      </div>
    );
  }

  if (!meQuery.data) {
    return (
      <LoginPage
        onLoggedIn={() => {
          void meQuery.refetch();
        }}
      />
    );
  }

  return <AuthenticatedShell me={meQuery.data} />;
}

function FlagsRouteView() {
  const { projectKey } = useParams({ strict: false }) as { projectKey: string };
  const search = useSearch({ strict: false }) as EnvSearch;
  const navigate = useNavigate();
  const meQuery = useAuthMe();
  const envsQuery = useQuery({
    queryKey: ["environments", projectKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/environments", {
          params: { path: { projectKey } },
        }),
      ),
  });
  const environments = envsQuery.data?.items ?? [];
  const activeEnv = environments.find((e) => e.key === search.env) ?? environments[0];

  if (!meQuery.data) return null;

  return (
    <FlagsListPage
      projectKey={projectKey}
      activeEnv={activeEnv}
      userRole={meQuery.data.role}
      onOpenFlag={(flagKey) =>
        void navigate({
          to: "/projects/$projectKey/flags/$flagKey",
          params: { projectKey, flagKey },
          search: activeEnv ? { env: activeEnv.key } : {},
        })
      }
    />
  );
}

function FlagDetailRouteView() {
  const { projectKey, flagKey } = useParams({ strict: false }) as {
    projectKey: string;
    flagKey: string;
  };
  const search = useSearch({ strict: false }) as EnvSearch;
  const navigate = useNavigate();
  const meQuery = useAuthMe();
  const envsQuery = useQuery({
    queryKey: ["environments", projectKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/environments", {
          params: { path: { projectKey } },
        }),
      ),
  });
  const environments = envsQuery.data?.items ?? [];
  const activeEnv = environments.find((e) => e.key === search.env) ?? environments[0];

  if (!meQuery.data) return null;

  return (
    <FlagDetailPage
      projectKey={projectKey}
      flagKey={flagKey}
      activeEnv={activeEnv}
      environments={environments}
      userRole={meQuery.data.role}
      onBack={() =>
        void navigate({
          to: "/projects/$projectKey/flags",
          params: { projectKey },
          search: activeEnv ? { env: activeEnv.key } : {},
        })
      }
    />
  );
}

function SegmentsRouteView() {
  const { projectKey } = useParams({ strict: false }) as { projectKey: string };
  const search = useSearch({ strict: false }) as EnvSearch;
  const meQuery = useAuthMe();
  const envsQuery = useQuery({
    queryKey: ["environments", projectKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/environments", {
          params: { path: { projectKey } },
        }),
      ),
  });
  const environments = envsQuery.data?.items ?? [];
  const activeEnv = environments.find((e) => e.key === search.env) ?? environments[0];

  if (!meQuery.data) return null;

  return (
    <SegmentsPage
      projectKey={projectKey}
      activeEnv={activeEnv}
      userRole={meQuery.data.role}
    />
  );
}

function PipelinesRouteView() {
  const { projectKey } = useParams({ strict: false }) as { projectKey: string };
  const meQuery = useAuthMe();
  const envsQuery = useQuery({
    queryKey: ["environments", projectKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/environments", {
          params: { path: { projectKey } },
        }),
      ),
  });
  if (!meQuery.data) return null;
  return (
    <PipelinesPage
      projectKey={projectKey}
      environments={envsQuery.data?.items ?? []}
      userRole={meQuery.data.role}
    />
  );
}

function DebuggerRouteView() {
  const { projectKey } = useParams({ strict: false }) as { projectKey: string };
  const search = useSearch({ strict: false }) as EnvSearch;
  const envsQuery = useQuery({
    queryKey: ["environments", projectKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/environments", {
          params: { path: { projectKey } },
        }),
      ),
  });
  const environments = envsQuery.data?.items ?? [];
  const activeEnv = environments.find((e) => e.key === search.env) ?? environments[0];
  return <LiveDebuggerPage projectKey={projectKey} activeEnv={activeEnv} />;
}

function ApprovalsRouteView() {
  const meQuery = useAuthMe();
  if (!meQuery.data) return null;
  return <ApprovalsPage userRole={meQuery.data.role} />;
}

function SettingsEnvsRouteView() {
  const meQuery = useAuthMe();
  const projectsQuery = useQuery({
    queryKey: ["projects"],
    queryFn: async () => unwrapApiResult(await skotosApi.GET("/api/v1/projects")),
  });
  const projectKey = projectsQuery.data?.items[0]?.key ?? "default";
  if (!meQuery.data) return null;
  return <EnvironmentsSettingsPage projectKey={projectKey} userRole={meQuery.data.role} />;
}

function SettingsProjectsRouteView() {
  const meQuery = useAuthMe();
  if (!meQuery.data) return null;
  return <ProjectsSettingsPage userRole={meQuery.data.role} />;
}

function SettingsWebhooksRouteView() {
  const meQuery = useAuthMe();
  if (!meQuery.data) return null;
  return <WebhooksPage userRole={meQuery.data.role} />;
}

function SettingsTokensRouteView() {
  const meQuery = useAuthMe();
  if (!meQuery.data) return null;
  return <TokensSettingsPage userRole={meQuery.data.role} />;
}

function SettingsRolesRouteView() {
  const meQuery = useAuthMe();
  if (!meQuery.data) return null;
  return <RolesSettingsPage userRole={meQuery.data.role} />;
}

function SettingsTeamsRouteView() {
  const meQuery = useAuthMe();
  if (!meQuery.data) return null;
  return <TeamsSsoSettingsPage userRole={meQuery.data.role} />;
}

function SettingsMembersRouteView() {
  const meQuery = useAuthMe();
  if (!meQuery.data) return null;
  return <MembersSettingsPage userRole={meQuery.data.role} />;
}

function buildAppRouter(initialEntries?: string[]) {
  const rootRoute = createRootRoute({ component: RootLayout });

  const indexRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/",
    component: () => <Spinner label="Redirecting to workspace" />,
  });

  const loginRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/login",
    component: () => null,
  });

  const authCallbackRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/auth/callback",
    component: () => null,
  });

  const flagsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/projects/$projectKey/flags",
    validateSearch: (s: Record<string, unknown>): EnvSearch => ({
      env: typeof s.env === "string" ? s.env : undefined,
    }),
    component: FlagsRouteView,
  });

  const flagDetailRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/projects/$projectKey/flags/$flagKey",
    validateSearch: (s: Record<string, unknown>): EnvSearch => ({
      env: typeof s.env === "string" ? s.env : undefined,
    }),
    component: FlagDetailRouteView,
  });

  const segmentsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/projects/$projectKey/segments",
    validateSearch: (s: Record<string, unknown>): EnvSearch => ({
      env: typeof s.env === "string" ? s.env : undefined,
    }),
    component: SegmentsRouteView,
  });

  const pipelinesRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/projects/$projectKey/pipelines",
    validateSearch: (s: Record<string, unknown>): EnvSearch => ({
      env: typeof s.env === "string" ? s.env : undefined,
    }),
    component: PipelinesRouteView,
  });

  const debuggerRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/projects/$projectKey/debugger",
    validateSearch: (s: Record<string, unknown>): EnvSearch => ({
      env: typeof s.env === "string" ? s.env : undefined,
    }),
    component: DebuggerRouteView,
  });

  const approvalsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/approvals",
    component: ApprovalsRouteView,
  });

  const auditLogRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/audit-log",
    component: AuditLogPage,
  });

  const settingsProjectsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/settings/projects",
    component: SettingsProjectsRouteView,
  });

  const settingsEnvsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/settings/environments",
    component: SettingsEnvsRouteView,
  });

  const settingsWebhooksRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/settings/webhooks",
    component: SettingsWebhooksRouteView,
  });

  const settingsTokensRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/settings/tokens",
    component: SettingsTokensRouteView,
  });

  const settingsRolesRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/settings/roles",
    component: SettingsRolesRouteView,
  });

  const settingsTeamsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/settings/teams",
    component: SettingsTeamsRouteView,
  });

  const settingsMembersRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/settings/members",
    component: SettingsMembersRouteView,
  });

  const routeTree = rootRoute.addChildren([
    indexRoute,
    loginRoute,
    authCallbackRoute,
    flagsRoute,
    flagDetailRoute,
    segmentsRoute,
    pipelinesRoute,
    debuggerRoute,
    approvalsRoute,
    auditLogRoute,
    settingsProjectsRoute,
    settingsEnvsRoute,
    settingsWebhooksRoute,
    settingsTokensRoute,
    settingsRolesRoute,
    settingsTeamsRoute,
    settingsMembersRoute,
  ]);

  const history = initialEntries
    ? createMemoryHistory({ initialEntries })
    : createBrowserHistory();

  return createRouter({ routeTree, history });
}

export interface AppProps {
  initialEntries?: string[];
  queryClient?: QueryClient;
}

export function App({ initialEntries, queryClient }: AppProps = {}) {
  const client = useMemo(
    () =>
      queryClient ??
      new QueryClient({
        defaultOptions: {
          queries: {
            refetchOnWindowFocus: false,
            retry: false,
          },
        },
      }),
    [queryClient],
  );
  const router = useMemo(() => buildAppRouter(initialEntries), [initialEntries]);

  return (
    <ThemeProvider>
      <TooltipProvider>
        <ToastProvider>
          <QueryClientProvider client={client}>
            <RouterProvider router={router} />
          </QueryClientProvider>
        </ToastProvider>
      </TooltipProvider>
    </ThemeProvider>
  );
}
