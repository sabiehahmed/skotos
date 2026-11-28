use std::collections::HashMap;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use skotos_domain::{
    apply_flag_patch, detect_prerequisite_cycle, validate_hex_color, validate_key,
    variation_in_use_locations, ApiToken, AuditLogEntry, BuiltInRole, ClientSideAvailability,
    DomainError, Environment, EnvironmentKey, EnvironmentKeyKind, FeatureFlag, FlagEnvConfig,
    FlagEnvConfigBody, FlagEnvRevision, FlagKind, FlagPatchInstruction, Member, Organization,
    OutboxEvent, PatchSummaryItem, PlanTier, Project, Segment, SegmentKind, Variation,
    VariationOrRollout,
};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

use crate::error::StorageError;
use crate::keys::{generate_client_side_id, generate_prefixed_secret, generate_salt, sha256_hex};

/// Embedded PostgreSQL migrations executed on startup or `--migrate-only`.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Runs embedded SQLx migrations idempotently against the target Postgres pool.
pub async fn run_migrations(pool: &PgPool) -> Result<(), StorageError> {
    MIGRATOR.run(pool).await?;
    Ok(())
}

/// Actor metadata captured in `audit_logs` and `flag_env_revisions`.
#[derive(Debug, Clone)]
pub struct AuditActor {
    pub actor_id: Option<Uuid>,
    pub actor_type: String,
    pub actor_email: Option<String>,
}

impl AuditActor {
    pub fn system() -> Self {
        Self {
            actor_id: None,
            actor_type: "system".to_string(),
            actor_email: None,
        }
    }
}

/// Member record including password hash (used internally by `SelfHostedAuthAdapter`).
#[derive(Debug, Clone)]
pub struct MemberWithPassword {
    pub member: Member,
    pub password_hash: Option<String>,
}

/// Combined flag definition + per-environment configurations keyed by environment key.
#[derive(Debug, Clone)]
pub struct FlagWithEnvironments {
    pub flag: FeatureFlag,
    pub environments: HashMap<String, FlagEnvConfig>,
}

/// Input payload for creating a new flag across all environments in a project.
#[derive(Debug, Clone)]
pub struct CreateFlagInput {
    pub key: String,
    pub name: String,
    pub description: String,
    pub kind: FlagKind,
    pub variations: Vec<Variation>,
    pub temporary: bool,
    pub client_side_availability: Option<ClientSideAvailability>,
    pub tags: Vec<String>,
    pub maintainer_id: Option<Uuid>,
    pub default_on_variation_id: Option<Uuid>,
    pub default_off_variation_id: Option<Uuid>,
}

/// Output from patching a flag in a specific environment.
#[derive(Debug, Clone)]
pub struct PatchFlagOutcome {
    pub flag: FeatureFlag,
    pub env_config: FlagEnvConfig,
    pub summaries: Vec<PatchSummaryItem>,
}

/// Output from rotating an environment SDK or Mobile key (plaintext shown once).
#[derive(Debug, Clone)]
pub struct RotatedKeyOutcome {
    pub key: EnvironmentKey,
    pub plaintext_key: String,
}

/// Output from creating an API token (plaintext shown once).
#[derive(Debug, Clone)]
pub struct CreatedApiTokenOutcome {
    pub token: ApiToken,
    pub plaintext_token: String,
}

/// Query filter for cursor-paginated audit logs.
#[derive(Debug, Clone, Default)]
pub struct AuditLogFilter {
    pub project_id: Option<Uuid>,
    pub env_id: Option<Uuid>,
    pub resource_type: Option<String>,
    pub resource_key: Option<String>,
    pub actor_id: Option<Uuid>,
    pub limit: i64,
    pub cursor: Option<String>,
}

/// Cursor-paginated audit log response page.
#[derive(Debug, Clone)]
pub struct AuditLogPage {
    pub items: Vec<AuditLogEntry>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct AuditCursorPayload {
    ts: DateTime<Utc>,
    id: Uuid,
}

/// Multi-tenant PostgreSQL repository facade.
#[derive(Debug, Clone)]
pub struct Storage {
    pub(crate) pool: PgPool,
}

impl Storage {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn ping(&self) -> Result<(), StorageError> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    // =========================================================================
    // Organizations & Members
    // =========================================================================

    pub async fn get_organization(&self, org_id: Uuid) -> Result<Organization, StorageError> {
        let row = sqlx::query(
            "SELECT id, slug, name, external_auth_org_id, plan_tier, created_at
             FROM organizations WHERE id = $1",
        )
        .bind(org_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("organization {org_id}")))?;
        parse_organization_row(&row)
    }

    pub async fn get_organization_by_slug(
        &self,
        slug: &str,
    ) -> Result<Option<Organization>, StorageError> {
        let row = sqlx::query(
            "SELECT id, slug, name, external_auth_org_id, plan_tier, created_at
             FROM organizations WHERE slug = $1",
        )
        .bind(slug)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(parse_organization_row).transpose()
    }

    pub async fn get_organization_by_external_id(
        &self,
        external_org_id: &str,
    ) -> Result<Option<Organization>, StorageError> {
        let row = sqlx::query(
            "SELECT id, slug, name, external_auth_org_id, plan_tier, created_at
             FROM organizations WHERE external_auth_org_id = $1",
        )
        .bind(external_org_id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(parse_organization_row).transpose()
    }

    /// Idempotently creates an organization (used for bootstrap and external IdP sync).
    pub async fn ensure_organization(
        &self,
        slug: &str,
        name: &str,
        external_auth_org_id: Option<&str>,
        plan_tier: PlanTier,
    ) -> Result<Organization, StorageError> {
        validate_key(slug)?;
        if let Some(existing) = self.get_organization_by_slug(slug).await? {
            return Ok(existing);
        }
        let id = Uuid::new_v4();
        let row = sqlx::query(
            "INSERT INTO organizations (id, slug, name, external_auth_org_id, plan_tier)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (slug) DO UPDATE SET name = organizations.name
             RETURNING id, slug, name, external_auth_org_id, plan_tier, created_at",
        )
        .bind(id)
        .bind(slug)
        .bind(name)
        .bind(external_auth_org_id)
        .bind(plan_tier_str(plan_tier))
        .fetch_one(&self.pool)
        .await?;
        parse_organization_row(&row)
    }

    pub async fn find_member_by_email_global(
        &self,
        email: &str,
    ) -> Result<Option<MemberWithPassword>, StorageError> {
        let row = sqlx::query(
            "SELECT id, org_id, external_subject_id, email, display_name, password_hash, role, created_at
             FROM members WHERE lower(email) = lower($1) ORDER BY created_at ASC LIMIT 1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(parse_member_with_password).transpose()
    }

    pub async fn find_member_by_external_subject(
        &self,
        org_id: Uuid,
        external_subject_id: &str,
    ) -> Result<Option<Member>, StorageError> {
        let row = sqlx::query(
            "SELECT id, org_id, external_subject_id, email, display_name, role, created_at
             FROM members WHERE org_id = $1 AND external_subject_id = $2",
        )
        .bind(org_id)
        .bind(external_subject_id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(parse_member_row).transpose()
    }

    pub async fn get_member(&self, org_id: Uuid, member_id: Uuid) -> Result<Member, StorageError> {
        let row = sqlx::query(
            "SELECT id, org_id, external_subject_id, email, display_name, role, created_at
             FROM members WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(member_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("member {member_id}")))?;
        let mut member = parse_member_row(&row)?;
        member.custom_role_keys = sqlx::query_scalar(
            "SELECT cr.key FROM member_custom_roles mcr
             JOIN custom_roles cr ON cr.id = mcr.role_id
             WHERE mcr.org_id = $1 AND mcr.member_id = $2
             ORDER BY mcr.created_at ASC, cr.key ASC",
        )
        .bind(org_id)
        .bind(member_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(member)
    }

    pub async fn list_members(&self, org_id: Uuid) -> Result<Vec<Member>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, org_id, external_subject_id, email, display_name, role, created_at
             FROM members WHERE org_id = $1 ORDER BY created_at ASC",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await?;
        let mut members = rows
            .iter()
            .map(parse_member_row)
            .collect::<Result<Vec<_>, _>>()?;
        let mut role_keys = self.list_member_custom_role_keys(org_id).await?;
        for m in &mut members {
            m.custom_role_keys = role_keys.remove(&m.id).unwrap_or_default();
        }
        Ok(members)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_member(
        &self,
        org_id: Uuid,
        email: &str,
        display_name: &str,
        role: BuiltInRole,
        password_hash: Option<&str>,
        external_subject_id: Option<&str>,
        actor: &AuditActor,
    ) -> Result<Member, StorageError> {
        let trimmed_email = email.trim().to_lowercase();
        if trimmed_email.is_empty() || !trimmed_email.contains('@') {
            return Err(StorageError::Validation(DomainError::Validation(
                "valid email address is required".to_string(),
            )));
        }
        let mut tx = self.pool.begin().await?;
        let id = Uuid::new_v4();
        let row = sqlx::query(
            "INSERT INTO members (id, org_id, external_subject_id, email, display_name, password_hash, role)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING id, org_id, external_subject_id, email, display_name, role, created_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(external_subject_id)
        .bind(&trimmed_email)
        .bind(display_name.trim())
        .bind(password_hash)
        .bind(role.as_str())
        .fetch_one(&mut *tx)
        .await
        .map_err(map_unique_err("member"))?;
        let member = parse_member_row(&row)?;

        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "member",
            &member.email,
            "create",
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

    pub async fn update_member_role(
        &self,
        org_id: Uuid,
        member_id: Uuid,
        new_role: BuiltInRole,
        actor: &AuditActor,
    ) -> Result<Member, StorageError> {
        let mut tx = self.pool.begin().await?;
        let current_row = sqlx::query(
            "SELECT id, org_id, external_subject_id, email, display_name, role, created_at
             FROM members WHERE org_id = $1 AND id = $2 FOR UPDATE",
        )
        .bind(org_id)
        .bind(member_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("member {member_id}")))?;
        let before = parse_member_row(&current_row)?;

        if before.role == BuiltInRole::Owner && new_role != BuiltInRole::Owner {
            let owner_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM members WHERE org_id = $1 AND role = 'owner'",
            )
            .bind(org_id)
            .fetch_one(&mut *tx)
            .await?;
            if owner_count <= 1 {
                return Err(StorageError::ForbiddenLastOwner);
            }
        }

        let updated_row = sqlx::query(
            "UPDATE members SET role = $3 WHERE org_id = $1 AND id = $2
             RETURNING id, org_id, external_subject_id, email, display_name, role, created_at",
        )
        .bind(org_id)
        .bind(member_id)
        .bind(new_role.as_str())
        .fetch_one(&mut *tx)
        .await?;
        let after = parse_member_row(&updated_row)?;

        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "member",
            &after.email,
            "updateRole",
            actor,
            None,
            None,
            Some(serde_json::to_value(&before).unwrap_or(Value::Null)),
            Some(serde_json::to_value(&after).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(after)
    }

    pub async fn delete_member(
        &self,
        org_id: Uuid,
        member_id: Uuid,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let current_row = sqlx::query(
            "SELECT id, org_id, external_subject_id, email, display_name, role, created_at
             FROM members WHERE org_id = $1 AND id = $2 FOR UPDATE",
        )
        .bind(org_id)
        .bind(member_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("member {member_id}")))?;
        let before = parse_member_row(&current_row)?;

        if before.role == BuiltInRole::Owner {
            let owner_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM members WHERE org_id = $1 AND role = 'owner'",
            )
            .bind(org_id)
            .fetch_one(&mut *tx)
            .await?;
            if owner_count <= 1 {
                return Err(StorageError::ForbiddenLastOwner);
            }
        }

        sqlx::query("DELETE FROM members WHERE org_id = $1 AND id = $2")
            .bind(org_id)
            .bind(member_id)
            .execute(&mut *tx)
            .await?;

        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "member",
            &before.email,
            "delete",
            actor,
            None,
            None,
            Some(serde_json::to_value(&before).unwrap_or(Value::Null)),
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    // =========================================================================
    // Projects
    // =========================================================================

    pub async fn list_projects(&self, org_id: Uuid) -> Result<Vec<Project>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, org_id, key, name, default_client_side_availability, created_at
             FROM projects WHERE org_id = $1 ORDER BY created_at ASC",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(parse_project_row).collect()
    }

    pub async fn get_project(
        &self,
        org_id: Uuid,
        project_key: &str,
    ) -> Result<Project, StorageError> {
        let row = sqlx::query(
            "SELECT id, org_id, key, name, default_client_side_availability, created_at
             FROM projects WHERE org_id = $1 AND key = $2",
        )
        .bind(org_id)
        .bind(project_key)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("project '{project_key}'")))?;
        parse_project_row(&row)
    }

    pub async fn create_project(
        &self,
        org_id: Uuid,
        key: &str,
        name: &str,
        default_csa: ClientSideAvailability,
        seed_default_envs: bool,
        actor: &AuditActor,
    ) -> Result<(Project, Vec<Environment>), StorageError> {
        validate_key(key)?;
        if name.trim().is_empty() {
            return Err(StorageError::Validation(DomainError::Validation(
                "project name cannot be empty".to_string(),
            )));
        }

        let mut tx = self.pool.begin().await?;
        let project_id = Uuid::new_v4();
        let row = sqlx::query(
            "INSERT INTO projects (id, org_id, key, name, default_client_side_availability)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id, org_id, key, name, default_client_side_availability, created_at",
        )
        .bind(project_id)
        .bind(org_id)
        .bind(key)
        .bind(name.trim())
        .bind(sqlx::types::Json(default_csa))
        .fetch_one(&mut *tx)
        .await
        .map_err(map_unique_err("project"))?;
        let project = parse_project_row(&row)?;

        let mut created_envs = Vec::new();
        if seed_default_envs {
            let defaults = [
                ("development", "Development", "#10B981", false, false),
                ("production", "Production", "#EF4444", true, true),
            ];
            for (env_key, env_name, color, req_comments, confirm_changes) in defaults {
                let env = insert_environment_tx(
                    &mut tx,
                    org_id,
                    project.id,
                    env_key,
                    env_name,
                    color,
                    req_comments,
                    confirm_changes,
                )
                .await?;
                created_envs.push(env);
            }
        }

        insert_audit_log(
            &mut tx,
            org_id,
            Some(project.id),
            None,
            "project",
            &project.key,
            "create",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&project).unwrap_or(Value::Null)),
        )
        .await?;

