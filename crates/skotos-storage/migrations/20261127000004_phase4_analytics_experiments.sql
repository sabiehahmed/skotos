-- Phase 4: ClickHouse Analytics Fallback, Real-Time Data Export Sinks,
-- Experimentation Core (Metrics, Funnels, Mutual Exclusion Layers, Experiments),
-- Guarded Rollouts, 2/4/6-Stage Migration Flags, and Synced Segments.

-- 1. Real-Time Data Export Sinks
CREATE TABLE IF NOT EXISTS data_export_sinks (
    id                   UUID PRIMARY KEY,
    org_id               UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    env_id               UUID REFERENCES environments(id) ON DELETE CASCADE,
    name                 TEXT NOT NULL,
    kind                 TEXT NOT NULL,
    destination          TEXT NOT NULL,
    enabled              BOOLEAN NOT NULL DEFAULT TRUE,
    config_jsonb         JSONB NOT NULL DEFAULT '{}'::jsonb,
    last_exported_at     TIMESTAMPTZ,
    exported_event_count BIGINT NOT NULL DEFAULT 0,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_data_export_sinks_org ON data_export_sinks (org_id);

-- 2. Metric Definitions
CREATE TABLE IF NOT EXISTS metrics (
    id                 UUID PRIMARY KEY,
    org_id             UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    project_id         UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key                TEXT NOT NULL,
    name               TEXT NOT NULL,
    description        TEXT NOT NULL DEFAULT '',
    kind               TEXT NOT NULL,
    event_key          TEXT NOT NULL,
    success_direction  TEXT NOT NULL,
    aggregation        TEXT NOT NULL DEFAULT 'average',
    unit               TEXT NOT NULL DEFAULT '',
    randomization_unit TEXT NOT NULL DEFAULT 'user',
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, key)
);

-- 3. Metric Groups (Funnels & Collections)
CREATE TABLE IF NOT EXISTS metric_groups (
    id               UUID PRIMARY KEY,
    org_id           UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    project_id       UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key              TEXT NOT NULL,
    name             TEXT NOT NULL,
    kind             TEXT NOT NULL,
    description      TEXT NOT NULL DEFAULT '',
    metric_keys_json JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, key)
);

-- 4. Mutual Exclusion Layers
CREATE TABLE IF NOT EXISTS mutual_exclusion_layers (
    id                UUID PRIMARY KEY,
    org_id            UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    project_id        UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key               TEXT NOT NULL,
    name              TEXT NOT NULL,
    description       TEXT NOT NULL DEFAULT '',
    salt              TEXT NOT NULL,
    total_buckets     INTEGER NOT NULL DEFAULT 100000,
    allocated_buckets INTEGER NOT NULL DEFAULT 0,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, key)
);

-- 5. Full-Stack Experiments (A/B/n & A/A, Frequentist & Bayesian)
CREATE TABLE IF NOT EXISTS experiments (
    id                        UUID PRIMARY KEY,
    org_id                    UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    project_id                UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    env_id                    UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    key                       TEXT NOT NULL,
    name                      TEXT NOT NULL,
    hypothesis                TEXT NOT NULL DEFAULT '',
    flag_key                  TEXT NOT NULL,
    mode                      TEXT NOT NULL DEFAULT 'ab_test',
    engine                    TEXT NOT NULL DEFAULT 'frequentist',
    status                    TEXT NOT NULL DEFAULT 'draft',
    randomization_unit        TEXT NOT NULL DEFAULT 'user',
    control_variation_index   INTEGER NOT NULL DEFAULT 0,
    primary_metric_key        TEXT NOT NULL,
    secondary_metric_keys     JSONB NOT NULL DEFAULT '[]'::jsonb,
    metric_group_key          TEXT,
    layer_key                 TEXT,
    layer_bucket_start        INTEGER,
    layer_bucket_end          INTEGER,
    allocations_jsonb         JSONB NOT NULL DEFAULT '[]'::jsonb,
    alpha                     DOUBLE PRECISION NOT NULL DEFAULT 0.05,
    started_at                TIMESTAMPTZ,
    stopped_at                TIMESTAMPTZ,
    created_at                TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (env_id, key)
);
CREATE INDEX IF NOT EXISTS idx_experiments_org_env ON experiments (org_id, env_id);

