-- Phase 3 Enterprise Governance, Release Automation, Big Segments, SSO/SCIM & Code References

-- 1. Environment Mandatory Approval Settings
ALTER TABLE environments
    ADD COLUMN IF NOT EXISTS approval_required BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS min_approvals INTEGER NOT NULL DEFAULT 1,
    ADD COLUMN IF NOT EXISTS allow_self_approval BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS external_approval_webhook_url TEXT;

-- 2. Webhook First-Class Integration Kind (generic | slack | teams | datadog | pagerduty | jira | linear)
ALTER TABLE webhooks
    ADD COLUMN IF NOT EXISTS integration_kind TEXT NOT NULL DEFAULT 'generic';

-- 3. Custom RBAC Roles & Member Role Assignments
CREATE TABLE IF NOT EXISTS custom_roles (
    id          UUID PRIMARY KEY,
    org_id      UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    statements  JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, key)
);

CREATE TABLE IF NOT EXISTS member_custom_roles (
    org_id     UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    member_id  UUID NOT NULL REFERENCES members(id) ON DELETE CASCADE,
    role_id    UUID NOT NULL REFERENCES custom_roles(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (member_id, role_id)
);

-- 4. Teams & Team Membership
CREATE TABLE IF NOT EXISTS teams (
    id               UUID PRIMARY KEY,
    org_id           UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    key              TEXT NOT NULL,
    name             TEXT NOT NULL,
    description      TEXT NOT NULL DEFAULT '',
    custom_role_keys TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, key)
);

