//! Phase 4 Storage Repository Methods:
//! Metrics, Metric Groups (Funnels), Mutual Exclusion Layers, Experiments,
//! Guarded Rollouts, Engineering Insights, 2/4/6-Stage Migration Flags,
//! Synced Segments, and Real-Time Data Export Sinks.

use chrono::{Duration, Utc};
use serde_json::{json, Value};
use skotos_domain::{
    validate_key, DataExportSink, DataExportSinkKind, DomainError, EngineeringInsights, Experiment,
    ExperimentMode, ExperimentStatus, ExperimentVariationWeight, GuardedRollout,
    GuardedRolloutStatus, GuardrailMetricRule, MetricAggregation, MetricDefinition, MetricGroup,
    MetricGroupKind, MetricKind, MetricSuccessDirection, MigrationFlagConfig, MigrationStage,
    MigrationTemplate, MutualExclusionLayer, StatisticalEngine, SyncedSegmentIntegration,
    SyncedSegmentProvider,
};
use sqlx::{postgres::PgRow, Row};
use uuid::Uuid;

use crate::error::StorageError;
use crate::keys::{generate_prefixed_secret, generate_salt, sha256_hex};
use crate::repo::{insert_audit_log, map_unique_err, AuditActor, Storage};

#[derive(Debug, Clone)]
pub struct CreatedSyncedSegmentOutcome {
    pub integration: SyncedSegmentIntegration,
    pub plaintext_token: String,
    pub ingest_path: String,
}

#[derive(Debug, Clone)]
pub struct ActiveGuardedRolloutContext {
    pub rollout: GuardedRollout,
    pub project_key: String,
    pub env_key: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedSyncedSegment {
    pub integration: SyncedSegmentIntegration,
    pub project_key: String,
    pub env_key: String,
}

impl Storage {
    // =========================================================================
    // Real-Time Data Export Sinks
    // =========================================================================

    pub async fn list_data_export_sinks(
        &self,
        org_id: Uuid,
    ) -> Result<Vec<DataExportSink>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, org_id, env_id, name, kind, destination, enabled, config_jsonb,
                    last_exported_at, exported_event_count, created_at, updated_at
             FROM data_export_sinks
             WHERE org_id = $1
             ORDER BY created_at DESC",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(parse_sink_row).collect())
    }

