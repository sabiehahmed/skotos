import crypto from 'node:crypto';
import { Pool, type QueryResult, type QueryResultRow } from 'pg';
import { loadConfig } from './config';
import type {
  BuiltInRole,
  ClientSideAvailability,
  EnvironmentKeyKind,
  FlagKind,
  FlagPatchInstruction,
  Variation,
} from './types';

export const PHASE1_TABLES = Object.freeze([
  'organizations',
  'members',
  'projects',
  'environments',
  'environment_keys',
  'flags',
  'flag_env_configs',
  'flag_env_revisions',
  'segments',
  'audit_logs',
  'outbox_events',
  'api_tokens',
] as const);

export type Phase1TableName = (typeof PHASE1_TABLES)[number];

export interface DbClientOptions {
  readonly max?: number;
  readonly idleTimeoutMillis?: number;
  readonly connectionTimeoutMillis?: number;
}

export interface AuditLogPartitionInfo {
  partitionName: string;
  partitionBound: string;
  isDefault: boolean;
}

export interface AuditLogPartitionStatus {
  isPartitioned: boolean;
  strategy: string | null;
  partitions: AuditLogPartitionInfo[];
}

export interface DbEnvironmentRow {
  id: string;
  projectId: string;
  key: string;
  name: string;
  color: string;
  clientSideId: string;
  requireComments: boolean;
  confirmChanges: boolean;
  revision: number;
  createdAt: Date;
}

export interface DbFlagRow {
  id: string;
  projectId: string;
  key: string;
  name: string;
  description: string;
  kind: FlagKind;
  variationsJsonb: Variation[];
  temporary: boolean;
  clientSideAvailabilityJsonb: ClientSideAvailability;
  tags: string[];
  maintainerId: string | null;
  archived: boolean;
  createdAt: Date;
}

export interface DbFlagEnvConfigRow {
  flagId: string;
  envId: string;
  version: number;
  on: boolean;
  salt: string;
  configJsonb: Record<string, unknown>;
  updatedAt: Date;
}

export interface DbFlagEnvRevisionRow {
  id: string;
  flagId: string;
  envId: string;
  version: number;
  on: boolean;
  configJsonb: Record<string, unknown>;
  instructionsJsonb: FlagPatchInstruction[];
  actorId: string | null;
  comment: string | null;
  createdAt: Date;
}

export interface DbAuditLogRow {
  id: string;
  orgId: string;
  projectId: string | null;
  envId: string | null;
  resourceType: string;
  resourceKey: string;
  action: string | null;
  actorId: string | null;
  actorType: string | null;
  ip: string | null;
  comment: string | null;
  instructionsJsonb: unknown;
  beforeJsonb: unknown;
  afterJsonb: unknown;
  createdAt: Date;
}

export interface DbOutboxEventRow {
  id: number;
  orgId: string;
  envId: string | null;
  eventType: string;
  payloadJsonb: Record<string, unknown>;
  createdAt: Date;
  publishedAt: Date | null;
}

export interface DbEnvironmentKeyRow {
  id: string;
  envId: string;
  kind: EnvironmentKeyKind;
  keyPrefix: string;
  keyHash: string;
  expiresAt: Date | null;
  createdAt: Date;
}

export interface DbApiTokenRow {
  id: string;
  orgId: string;
  creatorMemberId: string | null;
  name: string;
  role: BuiltInRole;
  tokenPrefix: string;
  tokenHash: string;
  lastUsedAt: Date | null;
  expiresAt: Date | null;
  revokedAt: Date | null;
  createdAt: Date;
}

export interface DbMemberRow {
  id: string;
  orgId: string;
  email: string;
  displayName: string;
  role: BuiltInRole;
  passwordHash: string | null;
  createdAt: Date;
}

export interface MutationArtifactSnapshot {
  configVersion: number | null;
  envRevision: number | null;
  revisionCount: number;
  auditCount: number;
  outboxCount: number;
}

export function sha256Hex(plaintext: string): string {
  return crypto.createHash('sha256').update(plaintext, 'utf8').digest('hex');
}

function isValidPostgresUrl(url: string): boolean {
  const trimmed = url.trim();
  if (!/^postgres(ql)?:\/\//i.test(trimmed)) {
    return false;
  }
  try {
    new URL(trimmed);
    return true;
  } catch {
    return false;
  }
}

function toDate(value: unknown): Date {
  if (value instanceof Date) return value;
  return new Date(String(value));
}

function toNullableDate(value: unknown): Date | null {
  if (value === null || value === undefined) return null;
  return toDate(value);
}

function toNumber(value: unknown): number {
  if (typeof value === 'number') return value;
  return Number(value);
}

