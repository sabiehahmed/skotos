use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use skotos_domain::{
    apply_segment_patch, validate_key, validate_segment, DomainError, Environment,
    EnvironmentKeyKind, FeatureFlag, FlagEnvConfig, FlagEnvConfigBody, FlagPatchInstruction,
    OutboxEvent, PatchSummaryItem, ScheduledChange, ScheduledChangeStatus, Segment, SegmentKind,
    SegmentPatchInstruction, SegmentRule, SegmentTarget, TriggerAction, Webhook, WebhookDelivery,
};
use sqlx::Row;
use uuid::Uuid;

use crate::error::StorageError;
use crate::keys::{generate_prefixed_secret, generate_salt, sha256_hex};
use crate::repo::{
    insert_audit_log, insert_outbox_event, map_unique_err, parse_environment_row,
    parse_flag_env_config_row, parse_flag_row, parse_segment_row, AuditActor, PatchFlagOutcome,
    Storage,
};

#[derive(Debug, Clone)]
pub struct CreateSegmentInput {
    pub key: String,
    pub name: String,
    pub kind: SegmentKind,
    pub included: Vec<String>,
    pub excluded: Vec<String>,
    pub included_contexts: Vec<SegmentTarget>,
    pub excluded_contexts: Vec<SegmentTarget>,
    pub rules: Vec<SegmentRule>,
}

#[derive(Debug, Clone)]
pub struct PatchSegmentOutcome {
    pub segment: Segment,
    pub summaries: Vec<PatchSummaryItem>,
}

#[derive(Debug, Clone)]
pub struct CreatedTriggerOutcome {
    pub trigger: skotos_domain::FlagTrigger,
    pub plaintext_token: String,
}

#[derive(Debug, Clone)]
pub struct ExecutedTriggerOutcome {
    pub flag_key: String,
    pub env_key: String,
    pub action: TriggerAction,
    pub new_version: i64,
    pub on: bool,
}

#[derive(Debug, Clone)]
pub struct WebhookWithSecret {
    pub webhook: Webhook,
    pub secret: String,
}

#[derive(Debug, Clone)]
pub struct CreatedWebhookOutcome {
    pub webhook: Webhook,
    pub secret: String,
}

#[derive(Debug, Clone)]
pub struct ClaimedOutboxEvent {
    pub event: OutboxEvent,
    pub attempts: i32,
}

#[derive(Debug, Clone)]
pub struct DueScheduledChange {
    pub change: ScheduledChange,
    pub project_key: String,
    pub env_key: String,
    pub flag_key: String,
}

pub use skotos_domain::{EnvironmentAuthRecord, EnvironmentKeyHashRecord};

/// Compiled data bundle for an environment used by `skotos-fanout` to produce pre-compressed snapshots.
#[derive(Debug, Clone)]
pub struct EnvironmentCompileBundle {
    pub org_id: Uuid,
    pub environment: Environment,
    pub flags: Vec<(FeatureFlag, FlagEnvConfig)>,
    pub segments: Vec<Segment>,
}

impl Storage {
    // =========================================================================
    // Segments CRUD & Semantic Patching
    // =========================================================================

    pub async fn list_segments(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
    ) -> Result<Vec<Segment>, StorageError> {
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let rows = sqlx::query(
            "SELECT id, env_id, key, name, kind, version, salt, segment_jsonb, updated_at
             FROM segments
             WHERE org_id = $1 AND env_id = $2
             ORDER BY updated_at DESC, key ASC",
        )
        .bind(org_id)
        .bind(env.id)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(parse_segment_row).collect()
    }

