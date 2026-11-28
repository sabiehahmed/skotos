//! Pluggable dual-path authentication (Cloud SaaS OIDC & Self-Hosted), API tokens, and RBAC.

pub mod error;
pub mod password;
pub mod policy;
pub mod principal;
pub mod provider;

pub use error::{AuthError, AuthzError};
pub use password::{
    hash_password, sign_session_cookie, verify_password, verify_session_cookie, SessionPayload,
    SESSION_COOKIE_NAME,
};
pub use policy::{evaluate_policies, glob_match, PolicyDecision};
pub use principal::{Action, ActorType, AuthMode, AuthenticatedPrincipal, Authorizer, ResourceRef};
pub use provider::{
    ApiTokenVerifier, BootstrapAdminConfig, ExternalAuthAdapter, ExternalAuthConfig,
    IdentityProvider, PublicAuthConfig, SelfHostedAuthAdapter,
};

/// Canonical crate identifier for workspace wiring verification.
pub const CRATE_NAME: &str = "skotos-auth";

/// Returns the linked domain crate identifier.
pub fn domain_crate_name() -> &'static str {
    skotos_domain::CRATE_NAME
}

#[cfg(test)]
mod tests {
    use super::{domain_crate_name, CRATE_NAME};

    #[test]
    fn links_domain_crate() {
        assert_eq!(CRATE_NAME, "skotos-auth");
        assert_eq!(domain_crate_name(), "skotos-domain");
    }
}
