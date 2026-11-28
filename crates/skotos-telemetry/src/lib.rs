//! Skotos Telemetry & Evaluation Summary Storage (ADR-0005).
//!
//! Defines the backend-agnostic [`TelemetryStore`] trait and the Phase 2
//! PostgreSQL summary counter implementation (`flag_eval_rollups` + payload
//! idempotency). Phase 4 plugs in `ClickHouseTelemetryStore` behind this exact
//! interface with zero SDK or API breaking changes.

pub mod clickhouse;
pub mod export;
pub mod stats;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use skotos_domain::{
    validate_context_kind, ContextCatalog, FlagEvalUsage, FlagMonitoringSummary,
    FlagVariationMonitoring, MetricDefinition, MigrationStageMetrics, MigrationTelemetrySummary,
    VariationSufficientStats, VariationUsageCount,
};
use sqlx::{PgPool, Row};
use thiserror::Error;
use uuid::Uuid;

pub use clickhouse::{ClickHouseConfig, ClickHouseTelemetryStore};
pub use export::{dispatch_http_sink, encode_zstd_ndjson, format_sink_payload};
pub use stats::{check_srm, evaluate_guardrail_metric, evaluate_metric_experiment, layer_bucket};

pub const CRATE_NAME: &str = "skotos-telemetry";

#[derive(Debug, Error)]
pub enum TelemetryError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("invalid telemetry payload: {0}")]
    InvalidPayload(String),
    #[error("telemetry backend error: {0}")]
    Backend(String),
}

/// Wire format of an individual SDK telemetry event submitted to `services/event-ingest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SdkEvent {
    #[serde(rename_all = "camelCase")]
    Summary {
        start_date: i64,
        end_date: i64,
        features: HashMap<String, SummaryFeatureBucket>,
    },
    #[serde(rename_all = "camelCase")]
    Feature {
        creation_date: i64,
        key: String,
        #[serde(default)]
        version: Option<i64>,
        #[serde(default)]
        variation: Option<i32>,
        #[serde(default)]
        value: Option<Value>,
        #[serde(default)]
        default: Option<Value>,
        #[serde(default)]
        context_keys: HashMap<String, String>,
    },
    #[serde(rename_all = "camelCase")]
    Debug {
        creation_date: i64,
        key: String,
        #[serde(default)]
        version: Option<i64>,
        #[serde(default)]
        variation: Option<i32>,
        #[serde(default)]
        value: Option<Value>,
        #[serde(default)]
        context: Option<Value>,
    },
    #[serde(rename_all = "camelCase")]
    Identify {
        creation_date: i64,
        #[serde(default)]
        context: Option<Value>,
    },
    /// Sent by server SDKs the first time a context is seen in a flush window, so the
    /// control plane can learn which kinds and attributes exist (values are not stored).
    #[serde(rename_all = "camelCase")]
    Index {
        creation_date: i64,
        #[serde(default)]
        context: Option<Value>,
    },
    #[serde(rename_all = "camelCase")]
    Custom {
        creation_date: i64,
        key: String,
        #[serde(default)]
        context_keys: HashMap<String, String>,
        #[serde(default)]
        data: Option<Value>,
        #[serde(default)]
        metric_value: Option<f64>,
    },
    #[serde(rename = "migration_op", rename_all = "camelCase")]
    MigrationOp {
        creation_date: i64,
        key: String,
        #[serde(default = "default_migration_stage")]
        stage: String,
        #[serde(default = "default_migration_op")]
        operation: String,
        #[serde(default)]
        old_invoked: bool,
        #[serde(default)]
        new_invoked: bool,
        #[serde(default)]
        old_error: bool,
        #[serde(default)]
        new_error: bool,
        #[serde(default)]
        old_latency_ms: f64,
        #[serde(default)]
        new_latency_ms: f64,
        #[serde(default)]
        consistent: Option<bool>,
    },
}

fn default_migration_stage() -> String {
    "off".to_string()
}

fn default_migration_op() -> String {
    "read".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryFeatureBucket {
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub context_kinds: Vec<String>,
    #[serde(default)]
    pub counters: Vec<SummaryCounter>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryCounter {
    #[serde(default)]
    pub variation: Option<i32>,
    #[serde(default)]
    pub version: Option<i64>,
    #[serde(default)]
    pub value: Option<Value>,
    #[serde(default)]
    pub unknown: bool,
    pub count: i64,
}

/// Normalized hourly counter increment ready for batched upsert into `TelemetryStore`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryRollupIncrement {
    pub org_id: Uuid,
    pub env_id: Uuid,
    pub flag_key: String,
    pub bucket_hour: DateTime<Utc>,
    pub variation_index: i32,
    pub count: i64,
    pub context_kinds: Vec<String>,
    pub evaluated_at: DateTime<Utc>,
}

type RollupKey = (String, DateTime<Utc>, i32);
type RollupAccumulator = (i64, BTreeSet<String>, DateTime<Utc>);

/// Converts a slice of raw SDK events into aggregated hourly `SummaryRollupIncrement`s.
pub fn extract_rollup_increments(
    org_id: Uuid,
    env_id: Uuid,
    events: &[SdkEvent],
) -> Vec<SummaryRollupIncrement> {
    let mut merged: BTreeMap<RollupKey, RollupAccumulator> = BTreeMap::new();

    for ev in events {
        match ev {
            SdkEvent::Summary {
                end_date, features, ..
            } => {
                let ts = DateTime::from_timestamp_millis(*end_date).unwrap_or_else(Utc::now);
                let hour = truncate_to_hour(ts);
                for (flag_key, bucket) in features {
                    for c in &bucket.counters {
                        if c.count <= 0 {
                            continue;
                        }
                        let var_idx = c.variation.unwrap_or(-1);
                        let entry = merged
                            .entry((flag_key.clone(), hour, var_idx))
                            .or_insert_with(|| (0, BTreeSet::new(), ts));
                        entry.0 += c.count;
                        for ck in &bucket.context_kinds {
                            if !ck.is_empty() {
                                entry.1.insert(ck.clone());
                            }
                        }
                        if ts > entry.2 {
                            entry.2 = ts;
                        }
                    }
                }
            }
            SdkEvent::Feature {
                creation_date,
                key,
                variation,
                context_keys,
                ..
            } => {
                let ts = DateTime::from_timestamp_millis(*creation_date).unwrap_or_else(Utc::now);
                let hour = truncate_to_hour(ts);
                let var_idx = variation.unwrap_or(-1);
                let entry = merged
                    .entry((key.clone(), hour, var_idx))
                    .or_insert_with(|| (0, BTreeSet::new(), ts));
                entry.0 += 1;
                for ck in context_keys.keys() {
                    if !ck.is_empty() {
                        entry.1.insert(ck.clone());
                    }
                }
                if ts > entry.2 {
                    entry.2 = ts;
                }
            }
            _ => {}
        }
    }

    merged
        .into_iter()
        .map(
            |((flag_key, bucket_hour, variation_index), (count, kinds, evaluated_at))| {
                SummaryRollupIncrement {
                    org_id,
                    env_id,
                    flag_key,
                    bucket_hour,
                    variation_index,
                    count,
                    context_kinds: kinds.into_iter().collect(),
                    evaluated_at,
                }
            },
        )
        .collect()
}

fn truncate_to_hour(ts: DateTime<Utc>) -> DateTime<Utc> {
    ts.with_minute(0)
        .and_then(|t| t.with_second(0))
        .and_then(|t| t.with_nanosecond(0))
        .unwrap_or(ts)
}

/// Normalized exposure event linking a context unit to a flag variation (Experimentation & Guardrails).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExposureRecord {
    pub org_id: Uuid,
    pub env_id: Uuid,
    pub flag_key: String,
    pub variation_index: i32,
    pub context_kind: String,
    pub context_key: String,
    pub exposed_at: DateTime<Utc>,
}

