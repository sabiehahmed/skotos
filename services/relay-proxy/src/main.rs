use std::net::SocketAddr;
use std::time::Duration;

use relay_proxy::{build_router, RelayEnvConfig, RelayMode, RelayState};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _ = dotenvy::dotenv();
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,relay_proxy=debug"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let bind_addr: SocketAddr = std::env::var("SKOTOS_RELAY_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8083".to_string())
        .parse()?;

    let envs: Vec<RelayEnvConfig> = if let Ok(raw_json) = std::env::var("SKOTOS_RELAY_ENVS") {
        serde_json::from_str(&raw_json)?
    } else {
        let sdk_key = std::env::var("SKOTOS_SDK_KEY")
            .expect("SKOTOS_SDK_KEY or SKOTOS_RELAY_ENVS must be set");
        let client_side_id = std::env::var("SKOTOS_CLIENT_SIDE_ID").ok();
        let mobile_key = std::env::var("SKOTOS_MOBILE_KEY").ok();
        vec![RelayEnvConfig {
            name: "default".to_string(),
            sdk_key,
            client_side_id,
            mobile_key,
        }]
    };

    let mode = if let Ok(snapshot_path) = std::env::var("SKOTOS_OFFLINE_SNAPSHOT") {
        RelayMode::OfflineFile { snapshot_path }
    } else {
        RelayMode::Proxy {
            upstream_stream_url: std::env::var("SKOTOS_UPSTREAM_STREAM_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8081".to_string()),
            upstream_events_url: Some(
                std::env::var("SKOTOS_UPSTREAM_EVENTS_URL")
                    .unwrap_or_else(|_| "http://127.0.0.1:8082".to_string()),
            ),
        }
    };

    let state = RelayState::new(envs, &mode, Duration::from_secs(30))?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    if let RelayMode::Proxy {
        upstream_stream_url,
        ..
    } = mode
    {
        state.spawn_upstream_sync(upstream_stream_url, shutdown_rx.clone());
    }

    let app = build_router(state);
    let listener = TcpListener::bind(bind_addr).await?;
    info!(%bind_addr, "Skotos Relay Proxy listening (DB-free static binary)");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            let _ = shutdown_tx.send(true);
        })
        .await?;

    Ok(())
}
