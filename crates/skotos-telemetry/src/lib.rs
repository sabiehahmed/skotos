//! Skotos Telemetry & Evaluation Summary Storage (ADR-0005).
//!
//! Defines the backend-agnostic [`TelemetryStore`] trait and the Phase 2
//! PostgreSQL summary counter implementation (`flag_eval_rollups` + payload
//! idempotency). Phase 4 plugs in `ClickHouseTelemetryStore` behind this exact
//! interface with zero SDK or API breaking changes.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use async_trait::async_trait;
use chrono::{DateTime, Duration, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use skotos_domain::{FlagEvalUsage, VariationUsageCount};
use sqlx::{PgPool, Row};
use thiserror::Error;
use uuid::Uuid;

pub const CRATE_NAME: &str = "skotos-telemetry";

#[derive(Debug, Error)]
pub enum TelemetryError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("invalid telemetry payload: {0}")]
    InvalidPayload(String),
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
}

/// Phase 2 PostgreSQL `TelemetryStore` writing compact hourly counters to `flag_eval_rollups`.
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
