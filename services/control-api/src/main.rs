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

    let (state, router) = match init_app(&config).await {
        Ok(app) => app,
        Err(err) => {
            tracing::error!(error = %err, "startup failed");
            return ExitCode::from(1);
        }
    };

    let (worker_shutdown_tx, worker_shutdown_rx) = tokio::sync::watch::channel(false);
    let enable_workers = std::env::var("SKOTOS_ENABLE_WORKERS")
        .map(|v| v != "false" && v != "0")
        .unwrap_or(true);
    if enable_workers {
        let storage_for_workers = state.storage.clone();
        let rx_outbox = worker_shutdown_rx.clone();
        let rx_sched = worker_shutdown_rx.clone();

        let scheduler = skotos_fanout::SchedulerWorker::new(
            storage_for_workers.clone(),
            std::time::Duration::from_secs(1),
        )
        .with_telemetry(state.telemetry.clone());
        tokio::spawn(scheduler.run_until_shutdown(rx_sched));

        tokio::spawn(async move {
            let s3_cfg = skotos_fanout::S3Config::from_env();
            if let Err(err) = skotos_fanout::SnapshotStore::ensure_s3_bucket_exists(&s3_cfg).await {
                tracing::warn!(error = %err, "could not verify/create S3 bucket; falling back to in-memory snapshots");
            }
            let snapshots = skotos_fanout::SnapshotStore::from_s3_config(&s3_cfg)
                .unwrap_or_else(|_| skotos_fanout::SnapshotStore::in_memory());
            let nats_url =
                std::env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_string());
            let nats = match skotos_fanout::NatsFanout::connect(&nats_url).await {
                Ok(n) => Some(n),
                Err(err) => {
                    tracing::warn!(%nats_url, error = %err, "NATS unavailable; outbox worker running in snapshot + webhook mode");
                    None
                }
            };
            let outbox = skotos_fanout::OutboxWorker::new(
                storage_for_workers,
                snapshots,
                nats,
                skotos_fanout::OutboxWorkerConfig::default(),
            );
            if let Err(err) = outbox.sync_all_environments().await {
                tracing::warn!(error = %err, "initial environment snapshot warmup failed");
            }
            outbox.run_until_shutdown(rx_outbox).await;
        });
    }

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
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            let _ = worker_shutdown_tx.send(true);
        })
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
