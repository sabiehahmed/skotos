use std::sync::Arc;

use async_trait::async_trait;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use serde_json::Value;
use skotos_domain::{BuiltInRole, ClientSideAvailability, PlanTier};
use skotos_storage::{AuditActor, Storage};
use tokio::sync::RwLock;

use crate::error::AuthError;
use crate::password::{hash_password, sign_session_cookie, verify_password, verify_session_cookie};
use crate::principal::{ActorType, AuthMode, AuthenticatedPrincipal};

/// Public OIDC/auth mode configuration surfaced by `GET /api/v1/auth/config`.
#[derive(Debug, Clone)]
pub struct PublicAuthConfig {
    pub mode: AuthMode,
    pub oidc_issuer: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_audience: Option<String>,
}

/// Pluggable authentication provider implemented by both `SelfHostedAuthAdapter`
/// and `ExternalAuthAdapter`.
#[async_trait]
pub trait IdentityProvider: Send + Sync {
    fn mode(&self) -> AuthMode;

    fn public_config(&self) -> PublicAuthConfig;

    /// Authenticates an incoming HTTP request from an optional `Authorization: Bearer` token
    /// or `skotos_session` cookie.
    async fn authenticate(
        &self,
        bearer: Option<&str>,
        session_cookie: Option<&str>,
    ) -> Result<AuthenticatedPrincipal, AuthError>;

    /// Authenticates with email + password in self-hosted mode and returns
    /// `(AuthenticatedPrincipal, signed_session_cookie_value)`.
    async fn login_with_password(
        &self,
        email: &str,
        password: &str,
    ) -> Result<(AuthenticatedPrincipal, String), AuthError>;
}

/// Verifies personal/service API Access Tokens (`sk_api_...`) via SHA-256 hash lookup.
#[derive(Debug, Clone)]
pub struct ApiTokenVerifier {
    storage: Storage,
}

impl ApiTokenVerifier {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    pub async fn verify(&self, bearer: &str) -> Result<Option<AuthenticatedPrincipal>, AuthError> {
        if !bearer.starts_with("sk_api_") {
            return Ok(None);
        }
        let Some(token) = self.storage.verify_api_token(bearer).await? else {
            return Err(AuthError::InvalidToken);
        };
        Ok(Some(AuthenticatedPrincipal {
            org_id: token.org_id,
            actor_type: ActorType::ApiToken,
            member_id: None,
            api_token_id: Some(token.id),
            email: None,
            display_name: format!("API Token: {}", token.name),
            role: token.role,
            team_ids: vec![],
            role_ids: vec![],
            token_scopes: vec![token.role.as_str().to_string()],
            custom_policies: vec![],
        }))
    }
}

/// Configuration for the self-hosted bootstrap owner and starter workspace.
#[derive(Debug, Clone)]
pub struct BootstrapAdminConfig {
    pub org_slug: String,
    pub org_name: String,
    pub admin_email: String,
    pub admin_password: String,
}

/// First-party self-hosted authentication adapter (Argon2id + HMAC-SHA256 HttpOnly cookie).
#[derive(Debug, Clone)]
pub struct SelfHostedAuthAdapter {
    storage: Storage,
    session_secret: Vec<u8>,
    session_ttl_hours: i64,
}

impl SelfHostedAuthAdapter {
    pub fn new(storage: Storage, session_secret: String) -> Result<Self, AuthError> {
        if session_secret.len() < 32 {
            return Err(AuthError::Configuration(
                "SKOTOS_SESSION_SECRET must be at least 32 bytes".to_string(),
            ));
        }
        Ok(Self {
            storage,
            session_secret: session_secret.into_bytes(),
            session_ttl_hours: 24 * 7,
        })
    }

    /// Idempotently creates the bootstrap organization, owner member, and starter `default` project.
    pub async fn ensure_bootstrap_admin(
        &self,
        cfg: &BootstrapAdminConfig,
    ) -> Result<(), AuthError> {
        let org = self
            .storage
            .ensure_organization(&cfg.org_slug, &cfg.org_name, None, PlanTier::Enterprise)
            .await?;

        if self
            .storage
            .find_member_by_email_global(&cfg.admin_email)
            .await?
            .is_none()
        {
            let pwd_hash = hash_password(&cfg.admin_password)?;
            self.storage
                .create_member(
                    org.id,
                    &cfg.admin_email,
                    "Skotos Admin",
                    BuiltInRole::Owner,
                    Some(&pwd_hash),
                    None,
                    &AuditActor::system(),
                )
                .await?;
        }

        // Seed a default starter project with development + production environments if empty
        if self.storage.list_projects(org.id).await?.is_empty() {
            self.storage
                .create_project(
                    org.id,
                    "default",
                    "Default Project",
                    ClientSideAvailability::default(),
                    true,
                    &AuditActor::system(),
                )
                .await?;
        }

        Ok(())
    }
}

