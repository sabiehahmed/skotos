use serde::{Deserialize, Serialize};
use skotos_domain::BuiltInRole;
use skotos_storage::AuditActor;
use uuid::Uuid;

use crate::error::AuthzError;

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

/// Resource descriptor for authorization checks (extensible for Phase 3 custom policies).
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

    pub fn flag(project_key: impl Into<String>, flag_key: impl Into<String>) -> Self {
        Self {
            resource_type: "flag".to_string(),
            project_key: Some(project_key.into()),
            env_key: None,
            resource_key: Some(flag_key.into()),
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
    ManageProject,
    ManageEnvironment,
    RotateEnvironmentKey,
    ManageApiTokens,
    ManageMembers,
    ManageOwnerMembers,
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
            Self::ManageProject => "manageProject",
            Self::ManageEnvironment => "manageEnvironment",
            Self::RotateEnvironmentKey => "rotateEnvironmentKey",
            Self::ManageApiTokens => "manageApiTokens",
            Self::ManageMembers => "manageMembers",
            Self::ManageOwnerMembers => "manageOwnerMembers",
        }
    }
}

/// Stateless built-in RBAC authorizer enforcing the locked matrix in `PROJECT.md`:
/// - `Reader`: Read + SimulateFlag
/// - `Writer`: + CreateFlag, PatchFlag, RestoreFlagRevision
/// - `Admin`: + DeleteFlag, ManageProject, ManageEnvironment, RotateEnvironmentKey, ManageApiTokens, ManageMembers
/// - `Owner`: + ManageOwnerMembers
#[derive(Debug, Default, Clone, Copy)]
pub struct Authorizer;

impl Authorizer {
    pub fn authorize(
        &self,
        principal: &AuthenticatedPrincipal,
        action: Action,
        _resource: &ResourceRef,
    ) -> Result<(), AuthzError> {
        let min_role = match action {
            Action::Read | Action::SimulateFlag => BuiltInRole::Reader,
            Action::CreateFlag | Action::PatchFlag | Action::RestoreFlagRevision => {
                BuiltInRole::Writer
            }
            Action::DeleteFlag
            | Action::ManageProject
            | Action::ManageEnvironment
            | Action::RotateEnvironmentKey
            | Action::ManageApiTokens
            | Action::ManageMembers => BuiltInRole::Admin,
            Action::ManageOwnerMembers => BuiltInRole::Owner,
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
}