    pub async fn get_segment(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        segment_key: &str,
    ) -> Result<Segment, StorageError> {
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let row = sqlx::query(
            "SELECT id, env_id, key, name, kind, version, salt, segment_jsonb, updated_at
             FROM segments
             WHERE org_id = $1 AND env_id = $2 AND key = $3",
        )
        .bind(org_id)
        .bind(env.id)
        .bind(segment_key)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("segment '{segment_key}'")))?;
        parse_segment_row(&row)
    }

    pub async fn create_segment(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        input: CreateSegmentInput,
        actor: &AuditActor,
    ) -> Result<Segment, StorageError> {
        validate_key(&input.key)?;
        if input.name.trim().is_empty() {
            return Err(StorageError::Validation(DomainError::Validation(
                "segment name cannot be empty".to_string(),
            )));
        }

        let project = self.get_project(org_id, project_key).await?;
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let now = Utc::now();
        let seg = Segment {
            id: Uuid::now_v7(),
            env_id: env.id,
            key: input.key.clone(),
            name: input.name.trim().to_string(),
            kind: input.kind,
            version: 1,
            salt: generate_salt(),
            included: input.included,
            excluded: input.excluded,
            included_contexts: input.included_contexts,
            excluded_contexts: input.excluded_contexts,
            rules: input.rules,
            updated_at: now,
        };
        validate_segment(&seg)?;

        let seg_body = json!({
            "included": seg.included,
            "excluded": seg.excluded,
            "includedContexts": seg.included_contexts,
            "excludedContexts": seg.excluded_contexts,
            "rules": seg.rules,
        });
        let kind_str = match seg.kind {
            SegmentKind::Standard => "standard",
            SegmentKind::Big => "big",
            SegmentKind::Synced => "synced",
        };

        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO segments (id, org_id, env_id, key, name, kind, version, salt, segment_jsonb, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, 1, $7, $8, $9)",
        )
        .bind(seg.id)
        .bind(org_id)
        .bind(env.id)
        .bind(&seg.key)
        .bind(&seg.name)
        .bind(kind_str)
        .bind(&seg.salt)
        .bind(&seg_body)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(map_unique_err("segment"))?;

        let new_env_rev: i64 = sqlx::query_scalar(
            "UPDATE environments SET revision = revision + 1 WHERE org_id = $1 AND id = $2 RETURNING revision",
        )
        .bind(org_id)
        .bind(env.id)
        .fetch_one(&mut *tx)
        .await?;

        insert_audit_log(
            &mut tx,
            org_id,
            Some(project.id),
            Some(env.id),
            "segment",
            &seg.key,
            "create",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&seg).unwrap_or(Value::Null)),
        )
        .await?;

        insert_outbox_event(
            &mut tx,
            org_id,
            Some(env.id),
            "segment.created",
            json!({
                "segmentId": seg.id,
                "segmentKey": seg.key,
                "envId": env.id,
                "version": seg.version,
                "envRevision": new_env_rev,
                "clientSideAffected": true
            }),
        )
        .await?;

        tx.commit().await?;
        Ok(seg)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn patch_segment(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        segment_key: &str,
        expected_version: Option<i64>,
        comment: Option<&str>,
        instructions: &[SegmentPatchInstruction],
        actor: &AuditActor,
    ) -> Result<PatchSegmentOutcome, StorageError> {
        let project = self.get_project(org_id, project_key).await?;
        let env = self.get_environment(org_id, project_key, env_key).await?;

        if env.require_comments && comment.map(str::trim).unwrap_or("").is_empty() {
            return Err(StorageError::CommentRequired);
        }

        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT id, env_id, key, name, kind, version, salt, segment_jsonb, updated_at
             FROM segments
             WHERE org_id = $1 AND env_id = $2 AND key = $3
             FOR UPDATE",
        )
        .bind(org_id)
        .bind(env.id)
        .bind(segment_key)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("segment '{segment_key}'")))?;

        let before = parse_segment_row(&row)?;
        if let Some(expected) = expected_version {
            if before.version != expected {
                return Err(StorageError::VersionConflict {
                    expected,
                    actual: before.version,
                });
            }
        }

        let patched = apply_segment_patch(&before, instructions)?;
        let mut next = patched.segment;
        next.version = before.version + 1;
        next.updated_at = Utc::now();

        let seg_body = json!({
            "included": next.included,
            "excluded": next.excluded,
            "includedContexts": next.included_contexts,
            "excludedContexts": next.excluded_contexts,
            "rules": next.rules,
        });

        sqlx::query(
            "UPDATE segments
             SET name = $4, version = $5, segment_jsonb = $6, updated_at = $7
             WHERE org_id = $1 AND env_id = $2 AND id = $3",
        )
        .bind(org_id)
        .bind(env.id)
        .bind(next.id)
        .bind(&next.name)
        .bind(next.version)
        .bind(&seg_body)
        .bind(next.updated_at)
        .execute(&mut *tx)
        .await?;

        let new_env_rev: i64 = sqlx::query_scalar(
            "UPDATE environments SET revision = revision + 1 WHERE org_id = $1 AND id = $2 RETURNING revision",
        )
        .bind(org_id)
        .bind(env.id)
        .fetch_one(&mut *tx)
        .await?;

        insert_audit_log(
            &mut tx,
            org_id,
            Some(project.id),
            Some(env.id),
            "segment",
            &next.key,
            "patch",
            actor,
            comment,
            Some(serde_json::to_value(instructions).unwrap_or(Value::Null)),
            Some(serde_json::to_value(&before).unwrap_or(Value::Null)),
            Some(serde_json::to_value(&next).unwrap_or(Value::Null)),
        )
        .await?;

        insert_outbox_event(
            &mut tx,
            org_id,
            Some(env.id),
            "segment.updated",
            json!({
                "segmentId": next.id,
                "segmentKey": next.key,
                "envId": env.id,
                "version": next.version,
                "envRevision": new_env_rev,
                "clientSideAffected": true
            }),
        )
        .await?;

        tx.commit().await?;
        Ok(PatchSegmentOutcome {
            segment: next,
            summaries: patched.summaries,
        })
    }

    pub async fn delete_segment(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        segment_key: &str,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let project = self.get_project(org_id, project_key).await?;
        let env = self.get_environment(org_id, project_key, env_key).await?;

        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT id, env_id, key, name, kind, version, salt, segment_jsonb, updated_at
             FROM segments
             WHERE org_id = $1 AND env_id = $2 AND key = $3
             FOR UPDATE",
        )
        .bind(org_id)
        .bind(env.id)
        .bind(segment_key)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("segment '{segment_key}'")))?;
        let before = parse_segment_row(&row)?;

        sqlx::query("DELETE FROM segments WHERE org_id = $1 AND id = $2")
            .bind(org_id)
            .bind(before.id)
            .execute(&mut *tx)
            .await?;

        let new_env_rev: i64 = sqlx::query_scalar(
            "UPDATE environments SET revision = revision + 1 WHERE org_id = $1 AND id = $2 RETURNING revision",
        )
        .bind(org_id)
        .bind(env.id)
        .fetch_one(&mut *tx)
        .await?;

        insert_audit_log(
            &mut tx,
            org_id,
            Some(project.id),
            Some(env.id),
            "segment",
            &before.key,
            "delete",
            actor,
            None,
            None,
            Some(serde_json::to_value(&before).unwrap_or(Value::Null)),
            None,
        )
        .await?;

        insert_outbox_event(
            &mut tx,
            org_id,
            Some(env.id),
            "segment.deleted",
            json!({
                "segmentId": before.id,
                "segmentKey": before.key,
                "envId": env.id,
                "version": before.version + 1,
                "envRevision": new_env_rev,
                "clientSideAffected": true
            }),
        )
        .await?;

        tx.commit().await?;
        Ok(())
    }

    // =========================================================================
    // Copy Targeting Between Environments & Bulk Targeting
    // =========================================================================

    #[allow(clippy::too_many_arguments)]
    pub async fn copy_flag_env_config(
        &self,
        org_id: Uuid,
        project_key: &str,
        flag_key: &str,
        source_env_key: &str,
        target_env_key: &str,
        copy_on_state: bool,
        expected_version: Option<i64>,
        comment: Option<&str>,
        actor: &AuditActor,
    ) -> Result<FlagEnvConfig, StorageError> {
        if source_env_key == target_env_key {
            return Err(StorageError::Validation(DomainError::Validation(
                "source and target environments must be different".to_string(),
            )));
        }

        let project = self.get_project(org_id, project_key).await?;
        let source_env = self
            .get_environment(org_id, project_key, source_env_key)
            .await?;
        let target_env = self
            .get_environment(org_id, project_key, target_env_key)
            .await?;

        if target_env.require_comments && comment.map(str::trim).unwrap_or("").is_empty() {
            return Err(StorageError::CommentRequired);
        }

        let mut tx = self.pool.begin().await?;
        let flag_row = sqlx::query(
            "SELECT id, project_id, key, name, description, kind, variations_jsonb, temporary,
                    client_side_availability_jsonb, tags, maintainer_id, archived, created_at, updated_at
             FROM flags WHERE org_id = $1 AND project_id = $2 AND key = $3",
        )
        .bind(org_id)
        .bind(project.id)
        .bind(flag_key)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("flag '{flag_key}'")))?;
        let flag = parse_flag_row(&flag_row)?;

        let src_row = sqlx::query(
            "SELECT flag_id, env_id, version, \"on\", salt, config_jsonb, updated_at
             FROM flag_env_configs WHERE org_id = $1 AND flag_id = $2 AND env_id = $3",
        )
        .bind(org_id)
        .bind(flag.id)
        .bind(source_env.id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("source config '{source_env_key}'")))?;
        let src_cfg = parse_flag_env_config_row(&src_row)?;

        let dst_row = sqlx::query(
            "SELECT flag_id, env_id, version, \"on\", salt, config_jsonb, updated_at
             FROM flag_env_configs WHERE org_id = $1 AND flag_id = $2 AND env_id = $3 FOR UPDATE",
        )
        .bind(org_id)
        .bind(flag.id)
        .bind(target_env.id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("target config '{target_env_key}'")))?;
        let dst_before = parse_flag_env_config_row(&dst_row)?;

        if let Some(expected) = expected_version {
            if dst_before.version != expected {
                return Err(StorageError::VersionConflict {
                    expected,
                    actual: dst_before.version,
                });
            }
        }

        let next_version = dst_before.version + 1;
        let next_on = if copy_on_state {
            src_cfg.on
        } else {
            dst_before.on
        };
        let next_body: FlagEnvConfigBody = src_cfg.config.clone();
        let now = Utc::now();
        let cfg_val = serde_json::to_value(&next_body)
            .map_err(|e| StorageError::Validation(DomainError::Validation(e.to_string())))?;

        sqlx::query(
            "UPDATE flag_env_configs
             SET version = $4, \"on\" = $5, config_jsonb = $6, updated_at = $7
             WHERE org_id = $1 AND flag_id = $2 AND env_id = $3",
        )
        .bind(org_id)
        .bind(flag.id)
        .bind(target_env.id)
        .bind(next_version)
        .bind(next_on)
        .bind(&cfg_val)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        let copy_instr = json!([{
            "kind": "copyFromEnvironment",
            "sourceEnvironmentKey": source_env_key,
            "copyOnState": copy_on_state
        }]);

        sqlx::query(
            "INSERT INTO flag_env_revisions (
                id, org_id, flag_id, env_id, version, \"on\", config_jsonb, instructions_jsonb, actor_id, comment
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(Uuid::now_v7())
        .bind(org_id)
        .bind(flag.id)
        .bind(target_env.id)
        .bind(next_version)
        .bind(next_on)
        .bind(&cfg_val)
        .bind(&copy_instr)
        .bind(actor.actor_id)
        .bind(comment)
        .execute(&mut *tx)
        .await?;

        let new_env_rev: i64 = sqlx::query_scalar(
            "UPDATE environments SET revision = revision + 1 WHERE org_id = $1 AND id = $2 RETURNING revision",
        )
        .bind(org_id)
        .bind(target_env.id)
        .fetch_one(&mut *tx)
        .await?;

        let next_cfg = FlagEnvConfig {
            flag_id: flag.id,
            env_id: target_env.id,
            version: next_version,
            on: next_on,
            salt: dst_before.salt.clone(),
            config: next_body,
            updated_at: now,
        };

        insert_audit_log(
            &mut tx,
            org_id,
            Some(project.id),
            Some(target_env.id),
            "flag",
            &flag.key,
            "copy",
            actor,
            comment,
            Some(copy_instr),
            Some(serde_json::to_value(&dst_before).unwrap_or(Value::Null)),
            Some(serde_json::to_value(&next_cfg).unwrap_or(Value::Null)),
        )
        .await?;

        insert_outbox_event(
            &mut tx,
            org_id,
            Some(target_env.id),
            "flag.updated",
            json!({
                "flagId": flag.id,
                "flagKey": flag.key,
                "envId": target_env.id,
                "version": next_version,
                "envRevision": new_env_rev,
                "clientSideAffected": flag.client_side_availability.any_enabled()
            }),
        )
        .await?;

        tx.commit().await?;
        Ok(next_cfg)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn bulk_patch_flags(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        flag_keys: &[String],
        instructions: &[FlagPatchInstruction],
        comment: Option<&str>,
        actor: &AuditActor,
    ) -> Result<Vec<PatchFlagOutcome>, StorageError> {
        if flag_keys.is_empty() {
            return Err(StorageError::Validation(DomainError::Validation(
                "bulk patch requires at least one flag key".to_string(),
            )));
        }
        let mut outcomes = Vec::with_capacity(flag_keys.len());
        for key in flag_keys {
            let outcome = self
                .patch_flag(
                    org_id,
                    project_key,
                    key,
                    env_key,
                    None,
                    comment,
                    instructions,
                    actor,
                )
                .await?;
            outcomes.push(outcome);
        }
        Ok(outcomes)
    }

    // =========================================================================
    // Flag Scheduling
    // =========================================================================

    #[allow(clippy::too_many_arguments)]
    pub async fn create_scheduled_change(
        &self,
        org_id: Uuid,
        project_key: &str,
        flag_key: &str,
        env_key: &str,
        execute_at: DateTime<Utc>,
        instructions: &[FlagPatchInstruction],
        comment: Option<&str>,
        actor: &AuditActor,
    ) -> Result<ScheduledChange, StorageError> {
        if instructions.is_empty() {
            return Err(StorageError::Validation(DomainError::Validation(
                "scheduled change must contain at least one instruction".to_string(),
            )));
        }
        let project = self.get_project(org_id, project_key).await?;
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let flag = self.get_flag(org_id, project_key, flag_key).await?.flag;

        let id = Uuid::now_v7();
        let instr_json = serde_json::to_value(instructions)
            .map_err(|e| StorageError::Validation(DomainError::Validation(e.to_string())))?;

        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO scheduled_changes (
                id, org_id, flag_id, env_id, execute_at, instructions_jsonb, comment, created_by, status
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'pending')
             RETURNING id, org_id, flag_id, env_id, execute_at, instructions_jsonb, comment,
                       created_by, status, error_message, executed_at, created_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(flag.id)
        .bind(env.id)
        .bind(execute_at)
        .bind(&instr_json)
        .bind(comment)
        .bind(actor.actor_id)
        .fetch_one(&mut *tx)
        .await?;

        let scheduled = parse_scheduled_change_row(&row)?;

        insert_audit_log(
            &mut tx,
            org_id,
            Some(project.id),
            Some(env.id),
            "scheduled_change",
            &flag.key,
            "create",
            actor,
            comment,
            Some(instr_json),
            None,
            Some(serde_json::to_value(&scheduled).unwrap_or(Value::Null)),
        )
        .await?;

        tx.commit().await?;
        Ok(scheduled)
    }

    pub async fn list_scheduled_changes(
        &self,
        org_id: Uuid,
        project_key: &str,
        flag_key: &str,
        env_key: &str,
    ) -> Result<Vec<ScheduledChange>, StorageError> {
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let flag = self.get_flag(org_id, project_key, flag_key).await?.flag;

        let rows = sqlx::query(
            "SELECT id, org_id, flag_id, env_id, execute_at, instructions_jsonb, comment,
                    created_by, status, error_message, executed_at, created_at
             FROM scheduled_changes
             WHERE org_id = $1 AND flag_id = $2 AND env_id = $3
             ORDER BY execute_at ASC",
        )
        .bind(org_id)
        .bind(flag.id)
        .bind(env.id)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(parse_scheduled_change_row).collect()
    }

    pub async fn cancel_scheduled_change(
        &self,
        org_id: Uuid,
        schedule_id: Uuid,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "UPDATE scheduled_changes
             SET status = 'canceled'
             WHERE org_id = $1 AND id = $2 AND status = 'pending'
             RETURNING flag_id, env_id",
        )
        .bind(org_id)
        .bind(schedule_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("pending schedule '{schedule_id}'")))?;

        let env_id: Uuid = row.get("env_id");
        insert_audit_log(
            &mut tx,
            org_id,
            None,
            Some(env_id),
            "scheduled_change",
            &schedule_id.to_string(),
            "cancel",
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

    /// Claims up to `limit` pending scheduled changes whose `execute_at <= now()`.
    pub async fn list_due_scheduled_changes(
        &self,
        limit: i64,
    ) -> Result<Vec<DueScheduledChange>, StorageError> {
        let rows = sqlx::query(
            "SELECT s.id, s.org_id, s.flag_id, s.env_id, s.execute_at, s.instructions_jsonb,
                    s.comment, s.created_by, s.status, s.error_message, s.executed_at, s.created_at,
                    p.key AS project_key, e.key AS env_key, f.key AS flag_key
             FROM scheduled_changes s
             JOIN flags f ON f.id = s.flag_id
             JOIN environments e ON e.id = s.env_id
             JOIN projects p ON p.id = f.project_id
             WHERE s.status = 'pending' AND s.execute_at <= now()
             ORDER BY s.execute_at ASC
             LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            out.push(DueScheduledChange {
                change: parse_scheduled_change_row(row)?,
                project_key: row.get("project_key"),
                env_key: row.get("env_key"),
                flag_key: row.get("flag_key"),
            });
        }
        Ok(out)
    }

    pub async fn mark_scheduled_change_result(
        &self,
        schedule_id: Uuid,
        status: ScheduledChangeStatus,
        error_message: Option<&str>,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE scheduled_changes
             SET status = $2, error_message = $3, executed_at = now()
             WHERE id = $1 AND status = 'pending'",
        )
        .bind(schedule_id)
        .bind(status.as_str())
        .bind(error_message)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // =========================================================================
    // Flag Triggers (APM / Instant Rollback URLs)
    // =========================================================================

    #[allow(clippy::too_many_arguments)]
    pub async fn create_flag_trigger(
        &self,
        org_id: Uuid,
        project_key: &str,
        flag_key: &str,
        env_key: &str,
        action: TriggerAction,
        description: &str,
        actor: &AuditActor,
    ) -> Result<CreatedTriggerOutcome, StorageError> {
        let project = self.get_project(org_id, project_key).await?;
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let flag = self.get_flag(org_id, project_key, flag_key).await?.flag;

        let secret = generate_prefixed_secret("sk_trg_");
        let id = Uuid::now_v7();
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO flag_triggers (
                id, org_id, flag_id, env_id, action, token_prefix, token_hash, description, enabled, created_by
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, true, $9)
             RETURNING id, flag_id, env_id, action, token_prefix, description, enabled,
                       trigger_count, last_triggered_at, created_by, created_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(flag.id)
        .bind(env.id)
        .bind(action.as_str())
        .bind(&secret.prefix)
        .bind(&secret.hash)
        .bind(description.trim())
        .bind(actor.actor_id)
        .fetch_one(&mut *tx)
        .await?;

        let trigger = parse_flag_trigger_row(&row)?;
        insert_audit_log(
            &mut tx,
            org_id,
            Some(project.id),
            Some(env.id),
            "flag_trigger",
            &flag.key,
            "create",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&trigger).unwrap_or(Value::Null)),
        )
        .await?;

        tx.commit().await?;
        Ok(CreatedTriggerOutcome {
            trigger,
            plaintext_token: secret.plaintext,
        })
    }

    pub async fn list_flag_triggers(
        &self,
        org_id: Uuid,
        project_key: &str,
        flag_key: &str,
        env_key: &str,
    ) -> Result<Vec<skotos_domain::FlagTrigger>, StorageError> {
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let flag = self.get_flag(org_id, project_key, flag_key).await?.flag;
        let rows = sqlx::query(
            "SELECT id, flag_id, env_id, action, token_prefix, description, enabled,
                    trigger_count, last_triggered_at, created_by, created_at
             FROM flag_triggers
             WHERE org_id = $1 AND flag_id = $2 AND env_id = $3
             ORDER BY created_at DESC",
        )
        .bind(org_id)
        .bind(flag.id)
        .bind(env.id)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(parse_flag_trigger_row).collect()
    }

    pub async fn delete_flag_trigger(
        &self,
        org_id: Uuid,
        trigger_id: Uuid,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let res = sqlx::query("DELETE FROM flag_triggers WHERE org_id = $1 AND id = $2")
            .bind(org_id)
            .bind(trigger_id)
            .execute(&mut *tx)
            .await?;
        if res.rows_affected() == 0 {
            return Err(StorageError::NotFound(format!(
                "flag trigger '{trigger_id}'"
            )));
        }
        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "flag_trigger",
            &trigger_id.to_string(),
            "delete",
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

    /// Executes an enabled flag trigger using its unauthenticated secret token.
    pub async fn execute_flag_trigger_by_token(
        &self,
        plaintext_token: &str,
    ) -> Result<ExecutedTriggerOutcome, StorageError> {
        let token_hash = sha256_hex(plaintext_token);
        let row = sqlx::query(
            "SELECT t.id, t.org_id, t.action, t.enabled, t.description,
                    p.key AS project_key, e.key AS env_key, f.key AS flag_key
             FROM flag_triggers t
             JOIN flags f ON f.id = t.flag_id
             JOIN environments e ON e.id = t.env_id
             JOIN projects p ON p.id = f.project_id
             WHERE t.token_hash = $1",
        )
        .bind(&token_hash)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound("flag trigger".to_string()))?;

        let enabled: bool = row.get("enabled");
        if !enabled {
            return Err(StorageError::Validation(DomainError::Validation(
                "flag trigger is disabled".to_string(),
            )));
        }

        let trigger_id: Uuid = row.get("id");
        let org_id: Uuid = row.get("org_id");
        let project_key: String = row.get("project_key");
        let env_key: String = row.get("env_key");
        let flag_key: String = row.get("flag_key");
        let description: String = row.get("description");
        let action = parse_trigger_action(row.get::<String, _>("action").as_str())?;

        let instr = match action {
            TriggerAction::TurnFlagOn => FlagPatchInstruction::TurnFlagOn,
            TriggerAction::TurnFlagOff => FlagPatchInstruction::TurnFlagOff,
        };
        let comment = if description.is_empty() {
            format!("Executed flag trigger {}", trigger_id)
        } else {
            format!("Executed flag trigger: {}", description)
        };
        let actor = AuditActor {
            actor_id: Some(trigger_id),
            actor_type: "trigger".to_string(),
            actor_email: None,
        };

        let outcome = self
            .patch_flag(
                org_id,
                &project_key,
                &flag_key,
                &env_key,
                None,
                Some(&comment),
                &[instr],
                &actor,
            )
            .await?;

        sqlx::query(
            "UPDATE flag_triggers
             SET trigger_count = trigger_count + 1, last_triggered_at = now()
             WHERE id = $1",
        )
        .bind(trigger_id)
        .execute(&self.pool)
        .await?;

        Ok(ExecutedTriggerOutcome {
            flag_key,
            env_key,
            action,
            new_version: outcome.env_config.version,
            on: outcome.env_config.on,
        })
    }

    // =========================================================================
    // Signed Outbound Webhooks
    // =========================================================================

    #[allow(clippy::too_many_arguments)]
    pub async fn create_webhook(
        &self,
        org_id: Uuid,
        name: &str,
        url: &str,
        secret: Option<&str>,
        events: Vec<String>,
        env_filter: Vec<String>,
        actor: &AuditActor,
    ) -> Result<CreatedWebhookOutcome, StorageError> {
        if name.trim().is_empty() {
            return Err(StorageError::Validation(DomainError::Validation(
                "webhook name cannot be empty".to_string(),
            )));
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(StorageError::Validation(DomainError::Validation(
                "webhook url must start with http:// or https://".to_string(),
            )));
        }
        let resolved_secret = secret
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| generate_prefixed_secret("sk_whsec_").plaintext);

        let id = Uuid::now_v7();
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO webhooks (id, org_id, name, url, secret, enabled, events, env_filter, created_by)
             VALUES ($1, $2, $3, $4, $5, true, $6, $7, $8)
             RETURNING id, org_id, name, url, enabled, events, env_filter, created_by, created_at, updated_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(name.trim())
        .bind(url.trim())
        .bind(&resolved_secret)
        .bind(&events)
        .bind(&env_filter)
        .bind(actor.actor_id)
        .fetch_one(&mut *tx)
        .await?;
        let webhook = parse_webhook_row(&row);

        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "webhook",
            &webhook.id.to_string(),
            "create",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&webhook).unwrap_or(Value::Null)),
        )
        .await?;

        tx.commit().await?;
        Ok(CreatedWebhookOutcome {
            webhook,
            secret: resolved_secret,
        })
    }

    pub async fn list_webhooks(&self, org_id: Uuid) -> Result<Vec<Webhook>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, org_id, name, url, enabled, events, env_filter, created_by, created_at, updated_at
             FROM webhooks WHERE org_id = $1 ORDER BY created_at DESC",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(parse_webhook_row).collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_webhook(
        &self,
        org_id: Uuid,
        webhook_id: Uuid,
        name: Option<&str>,
        url: Option<&str>,
        enabled: Option<bool>,
        events: Option<Vec<String>>,
        env_filter: Option<Vec<String>>,
        actor: &AuditActor,
    ) -> Result<Webhook, StorageError> {
        if let Some(u) = url {
            if !u.starts_with("http://") && !u.starts_with("https://") {
                return Err(StorageError::Validation(DomainError::Validation(
                    "webhook url must start with http:// or https://".to_string(),
                )));
            }
        }
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "UPDATE webhooks
             SET name = COALESCE($3, name),
                 url = COALESCE($4, url),
                 enabled = COALESCE($5, enabled),
                 events = COALESCE($6, events),
                 env_filter = COALESCE($7, env_filter),
                 updated_at = now()
             WHERE org_id = $1 AND id = $2
             RETURNING id, org_id, name, url, enabled, events, env_filter, created_by, created_at, updated_at",
        )
        .bind(org_id)
        .bind(webhook_id)
        .bind(name.map(str::trim))
        .bind(url.map(str::trim))
        .bind(enabled)
        .bind(events)
        .bind(env_filter)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("webhook '{webhook_id}'")))?;
        let webhook = parse_webhook_row(&row);

        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "webhook",
            &webhook.id.to_string(),
            "update",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&webhook).unwrap_or(Value::Null)),
        )
        .await?;

        tx.commit().await?;
        Ok(webhook)
    }

    pub async fn delete_webhook(
        &self,
        org_id: Uuid,
        webhook_id: Uuid,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let res = sqlx::query("DELETE FROM webhooks WHERE org_id = $1 AND id = $2")
            .bind(org_id)
            .bind(webhook_id)
            .execute(&mut *tx)
            .await?;
        if res.rows_affected() == 0 {
            return Err(StorageError::NotFound(format!("webhook '{webhook_id}'")));
        }
        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "webhook",
            &webhook_id.to_string(),
            "delete",
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

    pub async fn list_active_webhooks_with_secret(
        &self,
        org_id: Uuid,
    ) -> Result<Vec<WebhookWithSecret>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, org_id, name, url, secret, enabled, events, env_filter, created_by, created_at, updated_at
             FROM webhooks WHERE org_id = $1 AND enabled = true",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| WebhookWithSecret {
                webhook: parse_webhook_row(r),
                secret: r.get("secret"),
            })
            .collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_webhook_delivery(
        &self,
        org_id: Uuid,
        webhook_id: Uuid,
        event_type: &str,
        status_code: Option<i32>,
        attempt: i32,
        success: bool,
        error_message: Option<&str>,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO webhook_deliveries (
                id, webhook_id, org_id, event_type, status_code, attempt, success, error_message
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(Uuid::now_v7())
        .bind(webhook_id)
        .bind(org_id)
        .bind(event_type)
        .bind(status_code)
        .bind(attempt)
        .bind(success)
        .bind(error_message)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_webhook_deliveries(
        &self,
        org_id: Uuid,
        webhook_id: Uuid,
        limit: i64,
    ) -> Result<Vec<WebhookDelivery>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, webhook_id, event_type, status_code, attempt, success, error_message, delivered_at
             FROM webhook_deliveries
             WHERE org_id = $1 AND webhook_id = $2
             ORDER BY delivered_at DESC
             LIMIT $3",
        )
        .bind(org_id)
        .bind(webhook_id)
        .bind(limit.clamp(1, 100))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| WebhookDelivery {
                id: r.get("id"),
                webhook_id: r.get("webhook_id"),
                event_type: r.get("event_type"),
                status_code: r.get("status_code"),
                attempt: r.get("attempt"),
                success: r.get("success"),
                error_message: r.get("error_message"),
                delivered_at: r.get("delivered_at"),
            })
            .collect())
    }

    // =========================================================================
    // Outbox Worker & Delivery Snapshot Compilation Helpers
    // =========================================================================

    /// Claims up to `limit` unpublished outbox events ready for delivery using
    /// `FOR UPDATE SKIP LOCKED`, bumping `attempts` and pushing `next_attempt_at`
    /// forward as a visibility lease so multiple workers never double-publish.
    pub async fn claim_outbox_events(
        &self,
        limit: i64,
        lease_seconds: i64,
    ) -> Result<Vec<ClaimedOutboxEvent>, StorageError> {
        let lease_until = Utc::now() + Duration::seconds(lease_seconds.max(5));
        let rows = sqlx::query(
            "WITH claimed AS (
                SELECT id
                FROM outbox_events
                WHERE published_at IS NULL AND next_attempt_at <= now()
                ORDER BY created_at ASC
                LIMIT $1
                FOR UPDATE SKIP LOCKED
             )
             UPDATE outbox_events o
             SET attempts = o.attempts + 1,
                 next_attempt_at = $2
             FROM claimed
             WHERE o.id = claimed.id
             RETURNING o.id, o.org_id, o.env_id, o.event_type, o.payload_jsonb,
                       o.created_at, o.published_at, o.attempts",
        )
        .bind(limit.clamp(1, 200))
        .bind(lease_until)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| ClaimedOutboxEvent {
                event: OutboxEvent {
                    id: r.get("id"),
                    org_id: r.get("org_id"),
                    env_id: r.get("env_id"),
                    event_type: r.get("event_type"),
                    payload: r.get("payload_jsonb"),
                    created_at: r.get("created_at"),
                    published_at: r.get("published_at"),
                },
                attempts: r.get("attempts"),
            })
            .collect())
    }

    pub async fn mark_outbox_published(&self, event_id: Uuid) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE outbox_events SET published_at = now(), last_error = NULL WHERE id = $1",
        )
        .bind(event_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_outbox_failed(
        &self,
        event_id: Uuid,
        attempts: i32,
        error: &str,
    ) -> Result<(), StorageError> {
        let backoff_secs = (1_i64 << attempts.clamp(0, 8)).min(300);
        let next_at = Utc::now() + Duration::seconds(backoff_secs);
        sqlx::query("UPDATE outbox_events SET next_attempt_at = $2, last_error = $3 WHERE id = $1")
            .bind(event_id)
            .bind(next_at)
            .bind(error)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Loads an entire environment's flags + env configs + segments by `env_id`
    /// for snapshot compilation in `crates/skotos-fanout`.
    pub async fn load_environment_compile_bundle_by_id(
        &self,
        env_id: Uuid,
    ) -> Result<EnvironmentCompileBundle, StorageError> {
        let env_row = sqlx::query(
            "SELECT id, org_id, project_id, key, name, color, client_side_id,
                    require_comments, confirm_changes, revision, created_at
             FROM environments WHERE id = $1",
        )
        .bind(env_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("environment '{env_id}'")))?;
        let org_id: Uuid = env_row.get("org_id");
        let environment = parse_environment_row(&env_row)?;

        let flag_rows = sqlx::query(
            "SELECT f.id, f.project_id, f.key, f.name, f.description, f.kind, f.variations_jsonb,
                    f.temporary, f.client_side_availability_jsonb, f.tags, f.maintainer_id,
                    f.archived, f.created_at, f.updated_at,
                    c.flag_id, c.env_id, c.version, c.\"on\", c.salt, c.config_jsonb,
                    c.updated_at AS cfg_updated_at
             FROM flags f
             JOIN flag_env_configs c ON c.flag_id = f.id AND c.env_id = $2
             WHERE f.org_id = $1 AND f.project_id = $3 AND f.archived = false",
        )
        .bind(org_id)
        .bind(env_id)
        .bind(environment.project_id)
        .fetch_all(&self.pool)
        .await?;

        let mut flags = Vec::with_capacity(flag_rows.len());
        for row in &flag_rows {
            let flag = parse_flag_row(row)?;
            let config: FlagEnvConfigBody = serde_json::from_value(row.get("config_jsonb"))
                .map_err(|e| StorageError::Validation(DomainError::Validation(e.to_string())))?;
            let cfg = FlagEnvConfig {
                flag_id: flag.id,
                env_id,
                version: row.get("version"),
                on: row.get("on"),
                salt: row.get("salt"),
                config,
                updated_at: row.get("cfg_updated_at"),
            };
            flags.push((flag, cfg));
        }

        let seg_rows = sqlx::query(
            "SELECT id, env_id, key, name, kind, version, salt, segment_jsonb, updated_at
             FROM segments WHERE org_id = $1 AND env_id = $2",
        )
        .bind(org_id)
        .bind(env_id)
        .fetch_all(&self.pool)
        .await?;
        let mut segments = Vec::with_capacity(seg_rows.len());
        for row in &seg_rows {
            segments.push(parse_segment_row(row)?);
        }

        Ok(EnvironmentCompileBundle {
            org_id,
            environment,
            flags,
            segments,
        })
    }

    /// Loads auth credentials (client-side ID + hashed SDK/Mobile keys) for one environment.
    pub async fn load_environment_auth_record(
        &self,
        env_id: Uuid,
    ) -> Result<EnvironmentAuthRecord, StorageError> {
        let env_row = sqlx::query(
            "SELECT id, org_id, project_id, key, client_side_id FROM environments WHERE id = $1",
        )
        .bind(env_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("environment '{env_id}'")))?;

        let key_rows = sqlx::query(
            "SELECT id, kind, key_hash, expires_at
             FROM environment_keys
             WHERE env_id = $1 AND (expires_at IS NULL OR expires_at > now())",
        )
        .bind(env_id)
        .fetch_all(&self.pool)
        .await?;

        let mut keys = Vec::with_capacity(key_rows.len());
        for r in &key_rows {
            let kind = match r.get::<String, _>("kind").as_str() {
                "sdk" => EnvironmentKeyKind::Sdk,
                "mobile" => EnvironmentKeyKind::Mobile,
                _ => continue,
            };
            keys.push(EnvironmentKeyHashRecord {
                key_id: r.get("id"),
                kind,
                key_hash: r.get("key_hash"),
                expires_at: r.get("expires_at"),
            });
        }

        Ok(EnvironmentAuthRecord {
            org_id: env_row.get("org_id"),
            project_id: env_row.get("project_id"),
            env_id: env_row.get("id"),
            env_key: env_row.get("key"),
            client_side_id: env_row.get("client_side_id"),
            keys,
        })
    }

    /// Lists all environment auth records across the cluster (used on bootstrap sync to R2/S3).
    pub async fn list_all_environment_auth_records(
        &self,
    ) -> Result<Vec<EnvironmentAuthRecord>, StorageError> {
        let env_rows =
            sqlx::query("SELECT id, org_id, project_id, key, client_side_id FROM environments")
                .fetch_all(&self.pool)
                .await?;

        let key_rows = sqlx::query(
            "SELECT id, env_id, kind, key_hash, expires_at
             FROM environment_keys
             WHERE expires_at IS NULL OR expires_at > now()",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut keys_by_env: HashMap<Uuid, Vec<EnvironmentKeyHashRecord>> = HashMap::new();
        for r in &key_rows {
            let kind = match r.get::<String, _>("kind").as_str() {
                "sdk" => EnvironmentKeyKind::Sdk,
                "mobile" => EnvironmentKeyKind::Mobile,
                _ => continue,
            };
            let env_id: Uuid = r.get("env_id");
            keys_by_env
                .entry(env_id)
                .or_default()
                .push(EnvironmentKeyHashRecord {
                    key_id: r.get("id"),
                    kind,
                    key_hash: r.get("key_hash"),
                    expires_at: r.get("expires_at"),
                });
        }

        Ok(env_rows
            .iter()
            .map(|r| {
                let env_id: Uuid = r.get("id");
                EnvironmentAuthRecord {
                    org_id: r.get("org_id"),
                    project_id: r.get("project_id"),
                    env_id,
                    env_key: r.get("key"),
                    client_side_id: r.get("client_side_id"),
                    keys: keys_by_env.remove(&env_id).unwrap_or_default(),
                }
            })
            .collect())
    }
}