#[async_trait]
impl IdentityProvider for SelfHostedAuthAdapter {
    fn mode(&self) -> AuthMode {
        AuthMode::SelfHosted
    }

    fn public_config(&self) -> PublicAuthConfig {
        PublicAuthConfig {
            mode: AuthMode::SelfHosted,
            oidc_issuer: None,
            oidc_client_id: None,
            oidc_audience: None,
        }
    }

    async fn authenticate(
        &self,
        bearer: Option<&str>,
        session_cookie: Option<&str>,
    ) -> Result<AuthenticatedPrincipal, AuthError> {
        let raw_token = session_cookie
            .or(bearer)
            .ok_or(AuthError::Unauthenticated)?;
        let payload = verify_session_cookie(raw_token, &self.session_secret)?;
        let member = self
            .storage
            .get_member(payload.org_id, payload.member_id)
            .await
            .map_err(|_| AuthError::InvalidToken)?;
        let rbac = self
            .storage
            .resolve_member_rbac(member.org_id, member.id)
            .await
            .unwrap_or(skotos_storage::ResolvedMemberRbac {
                team_ids: vec![],
                role_ids: vec![],
                custom_policies: vec![],
            });
        Ok(AuthenticatedPrincipal {
            org_id: member.org_id,
            actor_type: ActorType::Member,
            member_id: Some(member.id),
            api_token_id: None,
            email: Some(member.email),
            display_name: member.display_name,
            role: member.role,
            team_ids: rbac.team_ids,
            role_ids: rbac.role_ids,
            token_scopes: vec![],
            custom_policies: rbac.custom_policies,
        })
    }

    async fn login_with_password(
        &self,
        email: &str,
        password: &str,
    ) -> Result<(AuthenticatedPrincipal, String), AuthError> {
        let Some(found) = self.storage.find_member_by_email_global(email).await? else {
            return Err(AuthError::InvalidCredentials);
        };
        let Some(ref hash) = found.password_hash else {
            return Err(AuthError::InvalidCredentials);
        };
        if !verify_password(password, hash) {
            return Err(AuthError::InvalidCredentials);
        }

        let cookie = sign_session_cookie(
            found.member.org_id,
            found.member.id,
            self.session_ttl_hours,
            &self.session_secret,
        )?;
        let rbac = self
            .storage
            .resolve_member_rbac(found.member.org_id, found.member.id)
            .await
            .unwrap_or(skotos_storage::ResolvedMemberRbac {
                team_ids: vec![],
                role_ids: vec![],
                custom_policies: vec![],
            });
        let principal = AuthenticatedPrincipal {
            org_id: found.member.org_id,
            actor_type: ActorType::Member,
            member_id: Some(found.member.id),
            api_token_id: None,
            email: Some(found.member.email),
            display_name: found.member.display_name,
            role: found.member.role,
            team_ids: rbac.team_ids,
            role_ids: rbac.role_ids,
            token_scopes: vec![],
            custom_policies: rbac.custom_policies,
        };
        Ok((principal, cookie))
    }
}

/// Configuration for the Cloud SaaS external OIDC/JWT provider (WorkOS AuthKit / Auth0 / Clerk).
#[derive(Debug, Clone)]
pub struct ExternalAuthConfig {
    pub issuer: String,
    pub jwks_url: String,
    pub audience: String,
    pub client_id: Option<String>,
    pub org_claim: String,
}

/// Cloud SaaS OIDC/JWT JWKS authentication adapter supporting RS256, ES256, and EdDSA.
#[derive(Clone)]
pub struct ExternalAuthAdapter {
    storage: Storage,
    config: ExternalAuthConfig,
    http: reqwest::Client,
    jwks_cache: Arc<RwLock<Option<JwkSet>>>,
}

#[derive(Debug, Deserialize)]
struct JwtClaims {
    sub: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}