-- 6. Guarded Rollouts (Release Guardian & Auto-Rollback)
CREATE TABLE IF NOT EXISTS guarded_rollouts (
    id                        UUID PRIMARY KEY,
    org_id                    UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    project_id                UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    env_id                    UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    flag_key                  TEXT NOT NULL,
    name                      TEXT NOT NULL,
    status                    TEXT NOT NULL DEFAULT 'monitoring',
    control_variation_index   INTEGER NOT NULL DEFAULT 1,
    treatment_variation_index INTEGER NOT NULL DEFAULT 0,
    current_traffic_pct       INTEGER NOT NULL DEFAULT 10,
    auto_rollback             BOOLEAN NOT NULL DEFAULT TRUE,
    guardrails_jsonb          JSONB NOT NULL DEFAULT '[]'::jsonb,
    last_evaluated_at         TIMESTAMPTZ,
    last_verdict              TEXT,
    created_at                TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_guarded_rollouts_status ON guarded_rollouts (org_id, status);

-- 7. 2/4/6-Stage Migration Flags
CREATE TABLE IF NOT EXISTS flag_migration_configs (
    id                   UUID PRIMARY KEY,
    org_id               UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    env_id               UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    flag_key             TEXT NOT NULL,
    template             TEXT NOT NULL DEFAULT 'six_stage',
    current_stage        TEXT NOT NULL DEFAULT 'off',
    check_ratio          INTEGER NOT NULL DEFAULT 1,
    read_execution_order TEXT NOT NULL DEFAULT 'parallel',
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (env_id, flag_key)
);

-- 8. Synced Segments (External CDP & Reverse-ETL Integrations)
CREATE TABLE IF NOT EXISTS synced_segment_integrations (
    id                  UUID PRIMARY KEY,
    org_id              UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    env_id              UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    segment_key         TEXT NOT NULL,
    provider            TEXT NOT NULL,
    external_cohort_id  TEXT NOT NULL,
    context_kind        TEXT NOT NULL DEFAULT 'user',
    token_prefix        TEXT NOT NULL,
    token_hash          TEXT NOT NULL UNIQUE,
    enabled             BOOLEAN NOT NULL DEFAULT TRUE,
    last_synced_at      TIMESTAMPTZ,
    synced_member_count BIGINT NOT NULL DEFAULT 0,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_synced_segments_env ON synced_segment_integrations (org_id, env_id);

-- 9. Phase 4 Lightweight PostgreSQL Analytics Fallback Tables (mirroring ClickHouse DDL)
CREATE TABLE IF NOT EXISTS experiment_exposures (
    id              UUID PRIMARY KEY,
    org_id          UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    env_id          UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    flag_key        TEXT NOT NULL,
    variation_index INTEGER NOT NULL,
    context_kind    TEXT NOT NULL,
    context_key     TEXT NOT NULL,
    exposed_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_exp_exposures_lookup
    ON experiment_exposures (org_id, env_id, flag_key, context_kind, exposed_at);

CREATE TABLE IF NOT EXISTS custom_metric_events (
    id           UUID PRIMARY KEY,
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    env_id       UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    event_key    TEXT NOT NULL,
    context_kind TEXT NOT NULL,
    context_key  TEXT NOT NULL,
    metric_value DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    occurred_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_custom_metric_events_lookup
    ON custom_metric_events (org_id, env_id, event_key, context_kind, occurred_at);

CREATE TABLE IF NOT EXISTS migration_op_metrics (
    id             UUID PRIMARY KEY,
    org_id         UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    env_id         UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    flag_key       TEXT NOT NULL,
    stage          TEXT NOT NULL,
    operation      TEXT NOT NULL,
    old_invoked    BOOLEAN NOT NULL DEFAULT FALSE,
    new_invoked    BOOLEAN NOT NULL DEFAULT FALSE,
    old_error      BOOLEAN NOT NULL DEFAULT FALSE,
    new_error      BOOLEAN NOT NULL DEFAULT FALSE,
    old_latency_ms DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    new_latency_ms DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    consistent     BOOLEAN,
    occurred_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_migration_ops_lookup
    ON migration_op_metrics (org_id, env_id, flag_key, occurred_at);
