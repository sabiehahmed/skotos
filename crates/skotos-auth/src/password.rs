use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{Duration, Utc};
use hmac::{Hmac, Mac};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::error::AuthError;

type HmacSha256 = Hmac<Sha256>;

/// Session cookie name used in self-hosted mode.
pub const SESSION_COOKIE_NAME: &str = "skotos_session";

/// Signed session payload stored inside `skotos_session` HttpOnly cookie.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPayload {
    pub org_id: Uuid,
    pub member_id: Uuid,
    pub exp: i64,
}

/// Hashes a plaintext password using Argon2id with a random OS salt.
pub fn hash_password(password: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AuthError::Configuration(format!("failed to hash password: {e}")))
}

/// Verifies a plaintext password against a stored Argon2id PHC hash string.
pub fn verify_password(password: &str, phc_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(phc_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// Signs a session payload into `<base64url(json)>.<base64url(hmac-sha256)>`.
pub fn sign_session_cookie(
    org_id: Uuid,
    member_id: Uuid,
    ttl_hours: i64,
    secret: &[u8],
) -> Result<String, AuthError> {
    if secret.len() < 32 {
        return Err(AuthError::Configuration(
            "SKOTOS_SESSION_SECRET must be at least 32 bytes".to_string(),
        ));
    }
    let payload = SessionPayload {
        org_id,
        member_id,
        exp: (Utc::now() + Duration::hours(ttl_hours)).timestamp(),
    };
    let json_bytes = serde_json::to_vec(&payload)
        .map_err(|e| AuthError::Configuration(format!("failed to serialize session: {e}")))?;
    let encoded_payload = URL_SAFE_NO_PAD.encode(&json_bytes);

    let mut mac = HmacSha256::new_from_slice(secret)
        .map_err(|e| AuthError::Configuration(format!("invalid HMAC secret: {e}")))?;
    mac.update(encoded_payload.as_bytes());
    let signature = mac.finalize().into_bytes();
    let encoded_sig = URL_SAFE_NO_PAD.encode(signature);

    Ok(format!("{encoded_payload}.{encoded_sig}"))
}

/// Verifies a signed `skotos_session` cookie value using constant-time MAC comparison.
pub fn verify_session_cookie(
    cookie_value: &str,
    secret: &[u8],
) -> Result<SessionPayload, AuthError> {
    let Some((encoded_payload, encoded_sig)) = cookie_value.split_once('.') else {
        return Err(AuthError::InvalidToken);
    };

    let provided_sig = URL_SAFE_NO_PAD
        .decode(encoded_sig)
        .map_err(|_| AuthError::InvalidToken)?;

    let mut mac = HmacSha256::new_from_slice(secret)
        .map_err(|e| AuthError::Configuration(format!("invalid HMAC secret: {e}")))?;
    mac.update(encoded_payload.as_bytes());
    let expected_sig = mac.finalize().into_bytes();

    if provided_sig.len() != expected_sig.len()
        || !bool::from(provided_sig.as_slice().ct_eq(expected_sig.as_slice()))
    {
        return Err(AuthError::InvalidToken);
    }

    let payload_bytes = URL_SAFE_NO_PAD
        .decode(encoded_payload)
        .map_err(|_| AuthError::InvalidToken)?;
    let payload: SessionPayload =
        serde_json::from_slice(&payload_bytes).map_err(|_| AuthError::InvalidToken)?;

    if payload.exp <= Utc::now().timestamp() {
        return Err(AuthError::InvalidToken);
    }

    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_verifies_argon2_password() {
        let hash = hash_password("SkotosDevPass123!").unwrap();
        assert!(verify_password("SkotosDevPass123!", &hash));
        assert!(!verify_password("wrong-pass", &hash));
    }

    #[test]
    fn signs_and_verifies_session_cookie_and_rejects_tampering() {
        let secret = b"skotos-test-session-secret-at-least-32-bytes-long";
        let org_id = Uuid::new_v4();
        let member_id = Uuid::new_v4();

        let token = sign_session_cookie(org_id, member_id, 24, secret).unwrap();
        let parsed = verify_session_cookie(&token, secret).unwrap();
        assert_eq!(parsed.org_id, org_id);
        assert_eq!(parsed.member_id, member_id);

        let tampered = format!("{token}x");
        assert!(verify_session_cookie(&tampered, secret).is_err());
    }
}
