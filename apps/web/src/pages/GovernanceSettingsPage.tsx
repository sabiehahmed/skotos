import {
  Alert,
  AlertDialog,
  Badge,
  Button,
  Card,
  Checkbox,
  CodeBlock,
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
  type CustomRole,
  type PolicyEffect,
  type PolicyStatement,
  type SsoProtocol,
  type SsoProviderConfig,
  type Team,
} from "../lib/api";

// =============================================================================
// Custom RBAC Roles Settings Page (/settings/roles)
// =============================================================================

export interface GovernanceSettingsProps {
  userRole: BuiltInRole;
}

export function RolesSettingsPage({ userRole }: GovernanceSettingsProps) {
  const qc = useQueryClient();
  const { toast } = useToast();
  const adminAllowed = canAdmin(userRole);

  const [createOpen, setCreateOpen] = useState(false);
  const [roleKey, setRoleKey] = useState("");
  const [roleName, setRoleName] = useState("");
  const [roleDesc, setRoleDesc] = useState("");
  const [effect, setEffect] = useState<PolicyEffect>("deny");
  const [actions, setActions] = useState<string[]>(["*"]);
  const [resources, setResources] = useState<string[]>(["proj/*:env/production:flag/*"]);
  const [formErr, setFormErr] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<CustomRole | null>(null);

  const rolesQuery = useQuery({
    queryKey: ["custom-roles"],
    queryFn: async () => unwrapApiResult(await skotosApi.GET("/api/v1/roles")),
  });

  const createMutation = useMutation({
    mutationFn: async () => {
      const statements: PolicyStatement[] = [
        {
          effect,
          actions,
          resources,
        },
      ];
      return unwrapApiResult(
        await skotosApi.POST("/api/v1/roles", {
          body: {
            key: roleKey.trim(),
            name: roleName.trim(),
            description: roleDesc.trim(),
            statements,
          },
        }),
      );
    },
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["custom-roles"] });
      setCreateOpen(false);
      setRoleKey("");
      setRoleName("");
      setRoleDesc("");
      setFormErr(null);
      toast({ title: "Custom role created", tone: "success" });
    },
    onError: (err) => setFormErr(formatErrorMessage(err)),
  });

  const deleteMutation = useMutation({
    mutationFn: async (key: string) =>
      unwrapApiResult(
        await skotosApi.DELETE("/api/v1/roles/{roleKey}", {
          params: { path: { roleKey: key } },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["custom-roles"] });
      setDeleteTarget(null);
      toast({ title: "Custom role deleted", tone: "success" });
    },
    onError: (err) =>
      toast({ title: "Delete failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const roles = rolesQuery.data?.items ?? [];

  return (
    <Stack gap={5}>
      <PageHeader
        title={<Heading level={1}>Custom RBAC Roles</Heading>}
        description={
          <Text tone="muted" size="sm">
            Define fine-grained resource-path allow/deny policies (explicit{" "}
            <Text as="span" mono>
              deny
            </Text>{" "}
            always overrides{" "}
            <Text as="span" mono>
              allow
            </Text>
            ).
          </Text>
        }
        actions={
          <Button
            variant="primary"
            disabled={!adminAllowed}
            onClick={() => {
              setFormErr(null);
              setCreateOpen(true);
            }}
          >
            <Icons.Shield size={14} />
            Create Custom Role
          </Button>
        }
      />

      {rolesQuery.isError && (
        <Alert tone="danger" title="Could not load custom roles">
          {formatErrorMessage(rolesQuery.error)}
        </Alert>
      )}

      {rolesQuery.isPending ? (
        <Skeleton height={120} />
      ) : (
        <DataTable
          ariaLabel="Custom RBAC roles"
          rows={roles}
          rowKey={(r) => r.id}
          emptyTitle="No custom roles defined"
          emptyDescription="Create custom RBAC policies to guard production flags or delegate project-level write access."
          columns={[
            {
              key: "name",
              header: "Role",
              render: (r) => (
                <Stack gap={1}>
                  <Text bold>{r.name}</Text>
                  <Text size="xs" mono tone="muted">
                    {r.key}
                  </Text>
                </Stack>
              ),
            },
            {
              key: "statements",
              header: "Policy Statements",
              render: (r) => (
                <Stack gap={1}>
                  {(r.statements ?? []).map((st, idx) => (
                    <Stack key={idx} direction="horizontal" align="center" gap={2} style={{ flexWrap: "wrap" }}>
                      <Badge tone={st.effect === "deny" ? "danger" : "success"}>{st.effect}</Badge>
                      <Text size="xs" mono>
                        {(st.actions ?? []).join(", ")} on {(st.resources ?? []).join(", ")}
                      </Text>
                    </Stack>
                  ))}
                </Stack>
              ),
            },
            {
              key: "actions",
              header: "Actions",
              width: "100px",
              render: (r) =>
                adminAllowed ? (
                  <Button
                    size="sm"
                    variant="ghost"
                    aria-label={`Delete role ${r.name}`}
                    onClick={() => setDeleteTarget(r)}
                  >
                    <Icons.Trash2 size={14} aria-hidden="true" />
                  </Button>
                ) : null,
            },
          ]}
        />
      )}

      <Dialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        wide
        title="Create custom RBAC role"
        description="Attach resource path globs such as proj/*:env/production:flag/*."
        footer={
          <>
            <Button variant="secondary" onClick={() => setCreateOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={!roleKey.trim() || !roleName.trim() || createMutation.isPending}
              onClick={() => createMutation.mutate()}
            >
              {createMutation.isPending ? "Creating..." : "Create role"}
            </Button>
          </>
        }
      >
        <Stack gap={4}>
          {formErr && <Alert tone="danger">{formErr}</Alert>}
          <FormField label="Role Key" htmlFor="role-key" required>
            <Input
              id="role-key"
              mono
              placeholder="prod-guard"
              value={roleKey}
              onChange={(e) => setRoleKey(e.target.value)}
            />
          </FormField>
          <FormField label="Role Name" htmlFor="role-name" required>
            <Input
              id="role-name"
              placeholder="Production Write Guard"
              value={roleName}
              onChange={(e) => setRoleName(e.target.value)}
            />
          </FormField>
          <FormField label="Description" htmlFor="role-desc">
            <Input
              id="role-desc"
              placeholder="Prevents direct flag modifications in production"
              value={roleDesc}
              onChange={(e) => setRoleDesc(e.target.value)}
            />
          </FormField>
          <FormField label="Policy Effect" htmlFor="role-effect">
            <Select
              id="role-effect"
              ariaLabel="Policy effect"
              value={effect}
              options={[
                { value: "deny", label: "Deny (overrides Allow)" },
                { value: "allow", label: "Allow" },
              ]}
              onValueChange={(v) => setEffect(v as PolicyEffect)}
            />
          </FormField>
          <FormField label="Actions (e.g. *, patchFlag, createFlag)">
            <TagInput ariaLabel="Policy actions" values={actions} onChange={setActions} />
          </FormField>
          <FormField label="Resource Paths (e.g. proj/*:env/production:flag/*)">
            <TagInput ariaLabel="Policy resources" values={resources} onChange={setResources} />
          </FormField>
        </Stack>
      </Dialog>

      <AlertDialog
        open={deleteTarget !== null}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
        title={`Delete role '${deleteTarget?.name ?? ""}'?`}
        description="Members and teams inheriting this role will immediately lose its policy statements."
        confirmLabel="Delete role"
        onConfirm={() => deleteTarget && deleteMutation.mutate(deleteTarget.key)}
      />
    </Stack>
  );
}

// =============================================================================
// Teams, Enterprise SSO/SCIM & Audit Archives Page (/settings/teams)
// =============================================================================

export function TeamsSsoSettingsPage({ userRole }: GovernanceSettingsProps) {
  const qc = useQueryClient();
  const { toast } = useToast();
  const adminAllowed = canAdmin(userRole);

  // Team create modal
  const [teamOpen, setTeamOpen] = useState(false);
  const [teamKey, setTeamKey] = useState("");
  const [teamName, setTeamName] = useState("");
  const [teamDesc, setTeamDesc] = useState("");
  const [teamRoles, setTeamRoles] = useState<string[]>([]);
  const [teamErr, setTeamErr] = useState<string | null>(null);
  const [deleteTeam, setDeleteTeam] = useState<Team | null>(null);

  // SSO provider create modal
  const [ssoOpen, setSsoOpen] = useState(false);
  const [ssoName, setSsoName] = useState("");
  const [ssoProto, setSsoProto] = useState<SsoProtocol>("oidc");
  const [ssoIssuer, setSsoIssuer] = useState("");
  const [ssoClientId, setSsoClientId] = useState("");
  const [ssoScim, setSsoScim] = useState(true);
  const [ssoErr, setSsoErr] = useState<string | null>(null);
  const [deleteSso, setDeleteSso] = useState<SsoProviderConfig | null>(null);

  const teamsQuery = useQuery({
    queryKey: ["teams"],
    queryFn: async () => unwrapApiResult(await skotosApi.GET("/api/v1/teams")),
  });

  const ssoQuery = useQuery({
    queryKey: ["sso-providers"],
    queryFn: async () => unwrapApiResult(await skotosApi.GET("/api/v1/sso/providers")),
  });

  const archivesQuery = useQuery({
    queryKey: ["audit-archives"],
    queryFn: async () => unwrapApiResult(await skotosApi.GET("/api/v1/audit-log/archives")),
  });

  const createTeamMutation = useMutation({
    mutationFn: async () =>
      unwrapApiResult(
        await skotosApi.POST("/api/v1/teams", {
          body: {
            key: teamKey.trim(),
            name: teamName.trim(),
            description: teamDesc.trim(),
            customRoleKeys: teamRoles,
          },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["teams"] });
      setTeamOpen(false);
      setTeamKey("");
      setTeamName("");
      setTeamDesc("");
      setTeamRoles([]);
      setTeamErr(null);
      toast({ title: "Team created", tone: "success" });
    },
    onError: (err) => setTeamErr(formatErrorMessage(err)),
  });

  const deleteTeamMutation = useMutation({
    mutationFn: async (key: string) =>
      unwrapApiResult(
        await skotosApi.DELETE("/api/v1/teams/{teamKey}", {
          params: { path: { teamKey: key } },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["teams"] });
      setDeleteTeam(null);
      toast({ title: "Team deleted", tone: "success" });
    },
    onError: (err) =>
      toast({ title: "Delete failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const createSsoMutation = useMutation({
    mutationFn: async () =>
      unwrapApiResult(
        await skotosApi.POST("/api/v1/sso/providers", {
          body: {
            name: ssoName.trim(),
            protocol: ssoProto,
            issuerUrl: ssoIssuer.trim(),
            clientId: ssoClientId.trim() || null,
            scimEnabled: ssoScim,
          },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["sso-providers"] });
      setSsoOpen(false);
      setSsoName("");
      setSsoIssuer("");
      setSsoClientId("");
      setSsoErr(null);
      toast({ title: "SSO provider configured", tone: "success" });
    },
    onError: (err) => setSsoErr(formatErrorMessage(err)),
  });

  const deleteSsoMutation = useMutation({
    mutationFn: async (id: string) =>
      unwrapApiResult(
        await skotosApi.DELETE("/api/v1/sso/providers/{providerId}", {
          params: { path: { providerId: id } },
        }),
      ),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["sso-providers"] });
      setDeleteSso(null);
      toast({ title: "SSO provider removed", tone: "success" });
    },
    onError: (err) =>
      toast({ title: "Delete failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  const exportArchiveMutation = useMutation({
    mutationFn: async () =>
      unwrapApiResult(
        await skotosApi.POST("/api/v1/audit-log/export", {
          body: {},
        }),
      ),
    onSuccess: async (rec) => {
      await qc.invalidateQueries({ queryKey: ["audit-archives"] });
      toast({
        title: "Audit log archived to Object Store",
        description: `${rec.recordCount} entries compressed (${rec.compressedBytes} bytes)`,
        tone: "success",
      });
    },
    onError: (err) =>
      toast({ title: "Export failed", description: formatErrorMessage(err), tone: "danger" }),
  });

  return (
    <Stack gap={6}>
      <PageHeader
        title={<Heading level={1}>Teams, SSO/SCIM &amp; Compliance</Heading>}
        description={
          <Text tone="muted" size="sm">
            Organize members into teams with inherited custom roles, configure OIDC/SAML SSO +
            SCIM 2.0 provisioning, and archive immutable Zstd audit snapshots.
          </Text>
        }
        actions={
          <Stack direction="horizontal" gap={2}>
            <Button
              variant="secondary"
              disabled={!adminAllowed}
              onClick={() => {
                setSsoErr(null);
                setSsoOpen(true);
              }}
            >
              <Icons.Shield size={14} />
              Add SSO Provider
            </Button>
            <Button
              variant="primary"
              disabled={!adminAllowed}
              onClick={() => {
                setTeamErr(null);
                setTeamOpen(true);
              }}
            >
              <Icons.Users size={14} />
              Create Team
            </Button>
          </Stack>
        }
      />

      {/* Teams */}
      <Stack gap={3}>
        <Heading level={2}>Teams</Heading>
        {teamsQuery.isPending ? (
          <Skeleton height={100} />
        ) : (
          <DataTable
            ariaLabel="Organization teams"
            rows={teamsQuery.data?.items ?? []}
            rowKey={(t) => t.id}
            emptyTitle="No teams created"
            emptyDescription="Group engineers into teams to assign flag maintainers and custom RBAC roles."
            columns={[
              {
                key: "name",
                header: "Team",
                render: (t) => (
                  <Stack gap={1}>
                    <Text bold>{t.name}</Text>
                    <Text size="xs" mono tone="muted">
                      {t.key}
                    </Text>
                  </Stack>
                ),
              },
              {
                key: "roles",
                header: "Inherited Custom Roles",
                render: (t) => (
                  <Stack direction="horizontal" gap={1} style={{ flexWrap: "wrap" }}>
                    {(t.customRoleKeys ?? []).length === 0 ? (
                      <Text size="xs" tone="muted">
                        None
                      </Text>
                    ) : (
                      (t.customRoleKeys ?? []).map((rk) => (
                        <Badge key={rk} tone="accent">
                          {rk}
                        </Badge>
                      ))
                    )}
                  </Stack>
                ),
              },
              {
                key: "members",
                header: "Members",
                width: "110px",
                render: (t) => <Badge tone="neutral">{(t.memberIds ?? []).length}</Badge>,
              },
              {
                key: "actions",
                header: "Actions",
                width: "90px",
                render: (t) =>
                  adminAllowed ? (
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={`Delete team ${t.name}`}
                      onClick={() => setDeleteTeam(t)}
                    >
                      <Icons.Trash2 size={14} aria-hidden="true" />
                    </Button>
                  ) : null,
              },
            ]}
          />
        )}
      </Stack>

      <Separator />

      {/* SSO & SCIM */}
      <Stack gap={3}>
        <Heading level={2}>Enterprise SSO &amp; SCIM 2.0 Provisioning</Heading>
        <Card padded>
          <Stack gap={2}>
            <Text size="sm" bold>
              SCIM 2.0 Provisioning Endpoints (Okta / Entra ID / JumpCloud)
            </Text>
            <CodeBlock
              label="SCIM Base Paths"
              code={"POST /scim/v2/Users\nGET  /scim/v2/Users\nPOST /scim/v2/Groups"}
            />
          </Stack>
        </Card>
        {ssoQuery.isPending ? (
          <Skeleton height={80} />
        ) : (
          <DataTable
            ariaLabel="Enterprise SSO providers"
            rows={ssoQuery.data?.items ?? []}
            rowKey={(p) => p.id}
            emptyTitle="No SSO providers configured"
            emptyDescription="Connect Okta, Auth0, Entra ID, or Google Workspace via OIDC or SAML 2.0."
            columns={[
              {
                key: "name",
                header: "Provider",
                render: (p) => (
                  <Stack gap={1}>
                    <Stack direction="horizontal" align="center" gap={2}>
                      <Text bold>{p.name}</Text>
                      <Badge tone="accent">{p.protocol.toUpperCase()}</Badge>
                      {p.scimEnabled && <Badge tone="success">SCIM 2.0</Badge>}
                    </Stack>
                    <Text size="xs" mono tone="muted">
                      {p.issuerUrl}
                    </Text>
                  </Stack>
                ),
              },
              {
                key: "actions",
                header: "Actions",
                width: "90px",
                render: (p) =>
                  adminAllowed ? (
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={`Delete SSO provider ${p.name}`}
                      onClick={() => setDeleteSso(p)}
                    >
                      <Icons.Trash2 size={14} aria-hidden="true" />
                    </Button>
                  ) : null,
              },
            ]}
          />
        )}
      </Stack>

      <Separator />

      {/* Immutable Audit Log Archival */}
      <Stack gap={3}>
        <Stack direction="horizontal" align="center" justify="space-between">
          <Heading level={2}>Immutable Audit Log Archives (Zstd NDJSON)</Heading>
          <Button
            variant="secondary"
            size="sm"
            disabled={!adminAllowed || exportArchiveMutation.isPending}
            onClick={() => exportArchiveMutation.mutate()}
          >
            <Icons.Archive size={14} />
            {exportArchiveMutation.isPending ? "Archiving..." : "Export Archive Now"}
          </Button>
        </Stack>
        {archivesQuery.isPending ? (
          <Skeleton height={80} />
        ) : (
          <DataTable
            ariaLabel="Immutable audit log archives"
            rows={archivesQuery.data?.items ?? []}
            rowKey={(a) => a.id}
            emptyTitle="No audit archives exported yet"
            emptyDescription="Export tamper-evident SHA-256 verified Zstd NDJSON archives to R2/S3."
            columns={[
              {
                key: "objectKey",
                header: "Object Key",
                render: (a) => (
                  <Text size="xs" mono>
                    {a.objectKey}
                  </Text>
                ),
              },
              {
                key: "counts",
                header: "Entries / Size",
                width: "160px",
                render: (a) => (
                  <Text size="xs" mono>
                    {a.recordCount} entries ({a.compressedBytes} B)
                  </Text>
                ),
              },
              {
                key: "sha",
                header: "SHA-256",
                width: "180px",
                render: (a) => (
                  <Text size="xs" mono tone="muted">
                    {a.sha256.slice(0, 16)}…
                  </Text>
                ),
              },
            ]}
          />
        )}
      </Stack>

      {/* Create Team Dialog */}
      <Dialog
        open={teamOpen}
        onOpenChange={setTeamOpen}
        title="Create team"
        description="Assign inherited custom RBAC roles to every member of this team."
        footer={
          <>
            <Button variant="secondary" onClick={() => setTeamOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={!teamKey.trim() || !teamName.trim() || createTeamMutation.isPending}
              onClick={() => createTeamMutation.mutate()}
            >
              {createTeamMutation.isPending ? "Creating..." : "Create team"}
            </Button>
          </>
        }
      >
        <Stack gap={4}>
          {teamErr && <Alert tone="danger">{teamErr}</Alert>}
          <FormField label="Team Key" htmlFor="team-key" required>
            <Input
              id="team-key"
              mono
              placeholder="checkout-eng"
              value={teamKey}
              onChange={(e) => setTeamKey(e.target.value)}
            />
          </FormField>
          <FormField label="Team Name" htmlFor="team-name" required>
            <Input
              id="team-name"
              placeholder="Checkout Engineering"
              value={teamName}
              onChange={(e) => setTeamName(e.target.value)}
            />
          </FormField>
          <FormField label="Description" htmlFor="team-desc">
            <Input
              id="team-desc"
              placeholder="Owns payments & cart flags"
              value={teamDesc}
              onChange={(e) => setTeamDesc(e.target.value)}
            />
          </FormField>
          <FormField label="Custom Role Keys (optional)">
            <TagInput
              ariaLabel="Team custom role keys"
              values={teamRoles}
              onChange={setTeamRoles}
              placeholder="e.g. prod-guard"
            />
          </FormField>
        </Stack>
      </Dialog>

      {/* Create SSO Provider Dialog */}
      <Dialog
        open={ssoOpen}
        onOpenChange={setSsoOpen}
        title="Configure Enterprise SSO Provider"
        description="Enable OIDC or SAML 2.0 single sign-on with optional SCIM 2.0 directory sync."
        footer={
          <>
            <Button variant="secondary" onClick={() => setSsoOpen(false)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={!ssoName.trim() || !ssoIssuer.trim() || createSsoMutation.isPending}
              onClick={() => createSsoMutation.mutate()}
            >
              {createSsoMutation.isPending ? "Saving..." : "Save provider"}
            </Button>
          </>
        }
      >
        <Stack gap={4}>
          {ssoErr && <Alert tone="danger">{ssoErr}</Alert>}
          <FormField label="Provider Name" htmlFor="sso-name" required>
            <Input
              id="sso-name"
              placeholder="Okta Workforce"
              value={ssoName}
              onChange={(e) => setSsoName(e.target.value)}
            />
          </FormField>
          <FormField label="Protocol" htmlFor="sso-proto">
            <Select
              id="sso-proto"
              ariaLabel="SSO protocol"
              value={ssoProto}
              options={[
                { value: "oidc", label: "OIDC (OpenID Connect)" },
                { value: "saml", label: "SAML 2.0" },
              ]}
              onValueChange={(v) => setSsoProto(v as SsoProtocol)}
            />
          </FormField>
          <FormField label="Issuer URL" htmlFor="sso-issuer" required>
            <Input
              id="sso-issuer"
              mono
              placeholder="https://company.okta.com/oauth2/default"
              value={ssoIssuer}
              onChange={(e) => setSsoIssuer(e.target.value)}
            />
          </FormField>
          <FormField label="Client ID (optional)" htmlFor="sso-client">
            <Input
              id="sso-client"
              mono
              placeholder="0oa1234567890"
              value={ssoClientId}
              onChange={(e) => setSsoClientId(e.target.value)}
            />
          </FormField>
          <Checkbox
            checked={ssoScim}
            label="Enable SCIM 2.0 user & group provisioning"
            onCheckedChange={setSsoScim}
          />
        </Stack>
      </Dialog>

      <AlertDialog
        open={deleteTeam !== null}
        onOpenChange={(o) => !o && setDeleteTeam(null)}
        title={`Delete team '${deleteTeam?.name ?? ""}'?`}
        description="Members will remain in the organization but lose roles inherited exclusively from this team."
        confirmLabel="Delete team"
        onConfirm={() => deleteTeam && deleteTeamMutation.mutate(deleteTeam.key)}
      />

      <AlertDialog
        open={deleteSso !== null}
        onOpenChange={(o) => !o && setDeleteSso(null)}
        title={`Remove SSO provider '${deleteSso?.name ?? ""}'?`}
        description="Users will no longer authenticate through this identity provider."
        confirmLabel="Remove provider"
        onConfirm={() => deleteSso && deleteSsoMutation.mutate(deleteSso.id)}
      />
    </Stack>
  );
}