/// Normalized custom metric event (`client.track(...)`) joined against exposures.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomEventRecord {
    pub org_id: Uuid,
    pub env_id: Uuid,
    pub event_key: String,
    pub context_kind: String,
    pub context_key: String,
    pub metric_value: f64,
    pub occurred_at: DateTime<Utc>,
}

/// Normalized 2/4/6-stage migration flag operation telemetry record.
#[derive(Debug, Clone, PartialEq)]
pub struct MigrationOpRecord {
    pub org_id: Uuid,
    pub env_id: Uuid,
    pub flag_key: String,
    pub stage: String,
    pub operation: String,
    pub old_invoked: bool,
    pub new_invoked: bool,
    pub old_error: bool,
    pub new_error: bool,
    pub old_latency_ms: f64,
    pub new_latency_ms: f64,
    pub consistent: Option<bool>,
    pub occurred_at: DateTime<Utc>,
}

/// Extracts individual flag exposure records from `SdkEvent::Feature` / `Debug` events.
pub fn extract_exposures(org_id: Uuid, env_id: Uuid, events: &[SdkEvent]) -> Vec<ExposureRecord> {
    let mut out = Vec::new();
    for ev in events {
        if let SdkEvent::Feature {
            creation_date,
            key,
            variation,
            context_keys,
            ..
        } = ev
        {
            let Some(var_idx) = variation else {
                continue;
            };
            let exposed_at =
                DateTime::from_timestamp_millis(*creation_date).unwrap_or_else(Utc::now);
            for (kind, ctx_key) in context_keys {
                if !ctx_key.is_empty() {
                    out.push(ExposureRecord {
                        org_id,
                        env_id,
                        flag_key: key.clone(),
                        variation_index: *var_idx,
                        context_kind: kind.clone(),
                        context_key: ctx_key.clone(),
                        exposed_at,
                    });
                }
            }
        }
    }
    out
}

/// Extracts custom metric events (`SdkEvent::Custom`) for experimentation and guardrails.
pub fn extract_custom_events(
    org_id: Uuid,
    env_id: Uuid,
    events: &[SdkEvent],
) -> Vec<CustomEventRecord> {
    let mut out = Vec::new();
    for ev in events {
        if let SdkEvent::Custom {
            creation_date,
            key,
            context_keys,
            metric_value,
            ..
        } = ev
        {
            let occurred_at =
                DateTime::from_timestamp_millis(*creation_date).unwrap_or_else(Utc::now);
            let val = metric_value.unwrap_or(1.0);
            for (kind, ctx_key) in context_keys {
                if !ctx_key.is_empty() {
                    out.push(CustomEventRecord {
                        org_id,
                        env_id,
                        event_key: key.clone(),
                        context_kind: kind.clone(),
                        context_key: ctx_key.clone(),
                        metric_value: val,
                        occurred_at,
                    });
                }
            }
        }
    }
    out
}

/// Extracts migration operation telemetry events (`SdkEvent::MigrationOp`).
pub fn extract_migration_ops(
    org_id: Uuid,
    env_id: Uuid,
    events: &[SdkEvent],
) -> Vec<MigrationOpRecord> {
    let mut out = Vec::new();
    for ev in events {
        if let SdkEvent::MigrationOp {
            creation_date,
            key,
            stage,
            operation,
            old_invoked,
            new_invoked,
            old_error,
            new_error,
            old_latency_ms,
            new_latency_ms,
            consistent,
        } = ev
        {
            let occurred_at =
                DateTime::from_timestamp_millis(*creation_date).unwrap_or_else(Utc::now);
            out.push(MigrationOpRecord {
                org_id,
                env_id,
                flag_key: key.clone(),
                stage: stage.clone(),
                operation: operation.clone(),
                old_invoked: *old_invoked,
                new_invoked: *new_invoked,
                old_error: *old_error,
                new_error: *new_error,
                old_latency_ms: *old_latency_ms,
                new_latency_ms: *new_latency_ms,
                consistent: *consistent,
                occurred_at,
            });
        }
    }
    out
}

/// Max attribute names recorded per context kind per batch.
pub const MAX_CATALOG_ATTRIBUTES_PER_KIND: usize = 64;
/// Max context keys recorded per context kind per batch.
pub const MAX_CATALOG_KEYS_PER_KIND: usize = 100;
/// Max context keys retained per context kind per environment.
pub const MAX_RETAINED_KEYS_PER_KIND: i64 = 200;
const MAX_CATALOG_NAME_LEN: usize = 128;
const MAX_CATALOG_KEY_LEN: usize = 256;