    pub async fn get_data_export_sink(
        &self,
        org_id: Uuid,
        sink_id: Uuid,
    ) -> Result<DataExportSink, StorageError> {
        let row = sqlx::query(
            "SELECT id, org_id, env_id, name, kind, destination, enabled, config_jsonb,
                    last_exported_at, exported_event_count, created_at, updated_at
             FROM data_export_sinks
             WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(sink_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound("data export sink".to_string()))?;
        Ok(parse_sink_row(&row))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_data_export_sink(
        &self,
        org_id: Uuid,
        env_id: Option<Uuid>,
        name: &str,
        kind: DataExportSinkKind,
        destination: &str,
        config: Value,
        actor: &AuditActor,
    ) -> Result<DataExportSink, StorageError> {
        if name.trim().is_empty() || destination.trim().is_empty() {
            return Err(StorageError::Validation(DomainError::Validation(
                "data export sink name and destination are required".to_string(),
            )));
        }
        let id = Uuid::now_v7();
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO data_export_sinks (id, org_id, env_id, name, kind, destination, config_jsonb)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING id, org_id, env_id, name, kind, destination, enabled, config_jsonb,
                       last_exported_at, exported_event_count, created_at, updated_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(env_id)
        .bind(name.trim())
        .bind(kind.as_str())
        .bind(destination.trim())
        .bind(config)
        .fetch_one(&mut *tx)
        .await?;

        let sink = parse_sink_row(&row);
        insert_audit_log(
            &mut tx,
            org_id,
            None,
            env_id,
            "data_export_sink",
            &sink.id.to_string(),
            "data_export_sink.created",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&sink).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(sink)
    }

    pub async fn record_data_export_flush(
        &self,
        org_id: Uuid,
        sink_id: Uuid,
        events_exported: i64,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE data_export_sinks
             SET exported_event_count = exported_event_count + $3,
                 last_exported_at = now(),
                 updated_at = now()
             WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(sink_id)
        .bind(events_exported)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_data_export_sink(
        &self,
        org_id: Uuid,
        sink_id: Uuid,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let res = sqlx::query("DELETE FROM data_export_sinks WHERE org_id = $1 AND id = $2")
            .bind(org_id)
            .bind(sink_id)
            .execute(&mut *tx)
            .await?;
        if res.rows_affected() == 0 {
            return Err(StorageError::NotFound("data export sink".to_string()));
        }
        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "data_export_sink",
            &sink_id.to_string(),
            "data_export_sink.deleted",
            actor,
            None,
            None,
            None,
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Loads recent raw exposure + custom events for Data Export Sink flushing.
    pub async fn load_recent_export_events(
        &self,
        org_id: Uuid,
        env_id: Option<Uuid>,
        limit: i64,
    ) -> Result<Vec<Value>, StorageError> {
        let mut events = Vec::new();
        let exp_rows = sqlx::query(
            "SELECT env_id, flag_key, variation_index, context_kind, context_key, exposed_at
             FROM experiment_exposures
             WHERE org_id = $1 AND ($2::UUID IS NULL OR env_id = $2)
             ORDER BY exposed_at DESC
             LIMIT $3",
        )
        .bind(org_id)
        .bind(env_id)
        .bind(limit.clamp(1, 5_000))
        .fetch_all(&self.pool)
        .await?;
        for r in exp_rows {
            events.push(json!({
                "kind": "exposure",
                "envId": r.get::<Uuid, _>("env_id"),
                "flagKey": r.get::<String, _>("flag_key"),
                "variationIndex": r.get::<i32, _>("variation_index"),
                "contextKind": r.get::<String, _>("context_kind"),
                "contextKey": r.get::<String, _>("context_key"),
                "timestamp": r.get::<chrono::DateTime<Utc>, _>("exposed_at"),
            }));
        }
        let custom_rows = sqlx::query(
            "SELECT env_id, event_key, context_kind, context_key, metric_value, occurred_at
             FROM custom_metric_events
             WHERE org_id = $1 AND ($2::UUID IS NULL OR env_id = $2)
             ORDER BY occurred_at DESC
             LIMIT $3",
        )
        .bind(org_id)
        .bind(env_id)
        .bind(limit.clamp(1, 5_000))
        .fetch_all(&self.pool)
        .await?;
        for r in custom_rows {
            events.push(json!({
                "kind": "custom",
                "envId": r.get::<Uuid, _>("env_id"),
                "eventKey": r.get::<String, _>("event_key"),
                "contextKind": r.get::<String, _>("context_kind"),
                "contextKey": r.get::<String, _>("context_key"),
                "metricValue": r.get::<f64, _>("metric_value"),
                "timestamp": r.get::<chrono::DateTime<Utc>, _>("occurred_at"),
            }));
        }
        Ok(events)
    }

    // =========================================================================
    // Metrics & Metric Groups (Funnels)
    // =========================================================================

    pub async fn list_metrics(
        &self,
        org_id: Uuid,
        project_key: &str,
    ) -> Result<Vec<MetricDefinition>, StorageError> {
        let project = self.get_project(org_id, project_key).await?;
        let rows = sqlx::query(
            "SELECT id, org_id, project_id, key, name, description, kind, event_key,
                    success_direction, aggregation, unit, randomization_unit, created_at, updated_at
             FROM metrics
             WHERE org_id = $1 AND project_id = $2
             ORDER BY created_at DESC",
        )
        .bind(org_id)
        .bind(project.id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(parse_metric_row).collect())
    }

    pub async fn get_metric(
        &self,
        org_id: Uuid,
        project_id: Uuid,
        metric_key: &str,
    ) -> Result<MetricDefinition, StorageError> {
        let row = sqlx::query(
            "SELECT id, org_id, project_id, key, name, description, kind, event_key,
                    success_direction, aggregation, unit, randomization_unit, created_at, updated_at
             FROM metrics
             WHERE org_id = $1 AND project_id = $2 AND key = $3",
        )
        .bind(org_id)
        .bind(project_id)
        .bind(metric_key)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("metric '{metric_key}'")))?;
        Ok(parse_metric_row(&row))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_metric(
        &self,
        org_id: Uuid,
        project_key: &str,
        key: &str,
        name: &str,
        description: &str,
        kind: MetricKind,
        event_key: &str,
        success_direction: MetricSuccessDirection,
        aggregation: MetricAggregation,
        unit: &str,
        randomization_unit: &str,
        actor: &AuditActor,
    ) -> Result<MetricDefinition, StorageError> {
        validate_key(key)?;
        if event_key.trim().is_empty() {
            return Err(StorageError::Validation(DomainError::Validation(
                "metric event_key cannot be empty".to_string(),
            )));
        }
        let project = self.get_project(org_id, project_key).await?;
        let id = Uuid::now_v7();
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO metrics (
                id, org_id, project_id, key, name, description, kind, event_key,
                success_direction, aggregation, unit, randomization_unit
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             RETURNING id, org_id, project_id, key, name, description, kind, event_key,
                       success_direction, aggregation, unit, randomization_unit, created_at, updated_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(project.id)
        .bind(key)
        .bind(name.trim())
        .bind(description.trim())
        .bind(kind.as_str())
        .bind(event_key.trim())
        .bind(success_direction.as_str())
        .bind(aggregation.as_str())
        .bind(unit.trim())
        .bind(if randomization_unit.trim().is_empty() {
            "user"
        } else {
            randomization_unit.trim()
        })
        .fetch_one(&mut *tx)
        .await
        .map_err(map_unique_err("metric"))?;

        let metric = parse_metric_row(&row);
        insert_audit_log(
            &mut tx,
            org_id,
            Some(project.id),
            None,
            "metric",
            key,
            "metric.created",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&metric).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(metric)
    }

    pub async fn delete_metric(
        &self,
        org_id: Uuid,
        project_key: &str,
        metric_key: &str,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let project = self.get_project(org_id, project_key).await?;
        let mut tx = self.pool.begin().await?;
        let res =
            sqlx::query("DELETE FROM metrics WHERE org_id = $1 AND project_id = $2 AND key = $3")
                .bind(org_id)
                .bind(project.id)
                .bind(metric_key)
                .execute(&mut *tx)
                .await?;
        if res.rows_affected() == 0 {
            return Err(StorageError::NotFound(format!("metric '{metric_key}'")));
        }
        insert_audit_log(
            &mut tx,
            org_id,
            Some(project.id),
            None,
            "metric",
            metric_key,
            "metric.deleted",
            actor,
            None,
            None,
            None,
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_metric_groups(
        &self,
        org_id: Uuid,
        project_key: &str,
    ) -> Result<Vec<MetricGroup>, StorageError> {
        let project = self.get_project(org_id, project_key).await?;
        let rows = sqlx::query(
            "SELECT id, org_id, project_id, key, name, kind, description, metric_keys_json, created_at, updated_at
             FROM metric_groups
             WHERE org_id = $1 AND project_id = $2
             ORDER BY created_at DESC",
        )
        .bind(org_id)
        .bind(project.id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(parse_metric_group_row).collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_metric_group(
        &self,
        org_id: Uuid,
        project_key: &str,
        key: &str,
        name: &str,
        kind: MetricGroupKind,
        description: &str,
        metric_keys: &[String],
        actor: &AuditActor,
    ) -> Result<MetricGroup, StorageError> {
        validate_key(key)?;
        if metric_keys.is_empty() {
            return Err(StorageError::Validation(DomainError::Validation(
                "metric group requires at least one metric key".to_string(),
            )));
        }
        let project = self.get_project(org_id, project_key).await?;
        let id = Uuid::now_v7();
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO metric_groups (id, org_id, project_id, key, name, kind, description, metric_keys_json)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING id, org_id, project_id, key, name, kind, description, metric_keys_json, created_at, updated_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(project.id)
        .bind(key)
        .bind(name.trim())
        .bind(kind.as_str())
        .bind(description.trim())
        .bind(serde_json::to_value(metric_keys).unwrap_or_else(|_| json!([])))
        .fetch_one(&mut *tx)
        .await
        .map_err(map_unique_err("metric group"))?;

        let group = parse_metric_group_row(&row);
        insert_audit_log(
            &mut tx,
            org_id,
            Some(project.id),
            None,
            "metric_group",
            key,
            "metric_group.created",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&group).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(group)
    }

    pub async fn delete_metric_group(
        &self,
        org_id: Uuid,
        project_key: &str,
        group_key: &str,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let project = self.get_project(org_id, project_key).await?;
        let mut tx = self.pool.begin().await?;
        let res = sqlx::query(
            "DELETE FROM metric_groups WHERE org_id = $1 AND project_id = $2 AND key = $3",
        )
        .bind(org_id)
        .bind(project.id)
        .bind(group_key)
        .execute(&mut *tx)
        .await?;
        if res.rows_affected() == 0 {
            return Err(StorageError::NotFound(format!(
                "metric group '{group_key}'"
            )));
        }
        insert_audit_log(
            &mut tx,
            org_id,
            Some(project.id),
            None,
            "metric_group",
            group_key,
            "metric_group.deleted",
            actor,
            None,
            None,
            None,
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    // =========================================================================
    // Mutual Exclusion Layers
    // =========================================================================

    pub async fn list_layers(
        &self,
        org_id: Uuid,
        project_key: &str,
    ) -> Result<Vec<MutualExclusionLayer>, StorageError> {
        let project = self.get_project(org_id, project_key).await?;
        let rows = sqlx::query(
            "SELECT id, org_id, project_id, key, name, description, salt, total_buckets, allocated_buckets, created_at, updated_at
             FROM mutual_exclusion_layers
             WHERE org_id = $1 AND project_id = $2
             ORDER BY created_at DESC",
        )
        .bind(org_id)
        .bind(project.id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(parse_layer_row).collect())
    }

    pub async fn create_layer(
        &self,
        org_id: Uuid,
        project_key: &str,
        key: &str,
        name: &str,
        description: &str,
        actor: &AuditActor,
    ) -> Result<MutualExclusionLayer, StorageError> {
        validate_key(key)?;
        let project = self.get_project(org_id, project_key).await?;
        let id = Uuid::now_v7();
        let salt = generate_salt();
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO mutual_exclusion_layers (id, org_id, project_id, key, name, description, salt)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING id, org_id, project_id, key, name, description, salt, total_buckets, allocated_buckets, created_at, updated_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(project.id)
        .bind(key)
        .bind(name.trim())
        .bind(description.trim())
        .bind(&salt)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_unique_err("mutual exclusion layer"))?;

        let layer = parse_layer_row(&row);
        insert_audit_log(
            &mut tx,
            org_id,
            Some(project.id),
            None,
            "layer",
            key,
            "layer.created",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&layer).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(layer)
    }

    pub async fn delete_layer(
        &self,
        org_id: Uuid,
        project_key: &str,
        layer_key: &str,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let project = self.get_project(org_id, project_key).await?;
        let mut tx = self.pool.begin().await?;
        let res = sqlx::query(
            "DELETE FROM mutual_exclusion_layers WHERE org_id = $1 AND project_id = $2 AND key = $3",
        )
        .bind(org_id)
        .bind(project.id)
        .bind(layer_key)
        .execute(&mut *tx)
        .await?;
        if res.rows_affected() == 0 {
            return Err(StorageError::NotFound(format!("layer '{layer_key}'")));
        }
        insert_audit_log(
            &mut tx,
            org_id,
            Some(project.id),
            None,
            "layer",
            layer_key,
            "layer.deleted",
            actor,
            None,
            None,
            None,
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    // =========================================================================
    // Experiments
    // =========================================================================

    pub async fn list_experiments(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
    ) -> Result<Vec<Experiment>, StorageError> {
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let rows = sqlx::query(
            "SELECT id, org_id, project_id, env_id, key, name, hypothesis, flag_key,
                    mode, engine, status, randomization_unit, control_variation_index,
                    primary_metric_key, secondary_metric_keys, metric_group_key,
                    layer_key, layer_bucket_start, layer_bucket_end, allocations_jsonb,
                    alpha, started_at, stopped_at, created_at, updated_at
             FROM experiments
             WHERE org_id = $1 AND env_id = $2
             ORDER BY created_at DESC",
        )
        .bind(org_id)
        .bind(env.id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(parse_experiment_row).collect())
    }

    pub async fn get_experiment(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        exp_key: &str,
    ) -> Result<Experiment, StorageError> {
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let row = sqlx::query(
            "SELECT id, org_id, project_id, env_id, key, name, hypothesis, flag_key,
                    mode, engine, status, randomization_unit, control_variation_index,
                    primary_metric_key, secondary_metric_keys, metric_group_key,
                    layer_key, layer_bucket_start, layer_bucket_end, allocations_jsonb,
                    alpha, started_at, stopped_at, created_at, updated_at
             FROM experiments
             WHERE org_id = $1 AND env_id = $2 AND key = $3",
        )
        .bind(org_id)
        .bind(env.id)
        .bind(exp_key)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("experiment '{exp_key}'")))?;
        Ok(parse_experiment_row(&row))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_experiment(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        key: &str,
        name: &str,
        hypothesis: &str,
        flag_key: &str,
        mode: ExperimentMode,
        engine: StatisticalEngine,
        randomization_unit: &str,
        control_variation_index: i32,
        primary_metric_key: &str,
        secondary_metric_keys: &[String],
        metric_group_key: Option<&str>,
        layer_key: Option<&str>,
        layer_reservation_buckets: Option<i32>,
        allocations: &[ExperimentVariationWeight],
        alpha: f64,
        actor: &AuditActor,
    ) -> Result<Experiment, StorageError> {
        validate_key(key)?;
        let env = self.get_environment(org_id, project_key, env_key).await?;
        // Verify referenced flag & primary metric exist
        let _ = self.get_flag(org_id, project_key, flag_key).await?;
        let _ = self
            .get_metric(org_id, env.project_id, primary_metric_key)
            .await?;

        let mut tx = self.pool.begin().await?;
        let (layer_start, layer_end) = if let Some(lk) = layer_key {
            let reserve = layer_reservation_buckets
                .unwrap_or(20_000)
                .clamp(1, 100_000);
            let layer_row = sqlx::query(
                "SELECT id, total_buckets, allocated_buckets
                 FROM mutual_exclusion_layers
                 WHERE org_id = $1 AND project_id = $2 AND key = $3
                 FOR UPDATE",
            )
            .bind(org_id)
            .bind(env.project_id)
            .bind(lk)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| StorageError::NotFound(format!("layer '{lk}'")))?;
            let layer_id: Uuid = layer_row.get("id");
            let total: i32 = layer_row.get("total_buckets");
            let used: i32 = layer_row.get("allocated_buckets");
            if used + reserve > total {
                return Err(StorageError::Validation(DomainError::Validation(format!(
                    "mutual exclusion layer '{lk}' has only {} remaining buckets (requested {reserve})",
                    total - used
                ))));
            }
            sqlx::query(
                "UPDATE mutual_exclusion_layers
                 SET allocated_buckets = allocated_buckets + $2, updated_at = now()
                 WHERE id = $1",
            )
            .bind(layer_id)
            .bind(reserve)
            .execute(&mut *tx)
            .await?;
            (Some(used), Some(used + reserve))
        } else {
            (None, None)
        };

        let resolved_allocs = if allocations.is_empty() {
            vec![
                ExperimentVariationWeight {
                    variation_index: 0,
                    weight: 50_000,
                },
                ExperimentVariationWeight {
                    variation_index: 1,
                    weight: 50_000,
                },
            ]
        } else {
            allocations.to_vec()
        };

        let id = Uuid::now_v7();
        let row = sqlx::query(
            "INSERT INTO experiments (
                id, org_id, project_id, env_id, key, name, hypothesis, flag_key,
                mode, engine, status, randomization_unit, control_variation_index,
                primary_metric_key, secondary_metric_keys, metric_group_key,
                layer_key, layer_bucket_start, layer_bucket_end, allocations_jsonb, alpha
             )
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'draft',$11,$12,$13,$14,$15,$16,$17,$18,$19,$20)
             RETURNING id, org_id, project_id, env_id, key, name, hypothesis, flag_key,
                       mode, engine, status, randomization_unit, control_variation_index,
                       primary_metric_key, secondary_metric_keys, metric_group_key,
                       layer_key, layer_bucket_start, layer_bucket_end, allocations_jsonb,
                       alpha, started_at, stopped_at, created_at, updated_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(env.project_id)
        .bind(env.id)
        .bind(key)
        .bind(name.trim())
        .bind(hypothesis.trim())
        .bind(flag_key)
        .bind(mode.as_str())
        .bind(engine.as_str())
        .bind(if randomization_unit.trim().is_empty() {
            "user"
        } else {
            randomization_unit.trim()
        })
        .bind(control_variation_index)
        .bind(primary_metric_key)
        .bind(serde_json::to_value(secondary_metric_keys).unwrap_or_else(|_| json!([])))
        .bind(metric_group_key)
        .bind(layer_key)
        .bind(layer_start)
        .bind(layer_end)
        .bind(serde_json::to_value(&resolved_allocs).unwrap_or_else(|_| json!([])))
        .bind(alpha.clamp(0.001, 0.25))
        .fetch_one(&mut *tx)
        .await
        .map_err(map_unique_err("experiment"))?;

        let exp = parse_experiment_row(&row);
        insert_audit_log(
            &mut tx,
            org_id,
            Some(env.project_id),
            Some(env.id),
            "experiment",
            key,
            "experiment.created",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&exp).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(exp)
    }

    pub async fn transition_experiment_status(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        exp_key: &str,
        next_status: ExperimentStatus,
        actor: &AuditActor,
    ) -> Result<Experiment, StorageError> {
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "UPDATE experiments
             SET status = $4,
                 started_at = CASE WHEN $4 = 'running' AND started_at IS NULL THEN now() ELSE started_at END,
                 stopped_at = CASE WHEN $4 IN ('stopped', 'archived') THEN now() ELSE stopped_at END,
                 updated_at = now()
             WHERE org_id = $1 AND env_id = $2 AND key = $3
             RETURNING id, org_id, project_id, env_id, key, name, hypothesis, flag_key,
                       mode, engine, status, randomization_unit, control_variation_index,
                       primary_metric_key, secondary_metric_keys, metric_group_key,
                       layer_key, layer_bucket_start, layer_bucket_end, allocations_jsonb,
                       alpha, started_at, stopped_at, created_at, updated_at",
        )
        .bind(org_id)
        .bind(env.id)
        .bind(exp_key)
        .bind(next_status.as_str())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("experiment '{exp_key}'")))?;

        let exp = parse_experiment_row(&row);
        insert_audit_log(
            &mut tx,
            org_id,
            Some(env.project_id),
            Some(env.id),
            "experiment",
            exp_key,
            &format!("experiment.{}", next_status.as_str()),
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&exp).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(exp)
    }

    pub async fn delete_experiment(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        exp_key: &str,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let mut tx = self.pool.begin().await?;
        let res =
            sqlx::query("DELETE FROM experiments WHERE org_id = $1 AND env_id = $2 AND key = $3")
                .bind(org_id)
                .bind(env.id)
                .bind(exp_key)
                .execute(&mut *tx)
                .await?;
        if res.rows_affected() == 0 {
            return Err(StorageError::NotFound(format!("experiment '{exp_key}'")));
        }
        insert_audit_log(
            &mut tx,
            org_id,
            Some(env.project_id),
            Some(env.id),
            "experiment",
            exp_key,
            "experiment.deleted",
            actor,
            None,
            None,
            None,
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    // =========================================================================
    // Guarded Rollouts
    // =========================================================================

    pub async fn list_guarded_rollouts(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
    ) -> Result<Vec<GuardedRollout>, StorageError> {
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let rows = sqlx::query(
            "SELECT id, org_id, project_id, env_id, flag_key, name, status,
                    control_variation_index, treatment_variation_index, current_traffic_pct,
                    auto_rollback, guardrails_jsonb, last_evaluated_at, last_verdict,
                    created_at, updated_at
             FROM guarded_rollouts
             WHERE org_id = $1 AND env_id = $2
             ORDER BY created_at DESC",
        )
        .bind(org_id)
        .bind(env.id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(parse_guarded_rollout_row).collect())
    }

    pub async fn get_guarded_rollout(
        &self,
        org_id: Uuid,
        rollout_id: Uuid,
    ) -> Result<GuardedRollout, StorageError> {
        let row = sqlx::query(
            "SELECT id, org_id, project_id, env_id, flag_key, name, status,
                    control_variation_index, treatment_variation_index, current_traffic_pct,
                    auto_rollback, guardrails_jsonb, last_evaluated_at, last_verdict,
                    created_at, updated_at
             FROM guarded_rollouts
             WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(rollout_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound("guarded rollout".to_string()))?;
        Ok(parse_guarded_rollout_row(&row))
    }

    pub async fn list_monitoring_guarded_rollouts(
        &self,
        limit: i64,
    ) -> Result<Vec<ActiveGuardedRolloutContext>, StorageError> {
        let rows = sqlx::query(
            "SELECT gr.id, gr.org_id, gr.project_id, gr.env_id, gr.flag_key, gr.name, gr.status,
                    gr.control_variation_index, gr.treatment_variation_index, gr.current_traffic_pct,
                    gr.auto_rollback, gr.guardrails_jsonb, gr.last_evaluated_at, gr.last_verdict,
                    gr.created_at, gr.updated_at,
                    p.key AS project_key, e.key AS env_key
             FROM guarded_rollouts gr
             JOIN projects p ON p.id = gr.project_id
             JOIN environments e ON e.id = gr.env_id
             WHERE gr.status = 'monitoring'
             ORDER BY gr.updated_at ASC
             LIMIT $1",
        )
        .bind(limit.clamp(1, 100))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| ActiveGuardedRolloutContext {
                rollout: parse_guarded_rollout_row(r),
                project_key: r.get("project_key"),
                env_key: r.get("env_key"),
            })
            .collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_guarded_rollout(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        flag_key: &str,
        name: &str,
        control_variation_index: i32,
        treatment_variation_index: i32,
        current_traffic_pct: i32,
        auto_rollback: bool,
        guardrails: &[GuardrailMetricRule],
        actor: &AuditActor,
    ) -> Result<GuardedRollout, StorageError> {
        if guardrails.is_empty() {
            return Err(StorageError::Validation(DomainError::Validation(
                "guarded rollout requires at least one guardrail metric rule".to_string(),
            )));
        }
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let _ = self.get_flag(org_id, project_key, flag_key).await?;
        for g in guardrails {
            let _ = self
                .get_metric(org_id, env.project_id, &g.metric_key)
                .await?;
        }

        let id = Uuid::now_v7();
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO guarded_rollouts (
                id, org_id, project_id, env_id, flag_key, name, status,
                control_variation_index, treatment_variation_index, current_traffic_pct,
                auto_rollback, guardrails_jsonb
             )
             VALUES ($1,$2,$3,$4,$5,$6,'monitoring',$7,$8,$9,$10,$11)
             RETURNING id, org_id, project_id, env_id, flag_key, name, status,
                       control_variation_index, treatment_variation_index, current_traffic_pct,
                       auto_rollback, guardrails_jsonb, last_evaluated_at, last_verdict,
                       created_at, updated_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(env.project_id)
        .bind(env.id)
        .bind(flag_key)
        .bind(name.trim())
        .bind(control_variation_index)
        .bind(treatment_variation_index)
        .bind(current_traffic_pct.clamp(1, 100))
        .bind(auto_rollback)
        .bind(serde_json::to_value(guardrails).unwrap_or_else(|_| json!([])))
        .fetch_one(&mut *tx)
        .await?;

        let rollout = parse_guarded_rollout_row(&row);
        insert_audit_log(
            &mut tx,
            org_id,
            Some(env.project_id),
            Some(env.id),
            "guarded_rollout",
            flag_key,
            "guarded_rollout.created",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&rollout).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(rollout)
    }

    pub async fn update_guarded_rollout_verdict(
        &self,
        org_id: Uuid,
        rollout_id: Uuid,
        status: GuardedRolloutStatus,
        verdict: &str,
    ) -> Result<GuardedRollout, StorageError> {
        let row = sqlx::query(
            "UPDATE guarded_rollouts
             SET status = $3,
                 last_evaluated_at = now(),
                 last_verdict = $4,
                 updated_at = now()
             WHERE org_id = $1 AND id = $2
             RETURNING id, org_id, project_id, env_id, flag_key, name, status,
                       control_variation_index, treatment_variation_index, current_traffic_pct,
                       auto_rollback, guardrails_jsonb, last_evaluated_at, last_verdict,
                       created_at, updated_at",
        )
        .bind(org_id)
        .bind(rollout_id)
        .bind(status.as_str())
        .bind(verdict)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound("guarded rollout".to_string()))?;
        Ok(parse_guarded_rollout_row(&row))
    }

    // =========================================================================
    // Engineering Insights (DORA Metrics + Flag Health)
    // =========================================================================

    pub async fn compute_engineering_insights(
        &self,
        org_id: Uuid,
        project_key: &str,
        lookback_days: i64,
    ) -> Result<EngineeringInsights, StorageError> {
        let days = lookback_days.clamp(1, 365);
        let project = self.get_project(org_id, project_key).await?;
        let since = Utc::now() - Duration::days(days);

        let deployments: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::BIGINT FROM audit_logs
             WHERE org_id = $1 AND project_id = $2
               AND (
                    (resource_type = 'flag' AND action IN ('create', 'patch', 'restore', 'copy'))
                    OR action IN ('flag.updated', 'flag.created', 'flag.restored')
               )
               AND created_at >= $3",
        )
        .bind(org_id)
        .bind(project.id)
        .bind(since)
        .fetch_one(&self.pool)
        .await?;

        let rollbacks: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::BIGINT FROM audit_logs
             WHERE org_id = $1 AND project_id = $2
               AND (
                    (resource_type = 'flag' AND action = 'restore')
                    OR action IN ('flag.restored', 'trigger.executed', 'guarded_rollout.rolled_back')
                    OR comment ILIKE '%rollback%'
               )
               AND created_at >= $3",
        )
        .bind(org_id)
        .bind(project.id)
        .bind(since)
        .fetch_one(&self.pool)
        .await?;

        let total_flags: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::BIGINT FROM flags WHERE org_id = $1 AND project_id = $2 AND archived = false",
        )
        .bind(org_id)
        .bind(project.id)
        .fetch_one(&self.pool)
        .await?;

        let temporary_flags: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::BIGINT FROM flags
             WHERE org_id = $1 AND project_id = $2 AND archived = false AND temporary = true",
        )
        .bind(org_id)
        .bind(project.id)
        .fetch_one(&self.pool)
        .await?;

        let stale_cutoff = Utc::now() - Duration::days(30);
        let stale_flags: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::BIGINT FROM flags
             WHERE org_id = $1 AND project_id = $2 AND archived = false
               AND temporary = true AND updated_at < $3",
        )
        .bind(org_id)
        .bind(project.id)
        .bind(stale_cutoff)
        .fetch_one(&self.pool)
        .await?;

        let active_experiments: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::BIGINT FROM experiments
             WHERE org_id = $1 AND project_id = $2 AND status = 'running'",
        )
        .bind(org_id)
        .bind(project.id)
        .fetch_one(&self.pool)
        .await?;

        let active_guarded_rollouts: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::BIGINT FROM guarded_rollouts
             WHERE org_id = $1 AND project_id = $2 AND status = 'monitoring'",
        )
        .bind(org_id)
        .bind(project.id)
        .fetch_one(&self.pool)
        .await?;

        let rolled_back_guardrails: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::BIGINT FROM guarded_rollouts
             WHERE org_id = $1 AND project_id = $2 AND status = 'rolled_back' AND updated_at >= $3",
        )
        .bind(org_id)
        .bind(project.id)
        .bind(since)
        .fetch_one(&self.pool)
        .await?;

