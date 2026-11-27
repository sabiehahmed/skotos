use skotos_storage::StorageError;
use thiserror::Error;

/// Authentication errors (mapped to HTTP 401 Unauthorized or 400 Bad Request).
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("authentication required")]
    Unauthenticated,

    #[error("invalid credentials")]
    InvalidCredentials,

    #[error("invalid or expired session token")]
    InvalidToken,

    #[error("unsupported auth operation in current auth mode: {0}")]
    UnsupportedInMode(String),

    #[error("auth configuration error: {0}")]
    Configuration(String),

    #[error("external identity provider error: {0}")]
    ExternalProvider(String),

    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
}

/// Authorization / RBAC errors (mapped to HTTP 403 Forbidden).
#[derive(Debug, Error)]
pub enum AuthzError {
    #[error("forbidden: role '{role}' cannot perform '{action}'")]
    Forbidden { role: String, action: String },
}