export class SkotosDbClient {
  private readonly connectionString: string;
  private readonly maxConnections: number;
  private readonly idleTimeoutMillis: number;
  private readonly connectionTimeoutMillis: number;
  private pool: Pool | null = null;

  constructor(connectionString?: string, options: DbClientOptions = {}) {
    this.connectionString =
      connectionString !== undefined ? connectionString.trim() : loadConfig().databaseUrl;
    const requestedMax = options.max ?? 3;
    this.maxConnections = Math.max(1, Math.min(4, Math.trunc(requestedMax)));
    this.idleTimeoutMillis = options.idleTimeoutMillis ?? 10_000;
    this.connectionTimeoutMillis = options.connectionTimeoutMillis ?? 5_000;
  }

  public isConfigured(): boolean {
    return isValidPostgresUrl(this.connectionString);
  }

  private getPool(): Pool {
    if (!this.isConfigured()) {
      throw new Error(
        `Database client is not configured with a valid postgres:// or postgresql:// connection string (got "${this.connectionString}")`,
      );
    }
    if (!this.pool) {
      this.pool = new Pool({
        connectionString: this.connectionString,
        max: this.maxConnections,
        idleTimeoutMillis: this.idleTimeoutMillis,
        connectionTimeoutMillis: this.connectionTimeoutMillis,
      });
    }
    return this.pool;
  }

  public async query<T extends QueryResultRow = Record<string, unknown>>(
    sql: string,
    params: readonly unknown[] = [],
  ): Promise<QueryResult<T>> {
    const pool = this.getPool();
    return pool.query<T>(sql, [...params]);
  }

  public async ping(): Promise<boolean> {
    const result = await this.query<{ ok: number }>('SELECT 1 AS ok');
    return result.rows[0]?.ok === 1;
  }

  public async close(): Promise<void> {
    if (this.pool) {
      const current = this.pool;
      this.pool = null;
      await current.end();
    }
  }

  // ---- Schema & Partition Verification ----

  public async listTables(): Promise<string[]> {
    const result = await this.query<{ table_name: string }>(
      `SELECT c.relname AS table_name
       FROM pg_class c
       JOIN pg_namespace n ON n.oid = c.relnamespace
       WHERE n.nspname = current_schema()
         AND c.relkind IN ('r', 'p')
       ORDER BY c.relname ASC`,
    );
    return result.rows.map((row) => row.table_name);
  }

  public async getAuditLogPartitionStatus(): Promise<AuditLogPartitionStatus> {
    const parentResult = await this.query<{ relkind: string; partstrat: string | null }>(
      `SELECT c.relkind::text AS relkind, pt.partstrat::text AS partstrat
       FROM pg_class c
       JOIN pg_namespace n ON n.oid = c.relnamespace
       LEFT JOIN pg_partitioned_table pt ON pt.partrelid = c.oid
       WHERE n.nspname = current_schema()
         AND c.relname = 'audit_logs'`,
    );

    const parent = parentResult.rows[0];
    if (!parent) {
      return { isPartitioned: false, strategy: null, partitions: [] };
    }

    const childResult = await this.query<{ partition_name: string; partition_bound: string }>(
      `SELECT
         c.relname AS partition_name,
         pg_get_expr(c.relpartbound, c.oid) AS partition_bound
       FROM pg_inherits i
       JOIN pg_class p ON p.oid = i.inhparent
       JOIN pg_class c ON c.oid = i.inhrelid
       JOIN pg_namespace n ON n.oid = p.relnamespace
       WHERE n.nspname = current_schema()
         AND p.relname = 'audit_logs'
       ORDER BY c.relname ASC`,
    );

    const partitions: AuditLogPartitionInfo[] = childResult.rows.map((row) => ({
      partitionName: row.partition_name,
      partitionBound: row.partition_bound,
      isDefault: row.partition_bound.trim().toUpperCase() === 'DEFAULT',
    }));

    return {
      isPartitioned: parent.relkind === 'p',
      strategy: parent.partstrat ?? null,
      partitions,
    };
  }

  // ---- Domain Verification Helpers ----

