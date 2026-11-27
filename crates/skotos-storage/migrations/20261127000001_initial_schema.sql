-- Skotos Phase 1 Initial Schema (12 Core Tables)
-- PostgreSQL 16+: Hybrid relational hierarchy + versioned JSONB configs + transactional outbox + partitioned audit_logs

CREATE TABLE IF NOT EXISTS organizations (
    id UUID PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    external_auth_org_id TEXT UNIQUE,
    plan_tier TEXT NOT NULL DEFAULT 'free' CHECK (plan_tier IN ('free', 'pro', 'team', 'enterprise')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS members (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    external_subject_id TEXT,
    email TEXT NOT NULL,
    display_name TEXT NOT NULL,
    password_hash TEXT,
    role TEXT NOT NULL CHECK (role IN ('reader', 'writer', 'admin', 'owner')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, email)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_members_org_external_sub
    ON members (org_id, external_subject_id)
    WHERE external_subject_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_members_email ON members (lower(email));

CREATE TABLE IF NOT EXISTS projects (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    name TEXT NOT NULL,
    default_client_side_availability JSONB NOT NULL DEFAULT '{"usingEnvironmentId":false,"usingMobileKey":false}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, key)
);

CREATE TABLE IF NOT EXISTS environments (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    name TEXT NOT NULL,
    color TEXT NOT NULL DEFAULT '#3B82F6',
    client_side_id TEXT NOT NULL UNIQUE,
    require_comments BOOLEAN NOT NULL DEFAULT false,
    confirm_changes BOOLEAN NOT NULL DEFAULT false,
    revision BIGINT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, key)
);

CREATE INDEX IF NOT EXISTS idx_environments_org_project ON environments (org_id, project_id);

CREATE TABLE IF NOT EXISTS environment_keys (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    env_id UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('sdk', 'mobile')),
    key_prefix TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_environment_keys_env_kind ON environment_keys (org_id, env_id, kind);

CREATE TABLE IF NOT EXISTS flags (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL CHECK (kind IN ('boolean', 'string', 'number', 'json')),
    variations_jsonb JSONB NOT NULL,
    temporary BOOLEAN NOT NULL DEFAULT true,
    client_side_availability_jsonb JSONB NOT NULL DEFAULT '{"usingEnvironmentId":false,"usingMobileKey":false}'::jsonb,
    tags TEXT[] NOT NULL DEFAULT '{}',
    maintainer_id UUID REFERENCES members(id) ON DELETE SET NULL,
    archived BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, key)
);

CREATE INDEX IF NOT EXISTS idx_flags_org_project ON flags (org_id, project_id);
CREATE INDEX IF NOT EXISTS idx_flags_tags_gin ON flags USING GIN (tags);

CREATE TABLE IF NOT EXISTS flag_env_configs (
    flag_id UUID NOT NULL REFERENCES flags(id) ON DELETE CASCADE,
    env_id UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    version BIGINT NOT NULL DEFAULT 1,
    "on" BOOLEAN NOT NULL DEFAULT false,
    salt TEXT NOT NULL,
    config_jsonb JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (flag_id, env_id)
);

CREATE INDEX IF NOT EXISTS idx_flag_env_configs_org_env ON flag_env_configs (org_id, env_id);

CREATE TABLE IF NOT EXISTS flag_env_revisions (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    flag_id UUID NOT NULL REFERENCES flags(id) ON DELETE CASCADE,
    env_id UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    version BIGINT NOT NULL,
    "on" BOOLEAN NOT NULL,
    config_jsonb JSONB NOT NULL,
    instructions_jsonb JSONB NOT NULL DEFAULT '[]'::jsonb,
    actor_id UUID,
    comment TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (flag_id, env_id, version)
);

CREATE INDEX IF NOT EXISTS idx_flag_env_revisions_lookup
    ON flag_env_revisions (org_id, flag_id, env_id, version DESC);

CREATE TABLE IF NOT EXISTS segments (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    env_id UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'standard' CHECK (kind IN ('standard', 'big', 'synced')),
    version BIGINT NOT NULL DEFAULT 1,
    salt TEXT NOT NULL,
    segment_jsonb JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (env_id, key)
);

CREATE INDEX IF NOT EXISTS idx_segments_org_env ON segments (org_id, env_id);

-- Monthly range-partitioned immutable audit log with catch-all default partition
CREATE TABLE IF NOT EXISTS audit_logs (
    id UUID NOT NULL,
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    project_id UUID,
    env_id UUID,
    resource_type TEXT NOT NULL,
    resource_key TEXT NOT NULL,
    action TEXT NOT NULL,
    actor_id UUID,
    actor_type TEXT NOT NULL DEFAULT 'member',
    actor_email TEXT,
    comment TEXT,
    instructions_jsonb JSONB,
    before_jsonb JSONB,
    after_jsonb JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (id, created_at)
) PARTITION BY RANGE (created_at);

CREATE TABLE IF NOT EXISTS audit_logs_2026_11 PARTITION OF audit_logs
    FOR VALUES FROM ('2026-11-01 00:00:00+00') TO ('2026-12-01 00:00:00+00');

CREATE TABLE IF NOT EXISTS audit_logs_2026_12 PARTITION OF audit_logs
    FOR VALUES FROM ('2026-12-01 00:00:00+00') TO ('2027-01-01 00:00:00+00');

CREATE TABLE IF NOT EXISTS audit_logs_default PARTITION OF audit_logs DEFAULT;

CREATE INDEX IF NOT EXISTS idx_audit_logs_org_created
    ON audit_logs (org_id, created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_org_resource
    ON audit_logs (org_id, resource_type, resource_key, created_at DESC);

CREATE TABLE IF NOT EXISTS outbox_events (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    env_id UUID,
    event_type TEXT NOT NULL,
    payload_jsonb JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    published_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_outbox_events_unpublished
    ON outbox_events (created_at ASC)
    WHERE published_at IS NULL;

CREATE TABLE IF NOT EXISTS api_tokens (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    key_prefix TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    role TEXT NOT NULL CHECK (role IN ('reader', 'writer', 'admin', 'owner')),
    created_by UUID REFERENCES members(id) ON DELETE SET NULL,
    last_used_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_api_tokens_org ON api_tokens (org_id, created_at DESC);
