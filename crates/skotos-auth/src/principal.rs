use serde::{Deserialize, Serialize};
use skotos_domain::{BuiltInRole, PolicyStatement};
use skotos_storage::AuditActor;
use uuid::Uuid;

use crate::error::AuthzError;
use crate::policy::{evaluate_policies, PolicyDecision};

/// Active authentication deployment mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    SelfHosted,
    External,
}

impl AuthMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SelfHosted => "self_hosted",
            Self::External => "external",
        }
    }
}

/// Kind of authenticated caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActorType {
    Member,
    ApiToken,
    System,
}

impl ActorType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Member => "member",
            Self::ApiToken => "api_token",
            Self::System => "system",
        }
    }
}

/// Uniform authenticated identity yielded by BOTH Cloud SaaS (`ExternalAuthAdapter`)
/// and Self-Hosted (`SelfHostedAuthAdapter`) adapters as well as API tokens (`sk_api_...`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticatedPrincipal {
    pub org_id: Uuid,
    pub actor_type: ActorType,
    pub member_id: Option<Uuid>,
    pub api_token_id: Option<Uuid>,
    pub email: Option<String>,
    pub display_name: String,
    pub role: BuiltInRole,
    pub team_ids: Vec<Uuid>,
    pub role_ids: Vec<Uuid>,
    pub token_scopes: Vec<String>,
    /// Effective Custom RBAC allow/deny policy statements resolved from assigned custom roles & teams.
    #[serde(default)]
    pub custom_policies: Vec<PolicyStatement>,
}

impl AuthenticatedPrincipal {
    pub fn to_audit_actor(&self) -> AuditActor {
        AuditActor {
            actor_id: self.member_id.or(self.api_token_id),
            actor_type: self.actor_type.as_str().to_string(),
            actor_email: self.email.clone(),
        }
    }
}

/// Resource descriptor for authorization checks (supports Phase 3 Custom RBAC resource paths).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceRef {
    pub resource_type: String,
    pub project_key: Option<String>,
    pub env_key: Option<String>,
    pub resource_key: Option<String>,
}

impl ResourceRef {
    pub fn org() -> Self {
        Self {
            resource_type: "organization".to_string(),
            project_key: None,
            env_key: None,
            resource_key: None,
        }
    }

    pub fn project(project_key: impl Into<String>) -> Self {
        Self {
            resource_type: "project".to_string(),
            project_key: Some(project_key.into()),
            env_key: None,
            resource_key: None,
        }
    }

    pub fn environment(project_key: impl Into<String>, env_key: impl Into<String>) -> Self {
        Self {
            resource_type: "environment".to_string(),
            project_key: Some(project_key.into()),
            env_key: Some(env_key.into()),
            resource_key: None,
        }
    }

    pub fn flag(project_key: impl Into<String>, flag_key: impl Into<String>) -> Self {
        Self {
            resource_type: "flag".to_string(),
            project_key: Some(project_key.into()),
            env_key: None,
            resource_key: Some(flag_key.into()),
        }
    }

    pub fn flag_in_env(
        project_key: impl Into<String>,
        env_key: impl Into<String>,
        flag_key: impl Into<String>,
    ) -> Self {
        Self {
            resource_type: "flag".to_string(),
            project_key: Some(project_key.into()),
            env_key: Some(env_key.into()),
            resource_key: Some(flag_key.into()),
        }
    }

    pub fn segment(
        project_key: impl Into<String>,
        env_key: impl Into<String>,
        segment_key: impl Into<String>,
    ) -> Self {
        Self {
            resource_type: "segment".to_string(),
            project_key: Some(project_key.into()),
            env_key: Some(env_key.into()),
            resource_key: Some(segment_key.into()),
        }
    }

