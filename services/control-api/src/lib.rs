//! Skotos Control Plane REST API service, PostgreSQL migration runner, and OpenAPI 3.1 exporter.

pub mod config;
pub mod dto;
pub mod extract;
pub mod phase2_routes;
pub mod phase3_routes;
pub mod phase4_routes;
pub mod problem;
pub mod routes;

use std::sync::Arc;

use axum::Router;
use skotos_auth::{
    ApiTokenVerifier, AuthMode, Authorizer, ExternalAuthAdapter, IdentityProvider,
    SelfHostedAuthAdapter,
};
use skotos_storage::{run_migrations, Storage};
use skotos_telemetry::create_telemetry_store;
use sqlx::postgres::PgPoolOptions;

pub use config::AppConfig;
pub use extract::AppState;
pub use routes::{build_router, ApiDoc};

/// Canonical service name.
pub const SERVICE_NAME: &str = "control-api";

/// Usage summary printed by `control-api --help`.
pub const USAGE: &str = "\
Usage: control-api [OPTIONS]

Options:
  --migrate-only    Run embedded PostgreSQL migrations and exit
  -h, --help        Print this help message
";

/// Parsed CLI command for the `control-api` binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    /// Start the Control Plane HTTP server.
    Serve,
    /// Run embedded SQLx migrations and exit without starting HTTP.
    MigrateOnly,
    /// Print CLI usage and exit 0.
    Help,
}

/// Parses command-line arguments (excluding `argv[0]`) for `control-api`.
pub fn parse_cli_args<I, S>(args: I) -> Result<CliCommand, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut command = CliCommand::Serve;

    for arg in args {
        match arg.as_ref() {
            "--help" | "-h" => return Ok(CliCommand::Help),
            "--migrate-only" => command = CliCommand::MigrateOnly,
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(command)
}

/// Returns the four internal workspace crates linked into `control-api`.
pub fn linked_crates() -> [&'static str; 4] {
    [
        skotos_domain::CRATE_NAME,
        skotos_eval::CRATE_NAME,
        skotos_storage::CRATE_NAME,
        skotos_auth::CRATE_NAME,
    ]
}

/// Connects to PostgreSQL, optionally runs embedded migrations, initializes the configured
/// [`IdentityProvider`] (bootstrapping admin in self-hosted mode), and builds the Axum router.
pub async fn init_app(config: &AppConfig) -> Result<(AppState, Router), String> {
    let pool = PgPoolOptions::new()
        .max_connections(config.database_max_connections)
        .connect(&config.database_url)
        .await
        .map_err(|e| format!("failed to connect to PostgreSQL: {e}"))?;

    if config.run_migrations {
        run_migrations(&pool)
            .await
            .map_err(|e| format!("failed to run database migrations: {e}"))?;
    }

    let telemetry = create_telemetry_store(pool.clone()).await;
    let storage = Storage::new(pool);

    let identity_provider: Arc<dyn IdentityProvider> = match config.auth_mode {
        AuthMode::SelfHosted => {
            let adapter =
                SelfHostedAuthAdapter::new(storage.clone(), config.session_secret.clone())
                    .map_err(|e| format!("self-hosted auth init failed: {e}"))?;
            adapter
                .ensure_bootstrap_admin(&config.bootstrap)
                .await
                .map_err(|e| format!("bootstrap admin init failed: {e}"))?;
            Arc::new(adapter)
        }
        AuthMode::External => {
            let oidc_cfg = config
                .external_oidc
                .clone()
                .ok_or_else(|| "missing external OIDC configuration".to_string())?;
            Arc::new(ExternalAuthAdapter::new(storage.clone(), oidc_cfg))
        }
    };

    let s3_cfg = skotos_fanout::S3Config::from_env();
    let snapshots = skotos_fanout::SnapshotStore::from_s3_config(&s3_cfg).ok();
    let nats = if let Ok(nats_url) = std::env::var("NATS_URL") {
        skotos_fanout::NatsFanout::connect(&nats_url).await.ok()
    } else {
        skotos_fanout::NatsFanout::connect("nats://127.0.0.1:4222")
            .await
            .ok()
    };

    let state = AppState {
        api_token_verifier: ApiTokenVerifier::new(storage.clone()),
        storage,
        telemetry,
        snapshots,
        nats,
        identity_provider,
        authorizer: Authorizer,
        cookie_secure: config.cookie_secure,
    };

    let router = build_router(state.clone(), &config.cors_origins);
    Ok((state, router))
}

/// Runs only the embedded PostgreSQL migrations and exits.
pub async fn run_migrations_only(config: &AppConfig) -> Result<(), String> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&config.database_url)
        .await
        .map_err(|e| format!("failed to connect to PostgreSQL: {e}"))?;
    run_migrations(&pool)
        .await
        .map_err(|e| format!("failed to run database migrations: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{linked_crates, parse_cli_args, CliCommand, SERVICE_NAME};

    #[test]
    fn links_all_workspace_crates() {
        assert_eq!(SERVICE_NAME, "control-api");
        assert_eq!(
            linked_crates(),
            [
                "skotos-domain",
                "skotos-eval",
                "skotos-storage",
                "skotos-auth"
            ]
        );
    }

    #[test]
    fn parses_default_serve_command() {
        assert_eq!(parse_cli_args(Vec::<&str>::new()), Ok(CliCommand::Serve));
    }

    #[test]
    fn parses_migrate_only_flag() {
        assert_eq!(
            parse_cli_args(["--migrate-only"]),
            Ok(CliCommand::MigrateOnly)
        );
    }

    #[test]
    fn parses_help_flags() {
        assert_eq!(parse_cli_args(["--help"]), Ok(CliCommand::Help));
        assert_eq!(parse_cli_args(["-h"]), Ok(CliCommand::Help));
    }

    #[test]
    fn rejects_unknown_argument() {
        let err = parse_cli_args(["--invalid"]).expect_err("expected unknown argument error");
        assert!(err.contains("--invalid"));
    }
}
