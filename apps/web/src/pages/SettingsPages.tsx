import {
  Alert,
  AlertDialog,
  Badge,
  Button,
  Card,
  CodeBlock,
  DataTable,
  Dialog,
  FormField,
  Heading,
  Icons,
  Input,
  PageHeader,
  Select,
  Stack,
  StatusDot,
  Switch,
  Text,
  useToast,
} from "@skotos/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import {
  canAdmin,
  canOwner,
  formatErrorMessage,
  skotosApi,
  unwrapApiResult,
  type BuiltInRole,
  type CustomRole,
  type Environment,
  type Member,
} from "../lib/api";

// =============================================================================
// Projects Settings Page
// =============================================================================

export function ProjectsSettingsPage({ userRole }: { userRole: BuiltInRole }) {
  const qc = useQueryClient();
  const { toast } = useToast();
  const admin = canAdmin(userRole);

  const [createOpen, setCreateOpen] = useState(false);
  const [key, setKey] = useState("");
  const [name, setName] = useState("");
  const [deleteKey, setDeleteKey] = useState<string | null>(null);

  const projectsQuery = useQuery({
    queryKey: ["projects"],
    queryFn: async () => unwrapApiResult(await skotosApi.GET("/api/v1/projects")),
  });

  const createMutation = useMutation({
    mutationFn: async () =>
      unwrapApiResult(
        await skotosApi.POST("/api/v1/projects", {
          body: { key: key.trim(), name: name.trim(), seedDefaultEnvironments: true },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["projects"] });
      setCreateOpen(false);
      setKey("");
      setName("");
      toast({ title: "Project created", tone: "success" });
    },
    onError: (err) => toast({ title: "Error", description: formatErrorMessage(err), tone: "danger" }),
  });

  const deleteMutation = useMutation({
    mutationFn: async (projectKey: string) =>
      unwrapApiResult(
        await skotosApi.DELETE("/api/v1/projects/{projectKey}", {
          params: { path: { projectKey } },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["projects"] });
      setDeleteKey(null);
      toast({ title: "Project deleted", tone: "success" });
    },
    onError: (err) => toast({ title: "Error", description: formatErrorMessage(err), tone: "danger" }),
  });

  return (
    <Stack gap={5}>
      <PageHeader
        title={<Heading level={1}>Projects</Heading>}
        actions={
          admin && (
            <Button variant="primary" onClick={() => setCreateOpen(true)}>
              <Icons.Plus size={14} />
              New project
            </Button>
          )
        }
      />

      <DataTable
        ariaLabel="Projects table"
        rows={projectsQuery.data?.items ?? []}
        rowKey={(p) => p.id}
        columns={[
          { key: "name", header: "Name", render: (p) => <Text bold>{p.name}</Text> },
          {
            key: "key",
            header: "Key",
            render: (p) => (
              <Text size="xs" mono tone="muted">
                {p.key}
              </Text>
            ),
          },
          {
            key: "actions",
            header: "Actions",
            width: "120px",
            render: (p) =>
              admin ? (
                <Button size="sm" variant="danger" onClick={() => setDeleteKey(p.key)}>
                  Delete
                </Button>
              ) : null,
          },
        ]}
      />

      <Dialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        title="Create Project"
        footer={
          <>
            <Button variant="secondary" onClick={() => setCreateOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={!key.trim() || !name.trim() || createMutation.isPending}
              onClick={() => createMutation.mutate()}
            >
              Create
            </Button>
          </>
        }
      >
        <Stack gap={3}>
          <FormField label="Project Name" htmlFor="proj-name" required>
            <Input id="proj-name" value={name} onChange={(e) => setName(e.target.value)} />
          </FormField>
          <FormField label="Project Key" htmlFor="proj-key" required>
            <Input id="proj-key" mono value={key} onChange={(e) => setKey(e.target.value)} />
          </FormField>
        </Stack>
      </Dialog>

      <AlertDialog
        open={Boolean(deleteKey)}
        onOpenChange={(o) => {
          if (!o) setDeleteKey(null);
        }}
        title={`Delete project '${deleteKey ?? ""}'?`}
        description="Deletes all environments and feature flags inside this project."
        confirmLabel="Delete project"
        onConfirm={() => {
          if (deleteKey) deleteMutation.mutate(deleteKey);
        }}
      />
    </Stack>
  );
}

// =============================================================================
// Environments & Zero-Downtime Key Rotation Page
// =============================================================================

export function EnvironmentsSettingsPage({
  projectKey,
  userRole,
}: {
  projectKey: string;
  userRole: BuiltInRole;
}) {
  const qc = useQueryClient();
  const { toast } = useToast();
  const admin = canAdmin(userRole);

  const [createOpen, setCreateOpen] = useState(false);
  const [envKey, setEnvKey] = useState("");
  const [envName, setEnvName] = useState("");
  const [envColor, setEnvColor] = useState("#8B5CF6");
  const [reqComments, setReqComments] = useState(false);
  const [confirmChanges, setConfirmChanges] = useState(false);

  const [selectedEnv, setSelectedEnv] = useState<Environment | null>(null);
  const [rotatedSecret, setRotatedSecret] = useState<string | null>(null);

  const envsQuery = useQuery({
    queryKey: ["environments", projectKey],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/environments", {
          params: { path: { projectKey } },
        }),
      ),
    enabled: Boolean(projectKey),
  });

  const keysQuery = useQuery({
    queryKey: ["env-keys", projectKey, selectedEnv?.key],
    queryFn: async () =>
      unwrapApiResult(
        await skotosApi.GET("/api/v1/projects/{projectKey}/environments/{envKey}/keys", {
          params: { path: { projectKey, envKey: selectedEnv!.key } },
        }),
      ),
    enabled: Boolean(projectKey && selectedEnv?.key),
  });

  const createMutation = useMutation({
    mutationFn: async () =>
      unwrapApiResult(
        await skotosApi.POST("/api/v1/projects/{projectKey}/environments", {
          params: { path: { projectKey } },
          body: {
            key: envKey.trim(),
            name: envName.trim(),
            color: envColor.trim(),
            requireComments: reqComments,
            confirmChanges,
          },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["environments", projectKey] });
      setCreateOpen(false);
      setEnvKey("");
      setEnvName("");
      toast({ title: "Environment created", tone: "success" });
    },
    onError: (err) => toast({ title: "Error", description: formatErrorMessage(err), tone: "danger" }),
  });

  const rotateMutation = useMutation({
    mutationFn: async (kind: "sdk" | "mobile") =>
      unwrapApiResult(
        await skotosApi.POST("/api/v1/projects/{projectKey}/environments/{envKey}/keys/rotate", {
          params: { path: { projectKey, envKey: selectedEnv!.key } },
          body: { kind, gracePeriodSeconds: 3600 },
        }),
      ),
    onSuccess: async (res) => {
      await qc.invalidateQueries({ queryKey: ["env-keys", projectKey, selectedEnv?.key] });
      setRotatedSecret(res.plaintextKey);
      toast({ title: "Key rotated (1h grace window active)", tone: "success" });
    },
    onError: (err) => toast({ title: "Error", description: formatErrorMessage(err), tone: "danger" }),
  });

  return (
    <Stack gap={5}>
      <PageHeader
        title={<Heading level={1}>Environments &amp; SDK Keys</Heading>}
        description={
          <Text size="sm" tone="muted">
            Project: {projectKey}
          </Text>
        }
        actions={
          admin && (
            <Button variant="primary" onClick={() => setCreateOpen(true)}>
              <Icons.Plus size={14} />
              New environment
            </Button>
          )
        }
      />

      <DataTable
        ariaLabel="Environments table"
        rows={envsQuery.data?.items ?? []}
        rowKey={(e) => e.id}
        columns={[
          {
            key: "env",
            header: "Environment",
            render: (e) => (
              <Stack direction="horizontal" align="center" gap={2}>
                <StatusDot color={e.color} />
                <Text bold>{e.name}</Text>
                <Text size="xs" mono tone="muted">
                  ({e.key})
                </Text>
              </Stack>
            ),
          },
          {
            key: "cid",
            header: "Client-Side ID",
            render: (e) => (
              <Text size="xs" mono>
                {e.clientSideId}
              </Text>
            ),
          },
          {
            key: "protection",
            header: "Protection",
            render: (e) => (
              <Stack direction="horizontal" gap={1}>
                {e.confirmChanges && <Badge tone="warning">Confirm</Badge>}
                {e.requireComments && <Badge tone="danger">Comment required</Badge>}
                {!e.confirmChanges && !e.requireComments && <Badge>Standard</Badge>}
              </Stack>
            ),
          },
          {
            key: "keys",
            header: "Credentials",
            width: "150px",
            render: (e) => (
              <Button size="sm" variant="secondary" onClick={() => setSelectedEnv(e)}>
                <Icons.KeyRound size={13} />
                Manage keys
              </Button>
            ),
          },
        ]}
      />

      {/* Create Environment Dialog */}
      <Dialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        title="Create Environment"
        footer={
          <>
            <Button variant="secondary" onClick={() => setCreateOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={!envKey.trim() || !envName.trim() || createMutation.isPending}
              onClick={() => createMutation.mutate()}
            >
              Create
            </Button>
          </>
        }
      >
        <Stack gap={3}>
          <FormField label="Name" htmlFor="env-name" required>
            <Input id="env-name" value={envName} onChange={(e) => setEnvName(e.target.value)} />
          </FormField>
          <FormField label="Key" htmlFor="env-key" required>
            <Input id="env-key" mono value={envKey} onChange={(e) => setEnvKey(e.target.value)} />
          </FormField>
          <FormField label="Accent Color (#RRGGBB)" htmlFor="env-color">
            <Input
              id="env-color"
              mono
              value={envColor}
              onChange={(e) => setEnvColor(e.target.value)}
            />
          </FormField>
          <Switch
            checked={confirmChanges}
            onCheckedChange={setConfirmChanges}
            label="Require confirmation dialog before saving changes"
          />
          <Switch
            checked={reqComments}
            onCheckedChange={setReqComments}
            label="Require non-empty change comment"
          />
        </Stack>
      </Dialog>

      {/* Keys & Zero-Downtime Rotation Dialog */}
      <Dialog
        open={Boolean(selectedEnv)}
        onOpenChange={(o) => {
          if (!o) {
            setSelectedEnv(null);
            setRotatedSecret(null);
          }
        }}
        wide
        title={`Credentials — ${selectedEnv?.name ?? ""}`}
      >
        <Stack gap={4}>
          {keysQuery.data && (
            <CodeBlock label="Client-Side ID (Public)" code={keysQuery.data.clientSideId} />
          )}

          {rotatedSecret && (
            <Alert tone="warning" title="Copy your new key now (shown once)">
              <CodeBlock label="New Secret Key" code={rotatedSecret} />
            </Alert>
          )}

          <Card padded>
            <Stack gap={3}>
              <Text bold size="sm">
                Active Secret Key Prefixes
              </Text>
              {(keysQuery.data?.keys ?? []).map((k) => (
                <Stack
                  key={k.id}
                  direction="horizontal"
                  align="center"
                  justify="space-between"
                >
                  <Stack direction="horizontal" align="center" gap={2}>
                    <Badge tone="accent">{k.kind.toUpperCase()}</Badge>
                    <Text as="span" size="xs" mono>
                      {k.keyPrefix}...
                    </Text>
                  </Stack>
                  {k.expiresAt && (
                    <Badge tone="warning">
                      Expires {new Date(k.expiresAt).toLocaleTimeString()}
                    </Badge>
                  )}
                </Stack>
              ))}
              {admin && (
                <Stack direction="horizontal" gap={2}>
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={rotateMutation.isPending}
                    onClick={() => rotateMutation.mutate("sdk")}
                  >
                    Rotate SDK Key (1h grace)
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={rotateMutation.isPending}
                    onClick={() => rotateMutation.mutate("mobile")}
                  >
                    Rotate Mobile Key (1h grace)
                  </Button>
                </Stack>
              )}
            </Stack>
          </Card>
        </Stack>
      </Dialog>
    </Stack>
  );
}