CREATE TABLE IF NOT EXISTS team_members (
    org_id     UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    team_id    UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    member_id  UUID NOT NULL REFERENCES members(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (team_id, member_id)
);
CREATE INDEX IF NOT EXISTS idx_team_members_member ON team_members (org_id, member_id);

-- 5. Mandatory Approval Workflows
CREATE TABLE IF NOT EXISTS approval_requests (
    id                 UUID PRIMARY KEY,
    org_id             UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    project_key        TEXT NOT NULL,
    env_key            TEXT NOT NULL,
    flag_key           TEXT NOT NULL,
    flag_id            UUID NOT NULL REFERENCES flags(id) ON DELETE CASCADE,
    env_id             UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    requested_by       UUID REFERENCES members(id) ON DELETE SET NULL,
    requested_by_email TEXT,
    description        TEXT NOT NULL DEFAULT '',
    instructions       JSONB NOT NULL,
    comment            TEXT,
    status             TEXT NOT NULL DEFAULT 'pending',
    min_approvals      INTEGER NOT NULL DEFAULT 1,
    applied_version    BIGINT,
    applied_at         TIMESTAMPTZ,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_approval_requests_org_status
    ON approval_requests (org_id, status, created_at DESC);

CREATE TABLE IF NOT EXISTS approval_reviews (
    id             UUID PRIMARY KEY,
    org_id         UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    approval_id    UUID NOT NULL REFERENCES approval_requests(id) ON DELETE CASCADE,
    reviewer_id    UUID NOT NULL REFERENCES members(id) ON DELETE CASCADE,
    reviewer_email TEXT,
    kind           TEXT NOT NULL,
    comment        TEXT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (approval_id, reviewer_id)
);

-- 6. Enterprise SSO / OIDC / SAML Providers & Audit Log Archives
CREATE TABLE IF NOT EXISTS sso_providers (
    id                UUID PRIMARY KEY,
    org_id            UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    protocol          TEXT NOT NULL,
    name              TEXT NOT NULL,
    issuer_url        TEXT NOT NULL,
    client_id         TEXT,
    jwks_url          TEXT,
    saml_metadata_url TEXT,
    enabled           BOOLEAN NOT NULL DEFAULT TRUE,
    scim_enabled      BOOLEAN NOT NULL DEFAULT FALSE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS audit_archives (
    id               UUID PRIMARY KEY,
    org_id           UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    object_key       TEXT NOT NULL,
    format           TEXT NOT NULL DEFAULT 'ndjson.zst',
    record_count     BIGINT NOT NULL,
    compressed_bytes BIGINT NOT NULL,
    sha256           TEXT NOT NULL,
    from_timestamp   TIMESTAMPTZ NOT NULL,
    to_timestamp     TIMESTAMPTZ NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 7. Multi-Step Flag Workflows
CREATE TABLE IF NOT EXISTS flag_workflows (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    project_key         TEXT NOT NULL,
    env_key             TEXT NOT NULL,
    flag_key            TEXT NOT NULL,
    flag_id             UUID NOT NULL REFERENCES flags(id) ON DELETE CASCADE,
    env_id              UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    name                TEXT NOT NULL,
    description         TEXT NOT NULL DEFAULT '',
    status              TEXT NOT NULL DEFAULT 'active',
    current_stage_index INTEGER NOT NULL DEFAULT 0,
    next_run_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    stages              JSONB NOT NULL,
    created_by          UUID REFERENCES members(id) ON DELETE SET NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_flag_workflows_due
    ON flag_workflows (status, next_run_at) WHERE status = 'active';

-- 8. Release Pipelines & Flag Releases
CREATE TABLE IF NOT EXISTS release_pipelines (
    id          UUID PRIMARY KEY,
    org_id      UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    phases      JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, key)
);

CREATE TABLE IF NOT EXISTS flag_releases (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    project_key         TEXT NOT NULL,
    flag_key            TEXT NOT NULL,
    flag_id             UUID NOT NULL REFERENCES flags(id) ON DELETE CASCADE,
    pipeline_id         UUID NOT NULL REFERENCES release_pipelines(id) ON DELETE CASCADE,
    pipeline_key        TEXT NOT NULL,
    pipeline_name       TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'active',
    current_phase_index INTEGER NOT NULL DEFAULT 0,
    phases              JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (flag_id)
);

-- 9. Applications & App Versions
CREATE TABLE IF NOT EXISTS applications (
    id                 UUID PRIMARY KEY,
    org_id             UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    key                TEXT NOT NULL,
    name               TEXT NOT NULL,
    kind               TEXT NOT NULL DEFAULT 'server',
    description        TEXT NOT NULL DEFAULT '',
    maintainer_team_id UUID REFERENCES teams(id) ON DELETE SET NULL,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, key)
);

CREATE TABLE IF NOT EXISTS application_versions (
    id          UUID PRIMARY KEY,
    org_id      UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    app_id      UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    version_key TEXT NOT NULL,
    name        TEXT NOT NULL,
    supported   BOOLEAN NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (app_id, version_key)
);

-- 10. High-Scale Big Segments Membership & Scheduled Segment Changes
CREATE TABLE IF NOT EXISTS big_segment_members (
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    segment_id   UUID NOT NULL REFERENCES segments(id) ON DELETE CASCADE,
    env_id       UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    context_kind TEXT NOT NULL DEFAULT 'user',
    context_key  TEXT NOT NULL,
    key_hash     BIGINT NOT NULL,
    included     BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (segment_id, context_kind, context_key)
);
CREATE INDEX IF NOT EXISTS idx_big_segment_members_lookup
    ON big_segment_members (segment_id, included);

CREATE TABLE IF NOT EXISTS scheduled_segment_changes (
    id            UUID PRIMARY KEY,
    org_id        UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    segment_id    UUID NOT NULL REFERENCES segments(id) ON DELETE CASCADE,
    env_id        UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    project_key   TEXT NOT NULL,
    env_key       TEXT NOT NULL,
    segment_key   TEXT NOT NULL,
    execute_at    TIMESTAMPTZ NOT NULL,
    instructions  JSONB NOT NULL,
    comment       TEXT,
    created_by    UUID REFERENCES members(id) ON DELETE SET NULL,
    status        TEXT NOT NULL DEFAULT 'pending',
    error_message TEXT,
    executed_at   TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_scheduled_segment_changes_due
    ON scheduled_segment_changes (status, execute_at) WHERE status = 'pending';

-- 11. Code References Tracking (skotos-code-refs CI scanner + Extinction checks)
CREATE TABLE IF NOT EXISTS flag_code_refs (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    flag_key        TEXT NOT NULL,
    repo_name       TEXT NOT NULL,
    branch          TEXT NOT NULL,
    commit_sha      TEXT NOT NULL DEFAULT '',
    hunks           JSONB NOT NULL DEFAULT '[]'::jsonb,
    reference_count BIGINT NOT NULL DEFAULT 0,
    extinct         BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, flag_key, repo_name, branch)
);
CREATE INDEX IF NOT EXISTS idx_flag_code_refs_lookup
    ON flag_code_refs (org_id, project_id, flag_key);