  public async getEnvironment(
    projectKey: string,
    envKey: string,
  ): Promise<DbEnvironmentRow | null> {
    const result = await this.query<Record<string, unknown>>(
      `SELECT
         e.id,
         e.project_id,
         e.key,
         e.name,
         e.color,
         e.client_side_id,
         e.require_comments,
         e.confirm_changes,
         e.revision,
         e.created_at
       FROM environments e
       JOIN projects p ON p.id = e.project_id
       WHERE p.key = $1 AND e.key = $2
       LIMIT 1`,
      [projectKey, envKey],
    );
    const row = result.rows[0];
    if (!row) return null;
    return {
      id: String(row.id),
      projectId: String(row.project_id),
      key: String(row.key),
      name: String(row.name),
      color: String(row.color),
      clientSideId: String(row.client_side_id),
      requireComments: Boolean(row.require_comments),
      confirmChanges: Boolean(row.confirm_changes),
      revision: toNumber(row.revision),
      createdAt: toDate(row.created_at),
    };
  }

  public async getFlag(projectKey: string, flagKey: string): Promise<DbFlagRow | null> {
    const result = await this.query<Record<string, unknown>>(
      `SELECT
         f.id,
         f.project_id,
         f.key,
         f.name,
         f.description,
         f.kind,
         f.variations_jsonb,
         f.temporary,
         f.client_side_availability_jsonb,
         f.tags,
         f.maintainer_id,
         f.archived,
         f.created_at
       FROM flags f
       JOIN projects p ON p.id = f.project_id
       WHERE p.key = $1 AND f.key = $2
       LIMIT 1`,
      [projectKey, flagKey],
    );
    const row = result.rows[0];
    if (!row) return null;
    return {
      id: String(row.id),
      projectId: String(row.project_id),
      key: String(row.key),
      name: String(row.name),
      description: String(row.description ?? ''),
      kind: row.kind as FlagKind,
      variationsJsonb: (row.variations_jsonb ?? []) as Variation[],
      temporary: Boolean(row.temporary),
      clientSideAvailabilityJsonb: row.client_side_availability_jsonb as ClientSideAvailability,
      tags: Array.isArray(row.tags) ? (row.tags as string[]) : [],
      maintainerId: row.maintainer_id ? String(row.maintainer_id) : null,
      archived: Boolean(row.archived),
      createdAt: toDate(row.created_at),
    };
  }

  public async getFlagEnvConfig(
    projectKey: string,
    flagKey: string,
    envKey: string,
  ): Promise<DbFlagEnvConfigRow | null> {
    const result = await this.query<Record<string, unknown>>(
      `SELECT
         fec.flag_id,
         fec.env_id,
         fec.version,
         fec."on" AS is_on,
         fec.salt,
         fec.config_jsonb,
         fec.updated_at
       FROM flag_env_configs fec
       JOIN flags f ON f.id = fec.flag_id
       JOIN environments e ON e.id = fec.env_id
       JOIN projects p ON p.id = f.project_id AND p.id = e.project_id
       WHERE p.key = $1 AND f.key = $2 AND e.key = $3
       LIMIT 1`,
      [projectKey, flagKey, envKey],
    );
    const row = result.rows[0];
    if (!row) return null;
    return {
      flagId: String(row.flag_id),
      envId: String(row.env_id),
      version: toNumber(row.version),
      on: Boolean(row.is_on),
      salt: String(row.salt),
      configJsonb: (row.config_jsonb ?? {}) as Record<string, unknown>,
      updatedAt: toDate(row.updated_at),
    };
  }

  public async getFlagEnvRevisions(
    projectKey: string,
    flagKey: string,
    envKey: string,
  ): Promise<DbFlagEnvRevisionRow[]> {
    const result = await this.query<Record<string, unknown>>(
      `SELECT
         r.id,
         r.flag_id,
         r.env_id,
         r.version,
         r."on" AS is_on,
         r.config_jsonb,
         r.instructions_jsonb,
         r.actor_id,
         r.comment,
         r.created_at
       FROM flag_env_revisions r
       JOIN flags f ON f.id = r.flag_id
       JOIN environments e ON e.id = r.env_id
       JOIN projects p ON p.id = f.project_id AND p.id = e.project_id
       WHERE p.key = $1 AND f.key = $2 AND e.key = $3
       ORDER BY r.version DESC`,
      [projectKey, flagKey, envKey],
    );
    return result.rows.map((row) => ({
      id: String(row.id),
      flagId: String(row.flag_id),
      envId: String(row.env_id),
      version: toNumber(row.version),
      on: Boolean(row.is_on),
      configJsonb: (row.config_jsonb ?? {}) as Record<string, unknown>,
      instructionsJsonb: (row.instructions_jsonb ?? []) as FlagPatchInstruction[],
      actorId: row.actor_id ? String(row.actor_id) : null,
      comment: row.comment !== null && row.comment !== undefined ? String(row.comment) : null,
      createdAt: toDate(row.created_at),
    }));
  }