// =============================================================================
// API Access Tokens Page
// =============================================================================

export function TokensSettingsPage({ userRole }: { userRole: BuiltInRole }) {
  const qc = useQueryClient();
  const { toast } = useToast();
  const admin = canAdmin(userRole);
  const owner = canOwner(userRole);

  const [name, setName] = useState("");
  const [role, setRole] = useState<BuiltInRole>("writer");
  const [createdSecret, setCreatedSecret] = useState<string | null>(null);
  const [revokeId, setRevokeId] = useState<string | null>(null);

  const tokensQuery = useQuery({
    queryKey: ["tokens"],
    queryFn: async () => unwrapApiResult(await skotosApi.GET("/api/v1/tokens")),
    enabled: admin,
  });

  const createMutation = useMutation({
    mutationFn: async () =>
      unwrapApiResult(
        await skotosApi.POST("/api/v1/tokens", { body: { name: name.trim(), role } }),
      ),
    onSuccess: async (res) => {
      await qc.invalidateQueries({ queryKey: ["tokens"] });
      setName("");
      setCreatedSecret(res.plaintextToken);
      toast({ title: "API token created", tone: "success" });
    },
    onError: (err) => toast({ title: "Error", description: formatErrorMessage(err), tone: "danger" }),
  });

  const revokeMutation = useMutation({
    mutationFn: async (tokenId: string) =>
      unwrapApiResult(
        await skotosApi.DELETE("/api/v1/tokens/{tokenId}", {
          params: { path: { tokenId } },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["tokens"] });
      setRevokeId(null);
      toast({ title: "API token revoked", tone: "success" });
    },
  });

  if (!admin) {
    return (
      <Alert tone="warning" title="Admin role required">
        Only Admins and Owners can manage API access tokens.
      </Alert>
    );
  }

  return (
    <Stack gap={5}>
      <PageHeader title={<Heading level={1}>API Access Tokens</Heading>} />

      {createdSecret && (
        <Alert tone="warning" title="Copy your API token now (it will never be shown again)">
          <CodeBlock label="Personal / Service API Token" code={createdSecret} />
        </Alert>
      )}

      <Card padded>
        <Stack direction="horizontal" gap={3} align="flex-end">
          <FormField label="Token Name" htmlFor="token-name">
            <Input
              id="token-name"
              placeholder="GitHub Actions CI"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </FormField>
          <FormField label="Role" htmlFor="token-role">
            <Select
              id="token-role"
              value={role}
              onValueChange={(v) => setRole(v as BuiltInRole)}
              options={[
                { value: "reader", label: "Reader" },
                { value: "writer", label: "Writer" },
                { value: "admin", label: "Admin" },
                ...(owner ? [{ value: "owner", label: "Owner" }] : []),
              ]}
            />
          </FormField>
          <Button
            variant="primary"
            disabled={!name.trim() || createMutation.isPending}
            onClick={() => createMutation.mutate()}
          >
            Create token
          </Button>
        </Stack>
      </Card>

      <DataTable
        ariaLabel="API tokens table"
        rows={tokensQuery.data?.items ?? []}
        rowKey={(t) => t.id}
        columns={[
          { key: "name", header: "Name", render: (t) => <Text bold>{t.name}</Text> },
          {
            key: "prefix",
            header: "Prefix",
            render: (t) => (
              <Text size="xs" mono>
                {t.keyPrefix}...
              </Text>
            ),
          },
          { key: "role", header: "Role", render: (t) => <Badge tone="accent">{t.role}</Badge> },
          {
            key: "actions",
            header: "Actions",
            width: "120px",
            render: (t) => (
              <Button size="sm" variant="danger" onClick={() => setRevokeId(t.id)}>
                Revoke
              </Button>
            ),
          },
        ]}
      />

      <AlertDialog
        open={Boolean(revokeId)}
        onOpenChange={(o) => {
          if (!o) setRevokeId(null);
        }}
        title="Revoke API token?"
        description="Any scripts or CI pipelines using this token will immediately fail with 401."
        confirmLabel="Revoke token"
        onConfirm={() => {
          if (revokeId) revokeMutation.mutate(revokeId);
        }}
      />
    </Stack>
  );
}

