use skotos_domain::DomainError;
use thiserror::Error;

/// Storage layer errors mapped cleanly to HTTP RFC 9457 problem details by `control-api`.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("resource not found: {0}")]
    NotFound(String),

    #[error("resource already exists: {0}")]
    AlreadyExists(String),

    #[error(
        "optimistic concurrency conflict: expected version {expected}, current version is {actual}"
    )]
    VersionConflict { expected: i64, actual: i64 },

    #[error("validation error: {0}")]
    Validation(#[from] DomainError),

    #[error("a non-empty change comment is required for this environment")]
    CommentRequired,

    #[error("variation is still referenced by: {}", .0.join(", "))]
    VariationInUse(Vec<String>),

    #[error("prerequisite cycle detected")]
    PrerequisiteCycle,

    #[error("cannot remove or demote the last owner of an organization")]
    ForbiddenLastOwner,

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
}