impl ExternalAuthAdapter {
    pub fn new(storage: Storage, config: ExternalAuthConfig) -> Self {
        Self {
            storage,
            config,
            http: reqwest::Client::new(),
            jwks_cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Pre-populates the JWKS cache (useful for deterministic unit & integration testing).
    pub async fn set_static_jwks(&self, jwks: JwkSet) {
        let mut guard = self.jwks_cache.write().await;
        *guard = Some(jwks);
    }

    async fn resolve_jwk(&self, kid: Option<&str>) -> Result<jsonwebtoken::jwk::Jwk, AuthError> {
        if let Some(jwk) = self.find_cached_jwk(kid).await {
            return Ok(jwk);
        }
        // Fetch fresh JWKS from configured endpoint
        let fetched: JwkSet = self
            .http
            .get(&self.config.jwks_url)
            .send()
            .await
            .map_err(|e| AuthError::ExternalProvider(format!("JWKS fetch failed: {e}")))?
            .error_for_status()
            .map_err(|e| AuthError::ExternalProvider(format!("JWKS HTTP error: {e}")))?
            .json()
            .await
            .map_err(|e| AuthError::ExternalProvider(format!("invalid JWKS JSON: {e}")))?;
        {
            let mut guard = self.jwks_cache.write().await;
            *guard = Some(fetched);
        }
        self.find_cached_jwk(kid)
            .await
            .ok_or(AuthError::InvalidToken)
    }

    async fn find_cached_jwk(&self, kid: Option<&str>) -> Option<jsonwebtoken::jwk::Jwk> {
        let guard = self.jwks_cache.read().await;
        let set = guard.as_ref()?;
        match kid {
            Some(k) => set.find(k).cloned(),
            None => set.keys.first().cloned(),
        }
    }
}

#[async_trait]
impl IdentityProvider for ExternalAuthAdapter {
    fn mode(&self) -> AuthMode {
        AuthMode::External
    }

    fn public_config(&self) -> PublicAuthConfig {
        PublicAuthConfig {
            mode: AuthMode::External,
            oidc_issuer: Some(self.config.issuer.clone()),
            oidc_client_id: self.config.client_id.clone(),
            oidc_audience: Some(self.config.audience.clone()),
        }
    }

    async fn authenticate(
        &self,
        bearer: Option<&str>,
        _session_cookie: Option<&str>,
    ) -> Result<AuthenticatedPrincipal, AuthError> {
        let token = bearer.ok_or(AuthError::Unauthenticated)?;
        let header = decode_header(token).map_err(|_| AuthError::InvalidToken)?;
        if !matches!(
            header.alg,
            Algorithm::RS256 | Algorithm::ES256 | Algorithm::EdDSA
        ) {
            return Err(AuthError::InvalidToken);
        }

        let jwk = self.resolve_jwk(header.kid.as_deref()).await?;
        let decoding_key = DecodingKey::from_jwk(&jwk).map_err(|_| AuthError::InvalidToken)?;

        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[&self.config.issuer]);
        validation.set_audience(&[&self.config.audience]);

        let token_data = decode::<JwtClaims>(token, &decoding_key, &validation)
            .map_err(|_| AuthError::InvalidToken)?;
        let claims = token_data.claims;

        let external_org_id = claims
            .extra
            .get(&self.config.org_claim)
            .and_then(Value::as_str)
            .ok_or(AuthError::InvalidToken)?;

        // Map external org -> internal organization (or auto-provision on first login)
        let org = match self
            .storage
            .get_organization_by_external_id(external_org_id)
            .await?
        {
            Some(existing) => existing,
            None => {
                let safe_slug: String = external_org_id
                    .chars()
                    .map(|c| {
                        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                            c
                        } else {
                            '-'
                        }
                    })
                    .collect();
                self.storage
                    .ensure_organization(
                        &safe_slug,
                        external_org_id,
                        Some(external_org_id),
                        PlanTier::Free,
                    )
                    .await?
            }
        };

        let member = match self
            .storage
            .find_member_by_external_subject(org.id, &claims.sub)
            .await?
        {
            Some(existing) => existing,
            None => {
                let email = claims
                    .email
                    .unwrap_or_else(|| format!("{}@external.skotos.local", claims.sub));
                let display_name = claims.name.unwrap_or_else(|| email.clone());
                let existing_members = self.storage.list_members(org.id).await?;
                let initial_role = if existing_members.is_empty() {
                    BuiltInRole::Owner
                } else {
                    BuiltInRole::Reader
                };
                self.storage
                    .create_member(
                        org.id,
                        &email,
                        &display_name,
                        initial_role,
                        None,
                        Some(&claims.sub),
                        &AuditActor::system(),
                    )
                    .await?
            }
        };

        let rbac = self
            .storage
            .resolve_member_rbac(member.org_id, member.id)
            .await
            .unwrap_or(skotos_storage::ResolvedMemberRbac {
                team_ids: vec![],
                role_ids: vec![],
                custom_policies: vec![],
            });

        Ok(AuthenticatedPrincipal {
            org_id: member.org_id,
            actor_type: ActorType::Member,
            member_id: Some(member.id),
            api_token_id: None,
            email: Some(member.email),
            display_name: member.display_name,
            role: member.role,
            team_ids: rbac.team_ids,
            role_ids: rbac.role_ids,
            token_scopes: vec![],
            custom_policies: rbac.custom_policies,
        })
    }

    async fn login_with_password(
        &self,
        _email: &str,
        _password: &str,
    ) -> Result<(AuthenticatedPrincipal, String), AuthError> {
        Err(AuthError::UnsupportedInMode(
            "password login is disabled in external OIDC auth mode".to_string(),
        ))
    }
}
