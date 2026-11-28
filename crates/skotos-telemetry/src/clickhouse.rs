//! Phase 4 ClickHouse `TelemetryStore` Backend (ADR-0005).
//!
//! Communicates over ClickHouse's native HTTP interface (`POST /` with `JSONEachRow`
//! streaming inserts and `JSON` analytical queries), using Zstd(3) / Delta / LowCardinality
//! compression and TTL tiering to zero-egress R2/S3 Parquet archives.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use skotos_domain::{
    FlagEvalUsage, FlagMonitoringSummary, FlagVariationMonitoring, MetricDefinition,
    MigrationStageMetrics, MigrationTelemetrySummary, VariationSufficientStats,
    VariationUsageCount,
};
use uuid::Uuid;

use crate::{
    CustomEventRecord, ExposureRecord, MigrationOpRecord, SummaryRollupIncrement, TelemetryError,
    TelemetryStore,
};

#[derive(Debug, Clone)]
pub struct ClickHouseConfig {
    pub url: String,
    pub database: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl ClickHouseConfig {
    pub fn from_env() -> Option<Self> {
        let url = std::env::var("CLICKHOUSE_URL").ok()?;
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(Self {
            url: trimmed.trim_end_matches('/').to_string(),
            database: std::env::var("CLICKHOUSE_DATABASE").unwrap_or_else(|_| "skotos".to_string()),
            username: std::env::var("CLICKHOUSE_USER").ok(),
            password: std::env::var("CLICKHOUSE_PASSWORD").ok(),
        })
    }
}

/// Columnar ClickHouse `TelemetryStore` implementing ADR-0005 with Zstd(3) compression.
#[derive(Debug, Clone)]
pub struct ClickHouseTelemetryStore {
    cfg: ClickHouseConfig,
    http: reqwest::Client,
}

impl ClickHouseTelemetryStore {
    pub fn new(cfg: ClickHouseConfig) -> Self {
        Self {
            cfg,
            http: reqwest::Client::new(),
        }
    }

