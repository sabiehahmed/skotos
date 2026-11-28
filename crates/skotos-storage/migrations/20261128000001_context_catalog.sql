-- Context catalog: which context kinds, attribute names and keys SDKs send.
-- Feeds the rule clause dropdowns in the management UI. Attribute values are never stored;
-- keys are kept only for non-anonymous contexts and capped per kind by the TelemetryStore.

CREATE TABLE IF NOT EXISTS context_kind_catalog (
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    env_id       UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    context_kind TEXT NOT NULL,
    last_seen_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (env_id, context_kind)
);

CREATE TABLE IF NOT EXISTS context_attribute_catalog (
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    env_id       UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    context_kind TEXT NOT NULL,
    attribute    TEXT NOT NULL,
    last_seen_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (env_id, context_kind, attribute)
);

CREATE INDEX IF NOT EXISTS idx_context_attribute_catalog_recent
    ON context_attribute_catalog (env_id, context_kind, last_seen_at DESC);

CREATE TABLE IF NOT EXISTS context_key_samples (
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    env_id       UUID NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    context_kind TEXT NOT NULL,
    context_key  TEXT NOT NULL,
    last_seen_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (env_id, context_kind, context_key)
);

CREATE INDEX IF NOT EXISTS idx_context_key_samples_recent
    ON context_key_samples (env_id, context_kind, last_seen_at DESC);
