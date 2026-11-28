use std::net::SocketAddr;
use std::time::Duration;

use event_ingest::{build_router, spawn_rollup_flusher, IngestAuthCache, IngestState};
use skotos_fanout::{S3Config, SnapshotStore};
use skotos_storage::{run_migrations, Storage};
use skotos_telemetry::create_telemetry_store;
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::dotenv();
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,event_ingest=debug,skotos_telemetry=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let bind_addr: SocketAddr = std::env::var("SKOTOS_EVENT_INGEST_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8082".to_string())
        .parse()?;
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://skotos:skotos@127.0.0.1:5432/skotos".to_string());

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;
    run_migrations(&pool).await?;

    let storage = Storage::new(pool.clone());
    let telemetry = create_telemetry_store(pool).await;
    let snapshots = match SnapshotStore::from_s3_config(&S3Config::from_env()) {
        Ok(s) => Some(s),
        Err(err) => {
            warn!(error = %err, "event-ingest object store unavailable; using DB auth fallback");
            None
        }
    };

    let auth = IngestAuthCache::new(snapshots, Some(storage));
    auth.refresh().await;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let tx = spawn_rollup_flusher(
        telemetry.clone(),
        Duration::from_millis(500),
        shutdown_rx.clone(),
    );

    let nats_url =
        std::env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_string());
    let nats = match skotos_fanout::NatsFanout::connect(&nats_url).await {
        Ok(n) => Some(n),
        Err(err) => {
            warn!(error = %err, "event-ingest running without NATS live-debugger tap");
            None
        }
    };

    let app = build_router(IngestState {
        auth,
        telemetry,
        tx,
        nats,
    });

    let listener = TcpListener::bind(bind_addr).await?;
    info!(%bind_addr, "Skotos Event Ingestion service listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            let _ = shutdown_tx.send(true);
        })
        .await?;

    Ok(())
}
