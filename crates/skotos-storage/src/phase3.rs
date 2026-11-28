use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use skotos_domain::{
    big_segment_key_hash, validate_key, Application, ApplicationKind, ApplicationVersion,
    ApprovalRequest, ApprovalReview, ApprovalReviewKind, ApprovalStatus, AuditArchiveRecord,
    AuditLogEntry, BigSegmentSnapshot, BigSegmentStatus, BuiltInRole, CodeRefHunk, CustomRole,
    DomainError, Environment, EnvironmentApprovalSettings, FlagCodeRefSummary,
    FlagPatchInstruction, FlagRelease, FlagReleasePhaseState, FlagWorkflow, Member,
    PolicyStatement, ReleasePhase, ReleasePipeline, ScheduledChangeStatus, ScheduledSegmentChange,
    SegmentPatchInstruction, SsoProtocol, SsoProviderConfig, Team, WebhookIntegrationKind,
    WorkflowStage, WorkflowStageStatus, WorkflowStatus,
};
use sqlx::Row;
use uuid::Uuid;

use crate::error::StorageError;
use crate::repo::{
    insert_audit_log, insert_outbox_event, map_unique_err, parse_environment_row, AuditActor,
    PatchFlagOutcome, Storage,
};

#[derive(Debug, Clone)]
pub struct ResolvedMemberRbac {
    pub team_ids: Vec<Uuid>,
    pub role_ids: Vec<Uuid>,
    pub custom_policies: Vec<PolicyStatement>,
}

#[derive(Debug, Clone)]
pub struct DueScheduledSegmentChange {
    pub change: ScheduledSegmentChange,
    pub project_key: String,
    pub env_key: String,
    pub segment_key: String,
}

impl Storage {
    // =========================================================================
    // Custom RBAC Roles & Member Role Resolution
    // =========================================================================

