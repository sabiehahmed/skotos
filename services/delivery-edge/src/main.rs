use std::net::SocketAddr;
use std::time::Duration;

use delivery_edge::{build_router, AuthRegistry, EdgeState, EnvironmentCache};
use skotos_fanout::{NatsFanout, S3Config, SnapshotStore};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::dotenv();
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,delivery_edge=debug,skotos_fanout=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let bind_addr: SocketAddr = std::env::var("SKOTOS_DELIVERY_EDGE_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8081".to_string())
        .parse()?;
    let nats_url =
        std::env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_string());
    let s3_cfg = S3Config::from_env();

    let snapshots = SnapshotStore::from_s3_config(&s3_cfg)?;
    let nats = match NatsFanout::connect(&nats_url).await {
        Ok(client) => {
            info!(%nats_url, "delivery-edge connected to NATS JetStream");
            Some(client)
        }
        Err(err) => {
            warn!(%nats_url, error = %err, "delivery-edge starting without live NATS (object-store snapshot mode)");
            None
        }
    };

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let auth = AuthRegistry::new(snapshots.clone());
    let seeded = auth.refresh_from_object_store().await;
    info!(
        environments = seeded,
        "loaded initial DB-free auth registry"
    );

    if let Some(n) = nats.clone() {
        auth.spawn_nats_sync(n, shutdown_rx.clone()).await;
    }

    let cache = EnvironmentCache::new(snapshots, nats, shutdown_rx.clone());
    let keepalive_secs: u64 = std::env::var("SKOTOS_SSE_KEEPALIVE_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);

    let app = build_router(EdgeState {
        auth,
        cache,
        keepalive_interval: Duration::from_secs(keepalive_secs.max(5)),
    });

    let listener = TcpListener::bind(bind_addr).await?;
    info!(%bind_addr, "Skotos Flag Delivery Edge listening (DB-free hot path)");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            let _ = shutdown_tx.send(true);
        })
        .await?;

    Ok(())
}