        let cfr = if deployments > 0 {
            ((rollbacks + rolled_back_guardrails) as f64 / deployments as f64) * 100.0
        } else {
            0.0
        };
        // Fast flag kill-switch / guardrail MTTR (~1.5 minutes when automated rollbacks active)
        let mttr_minutes = if rollbacks + rolled_back_guardrails > 0 {
            1.5
        } else {
            0.0
        };

        Ok(EngineeringInsights {
            project_key: project_key.to_string(),
            lookback_days: days,
            flag_deployments_count: deployments,
            deployments_per_day: deployments as f64 / days as f64,
            rollbacks_count: rollbacks + rolled_back_guardrails,
            change_failure_rate_pct: cfr,
            mean_time_to_recovery_minutes: mttr_minutes,
            total_flags,
            temporary_flags,
            stale_flags,
            active_experiments,
            active_guarded_rollouts,
            rolled_back_guardrails,
            computed_at: Utc::now(),
        })
    }

    // =========================================================================
    // 2/4/6-Stage Migration Flags
    // =========================================================================

    pub async fn get_migration_config(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        flag_key: &str,
    ) -> Result<Option<MigrationFlagConfig>, StorageError> {
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let row = sqlx::query(
            "SELECT id, org_id, env_id, flag_key, template, current_stage, check_ratio, read_execution_order, updated_at
             FROM flag_migration_configs
             WHERE org_id = $1 AND env_id = $2 AND flag_key = $3",
        )
        .bind(org_id)
        .bind(env.id)
        .bind(flag_key)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(parse_migration_config_row))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_migration_config(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        flag_key: &str,
        template: MigrationTemplate,
        current_stage: MigrationStage,
        check_ratio: i32,
        read_execution_order: &str,
        actor: &AuditActor,
    ) -> Result<MigrationFlagConfig, StorageError> {
        if !template.allowed_stages().contains(&current_stage) {
            return Err(StorageError::Validation(DomainError::Validation(format!(
                "stage '{}' is not valid for template '{}'",
                current_stage.as_str(),
                template.as_str()
            ))));
        }
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let _ = self.get_flag(org_id, project_key, flag_key).await?;
        let order = match read_execution_order {
            "parallel" | "serial" | "random" => read_execution_order,
            _ => "parallel",
        };
        let id = Uuid::now_v7();
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO flag_migration_configs (
                id, org_id, env_id, flag_key, template, current_stage, check_ratio, read_execution_order
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (env_id, flag_key)
             DO UPDATE SET
                template = EXCLUDED.template,
                current_stage = EXCLUDED.current_stage,
                check_ratio = EXCLUDED.check_ratio,
                read_execution_order = EXCLUDED.read_execution_order,
                updated_at = now()
             RETURNING id, org_id, env_id, flag_key, template, current_stage, check_ratio, read_execution_order, updated_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(env.id)
        .bind(flag_key)
        .bind(template.as_str())
        .bind(current_stage.as_str())
        .bind(check_ratio.clamp(0, 10_000))
        .bind(order)
        .fetch_one(&mut *tx)
        .await?;

        let cfg = parse_migration_config_row(&row);
        insert_audit_log(
            &mut tx,
            org_id,
            Some(env.project_id),
            Some(env.id),
            "migration_flag",
            flag_key,
            "migration_flag.updated",
            actor,
            Some(&format!(
                "Migration stage set to {}",
                current_stage.as_str()
            )),
            None,
            None,
            Some(serde_json::to_value(&cfg).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(cfg)
    }

    // =========================================================================
    // Synced Segments (External Warehouse/CDP Integrations)
    // =========================================================================

    pub async fn list_synced_segments(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
    ) -> Result<Vec<SyncedSegmentIntegration>, StorageError> {
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let rows = sqlx::query(
            "SELECT id, org_id, env_id, segment_key, provider, external_cohort_id,
                    context_kind, token_prefix, enabled, last_synced_at, synced_member_count, created_at
             FROM synced_segment_integrations
             WHERE org_id = $1 AND env_id = $2
             ORDER BY created_at DESC",
        )
        .bind(org_id)
        .bind(env.id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(parse_synced_segment_row).collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_synced_segment(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        segment_key: &str,
        provider: SyncedSegmentProvider,
        external_cohort_id: &str,
        context_kind: &str,
        actor: &AuditActor,
    ) -> Result<CreatedSyncedSegmentOutcome, StorageError> {
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let _ = self
            .get_segment(org_id, project_key, env_key, segment_key)
            .await?;

        let secret = generate_prefixed_secret("sk_sync_");
        let id = Uuid::now_v7();
        let ck = if context_kind.trim().is_empty() {
            "user"
        } else {
            context_kind.trim()
        };

        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO synced_segment_integrations (
                id, org_id, env_id, segment_key, provider, external_cohort_id,
                context_kind, token_prefix, token_hash
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             RETURNING id, org_id, env_id, segment_key, provider, external_cohort_id,
                       context_kind, token_prefix, enabled, last_synced_at, synced_member_count, created_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(env.id)
        .bind(segment_key)
        .bind(provider.as_str())
        .bind(external_cohort_id.trim())
        .bind(ck)
        .bind(&secret.prefix)
        .bind(&secret.hash)
        .fetch_one(&mut *tx)
        .await?;

        let integration = parse_synced_segment_row(&row);
        insert_audit_log(
            &mut tx,
            org_id,
            Some(env.project_id),
            Some(env.id),
            "synced_segment",
            segment_key,
            "synced_segment.created",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&integration).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;

        let ingest_path = format!("/api/v1/synced-segments/ingest/{}", secret.plaintext);
        Ok(CreatedSyncedSegmentOutcome {
            integration,
            plaintext_token: secret.plaintext,
            ingest_path,
        })
    }

    pub async fn resolve_synced_segment_by_token(
        &self,
        raw_token: &str,
    ) -> Result<ResolvedSyncedSegment, StorageError> {
        let hash = sha256_hex(raw_token);
        let row = sqlx::query(
            "SELECT s.id, s.org_id, s.env_id, s.segment_key, s.provider, s.external_cohort_id,
                    s.context_kind, s.token_prefix, s.enabled, s.last_synced_at, s.synced_member_count, s.created_at,
                    p.key AS project_key, e.key AS env_key
             FROM synced_segment_integrations s
             JOIN environments e ON e.id = s.env_id
             JOIN projects p ON p.id = e.project_id
             WHERE s.token_hash = $1 AND s.enabled = true",
        )
        .bind(&hash)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound("synced segment token".to_string()))?;

        Ok(ResolvedSyncedSegment {
            integration: parse_synced_segment_row(&row),
            project_key: row.get("project_key"),
            env_key: row.get("env_key"),
        })
    }

    pub async fn mark_synced_segment_ingested(
        &self,
        integration_id: Uuid,
        member_count: i64,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE synced_segment_integrations
             SET last_synced_at = now(), synced_member_count = $2
             WHERE id = $1",
        )
        .bind(integration_id)
        .bind(member_count)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_synced_segment(
        &self,
        org_id: Uuid,
        integration_id: Uuid,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let res =
            sqlx::query("DELETE FROM synced_segment_integrations WHERE org_id = $1 AND id = $2")
                .bind(org_id)
                .bind(integration_id)
                .execute(&mut *tx)
                .await?;
        if res.rows_affected() == 0 {
            return Err(StorageError::NotFound(
                "synced segment integration".to_string(),
            ));
        }
        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "synced_segment",
            &integration_id.to_string(),
            "synced_segment.deleted",
            actor,
            None,
            None,
            None,
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

fn parse_sink_row(r: &PgRow) -> DataExportSink {
    let kind_str: String = r.get("kind");
    DataExportSink {
        id: r.get("id"),
        org_id: r.get("org_id"),
        env_id: r.get("env_id"),
        name: r.get("name"),
        kind: DataExportSinkKind::parse_str(&kind_str).unwrap_or(DataExportSinkKind::WebhookNdjson),
        destination: r.get("destination"),
        enabled: r.get("enabled"),
        config: r.get("config_jsonb"),
        last_exported_at: r.get("last_exported_at"),
        exported_event_count: r.get("exported_event_count"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }
}

fn parse_metric_row(r: &PgRow) -> MetricDefinition {
    let kind_s: String = r.get("kind");
    let dir_s: String = r.get("success_direction");
    let agg_s: String = r.get("aggregation");
    MetricDefinition {
        id: r.get("id"),
        org_id: r.get("org_id"),
        project_id: r.get("project_id"),
        key: r.get("key"),
        name: r.get("name"),
        description: r.get("description"),
        kind: MetricKind::parse_str(&kind_s).unwrap_or(MetricKind::Conversion),
        event_key: r.get("event_key"),
        success_direction: MetricSuccessDirection::parse_str(&dir_s)
            .unwrap_or(MetricSuccessDirection::HigherIsBetter),
        aggregation: MetricAggregation::parse_str(&agg_s).unwrap_or(MetricAggregation::Average),
        unit: r.get("unit"),
        randomization_unit: r.get("randomization_unit"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }
}

fn parse_metric_group_row(r: &PgRow) -> MetricGroup {
    let kind_s: String = r.get("kind");
    let keys_val: Value = r.get("metric_keys_json");
    let metric_keys: Vec<String> = serde_json::from_value(keys_val).unwrap_or_default();
    MetricGroup {
        id: r.get("id"),
        org_id: r.get("org_id"),
        project_id: r.get("project_id"),
        key: r.get("key"),
        name: r.get("name"),
        kind: MetricGroupKind::parse_str(&kind_s).unwrap_or(MetricGroupKind::Funnel),
        description: r.get("description"),
        metric_keys,
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }
}

fn parse_layer_row(r: &PgRow) -> MutualExclusionLayer {
    MutualExclusionLayer {
        id: r.get("id"),
        org_id: r.get("org_id"),
        project_id: r.get("project_id"),
        key: r.get("key"),
        name: r.get("name"),
        description: r.get("description"),
        salt: r.get("salt"),
        total_buckets: r.get("total_buckets"),
        allocated_buckets: r.get("allocated_buckets"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }
}

fn parse_experiment_row(r: &PgRow) -> Experiment {
    let mode_s: String = r.get("mode");
    let eng_s: String = r.get("engine");
    let status_s: String = r.get("status");
    let sec_val: Value = r.get("secondary_metric_keys");
    let alloc_val: Value = r.get("allocations_jsonb");
    Experiment {
        id: r.get("id"),
        org_id: r.get("org_id"),
        project_id: r.get("project_id"),
        env_id: r.get("env_id"),
        key: r.get("key"),
        name: r.get("name"),
        hypothesis: r.get("hypothesis"),
        flag_key: r.get("flag_key"),
        mode: ExperimentMode::parse_str(&mode_s).unwrap_or(ExperimentMode::AbTest),
        engine: StatisticalEngine::parse_str(&eng_s).unwrap_or(StatisticalEngine::Frequentist),
        status: ExperimentStatus::parse_str(&status_s).unwrap_or(ExperimentStatus::Draft),
        randomization_unit: r.get("randomization_unit"),
        control_variation_index: r.get("control_variation_index"),
        primary_metric_key: r.get("primary_metric_key"),
        secondary_metric_keys: serde_json::from_value(sec_val).unwrap_or_default(),
        metric_group_key: r.get("metric_group_key"),
        layer_key: r.get("layer_key"),
        layer_bucket_start: r.get("layer_bucket_start"),
        layer_bucket_end: r.get("layer_bucket_end"),
        allocations: serde_json::from_value(alloc_val).unwrap_or_default(),
        alpha: r.get("alpha"),
        started_at: r.get("started_at"),
        stopped_at: r.get("stopped_at"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }
}

fn parse_guarded_rollout_row(r: &PgRow) -> GuardedRollout {
    let status_s: String = r.get("status");
    let rules_val: Value = r.get("guardrails_jsonb");
    GuardedRollout {
        id: r.get("id"),
        org_id: r.get("org_id"),
        project_id: r.get("project_id"),
        env_id: r.get("env_id"),
        flag_key: r.get("flag_key"),
        name: r.get("name"),
        status: GuardedRolloutStatus::parse_str(&status_s)
            .unwrap_or(GuardedRolloutStatus::Monitoring),
        control_variation_index: r.get("control_variation_index"),
        treatment_variation_index: r.get("treatment_variation_index"),
        current_traffic_pct: r.get("current_traffic_pct"),
        auto_rollback: r.get("auto_rollback"),
        guardrails: serde_json::from_value(rules_val).unwrap_or_default(),
        last_evaluated_at: r.get("last_evaluated_at"),
        last_verdict: r.get("last_verdict"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }
}

fn parse_migration_config_row(r: &PgRow) -> MigrationFlagConfig {
    let tpl_s: String = r.get("template");
    let stg_s: String = r.get("current_stage");
    MigrationFlagConfig {
        id: r.get("id"),
        org_id: r.get("org_id"),
        env_id: r.get("env_id"),
        flag_key: r.get("flag_key"),
        template: MigrationTemplate::parse_str(&tpl_s).unwrap_or(MigrationTemplate::SixStage),
        current_stage: MigrationStage::parse_str(&stg_s).unwrap_or(MigrationStage::Off),
        check_ratio: r.get("check_ratio"),
        read_execution_order: r.get("read_execution_order"),
        updated_at: r.get("updated_at"),
    }
}

fn parse_synced_segment_row(r: &PgRow) -> SyncedSegmentIntegration {
    let prov_s: String = r.get("provider");
    SyncedSegmentIntegration {
        id: r.get("id"),
        org_id: r.get("org_id"),
        env_id: r.get("env_id"),
        segment_key: r.get("segment_key"),
        provider: SyncedSegmentProvider::parse_str(&prov_s)
            .unwrap_or(SyncedSegmentProvider::Webhook),
        external_cohort_id: r.get("external_cohort_id"),
        context_kind: r.get("context_kind"),
        token_prefix: r.get("token_prefix"),
        enabled: r.get("enabled"),
        last_synced_at: r.get("last_synced_at"),
        synced_member_count: r.get("synced_member_count"),
        created_at: r.get("created_at"),
    }
}
