use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use skotos_auth::{AuthError, AuthzError};
use skotos_domain::DomainError;
use skotos_storage::StorageError;
use utoipa::ToSchema;

/// RFC 9457 `application/problem+json` error payload returned by all `control-api` 4xx/5xx responses.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProblemDetails {
    #[serde(rename = "type")]
    pub problem_type: String,
    pub title: String,
    pub status: u16,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_version: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<String>>,
}

/// Application error wrapper that serializes into RFC 9457 `application/problem+json`.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub body: Box<ProblemDetails>,
}

impl ApiError {
    pub fn new(
        status: StatusCode,
        code: impl Into<String>,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        let code_str = code.into();
        Self {
            status,
            body: Box::new(ProblemDetails {
                problem_type: format!("https://skotos.dev/problems/{code_str}"),
                title: title.into(),
                status: status.as_u16(),
                detail: detail.into(),
                instance: None,
                code: code_str,
                current_version: None,
                errors: None,
            }),
        }
    }

    pub fn bad_request(code: &str, detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, "Bad Request", detail)
    }

    pub fn unauthorized(detail: impl Into<String>) -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthenticated",
            "Unauthorized",
            detail,
        )
    }

    pub fn forbidden(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", "Forbidden", detail)
    }

    pub fn not_found(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "Not Found", detail)
    }

    pub fn conflict(code: &str, detail: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, "Conflict", detail)
    }

    pub fn version_conflict(expected: i64, actual: i64) -> Self {
        let mut err = Self::new(
            StatusCode::CONFLICT,
            "version_conflict",
            "Version Conflict",
            format!("expected version {expected}, but current version is {actual}"),
        );
        err.body.current_version = Some(actual);
        err
    }

    pub fn with_errors(mut self, errors: Vec<String>) -> Self {
        self.body.errors = Some(errors);
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let json = serde_json::to_vec(self.body.as_ref()).unwrap_or_else(|_| {
            br#"{"type":"about:blank","title":"Internal Server Error","status":500,"detail":"serialization failure","code":"internal_error"}"#.to_vec()
        });
        (
            self.status,
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/problem+json"),
            )],
            json,
        )
            .into_response()
    }
}

impl From<DomainError> for ApiError {
    fn from(err: DomainError) -> Self {
        match err {
            DomainError::VariationInUse { references, .. } => {
                Self::conflict("variation_in_use", "Variation is still in use")
                    .with_errors(references)
            }
            DomainError::PrerequisiteCycle { path } => Self::bad_request(
                "prerequisite_cycle",
                format!("prerequisite cycle detected: {}", path.join(" -> ")),
            ),
            other => Self::bad_request("validation_error", other.to_string()),
        }
    }
}

impl From<StorageError> for ApiError {
    fn from(err: StorageError) -> Self {
        match err {
            StorageError::NotFound(msg) => Self::not_found(msg),
            StorageError::AlreadyExists(msg) => Self::conflict("already_exists", msg),
            StorageError::VersionConflict { expected, actual } => {
                Self::version_conflict(expected, actual)
            }
            StorageError::Validation(domain_err) => ApiError::from(domain_err),
            StorageError::CommentRequired => Self::bad_request(
                "comment_required",
                "a non-empty change comment is required for this environment",
            ),
            StorageError::VariationInUse(refs) => {
                Self::conflict("variation_in_use", "Variation is still referenced")
                    .with_errors(refs)
            }
            StorageError::PrerequisiteCycle => Self::bad_request(
                "prerequisite_cycle",
                "adding this prerequisite would create a dependency cycle",
            ),
            StorageError::ForbiddenLastOwner => Self::conflict(
                "last_owner_protected",
                "cannot remove or demote the last owner of an organization",
            ),
            StorageError::Database(e) => {
                tracing::error!(error = %e, "database error");
                Self::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_error",
                    "Internal Server Error",
                    "an internal database error occurred",
                )
            }
            StorageError::Migration(e) => {
                tracing::error!(error = %e, "migration error");
                Self::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "migration_error",
                    "Internal Server Error",
                    "database migration error",
                )
            }
        }
    }
}

impl From<AuthError> for ApiError {
    fn from(err: AuthError) -> Self {
        match err {
            AuthError::Unauthenticated => Self::unauthorized("authentication required"),
            AuthError::InvalidCredentials => Self::unauthorized("invalid email or password"),
            AuthError::InvalidToken => {
                Self::unauthorized("invalid or expired authentication token")
            }
            AuthError::UnsupportedInMode(msg) => Self::bad_request("unsupported_auth_mode", msg),
            AuthError::Configuration(msg) => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "auth_config_error",
                "Internal Server Error",
                msg,
            ),
            AuthError::ExternalProvider(msg) => Self::new(
                StatusCode::BAD_GATEWAY,
                "external_auth_error",
                "Bad Gateway",
                msg,
            ),
            AuthError::Storage(e) => ApiError::from(e),
        }
    }
}

impl From<AuthzError> for ApiError {
    fn from(err: AuthzError) -> Self {
        Self::forbidden(err.to_string())
    }
}
