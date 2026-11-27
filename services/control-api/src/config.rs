use skotos_auth::{AuthMode, BootstrapAdminConfig, ExternalAuthConfig};

/// Runtime configuration parsed from environment variables matching `.env.example`.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub database_max_connections: u32,
    pub run_migrations: bool,
    pub listen_addr: String,
    pub cors_origins: Vec<String>,
    pub auth_mode: AuthMode,
    pub session_secret: String,
    pub cookie_secure: bool,
    pub bootstrap: BootstrapAdminConfig,
    pub external_oidc: Option<ExternalAuthConfig>,
    pub log_format: String,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, String> {
        let _ = dotenvy::dotenv();

        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://skotos:skotos@localhost:5432/skotos".to_string());
        let database_max_connections = std::env::var("DATABASE_MAX_CONNECTIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(20);
        let run_migrations = std::env::var("SKOTOS_RUN_MIGRATIONS")
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);

        let listen_addr = std::env::var("SKOTOS_CONTROL_API_ADDR").unwrap_or_else(|_| {
            let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
            format!("0.0.0.0:{port}")
        });

        let cors_origins = std::env::var("SKOTOS_CORS_ORIGINS")
            .unwrap_or_else(|_| "http://localhost:5173,http://127.0.0.1:5173".to_string())
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();

        let auth_mode = match std::env::var("SKOTOS_AUTH_MODE")
            .unwrap_or_else(|_| "self_hosted".to_string())
            .as_str()
        {
            "self_hosted" => AuthMode::SelfHosted,
            "external" => AuthMode::External,
            other => {
                return Err(format!(
                    "invalid SKOTOS_AUTH_MODE '{other}': expected 'self_hosted' or 'external'"
                ));
            }
        };

        let session_secret = std::env::var("SKOTOS_SESSION_SECRET").unwrap_or_else(|_| {
            "skotos-dev-session-secret-change-me-at-least-32-bytes".to_string()
        });
        let cookie_secure = std::env::var("SKOTOS_COOKIE_SECURE")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        let bootstrap = BootstrapAdminConfig {
            org_slug: std::env::var("SKOTOS_BOOTSTRAP_ORG_SLUG")
                .unwrap_or_else(|_| "default".to_string()),
            org_name: std::env::var("SKOTOS_BOOTSTRAP_ORG_NAME")
                .unwrap_or_else(|_| "Skotos Local".to_string()),
            admin_email: std::env::var("SKOTOS_BOOTSTRAP_ADMIN_EMAIL")
                .unwrap_or_else(|_| "admin@skotos.local".to_string()),
            admin_password: std::env::var("SKOTOS_BOOTSTRAP_ADMIN_PASSWORD")
                .unwrap_or_else(|_| "SkotosDevPass123!".to_string()),
        };

        let external_oidc = if auth_mode == AuthMode::External {
            let issuer = std::env::var("SKOTOS_OIDC_ISSUER")
                .map_err(|_| "SKOTOS_OIDC_ISSUER is required when SKOTOS_AUTH_MODE=external")?;
            let jwks_url = std::env::var("SKOTOS_OIDC_JWKS_URL")
                .map_err(|_| "SKOTOS_OIDC_JWKS_URL is required when SKOTOS_AUTH_MODE=external")?;
            let audience = std::env::var("SKOTOS_OIDC_AUDIENCE")
                .map_err(|_| "SKOTOS_OIDC_AUDIENCE is required when SKOTOS_AUTH_MODE=external")?;
            let client_id = std::env::var("SKOTOS_OIDC_CLIENT_ID").ok();
            let org_claim =
                std::env::var("SKOTOS_OIDC_ORG_CLAIM").unwrap_or_else(|_| "org_id".to_string());
            Some(ExternalAuthConfig {
                issuer,
                jwks_url,
                audience,
                client_id,
                org_claim,
            })
        } else {
            None
        };

        let log_format =
            std::env::var("SKOTOS_LOG_FORMAT").unwrap_or_else(|_| "pretty".to_string());

        Ok(Self {
            database_url,
            database_max_connections,
            run_migrations,
            listen_addr,
            cors_origins,
            auth_mode,
            session_secret,
            cookie_secure,
            bootstrap,
            external_oidc,
            log_format,
        })
    }
}