  public async getAuditLogs(
    filter: {
      resourceKey?: string;
      resourceType?: string;
      envId?: string;
      limit?: number;
    } = {},
  ): Promise<DbAuditLogRow[]> {
    const conditions: string[] = [];
    const params: unknown[] = [];

    if (filter.resourceKey !== undefined) {
      params.push(filter.resourceKey);
      conditions.push(`a.resource_key = $${params.length}`);
    }
    if (filter.resourceType !== undefined) {
      params.push(filter.resourceType);
      conditions.push(`a.resource_type = $${params.length}`);
    }
    if (filter.envId !== undefined) {
      params.push(filter.envId);
      conditions.push(`a.env_id = $${params.length}`);
    }

    const limit = filter.limit ?? 50;
    params.push(limit);
    const whereClause = conditions.length > 0 ? `WHERE ${conditions.join(' AND ')}` : '';

    const result = await this.query<{ row: Record<string, unknown> }>(
      `SELECT to_jsonb(a) AS row
       FROM audit_logs a
       ${whereClause}
       ORDER BY a.created_at DESC, a.id DESC
       LIMIT $${params.length}`,
      params,
    );

    return result.rows.map(({ row }) => ({
      id: String(row.id),
      orgId: String(row.org_id),
      projectId: row.project_id ? String(row.project_id) : null,
      envId: row.env_id ? String(row.env_id) : null,
      resourceType: String(row.resource_type),
      resourceKey: String(row.resource_key),
      action: row.action !== undefined && row.action !== null ? String(row.action) : null,
      actorId: row.actor_id ? String(row.actor_id) : null,
      actorType:
        row.actor_type !== undefined && row.actor_type !== null ? String(row.actor_type) : null,
      ip: row.ip !== undefined && row.ip !== null ? String(row.ip) : null,
      comment: row.comment !== undefined && row.comment !== null ? String(row.comment) : null,
      instructionsJsonb: row.instructions_jsonb ?? null,
      beforeJsonb: row.before_jsonb ?? null,
      afterJsonb: row.after_jsonb ?? null,
      createdAt: toDate(row.created_at),
    }));
  }

  public async getOutboxEvents(
    filter: {
      envId?: string;
      eventType?: string;
      sinceId?: number;
      limit?: number;
    } = {},
  ): Promise<DbOutboxEventRow[]> {
    const conditions: string[] = [];
    const params: unknown[] = [];

    if (filter.envId !== undefined) {
      params.push(filter.envId);
      conditions.push(`o.env_id = $${params.length}`);
    }
    if (filter.eventType !== undefined) {
      params.push(filter.eventType);
      conditions.push(`o.event_type = $${params.length}`);
    }
    if (filter.sinceId !== undefined) {
      params.push(filter.sinceId);
      conditions.push(`o.id > $${params.length}`);
    }

    const limit = filter.limit ?? 100;
    params.push(limit);
    const whereClause = conditions.length > 0 ? `WHERE ${conditions.join(' AND ')}` : '';

    const result = await this.query<Record<string, unknown>>(
      `SELECT
         o.id,
         o.org_id,
         o.env_id,
         o.event_type,
         o.payload_jsonb,
         o.created_at,
         o.published_at
       FROM outbox_events o
       ${whereClause}
       ORDER BY o.id ASC
       LIMIT $${params.length}`,
      params,
    );

    return result.rows.map((row) => ({
      id: toNumber(row.id),
      orgId: String(row.org_id),
      envId: row.env_id ? String(row.env_id) : null,
      eventType: String(row.event_type),
      payloadJsonb: (row.payload_jsonb ?? {}) as Record<string, unknown>,
      createdAt: toDate(row.created_at),
      publishedAt: toNullableDate(row.published_at),
    }));
  }

  public async getOutboxEventsForEnv(
    projectKey: string,
    envKey: string,
    options: { sinceId?: number; limit?: number } = {},
  ): Promise<DbOutboxEventRow[]> {
    const env = await this.getEnvironment(projectKey, envKey);
    if (!env) return [];
    return this.getOutboxEvents({
      envId: env.id,
      sinceId: options.sinceId,
      limit: options.limit,
    });
  }