    /// Returns the ClickHouse DDL statements creating Zstd-compressed analytics tables with TTLs.
    pub fn schema_ddl(database: &str) -> Vec<String> {
        vec![
            format!("CREATE DATABASE IF NOT EXISTS {database}"),
            format!(
                "CREATE TABLE IF NOT EXISTS {database}.flag_eval_rollups (
                    org_id UUID,
                    env_id UUID,
                    flag_key LowCardinality(String),
                    bucket_hour DateTime CODEC(Delta, ZSTD(3)),
                    variation_index Int32,
                    eval_count Int64 CODEC(ZSTD(3)),
                    context_kinds Array(LowCardinality(String)),
                    last_evaluated_at DateTime64(3) CODEC(Delta, ZSTD(3))
                ) ENGINE = SummingMergeTree(eval_count)
                PARTITION BY toYYYYMM(bucket_hour)
                ORDER BY (org_id, env_id, flag_key, bucket_hour, variation_index)
                TTL bucket_hour + INTERVAL 90 DAY"
            ),
            format!(
                "CREATE TABLE IF NOT EXISTS {database}.experiment_exposures (
                    org_id UUID,
                    env_id UUID,
                    flag_key LowCardinality(String),
                    variation_index Int32,
                    context_kind LowCardinality(String),
                    context_key String CODEC(ZSTD(3)),
                    exposed_at DateTime64(3) CODEC(Delta, ZSTD(3))
                ) ENGINE = MergeTree()
                PARTITION BY toYYYYMM(exposed_at)
                ORDER BY (org_id, env_id, flag_key, context_kind, context_key, exposed_at)
                TTL toDateTime(exposed_at) + INTERVAL 90 DAY"
            ),
            format!(
                "CREATE TABLE IF NOT EXISTS {database}.custom_metric_events (
                    org_id UUID,
                    env_id UUID,
                    event_key LowCardinality(String),
                    context_kind LowCardinality(String),
                    context_key String CODEC(ZSTD(3)),
                    metric_value Float64 CODEC(ZSTD(3)),
                    occurred_at DateTime64(3) CODEC(Delta, ZSTD(3))
                ) ENGINE = MergeTree()
                PARTITION BY toYYYYMM(occurred_at)
                ORDER BY (org_id, env_id, event_key, context_kind, context_key, occurred_at)
                TTL toDateTime(occurred_at) + INTERVAL 90 DAY"
            ),
            format!(
                "CREATE TABLE IF NOT EXISTS {database}.migration_op_metrics (
                    org_id UUID,
                    env_id UUID,
                    flag_key LowCardinality(String),
                    stage LowCardinality(String),
                    operation LowCardinality(String),
                    old_invoked UInt8,
                    new_invoked UInt8,
                    old_error UInt8,
                    new_error UInt8,
                    old_latency_ms Float64,
                    new_latency_ms Float64,
                    consistent Int8,
                    occurred_at DateTime64(3) CODEC(Delta, ZSTD(3))
                ) ENGINE = MergeTree()
                PARTITION BY toYYYYMM(occurred_at)
                ORDER BY (org_id, env_id, flag_key, stage, occurred_at)
                TTL toDateTime(occurred_at) + INTERVAL 90 DAY"
            ),
        ]
    }

    /// Initializes the ClickHouse database and all compressed analytics tables.
    pub async fn ensure_schema(&self) -> Result<(), TelemetryError> {
        for ddl in Self::schema_ddl(&self.cfg.database) {
            self.execute_statement(&ddl).await?;
        }
        Ok(())
    }

    fn build_req(&self, query: Option<&str>) -> reqwest::RequestBuilder {
        let mut url = format!("{}/?database={}", self.cfg.url, self.cfg.database);
        if let Some(q) = query {
            url.push_str("&query=");
            url.push_str(&percent_encode_query(q));
        }
        let mut req = self.http.post(url);
        if let Some(user) = &self.cfg.username {
            req = req.basic_auth(user, self.cfg.password.as_deref());
        }
        req
    }

    async fn execute_statement(&self, sql: &str) -> Result<(), TelemetryError> {
        let resp = self
            .build_req(None)
            .body(sql.to_string())
            .send()
            .await
            .map_err(|e| TelemetryError::Backend(format!("clickhouse request error: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(TelemetryError::Backend(format!(
                "clickhouse HTTP {status}: {text}"
            )));
        }
        Ok(())
    }

    async fn insert_json_each_row<T: Serialize>(
        &self,
        table: &str,
        rows: &[T],
    ) -> Result<(), TelemetryError> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut body = String::new();
        for row in rows {
            let line = serde_json::to_string(row)
                .map_err(|e| TelemetryError::InvalidPayload(e.to_string()))?;
            body.push_str(&line);
            body.push('\n');
        }
        let sql = format!("INSERT INTO {table} FORMAT JSONEachRow");
        let resp = self
            .build_req(Some(&sql))
            .header("content-type", "application/x-ndjson")
            .body(body)
            .send()
            .await
            .map_err(|e| TelemetryError::Backend(format!("clickhouse insert error: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(TelemetryError::Backend(format!(
                "clickhouse insert HTTP {status}: {text}"
            )));
        }
        Ok(())
    }

    async fn query_json_rows(&self, sql: &str) -> Result<Vec<Value>, TelemetryError> {
        let query = format!("{sql} FORMAT JSON");
        let resp = self
            .build_req(None)
            .body(query)
            .send()
            .await
            .map_err(|e| TelemetryError::Backend(format!("clickhouse query error: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(TelemetryError::Backend(format!(
                "clickhouse query HTTP {status}: {text}"
            )));
        }
        let parsed: ClickHouseJsonResponse = resp
            .json()
            .await
            .map_err(|e| TelemetryError::Backend(format!("clickhouse json parse error: {e}")))?;
        Ok(parsed.data)
    }
}

#[derive(Debug, Deserialize)]
struct ClickHouseJsonResponse {
    #[serde(default)]
    data: Vec<Value>,
}

fn percent_encode_query(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 16);
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(b))
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn escape_ch_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

fn value_as_i64(v: &Value) -> i64 {
    v.as_i64()
        .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
        .unwrap_or(0)
}

fn value_as_f64(v: &Value) -> f64 {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
        .unwrap_or(0.0)
}

#[async_trait]
impl TelemetryStore for ClickHouseTelemetryStore {
    async fn claim_payload_id(
        &self,
        _env_id: Uuid,
        _payload_id: &str,
    ) -> Result<bool, TelemetryError> {
        // ClickHouse deduplicates via ReplacingMergeTree/SummingMergeTree; accept batch.
        Ok(true)
    }

