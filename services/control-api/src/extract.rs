use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{header, HeaderMap};
use skotos_auth::{
    ApiTokenVerifier, AuthenticatedPrincipal, Authorizer, IdentityProvider, SESSION_COOKIE_NAME,
};
use skotos_storage::Storage;
use skotos_telemetry::TelemetryStore;

use crate::problem::ApiError;

/// Shared Axum router state.
#[derive(Clone)]
pub struct AppState {
    pub storage: Storage,
    pub telemetry: Arc<dyn TelemetryStore>,
    pub identity_provider: Arc<dyn IdentityProvider>,
    pub api_token_verifier: ApiTokenVerifier,
    pub authorizer: Authorizer,
    pub cookie_secure: bool,
}

/// Axum extractor that resolves the authenticated caller:
/// 1. Checks `Authorization: Bearer sk_api_...` via [`ApiTokenVerifier`] first.
/// 2. Otherwise delegates `skotos_session` cookie or Bearer JWT to [`IdentityProvider`].
#[derive(Debug, Clone)]
pub struct AuthPrincipal(pub AuthenticatedPrincipal);

impl FromRequestParts<AppState> for AuthPrincipal {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let bearer = extract_bearer_token(&parts.headers);
        let cookie = extract_cookie(&parts.headers, SESSION_COOKIE_NAME);

        if let Some(b) = bearer {
            if let Some(principal) = state.api_token_verifier.verify(b).await? {
                return Ok(AuthPrincipal(principal));
            }
        }

        let principal = state
            .identity_provider
            .authenticate(bearer, cookie.as_deref())
            .await?;
        Ok(AuthPrincipal(principal))
    }
}

pub fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let trimmed = raw.trim();
    trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

pub fn extract_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    for val in headers.get_all(header::COOKIE) {
        let Ok(raw) = val.to_str() else { continue };
        for pair in raw.split(';') {
            if let Some((k, v)) = pair.trim().split_once('=') {
                if k.trim() == name {
                    let trimmed = v.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Parses an integer version from an `If-Match` header (supports `14`, `"14"`, `W/"14"`).
pub fn parse_if_match_version(headers: &HeaderMap) -> Result<Option<i64>, ApiError> {
    let Some(raw) = headers.get(header::IF_MATCH) else {
        return Ok(None);
    };
    let s = raw
        .to_str()
        .map_err(|_| ApiError::bad_request("invalid_if_match", "invalid If-Match header"))?
        .trim();
    if s.is_empty() || s == "*" {
        return Ok(None);
    }
    let cleaned = s.strip_prefix("W/").unwrap_or(s).trim_matches('"').trim();
    let parsed = cleaned.parse::<i64>().map_err(|_| {
        ApiError::bad_request(
            "invalid_if_match",
            format!("If-Match header '{s}' must be an integer flag version"),
        )
    })?;
    Ok(Some(parsed))
}

/// Builds a `Set-Cookie` header for `skotos_session`.
pub fn build_session_set_cookie(token: &str, secure: bool, max_age_secs: i64) -> String {
    let secure_attr = if secure { "; Secure" } else { "" };
    format!(
        "{SESSION_COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}{secure_attr}"
    )
}