/// What one event batch taught us about a context kind: attribute names and keys seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextObservation {
    pub org_id: Uuid,
    pub env_id: Uuid,
    pub context_kind: String,
    pub attributes: Vec<String>,
    pub keys: Vec<String>,
    pub seen_at: DateTime<Utc>,
}

#[derive(Default)]
struct KindAccumulator {
    attributes: BTreeSet<String>,
    keys: BTreeSet<String>,
    seen_at: Option<DateTime<Utc>>,
}

impl KindAccumulator {
    fn touch(&mut self, ts: DateTime<Utc>) {
        self.seen_at = Some(self.seen_at.map_or(ts, |prev| prev.max(ts)));
    }

    fn add_key(&mut self, key: &str) {
        if !key.is_empty()
            && key.len() <= MAX_CATALOG_KEY_LEN
            && self.keys.len() < MAX_CATALOG_KEYS_PER_KIND
        {
            self.keys.insert(key.to_string());
        }
    }
}

fn catalog_kind_entry<'a>(
    acc: &'a mut BTreeMap<String, KindAccumulator>,
    kind: &str,
) -> Option<&'a mut KindAccumulator> {
    if validate_context_kind(kind).is_err() {
        return None;
    }
    Some(acc.entry(kind.to_string()).or_default())
}

/// Records attribute names (never values) and the key of one single-kind context object.
fn observe_single_context(
    acc: &mut BTreeMap<String, KindAccumulator>,
    kind: &str,
    obj: &serde_json::Map<String, Value>,
    ts: DateTime<Utc>,
) {
    let Some(entry) = catalog_kind_entry(acc, kind) else {
        return;
    };
    entry.touch(ts);
    let anonymous = obj
        .get("anonymous")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !anonymous {
        if let Some(key) = obj.get("key").and_then(Value::as_str) {
            entry.add_key(key);
        }
    }
    for name in obj.keys() {
        if matches!(name.as_str(), "kind" | "key" | "_meta")
            || name.is_empty()
            || name.len() > MAX_CATALOG_NAME_LEN
            || entry.attributes.len() >= MAX_CATALOG_ATTRIBUTES_PER_KIND
        {
            continue;
        }
        entry.attributes.insert(name.clone());
    }
}

fn observe_context_value(
    acc: &mut BTreeMap<String, KindAccumulator>,
    ctx: &Value,
    ts: DateTime<Utc>,
) {
    let Some(obj) = ctx.as_object() else {
        return;
    };
    match obj.get("kind").and_then(Value::as_str) {
        Some("multi") => {
            for (kind, inner) in obj {
                if kind == "kind" {
                    continue;
                }
                if let Some(inner_obj) = inner.as_object() {
                    observe_single_context(acc, kind, inner_obj, ts);
                }
            }
        }
        Some(kind) => observe_single_context(acc, kind, obj, ts),
        // Legacy contexts without a kind are users.
        None => observe_single_context(acc, "user", obj, ts),
    }
}