    /// Formats this resource reference into the canonical Custom RBAC path syntax:
    /// e.g. `proj/{project}:env/{env}:flag/{flag}` or `proj/{project}:env/{env}:segment/{seg}`.
    pub fn to_resource_path(&self) -> String {
        match (
            self.project_key.as_deref(),
            self.env_key.as_deref(),
            self.resource_key.as_deref(),
        ) {
            (Some(p), Some(e), Some(k)) => format!("proj/{p}:env/{e}:{}/{k}", self.resource_type),
            (Some(p), Some(e), None) => format!("proj/{p}:env/{e}"),
            (Some(p), None, Some(k)) => format!("proj/{p}:{}/{k}", self.resource_type),
            (Some(p), None, None) => format!("proj/{p}"),
            (None, None, Some(k)) => format!("{}/{k}", self.resource_type),
            _ => self.resource_type.clone(),
        }
    }
}

/// Canonical RBAC actions enforced by [`Authorizer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Read,
    SimulateFlag,
    CreateFlag,
    PatchFlag,
    RestoreFlagRevision,
    DeleteFlag,
    ApproveChange,
    ManageProject,
    ManageEnvironment,
    RotateEnvironmentKey,
    ManageApiTokens,
    ManageMembers,
    ManageOwnerMembers,
    ManageRoles,
    ManageTeams,
    ManagePipelines,
    ManageSso,
    ViewLiveEvents,
}

impl Action {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::SimulateFlag => "simulateFlag",
            Self::CreateFlag => "createFlag",
            Self::PatchFlag => "patchFlag",
            Self::RestoreFlagRevision => "restoreFlagRevision",
            Self::DeleteFlag => "deleteFlag",
            Self::ApproveChange => "approveChange",
            Self::ManageProject => "manageProject",
            Self::ManageEnvironment => "manageEnvironment",
            Self::RotateEnvironmentKey => "rotateEnvironmentKey",
            Self::ManageApiTokens => "manageApiTokens",
            Self::ManageMembers => "manageMembers",
            Self::ManageOwnerMembers => "manageOwnerMembers",
            Self::ManageRoles => "manageRoles",
            Self::ManageTeams => "manageTeams",
            Self::ManagePipelines => "managePipelines",
            Self::ManageSso => "manageSso",
            Self::ViewLiveEvents => "viewLiveEvents",
        }
    }
}

/// Stateless RBAC authorizer enforcing:
/// 1. Custom RBAC policy statements (`Deny` ALWAYS overrides `Allow` and built-in non-Owner roles;
///    `Allow` grants explicit permission even if built-in role is `Reader`).
/// 2. Built-in RBAC matrix fallback:
///    - `Reader`: Read + SimulateFlag + ViewLiveEvents
///    - `Writer`: + CreateFlag, PatchFlag, RestoreFlagRevision, ApproveChange
///    - `Admin`: + DeleteFlag, ManageProject, ManageEnvironment, RotateEnvironmentKey,
///      ManageApiTokens, ManageMembers, ManageRoles, ManageTeams, ManagePipelines
///    - `Owner`: + ManageOwnerMembers, ManageSso
#[derive(Debug, Default, Clone, Copy)]
pub struct Authorizer;