fn parse_scheduled_change_row(
    row: &sqlx::postgres::PgRow,
) -> Result<ScheduledChange, StorageError> {
    let status = match row.get::<String, _>("status").as_str() {
        "pending" => ScheduledChangeStatus::Pending,
        "executed" => ScheduledChangeStatus::Executed,
        "failed" => ScheduledChangeStatus::Failed,
        "canceled" => ScheduledChangeStatus::Canceled,
        other => {
            return Err(StorageError::Validation(DomainError::Validation(format!(
                "unknown scheduled change status '{other}'"
            ))));
        }
    };
    Ok(ScheduledChange {
        id: row.get("id"),
        org_id: row.get("org_id"),
        flag_id: row.get("flag_id"),
        env_id: row.get("env_id"),
        execute_at: row.get("execute_at"),
        instructions: row.get("instructions_jsonb"),
        comment: row.get("comment"),
        created_by: row.get("created_by"),
        status,
        error_message: row.get("error_message"),
        executed_at: row.get("executed_at"),
        created_at: row.get("created_at"),
    })
}

fn parse_trigger_action(value: &str) -> Result<TriggerAction, StorageError> {
    match value {
        "turnFlagOn" => Ok(TriggerAction::TurnFlagOn),
        "turnFlagOff" => Ok(TriggerAction::TurnFlagOff),
        other => Err(StorageError::Validation(DomainError::Validation(format!(
            "unknown trigger action '{other}'"
        )))),
    }
}

fn parse_flag_trigger_row(
    row: &sqlx::postgres::PgRow,
) -> Result<skotos_domain::FlagTrigger, StorageError> {
    Ok(skotos_domain::FlagTrigger {
        id: row.get("id"),
        flag_id: row.get("flag_id"),
        env_id: row.get("env_id"),
        action: parse_trigger_action(row.get::<String, _>("action").as_str())?,
        token_prefix: row.get("token_prefix"),
        description: row.get("description"),
        enabled: row.get("enabled"),
        trigger_count: row.get("trigger_count"),
        last_triggered_at: row.get("last_triggered_at"),
        created_by: row.get("created_by"),
        created_at: row.get("created_at"),
    })
}

fn parse_webhook_row(row: &sqlx::postgres::PgRow) -> Webhook {
    Webhook {
        id: row.get("id"),
        org_id: row.get("org_id"),
        name: row.get("name"),
        url: row.get("url"),
        enabled: row.get("enabled"),
        events: row.get("events"),
        env_filter: row.get("env_filter"),
        created_by: row.get("created_by"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}
