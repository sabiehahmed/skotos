use std::process::ExitCode;

use control_api::{
    init_app, parse_cli_args, run_migrations_only, AppConfig, CliCommand, SERVICE_NAME, USAGE,
};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    let command = match parse_cli_args(std::env::args().skip(1)) {
        Ok(cmd) => cmd,
        Err(err) => {
            eprintln!("{SERVICE_NAME}: {err}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    if command == CliCommand::Help {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let config = match AppConfig::from_env() {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("{SERVICE_NAME}: configuration error: {err}");
            return ExitCode::from(1);
        }
    };

    init_tracing(&config.log_format);

    if command == CliCommand::MigrateOnly {
        return match run_migrations_only(&config).await {
            Ok(()) => {
                tracing::info!("PostgreSQL migrations applied successfully");
                ExitCode::SUCCESS
            }
            Err(err) => {
                tracing::error!(error = %err, "migration failed");
                ExitCode::from(1)
            }
        };
    }

    let (_state, router) = match init_app(&config).await {
        Ok(app) => app,
        Err(err) => {
            tracing::error!(error = %err, "startup failed");
            return ExitCode::from(1);
        }
    };

    let listener = match tokio::net::TcpListener::bind(&config.listen_addr).await {
        Ok(l) => l,
        Err(err) => {
            tracing::error!(addr = %config.listen_addr, error = %err, "failed to bind TCP listener");
            return ExitCode::from(1);
        }
    };

    tracing::info!(
        addr = %config.listen_addr,
        auth_mode = %config.auth_mode.as_str(),
        "Skotos Control Plane API listening"
    );

    if let Err(err) = axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        tracing::error!(error = %err, "HTTP server exited with error");
        return ExitCode::from(1);
    }

    tracing::info!("Skotos Control Plane API shut down cleanly");
    ExitCode::SUCCESS
}

fn init_tracing(format: &str) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,control_api=debug,sqlx=warn"));
    if format == "json" {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .try_init();
    } else {
        let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
