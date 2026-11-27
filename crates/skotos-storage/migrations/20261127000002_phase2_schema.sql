-- Skotos Phase 2 Schema Extensions
-- Adds outbox retry/backoff metadata, evaluation summary rollups, payload idempotency,
-- scheduled changes, flag triggers, and signed outbound webhooks.

ALTER TABLE outbox_events
    ADD COLUMN IF NOT EXISTS attempts INT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS last_error TEXT;

CREATE INDEX IF NOT EXISTS idx_outbox_events_claimable
    ON outbox_events (next_attempt_at ASC, created_at ASC)
    WHERE published_at IS NULL;

-- Hourly per-variation evaluation counters (ADR-0005 Phase 2 Postgres TelemetryStore)
CREATE TABLE IF NOT EXISTS flag_eval_rollups (
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    env_id UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    flag_key TEXT NOT NULL,
    bucket_hour TIMESTAMPTZ NOT NULL,
    variation_index INT NOT NULL DEFAULT -1,
    eval_count BIGINT NOT NULL DEFAULT 0,
    context_kinds TEXT[] NOT NULL DEFAULT '{}',
    last_evaluated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (env_id, flag_key, bucket_hour, variation_index)
);

CREATE INDEX IF NOT EXISTS idx_flag_eval_rollups_org_env_flag
    ON flag_eval_rollups (org_id, env_id, flag_key, bucket_hour DESC);

-- Event ingest X-Skotos-Payload-ID idempotency deduplication table
CREATE TABLE IF NOT EXISTS event_payload_dedup (
    env_id UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    payload_id TEXT NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (env_id, payload_id)
);

CREATE INDEX IF NOT EXISTS idx_event_payload_dedup_received
    ON event_payload_dedup (received_at ASC);

-- Flag Scheduling queue (claimed via FOR UPDATE SKIP LOCKED)
CREATE TABLE IF NOT EXISTS scheduled_changes (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    flag_id UUID NOT NULL REFERENCES flags(id) ON DELETE CASCADE,
    env_id UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    execute_at TIMESTAMPTZ NOT NULL,
    instructions_jsonb JSONB NOT NULL,
    comment TEXT,
    created_by UUID REFERENCES members(id) ON DELETE SET NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'executed', 'failed', 'canceled')),
    error_message TEXT,
    executed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_scheduled_changes_due
    ON scheduled_changes (execute_at ASC)
    WHERE status = 'pending';

CREATE INDEX IF NOT EXISTS idx_scheduled_changes_flag_env
    ON scheduled_changes (org_id, flag_id, env_id, execute_at ASC);

-- APM / Webhook Flag Triggers (secret token hashed with SHA-256)
CREATE TABLE IF NOT EXISTS flag_triggers (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    flag_id UUID NOT NULL REFERENCES flags(id) ON DELETE CASCADE,
    env_id UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    action TEXT NOT NULL CHECK (action IN ('turnFlagOn', 'turnFlagOff')),
    token_prefix TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    enabled BOOLEAN NOT NULL DEFAULT true,
    trigger_count BIGINT NOT NULL DEFAULT 0,
    last_triggered_at TIMESTAMPTZ,
    created_by UUID REFERENCES members(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_flag_triggers_flag_env
    ON flag_triggers (org_id, flag_id, env_id);

-- Signed Outbound Webhooks (HMAC-SHA256)
CREATE TABLE IF NOT EXISTS webhooks (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    url TEXT NOT NULL,
    secret TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT true,
    events TEXT[] NOT NULL DEFAULT '{}',
    env_filter TEXT[] NOT NULL DEFAULT '{}',
    created_by UUID REFERENCES members(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_webhooks_org ON webhooks (org_id, created_at DESC);

CREATE TABLE IF NOT EXISTS webhook_deliveries (
    id UUID PRIMARY KEY,
    webhook_id UUID NOT NULL REFERENCES webhooks(id) ON DELETE CASCADE,
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    status_code INT,
    attempt INT NOT NULL DEFAULT 1,
    success BOOLEAN NOT NULL DEFAULT false,
    error_message TEXT,
    delivered_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_webhook_deliveries_webhook
    ON webhook_deliveries (org_id, webhook_id, delivered_at DESC);