// =============================================================================
// Members & RBAC Page
// =============================================================================

const CUSTOM_ROLE_PREFIX = "custom:";

/** Built-in role a member keeps when a custom role is picked; the custom policy grants the rest. */
const CUSTOM_ROLE_BASE: BuiltInRole = "reader";

interface RoleSelection {
  role: BuiltInRole;
  customRoleKeys: string[];
}

function memberRoleValue(m: Pick<Member, "role" | "customRoleKeys">): string {
  const firstCustom = m.customRoleKeys?.[0];
  return firstCustom ? `${CUSTOM_ROLE_PREFIX}${firstCustom}` : m.role;
}

function parseRoleValue(value: string): RoleSelection {
  if (value.startsWith(CUSTOM_ROLE_PREFIX)) {
    return { role: CUSTOM_ROLE_BASE, customRoleKeys: [value.slice(CUSTOM_ROLE_PREFIX.length)] };
  }
  return { role: value as BuiltInRole, customRoleKeys: [] };
}

function roleOptions(owner: boolean, customRoles: CustomRole[]) {
  const builtIn = [
    { value: "reader", label: "Reader" },
    { value: "writer", label: "Writer" },
    { value: "admin", label: "Admin" },
    ...(owner ? [{ value: "owner", label: "Owner" }] : []),
  ].map((o) => (customRoles.length ? { ...o, group: "Built-in" } : o));
  const custom = customRoles.map((r) => ({
    value: `${CUSTOM_ROLE_PREFIX}${r.key}`,
    label: r.name || r.key,
    group: "Custom",
  }));
  return [...builtIn, ...custom];
}