  public async getEnvironmentKeys(
    projectKey: string,
    envKey: string,
    kind?: EnvironmentKeyKind,
  ): Promise<DbEnvironmentKeyRow[]> {
    const params: unknown[] = [projectKey, envKey];
    let kindClause = '';
    if (kind !== undefined) {
      params.push(kind);
      kindClause = `AND ek.kind = $${params.length}`;
    }

    const result = await this.query<Record<string, unknown>>(
      `SELECT
         ek.id,
         ek.env_id,
         ek.kind,
         ek.key_prefix,
         ek.key_hash,
         ek.expires_at,
         ek.created_at
       FROM environment_keys ek
       JOIN environments e ON e.id = ek.env_id
       JOIN projects p ON p.id = e.project_id
       WHERE p.key = $1 AND e.key = $2 ${kindClause}
       ORDER BY ek.created_at DESC`,
      params,
    );

    return result.rows.map((row) => ({
      id: String(row.id),
      envId: String(row.env_id),
      kind: row.kind as EnvironmentKeyKind,
      keyPrefix: String(row.key_prefix),
      keyHash: String(row.key_hash),
      expiresAt: toNullableDate(row.expires_at),
      createdAt: toDate(row.created_at),
    }));
  }

  public async getApiToken(tokenId: string): Promise<DbApiTokenRow | null> {
    const result = await this.query<Record<string, unknown>>(
      `SELECT
         id,
         org_id,
         creator_member_id,
         name,
         role,
         token_prefix,
         token_hash,
         last_used_at,
         expires_at,
         revoked_at,
         created_at
       FROM api_tokens
       WHERE id = $1
       LIMIT 1`,
      [tokenId],
    );
    const row = result.rows[0];
    if (!row) return null;
    return {
      id: String(row.id),
      orgId: String(row.org_id),
      creatorMemberId: row.creator_member_id ? String(row.creator_member_id) : null,
      name: String(row.name),
      role: row.role as BuiltInRole,
      tokenPrefix: String(row.token_prefix),
      tokenHash: String(row.token_hash),
      lastUsedAt: toNullableDate(row.last_used_at),
      expiresAt: toNullableDate(row.expires_at),
      revokedAt: toNullableDate(row.revoked_at),
      createdAt: toDate(row.created_at),
    };
  }

  public async getMemberByEmail(email: string): Promise<DbMemberRow | null> {
    const result = await this.query<Record<string, unknown>>(
      `SELECT
         id,
         org_id,
         email,
         display_name,
         role,
         password_hash,
         created_at
       FROM members
       WHERE lower(email) = lower($1)
       LIMIT 1`,
      [email],
    );
    const row = result.rows[0];
    if (!row) return null;
    return {
      id: String(row.id),
      orgId: String(row.org_id),
      email: String(row.email),
      displayName: String(row.display_name),
      role: row.role as BuiltInRole,
      passwordHash:
        row.password_hash !== null && row.password_hash !== undefined
          ? String(row.password_hash)
          : null,
      createdAt: toDate(row.created_at),
    };
  }

  public async getMutationArtifactSnapshot(
    projectKey: string,
    flagKey: string,
    envKey: string,
  ): Promise<MutationArtifactSnapshot> {
    const env = await this.getEnvironment(projectKey, envKey);
    const config = await this.getFlagEnvConfig(projectKey, flagKey, envKey);

    if (!env) {
      return {
        configVersion: config?.version ?? null,
        envRevision: null,
        revisionCount: 0,
        auditCount: 0,
        outboxCount: 0,
      };
    }

    const [revRes, auditRes, outboxRes] = await Promise.all([
      config
        ? this.query<{ cnt: string }>(
            `SELECT COUNT(*)::text AS cnt
             FROM flag_env_revisions
             WHERE flag_id = $1 AND env_id = $2`,
            [config.flagId, env.id],
          )
        : Promise.resolve({ rows: [{ cnt: '0' }] }),
      this.query<{ cnt: string }>(
        `SELECT COUNT(*)::text AS cnt
         FROM audit_logs
         WHERE env_id = $1 AND resource_key = $2`,
        [env.id, flagKey],
      ),
      this.query<{ cnt: string }>(
        `SELECT COUNT(*)::text AS cnt
         FROM outbox_events
         WHERE env_id = $1`,
        [env.id],
      ),
    ]);

    return {
      configVersion: config?.version ?? null,
      envRevision: env.revision,
      revisionCount: toNumber(revRes.rows[0]?.cnt ?? 0),
      auditCount: toNumber(auditRes.rows[0]?.cnt ?? 0),
      outboxCount: toNumber(outboxRes.rows[0]?.cnt ?? 0),
    };
  }
}

export { SkotosDbClient as SkotosDb };

export function createDb(
  connectionString?: string,
  options?: DbClientOptions,
): SkotosDbClient {
  const candidate =
    connectionString !== undefined ? connectionString.trim() : loadConfig().databaseUrl;
  if (!isValidPostgresUrl(candidate)) {
    throw new Error(
      `Invalid PostgreSQL connection string "${connectionString ?? ''}": must start with postgres:// or postgresql://`,
    );
  }
  return new SkotosDbClient(candidate, options);
}