/// Collects the context kinds, attribute names and context keys an event batch reveals.
/// Full contexts come from `index`, `identify` and `debug` events; `feature` and `custom`
/// events add keys; summaries add kinds only.
pub fn extract_context_observations(
    org_id: Uuid,
    env_id: Uuid,
    events: &[SdkEvent],
) -> Vec<ContextObservation> {
    let mut acc: BTreeMap<String, KindAccumulator> = BTreeMap::new();
    let ts_of = |millis: i64| DateTime::from_timestamp_millis(millis).unwrap_or_else(Utc::now);

    for ev in events {
        match ev {
            SdkEvent::Index {
                creation_date,
                context: Some(ctx),
            }
            | SdkEvent::Identify {
                creation_date,
                context: Some(ctx),
            }
            | SdkEvent::Debug {
                creation_date,
                context: Some(ctx),
                ..
            } => observe_context_value(&mut acc, ctx, ts_of(*creation_date)),
            SdkEvent::Feature {
                creation_date,
                context_keys,
                ..
            }
            | SdkEvent::Custom {
                creation_date,
                context_keys,
                ..
            } => {
                let ts = ts_of(*creation_date);
                for (kind, key) in context_keys {
                    if let Some(entry) = catalog_kind_entry(&mut acc, kind) {
                        entry.touch(ts);
                        entry.add_key(key);
                    }
                }
            }
            SdkEvent::Summary {
                end_date, features, ..
            } => {
                let ts = ts_of(*end_date);
                for bucket in features.values() {
                    for kind in &bucket.context_kinds {
                        if let Some(entry) = catalog_kind_entry(&mut acc, kind) {
                            entry.touch(ts);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    acc.into_iter()
        .filter_map(|(context_kind, a)| {
            Some(ContextObservation {
                org_id,
                env_id,
                context_kind,
                attributes: a.attributes.into_iter().collect(),
                keys: a.keys.into_iter().collect(),
                seen_at: a.seen_at?,
            })
        })
        .collect()
}

/// Max attribute names returned per context kind by `query_context_catalog`.
pub const MAX_RETAINED_ATTRIBUTES_PER_KIND: i64 = 500;
/// Max recent keys returned per context kind by `query_context_catalog`.
pub const CATALOG_RECENT_KEYS_LIMIT: usize = 50;

/// Builds a [`ContextCatalog`] from flat rows. Attribute and key rows must be ordered
/// newest first; rows for kinds absent from `kinds` are dropped.
pub fn assemble_context_catalog(
    kinds: Vec<(String, DateTime<Utc>)>,
    attributes: Vec<(String, String, DateTime<Utc>)>,
    keys: Vec<(String, String)>,
) -> ContextCatalog {
    let mut entries: BTreeMap<String, skotos_domain::ContextKindCatalogEntry> = kinds
        .into_iter()
        .map(|(kind, last_seen_at)| {
            (
                kind.clone(),
                skotos_domain::ContextKindCatalogEntry {
                    kind,
                    last_seen_at,
                    attributes: Vec::new(),
                    recent_keys: Vec::new(),
                },
            )
        })
        .collect();
    for (kind, name, last_seen_at) in attributes {
        if let Some(e) = entries.get_mut(&kind) {
            if e.attributes.len() < MAX_RETAINED_ATTRIBUTES_PER_KIND as usize {
                e.attributes
                    .push(skotos_domain::ContextAttributeCatalogEntry { name, last_seen_at });
            }
        }
    }
    for (kind, key) in keys {
        if let Some(e) = entries.get_mut(&kind) {
            if e.recent_keys.len() < CATALOG_RECENT_KEYS_LIMIT {
                e.recent_keys.push(key);
            }
        }
    }
    let mut kinds: Vec<_> = entries.into_values().collect();
    kinds.sort_by(|a, b| {
        b.last_seen_at
            .cmp(&a.last_seen_at)
            .then(a.kind.cmp(&b.kind))
    });
    ContextCatalog { kinds }
}

/// Backend-agnostic telemetry storage trait (ADR-0005).
#[async_trait]
pub trait TelemetryStore: Send + Sync {
    /// Checks `X-Skotos-Payload-ID` idempotency; returns `Ok(true)` if this payload is new
    /// and should be processed, or `Ok(false)` if it was already ingested.
    async fn claim_payload_id(
        &self,
        env_id: Uuid,
        payload_id: &str,
    ) -> Result<bool, TelemetryError>;

    /// Persists batched evaluation summary increments.
    async fn record_rollups(
        &self,
        increments: &[SummaryRollupIncrement],
    ) -> Result<(), TelemetryError>;

    /// Returns rolled-up evaluation counts and `lastEvaluatedAt` for a single flag.
    async fn query_flag_usage(
        &self,
        org_id: Uuid,
        env_id: Uuid,
        flag_key: &str,
        lookback_days: i64,
    ) -> Result<FlagEvalUsage, TelemetryError>;

    /// Returns rolled-up evaluation usage across all flags in an environment.
    async fn query_environment_usage(
        &self,
        org_id: Uuid,
        env_id: Uuid,
        lookback_days: i64,
    ) -> Result<Vec<FlagEvalUsage>, TelemetryError>;

    /// Persists individual flag exposure records for Experimentation & Guarded Rollouts.
    async fn record_exposures(&self, exposures: &[ExposureRecord]) -> Result<(), TelemetryError>;

    /// Persists custom metric events (`client.track`).
    async fn record_custom_events(
        &self,
        events: &[CustomEventRecord],
    ) -> Result<(), TelemetryError>;

    /// Persists 2/4/6-stage migration operation metrics.
    async fn record_migration_ops(&self, ops: &[MigrationOpRecord]) -> Result<(), TelemetryError>;

    /// Computes sufficient statistics (`exposures`, `conversions`, `sum`, `sum_sq`) per variation.
    async fn query_sufficient_stats(
        &self,
        org_id: Uuid,
        env_id: Uuid,
        flag_key: &str,
        metric: &MetricDefinition,
        since: DateTime<Utc>,
        until: DateTime<Utc>,
    ) -> Result<Vec<VariationSufficientStats>, TelemetryError>;

    /// Returns per-variation Feature Monitoring telemetry (exposures, error rate, p95 latency, conversions).
    async fn query_flag_monitoring(
        &self,
        org_id: Uuid,
        env_id: Uuid,
        flag_key: &str,
        lookback_hours: i64,
    ) -> Result<FlagMonitoringSummary, TelemetryError>;

    /// Returns per-stage migration consistency, invocation, error, and latency metrics.
    async fn query_migration_telemetry(
        &self,
        org_id: Uuid,
        env_id: Uuid,
        flag_key: &str,
        lookback_days: i64,
    ) -> Result<MigrationTelemetrySummary, TelemetryError>;

    /// Upserts the context kinds, attribute names and keys SDKs sent.
    async fn record_context_observations(
        &self,
        observations: &[ContextObservation],
    ) -> Result<(), TelemetryError>;

    /// Returns the context kinds, attributes and recent keys seen within the lookback window.
    async fn query_context_catalog(
        &self,
        org_id: Uuid,
        env_id: Uuid,
        lookback_days: i64,
    ) -> Result<ContextCatalog, TelemetryError>;
}

/// Instantiates the configured `TelemetryStore` backend per ADR-0005:
/// uses [`ClickHouseTelemetryStore`] when `CLICKHOUSE_URL` is set, otherwise [`PostgresTelemetryStore`].
pub async fn create_telemetry_store(pool: PgPool) -> Arc<dyn TelemetryStore> {
    if let Some(cfg) = ClickHouseConfig::from_env() {
        let store = ClickHouseTelemetryStore::new(cfg.clone());
        match store.ensure_schema().await {
            Ok(()) => {
                tracing::info!(url = %cfg.url, db = %cfg.database, "initialized ClickHouseTelemetryStore");
                return Arc::new(store);
            }
            Err(err) => {
                tracing::warn!(error = %err, url = %cfg.url, "ClickHouse unavailable; falling back to PostgresTelemetryStore");
            }
        }
    }
    Arc::new(PostgresTelemetryStore::new(pool))
}

/// Phase 2–4 PostgreSQL `TelemetryStore` for lightweight/self-hosted deployments.
#[derive(Debug, Clone)]
pub struct PostgresTelemetryStore {
    pool: PgPool,
}

impl PostgresTelemetryStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TelemetryStore for PostgresTelemetryStore {
    async fn claim_payload_id(
        &self,
        env_id: Uuid,
        payload_id: &str,
    ) -> Result<bool, TelemetryError> {
        let trimmed = payload_id.trim();
        if trimmed.is_empty() {
            return Ok(true);
        }
        let inserted: Option<String> = sqlx::query_scalar(
            "INSERT INTO event_payload_dedup (env_id, payload_id)
             VALUES ($1, $2)
             ON CONFLICT (env_id, payload_id) DO NOTHING
             RETURNING payload_id",
        )
        .bind(env_id)
        .bind(trimmed)
        .fetch_optional(&self.pool)
        .await?;
        Ok(inserted.is_some())
    }

    async fn record_rollups(
        &self,
        increments: &[SummaryRollupIncrement],
    ) -> Result<(), TelemetryError> {
        if increments.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for inc in increments {
            sqlx::query(
                "INSERT INTO flag_eval_rollups (
                    org_id, env_id, flag_key, bucket_hour, variation_index,
                    eval_count, context_kinds, last_evaluated_at
                 )
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                 ON CONFLICT (env_id, flag_key, bucket_hour, variation_index)
                 DO UPDATE SET
                    eval_count = flag_eval_rollups.eval_count + EXCLUDED.eval_count,
                    context_kinds = ARRAY(
                        SELECT DISTINCT k
                        FROM unnest(flag_eval_rollups.context_kinds || EXCLUDED.context_kinds) AS k
                        ORDER BY k
                    ),
                    last_evaluated_at = GREATEST(flag_eval_rollups.last_evaluated_at, EXCLUDED.last_evaluated_at)",
            )
            .bind(inc.org_id)
            .bind(inc.env_id)
            .bind(&inc.flag_key)
            .bind(inc.bucket_hour)
            .bind(inc.variation_index)
            .bind(inc.count)
            .bind(&inc.context_kinds)
            .bind(inc.evaluated_at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn query_flag_usage(
        &self,
        org_id: Uuid,
        env_id: Uuid,
        flag_key: &str,
        lookback_days: i64,
    ) -> Result<FlagEvalUsage, TelemetryError> {
        let since = Utc::now() - Duration::days(lookback_days.clamp(1, 365));
        let rows = sqlx::query(
            "SELECT variation_index,
                    SUM(eval_count)::BIGINT AS total_count,
                    MAX(last_evaluated_at) AS max_eval_at,
                    ARRAY_REMOVE(ARRAY_AGG(DISTINCT ck), NULL) AS kinds
             FROM flag_eval_rollups
             LEFT JOIN LATERAL unnest(context_kinds) AS ck ON true
             WHERE org_id = $1 AND env_id = $2 AND flag_key = $3 AND bucket_hour >= $4
             GROUP BY variation_index
             ORDER BY variation_index ASC",
        )
        .bind(org_id)
        .bind(env_id)
        .bind(flag_key)
        .bind(since)
        .fetch_all(&self.pool)
        .await?;

        let mut total: i64 = 0;
        let mut variations = Vec::with_capacity(rows.len());
        let mut kinds_set: BTreeSet<String> = BTreeSet::new();
        let mut last_evaluated_at: Option<DateTime<Utc>> = None;

        for row in &rows {
            let var_idx: i32 = row.get("variation_index");
            let cnt: i64 = row.get("total_count");
            let max_at: Option<DateTime<Utc>> = row.get("max_eval_at");
            let kinds: Vec<String> = row
                .get::<Option<Vec<String>>, _>("kinds")
                .unwrap_or_default();
            total += cnt;
            variations.push(VariationUsageCount {
                variation_index: var_idx,
                count: cnt,
            });
            for k in kinds {
                kinds_set.insert(k);
            }
            if let Some(ts) = max_at {
                last_evaluated_at = Some(match last_evaluated_at {
                    Some(prev) => prev.max(ts),
                    None => ts,
                });
            }
        }

        Ok(FlagEvalUsage {
            flag_key: flag_key.to_string(),
            total_evaluations: total,
            variations,
            context_kinds: kinds_set.into_iter().collect(),
            last_evaluated_at,
        })
    }

    async fn query_environment_usage(
        &self,
        org_id: Uuid,
        env_id: Uuid,
        lookback_days: i64,
    ) -> Result<Vec<FlagEvalUsage>, TelemetryError> {
        let since = Utc::now() - Duration::days(lookback_days.clamp(1, 365));
        let rows = sqlx::query(
            "SELECT flag_key,
                    variation_index,
                    SUM(eval_count)::BIGINT AS total_count,
                    MAX(last_evaluated_at) AS max_eval_at,
                    ARRAY_REMOVE(ARRAY_AGG(DISTINCT ck), NULL) AS kinds
             FROM flag_eval_rollups
             LEFT JOIN LATERAL unnest(context_kinds) AS ck ON true
             WHERE org_id = $1 AND env_id = $2 AND bucket_hour >= $3
             GROUP BY flag_key, variation_index
             ORDER BY flag_key ASC, variation_index ASC",
        )
        .bind(org_id)
        .bind(env_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await?;

        let mut by_flag: BTreeMap<
            String,
            (
                i64,
                Vec<VariationUsageCount>,
                BTreeSet<String>,
                Option<DateTime<Utc>>,
            ),
        > = BTreeMap::new();

        for row in &rows {
            let flag_key: String = row.get("flag_key");
            let var_idx: i32 = row.get("variation_index");
            let cnt: i64 = row.get("total_count");
            let max_at: Option<DateTime<Utc>> = row.get("max_eval_at");
            let kinds: Vec<String> = row
                .get::<Option<Vec<String>>, _>("kinds")
                .unwrap_or_default();

            let entry = by_flag
                .entry(flag_key)
                .or_insert_with(|| (0, Vec::new(), BTreeSet::new(), None));
            entry.0 += cnt;
            entry.1.push(VariationUsageCount {
                variation_index: var_idx,
                count: cnt,
            });
            for k in kinds {
                entry.2.insert(k);
            }
            if let Some(ts) = max_at {
                entry.3 = Some(match entry.3 {
                    Some(prev) => prev.max(ts),
                    None => ts,
                });
            }
        }

        Ok(by_flag
            .into_iter()
            .map(
                |(flag_key, (total_evaluations, variations, kinds, last_evaluated_at))| {
                    FlagEvalUsage {
                        flag_key,
                        total_evaluations,
                        variations,
                        context_kinds: kinds.into_iter().collect(),
                        last_evaluated_at,
                    }
                },
            )
            .collect())
    }

    async fn record_exposures(&self, exposures: &[ExposureRecord]) -> Result<(), TelemetryError> {
        if exposures.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for e in exposures {
            sqlx::query(
                "INSERT INTO experiment_exposures (
                    id, org_id, env_id, flag_key, variation_index, context_kind, context_key, exposed_at
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(Uuid::now_v7())
            .bind(e.org_id)
            .bind(e.env_id)
            .bind(&e.flag_key)
            .bind(e.variation_index)
            .bind(&e.context_kind)
            .bind(&e.context_key)
            .bind(e.exposed_at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn record_custom_events(
        &self,
        events: &[CustomEventRecord],
    ) -> Result<(), TelemetryError> {
        if events.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for e in events {
            sqlx::query(
                "INSERT INTO custom_metric_events (
                    id, org_id, env_id, event_key, context_kind, context_key, metric_value, occurred_at
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(Uuid::now_v7())
            .bind(e.org_id)
            .bind(e.env_id)
            .bind(&e.event_key)
            .bind(&e.context_kind)
            .bind(&e.context_key)
            .bind(e.metric_value)
            .bind(e.occurred_at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn record_migration_ops(&self, ops: &[MigrationOpRecord]) -> Result<(), TelemetryError> {
        if ops.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for o in ops {
            sqlx::query(
                "INSERT INTO migration_op_metrics (
                    id, org_id, env_id, flag_key, stage, operation,
                    old_invoked, new_invoked, old_error, new_error,
                    old_latency_ms, new_latency_ms, consistent, occurred_at
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
            )
            .bind(Uuid::now_v7())
            .bind(o.org_id)
            .bind(o.env_id)
            .bind(&o.flag_key)
            .bind(&o.stage)
            .bind(&o.operation)
            .bind(o.old_invoked)
            .bind(o.new_invoked)
            .bind(o.old_error)
            .bind(o.new_error)
            .bind(o.old_latency_ms)
            .bind(o.new_latency_ms)
            .bind(o.consistent)
            .bind(o.occurred_at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn query_sufficient_stats(
        &self,
        org_id: Uuid,
        env_id: Uuid,
        flag_key: &str,
        metric: &MetricDefinition,
        since: DateTime<Utc>,
        until: DateTime<Utc>,
    ) -> Result<Vec<VariationSufficientStats>, TelemetryError> {
        let rows = sqlx::query(
            "WITH first_exp AS (
                SELECT DISTINCT ON (context_key)
                    context_key, variation_index, exposed_at
                FROM experiment_exposures
                WHERE org_id = $1 AND env_id = $2 AND flag_key = $3
                  AND context_kind = $4 AND exposed_at >= $5 AND exposed_at <= $6
                ORDER BY context_key, exposed_at ASC
            ),
            unit_metrics AS (
                SELECT e.variation_index,
                       e.context_key,
                       COUNT(m.id)::BIGINT AS evt_cnt,
                       COALESCE(SUM(m.metric_value), 0.0)::DOUBLE PRECISION AS unit_val
                FROM first_exp e
                LEFT JOIN custom_metric_events m
                  ON m.org_id = $1 AND m.env_id = $2 AND m.event_key = $7
                 AND m.context_kind = $4 AND m.context_key = e.context_key
                 AND m.occurred_at >= e.exposed_at AND m.occurred_at <= $6
                GROUP BY e.variation_index, e.context_key
            )
            SELECT variation_index,
                   COUNT(*)::BIGINT AS exposures,
                   COUNT(*) FILTER (WHERE evt_cnt > 0)::BIGINT AS conversions,
                   COALESCE(SUM(unit_val), 0.0)::DOUBLE PRECISION AS sum_val,
                   COALESCE(SUM(unit_val * unit_val), 0.0)::DOUBLE PRECISION AS sum_sq
            FROM unit_metrics
            GROUP BY variation_index
            ORDER BY variation_index ASC",
        )
        .bind(org_id)
        .bind(env_id)
        .bind(flag_key)
        .bind(&metric.randomization_unit)
        .bind(since)
        .bind(until)
        .bind(&metric.event_key)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| VariationSufficientStats {
                variation_index: r.get("variation_index"),
                exposures: r.get("exposures"),
                conversions: r.get("conversions"),
                sum: r.get("sum_val"),
                sum_sq: r.get("sum_sq"),
            })
            .collect())
    }

    async fn query_flag_monitoring(
        &self,
        org_id: Uuid,
        env_id: Uuid,
        flag_key: &str,
        lookback_hours: i64,
    ) -> Result<FlagMonitoringSummary, TelemetryError> {
        let since = Utc::now() - Duration::hours(lookback_hours.clamp(1, 720));
        let rows = sqlx::query(
            "WITH exp AS (
                SELECT DISTINCT ON (context_key)
                    context_key, variation_index, exposed_at
                FROM experiment_exposures
                WHERE org_id = $1 AND env_id = $2 AND flag_key = $3 AND exposed_at >= $4
                ORDER BY context_key, exposed_at ASC
            )
            SELECT e.variation_index,
                   COUNT(DISTINCT e.context_key)::BIGINT AS exposures,
                   COUNT(m.id) FILTER (WHERE m.event_key IN ('error', 'flag.error'))::BIGINT AS error_count,
                   COALESCE(AVG(m.metric_value) FILTER (WHERE m.event_key IN ('latency_ms', 'latency')), 0.0)::DOUBLE PRECISION AS avg_latency_ms,
                   COALESCE(PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY m.metric_value) FILTER (WHERE m.event_key IN ('latency_ms', 'latency')), 0.0)::DOUBLE PRECISION AS p95_latency_ms,
                   COUNT(DISTINCT e.context_key) FILTER (WHERE m.event_key IN ('conversion', 'checkout.completed'))::BIGINT AS conversion_count
            FROM exp e
            LEFT JOIN custom_metric_events m
              ON m.org_id = $1 AND m.env_id = $2 AND m.context_key = e.context_key
             AND m.occurred_at >= e.exposed_at
            GROUP BY e.variation_index
            ORDER BY e.variation_index ASC",
        )
        .bind(org_id)
        .bind(env_id)
        .bind(flag_key)
        .bind(since)
        .fetch_all(&self.pool)
        .await?;

        let variations = rows
            .into_iter()
            .map(|r| {
                let exposures: i64 = r.get("exposures");
                let error_count: i64 = r.get("error_count");
                let conversion_count: i64 = r.get("conversion_count");
                let denom = exposures.max(1) as f64;
                FlagVariationMonitoring {
                    variation_index: r.get("variation_index"),
                    exposures,
                    error_count,
                    error_rate: error_count as f64 / denom,
                    avg_latency_ms: r.get("avg_latency_ms"),
                    p95_latency_ms: r.get("p95_latency_ms"),
                    conversion_count,
                    conversion_rate: conversion_count as f64 / denom,
                }
            })
            .collect();

        Ok(FlagMonitoringSummary {
            flag_key: flag_key.to_string(),
            lookback_hours,
            variations,
            generated_at: Utc::now(),
        })
    }

    async fn query_migration_telemetry(
        &self,
        org_id: Uuid,
        env_id: Uuid,
        flag_key: &str,
        lookback_days: i64,
    ) -> Result<MigrationTelemetrySummary, TelemetryError> {
        let since = Utc::now() - Duration::days(lookback_days.clamp(1, 90));
        let rows = sqlx::query(
            "SELECT stage,
                    COUNT(*) FILTER (WHERE old_invoked)::BIGINT AS old_invocations,
                    COUNT(*) FILTER (WHERE new_invoked)::BIGINT AS new_invocations,
                    COUNT(*) FILTER (WHERE old_error)::BIGINT AS old_errors,
                    COUNT(*) FILTER (WHERE new_error)::BIGINT AS new_errors,
                    COALESCE(AVG(old_latency_ms) FILTER (WHERE old_invoked), 0.0)::DOUBLE PRECISION AS old_avg_latency_ms,
                    COALESCE(AVG(new_latency_ms) FILTER (WHERE new_invoked), 0.0)::DOUBLE PRECISION AS new_avg_latency_ms,
                    COUNT(*) FILTER (WHERE consistent = true)::BIGINT AS consistency_matches,
                    COUNT(*) FILTER (WHERE consistent = false)::BIGINT AS consistency_mismatches
             FROM migration_op_metrics
             WHERE org_id = $1 AND env_id = $2 AND flag_key = $3 AND occurred_at >= $4
             GROUP BY stage
             ORDER BY stage ASC",
        )
        .bind(org_id)
        .bind(env_id)
        .bind(flag_key)
        .bind(since)
        .fetch_all(&self.pool)
        .await?;

        let stages = rows
            .into_iter()
            .map(|r| {
                let matches: i64 = r.get("consistency_matches");
                let mismatches: i64 = r.get("consistency_mismatches");
                let total = matches + mismatches;
                MigrationStageMetrics {
                    stage: r.get("stage"),
                    old_invocations: r.get("old_invocations"),
                    new_invocations: r.get("new_invocations"),
                    old_errors: r.get("old_errors"),
                    new_errors: r.get("new_errors"),
                    old_avg_latency_ms: r.get("old_avg_latency_ms"),
                    new_avg_latency_ms: r.get("new_avg_latency_ms"),
                    consistency_matches: matches,
                    consistency_mismatches: mismatches,
                    consistency_rate_pct: if total > 0 {
                        (matches as f64 / total as f64) * 100.0
                    } else {
                        100.0
                    },
                }
            })
            .collect();

        Ok(MigrationTelemetrySummary {
            flag_key: flag_key.to_string(),
            stages,
            generated_at: Utc::now(),
        })
    }

    async fn record_context_observations(
        &self,
        observations: &[ContextObservation],
    ) -> Result<(), TelemetryError> {
        if observations.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for o in observations {
            sqlx::query(
                "INSERT INTO context_kind_catalog (org_id, env_id, context_kind, last_seen_at)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (env_id, context_kind) DO UPDATE
                 SET last_seen_at = GREATEST(context_kind_catalog.last_seen_at, EXCLUDED.last_seen_at)",
            )
            .bind(o.org_id)
            .bind(o.env_id)
            .bind(&o.context_kind)
            .bind(o.seen_at)
            .execute(&mut *tx)
            .await?;

            if !o.attributes.is_empty() {
                sqlx::query(
                    "INSERT INTO context_attribute_catalog (org_id, env_id, context_kind, attribute, last_seen_at)
                     SELECT $1, $2, $3, a, $5 FROM unnest($4::text[]) AS a
                     ON CONFLICT (env_id, context_kind, attribute) DO UPDATE
                     SET last_seen_at = GREATEST(context_attribute_catalog.last_seen_at, EXCLUDED.last_seen_at)",
                )
                .bind(o.org_id)
                .bind(o.env_id)
                .bind(&o.context_kind)
                .bind(&o.attributes)
                .bind(o.seen_at)
                .execute(&mut *tx)
                .await?;
                sqlx::query(
                    "DELETE FROM context_attribute_catalog
                     WHERE env_id = $1 AND context_kind = $2 AND attribute IN (
                         SELECT attribute FROM context_attribute_catalog
                         WHERE env_id = $1 AND context_kind = $2
                         ORDER BY last_seen_at DESC, attribute ASC
                         OFFSET $3
                     )",
                )
                .bind(o.env_id)
                .bind(&o.context_kind)
                .bind(MAX_RETAINED_ATTRIBUTES_PER_KIND)
                .execute(&mut *tx)
                .await?;
            }

            if !o.keys.is_empty() {
                sqlx::query(
                    "INSERT INTO context_key_samples (org_id, env_id, context_kind, context_key, last_seen_at)
                     SELECT $1, $2, $3, k, $5 FROM unnest($4::text[]) AS k
                     ON CONFLICT (env_id, context_kind, context_key) DO UPDATE
                     SET last_seen_at = GREATEST(context_key_samples.last_seen_at, EXCLUDED.last_seen_at)",
                )
                .bind(o.org_id)
                .bind(o.env_id)
                .bind(&o.context_kind)
                .bind(&o.keys)
                .bind(o.seen_at)
                .execute(&mut *tx)
                .await?;
                sqlx::query(
                    "DELETE FROM context_key_samples
                     WHERE env_id = $1 AND context_kind = $2 AND context_key IN (
                         SELECT context_key FROM context_key_samples
                         WHERE env_id = $1 AND context_kind = $2
                         ORDER BY last_seen_at DESC, context_key ASC
                         OFFSET $3
                     )",
                )
                .bind(o.env_id)
                .bind(&o.context_kind)
                .bind(MAX_RETAINED_KEYS_PER_KIND)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    async fn query_context_catalog(
        &self,
        org_id: Uuid,
        env_id: Uuid,
        lookback_days: i64,
    ) -> Result<ContextCatalog, TelemetryError> {
        let since = Utc::now() - Duration::days(lookback_days.clamp(1, 365));
        let kinds = sqlx::query(
            "SELECT context_kind, last_seen_at FROM context_kind_catalog
             WHERE org_id = $1 AND env_id = $2 AND last_seen_at >= $3",
        )
        .bind(org_id)
        .bind(env_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|r| (r.get("context_kind"), r.get("last_seen_at")))
        .collect();
        let attributes = sqlx::query(
            "SELECT context_kind, attribute, last_seen_at FROM context_attribute_catalog
             WHERE org_id = $1 AND env_id = $2 AND last_seen_at >= $3
             ORDER BY last_seen_at DESC, attribute ASC",
        )
        .bind(org_id)
        .bind(env_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|r| {
            (
                r.get("context_kind"),
                r.get("attribute"),
                r.get("last_seen_at"),
            )
        })
        .collect();
        let keys = sqlx::query(
            "SELECT context_kind, context_key FROM context_key_samples
             WHERE org_id = $1 AND env_id = $2 AND last_seen_at >= $3
             ORDER BY last_seen_at DESC, context_key ASC",
        )
        .bind(org_id)
        .bind(env_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|r| (r.get("context_kind"), r.get("context_key")))
        .collect();
        Ok(assemble_context_catalog(kinds, attributes, keys))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_context_catalog_from_index_identify_feature_and_summary_events() {
        let org = Uuid::nil();
        let env = Uuid::nil();
        let events: Vec<SdkEvent> = serde_json::from_value(serde_json::json!([
            {
                "kind": "index",
                "creationDate": 1_700_000_000_000i64,
                "context": {
                    "kind": "multi",
                    "user": { "key": "u-1", "email": "a@b.co", "plan": "pro",
                              "_meta": { "redactedAttributes": ["ssn"] } },
                    "organization": { "key": "org-9", "name": "Acme", "tier": 2 }
                }
            },
            {
                "kind": "identify",
                "creationDate": 1_700_000_100_000i64,
                "context": { "kind": "user", "key": "anon-7", "anonymous": true, "country": "PK" }
            },
            { "kind": "identify", "creationDate": 1_700_000_000_000i64,
              "context": { "key": "legacy-1", "firstName": "Sam" } },
            {
                "kind": "feature",
                "creationDate": 1_700_000_200_000i64,
                "key": "f",
                "contextKeys": { "device": "d-42", "bad kind!": "x" }
            },
            {
                "kind": "summary", "startDate": 0, "endDate": 1_700_000_300_000i64,
                "features": { "f": { "contextKinds": ["service"], "counters": [] } }
            }
        ]))
        .unwrap();

        let obs = extract_context_observations(org, env, &events);
        let by_kind: BTreeMap<_, _> = obs.iter().map(|o| (o.context_kind.as_str(), o)).collect();
        assert_eq!(
            by_kind.keys().copied().collect::<Vec<_>>(),
            vec!["device", "organization", "service", "user"]
        );

        let user = by_kind["user"];
        assert_eq!(
            user.attributes,
            vec!["anonymous", "country", "email", "firstName", "plan"]
        );
        // Anonymous keys are not kept; legacy kind-less contexts count as users.
        assert_eq!(user.keys, vec!["legacy-1", "u-1"]);
        assert_eq!(
            user.seen_at,
            DateTime::from_timestamp_millis(1_700_000_100_000).unwrap()
        );

        assert_eq!(by_kind["organization"].attributes, vec!["name", "tier"]);
        assert_eq!(by_kind["organization"].keys, vec!["org-9"]);
        assert_eq!(by_kind["device"].keys, vec!["d-42"]);
        assert!(by_kind["service"].attributes.is_empty());
        assert!(by_kind["service"].keys.is_empty());
    }

    #[test]
    fn assembles_context_catalog_newest_kind_first_and_caps_keys() {
        let t0 = DateTime::from_timestamp_millis(1_000).unwrap();
        let t1 = DateTime::from_timestamp_millis(2_000).unwrap();
        let keys: Vec<(String, String)> = (0..80)
            .map(|i| ("user".to_string(), format!("u-{i}")))
            .chain([("ghost".to_string(), "g".to_string())])
            .collect();
        let catalog = assemble_context_catalog(
            vec![("org".into(), t0), ("user".into(), t1)],
            vec![
                ("user".into(), "plan".into(), t1),
                ("org".into(), "tier".into(), t0),
            ],
            keys,
        );
        let kinds: Vec<_> = catalog.kinds.iter().map(|k| k.kind.as_str()).collect();
        assert_eq!(kinds, vec!["user", "org"]);
        assert_eq!(catalog.kinds[0].attributes[0].name, "plan");
        assert_eq!(
            catalog.kinds[0].recent_keys.len(),
            CATALOG_RECENT_KEYS_LIMIT
        );
        assert_eq!(catalog.kinds[0].recent_keys[0], "u-0");
        assert!(catalog.kinds[1].recent_keys.is_empty());
    }

    #[test]
    fn merges_summary_and_feature_events_into_hourly_buckets() {
        let org_id = Uuid::new_v4();
        let env_id = Uuid::new_v4();
        let ts_ms = 1_795_800_123_000_i64;
        let mut features = HashMap::new();
        features.insert(
            "checkout-v2".to_string(),
            SummaryFeatureBucket {
                default: Some(Value::Bool(false)),
                context_kinds: vec!["user".to_string()],
                counters: vec![
                    SummaryCounter {
                        variation: Some(0),
                        version: Some(3),
                        value: Some(Value::Bool(true)),
                        unknown: false,
                        count: 15,
                    },
                    SummaryCounter {
                        variation: Some(1),
                        version: Some(3),
                        value: Some(Value::Bool(false)),
                        unknown: false,
                        count: 5,
                    },
                ],
            },
        );
        let events = vec![
            SdkEvent::Summary {
                start_date: ts_ms - 5_000,
                end_date: ts_ms,
                features,
            },
            SdkEvent::Feature {
                creation_date: ts_ms + 1_000,
                key: "checkout-v2".to_string(),
                version: Some(3),
                variation: Some(0),
                value: Some(Value::Bool(true)),
                default: Some(Value::Bool(false)),
                context_keys: HashMap::from([("organization".to_string(), "acme".to_string())]),
            },
        ];

        let incs = extract_rollup_increments(org_id, env_id, &events);
        assert_eq!(incs.len(), 2);
        let var0 = incs.iter().find(|i| i.variation_index == 0).unwrap();
        assert_eq!(var0.count, 16);
        assert_eq!(var0.context_kinds, vec!["organization", "user"]);
        let var1 = incs.iter().find(|i| i.variation_index == 1).unwrap();
        assert_eq!(var1.count, 5);
    }
}