        tx.commit().await?;
        Ok((project, created_envs))
    }

    pub async fn update_project(
        &self,
        org_id: Uuid,
        project_key: &str,
        name: Option<&str>,
        default_csa: Option<ClientSideAvailability>,
        actor: &AuditActor,
    ) -> Result<Project, StorageError> {
        let mut tx = self.pool.begin().await?;
        let current_row = sqlx::query(
            "SELECT id, org_id, key, name, default_client_side_availability, created_at
             FROM projects WHERE org_id = $1 AND key = $2 FOR UPDATE",
        )
        .bind(org_id)
        .bind(project_key)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("project '{project_key}'")))?;
        let before = parse_project_row(&current_row)?;

        let next_name = match name {
            Some(n) if n.trim().is_empty() => {
                return Err(StorageError::Validation(DomainError::Validation(
                    "project name cannot be empty".to_string(),
                )));
            }
            Some(n) => n.trim().to_string(),
            None => before.name.clone(),
        };
        let next_csa = default_csa.unwrap_or(before.default_client_side_availability);

        let updated_row = sqlx::query(
            "UPDATE projects SET name = $3, default_client_side_availability = $4
             WHERE org_id = $1 AND id = $2
             RETURNING id, org_id, key, name, default_client_side_availability, created_at",
        )
        .bind(org_id)
        .bind(before.id)
        .bind(&next_name)
        .bind(sqlx::types::Json(next_csa))
        .fetch_one(&mut *tx)
        .await?;
        let after = parse_project_row(&updated_row)?;

        insert_audit_log(
            &mut tx,
            org_id,
            Some(after.id),
            None,
            "project",
            &after.key,
            "update",
            actor,
            None,
            None,
            Some(serde_json::to_value(&before).unwrap_or(Value::Null)),
            Some(serde_json::to_value(&after).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(after)
    }

    pub async fn delete_project(
        &self,
        org_id: Uuid,
        project_key: &str,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT id, org_id, key, name, default_client_side_availability, created_at
             FROM projects WHERE org_id = $1 AND key = $2 FOR UPDATE",
        )
        .bind(org_id)
        .bind(project_key)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("project '{project_key}'")))?;
        let before = parse_project_row(&row)?;

        sqlx::query("DELETE FROM projects WHERE org_id = $1 AND id = $2")
            .bind(org_id)
            .bind(before.id)
            .execute(&mut *tx)
            .await?;

        insert_audit_log(
            &mut tx,
            org_id,
            Some(before.id),
            None,
            "project",
            &before.key,
            "delete",
            actor,
            None,
            None,
            Some(serde_json::to_value(&before).unwrap_or(Value::Null)),
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    // =========================================================================
    // Environments & Keys
    // =========================================================================

    pub async fn list_environments(
        &self,
        org_id: Uuid,
        project_key: &str,
    ) -> Result<Vec<Environment>, StorageError> {
        let project = self.get_project(org_id, project_key).await?;
        let rows = sqlx::query(
            "SELECT id, project_id, key, name, color, client_side_id, require_comments, confirm_changes,
                    approval_required, min_approvals, allow_self_approval, external_approval_webhook_url,
                    revision, created_at
             FROM environments WHERE org_id = $1 AND project_id = $2 ORDER BY created_at ASC, key ASC",
        )
        .bind(org_id)
        .bind(project.id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(parse_environment_row).collect()
    }

    pub async fn get_environment(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
    ) -> Result<Environment, StorageError> {
        let project = self.get_project(org_id, project_key).await?;
        let row = sqlx::query(
            "SELECT id, project_id, key, name, color, client_side_id, require_comments, confirm_changes,
                    approval_required, min_approvals, allow_self_approval, external_approval_webhook_url,
                    revision, created_at
             FROM environments WHERE org_id = $1 AND project_id = $2 AND key = $3",
        )
        .bind(org_id)
        .bind(project.id)
        .bind(env_key)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("environment '{env_key}'")))?;
        parse_environment_row(&row)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_environment(
        &self,
        org_id: Uuid,
        project_key: &str,
        key: &str,
        name: &str,
        color: &str,
        require_comments: bool,
        confirm_changes: bool,
        actor: &AuditActor,
    ) -> Result<Environment, StorageError> {
        validate_key(key)?;
        validate_hex_color(color)?;
        if name.trim().is_empty() {
            return Err(StorageError::Validation(DomainError::Validation(
                "environment name cannot be empty".to_string(),
            )));
        }

        let mut tx = self.pool.begin().await?;
        let project_id: Uuid =
            sqlx::query_scalar("SELECT id FROM projects WHERE org_id = $1 AND key = $2")
                .bind(org_id)
                .bind(project_key)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| StorageError::NotFound(format!("project '{project_key}'")))?;

        let env = insert_environment_tx(
            &mut tx,
            org_id,
            project_id,
            key,
            name.trim(),
            color,
            require_comments,
            confirm_changes,
        )
        .await?;

        // Backfill default flag_env_configs for any existing flags in this project
        let flag_rows = sqlx::query(
            "SELECT id, variations_jsonb FROM flags WHERE org_id = $1 AND project_id = $2",
        )
        .bind(org_id)
        .bind(project_id)
        .fetch_all(&mut *tx)
        .await?;

        for frow in flag_rows {
            let flag_id: Uuid = frow.get("id");
            let variations: Vec<Variation> = serde_json::from_value(frow.get("variations_jsonb"))
                .map_err(|e| {
                StorageError::Validation(DomainError::Validation(e.to_string()))
            })?;
            let first_var = variations.first().map(|v| v.id).ok_or_else(|| {
                StorageError::Validation(DomainError::InvalidVariation("missing variation".into()))
            })?;
            // Seed the new environment with the flag's own on/off defaults, taken from the
            // oldest environment that already has a config for it. Assuming "first variation
            // = on, second = off" inverts flags whose variations are ordered e.g. [false, true].
            let existing: Option<sqlx::types::Json<FlagEnvConfigBody>> = sqlx::query_scalar(
                "SELECT c.config_jsonb FROM flag_env_configs c
                 JOIN environments e ON e.id = c.env_id
                 WHERE c.flag_id = $1 AND c.env_id <> $2
                 ORDER BY e.created_at ASC
                 LIMIT 1",
            )
            .bind(flag_id)
            .bind(env.id)
            .fetch_optional(&mut *tx)
            .await?;
            let known = |id: Option<Uuid>| id.filter(|id| variations.iter().any(|v| v.id == *id));
            let (on_var, off_var) = default_on_off_variations(
                &variations,
                existing
                    .as_ref()
                    .and_then(|c| known(c.0.fallthrough.variation_id)),
                existing.as_ref().and_then(|c| known(c.0.off_variation_id)),
                first_var,
            );
            let body = FlagEnvConfigBody {
                off_variation_id: Some(off_var),
                prerequisites: vec![],
                targets: vec![],
                rules: vec![],
                fallthrough: VariationOrRollout::variation(on_var),
                track_events: false,
                track_events_fallthrough: false,
                debug_events_until_date: None,
            };
            sqlx::query(
                "INSERT INTO flag_env_configs (flag_id, env_id, org_id, version, \"on\", salt, config_jsonb)
                 VALUES ($1, $2, $3, 1, false, $4, $5)",
            )
            .bind(flag_id)
            .bind(env.id)
            .bind(org_id)
            .bind(generate_salt())
            .bind(sqlx::types::Json(&body))
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                "INSERT INTO flag_env_revisions (id, org_id, flag_id, env_id, version, \"on\", config_jsonb, instructions_jsonb, actor_id, comment)
                 VALUES ($1, $2, $3, $4, 1, false, $5, '[]'::jsonb, $6, 'Environment created')",
            )
            .bind(Uuid::new_v4())
            .bind(org_id)
            .bind(flag_id)
            .bind(env.id)
            .bind(sqlx::types::Json(&body))
            .bind(actor.actor_id)
            .execute(&mut *tx)
            .await?;
        }

        insert_audit_log(
            &mut tx,
            org_id,
            Some(project_id),
            Some(env.id),
            "environment",
            &env.key,
            "create",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&env).unwrap_or(Value::Null)),
        )
        .await?;

        insert_outbox_event(
            &mut tx,
            org_id,
            Some(env.id),
            "environment.created",
            json!({ "envId": env.id, "projectId": project_id, "revision": env.revision }),
        )
        .await?;

        tx.commit().await?;
        Ok(env)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_environment(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        name: Option<&str>,
        color: Option<&str>,
        require_comments: Option<bool>,
        confirm_changes: Option<bool>,
        actor: &AuditActor,
    ) -> Result<Environment, StorageError> {
        if let Some(c) = color {
            validate_hex_color(c)?;
        }
        let mut tx = self.pool.begin().await?;
        let project_id: Uuid =
            sqlx::query_scalar("SELECT id FROM projects WHERE org_id = $1 AND key = $2")
                .bind(org_id)
                .bind(project_key)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| StorageError::NotFound(format!("project '{project_key}'")))?;

        let row = sqlx::query(
            "SELECT id, project_id, key, name, color, client_side_id, require_comments, confirm_changes, revision, created_at
             FROM environments WHERE org_id = $1 AND project_id = $2 AND key = $3 FOR UPDATE",
        )
        .bind(org_id)
        .bind(project_id)
        .bind(env_key)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("environment '{env_key}'")))?;
        let before = parse_environment_row(&row)?;

        let next_name = match name {
            Some(n) if n.trim().is_empty() => {
                return Err(StorageError::Validation(DomainError::Validation(
                    "environment name cannot be empty".to_string(),
                )));
            }
            Some(n) => n.trim().to_string(),
            None => before.name.clone(),
        };
        let next_color = color.unwrap_or(&before.color).to_string();
        let next_req = require_comments.unwrap_or(before.require_comments);
        let next_confirm = confirm_changes.unwrap_or(before.confirm_changes);

        let updated = sqlx::query(
            "UPDATE environments
             SET name = $3, color = $4, require_comments = $5, confirm_changes = $6
             WHERE org_id = $1 AND id = $2
             RETURNING id, project_id, key, name, color, client_side_id, require_comments, confirm_changes, revision, created_at",
        )
        .bind(org_id)
        .bind(before.id)
        .bind(&next_name)
        .bind(&next_color)
        .bind(next_req)
        .bind(next_confirm)
        .fetch_one(&mut *tx)
        .await?;
        let after = parse_environment_row(&updated)?;

        insert_audit_log(
            &mut tx,
            org_id,
            Some(project_id),
            Some(after.id),
            "environment",
            &after.key,
            "update",
            actor,
            None,
            None,
            Some(serde_json::to_value(&before).unwrap_or(Value::Null)),
            Some(serde_json::to_value(&after).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(after)
    }

    pub async fn delete_environment(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let project_id: Uuid =
            sqlx::query_scalar("SELECT id FROM projects WHERE org_id = $1 AND key = $2")
                .bind(org_id)
                .bind(project_key)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| StorageError::NotFound(format!("project '{project_key}'")))?;

        let row = sqlx::query(
            "SELECT id, project_id, key, name, color, client_side_id, require_comments, confirm_changes, revision, created_at
             FROM environments WHERE org_id = $1 AND project_id = $2 AND key = $3 FOR UPDATE",
        )
        .bind(org_id)
        .bind(project_id)
        .bind(env_key)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("environment '{env_key}'")))?;
        let before = parse_environment_row(&row)?;

        sqlx::query("DELETE FROM environments WHERE org_id = $1 AND id = $2")
            .bind(org_id)
            .bind(before.id)
            .execute(&mut *tx)
            .await?;

        insert_audit_log(
            &mut tx,
            org_id,
            Some(project_id),
            Some(before.id),
            "environment",
            &before.key,
            "delete",
            actor,
            None,
            None,
            Some(serde_json::to_value(&before).unwrap_or(Value::Null)),
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_environment_keys(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
    ) -> Result<Vec<EnvironmentKey>, StorageError> {
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let rows = sqlx::query(
            "SELECT id, env_id, kind, key_prefix, expires_at, created_at
             FROM environment_keys
             WHERE org_id = $1 AND env_id = $2
               AND (expires_at IS NULL OR expires_at > now())
             ORDER BY created_at DESC",
        )
        .bind(org_id)
        .bind(env.id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(parse_env_key_row).collect()
    }

    /// Zero-downtime key rotation: sets optional grace window expiry on active keys of `kind`,
    /// inserts a new active key, and writes audit + outbox records in a single ACID transaction.
    pub async fn rotate_environment_key(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
        kind: EnvironmentKeyKind,
        grace_period_seconds: Option<i64>,
        actor: &AuditActor,
    ) -> Result<RotatedKeyOutcome, StorageError> {
        let mut tx = self.pool.begin().await?;
        let project_id: Uuid =
            sqlx::query_scalar("SELECT id FROM projects WHERE org_id = $1 AND key = $2")
                .bind(org_id)
                .bind(project_key)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| StorageError::NotFound(format!("project '{project_key}'")))?;

        let env_id: Uuid = sqlx::query_scalar(
            "SELECT id FROM environments WHERE org_id = $1 AND project_id = $2 AND key = $3 FOR UPDATE",
        )
        .bind(org_id)
        .bind(project_id)
        .bind(env_key)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("environment '{env_key}'")))?;

        let grace = grace_period_seconds.unwrap_or(0).max(0);
        let old_expiry = Utc::now() + Duration::seconds(grace);
        sqlx::query(
            "UPDATE environment_keys
             SET expires_at = LEAST(COALESCE(expires_at, $4), $4)
             WHERE org_id = $1 AND env_id = $2 AND kind = $3
               AND (expires_at IS NULL OR expires_at > now())",
        )
        .bind(org_id)
        .bind(env_id)
        .bind(kind.as_str())
        .bind(old_expiry)
        .execute(&mut *tx)
        .await?;

        let prefix_tag = match kind {
            EnvironmentKeyKind::Sdk => "sk_sdk_",
            EnvironmentKeyKind::Mobile => "sk_mob_",
        };
        let generated = generate_prefixed_secret(prefix_tag);
        let key_id = Uuid::new_v4();
        let row = sqlx::query(
            "INSERT INTO environment_keys (id, org_id, env_id, kind, key_prefix, key_hash)
             VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING id, env_id, kind, key_prefix, expires_at, created_at",
        )
        .bind(key_id)
        .bind(org_id)
        .bind(env_id)
        .bind(kind.as_str())
        .bind(&generated.prefix)
        .bind(&generated.hash)
        .fetch_one(&mut *tx)
        .await?;
        let env_key_record = parse_env_key_row(&row)?;

        insert_audit_log(
            &mut tx,
            org_id,
            Some(project_id),
            Some(env_id),
            "environmentKey",
            env_key,
            "rotateKey",
            actor,
            None,
            None,
            None,
            Some(json!({ "kind": kind, "newKeyPrefix": env_key_record.key_prefix, "gracePeriodSeconds": grace })),
        )
        .await?;

        insert_outbox_event(
            &mut tx,
            org_id,
            Some(env_id),
            "environment.key_rotated",
            json!({ "envId": env_id, "kind": kind, "keyId": env_key_record.id }),
        )
        .await?;

        tx.commit().await?;
        Ok(RotatedKeyOutcome {
            key: env_key_record,
            plaintext_key: generated.plaintext,
        })
    }

    // =========================================================================
    // Feature Flags, Transactional Semantic Patching & Revisions
    // =========================================================================

    pub async fn list_flags(
        &self,
        org_id: Uuid,
        project_key: &str,
        include_archived: bool,
        search: Option<&str>,
        tag: Option<&str>,
    ) -> Result<Vec<FlagWithEnvironments>, StorageError> {
        let project = self.get_project(org_id, project_key).await?;
        let envs = self.list_environments(org_id, project_key).await?;
        let env_key_by_id: HashMap<Uuid, String> =
            envs.into_iter().map(|e| (e.id, e.key)).collect();

        let rows = sqlx::query(
            "SELECT id, project_id, key, name, description, kind, variations_jsonb, temporary,
                    client_side_availability_jsonb, tags, maintainer_id, archived, created_at, updated_at
             FROM flags
             WHERE org_id = $1 AND project_id = $2
               AND ($3 OR archived = false)
               AND ($4::text IS NULL OR key ILIKE '%' || $4 || '%' OR name ILIKE '%' || $4 || '%')
               AND ($5::text IS NULL OR $5 = ANY(tags))
             ORDER BY created_at DESC",
        )
        .bind(org_id)
        .bind(project.id)
        .bind(include_archived)
        .bind(search)
        .bind(tag)
        .fetch_all(&self.pool)
        .await?;

        let mut flags = Vec::with_capacity(rows.len());
        let mut flag_ids = Vec::with_capacity(rows.len());
        for row in &rows {
            let f = parse_flag_row(row)?;
            flag_ids.push(f.id);
            flags.push(f);
        }

        let cfg_rows = if flag_ids.is_empty() {
            vec![]
        } else {
            sqlx::query(
                "SELECT flag_id, env_id, version, \"on\", salt, config_jsonb, updated_at
                 FROM flag_env_configs
                 WHERE org_id = $1 AND flag_id = ANY($2)",
            )
            .bind(org_id)
            .bind(&flag_ids)
            .fetch_all(&self.pool)
            .await?
        };

        let mut cfg_by_flag: HashMap<Uuid, HashMap<String, FlagEnvConfig>> = HashMap::new();
        for crow in &cfg_rows {
            let cfg = parse_flag_env_config_row(crow)?;
            if let Some(ekey) = env_key_by_id.get(&cfg.env_id) {
                cfg_by_flag
                    .entry(cfg.flag_id)
                    .or_default()
                    .insert(ekey.clone(), cfg);
            }
        }

        Ok(flags
            .into_iter()
            .map(|flag| {
                let environments = cfg_by_flag.remove(&flag.id).unwrap_or_default();
                FlagWithEnvironments { flag, environments }
            })
            .collect())
    }

    pub async fn get_flag(
        &self,
        org_id: Uuid,
        project_key: &str,
        flag_key: &str,
    ) -> Result<FlagWithEnvironments, StorageError> {
        let project = self.get_project(org_id, project_key).await?;
        let envs = self.list_environments(org_id, project_key).await?;
        let env_key_by_id: HashMap<Uuid, String> =
            envs.into_iter().map(|e| (e.id, e.key)).collect();

        let row = sqlx::query(
            "SELECT id, project_id, key, name, description, kind, variations_jsonb, temporary,
                    client_side_availability_jsonb, tags, maintainer_id, archived, created_at, updated_at
             FROM flags WHERE org_id = $1 AND project_id = $2 AND key = $3",
        )
        .bind(org_id)
        .bind(project.id)
        .bind(flag_key)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("flag '{flag_key}'")))?;
        let flag = parse_flag_row(&row)?;

        let cfg_rows = sqlx::query(
            "SELECT flag_id, env_id, version, \"on\", salt, config_jsonb, updated_at
             FROM flag_env_configs WHERE org_id = $1 AND flag_id = $2",
        )
        .bind(org_id)
        .bind(flag.id)
        .fetch_all(&self.pool)
        .await?;

        let mut environments = HashMap::new();
        for crow in &cfg_rows {
            let cfg = parse_flag_env_config_row(crow)?;
            if let Some(ekey) = env_key_by_id.get(&cfg.env_id) {
                environments.insert(ekey.clone(), cfg);
            }
        }

        Ok(FlagWithEnvironments { flag, environments })
    }

    pub async fn create_flag(
        &self,
        org_id: Uuid,
        project_key: &str,
        input: CreateFlagInput,
        actor: &AuditActor,
    ) -> Result<FlagWithEnvironments, StorageError> {
        validate_key(&input.key)?;
        if input.name.trim().is_empty() {
            return Err(StorageError::Validation(DomainError::Validation(
                "flag name cannot be empty".to_string(),
            )));
        }

        let mut tx = self.pool.begin().await?;
        let proj_row = sqlx::query(
            "SELECT id, org_id, key, name, default_client_side_availability, created_at
             FROM projects WHERE org_id = $1 AND key = $2",
        )
        .bind(org_id)
        .bind(project_key)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("project '{project_key}'")))?;
        let project = parse_project_row(&proj_row)?;

        let now = Utc::now();
        let flag_id = Uuid::new_v4();
        let csa = input
            .client_side_availability
            .unwrap_or(project.default_client_side_availability);

        let candidate_flag = FeatureFlag {
            id: flag_id,
            project_id: project.id,
            key: input.key.clone(),
            name: input.name.trim().to_string(),
            description: input.description.clone(),
            kind: input.kind,
            variations: input.variations.clone(),
            temporary: input.temporary,
            client_side_availability: csa,
            tags: input.tags.clone(),
            maintainer_id: input.maintainer_id,
            archived: false,
            created_at: now,
            updated_at: now,
        };
        candidate_flag.validate_variations()?;

        let on_var_id = input
            .default_on_variation_id
            .unwrap_or(candidate_flag.variations[0].id);
        let off_var_id = input
            .default_off_variation_id
            .unwrap_or(candidate_flag.variations[1].id);
        if candidate_flag.variation_index_by_id(on_var_id).is_none() {
            return Err(StorageError::Validation(DomainError::VariationNotFound(
                on_var_id,
            )));
        }
        if candidate_flag.variation_index_by_id(off_var_id).is_none() {
            return Err(StorageError::Validation(DomainError::VariationNotFound(
                off_var_id,
            )));
        }

        let frow = sqlx::query(
            "INSERT INTO flags (
                id, org_id, project_id, key, name, description, kind, variations_jsonb,
                temporary, client_side_availability_jsonb, tags, maintainer_id, archived
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, false)
             RETURNING id, project_id, key, name, description, kind, variations_jsonb, temporary,
                       client_side_availability_jsonb, tags, maintainer_id, archived, created_at, updated_at",
        )
        .bind(flag_id)
        .bind(org_id)
        .bind(project.id)
        .bind(&candidate_flag.key)
        .bind(&candidate_flag.name)
        .bind(&candidate_flag.description)
        .bind(candidate_flag.kind.as_str())
        .bind(sqlx::types::Json(&candidate_flag.variations))
        .bind(candidate_flag.temporary)
        .bind(sqlx::types::Json(candidate_flag.client_side_availability))
        .bind(&candidate_flag.tags)
        .bind(candidate_flag.maintainer_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_unique_err("flag"))?;
        let flag = parse_flag_row(&frow)?;

        let env_rows = sqlx::query(
            "SELECT id, project_id, key, name, color, client_side_id, require_comments, confirm_changes, revision, created_at
             FROM environments WHERE org_id = $1 AND project_id = $2 ORDER BY created_at ASC FOR UPDATE",
        )
        .bind(org_id)
        .bind(project.id)
        .fetch_all(&mut *tx)
        .await?;

        let mut env_configs = HashMap::new();
        for erow in &env_rows {
            let env = parse_environment_row(erow)?;
            let body = FlagEnvConfigBody {
                off_variation_id: Some(off_var_id),
                prerequisites: vec![],
                targets: vec![],
                rules: vec![],
                fallthrough: VariationOrRollout::variation(on_var_id),
                track_events: false,
                track_events_fallthrough: false,
                debug_events_until_date: None,
            };
            let crow = sqlx::query(
                "INSERT INTO flag_env_configs (flag_id, env_id, org_id, version, \"on\", salt, config_jsonb)
                 VALUES ($1, $2, $3, 1, false, $4, $5)
                 RETURNING flag_id, env_id, version, \"on\", salt, config_jsonb, updated_at",
            )
            .bind(flag.id)
            .bind(env.id)
            .bind(org_id)
            .bind(generate_salt())
            .bind(sqlx::types::Json(&body))
            .fetch_one(&mut *tx)
            .await?;
            let cfg = parse_flag_env_config_row(&crow)?;

            sqlx::query(
                "INSERT INTO flag_env_revisions (id, org_id, flag_id, env_id, version, \"on\", config_jsonb, instructions_jsonb, actor_id, comment)
                 VALUES ($1, $2, $3, $4, 1, false, $5, '[]'::jsonb, $6, 'Flag created')",
            )
            .bind(Uuid::new_v4())
            .bind(org_id)
            .bind(flag.id)
            .bind(env.id)
            .bind(sqlx::types::Json(&body))
            .bind(actor.actor_id)
            .execute(&mut *tx)
            .await?;

            let new_rev: i64 = sqlx::query_scalar(
                "UPDATE environments SET revision = revision + 1 WHERE id = $1 RETURNING revision",
            )
            .bind(env.id)
            .fetch_one(&mut *tx)
            .await?;

            insert_outbox_event(
                &mut tx,
                org_id,
                Some(env.id),
                "flag.created",
                json!({
                    "flagId": flag.id,
                    "flagKey": flag.key,
                    "envId": env.id,
                    "version": 1,
                    "envRevision": new_rev,
                    "clientSideAffected": flag.client_side_availability.using_environment_id || flag.client_side_availability.using_mobile_key
                }),
            )
            .await?;

            env_configs.insert(env.key, cfg);
        }

        insert_audit_log(
            &mut tx,
            org_id,
            Some(project.id),
            None,
            "flag",
            &flag.key,
            "create",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&flag).unwrap_or(Value::Null)),
        )
        .await?;

        tx.commit().await?;
        Ok(FlagWithEnvironments {
            flag,
            environments: env_configs,
        })
    }

    /// Atomically applies semantic `FlagPatchInstruction[]` to a flag & environment config:
    /// checks `require_comments`, checks `expected_version` (`409 Conflict`), checks cross-env
    /// `VariationInUse` and prerequisite cycles, bumps monotonic `version` & `environments.revision`,
    /// and writes `flag_env_revisions`, `audit_logs`, and `outbox_events` in ONE ACID transaction.
    #[allow(clippy::too_many_arguments)]
    pub async fn patch_flag(
        &self,
        org_id: Uuid,
        project_key: &str,
        flag_key: &str,
        env_key: &str,
        expected_version: Option<i64>,
        comment: Option<&str>,
        instructions: &[FlagPatchInstruction],
        actor: &AuditActor,
    ) -> Result<PatchFlagOutcome, StorageError> {
        let mut tx = self.pool.begin().await?;

        let project_id: Uuid =
            sqlx::query_scalar("SELECT id FROM projects WHERE org_id = $1 AND key = $2")
                .bind(org_id)
                .bind(project_key)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| StorageError::NotFound(format!("project '{project_key}'")))?;

        let env_row = sqlx::query(
            "SELECT id, project_id, key, name, color, client_side_id, require_comments, confirm_changes, revision, created_at
             FROM environments WHERE org_id = $1 AND project_id = $2 AND key = $3 FOR UPDATE",
        )
        .bind(org_id)
        .bind(project_id)
        .bind(env_key)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("environment '{env_key}'")))?;
        let env = parse_environment_row(&env_row)?;

        if env.require_comments && comment.map(str::trim).unwrap_or("").is_empty() {
            return Err(StorageError::CommentRequired);
        }

        let flag_row = sqlx::query(
            "SELECT id, project_id, key, name, description, kind, variations_jsonb, temporary,
                    client_side_availability_jsonb, tags, maintainer_id, archived, created_at, updated_at
             FROM flags WHERE org_id = $1 AND project_id = $2 AND key = $3 FOR UPDATE",
        )
        .bind(org_id)
        .bind(project_id)
        .bind(flag_key)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("flag '{flag_key}'")))?;
        let before_flag = parse_flag_row(&flag_row)?;

        let cfg_row = sqlx::query(
            "SELECT flag_id, env_id, version, \"on\", salt, config_jsonb, updated_at
             FROM flag_env_configs WHERE org_id = $1 AND flag_id = $2 AND env_id = $3 FOR UPDATE",
        )
        .bind(org_id)
        .bind(before_flag.id)
        .bind(env.id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            StorageError::NotFound(format!("flag config '{flag_key}' in env '{env_key}'"))
        })?;
        let before_cfg = parse_flag_env_config_row(&cfg_row)?;

        if let Some(exp) = expected_version {
            if exp != before_cfg.version {
                return Err(StorageError::VersionConflict {
                    expected: exp,
                    actual: before_cfg.version,
                });
            }
        }

        let patch_result = apply_flag_patch(&before_flag, &before_cfg, instructions)?;

        // Cross-environment variation-in-use validation for any RemoveVariation instructions
        for instr in instructions {
            if let FlagPatchInstruction::RemoveVariation { variation_id } = instr {
                let mut refs = Vec::new();
                let other_cfgs = sqlx::query(
                    "SELECT e.key AS env_key, c.config_jsonb
                     FROM flag_env_configs c
                     JOIN environments e ON e.id = c.env_id
                     WHERE c.org_id = $1 AND c.flag_id = $2 AND c.env_id <> $3",
                )
                .bind(org_id)
                .bind(before_flag.id)
                .bind(env.id)
                .fetch_all(&mut *tx)
                .await?;
                for row in &other_cfgs {
                    let other_env_key: String = row.get("env_key");
                    let other_body: FlagEnvConfigBody =
                        serde_json::from_value(row.get("config_jsonb")).map_err(|e| {
                            StorageError::Validation(DomainError::Validation(e.to_string()))
                        })?;
                    refs.extend(variation_in_use_locations(
                        &other_env_key,
                        &other_body,
                        *variation_id,
                    ));
                }
                // Also check cross-flag prerequisites referencing this flag + variation
                let prereq_rows = sqlx::query(
                    "SELECT f.key AS ref_flag_key, e.key AS env_key, c.config_jsonb
                     FROM flag_env_configs c
                     JOIN flags f ON f.id = c.flag_id
                     JOIN environments e ON e.id = c.env_id
                     WHERE c.org_id = $1 AND f.project_id = $2 AND c.flag_id <> $3",
                )
                .bind(org_id)
                .bind(project_id)
                .bind(before_flag.id)
                .fetch_all(&mut *tx)
                .await?;
                for row in &prereq_rows {
                    let ref_flag: String = row.get("ref_flag_key");
                    let ref_env: String = row.get("env_key");
                    let body: FlagEnvConfigBody = serde_json::from_value(row.get("config_jsonb"))
                        .map_err(|e| {
                        StorageError::Validation(DomainError::Validation(e.to_string()))
                    })?;
                    if body
                        .prerequisites
                        .iter()
                        .any(|p| p.flag_key == before_flag.key && p.variation_id == *variation_id)
                    {
                        refs.push(format!("{ref_env}:prerequisite({ref_flag})"));
                    }
                }
                if !refs.is_empty() {
                    return Err(StorageError::VariationInUse(refs));
                }
            }
        }

        // Write-time prerequisite existence + DAG cycle validation
        if !patch_result.env_config.config.prerequisites.is_empty() {
            let env_flags = sqlx::query(
                "SELECT f.key, f.variations_jsonb, c.config_jsonb
                 FROM flags f
                 JOIN flag_env_configs c ON c.flag_id = f.id AND c.env_id = $3
                 WHERE f.org_id = $1 AND f.project_id = $2",
            )
            .bind(org_id)
            .bind(project_id)
            .bind(env.id)
            .fetch_all(&mut *tx)
            .await?;

            let mut vars_by_flag: HashMap<String, Vec<Variation>> = HashMap::new();
            let mut adj: HashMap<String, Vec<String>> = HashMap::new();
            for row in &env_flags {
                let k: String = row.get("key");
                let vars: Vec<Variation> = serde_json::from_value(row.get("variations_jsonb"))
                    .map_err(|e| {
                        StorageError::Validation(DomainError::Validation(e.to_string()))
                    })?;
                let body: FlagEnvConfigBody = serde_json::from_value(row.get("config_jsonb"))
                    .map_err(|e| {
                        StorageError::Validation(DomainError::Validation(e.to_string()))
                    })?;
                vars_by_flag.insert(k.clone(), vars);
                adj.insert(
                    k,
                    body.prerequisites.into_iter().map(|p| p.flag_key).collect(),
                );
            }

            // Validate every prerequisite references an existing flag & valid variationId
            for prereq in &patch_result.env_config.config.prerequisites {
                let target_vars = vars_by_flag.get(&prereq.flag_key).ok_or_else(|| {
                    StorageError::Validation(DomainError::PrerequisiteFlagNotFound(
                        prereq.flag_key.clone(),
                    ))
                })?;
                if !target_vars.iter().any(|v| v.id == prereq.variation_id) {
                    return Err(StorageError::Validation(DomainError::VariationNotFound(
                        prereq.variation_id,
                    )));
                }
            }

            adj.insert(
                before_flag.key.clone(),
                patch_result
                    .env_config
                    .config
                    .prerequisites
                    .iter()
                    .map(|p| p.flag_key.clone())
                    .collect(),
            );
            if detect_prerequisite_cycle(&before_flag.key, &adj).is_err() {
                return Err(StorageError::PrerequisiteCycle);
            }
        }

        // Persist project-level flag changes if any
        let next_flag = if patch_result.flag_changed {
            let f = &patch_result.flag;
            let row = sqlx::query(
                "UPDATE flags
                 SET name = $3, description = $4, variations_jsonb = $5, temporary = $6,
                     client_side_availability_jsonb = $7, tags = $8, maintainer_id = $9,
                     archived = $10, updated_at = now()
                 WHERE org_id = $1 AND id = $2
                 RETURNING id, project_id, key, name, description, kind, variations_jsonb, temporary,
                           client_side_availability_jsonb, tags, maintainer_id, archived, created_at, updated_at",
            )
            .bind(org_id)
            .bind(f.id)
            .bind(&f.name)
            .bind(&f.description)
            .bind(sqlx::types::Json(&f.variations))
            .bind(f.temporary)
            .bind(sqlx::types::Json(f.client_side_availability))
            .bind(&f.tags)
            .bind(f.maintainer_id)
            .bind(f.archived)
            .fetch_one(&mut *tx)
            .await?;
            parse_flag_row(&row)?
        } else {
            before_flag.clone()
        };

        // Always bump flag_env_configs.version + environments.revision on patch
        let next_version = before_cfg.version + 1;
        let updated_cfg_row = sqlx::query(
            "UPDATE flag_env_configs
             SET version = $4, \"on\" = $5, config_jsonb = $6, updated_at = now()
             WHERE org_id = $1 AND flag_id = $2 AND env_id = $3
             RETURNING flag_id, env_id, version, \"on\", salt, config_jsonb, updated_at",
        )
        .bind(org_id)
        .bind(next_flag.id)
        .bind(env.id)
        .bind(next_version)
        .bind(patch_result.env_config.on)
        .bind(sqlx::types::Json(&patch_result.env_config.config))
        .fetch_one(&mut *tx)
        .await?;
        let next_cfg = parse_flag_env_config_row(&updated_cfg_row)?;

        let instructions_val = serde_json::to_value(instructions).unwrap_or(json!([]));
        sqlx::query(
            "INSERT INTO flag_env_revisions (id, org_id, flag_id, env_id, version, \"on\", config_jsonb, instructions_jsonb, actor_id, comment)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(Uuid::new_v4())
        .bind(org_id)
        .bind(next_flag.id)
        .bind(env.id)
        .bind(next_version)
        .bind(next_cfg.on)
        .bind(sqlx::types::Json(&next_cfg.config))
        .bind(&instructions_val)
        .bind(actor.actor_id)
        .bind(comment.map(str::trim).filter(|s| !s.is_empty()))
        .execute(&mut *tx)
        .await?;

        let new_env_rev: i64 = sqlx::query_scalar(
            "UPDATE environments SET revision = revision + 1 WHERE id = $1 RETURNING revision",
        )
        .bind(env.id)
        .fetch_one(&mut *tx)
        .await?;

        insert_audit_log(
            &mut tx,
            org_id,
            Some(project_id),
            Some(env.id),
            "flag",
            &next_flag.key,
            "patch",
            actor,
            comment.map(str::trim).filter(|s| !s.is_empty()),
            Some(instructions_val),
            Some(json!({ "flag": before_flag, "envConfig": before_cfg })),
            Some(json!({ "flag": next_flag, "envConfig": next_cfg })),
        )
        .await?;

        let client_side_affected = before_flag.client_side_availability.using_environment_id
            || before_flag.client_side_availability.using_mobile_key
            || next_flag.client_side_availability.using_environment_id
            || next_flag.client_side_availability.using_mobile_key;
        insert_outbox_event(
            &mut tx,
            org_id,
            Some(env.id),
            "flag.updated",
            json!({
                "flagId": next_flag.id,
                "flagKey": next_flag.key,
                "envId": env.id,
                "version": next_version,
                "envRevision": new_env_rev,
                "clientSideAffected": client_side_affected
            }),
        )
        .await?;

        tx.commit().await?;
        Ok(PatchFlagOutcome {
            flag: next_flag,
            env_config: next_cfg,
            summaries: patch_result.summaries,
        })
    }

    pub async fn list_flag_revisions(
        &self,
        org_id: Uuid,
        project_key: &str,
        flag_key: &str,
        env_key: &str,
        limit: i64,
    ) -> Result<Vec<FlagEnvRevision>, StorageError> {
        let project = self.get_project(org_id, project_key).await?;
        let env = self.get_environment(org_id, project_key, env_key).await?;
        let flag_id: Uuid = sqlx::query_scalar(
            "SELECT id FROM flags WHERE org_id = $1 AND project_id = $2 AND key = $3",
        )
        .bind(org_id)
        .bind(project.id)
        .bind(flag_key)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("flag '{flag_key}'")))?;

        let capped = limit.clamp(1, 200);
        let rows = sqlx::query(
            "SELECT id, flag_id, env_id, version, \"on\", config_jsonb, instructions_jsonb, actor_id, comment, created_at
             FROM flag_env_revisions
             WHERE org_id = $1 AND flag_id = $2 AND env_id = $3
             ORDER BY version DESC
             LIMIT $4",
        )
        .bind(org_id)
        .bind(flag_id)
        .bind(env.id)
        .bind(capped)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(parse_flag_revision_row).collect()
    }

    /// Restores a historical flag revision as a **new monotonic version `current + 1`**.
    #[allow(clippy::too_many_arguments)]
    pub async fn restore_flag_revision(
        &self,
        org_id: Uuid,
        project_key: &str,
        flag_key: &str,
        env_key: &str,
        target_version: i64,
        comment: Option<&str>,
        actor: &AuditActor,
    ) -> Result<FlagEnvConfig, StorageError> {
        let mut tx = self.pool.begin().await?;
        let project_id: Uuid =
            sqlx::query_scalar("SELECT id FROM projects WHERE org_id = $1 AND key = $2")
                .bind(org_id)
                .bind(project_key)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| StorageError::NotFound(format!("project '{project_key}'")))?;

        let env_row = sqlx::query(
            "SELECT id, project_id, key, name, color, client_side_id, require_comments, confirm_changes, revision, created_at
             FROM environments WHERE org_id = $1 AND project_id = $2 AND key = $3 FOR UPDATE",
        )
        .bind(org_id)
        .bind(project_id)
        .bind(env_key)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("environment '{env_key}'")))?;
        let env = parse_environment_row(&env_row)?;

        if env.require_comments && comment.map(str::trim).unwrap_or("").is_empty() {
            return Err(StorageError::CommentRequired);
        }

        let flag_row = sqlx::query(
            "SELECT id, project_id, key, name, description, kind, variations_jsonb, temporary,
                    client_side_availability_jsonb, tags, maintainer_id, archived, created_at, updated_at
             FROM flags WHERE org_id = $1 AND project_id = $2 AND key = $3 FOR UPDATE",
        )
        .bind(org_id)
        .bind(project_id)
        .bind(flag_key)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("flag '{flag_key}'")))?;
        let flag = parse_flag_row(&flag_row)?;

        let current_row = sqlx::query(
            "SELECT flag_id, env_id, version, \"on\", salt, config_jsonb, updated_at
             FROM flag_env_configs WHERE org_id = $1 AND flag_id = $2 AND env_id = $3 FOR UPDATE",
        )
        .bind(org_id)
        .bind(flag.id)
        .bind(env.id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("flag config '{flag_key}'")))?;
        let before_cfg = parse_flag_env_config_row(&current_row)?;

        let rev_row = sqlx::query(
            "SELECT id, flag_id, env_id, version, \"on\", config_jsonb, instructions_jsonb, actor_id, comment, created_at
             FROM flag_env_revisions
             WHERE org_id = $1 AND flag_id = $2 AND env_id = $3 AND version = $4",
        )
        .bind(org_id)
        .bind(flag.id)
        .bind(env.id)
        .bind(target_version)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("revision v{target_version}")))?;
        let target_rev = parse_flag_revision_row(&rev_row)?;

        skotos_domain::validate_flag_env_config(&flag, &target_rev.config)?;

        let next_version = before_cfg.version + 1;
        let updated_row = sqlx::query(
            "UPDATE flag_env_configs
             SET version = $4, \"on\" = $5, config_jsonb = $6, updated_at = now()
             WHERE org_id = $1 AND flag_id = $2 AND env_id = $3
             RETURNING flag_id, env_id, version, \"on\", salt, config_jsonb, updated_at",
        )
        .bind(org_id)
        .bind(flag.id)
        .bind(env.id)
        .bind(next_version)
        .bind(target_rev.on)
        .bind(sqlx::types::Json(&target_rev.config))
        .fetch_one(&mut *tx)
        .await?;
        let next_cfg = parse_flag_env_config_row(&updated_row)?;

        let effective_comment = comment
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_else(|| format!("Restored from version {target_version}"));
        let instr_val = json!([{ "kind": "restoreRevision", "fromVersion": target_version }]);

        sqlx::query(
            "INSERT INTO flag_env_revisions (id, org_id, flag_id, env_id, version, \"on\", config_jsonb, instructions_jsonb, actor_id, comment)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(Uuid::new_v4())
        .bind(org_id)
        .bind(flag.id)
        .bind(env.id)
        .bind(next_version)
        .bind(next_cfg.on)
        .bind(sqlx::types::Json(&next_cfg.config))
        .bind(&instr_val)
        .bind(actor.actor_id)
        .bind(&effective_comment)
        .execute(&mut *tx)
        .await?;

        let new_env_rev: i64 = sqlx::query_scalar(
            "UPDATE environments SET revision = revision + 1 WHERE id = $1 RETURNING revision",
        )
        .bind(env.id)
        .fetch_one(&mut *tx)
        .await?;

        insert_audit_log(
            &mut tx,
            org_id,
            Some(project_id),
            Some(env.id),
            "flag",
            &flag.key,
            "restore",
            actor,
            Some(&effective_comment),
            Some(instr_val),
            Some(serde_json::to_value(&before_cfg).unwrap_or(Value::Null)),
            Some(serde_json::to_value(&next_cfg).unwrap_or(Value::Null)),
        )
        .await?;

        insert_outbox_event(
            &mut tx,
            org_id,
            Some(env.id),
            "flag.restored",
            json!({
                "flagId": flag.id,
                "flagKey": flag.key,
                "envId": env.id,
                "version": next_version,
                "restoredFromVersion": target_version,
                "envRevision": new_env_rev
            }),
        )
        .await?;

        tx.commit().await?;
        Ok(next_cfg)
    }

    pub async fn delete_flag(
        &self,
        org_id: Uuid,
        project_key: &str,
        flag_key: &str,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let project_id: Uuid =
            sqlx::query_scalar("SELECT id FROM projects WHERE org_id = $1 AND key = $2")
                .bind(org_id)
                .bind(project_key)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| StorageError::NotFound(format!("project '{project_key}'")))?;

        let flag_row = sqlx::query(
            "SELECT id, project_id, key, name, description, kind, variations_jsonb, temporary,
                    client_side_availability_jsonb, tags, maintainer_id, archived, created_at, updated_at
             FROM flags WHERE org_id = $1 AND project_id = $2 AND key = $3 FOR UPDATE",
        )
        .bind(org_id)
        .bind(project_id)
        .bind(flag_key)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("flag '{flag_key}'")))?;
        let before = parse_flag_row(&flag_row)?;

        let env_versions: Vec<(Uuid, i64)> = sqlx::query_as(
            "SELECT env_id, version FROM flag_env_configs WHERE org_id = $1 AND flag_id = $2",
        )
        .bind(org_id)
        .bind(before.id)
        .fetch_all(&mut *tx)
        .await?;
        let version_map: HashMap<Uuid, i64> = env_versions.into_iter().collect();

        let env_rows: Vec<(Uuid, i64)> = sqlx::query_as(
            "UPDATE environments SET revision = revision + 1 WHERE org_id = $1 AND project_id = $2 RETURNING id, revision",
        )
        .bind(org_id)
        .bind(project_id)
        .fetch_all(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM flags WHERE org_id = $1 AND id = $2")
            .bind(org_id)
            .bind(before.id)
            .execute(&mut *tx)
            .await?;

        insert_audit_log(
            &mut tx,
            org_id,
            Some(project_id),
            None,
            "flag",
            &before.key,
            "delete",
            actor,
            None,
            None,
            Some(serde_json::to_value(&before).unwrap_or(Value::Null)),
            None,
        )
        .await?;

        let client_side_affected = before.client_side_availability.any_enabled();
        for (env_id, env_rev) in env_rows {
            let tombstone_version = version_map.get(&env_id).copied().unwrap_or(1) + 1;
            insert_outbox_event(
                &mut tx,
                org_id,
                Some(env_id),
                "flag.deleted",
                json!({
                    "flagId": before.id,
                    "flagKey": before.key,
                    "envId": env_id,
                    "version": tombstone_version,
                    "envRevision": env_rev,
                    "clientSideAffected": client_side_affected
                }),
            )
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Loads all flags + env configs + segments in an environment (used by `POST .../simulate`).
    pub async fn load_environment_eval_snapshot(
        &self,
        org_id: Uuid,
        project_key: &str,
        env_key: &str,
    ) -> Result<(Environment, Vec<(FeatureFlag, FlagEnvConfig)>, Vec<Segment>), StorageError> {
        let project = self.get_project(org_id, project_key).await?;
        let env = self.get_environment(org_id, project_key, env_key).await?;

        let flag_rows = sqlx::query(
            "SELECT f.id, f.project_id, f.key, f.name, f.description, f.kind, f.variations_jsonb,
                    f.temporary, f.client_side_availability_jsonb, f.tags, f.maintainer_id,
                    f.archived, f.created_at, f.updated_at,
                    c.version, c.\"on\", c.salt, c.config_jsonb, c.updated_at AS cfg_updated_at
             FROM flags f
             JOIN flag_env_configs c ON c.flag_id = f.id AND c.env_id = $3
             WHERE f.org_id = $1 AND f.project_id = $2",
        )
        .bind(org_id)
        .bind(project.id)
        .bind(env.id)
        .fetch_all(&self.pool)
        .await?;

        let mut flags_with_cfg = Vec::with_capacity(flag_rows.len());
        for row in &flag_rows {
            let flag = parse_flag_row(row)?;
            let cfg = FlagEnvConfig {
                flag_id: flag.id,
                env_id: env.id,
                version: row.get("version"),
                on: row.get("on"),
                salt: row.get("salt"),
                config: serde_json::from_value(row.get("config_jsonb")).map_err(|e| {
                    StorageError::Validation(DomainError::Validation(e.to_string()))
                })?,
                updated_at: row.get("cfg_updated_at"),
            };
            flags_with_cfg.push((flag, cfg));
        }

        let seg_rows = sqlx::query(
            "SELECT id, env_id, key, name, kind, version, salt, segment_jsonb, updated_at
             FROM segments WHERE org_id = $1 AND env_id = $2",
        )
        .bind(org_id)
        .bind(env.id)
        .fetch_all(&self.pool)
        .await?;

        let mut segments = Vec::with_capacity(seg_rows.len());
        for row in &seg_rows {
            segments.push(parse_segment_row(row)?);
        }

        Ok((env, flags_with_cfg, segments))
    }

    // =========================================================================
    // Audit Log & Outbox Queries
    // =========================================================================

    pub async fn list_audit_logs(
        &self,
        org_id: Uuid,
        filter: AuditLogFilter,
    ) -> Result<AuditLogPage, StorageError> {
        let limit = filter.limit.clamp(1, 100);
        let cursor_decoded: Option<AuditCursorPayload> = match filter.cursor {
            Some(ref raw) if !raw.is_empty() => {
                let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|e| {
                    StorageError::Validation(DomainError::Validation(e.to_string()))
                })?;
                let parsed: AuditCursorPayload = serde_json::from_slice(&bytes).map_err(|e| {
                    StorageError::Validation(DomainError::Validation(e.to_string()))
                })?;
                Some(parsed)
            }
            _ => None,
        };

        let rows = sqlx::query(
            "SELECT id, org_id, project_id, env_id, resource_type, resource_key, action,
                    actor_id, actor_type, actor_email, comment, instructions_jsonb,
                    before_jsonb, after_jsonb, created_at
             FROM audit_logs
             WHERE org_id = $1
               AND ($2::uuid IS NULL OR project_id = $2)
               AND ($3::uuid IS NULL OR env_id = $3)
               AND ($4::text IS NULL OR resource_type = $4)
               AND ($5::text IS NULL OR resource_key = $5)
               AND ($6::uuid IS NULL OR actor_id = $6)
               AND ($7::timestamptz IS NULL OR (created_at, id) < ($7, $8))
             ORDER BY created_at DESC, id DESC
             LIMIT $9",
        )
        .bind(org_id)
        .bind(filter.project_id)
        .bind(filter.env_id)
        .bind(filter.resource_type)
        .bind(filter.resource_key)
        .bind(filter.actor_id)
        .bind(cursor_decoded.as_ref().map(|c| c.ts))
        .bind(cursor_decoded.as_ref().map(|c| c.id))
        .bind(limit + 1)
        .fetch_all(&self.pool)
        .await?;

        let has_more = rows.len() as i64 > limit;
        let slice = if has_more {
            &rows[..limit as usize]
        } else {
            &rows[..]
        };

        let items: Vec<AuditLogEntry> = slice
            .iter()
            .map(parse_audit_log_row)
            .collect::<Result<_, _>>()?;

        let next_cursor = if has_more {
            items.last().and_then(|last| {
                serde_json::to_vec(&AuditCursorPayload {
                    ts: last.created_at,
                    id: last.id,
                })
                .ok()
                .map(|b| URL_SAFE_NO_PAD.encode(b))
            })
        } else {
            None
        };

        Ok(AuditLogPage {
            items,
            next_cursor,
            has_more,
        })
    }

    pub async fn list_outbox_events(
        &self,
        org_id: Uuid,
        limit: i64,
    ) -> Result<Vec<OutboxEvent>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, org_id, env_id, event_type, payload_jsonb, created_at, published_at
             FROM outbox_events WHERE org_id = $1 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(org_id)
        .bind(limit.clamp(1, 200))
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| OutboxEvent {
                id: r.get("id"),
                org_id: r.get("org_id"),
                env_id: r.get("env_id"),
                event_type: r.get("event_type"),
                payload: r.get("payload_jsonb"),
                created_at: r.get("created_at"),
                published_at: r.get("published_at"),
            })
            .collect())
    }

    // =========================================================================
    // API Access Tokens
    // =========================================================================

    pub async fn list_api_tokens(&self, org_id: Uuid) -> Result<Vec<ApiToken>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, org_id, name, key_prefix, role, created_by, last_used_at, expires_at, created_at
             FROM api_tokens WHERE org_id = $1 ORDER BY created_at DESC",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(parse_api_token_row).collect()
    }

    pub async fn create_api_token(
        &self,
        org_id: Uuid,
        name: &str,
        role: BuiltInRole,
        expires_at: Option<DateTime<Utc>>,
        actor: &AuditActor,
    ) -> Result<CreatedApiTokenOutcome, StorageError> {
        if name.trim().is_empty() {
            return Err(StorageError::Validation(DomainError::Validation(
                "token name cannot be empty".to_string(),
            )));
        }
        let generated = generate_prefixed_secret("sk_api_");
        let mut tx = self.pool.begin().await?;
        let id = Uuid::new_v4();
        let row = sqlx::query(
            "INSERT INTO api_tokens (id, org_id, name, key_prefix, key_hash, role, created_by, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING id, org_id, name, key_prefix, role, created_by, last_used_at, expires_at, created_at",
        )
        .bind(id)
        .bind(org_id)
        .bind(name.trim())
        .bind(&generated.prefix)
        .bind(&generated.hash)
        .bind(role.as_str())
        .bind(actor.actor_id)
        .bind(expires_at)
        .fetch_one(&mut *tx)
        .await?;
        let token = parse_api_token_row(&row)?;

        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "apiToken",
            &token.name,
            "create",
            actor,
            None,
            None,
            None,
            Some(serde_json::to_value(&token).unwrap_or(Value::Null)),
        )
        .await?;
        tx.commit().await?;
        Ok(CreatedApiTokenOutcome {
            token,
            plaintext_token: generated.plaintext,
        })
    }

    pub async fn verify_api_token(
        &self,
        plaintext: &str,
    ) -> Result<Option<ApiToken>, StorageError> {
        let hash = sha256_hex(plaintext);
        let row = sqlx::query(
            "UPDATE api_tokens
             SET last_used_at = now()
             WHERE key_hash = $1 AND (expires_at IS NULL OR expires_at > now())
             RETURNING id, org_id, name, key_prefix, role, created_by, last_used_at, expires_at, created_at",
        )
        .bind(hash)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(parse_api_token_row).transpose()
    }

    pub async fn revoke_api_token(
        &self,
        org_id: Uuid,
        token_id: Uuid,
        actor: &AuditActor,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT id, org_id, name, key_prefix, role, created_by, last_used_at, expires_at, created_at
             FROM api_tokens WHERE org_id = $1 AND id = $2 FOR UPDATE",
        )
        .bind(org_id)
        .bind(token_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("api token {token_id}")))?;
        let before = parse_api_token_row(&row)?;

        sqlx::query("DELETE FROM api_tokens WHERE org_id = $1 AND id = $2")
            .bind(org_id)
            .bind(token_id)
            .execute(&mut *tx)
            .await?;

        insert_audit_log(
            &mut tx,
            org_id,
            None,
            None,
            "apiToken",
            &before.name,
            "revoke",
            actor,
            None,
            None,
            Some(serde_json::to_value(&before).unwrap_or(Value::Null)),
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

// =============================================================================
// Internal transaction & row-parsing helpers
// =============================================================================

#[allow(clippy::too_many_arguments)]
async fn insert_environment_tx(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    project_id: Uuid,
    key: &str,
    name: &str,
    color: &str,
    require_comments: bool,
    confirm_changes: bool,
) -> Result<Environment, StorageError> {
    let env_id = Uuid::new_v4();
    let client_side_id = generate_client_side_id();
    let row = sqlx::query(
        "INSERT INTO environments (id, org_id, project_id, key, name, color, client_side_id, require_comments, confirm_changes)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING id, project_id, key, name, color, client_side_id, require_comments, confirm_changes, revision, created_at",
    )
    .bind(env_id)
    .bind(org_id)
    .bind(project_id)
    .bind(key)
    .bind(name)
    .bind(color)
    .bind(&client_side_id)
    .bind(require_comments)
    .bind(confirm_changes)
    .fetch_one(&mut **tx)
    .await
    .map_err(map_unique_err("environment"))?;
    let env = parse_environment_row(&row)?;

    // Issue initial SDK key and Mobile key for the new environment
    for (kind, prefix) in [
        (EnvironmentKeyKind::Sdk, "sk_sdk_"),
        (EnvironmentKeyKind::Mobile, "sk_mob_"),
    ] {
        let secret = generate_prefixed_secret(prefix);
        sqlx::query(
            "INSERT INTO environment_keys (id, org_id, env_id, kind, key_prefix, key_hash)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::new_v4())
        .bind(org_id)
        .bind(env.id)
        .bind(kind.as_str())
        .bind(&secret.prefix)
        .bind(&secret.hash)
        .execute(&mut **tx)
        .await?;
    }

    Ok(env)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert_audit_log(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    project_id: Option<Uuid>,
    env_id: Option<Uuid>,
    resource_type: &str,
    resource_key: &str,
    action: &str,
    actor: &AuditActor,
    comment: Option<&str>,
    instructions: Option<Value>,
    before: Option<Value>,
    after: Option<Value>,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO audit_logs (
            id, org_id, project_id, env_id, resource_type, resource_key, action,
            actor_id, actor_type, actor_email, comment, instructions_jsonb, before_jsonb, after_jsonb
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(Uuid::new_v4())
    .bind(org_id)
    .bind(project_id)
    .bind(env_id)
    .bind(resource_type)
    .bind(resource_key)
    .bind(action)
    .bind(actor.actor_id)
    .bind(&actor.actor_type)
    .bind(&actor.actor_email)
    .bind(comment)
    .bind(instructions)
    .bind(before)
    .bind(after)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(crate) async fn insert_outbox_event(
    tx: &mut Transaction<'_, Postgres>,
    org_id: Uuid,
    env_id: Option<Uuid>,
    event_type: &str,
    payload: Value,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO outbox_events (id, org_id, env_id, event_type, payload_jsonb)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::new_v4())
    .bind(org_id)
    .bind(env_id)
    .bind(event_type)
    .bind(payload)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(crate) fn map_unique_err(resource: &'static str) -> impl FnOnce(sqlx::Error) -> StorageError {
    move |err| {
        if let sqlx::Error::Database(ref db_err) = err {
            if db_err.code().as_deref() == Some("23505") {
                return StorageError::AlreadyExists(format!("{resource} already exists"));
            }
        }
        StorageError::Database(err)
    }
}

fn plan_tier_str(tier: PlanTier) -> &'static str {
    match tier {
        PlanTier::Free => "free",
        PlanTier::Pro => "pro",
        PlanTier::Team => "team",
        PlanTier::Enterprise => "enterprise",
    }
}

fn parse_plan_tier(s: &str) -> Result<PlanTier, StorageError> {
    match s {
        "free" => Ok(PlanTier::Free),
        "pro" => Ok(PlanTier::Pro),
        "team" => Ok(PlanTier::Team),
        "enterprise" => Ok(PlanTier::Enterprise),
        other => Err(StorageError::Validation(DomainError::Validation(format!(
            "unknown plan tier '{other}'"
        )))),
    }
}

fn parse_role(s: &str) -> Result<BuiltInRole, StorageError> {
    match s {
        "reader" => Ok(BuiltInRole::Reader),
        "writer" => Ok(BuiltInRole::Writer),
        "admin" => Ok(BuiltInRole::Admin),
        "owner" => Ok(BuiltInRole::Owner),
        other => Err(StorageError::Validation(DomainError::Validation(format!(
            "unknown role '{other}'"
        )))),
    }
}

fn parse_flag_kind(s: &str) -> Result<FlagKind, StorageError> {
    match s {
        "boolean" => Ok(FlagKind::Boolean),
        "string" => Ok(FlagKind::String),
        "number" => Ok(FlagKind::Number),
        "json" => Ok(FlagKind::Json),
        other => Err(StorageError::Validation(DomainError::Validation(format!(
            "unknown flag kind '{other}'"
        )))),
    }
}

fn parse_organization_row(row: &sqlx::postgres::PgRow) -> Result<Organization, StorageError> {
    Ok(Organization {
        id: row.get("id"),
        slug: row.get("slug"),
        name: row.get("name"),
        external_auth_org_id: row.get("external_auth_org_id"),
        plan_tier: parse_plan_tier(row.get::<String, _>("plan_tier").as_str())?,
        created_at: row.get("created_at"),
    })
}

fn parse_member_row(row: &sqlx::postgres::PgRow) -> Result<Member, StorageError> {
    Ok(Member {
        id: row.get("id"),
        org_id: row.get("org_id"),
        external_subject_id: row.get("external_subject_id"),
        email: row.get("email"),
        display_name: row.get("display_name"),
        role: parse_role(row.get::<String, _>("role").as_str())?,
        custom_role_keys: Vec::new(),
        created_at: row.get("created_at"),
    })
}

fn parse_member_with_password(
    row: &sqlx::postgres::PgRow,
) -> Result<MemberWithPassword, StorageError> {
    Ok(MemberWithPassword {
        member: parse_member_row(row)?,
        password_hash: row.get("password_hash"),
    })
}

fn parse_project_row(row: &sqlx::postgres::PgRow) -> Result<Project, StorageError> {
    let csa_val: Value = row.get("default_client_side_availability");
    let default_client_side_availability: ClientSideAvailability = serde_json::from_value(csa_val)
        .map_err(|e| StorageError::Validation(DomainError::Validation(e.to_string())))?;
    Ok(Project {
        id: row.get("id"),
        org_id: row.get("org_id"),
        key: row.get("key"),
        name: row.get("name"),
        default_client_side_availability,
        created_at: row.get("created_at"),
    })
}

pub(crate) fn parse_environment_row(
    row: &sqlx::postgres::PgRow,
) -> Result<Environment, StorageError> {
    let approval_settings = skotos_domain::EnvironmentApprovalSettings {
        required: row.try_get("approval_required").unwrap_or(false),
        min_approvals: row.try_get("min_approvals").unwrap_or(1),
        allow_self_approval: row.try_get("allow_self_approval").unwrap_or(false),
        external_approval_webhook_url: row.try_get("external_approval_webhook_url").unwrap_or(None),
    };
    Ok(Environment {
        id: row.get("id"),
        project_id: row.get("project_id"),
        key: row.get("key"),
        name: row.get("name"),
        color: row.get("color"),
        client_side_id: row.get("client_side_id"),
        require_comments: row.get("require_comments"),
        confirm_changes: row.get("confirm_changes"),
        approval_settings,
        revision: row.get("revision"),
        created_at: row.get("created_at"),
    })
}

fn parse_env_key_row(row: &sqlx::postgres::PgRow) -> Result<EnvironmentKey, StorageError> {
    let kind_str: String = row.get("kind");
    let kind = match kind_str.as_str() {
        "sdk" => EnvironmentKeyKind::Sdk,
        "mobile" => EnvironmentKeyKind::Mobile,
        other => {
            return Err(StorageError::Validation(DomainError::Validation(format!(
                "unknown env key kind '{other}'"
            ))));
        }
    };
    Ok(EnvironmentKey {
        id: row.get("id"),
        env_id: row.get("env_id"),
        kind,
        key_prefix: row.get("key_prefix"),
        expires_at: row.get("expires_at"),
        created_at: row.get("created_at"),
    })
}

pub(crate) fn parse_flag_row(row: &sqlx::postgres::PgRow) -> Result<FeatureFlag, StorageError> {
    let variations: Vec<Variation> = serde_json::from_value(row.get("variations_jsonb"))
        .map_err(|e| StorageError::Validation(DomainError::Validation(e.to_string())))?;
    let csa: ClientSideAvailability =
        serde_json::from_value(row.get("client_side_availability_jsonb"))
            .map_err(|e| StorageError::Validation(DomainError::Validation(e.to_string())))?;
    Ok(FeatureFlag {
        id: row.get("id"),
        project_id: row.get("project_id"),
        key: row.get("key"),
        name: row.get("name"),
        description: row.get("description"),
        kind: parse_flag_kind(row.get::<String, _>("kind").as_str())?,
        variations,
        temporary: row.get("temporary"),
        client_side_availability: csa,
        tags: row.get("tags"),
        maintainer_id: row.get("maintainer_id"),
        archived: row.get("archived"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

pub(crate) fn parse_flag_env_config_row(
    row: &sqlx::postgres::PgRow,
) -> Result<FlagEnvConfig, StorageError> {
    let config: FlagEnvConfigBody = serde_json::from_value(row.get("config_jsonb"))
        .map_err(|e| StorageError::Validation(DomainError::Validation(e.to_string())))?;
    Ok(FlagEnvConfig {
        flag_id: row.get("flag_id"),
        env_id: row.get("env_id"),
        version: row.get("version"),
        on: row.get("on"),
        salt: row.get("salt"),
        config,
        updated_at: row.get("updated_at"),
    })
}

fn parse_flag_revision_row(row: &sqlx::postgres::PgRow) -> Result<FlagEnvRevision, StorageError> {
    let config: FlagEnvConfigBody = serde_json::from_value(row.get("config_jsonb"))
        .map_err(|e| StorageError::Validation(DomainError::Validation(e.to_string())))?;
    Ok(FlagEnvRevision {
        id: row.get("id"),
        flag_id: row.get("flag_id"),
        env_id: row.get("env_id"),
        version: row.get("version"),
        on: row.get("on"),
        config,
        instructions: row.get("instructions_jsonb"),
        actor_id: row.get("actor_id"),
        comment: row.get("comment"),
        created_at: row.get("created_at"),
    })
}

pub(crate) fn parse_segment_row(row: &sqlx::postgres::PgRow) -> Result<Segment, StorageError> {
    #[derive(Deserialize, Default)]
    #[serde(rename_all = "camelCase")]
    struct SegBody {
        #[serde(default)]
        included: Vec<String>,
        #[serde(default)]
        excluded: Vec<String>,
        #[serde(default)]
        included_contexts: Vec<skotos_domain::SegmentTarget>,
        #[serde(default)]
        excluded_contexts: Vec<skotos_domain::SegmentTarget>,
        #[serde(default)]
        rules: Vec<skotos_domain::SegmentRule>,
    }
    let body: SegBody = serde_json::from_value(row.get("segment_jsonb"))
        .map_err(|e| StorageError::Validation(DomainError::Validation(e.to_string())))?;
    let kind_str: String = row.get("kind");
    let kind = match kind_str.as_str() {
        "standard" => SegmentKind::Standard,
        "big" => SegmentKind::Big,
        "synced" => SegmentKind::Synced,
        _ => SegmentKind::Standard,
    };
    Ok(Segment {
        id: row.get("id"),
        env_id: row.get("env_id"),
        key: row.get("key"),
        name: row.get("name"),
        kind,
        version: row.get("version"),
        salt: row.get("salt"),
        included: body.included,
        excluded: body.excluded,
        included_contexts: body.included_contexts,
        excluded_contexts: body.excluded_contexts,
        rules: body.rules,
        updated_at: row.get("updated_at"),
    })
}

fn parse_audit_log_row(row: &sqlx::postgres::PgRow) -> Result<AuditLogEntry, StorageError> {
    Ok(AuditLogEntry {
        id: row.get("id"),
        org_id: row.get("org_id"),
        project_id: row.get("project_id"),
        env_id: row.get("env_id"),
        resource_type: row.get("resource_type"),
        resource_key: row.get("resource_key"),
        action: row.get("action"),
        actor_id: row.get("actor_id"),
        actor_type: row.get("actor_type"),
        actor_email: row.get("actor_email"),
        comment: row.get("comment"),
        instructions: row.get("instructions_jsonb"),
        before: row.get("before_jsonb"),
        after: row.get("after_jsonb"),
        created_at: row.get("created_at"),
    })
}

fn parse_api_token_row(row: &sqlx::postgres::PgRow) -> Result<ApiToken, StorageError> {
    Ok(ApiToken {
        id: row.get("id"),
        org_id: row.get("org_id"),
        name: row.get("name"),
        key_prefix: row.get("key_prefix"),
        role: parse_role(row.get::<String, _>("role").as_str())?,
        created_by: row.get("created_by"),
        last_used_at: row.get("last_used_at"),
        expires_at: row.get("expires_at"),
        created_at: row.get("created_at"),
    })
}

/// Picks the fallthrough ("on") and off variations for a flag in a newly created environment.
///
/// Prefers the defaults already used by the flag in another environment. Without one, boolean
/// flags serve `true` when on and `false` when off; other flags fall back to the first and
/// second variation.
fn default_on_off_variations(
    variations: &[Variation],
    existing_on: Option<Uuid>,
    existing_off: Option<Uuid>,
    first_var: Uuid,
) -> (Uuid, Uuid) {
    let by_value = |want: bool| {
        variations
            .iter()
            .find(|v| v.value == Value::Bool(want))
            .map(|v| v.id)
    };
    let on = existing_on.or_else(|| by_value(true)).unwrap_or(first_var);
    let off = existing_off
        .or_else(|| by_value(false))
        .or_else(|| variations.get(1).map(|v| v.id))
        .unwrap_or(on);
    (on, off)
}

#[cfg(test)]
mod default_variation_tests {
    use super::*;

    fn var(value: Value) -> Variation {
        Variation {
            id: Uuid::new_v4(),
            value,
            name: None,
            description: None,
        }
    }

    #[test]
    fn boolean_flags_serve_true_on_and_false_off_regardless_of_order() {
        let vars = vec![var(json!(false)), var(json!(true))];
        let (on, off) = default_on_off_variations(&vars, None, None, vars[0].id);
        assert_eq!(on, vars[1].id);
        assert_eq!(off, vars[0].id);
    }

    #[test]
    fn existing_environment_defaults_win() {
        let vars = vec![var(json!("a")), var(json!("b")), var(json!("c"))];
        let (on, off) =
            default_on_off_variations(&vars, Some(vars[2].id), Some(vars[0].id), vars[0].id);
        assert_eq!(on, vars[2].id);
        assert_eq!(off, vars[0].id);
    }

    #[test]
    fn multivariate_flags_fall_back_to_first_and_second() {
        let vars = vec![var(json!("a")), var(json!("b"))];
        let (on, off) = default_on_off_variations(&vars, None, None, vars[0].id);
        assert_eq!(on, vars[0].id);
        assert_eq!(off, vars[1].id);
    }
}