impl Authorizer {
    pub fn authorize(
        &self,
        principal: &AuthenticatedPrincipal,
        action: Action,
        resource: &ResourceRef,
    ) -> Result<(), AuthzError> {
        if !principal.custom_policies.is_empty() {
            let path = resource.to_resource_path();
            match evaluate_policies(&principal.custom_policies, action.as_str(), &path) {
                PolicyDecision::ExplicitDeny => {
                    return Err(AuthzError::Forbidden {
                        role: format!("{} (denied by custom policy)", principal.role.as_str()),
                        action: action.as_str().to_string(),
                    });
                }
                PolicyDecision::ExplicitAllow => {
                    // Explicit allow cannot escalate to Owner-only org mutations unless Owner
                    if action != Action::ManageOwnerMembers || principal.role == BuiltInRole::Owner
                    {
                        return Ok(());
                    }
                }
                PolicyDecision::NoMatch => {}
            }
        }

        let min_role = match action {
            Action::Read | Action::SimulateFlag | Action::ViewLiveEvents => BuiltInRole::Reader,
            Action::CreateFlag
            | Action::PatchFlag
            | Action::RestoreFlagRevision
            | Action::ApproveChange => BuiltInRole::Writer,
            Action::DeleteFlag
            | Action::ManageProject
            | Action::ManageEnvironment
            | Action::RotateEnvironmentKey
            | Action::ManageApiTokens
            | Action::ManageMembers
            | Action::ManageRoles
            | Action::ManageTeams
            | Action::ManagePipelines => BuiltInRole::Admin,
            Action::ManageOwnerMembers | Action::ManageSso => BuiltInRole::Owner,
        };

        if principal.role >= min_role {
            Ok(())
        } else {
            Err(AuthzError::Forbidden {
                role: principal.role.as_str().to_string(),
                action: action.as_str().to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use skotos_domain::PolicyEffect;

    use super::*;

    fn principal(role: BuiltInRole) -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            org_id: Uuid::new_v4(),
            actor_type: ActorType::Member,
            member_id: Some(Uuid::new_v4()),
            api_token_id: None,
            email: Some("user@skotos.local".to_string()),
            display_name: "Test".to_string(),
            role,
            team_ids: vec![],
            role_ids: vec![],
            token_scopes: vec![],
            custom_policies: vec![],
        }
    }

    #[test]
    fn enforces_builtin_rbac_matrix() {
        let authz = Authorizer;
        let res = ResourceRef::org();

        assert!(authz
            .authorize(&principal(BuiltInRole::Reader), Action::Read, &res)
            .is_ok());
        assert!(authz
            .authorize(&principal(BuiltInRole::Reader), Action::SimulateFlag, &res)
            .is_ok());
        assert!(authz
            .authorize(&principal(BuiltInRole::Reader), Action::PatchFlag, &res)
            .is_err());

        assert!(authz
            .authorize(&principal(BuiltInRole::Writer), Action::CreateFlag, &res)
            .is_ok());
        assert!(authz
            .authorize(&principal(BuiltInRole::Writer), Action::PatchFlag, &res)
            .is_ok());
        assert!(authz
            .authorize(&principal(BuiltInRole::Writer), Action::DeleteFlag, &res)
            .is_err());

        assert!(authz
            .authorize(&principal(BuiltInRole::Admin), Action::DeleteFlag, &res)
            .is_ok());
        assert!(authz
            .authorize(&principal(BuiltInRole::Admin), Action::ManageMembers, &res)
            .is_ok());
        assert!(authz
            .authorize(
                &principal(BuiltInRole::Admin),
                Action::ManageOwnerMembers,
                &res
            )
            .is_err());

        assert!(authz
            .authorize(
                &principal(BuiltInRole::Owner),
                Action::ManageOwnerMembers,
                &res
            )
            .is_ok());
    }

    #[test]
    fn custom_rbac_policy_can_deny_writer_in_prod_and_allow_reader_in_dev() {
        let authz = Authorizer;
        let prod_flag = ResourceRef::flag_in_env("default", "production", "checkout-v2");
        let dev_flag = ResourceRef::flag_in_env("default", "development", "checkout-v2");

        let mut writer = principal(BuiltInRole::Writer);
        writer.custom_policies.push(PolicyStatement {
            effect: PolicyEffect::Deny,
            resources: vec!["proj/*:env/production:flag/*".to_string()],
            not_resources: vec![],
            actions: vec!["patchFlag".to_string()],
            not_actions: vec![],
        });
        assert!(authz
            .authorize(&writer, Action::PatchFlag, &prod_flag)
            .is_err());
        assert!(authz
            .authorize(&writer, Action::PatchFlag, &dev_flag)
            .is_ok());

        let mut reader = principal(BuiltInRole::Reader);
        reader.custom_policies.push(PolicyStatement {
            effect: PolicyEffect::Allow,
            resources: vec!["proj/default:env/development:flag/*".to_string()],
            not_resources: vec![],
            actions: vec!["patchFlag".to_string()],
            not_actions: vec![],
        });
        assert!(authz
            .authorize(&reader, Action::PatchFlag, &dev_flag)
            .is_ok());
        assert!(authz
            .authorize(&reader, Action::PatchFlag, &prod_flag)
            .is_err());
    }
}