    pub async fn list_custom_roles(&self, org_id: Uuid) -> Result<Vec<CustomRole>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, org_id, key, name, description, statements, created_at, updated_at
             FROM custom_roles WHERE org_id = $1 ORDER BY key ASC",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(parse_custom_role_row).collect()
    }

    pub async fn get_custom_role(
        &self,
        org_id: Uuid,
        role_key: &str,
    ) -> Result<CustomRole, StorageError> {
        let row = sqlx::query(
            "SELECT id, org_id, key, name, description, statements, created_at, updated_at
             FROM custom_roles WHERE org_id = $1 AND key = $2",
        )
        .bind(org_id)
        .bind(role_key)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("custom role '{role_key}'")))?;
        parse_custom_role_row(&row)
    }

    pub async fn create_custom_role(
        &self,
        org_id: Uuid,
        key: &str,
        name: &str,
        description: &str,
        statements: &[PolicyStatement],
        actor: &AuditActor,
    ) -> Result<CustomRole, StorageError> {
        validate_key(key)?;
        if name.trim().is_empty() {
            return Err(StorageError::Validation(DomainError::Validation(
                "custom role name cannot be empty".to_string(),
            )));
        }
        let id = Uuid::now_v7();
        let stmt_json = serde_json::to_value(statements)
            .map_err(|e| StorageError::Validation(DomainError::Validation(e.to_string())))?;

        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO custom_roles (id, org_id, key, name, description, statements)
             VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING id, org_id, key, name, description, statements, created_at, updated_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(key)
        .bind(name.trim())
        .bind(description.trim())
        .bind(&stmt_json)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_unique_err("custom_role"))?;

        let role = parse_custom_role_row(&row)?;
        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "custom_role",
            key,
            "create",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&role).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(role)
    }

    pub async fn update_custom_role(
        &self,
        org_id: Uuid,
        role_key: &str,
        name: Option<&str>,
        description: Option<&str>,
        statements: Option<&[PolicyStatement]>,
        actor: &AuditActor,
    ) -> Result<CustomRole, StorageError> {
        let stmt_json = statements
            .map(|s| {
                serde_json::to_value(s)
                    .map_err(|e| StorageError::Validation(DomainError::Validation(e.to_string())))
            })
            .transpose()?;

        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "UPDATE custom_roles
             SET name = COALESCE($3, name),
                 description = COALESCE($4, description),
                 statements = COALESCE($5, statements),
                 updated_at = now()
             WHERE org_id = $1 AND key = $2
             RETURNING id, org_id, key, name, description, statements, created_at, updated_at",
        )
        .bind(org_id)
        .bind(role_key)
        .bind(name.map(str::trim))
        .bind(description.map(str::trim))
        .bind(stmt_json)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("custom role '{role_key}'")))?;

        let role = parse_custom_role_row(&row)?;
        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "custom_role",
            role_key,
            "update",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&role).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(role)
    }

    pub async fn delete_custom_role(
        &self,
        org_id: Uuid,
        role_key: &str,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let res = sqlx::query("DELETE FROM custom_roles WHERE org_id = $1 AND key = $2")
            .bind(org_id)
            .bind(role_key)
            .execute(&mut *tx)
            .await?;
        if res.rows_affected() == 0 {
            return Err(StorageError::NotFound(format!("custom role '{role_key}'")));
        }
        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "custom_role",
            role_key,
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

    /// Returns the custom role keys assigned directly to each member of the org.
    pub async fn list_member_custom_role_keys(
        &self,
        org_id: Uuid,
    ) -> Result<HashMap<Uuid, Vec<String>>, StorageError> {
        let rows = sqlx::query(
            "SELECT mcr.member_id, cr.key
             FROM member_custom_roles mcr
             JOIN custom_roles cr ON cr.id = mcr.role_id
             WHERE mcr.org_id = $1
             ORDER BY mcr.created_at ASC, cr.key ASC",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await?;
        let mut out: HashMap<Uuid, Vec<String>> = HashMap::new();
        for r in rows {
            out.entry(r.get("member_id"))
                .or_default()
                .push(r.get("key"));
        }
        Ok(out)
    }

    /// Fails with `NotFound` if any of the given custom role keys does not exist in the org.
    pub async fn ensure_custom_roles_exist(
        &self,
        org_id: Uuid,
        role_keys: &[String],
    ) -> Result<(), StorageError> {
        if role_keys.is_empty() {
            return Ok(());
        }
        let found: Vec<String> =
            sqlx::query_scalar("SELECT key FROM custom_roles WHERE org_id = $1 AND key = ANY($2)")
                .bind(org_id)
                .bind(role_keys)
                .fetch_all(&self.pool)
                .await?;
        if let Some(missing) = role_keys.iter().find(|k| !found.contains(k)) {
            return Err(StorageError::NotFound(format!("custom role '{missing}'")));
        }
        Ok(())
    }

    pub async fn assign_member_custom_roles(
        &self,
        org_id: Uuid,
        member_id: Uuid,
        role_keys: &[String],
        actor: &AuditActor,
    ) -> Result<Vec<String>, StorageError> {
        let mut tx = self.pool.begin().await?;
        let exists: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM members WHERE org_id = $1 AND id = $2")
                .bind(org_id)
                .bind(member_id)
                .fetch_optional(&mut *tx)
                .await?;
        if exists.is_none() {
            return Err(StorageError::NotFound(format!("member '{member_id}'")));
        }

        sqlx::query("DELETE FROM member_custom_roles WHERE org_id = $1 AND member_id = $2")
            .bind(org_id)
            .bind(member_id)
            .execute(&mut *tx)
            .await?;

        let mut assigned = Vec::new();
        for key in role_keys {
            let role_id: Uuid =
                sqlx::query_scalar("SELECT id FROM custom_roles WHERE org_id = $1 AND key = $2")
                    .bind(org_id)
                    .bind(key)
                    .fetch_optional(&mut *tx)
                    .await?
                    .ok_or_else(|| StorageError::NotFound(format!("custom role '{key}'")))?;
            sqlx::query(
                "INSERT INTO member_custom_roles (org_id, member_id, role_id)
                 VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            )
            .bind(org_id)
            .bind(member_id)
            .bind(role_id)
            .execute(&mut *tx)
            .await?;
            assigned.push(key.clone());
        }

        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "member",
            &member_id.to_string(),
            "assign_custom_roles",
            actor,
            None,
            None,
            None,
            Some(json!({ "roleKeys": assigned })),
        )
        .await?;
        tx.commit().await?;
        Ok(assigned)
    }

    /// Resolves effective teams, custom role IDs, and merged Custom RBAC policy statements
    /// for a member (union of direct custom roles + all teams the member belongs to).
    pub async fn resolve_member_rbac(
        &self,
        org_id: Uuid,
        member_id: Uuid,
    ) -> Result<ResolvedMemberRbac, StorageError> {
        let team_rows = sqlx::query(
            "SELECT t.id, t.custom_role_keys
             FROM teams t
             JOIN team_members tm ON tm.team_id = t.id
             WHERE tm.org_id = $1 AND tm.member_id = $2",
        )
        .bind(org_id)
        .bind(member_id)
        .fetch_all(&self.pool)
        .await?;

        let mut team_ids = Vec::with_capacity(team_rows.len());
        let mut team_role_keys: HashSet<String> = HashSet::new();
        for tr in team_rows {
            team_ids.push(tr.get::<Uuid, _>("id"));
            let keys: Vec<String> = tr.get("custom_role_keys");
            for k in keys {
                team_role_keys.insert(k);
            }
        }

        let direct_roles = sqlx::query(
            "SELECT cr.id, cr.statements
             FROM custom_roles cr
             JOIN member_custom_roles mcr ON mcr.role_id = cr.id
             WHERE mcr.org_id = $1 AND mcr.member_id = $2",
        )
        .bind(org_id)
        .bind(member_id)
        .fetch_all(&self.pool)
        .await?;

        let mut role_id_set: HashSet<Uuid> = HashSet::new();
        let mut custom_policies: Vec<PolicyStatement> = Vec::new();
        for dr in direct_roles {
            let rid: Uuid = dr.get("id");
            if role_id_set.insert(rid) {
                let stmts: Vec<PolicyStatement> =
                    serde_json::from_value(dr.get("statements")).unwrap_or_default();
                custom_policies.extend(stmts);
            }
        }

        if !team_role_keys.is_empty() {
            let keys_vec: Vec<String> = team_role_keys.into_iter().collect();
            let team_roles = sqlx::query(
                "SELECT id, statements FROM custom_roles WHERE org_id = $1 AND key = ANY($2)",
            )
            .bind(org_id)
            .bind(&keys_vec)
            .fetch_all(&self.pool)
            .await?;
            for tr in team_roles {
                let rid: Uuid = tr.get("id");
                if role_id_set.insert(rid) {
                    let stmts: Vec<PolicyStatement> =
                        serde_json::from_value(tr.get("statements")).unwrap_or_default();
                    custom_policies.extend(stmts);
                }
            }
        }

        Ok(ResolvedMemberRbac {
            team_ids,
            role_ids: role_id_set.into_iter().collect(),
            custom_policies,
        })
    }

    // =========================================================================
    // Teams CRUD & Membership
    // =========================================================================

    pub async fn list_teams(&self, org_id: Uuid) -> Result<Vec<Team>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, org_id, key, name, description, custom_role_keys, created_at, updated_at
             FROM teams WHERE org_id = $1 ORDER BY key ASC",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await?;

        let mut teams = Vec::with_capacity(rows.len());
        for r in rows {
            let team_id: Uuid = r.get("id");
            let member_ids: Vec<Uuid> = sqlx::query_scalar(
                "SELECT member_id FROM team_members WHERE org_id = $1 AND team_id = $2",
            )
            .bind(org_id)
            .bind(team_id)
            .fetch_all(&self.pool)
            .await?;
            teams.push(Team {
                id: team_id,
                org_id: r.get("org_id"),
                key: r.get("key"),
                name: r.get("name"),
                description: r.get("description"),
                custom_role_keys: r.get("custom_role_keys"),
                member_ids,
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            });
        }
        Ok(teams)
    }

    pub async fn create_team(
        &self,
        org_id: Uuid,
        key: &str,
        name: &str,
        description: &str,
        custom_role_keys: &[String],
        actor: &AuditActor,
    ) -> Result<Team, StorageError> {
        validate_key(key)?;
        if name.trim().is_empty() {
            return Err(StorageError::Validation(DomainError::Validation(
                "team name cannot be empty".to_string(),
            )));
        }
        let id = Uuid::now_v7();
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO teams (id, org_id, key, name, description, custom_role_keys)
             VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING id, org_id, key, name, description, custom_role_keys, created_at, updated_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(key)
        .bind(name.trim())
        .bind(description.trim())
        .bind(custom_role_keys)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_unique_err("team"))?;

        let team = Team {
            id: row.get("id"),
            org_id: row.get("org_id"),
            key: row.get("key"),
            name: row.get("name"),
            description: row.get("description"),
            custom_role_keys: row.get("custom_role_keys"),
            member_ids: vec![],
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        };

        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "team",
            key,
            "create",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&team).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(team)
    }

    pub async fn update_team_roles_and_meta(
        &self,
        org_id: Uuid,
        team_key: &str,
        name: Option<&str>,
        description: Option<&str>,
        custom_role_keys: Option<&[String]>,
        actor: &AuditActor,
    ) -> Result<Team, StorageError> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "UPDATE teams
             SET name = COALESCE($3, name),
                 description = COALESCE($4, description),
                 custom_role_keys = COALESCE($5, custom_role_keys),
                 updated_at = now()
             WHERE org_id = $1 AND key = $2
             RETURNING id, org_id, key, name, description, custom_role_keys, created_at, updated_at",
        )
        .bind(org_id)
        .bind(team_key)
        .bind(name.map(str::trim))
        .bind(description.map(str::trim))
        .bind(custom_role_keys)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("team '{team_key}'")))?;

        let team_id: Uuid = row.get("id");
        let member_ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT member_id FROM team_members WHERE org_id = $1 AND team_id = $2",
        )
        .bind(org_id)
        .bind(team_id)
        .fetch_all(&mut *tx)
        .await?;

        let team = Team {
            id: team_id,
            org_id: row.get("org_id"),
            key: row.get("key"),
            name: row.get("name"),
            description: row.get("description"),
            custom_role_keys: row.get("custom_role_keys"),
            member_ids,
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        };

        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "team",
            team_key,
            "update",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&team).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(team)
    }

    pub async fn add_team_member(
        &self,
        org_id: Uuid,
        team_key: &str,
        member_id: Uuid,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let team_id: Uuid =
            sqlx::query_scalar("SELECT id FROM teams WHERE org_id = $1 AND key = $2")
                .bind(org_id)
                .bind(team_key)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| StorageError::NotFound(format!("team '{team_key}'")))?;
        sqlx::query(
            "INSERT INTO team_members (org_id, team_id, member_id)
             VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
        )
        .bind(org_id)
        .bind(team_id)
        .bind(member_id)
        .execute(&mut *tx)
        .await?;

        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "team",
            team_key,
            "add_member",
            actor,
            None,
            None,
            None,
            Some(json!({ "memberId": member_id })),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn remove_team_member(
        &self,
        org_id: Uuid,
        team_key: &str,
        member_id: Uuid,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let team_id: Uuid =
            sqlx::query_scalar("SELECT id FROM teams WHERE org_id = $1 AND key = $2")
                .bind(org_id)
                .bind(team_key)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| StorageError::NotFound(format!("team '{team_key}'")))?;
        sqlx::query(
            "DELETE FROM team_members WHERE org_id = $1 AND team_id = $2 AND member_id = $3",
        )
        .bind(org_id)
        .bind(team_id)
        .bind(member_id)
        .execute(&mut *tx)
        .await?;
        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "team",
            team_key,
            "remove_member",
            actor,
            None,
            None,
            None,
            Some(json!({ "memberId": member_id })),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_team(
        &self,
        org_id: Uuid,
        team_key: &str,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let res = sqlx::query("DELETE FROM teams WHERE org_id = $1 AND key = $2")
            .bind(org_id)
            .bind(team_key)
            .execute(&mut *tx)
            .await?;
        if res.rows_affected() == 0 {
            return Err(StorageError::NotFound(format!("team '{team_key}'")));
        }
        insert_audit_log(
            &mut tx, org_id, None, None, "team", team_key, "delete", actor, None, None, None, None,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    // =========================================================================
    // Environment Mandatory Approval Settings & Approval Workflows
    // =========================================================================

    pub async fn update_environment_approval_settings(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        settings: &EnvironmentApprovalSettings,
        actor: &AuditActor,
    ) -> Result<Environment, StorageError> {
        if settings.min_approvals < 1 || settings.min_approvals > 5 {
            return Err(StorageError::Validation(DomainError::Validation(
                "minApprovals must be between 1 and 5".to_string(),
            )));
        }
        let project = self.get_project(org_id, project_key).await?;
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "UPDATE environments
             SET approval_required = $4,
                 min_approvals = $5,
                 allow_self_approval = $6,
                 external_approval_webhook_url = $7
             WHERE org_id = $1 AND project_id = $2 AND key = $3
             RETURNING id, project_id, key, name, color, client_side_id, require_comments, confirm_changes,
                       approval_required, min_approvals, allow_self_approval, external_approval_webhook_url,
                       revision, created_at",
        )
        .bind(org_id)
        .bind(project.id)
        .bind(env_key)
        .bind(settings.required)
        .bind(settings.min_approvals)
        .bind(settings.allow_self_approval)
        .bind(&settings.external_approval_webhook_url)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("environment '{env_key}'")))?;

        let env = parse_environment_row(&row)?;
        insert_audit_log(
            &mut tx,
            org_id,
            Some(project.id),
            Some(env.id),
            "environment",
            env_key,
            "update_approval_settings",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(settings).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(env)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_approval_request(
        &self,
        org_id: Uuid,
        project_key: &str,
        flag_key: &str,
        env_key: &str,
        description: &str,
        instructions: &[FlagPatchInstruction],
        comment: Option<&str>,
        actor: &AuditActor,
    ) -> Result<ApprovalRequest, StorageError> {
        if instructions.is_empty() {
            return Err(StorageError::Validation(DomainError::Validation(
                "approval request must contain at least one instruction".to_string(),
            )));
        }
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let flag = self.get_flag(org_id, project_key, flag_key).await?.flag;

        let id = Uuid::now_v7();
        let instr_json = serde_json::to_value(instructions)
            .map_err(|e| StorageError::Validation(DomainError::Validation(e.to_string())))?;

        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO approval_requests (
                id, org_id, project_key, env_key, flag_key, flag_id, env_id,
                requested_by, requested_by_email, description, instructions, comment,
                status, min_approvals
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 'pending', $13)",
        )
        .bind(id)
        .bind(org_id)
        .bind(project_key)
        .bind(env_key)
        .bind(flag_key)
        .bind(flag.id)
        .bind(env.id)
        .bind(actor.actor_id)
        .bind(&actor.actor_email)
        .bind(description.trim())
        .bind(&instr_json)
        .bind(comment)
        .bind(env.approval_settings.min_approvals.max(1))
        .execute(&mut *tx)
        .await?;

        insert_audit_log(
            &mut tx,
            org_id,
            Some(env.project_id),
            Some(env.id),
            "approval_request",
            &id.to_string(),
            "create",
            actor,
            comment,
            Some(instr_json),
            None,
            Some(json!({ "flagKey": flag_key, "envKey": env_key })),
        )
        .await?;
        tx.commit().await?;

        self.get_approval_request(org_id, id).await
    }

    pub async fn get_approval_request(
        &self,
        org_id: Uuid,
        approval_id: Uuid,
    ) -> Result<ApprovalRequest, StorageError> {
        let row = sqlx::query(
            "SELECT id, org_id, project_key, env_key, flag_key, requested_by, requested_by_email,
                    description, instructions, comment, status, min_approvals,
                    applied_version, applied_at, created_at, updated_at
             FROM approval_requests WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(approval_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("approval request '{approval_id}'")))?;

        self.hydrate_approval_row(&row).await
    }

    pub async fn list_approval_requests(
        &self,
        org_id: Uuid,
        status_filter: Option<&str>,
    ) -> Result<Vec<ApprovalRequest>, StorageError> {
        let rows = if let Some(st) = status_filter {
            sqlx::query(
                "SELECT id, org_id, project_key, env_key, flag_key, requested_by, requested_by_email,
                        description, instructions, comment, status, min_approvals,
                        applied_version, applied_at, created_at, updated_at
                 FROM approval_requests WHERE org_id = $1 AND status = $2
                 ORDER BY created_at DESC LIMIT 100",
            )
            .bind(org_id)
            .bind(st)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT id, org_id, project_key, env_key, flag_key, requested_by, requested_by_email,
                        description, instructions, comment, status, min_approvals,
                        applied_version, applied_at, created_at, updated_at
                 FROM approval_requests WHERE org_id = $1
                 ORDER BY created_at DESC LIMIT 100",
            )
            .bind(org_id)
            .fetch_all(&self.pool)
            .await?
        };

        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            items.push(self.hydrate_approval_row(&row).await?);
        }
        Ok(items)
    }

    pub async fn list_flag_approval_requests(
        &self,
        org_id: Uuid,
        project_key: &str,
        flag_key: &str,
        env_key: &str,
    ) -> Result<Vec<ApprovalRequest>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, org_id, project_key, env_key, flag_key, requested_by, requested_by_email,
                    description, instructions, comment, status, min_approvals,
                    applied_version, applied_at, created_at, updated_at
             FROM approval_requests
             WHERE org_id = $1 AND project_key = $2 AND flag_key = $3 AND env_key = $4
             ORDER BY created_at DESC LIMIT 50",
        )
        .bind(org_id)
        .bind(project_key)
        .bind(flag_key)
        .bind(env_key)
        .fetch_all(&self.pool)
        .await?;

        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            items.push(self.hydrate_approval_row(&row).await?);
        }
        Ok(items)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn review_approval_request(
        &self,
        org_id: Uuid,
        approval_id: Uuid,
        reviewer_id: Uuid,
        reviewer_email: Option<&str>,
        kind: ApprovalReviewKind,
        comment: Option<&str>,
        actor: &AuditActor,
    ) -> Result<ApprovalRequest, StorageError> {
        let current = self.get_approval_request(org_id, approval_id).await?;
        if current.status != ApprovalStatus::Pending && current.status != ApprovalStatus::Approved {
            return Err(StorageError::Validation(DomainError::Validation(format!(
                "cannot review approval request in status '{}'",
                current.status.as_str()
            ))));
        }

        let env = self
            .get_environment(org_id, &current.project_key, &current.env_key)
            .await?;
        if !env.approval_settings.allow_self_approval
            && current.requested_by == Some(reviewer_id)
            && kind == ApprovalReviewKind::Approve
        {
            return Err(StorageError::Validation(DomainError::Validation(
                "self-approval is disabled for this environment".to_string(),
            )));
        }

        let review_id = Uuid::now_v7();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO approval_reviews (id, org_id, approval_id, reviewer_id, reviewer_email, kind, comment)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (approval_id, reviewer_id)
             DO UPDATE SET kind = EXCLUDED.kind, comment = EXCLUDED.comment, created_at = now()",
        )
        .bind(review_id)
        .bind(org_id)
        .bind(approval_id)
        .bind(reviewer_id)
        .bind(reviewer_email)
        .bind(kind.as_str())
        .bind(comment)
        .execute(&mut *tx)
        .await?;

        let approve_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM approval_reviews WHERE approval_id = $1 AND kind = 'approve'",
        )
        .bind(approval_id)
        .fetch_one(&mut *tx)
        .await?;

        let decline_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM approval_reviews WHERE approval_id = $1 AND kind = 'decline'",
        )
        .bind(approval_id)
        .fetch_one(&mut *tx)
        .await?;

        let next_status = if decline_count > 0 {
            ApprovalStatus::Rejected
        } else if approve_count >= i64::from(current.min_approvals) {
            ApprovalStatus::Approved
        } else {
            ApprovalStatus::Pending
        };

        sqlx::query(
            "UPDATE approval_requests SET status = $3, updated_at = now()
             WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(approval_id)
        .bind(next_status.as_str())
        .execute(&mut *tx)
        .await?;

        insert_audit_log(
            &mut tx,
            org_id,
            Some(env.project_id),
            Some(env.id),
            "approval_request",
            &approval_id.to_string(),
            kind.as_str(),
            actor,
            comment,
            None,
            None,
            Some(json!({ "status": next_status.as_str(), "approveCount": approve_count })),
        )
        .await?;
        tx.commit().await?;

        self.get_approval_request(org_id, approval_id).await
    }

    pub async fn apply_approval_request(
        &self,
        org_id: Uuid,
        approval_id: Uuid,
        actor: &AuditActor,
    ) -> Result<(ApprovalRequest, PatchFlagOutcome), StorageError> {
        let req = self.get_approval_request(org_id, approval_id).await?;
        if req.status != ApprovalStatus::Approved {
            return Err(StorageError::Validation(DomainError::Validation(format!(
                "approval request must be in 'approved' status before applying (currently '{}', {}/{} approvals)",
                req.status.as_str(),
                req.approval_count,
                req.min_approvals
            ))));
        }

        let comment = format!(
            "Applied approval request {}: {}",
            req.id,
            req.comment.as_deref().unwrap_or(req.description.as_str())
        );
        let outcome = self
            .patch_flag(
                org_id,
                &req.project_key,
                &req.flag_key,
                &req.env_key,
                None,
                Some(&comment),
                &req.instructions,
                actor,
            )
            .await?;

        sqlx::query(
            "UPDATE approval_requests
             SET status = 'applied', applied_version = $3, applied_at = now(), updated_at = now()
             WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(approval_id)
        .bind(outcome.env_config.version)
        .execute(&self.pool)
        .await?;

        let updated = self.get_approval_request(org_id, approval_id).await?;
        Ok((updated, outcome))
    }

    pub async fn cancel_approval_request(
        &self,
        org_id: Uuid,
        approval_id: Uuid,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let res = sqlx::query(
            "UPDATE approval_requests SET status = 'canceled', updated_at = now()
             WHERE org_id = $1 AND id = $2 AND status IN ('pending', 'approved')",
        )
        .bind(org_id)
        .bind(approval_id)
        .execute(&mut *tx)
        .await?;
        if res.rows_affected() == 0 {
            return Err(StorageError::NotFound(format!(
                "pending approval request '{approval_id}'"
            )));
        }
        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "approval_request",
            &approval_id.to_string(),
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

    async fn hydrate_approval_row(
        &self,
        row: &sqlx::postgres::PgRow,
    ) -> Result<ApprovalRequest, StorageError> {
        let id: Uuid = row.get("id");
        let review_rows = sqlx::query(
            "SELECT id, approval_id, reviewer_id, reviewer_email, kind, comment, created_at
             FROM approval_reviews WHERE approval_id = $1 ORDER BY created_at ASC",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;

        let mut reviews = Vec::with_capacity(review_rows.len());
        let mut approve_count = 0i32;
        for rr in review_rows {
            let kind = match rr.get::<String, _>("kind").as_str() {
                "approve" => {
                    approve_count += 1;
                    ApprovalReviewKind::Approve
                }
                _ => ApprovalReviewKind::Decline,
            };
            reviews.push(ApprovalReview {
                id: rr.get("id"),
                approval_id: rr.get("approval_id"),
                reviewer_id: rr.get("reviewer_id"),
                reviewer_email: rr.get("reviewer_email"),
                kind,
                comment: rr.get("comment"),
                created_at: rr.get("created_at"),
            });
        }

        let status = match row.get::<String, _>("status").as_str() {
            "approved" => ApprovalStatus::Approved,
            "rejected" => ApprovalStatus::Rejected,
            "applied" => ApprovalStatus::Applied,
            "canceled" => ApprovalStatus::Canceled,
            _ => ApprovalStatus::Pending,
        };
        let instructions: Vec<FlagPatchInstruction> =
            serde_json::from_value(row.get("instructions")).unwrap_or_default();

        Ok(ApprovalRequest {
            id,
            org_id: row.get("org_id"),
            project_key: row.get("project_key"),
            env_key: row.get("env_key"),
            flag_key: row.get("flag_key"),
            requested_by: row.get("requested_by"),
            requested_by_email: row.get("requested_by_email"),
            description: row.get("description"),
            instructions,
            comment: row.get("comment"),
            status,
            min_approvals: row.get("min_approvals"),
            approval_count: approve_count,
            reviews,
            applied_version: row.get("applied_version"),
            applied_at: row.get("applied_at"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }

    // =========================================================================
    // SSO / SCIM & Audit Archives
    // =========================================================================

    pub async fn list_sso_providers(
        &self,
        org_id: Uuid,
    ) -> Result<Vec<SsoProviderConfig>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, org_id, protocol, name, issuer_url, client_id, jwks_url, saml_metadata_url,
                    enabled, scim_enabled, created_at, updated_at
             FROM sso_providers WHERE org_id = $1 ORDER BY created_at DESC",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(parse_sso_row).collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_sso_provider(
        &self,
        org_id: Uuid,
        protocol: SsoProtocol,
        name: &str,
        issuer_url: &str,
        client_id: Option<&str>,
        jwks_url: Option<&str>,
        saml_metadata_url: Option<&str>,
        scim_enabled: bool,
        actor: &AuditActor,
    ) -> Result<SsoProviderConfig, StorageError> {
        if !issuer_url.starts_with("https://") && !issuer_url.starts_with("http://") {
            return Err(StorageError::Validation(DomainError::Validation(
                "SSO issuerUrl must start with http:// or https://".to_string(),
            )));
        }
        let id = Uuid::now_v7();
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO sso_providers (
                id, org_id, protocol, name, issuer_url, client_id, jwks_url, saml_metadata_url, enabled, scim_enabled
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, true, $9)
             RETURNING id, org_id, protocol, name, issuer_url, client_id, jwks_url, saml_metadata_url,
                       enabled, scim_enabled, created_at, updated_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(protocol.as_str())
        .bind(name.trim())
        .bind(issuer_url.trim())
        .bind(client_id)
        .bind(jwks_url)
        .bind(saml_metadata_url)
        .bind(scim_enabled)
        .fetch_one(&mut *tx)
        .await?;
        let cfg = parse_sso_row(&row);
        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "sso_provider",
            &id.to_string(),
            "create",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&cfg).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(cfg)
    }

    pub async fn delete_sso_provider(
        &self,
        org_id: Uuid,
        provider_id: Uuid,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let res = sqlx::query("DELETE FROM sso_providers WHERE org_id = $1 AND id = $2")
            .bind(org_id)
            .bind(provider_id)
            .execute(&mut *tx)
            .await?;
        if res.rows_affected() == 0 {
            return Err(StorageError::NotFound(format!(
                "sso provider '{provider_id}'"
            )));
        }
        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "sso_provider",
            &provider_id.to_string(),
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

    pub async fn scim_upsert_member(
        &self,
        org_id: Uuid,
        email: &str,
        display_name: &str,
        external_subject_id: Option<&str>,
        actor: &AuditActor,
    ) -> Result<Member, StorageError> {
        let email_norm = email.trim().to_lowercase();
        let id = Uuid::now_v7();
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO members (id, org_id, email, display_name, role, external_subject_id)
             VALUES ($1, $2, $3, $4, 'reader', $5)
             ON CONFLICT (org_id, email) DO UPDATE
             SET display_name = EXCLUDED.display_name,
                 external_subject_id = COALESCE(EXCLUDED.external_subject_id, members.external_subject_id)
             RETURNING id, org_id, email, display_name, role, external_subject_id, created_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(&email_norm)
        .bind(display_name.trim())
        .bind(external_subject_id)
        .fetch_one(&mut *tx)
        .await?;

        let member = Member {
            id: row.get("id"),
            org_id: row.get("org_id"),
            email: row.get("email"),
            display_name: row.get("display_name"),
            role: match row.get::<String, _>("role").as_str() {
                "owner" => BuiltInRole::Owner,
                "admin" => BuiltInRole::Admin,
                "writer" => BuiltInRole::Writer,
                _ => BuiltInRole::Reader,
            },
            external_subject_id: row.get("external_subject_id"),
            custom_role_keys: Vec::new(),
            created_at: row.get("created_at"),
        };

        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "member",
            &member.email,
            "scim_provision",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&member).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(member)
    }

    pub async fn list_audit_logs_in_range(
        &self,
        org_id: Uuid,
        from_ts: DateTime<Utc>,
        to_ts: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<AuditLogEntry>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, org_id, project_id, env_id, resource_type, resource_key, action,
                    actor_id, actor_type, actor_email, comment, instructions_jsonb, before_jsonb, after_jsonb, created_at
             FROM audit_logs
             WHERE org_id = $1 AND created_at >= $2 AND created_at <= $3
             ORDER BY created_at ASC
             LIMIT $4",
        )
        .bind(org_id)
        .bind(from_ts)
        .bind(to_ts)
        .bind(limit.clamp(1, 50_000))
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| AuditLogEntry {
                id: r.get("id"),
                org_id: r.get("org_id"),
                project_id: r.get("project_id"),
                env_id: r.get("env_id"),
                resource_type: r.get("resource_type"),
                resource_key: r.get("resource_key"),
                action: r.get("action"),
                actor_id: r.get("actor_id"),
                actor_type: r.get("actor_type"),
                actor_email: r.get("actor_email"),
                comment: r.get("comment"),
                instructions: r.get("instructions_jsonb"),
                before: r.get("before_jsonb"),
                after: r.get("after_jsonb"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_audit_archive(
        &self,
        org_id: Uuid,
        object_key: &str,
        format: &str,
        record_count: i64,
        compressed_bytes: i64,
        sha256: &str,
        from_ts: DateTime<Utc>,
        to_ts: DateTime<Utc>,
    ) -> Result<AuditArchiveRecord, StorageError> {
        let id = Uuid::now_v7();
        let row = sqlx::query(
            "INSERT INTO audit_archives (
                id, org_id, object_key, format, record_count, compressed_bytes, sha256, from_timestamp, to_timestamp
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             RETURNING id, org_id, object_key, format, record_count, compressed_bytes, sha256,
                       from_timestamp, to_timestamp, created_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(object_key)
        .bind(format)
        .bind(record_count)
        .bind(compressed_bytes)
        .bind(sha256)
        .bind(from_ts)
        .bind(to_ts)
        .fetch_one(&self.pool)
        .await?;

        Ok(AuditArchiveRecord {
            id: row.get("id"),
            org_id: row.get("org_id"),
            object_key: row.get("object_key"),
            format: row.get("format"),
            record_count: row.get("record_count"),
            compressed_bytes: row.get("compressed_bytes"),
            sha256: row.get("sha256"),
            from_timestamp: row.get("from_timestamp"),
            to_timestamp: row.get("to_timestamp"),
            created_at: row.get("created_at"),
        })
    }

    pub async fn list_audit_archives(
        &self,
        org_id: Uuid,
    ) -> Result<Vec<AuditArchiveRecord>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, org_id, object_key, format, record_count, compressed_bytes, sha256,
                    from_timestamp, to_timestamp, created_at
             FROM audit_archives WHERE org_id = $1 ORDER BY created_at DESC LIMIT 50",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| AuditArchiveRecord {
                id: row.get("id"),
                org_id: row.get("org_id"),
                object_key: row.get("object_key"),
                format: row.get("format"),
                record_count: row.get("record_count"),
                compressed_bytes: row.get("compressed_bytes"),
                sha256: row.get("sha256"),
                from_timestamp: row.get("from_timestamp"),
                to_timestamp: row.get("to_timestamp"),
                created_at: row.get("created_at"),
            })
            .collect())
    }

    // =========================================================================
    // Multi-Step Flag Workflows
    // =========================================================================

    #[allow(clippy::too_many_arguments)]
    pub async fn create_flag_workflow(
        &self,
        org_id: Uuid,
        project_key: &str,
        flag_key: &str,
        env_key: &str,
        name: &str,
        description: &str,
        mut stages: Vec<WorkflowStage>,
        actor: &AuditActor,
    ) -> Result<FlagWorkflow, StorageError> {
        if stages.is_empty() {
            return Err(StorageError::Validation(DomainError::Validation(
                "workflow requires at least one stage".to_string(),
            )));
        }
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let flag = self.get_flag(org_id, project_key, flag_key).await?.flag;

        for st in &mut stages {
            st.status = WorkflowStageStatus::Pending;
            st.executed_at = None;
            st.error_message = None;
        }
        let initial_wait = stages.first().map(|s| s.wait_seconds.max(0)).unwrap_or(0);
        let next_run_at = Utc::now() + Duration::seconds(initial_wait);
        let id = Uuid::now_v7();
        let stages_json = serde_json::to_value(&stages)
            .map_err(|e| StorageError::Validation(DomainError::Validation(e.to_string())))?;

        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO flag_workflows (
                id, org_id, project_key, env_key, flag_key, flag_id, env_id,
                name, description, status, current_stage_index, next_run_at, stages, created_by
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'active', 0, $10, $11, $12)
             RETURNING id, org_id, project_key, env_key, flag_key, name, description, status,
                       current_stage_index, next_run_at, stages, created_by, created_at, updated_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(project_key)
        .bind(env_key)
        .bind(flag_key)
        .bind(flag.id)
        .bind(env.id)
        .bind(name.trim())
        .bind(description.trim())
        .bind(next_run_at)
        .bind(&stages_json)
        .bind(actor.actor_id)
        .fetch_one(&mut *tx)
        .await?;

        let wf = parse_workflow_row(&row);
        insert_audit_log(
            &mut tx,
            org_id,
            Some(env.project_id),
            Some(env.id),
            "flag_workflow",
            &id.to_string(),
            "create",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&wf).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(wf)
    }

    pub async fn list_flag_workflows(
        &self,
        org_id: Uuid,
        project_key: &str,
        flag_key: &str,
        env_key: &str,
    ) -> Result<Vec<FlagWorkflow>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, org_id, project_key, env_key, flag_key, name, description, status,
                    current_stage_index, next_run_at, stages, created_by, created_at, updated_at
             FROM flag_workflows
             WHERE org_id = $1 AND project_key = $2 AND flag_key = $3 AND env_key = $4
             ORDER BY created_at DESC",
        )
        .bind(org_id)
        .bind(project_key)
        .bind(flag_key)
        .bind(env_key)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(parse_workflow_row).collect())
    }

    pub async fn list_due_workflows(&self, limit: i64) -> Result<Vec<FlagWorkflow>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, org_id, project_key, env_key, flag_key, name, description, status,
                    current_stage_index, next_run_at, stages, created_by, created_at, updated_at
             FROM flag_workflows
             WHERE status = 'active' AND next_run_at <= now()
             ORDER BY next_run_at ASC
             LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(parse_workflow_row).collect())
    }

    /// Executes the current stage of a workflow (advancing past approval gate when `approve_gate = true`).
    pub async fn advance_flag_workflow(
        &self,
        org_id: Uuid,
        workflow_id: Uuid,
        approve_gate: bool,
        actor: &AuditActor,
    ) -> Result<FlagWorkflow, StorageError> {
        let row = sqlx::query(
            "SELECT id, org_id, project_key, env_key, flag_key, name, description, status,
                    current_stage_index, next_run_at, stages, created_by, created_at, updated_at
             FROM flag_workflows WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(workflow_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("workflow '{workflow_id}'")))?;

        let mut wf = parse_workflow_row(&row);
        if wf.status == WorkflowStatus::Completed || wf.status == WorkflowStatus::Canceled {
            return Ok(wf);
        }

        let idx = wf.current_stage_index as usize;
        if idx >= wf.stages.len() {
            wf.status = WorkflowStatus::Completed;
            return self.save_workflow_state(&wf).await;
        }

        if wf.stages[idx].require_approval && !approve_gate {
            wf.stages[idx].status = WorkflowStageStatus::WaitingApproval;
            wf.status = WorkflowStatus::WaitingApproval;
            return self.save_workflow_state(&wf).await;
        }

        let stage_name = wf.stages[idx].name.clone();
        let instructions = wf.stages[idx].instructions.clone();
        let comment = format!("Workflow '{}' stage #{}: {}", wf.name, idx + 1, stage_name);

        match self
            .patch_flag(
                org_id,
                &wf.project_key,
                &wf.flag_key,
                &wf.env_key,
                None,
                Some(&comment),
                &instructions,
                actor,
            )
            .await
        {
            Ok(_) => {
                wf.stages[idx].status = WorkflowStageStatus::Executed;
                wf.stages[idx].executed_at = Some(Utc::now());
                wf.stages[idx].error_message = None;
                let next_idx = idx + 1;
                wf.current_stage_index = next_idx as i32;
                if next_idx >= wf.stages.len() {
                    wf.status = WorkflowStatus::Completed;
                } else {
                    wf.status = WorkflowStatus::Active;
                    let wait = wf.stages[next_idx].wait_seconds.max(0);
                    wf.next_run_at = Utc::now() + Duration::seconds(wait);
                }
            }
            Err(err) => {
                wf.stages[idx].status = WorkflowStageStatus::Failed;
                wf.stages[idx].error_message = Some(err.to_string());
                wf.status = WorkflowStatus::Failed;
            }
        }

        self.save_workflow_state(&wf).await
    }

    pub async fn cancel_flag_workflow(
        &self,
        org_id: Uuid,
        workflow_id: Uuid,
    ) -> Result<(), StorageError> {
        let res = sqlx::query(
            "UPDATE flag_workflows SET status = 'canceled', updated_at = now()
             WHERE org_id = $1 AND id = $2 AND status IN ('active', 'waiting_approval')",
        )
        .bind(org_id)
        .bind(workflow_id)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(StorageError::NotFound(format!(
                "active workflow '{workflow_id}'"
            )));
        }
        Ok(())
    }

    async fn save_workflow_state(&self, wf: &FlagWorkflow) -> Result<FlagWorkflow, StorageError> {
        let stages_json = serde_json::to_value(&wf.stages)
            .map_err(|e| StorageError::Validation(DomainError::Validation(e.to_string())))?;
        let row = sqlx::query(
            "UPDATE flag_workflows
             SET status = $3,
                 current_stage_index = $4,
                 next_run_at = $5,
                 stages = $6,
                 updated_at = now()
             WHERE org_id = $1 AND id = $2
             RETURNING id, org_id, project_key, env_key, flag_key, name, description, status,
                       current_stage_index, next_run_at, stages, created_by, created_at, updated_at",
        )
        .bind(wf.org_id)
        .bind(wf.id)
        .bind(wf.status.as_str())
        .bind(wf.current_stage_index)
        .bind(wf.next_run_at)
        .bind(&stages_json)
        .fetch_one(&self.pool)
        .await?;
        Ok(parse_workflow_row(&row))
    }

    // =========================================================================
    // Release Pipelines & Flag Releases
    // =========================================================================

    pub async fn list_release_pipelines(
        &self,
        org_id: Uuid,
        project_key: &str,
    ) -> Result<Vec<ReleasePipeline>, StorageError> {
        let project = self.get_project(org_id, project_key).await?;
        let rows = sqlx::query(
            "SELECT id, org_id, project_id, key, name, description, phases, created_at, updated_at
             FROM release_pipelines WHERE org_id = $1 AND project_id = $2 ORDER BY created_at ASC",
        )
        .bind(org_id)
        .bind(project.id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(parse_pipeline_row).collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_release_pipeline(
        &self,
        org_id: Uuid,
        project_key: &str,
        key: &str,
        name: &str,
        description: &str,
        phases: &[ReleasePhase],
        actor: &AuditActor,
    ) -> Result<ReleasePipeline, StorageError> {
        validate_key(key)?;
        if phases.is_empty() {
            return Err(StorageError::Validation(DomainError::Validation(
                "release pipeline requires at least one phase".to_string(),
            )));
        }
        let project = self.get_project(org_id, project_key).await?;
        let id = Uuid::now_v7();
        let phases_json = serde_json::to_value(phases)
            .map_err(|e| StorageError::Validation(DomainError::Validation(e.to_string())))?;

        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO release_pipelines (id, org_id, project_id, key, name, description, phases)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING id, org_id, project_id, key, name, description, phases, created_at, updated_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(project.id)
        .bind(key)
        .bind(name.trim())
        .bind(description.trim())
        .bind(&phases_json)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_unique_err("release_pipeline"))?;

        let pipeline = parse_pipeline_row(&row);
        insert_audit_log(
            &mut tx,
            org_id,
            Some(project.id),
            None,
            "release_pipeline",
            key,
            "create",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&pipeline).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(pipeline)
    }

    pub async fn delete_release_pipeline(
        &self,
        org_id: Uuid,
        project_key: &str,
        pipeline_key: &str,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let project = self.get_project(org_id, project_key).await?;
        let mut tx = self.pool.begin().await?;
        let res = sqlx::query(
            "DELETE FROM release_pipelines WHERE org_id = $1 AND project_id = $2 AND key = $3",
        )
        .bind(org_id)
        .bind(project.id)
        .bind(pipeline_key)
        .execute(&mut *tx)
        .await?;
        if res.rows_affected() == 0 {
            return Err(StorageError::NotFound(format!(
                "release pipeline '{pipeline_key}'"
            )));
        }
        insert_audit_log(
            &mut tx,
            org_id,
            Some(project.id),
            None,
            "release_pipeline",
            pipeline_key,
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

    pub async fn attach_flag_release(
        &self,
        org_id: Uuid,
        project_key: &str,
        flag_key: &str,
        pipeline_key: &str,
        actor: &AuditActor,
    ) -> Result<FlagRelease, StorageError> {
        let project = self.get_project(org_id, project_key).await?;
        let flag = self.get_flag(org_id, project_key, flag_key).await?.flag;
        let pipe_row = sqlx::query(
            "SELECT id, org_id, project_id, key, name, description, phases, created_at, updated_at
             FROM release_pipelines WHERE org_id = $1 AND project_id = $2 AND key = $3",
        )
        .bind(org_id)
        .bind(project.id)
        .bind(pipeline_key)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("release pipeline '{pipeline_key}'")))?;
        let pipeline = parse_pipeline_row(&pipe_row);

        let phase_states: Vec<FlagReleasePhaseState> = pipeline
            .phases
            .iter()
            .enumerate()
            .map(|(idx, p)| FlagReleasePhaseState {
                phase_index: idx as i32,
                name: p.name.clone(),
                environment_key: p.environment_key.clone(),
                status: "not_started".to_string(),
                completed_at: None,
            })
            .collect();
        let phases_json = serde_json::to_value(&phase_states).unwrap_or(Value::Array(vec![]));
        let id = Uuid::now_v7();

        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO flag_releases (
                id, org_id, project_key, flag_key, flag_id, pipeline_id, pipeline_key, pipeline_name,
                status, current_phase_index, phases
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'active', 0, $9)
             ON CONFLICT (flag_id) DO UPDATE
             SET pipeline_id = EXCLUDED.pipeline_id,
                 pipeline_key = EXCLUDED.pipeline_key,
                 pipeline_name = EXCLUDED.pipeline_name,
                 status = 'active',
                 current_phase_index = 0,
                 phases = EXCLUDED.phases,
                 updated_at = now()
             RETURNING id, org_id, project_key, flag_key, pipeline_key, pipeline_name,
                       status, current_phase_index, phases, created_at, updated_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(project_key)
        .bind(flag_key)
        .bind(flag.id)
        .bind(pipeline.id)
        .bind(&pipeline.key)
        .bind(&pipeline.name)
        .bind(&phases_json)
        .fetch_one(&mut *tx)
        .await?;

        let release = parse_flag_release_row(&row);
        insert_audit_log(
            &mut tx,
            org_id,
            Some(project.id),
            None,
            "flag_release",
            flag_key,
            "attach_pipeline",
            actor,
            None,
            None,
            None,
            Some(json!({ "pipelineKey": pipeline.key })),
        )
        .await?;
        tx.commit().await?;
        Ok(release)
    }

    pub async fn get_flag_release(
        &self,
        org_id: Uuid,
        project_key: &str,
        flag_key: &str,
    ) -> Result<Option<FlagRelease>, StorageError> {
        let row = sqlx::query(
            "SELECT id, org_id, project_key, flag_key, pipeline_key, pipeline_name,
                    status, current_phase_index, phases, created_at, updated_at
             FROM flag_releases WHERE org_id = $1 AND project_key = $2 AND flag_key = $3",
        )
        .bind(org_id)
        .bind(project_key)
        .bind(flag_key)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(parse_flag_release_row))
    }

    pub async fn start_flag_release_phase(
        &self,
        org_id: Uuid,
        project_key: &str,
        flag_key: &str,
        phase_index: usize,
        actor: &AuditActor,
    ) -> Result<FlagRelease, StorageError> {
        let mut release = self
            .get_flag_release(org_id, project_key, flag_key)
            .await?
            .ok_or_else(|| StorageError::NotFound(format!("release for flag '{flag_key}'")))?;
        let project = self.get_project(org_id, project_key).await?;
        let pipe_row = sqlx::query(
            "SELECT id, org_id, project_id, key, name, description, phases, created_at, updated_at
             FROM release_pipelines WHERE org_id = $1 AND project_id = $2 AND key = $3",
        )
        .bind(org_id)
        .bind(project.id)
        .bind(&release.pipeline_key)
        .fetch_one(&self.pool)
        .await?;
        let pipeline = parse_pipeline_row(&pipe_row);

        let phase = pipeline.phases.get(phase_index).ok_or_else(|| {
            StorageError::Validation(DomainError::Validation(format!(
                "phase index {phase_index} out of bounds"
            )))
        })?;

        if phase.copy_from_previous_env && phase_index > 0 {
            if let Some(prev) = pipeline.phases.get(phase_index - 1) {
                let _ = self
                    .copy_flag_env_config(
                        org_id,
                        project_key,
                        flag_key,
                        &prev.environment_key,
                        &phase.environment_key,
                        false,
                        None,
                        Some(&format!(
                            "Release pipeline '{}' phase '{}'",
                            pipeline.key, phase.name
                        )),
                        actor,
                    )
                    .await?;
            }
        }

        let instrs = if phase.instructions.is_empty() {
            vec![FlagPatchInstruction::TurnFlagOn]
        } else {
            phase.instructions.clone()
        };
        let _ = self
            .patch_flag(
                org_id,
                project_key,
                flag_key,
                &phase.environment_key,
                None,
                Some(&format!(
                    "Release pipeline '{}' phase '{}'",
                    pipeline.key, phase.name
                )),
                &instrs,
                actor,
            )
            .await?;

        if let Some(st) = release.phases.get_mut(phase_index) {
            st.status = "completed".to_string();
            st.completed_at = Some(Utc::now());
        }
        release.current_phase_index = (phase_index + 1) as i32;
        if release.phases.iter().all(|p| p.status == "completed") {
            release.status = "completed".to_string();
        }

        let phases_json = serde_json::to_value(&release.phases).unwrap_or(Value::Array(vec![]));
        let row = sqlx::query(
            "UPDATE flag_releases
             SET status = $4, current_phase_index = $5, phases = $6, updated_at = now()
             WHERE org_id = $1 AND project_key = $2 AND flag_key = $3
             RETURNING id, org_id, project_key, flag_key, pipeline_key, pipeline_name,
                       status, current_phase_index, phases, created_at, updated_at",
        )
        .bind(org_id)
        .bind(project_key)
        .bind(flag_key)
        .bind(&release.status)
        .bind(release.current_phase_index)
        .bind(&phases_json)
        .fetch_one(&self.pool)
        .await?;
        Ok(parse_flag_release_row(&row))
    }

    // =========================================================================
    // Applications & App Versions
    // =========================================================================

    pub async fn list_applications(&self, org_id: Uuid) -> Result<Vec<Application>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, org_id, key, name, kind, description, maintainer_team_id, created_at, updated_at
             FROM applications WHERE org_id = $1 ORDER BY key ASC",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await?;

        let mut apps = Vec::with_capacity(rows.len());
        for r in rows {
            let app_id: Uuid = r.get("id");
            let vrows = sqlx::query(
                "SELECT id, version_key, name, supported, created_at
                 FROM application_versions WHERE app_id = $1 ORDER BY created_at DESC",
            )
            .bind(app_id)
            .fetch_all(&self.pool)
            .await?;
            let versions = vrows
                .into_iter()
                .map(|vr| ApplicationVersion {
                    id: vr.get("id"),
                    version_key: vr.get("version_key"),
                    name: vr.get("name"),
                    supported: vr.get("supported"),
                    created_at: vr.get("created_at"),
                })
                .collect();
            apps.push(Application {
                id: app_id,
                org_id: r.get("org_id"),
                key: r.get("key"),
                name: r.get("name"),
                kind: parse_app_kind(r.get::<String, _>("kind").as_str()),
                description: r.get("description"),
                maintainer_team_id: r.get("maintainer_team_id"),
                versions,
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            });
        }
        Ok(apps)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_application(
        &self,
        org_id: Uuid,
        key: &str,
        name: &str,
        kind: ApplicationKind,
        description: &str,
        maintainer_team_id: Option<Uuid>,
        actor: &AuditActor,
    ) -> Result<Application, StorageError> {
        validate_key(key)?;
        let id = Uuid::now_v7();
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO applications (id, org_id, key, name, kind, description, maintainer_team_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING id, org_id, key, name, kind, description, maintainer_team_id, created_at, updated_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(key)
        .bind(name.trim())
        .bind(kind.as_str())
        .bind(description.trim())
        .bind(maintainer_team_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_unique_err("application"))?;

        let app = Application {
            id: row.get("id"),
            org_id: row.get("org_id"),
            key: row.get("key"),
            name: row.get("name"),
            kind,
            description: row.get("description"),
            maintainer_team_id: row.get("maintainer_team_id"),
            versions: vec![],
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        };
        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "application",
            key,
            "create",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&app).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(app)
    }

    pub async fn add_application_version(
        &self,
        org_id: Uuid,
        app_key: &str,
        version_key: &str,
        name: &str,
        supported: bool,
    ) -> Result<ApplicationVersion, StorageError> {
        let app_id: Uuid =
            sqlx::query_scalar("SELECT id FROM applications WHERE org_id = $1 AND key = $2")
                .bind(org_id)
                .bind(app_key)
                .fetch_optional(&self.pool)
                .await?
                .ok_or_else(|| StorageError::NotFound(format!("application '{app_key}'")))?;
        let id = Uuid::now_v7();
        let row = sqlx::query(
            "INSERT INTO application_versions (id, org_id, app_id, version_key, name, supported)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (app_id, version_key)
             DO UPDATE SET name = EXCLUDED.name, supported = EXCLUDED.supported
             RETURNING id, version_key, name, supported, created_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(app_id)
        .bind(version_key.trim())
        .bind(name.trim())
        .bind(supported)
        .fetch_one(&self.pool)
        .await?;

        Ok(ApplicationVersion {
            id: row.get("id"),
            version_key: row.get("version_key"),
            name: row.get("name"),
            supported: row.get("supported"),
            created_at: row.get("created_at"),
        })
    }

    // =========================================================================
    // Big Segments (High-Scale Compressed Membership) & Scheduled Segment Changes
    // =========================================================================

    #[allow(clippy::too_many_arguments)]
    pub async fn mutate_big_segment_members(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        segment_key: &str,
        context_kind: &str,
        add_included: &[String],
        remove_included: &[String],
        add_excluded: &[String],
        remove_excluded: &[String],
        actor: &AuditActor,
    ) -> Result<BigSegmentStatus, StorageError> {
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let seg = self
            .get_segment(org_id, project_key, env_key, segment_key)
            .await?;
        let ck = if context_kind.trim().is_empty() {
            "user"
        } else {
            context_kind.trim()
        };

        let mut tx = self.pool.begin().await?;
        // Ensure segment kind is marked 'big'
        let next_ver: i64 = sqlx::query_scalar(
            "UPDATE segments
             SET kind = 'big', version = version + 1, updated_at = now()
             WHERE org_id = $1 AND id = $2
             RETURNING version",
        )
        .bind(org_id)
        .bind(seg.id)
        .fetch_one(&mut *tx)
        .await?;

        for key in add_included {
            let h = big_segment_key_hash(ck, key) as i64;
            sqlx::query(
                "INSERT INTO big_segment_members (org_id, segment_id, env_id, context_kind, context_key, key_hash, included)
                 VALUES ($1, $2, $3, $4, $5, $6, true)
                 ON CONFLICT (segment_id, context_kind, context_key)
                 DO UPDATE SET included = true, key_hash = EXCLUDED.key_hash, updated_at = now()",
            )
            .bind(org_id)
            .bind(seg.id)
            .bind(env.id)
            .bind(ck)
            .bind(key)
            .bind(h)
            .execute(&mut *tx)
            .await?;
        }

        for key in add_excluded {
            let h = big_segment_key_hash(ck, key) as i64;
            sqlx::query(
                "INSERT INTO big_segment_members (org_id, segment_id, env_id, context_kind, context_key, key_hash, included)
                 VALUES ($1, $2, $3, $4, $5, $6, false)
                 ON CONFLICT (segment_id, context_kind, context_key)
                 DO UPDATE SET included = false, key_hash = EXCLUDED.key_hash, updated_at = now()",
            )
            .bind(org_id)
            .bind(seg.id)
            .bind(env.id)
            .bind(ck)
            .bind(key)
            .bind(h)
            .execute(&mut *tx)
            .await?;
        }

        for key in remove_included.iter().chain(remove_excluded.iter()) {
            sqlx::query(
                "DELETE FROM big_segment_members
                 WHERE segment_id = $1 AND context_kind = $2 AND context_key = $3",
            )
            .bind(seg.id)
            .bind(ck)
            .bind(key)
            .execute(&mut *tx)
            .await?;
        }

        let new_env_rev: i64 = sqlx::query_scalar(
            "UPDATE environments SET revision = revision + 1 WHERE id = $1 RETURNING revision",
        )
        .bind(env.id)
        .fetch_one(&mut *tx)
        .await?;

        insert_outbox_event(
            &mut tx,
            org_id,
            Some(env.id),
            "big_segment.updated",
            json!({
                "envId": env.id,
                "envKey": env.key,
                "segmentId": seg.id,
                "segmentKey": seg.key,
                "contextKind": ck,
                "generation": next_ver,
                "envRevision": new_env_rev,
            }),
        )
        .await?;

        insert_audit_log(
            &mut tx,
            org_id,
            Some(env.project_id),
            Some(env.id),
            "big_segment",
            segment_key,
            "mutate_members",
            actor,
            None,
            None,
            None,
            Some(json!({
                "addIncluded": add_included.len(),
                "removeIncluded": remove_included.len(),
                "addExcluded": add_excluded.len(),
                "removeExcluded": remove_excluded.len(),
                "generation": next_ver,
            })),
        )
        .await?;

        let included_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM big_segment_members WHERE segment_id = $1 AND included = true",
        )
        .bind(seg.id)
        .fetch_one(&mut *tx)
        .await?;
        let excluded_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM big_segment_members WHERE segment_id = $1 AND included = false",
        )
        .bind(seg.id)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(BigSegmentStatus {
            segment_key: seg.key,
            generation: next_ver,
            included_count,
            excluded_count,
            synced_to_object_store: true,
        })
    }

    pub async fn build_big_segment_snapshot(
        &self,
        env_id: Uuid,
        segment_key: &str,
        context_kind: &str,
    ) -> Result<Option<BigSegmentSnapshot>, StorageError> {
        let seg_row = sqlx::query(
            "SELECT id, version FROM segments WHERE env_id = $1 AND key = $2 AND kind = 'big'",
        )
        .bind(env_id)
        .bind(segment_key)
        .fetch_optional(&self.pool)
        .await?;
        let Some(sr) = seg_row else {
            return Ok(None);
        };
        let segment_id: Uuid = sr.get("id");
        let generation: i64 = sr.get("version");

        let rows = sqlx::query(
            "SELECT key_hash, included FROM big_segment_members
             WHERE segment_id = $1 AND context_kind = $2",
        )
        .bind(segment_id)
        .bind(context_kind)
        .fetch_all(&self.pool)
        .await?;

        let mut included_hashes = Vec::new();
        let mut excluded_hashes = Vec::new();
        for r in rows {
            let h = r.get::<i64, _>("key_hash") as u64;
            if r.get::<bool, _>("included") {
                included_hashes.push(h);
            } else {
                excluded_hashes.push(h);
            }
        }
        included_hashes.sort_unstable();
        included_hashes.dedup();
        excluded_hashes.sort_unstable();
        excluded_hashes.dedup();

        Ok(Some(BigSegmentSnapshot {
            env_id,
            segment_key: segment_key.to_string(),
            context_kind: context_kind.to_string(),
            generation,
            included_hashes,
            excluded_hashes,
        }))
    }

    pub async fn list_env_big_segment_snapshots(
        &self,
        env_id: Uuid,
    ) -> Result<Vec<BigSegmentSnapshot>, StorageError> {
        let seg_rows =
            sqlx::query("SELECT key FROM segments WHERE env_id = $1 AND kind = 'big' ORDER BY key")
                .bind(env_id)
                .fetch_all(&self.pool)
                .await?;
        let mut out = Vec::new();
        for sr in seg_rows {
            let key: String = sr.get("key");
            if let Some(snap) = self
                .build_big_segment_snapshot(env_id, &key, "user")
                .await?
            {
                out.push(snap);
            }
        }
        Ok(out)
    }

    pub async fn check_big_segment_member(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        segment_key: &str,
        context_kind: &str,
        context_key: &str,
    ) -> Result<Option<bool>, StorageError> {
        let seg = self
            .get_segment(org_id, project_key, env_key, segment_key)
            .await?;
        let res: Option<bool> = sqlx::query_scalar(
            "SELECT included FROM big_segment_members
             WHERE segment_id = $1 AND context_kind = $2 AND context_key = $3",
        )
        .bind(seg.id)
        .bind(context_kind)
        .bind(context_key)
        .fetch_optional(&self.pool)
        .await?;
        Ok(res)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_scheduled_segment_change(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        segment_key: &str,
        execute_at: DateTime<Utc>,
        instructions: &[SegmentPatchInstruction],
        comment: Option<&str>,
        actor: &AuditActor,
    ) -> Result<ScheduledSegmentChange, StorageError> {
        if instructions.is_empty() {
            return Err(StorageError::Validation(DomainError::Validation(
                "scheduled segment change requires at least one instruction".to_string(),
            )));
        }
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let seg = self
            .get_segment(org_id, project_key, env_key, segment_key)
            .await?;
        let id = Uuid::now_v7();
        let instr_json = serde_json::to_value(instructions)
            .map_err(|e| StorageError::Validation(DomainError::Validation(e.to_string())))?;

        let row = sqlx::query(
            "INSERT INTO scheduled_segment_changes (
                id, org_id, segment_id, env_id, project_key, env_key, segment_key,
                execute_at, instructions, comment, created_by, status
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'pending')
             RETURNING id, org_id, segment_id, env_id, execute_at, instructions, comment,
                       created_by, status, error_message, executed_at, created_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(seg.id)
        .bind(env.id)
        .bind(project_key)
        .bind(env_key)
        .bind(segment_key)
        .bind(execute_at)
        .bind(&instr_json)
        .bind(comment)
        .bind(actor.actor_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(parse_scheduled_segment_row(&row))
    }

    pub async fn list_scheduled_segment_changes(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        segment_key: &str,
    ) -> Result<Vec<ScheduledSegmentChange>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, org_id, segment_id, env_id, execute_at, instructions, comment,
                    created_by, status, error_message, executed_at, created_at
             FROM scheduled_segment_changes
             WHERE org_id = $1 AND project_key = $2 AND env_key = $3 AND segment_key = $4
             ORDER BY execute_at ASC",
        )
        .bind(org_id)
        .bind(project_key)
        .bind(env_key)
        .bind(segment_key)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(parse_scheduled_segment_row).collect())
    }

    pub async fn claim_due_scheduled_segment_changes(
        &self,
        limit: i64,
    ) -> Result<Vec<DueScheduledSegmentChange>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, org_id, segment_id, env_id, project_key, env_key, segment_key,
                    execute_at, instructions, comment, created_by, status, error_message, executed_at, created_at
             FROM scheduled_segment_changes
             WHERE status = 'pending' AND execute_at <= now()
             ORDER BY execute_at ASC
             LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| DueScheduledSegmentChange {
                change: parse_scheduled_segment_row(r),
                project_key: r.get("project_key"),
                env_key: r.get("env_key"),
                segment_key: r.get("segment_key"),
            })
            .collect())
    }

    pub async fn complete_scheduled_segment_change(
        &self,
        id: Uuid,
        status: ScheduledChangeStatus,
        error_message: Option<&str>,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE scheduled_segment_changes
             SET status = $2, error_message = $3, executed_at = now()
             WHERE id = $1",
        )
        .bind(id)
        .bind(status.as_str())
        .bind(error_message)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // =========================================================================
    // Code References Tracking & Webhook Integration Kind
    // =========================================================================

    pub async fn upsert_code_refs(
        &self,
        org_id: Uuid,
        project_key: &str,
        repo_name: &str,
        branch: &str,
        commit_sha: &str,
        hunks: &[CodeRefHunk],
    ) -> Result<Vec<FlagCodeRefSummary>, StorageError> {
        let project = self.get_project(org_id, project_key).await?;
        let mut grouped: std::collections::BTreeMap<String, Vec<CodeRefHunk>> =
            std::collections::BTreeMap::new();
        for h in hunks {
            grouped
                .entry(h.flag_key.clone())
                .or_default()
                .push(h.clone());
        }

        // Also mark any previously seen flags for this repo/branch that now have 0 hunks as extinct
        let existing_keys: Vec<String> = sqlx::query_scalar(
            "SELECT flag_key FROM flag_code_refs
             WHERE org_id = $1 AND project_id = $2 AND repo_name = $3 AND branch = $4",
        )
        .bind(org_id)
        .bind(project.id)
        .bind(repo_name)
        .bind(branch)
        .fetch_all(&self.pool)
        .await?;
        for ek in existing_keys {
            grouped.entry(ek).or_default();
        }

        let mut summaries = Vec::with_capacity(grouped.len());
        for (flag_key, flag_hunks) in grouped {
            let count = flag_hunks.len() as i64;
            let extinct = count == 0;
            let hunks_json = serde_json::to_value(&flag_hunks).unwrap_or(Value::Array(vec![]));
            let id = Uuid::now_v7();
            let row = sqlx::query(
                "INSERT INTO flag_code_refs (
                    id, org_id, project_id, flag_key, repo_name, branch, commit_sha, hunks, reference_count, extinct
                 )
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                 ON CONFLICT (project_id, flag_key, repo_name, branch)
                 DO UPDATE SET commit_sha = EXCLUDED.commit_sha,
                               hunks = EXCLUDED.hunks,
                               reference_count = EXCLUDED.reference_count,
                               extinct = EXCLUDED.extinct,
                               updated_at = now()
                 RETURNING flag_key, repo_name, branch, commit_sha, hunks, reference_count, extinct, updated_at",
            )
            .bind(id)
            .bind(org_id)
            .bind(project.id)
            .bind(&flag_key)
            .bind(repo_name)
            .bind(branch)
            .bind(commit_sha)
            .bind(&hunks_json)
            .bind(count)
            .bind(extinct)
            .fetch_one(&self.pool)
            .await?;
            summaries.push(parse_code_ref_row(&row));
        }

        Ok(summaries)
    }

    pub async fn list_flag_code_refs(
        &self,
        org_id: Uuid,
        project_key: &str,
        flag_key: &str,
    ) -> Result<Vec<FlagCodeRefSummary>, StorageError> {
        let project = self.get_project(org_id, project_key).await?;
        let rows = sqlx::query(
            "SELECT flag_key, repo_name, branch, commit_sha, hunks, reference_count, extinct, updated_at
             FROM flag_code_refs
             WHERE org_id = $1 AND project_id = $2 AND flag_key = $3
             ORDER BY updated_at DESC",
        )
        .bind(org_id)
        .bind(project.id)
        .bind(flag_key)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(parse_code_ref_row).collect())
    }

    pub async fn set_webhook_integration_kind(
        &self,
        org_id: Uuid,
        webhook_id: Uuid,
        kind: WebhookIntegrationKind,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE webhooks SET integration_kind = $3, updated_at = now()
             WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(webhook_id)
        .bind(kind.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn parse_custom_role_row(row: &sqlx::postgres::PgRow) -> Result<CustomRole, StorageError> {
    let statements: Vec<PolicyStatement> = serde_json::from_value(row.get("statements"))
        .map_err(|e| StorageError::Validation(DomainError::Validation(e.to_string())))?;
    Ok(CustomRole {
        id: row.get("id"),
        org_id: row.get("org_id"),
        key: row.get("key"),
        name: row.get("name"),
        description: row.get("description"),
        statements,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

fn parse_sso_row(row: &sqlx::postgres::PgRow) -> SsoProviderConfig {
    let protocol = match row.get::<String, _>("protocol").as_str() {
        "saml" => SsoProtocol::Saml,
        _ => SsoProtocol::Oidc,
    };
    SsoProviderConfig {
        id: row.get("id"),
        org_id: row.get("org_id"),
        protocol,
        name: row.get("name"),
        issuer_url: row.get("issuer_url"),
        client_id: row.get("client_id"),
        jwks_url: row.get("jwks_url"),
        saml_metadata_url: row.get("saml_metadata_url"),
        enabled: row.get("enabled"),
        scim_enabled: row.get("scim_enabled"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn parse_workflow_row(row: &sqlx::postgres::PgRow) -> FlagWorkflow {
    let status = match row.get::<String, _>("status").as_str() {
        "waiting_approval" => WorkflowStatus::WaitingApproval,
        "completed" => WorkflowStatus::Completed,
        "failed" => WorkflowStatus::Failed,
        "canceled" => WorkflowStatus::Canceled,
        _ => WorkflowStatus::Active,
    };
    let stages: Vec<WorkflowStage> = serde_json::from_value(row.get("stages")).unwrap_or_default();
    FlagWorkflow {
        id: row.get("id"),
        org_id: row.get("org_id"),
        project_key: row.get("project_key"),
        env_key: row.get("env_key"),
        flag_key: row.get("flag_key"),
        name: row.get("name"),
        description: row.get("description"),
        status,
        current_stage_index: row.get("current_stage_index"),
        next_run_at: row.get("next_run_at"),
        stages,
        created_by: row.get("created_by"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn parse_pipeline_row(row: &sqlx::postgres::PgRow) -> ReleasePipeline {
    let phases: Vec<ReleasePhase> = serde_json::from_value(row.get("phases")).unwrap_or_default();
    ReleasePipeline {
        id: row.get("id"),
        org_id: row.get("org_id"),
        project_id: row.get("project_id"),
        key: row.get("key"),
        name: row.get("name"),
        description: row.get("description"),
        phases,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn parse_flag_release_row(row: &sqlx::postgres::PgRow) -> FlagRelease {
    let phases: Vec<FlagReleasePhaseState> =
        serde_json::from_value(row.get("phases")).unwrap_or_default();
    FlagRelease {
        id: row.get("id"),
        org_id: row.get("org_id"),
        project_key: row.get("project_key"),
        flag_key: row.get("flag_key"),
        pipeline_key: row.get("pipeline_key"),
        pipeline_name: row.get("pipeline_name"),
        status: row.get("status"),
        current_phase_index: row.get("current_phase_index"),
        phases,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn parse_app_kind(raw: &str) -> ApplicationKind {
    match raw {
        "browser" => ApplicationKind::Browser,
        "mobile" => ApplicationKind::Mobile,
        _ => ApplicationKind::Server,
    }
}

fn parse_scheduled_segment_row(row: &sqlx::postgres::PgRow) -> ScheduledSegmentChange {
    let status = match row.get::<String, _>("status").as_str() {
        "executed" => ScheduledChangeStatus::Executed,
        "failed" => ScheduledChangeStatus::Failed,
        "canceled" => ScheduledChangeStatus::Canceled,
        _ => ScheduledChangeStatus::Pending,
    };
    let instructions: Vec<SegmentPatchInstruction> =
        serde_json::from_value(row.get("instructions")).unwrap_or_default();
    ScheduledSegmentChange {
        id: row.get("id"),
        org_id: row.get("org_id"),
        segment_id: row.get("segment_id"),
        env_id: row.get("env_id"),
        execute_at: row.get("execute_at"),
        instructions,
        comment: row.get("comment"),
        created_by: row.get("created_by"),
        status,
        error_message: row.get("error_message"),
        executed_at: row.get("executed_at"),
        created_at: row.get("created_at"),
    }
}

fn parse_code_ref_row(row: &sqlx::postgres::PgRow) -> FlagCodeRefSummary {
    let hunks: Vec<CodeRefHunk> = serde_json::from_value(row.get("hunks")).unwrap_or_default();
    FlagCodeRefSummary {
        flag_key: row.get("flag_key"),
        repo_name: row.get("repo_name"),
        branch: row.get("branch"),
        commit_sha: row.get("commit_sha"),
        reference_count: row.get("reference_count"),
        extinct: row.get("extinct"),
        hunks,
        updated_at: row.get("updated_at"),
    }
}