function memberRoleLabel(m: Member, customRoles: CustomRole[]): string {
  const keys = m.customRoleKeys ?? [];
  if (!keys.length) return m.role;
  return keys.map((k) => customRoles.find((r) => r.key === k)?.name || k).join(", ");
}

export function MembersSettingsPage({ userRole }: { userRole: BuiltInRole }) {
  const qc = useQueryClient();
  const { toast } = useToast();
  const admin = canAdmin(userRole);
  const owner = canOwner(userRole);

  const [email, setEmail] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [roleValue, setRoleValue] = useState<string>("writer");

  const membersQuery = useQuery({
    queryKey: ["members"],
    queryFn: async () => unwrapApiResult(await skotosApi.GET("/api/v1/members")),
  });

  // Shares the cache key with the Custom RBAC Roles page, so new roles show up here.
  const customRolesQuery = useQuery({
    queryKey: ["custom-roles"],
    queryFn: async () => unwrapApiResult(await skotosApi.GET("/api/v1/roles")),
  });
  const customRoles = customRolesQuery.data?.items ?? [];
  const options = roleOptions(owner, customRoles);

  const createMutation = useMutation({
    mutationFn: async () => {
      const { role, customRoleKeys } = parseRoleValue(roleValue);
      return unwrapApiResult(
        await skotosApi.POST("/api/v1/members", {
          body: {
            email: email.trim(),
            displayName: displayName.trim(),
            role,
            customRoleKeys,
            password: password || undefined,
          },
        }),
      );
    },
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["members"] });
      setEmail("");
      setDisplayName("");
      setPassword("");
      toast({ title: "Member added", tone: "success" });
    },
    onError: (err) => toast({ title: "Error", description: formatErrorMessage(err), tone: "danger" }),
  });

  const updateRoleMutation = useMutation({
    mutationFn: async (args: { memberId: string } & RoleSelection) =>
      unwrapApiResult(
        await skotosApi.PATCH("/api/v1/members/{memberId}", {
          params: { path: { memberId: args.memberId } },
          body: { role: args.role, customRoleKeys: args.customRoleKeys },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["members"] });
      toast({ title: "Member role updated", tone: "success" });
    },
    onError: (err) => toast({ title: "Error", description: formatErrorMessage(err), tone: "danger" }),
  });

  const deleteMutation = useMutation({
    mutationFn: async (memberId: string) =>
      unwrapApiResult(
        await skotosApi.DELETE("/api/v1/members/{memberId}", {
          params: { path: { memberId } },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["members"] });
      toast({ title: "Member removed", tone: "success" });
    },
    onError: (err) => toast({ title: "Error", description: formatErrorMessage(err), tone: "danger" }),
  });

  return (
    <Stack gap={5}>
      <PageHeader title={<Heading level={1}>Team Members &amp; Roles</Heading>} />

      {admin && (
        <Card padded>
          <Stack direction="horizontal" gap={3} align="flex-end">
            <FormField label="Email" htmlFor="mem-email">
              <Input
                id="mem-email"
                type="email"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
              />
            </FormField>
            <FormField label="Display Name" htmlFor="mem-name">
              <Input
                id="mem-name"
                value={displayName}
                onChange={(e) => setDisplayName(e.target.value)}
              />
            </FormField>
            <FormField label="Password (Self-Hosted)" htmlFor="mem-pwd">
              <Input
                id="mem-pwd"
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
            </FormField>
            <FormField label="Role" htmlFor="mem-role">
              <Select
                id="mem-role"
                value={roleValue}
                onValueChange={setRoleValue}
                options={options}
              />
            </FormField>
            <Button
              variant="primary"
              disabled={!email.trim() || !displayName.trim() || createMutation.isPending}
              onClick={() => createMutation.mutate()}
            >
              Add member
            </Button>
          </Stack>
        </Card>
      )}

      <DataTable
        ariaLabel="Organization members table"
        rows={membersQuery.data?.items ?? []}
        rowKey={(m) => m.id}
        columns={[
          {
            key: "member",
            header: "Member",
            render: (m) => (
              <Stack gap={1}>
                <Text bold>{m.displayName}</Text>
                <Text size="xs" tone="muted">
                  {m.email}
                </Text>
              </Stack>
            ),
          },
          {
            key: "role",
            header: "Role",
            width: "180px",
            render: (m) =>
              admin && (owner || m.role !== "owner") ? (
                <Select
                  ariaLabel={`Role for ${m.email}`}
                  value={memberRoleValue(m)}
                  onValueChange={(v) =>
                    updateRoleMutation.mutate({ memberId: m.id, ...parseRoleValue(v) })
                  }
                  options={options}
                />
              ) : (
                <Badge tone="accent">{memberRoleLabel(m, customRoles)}</Badge>
              ),
          },
          {
            key: "actions",
            header: "Actions",
            width: "120px",
            render: (m) =>
              admin && (owner || m.role !== "owner") ? (
                <Button size="sm" variant="danger" onClick={() => deleteMutation.mutate(m.id)}>
                  Remove
                </Button>
              ) : null,
          },
        ]}
      />
    </Stack>
  );
}