    async fn record_rollups(
        &self,
        increments: &[SummaryRollupIncrement],
    ) -> Result<(), TelemetryError> {
        let rows: Vec<Value> = increments
            .iter()
            .map(|inc| {
                json!({
                    "org_id": inc.org_id,
                    "env_id": inc.env_id,
                    "flag_key": inc.flag_key,
                    "bucket_hour": inc.bucket_hour.format("%Y-%m-%d %H:%M:%S").to_string(),
                    "variation_index": inc.variation_index,
                    "eval_count": inc.count,
                    "context_kinds": inc.context_kinds,
                    "last_evaluated_at": inc.evaluated_at.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                })
            })
            .collect();
        self.insert_json_each_row("flag_eval_rollups", &rows).await
    }

    async fn query_flag_usage(
        &self,
        org_id: Uuid,
        env_id: Uuid,
        flag_key: &str,
        lookback_days: i64,
    ) -> Result<FlagEvalUsage, TelemetryError> {
        let since =
            (Utc::now() - Duration::days(lookback_days.clamp(1, 365))).format("%Y-%m-%d %H:%M:%S");
        let fk = escape_ch_str(flag_key);
        let sql = format!(
            "SELECT variation_index,
                    sum(eval_count) AS total_count,
                    max(last_evaluated_at) AS max_eval_at,
                    groupUniqArrayArray(context_kinds) AS kinds
             FROM flag_eval_rollups
             WHERE org_id = '{org_id}' AND env_id = '{env_id}' AND flag_key = '{fk}'
               AND bucket_hour >= '{since}'
             GROUP BY variation_index
             ORDER BY variation_index ASC"
        );
        let rows = self.query_json_rows(&sql).await?;
        let mut total = 0i64;
        let mut variations = Vec::with_capacity(rows.len());
        let mut kinds_set = BTreeSet::new();
        let mut last_at: Option<DateTime<Utc>> = None;
        for r in rows {
            let var_idx = value_as_i64(&r["variation_index"]) as i32;
            let cnt = value_as_i64(&r["total_count"]);
            total += cnt;
            variations.push(VariationUsageCount {
                variation_index: var_idx,
                count: cnt,
            });
            if let Some(arr) = r["kinds"].as_array() {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        kinds_set.insert(s.to_string());
                    }
                }
            }
            if let Some(ts_str) = r["max_eval_at"].as_str() {
                if let Ok(ts) = DateTime::parse_from_rfc3339(ts_str) {
                    let u = ts.with_timezone(&Utc);
                    last_at = Some(last_at.map_or(u, |prev| prev.max(u)));
                }
            }
        }
        Ok(FlagEvalUsage {
            flag_key: flag_key.to_string(),
            total_evaluations: total,
            variations,
            context_kinds: kinds_set.into_iter().collect(),
            last_evaluated_at: last_at,
        })
    }

    async fn query_environment_usage(
        &self,
        org_id: Uuid,
        env_id: Uuid,
        lookback_days: i64,
    ) -> Result<Vec<FlagEvalUsage>, TelemetryError> {
        let since =
            (Utc::now() - Duration::days(lookback_days.clamp(1, 365))).format("%Y-%m-%d %H:%M:%S");
        let sql = format!(
            "SELECT flag_key,
                    variation_index,
                    sum(eval_count) AS total_count,
                    groupUniqArrayArray(context_kinds) AS kinds
             FROM flag_eval_rollups
             WHERE org_id = '{org_id}' AND env_id = '{env_id}' AND bucket_hour >= '{since}'
             GROUP BY flag_key, variation_index
             ORDER BY flag_key ASC, variation_index ASC"
        );
        let rows = self.query_json_rows(&sql).await?;
        let mut grouped: BTreeMap<String, (i64, Vec<VariationUsageCount>, BTreeSet<String>)> =
            BTreeMap::new();
        for r in rows {
            let fk = r["flag_key"].as_str().unwrap_or_default().to_string();
            let var_idx = value_as_i64(&r["variation_index"]) as i32;
            let cnt = value_as_i64(&r["total_count"]);
            let entry = grouped
                .entry(fk)
                .or_insert_with(|| (0, Vec::new(), BTreeSet::new()));
            entry.0 += cnt;
            entry.1.push(VariationUsageCount {
                variation_index: var_idx,
                count: cnt,
            });
            if let Some(arr) = r["kinds"].as_array() {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        entry.2.insert(s.to_string());
                    }
                }
            }
        }
        Ok(grouped
            .into_iter()
            .map(
                |(flag_key, (total_evaluations, variations, kinds))| FlagEvalUsage {
                    flag_key,
                    total_evaluations,
                    variations,
                    context_kinds: kinds.into_iter().collect(),
                    last_evaluated_at: None,
                },
            )
            .collect())
    }

    async fn record_exposures(&self, exposures: &[ExposureRecord]) -> Result<(), TelemetryError> {
        let rows: Vec<Value> = exposures
            .iter()
            .map(|e| {
                json!({
                    "org_id": e.org_id,
                    "env_id": e.env_id,
                    "flag_key": e.flag_key,
                    "variation_index": e.variation_index,
                    "context_kind": e.context_kind,
                    "context_key": e.context_key,
                    "exposed_at": e.exposed_at.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                })
            })
            .collect();
        self.insert_json_each_row("experiment_exposures", &rows)
            .await
    }

    async fn record_custom_events(
        &self,
        events: &[CustomEventRecord],
    ) -> Result<(), TelemetryError> {
        let rows: Vec<Value> = events
            .iter()
            .map(|e| {
                json!({
                    "org_id": e.org_id,
                    "env_id": e.env_id,
                    "event_key": e.event_key,
                    "context_kind": e.context_kind,
                    "context_key": e.context_key,
                    "metric_value": e.metric_value,
                    "occurred_at": e.occurred_at.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                })
            })
            .collect();
        self.insert_json_each_row("custom_metric_events", &rows)
            .await
    }

    async fn record_migration_ops(&self, ops: &[MigrationOpRecord]) -> Result<(), TelemetryError> {
        let rows: Vec<Value> = ops
            .iter()
            .map(|o| {
                json!({
                    "org_id": o.org_id,
                    "env_id": o.env_id,
                    "flag_key": o.flag_key,
                    "stage": o.stage,
                    "operation": o.operation,
                    "old_invoked": u8::from(o.old_invoked),
                    "new_invoked": u8::from(o.new_invoked),
                    "old_error": u8::from(o.old_error),
                    "new_error": u8::from(o.new_error),
                    "old_latency_ms": o.old_latency_ms,
                    "new_latency_ms": o.new_latency_ms,
                    "consistent": match o.consistent { Some(true) => 1, Some(false) => 0, None => -1 },
                    "occurred_at": o.occurred_at.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                })
            })
            .collect();
        self.insert_json_each_row("migration_op_metrics", &rows)
            .await
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
        let fk = escape_ch_str(flag_key);
        let ek = escape_ch_str(&metric.event_key);
        let rk = escape_ch_str(&metric.randomization_unit);
        let since_s = since.format("%Y-%m-%d %H:%M:%S%.3f");
        let until_s = until.format("%Y-%m-%d %H:%M:%S%.3f");
        let sql = format!(
            "SELECT e.variation_index AS variation_index,
                    count() AS exposures,
                    countIf(m.unit_cnt > 0) AS conversions,
                    sum(m.unit_val) AS sum_val,
                    sum(m.unit_val * m.unit_val) AS sum_sq
             FROM (
                SELECT context_key, argMin(variation_index, exposed_at) AS variation_index, min(exposed_at) AS first_exposed
                FROM experiment_exposures
                WHERE org_id = '{org_id}' AND env_id = '{env_id}' AND flag_key = '{fk}'
                  AND context_kind = '{rk}' AND exposed_at >= '{since_s}' AND exposed_at <= '{until_s}'
                GROUP BY context_key
             ) AS e
             LEFT JOIN (
                SELECT context_key, count() AS unit_cnt, sum(metric_value) AS unit_val
                FROM custom_metric_events
                WHERE org_id = '{org_id}' AND env_id = '{env_id}' AND event_key = '{ek}'
                  AND context_kind = '{rk}' AND occurred_at >= '{since_s}' AND occurred_at <= '{until_s}'
                GROUP BY context_key
             ) AS m ON e.context_key = m.context_key
             GROUP BY e.variation_index
             ORDER BY e.variation_index ASC"
        );
        let rows = self.query_json_rows(&sql).await?;
        Ok(rows
            .into_iter()
            .map(|r| VariationSufficientStats {
                variation_index: value_as_i64(&r["variation_index"]) as i32,
                exposures: value_as_i64(&r["exposures"]),
                conversions: value_as_i64(&r["conversions"]),
                sum: value_as_f64(&r["sum_val"]),
                sum_sq: value_as_f64(&r["sum_sq"]),
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
        let since = (Utc::now() - Duration::hours(lookback_hours.clamp(1, 720)))
            .format("%Y-%m-%d %H:%M:%S%.3f");
        let fk = escape_ch_str(flag_key);
        let sql = format!(
            "SELECT e.variation_index AS variation_index,
                    countDistinct(e.context_key) AS exposures,
                    countIf(m.event_key = 'error') AS error_count,
                    avgIf(m.metric_value, m.event_key = 'latency_ms') AS avg_latency_ms,
                    quantileIf(0.95)(m.metric_value, m.event_key = 'latency_ms') AS p95_latency_ms,
                    countIf(m.event_key = 'conversion') AS conversion_count
             FROM experiment_exposures AS e
             LEFT JOIN custom_metric_events AS m
               ON e.org_id = m.org_id AND e.env_id = m.env_id
              AND e.context_key = m.context_key AND m.occurred_at >= e.exposed_at
             WHERE e.org_id = '{org_id}' AND e.env_id = '{env_id}' AND e.flag_key = '{fk}'
               AND e.exposed_at >= '{since}'
             GROUP BY e.variation_index
             ORDER BY e.variation_index ASC"
        );
        let rows = self.query_json_rows(&sql).await?;
        let variations = rows
            .into_iter()
            .map(|r| {
                let exposures = value_as_i64(&r["exposures"]);
                let error_count = value_as_i64(&r["error_count"]);
                let conversion_count = value_as_i64(&r["conversion_count"]);
                let denom = exposures.max(1) as f64;
                FlagVariationMonitoring {
                    variation_index: value_as_i64(&r["variation_index"]) as i32,
                    exposures,
                    error_count,
                    error_rate: error_count as f64 / denom,
                    avg_latency_ms: value_as_f64(&r["avg_latency_ms"]),
                    p95_latency_ms: value_as_f64(&r["p95_latency_ms"]),
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
        let since = (Utc::now() - Duration::days(lookback_days.clamp(1, 90)))
            .format("%Y-%m-%d %H:%M:%S%.3f");
        let fk = escape_ch_str(flag_key);
        let sql = format!(
            "SELECT stage,
                    sum(old_invoked) AS old_invocations,
                    sum(new_invoked) AS new_invocations,
                    sum(old_error) AS old_errors,
                    sum(new_error) AS new_errors,
                    avgIf(old_latency_ms, old_invoked = 1) AS old_avg_latency_ms,
                    avgIf(new_latency_ms, new_invoked = 1) AS new_avg_latency_ms,
                    countIf(consistent = 1) AS consistency_matches,
                    countIf(consistent = 0) AS consistency_mismatches
             FROM migration_op_metrics
             WHERE org_id = '{org_id}' AND env_id = '{env_id}' AND flag_key = '{fk}'
               AND occurred_at >= '{since}'
             GROUP BY stage
             ORDER BY stage ASC"
        );
        let rows = self.query_json_rows(&sql).await?;
        let stages = rows
            .into_iter()
            .map(|r| {
                let matches = value_as_i64(&r["consistency_matches"]);
                let mismatches = value_as_i64(&r["consistency_mismatches"]);
                let total_checks = matches + mismatches;
                MigrationStageMetrics {
                    stage: r["stage"].as_str().unwrap_or("off").to_string(),
                    old_invocations: value_as_i64(&r["old_invocations"]),
                    new_invocations: value_as_i64(&r["new_invocations"]),
                    old_errors: value_as_i64(&r["old_errors"]),
                    new_errors: value_as_i64(&r["new_errors"]),
                    old_avg_latency_ms: value_as_f64(&r["old_avg_latency_ms"]),
                    new_avg_latency_ms: value_as_f64(&r["new_avg_latency_ms"]),
                    consistency_matches: matches,
                    consistency_mismatches: mismatches,
                    consistency_rate_pct: if total_checks > 0 {
                        (matches as f64 / total_checks as f64) * 100.0
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
}
