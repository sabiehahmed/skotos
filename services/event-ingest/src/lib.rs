//! Skotos Event Ingestion Service (`services/event-ingest`, ADR-0005).
//!
//! Accepts gzipped 5-second SDK summary batches (`POST /events/v1/bulk` and
//! `POST /events/v1/client/{clientSideId}`), enforces `X-Skotos-Payload-ID`
//! idempotency, immediately returns `202 Accepted`, and asynchronously flushes
//! aggregated hourly rollups into [`skotos_telemetry::TelemetryStore`].

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use bytes::Bytes;
use chrono::Utc;
use dashmap::DashMap;
use flate2::read::GzDecoder;
use serde_json::json;
use skotos_domain::EnvironmentKeyKind;
use skotos_fanout::SnapshotStore;
use skotos_storage::{sha256_hex, EnvironmentAuthRecord, Storage};
use skotos_telemetry::{
    extract_rollup_increments, SdkEvent, SummaryRollupIncrement, TelemetryStore,
};
use tokio::sync::{mpsc, watch};
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;
use tracing::{debug, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ResolvedEnvAuth {
    pub org_id: Uuid,
    pub env_id: Uuid,
}

type CachedKeyEntry = (
    ResolvedEnvAuth,
    EnvironmentKeyKind,
    Option<chrono::DateTime<Utc>>,
);

#[derive(Clone)]
pub struct IngestAuthCache {
    by_key_hash: Arc<DashMap<String, CachedKeyEntry>>,
    by_client_id: Arc<DashMap<String, ResolvedEnvAuth>>,
    snapshots: Option<SnapshotStore>,
    storage: Option<Storage>,
}

impl IngestAuthCache {
    pub fn new(snapshots: Option<SnapshotStore>, storage: Option<Storage>) -> Self {
        Self {
            by_key_hash: Arc::new(DashMap::new()),
            by_client_id: Arc::new(DashMap::new()),
            snapshots,
            storage,
        }
    }

    pub fn upsert(&self, rec: &EnvironmentAuthRecord) {
        self.by_client_id.insert(
            rec.client_side_id.clone(),
            ResolvedEnvAuth {
                org_id: rec.org_id,
                env_id: rec.env_id,
            },
        );
        for key in &rec.keys {
            self.by_key_hash.insert(
                key.key_hash.clone(),
                (
                    ResolvedEnvAuth {
                        org_id: rec.org_id,
                        env_id: rec.env_id,
                    },
                    key.kind,
                    key.expires_at,
                ),
            );
        }
    }

    pub async fn refresh(&self) {
        if let Some(snaps) = &self.snapshots {
            if let Ok(Some(idx)) = snaps.get_auth_index().await {
                for rec in idx.environments {
                    self.upsert(&rec);
                }
                return;
            }
        }
        if let Some(storage) = &self.storage {
            if let Ok(records) = storage.list_all_environment_auth_records().await {
                for rec in records {
                    self.upsert(&rec);
                }
            }
        }
    }

    pub async fn verify_sdk_key(&self, raw_header: &str) -> Option<ResolvedEnvAuth> {
        let token = raw_header
            .trim()
            .strip_prefix("Bearer ")
            .or_else(|| raw_header.trim().strip_prefix("bearer "))
            .unwrap_or(raw_header.trim());
        if token.is_empty() {
            return None;
        }
        let hash = sha256_hex(token);
        if let Some(found) = self.lookup_key(&hash) {
            return Some(found);
        }
        self.refresh().await;
        self.lookup_key(&hash)
    }

    fn lookup_key(&self, hash: &str) -> Option<ResolvedEnvAuth> {
        let entry = self.by_key_hash.get(hash)?;
        if let Some(exp) = entry.2 {
            if exp <= Utc::now() {
                return None;
            }
        }
        Some(entry.0.clone())
    }

    pub async fn resolve_client_id(&self, client_side_id: &str) -> Option<ResolvedEnvAuth> {
        if let Some(found) = self.by_client_id.get(client_side_id) {
            return Some(found.clone());
        }
        self.refresh().await;
        self.by_client_id.get(client_side_id).map(|e| e.clone())
    }
}

#[derive(Clone)]
pub struct IngestState {
    pub auth: IngestAuthCache,
    pub telemetry: Arc<dyn TelemetryStore>,
    pub tx: mpsc::Sender<Vec<SummaryRollupIncrement>>,
    pub nats: Option<skotos_fanout::NatsFanout>,
}

/// Spawns the background batched rollup flusher and returns the bounded channel sender.
pub fn spawn_rollup_flusher(
    telemetry: Arc<dyn TelemetryStore>,
    flush_interval: Duration,
    mut shutdown: watch::Receiver<bool>,
) -> mpsc::Sender<Vec<SummaryRollupIncrement>> {
    let (tx, mut rx) = mpsc::channel::<Vec<SummaryRollupIncrement>>(1024);

    tokio::spawn(async move {
        let mut buffer: Vec<SummaryRollupIncrement> = Vec::new();
        let mut ticker = tokio::time::interval(flush_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                maybe_batch = rx.recv() => {
                    match maybe_batch {
                        Some(mut batch) => {
                            buffer.append(&mut batch);
                            if buffer.len() >= 200 {
                                flush_buffer(&telemetry, &mut buffer).await;
                            }
                        }
                        None => {
                            flush_buffer(&telemetry, &mut buffer).await;
                            break;
                        }
                    }
                }
                _ = ticker.tick() => {
                    if !buffer.is_empty() {
                        flush_buffer(&telemetry, &mut buffer).await;
                    }
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        flush_buffer(&telemetry, &mut buffer).await;
                        break;
                    }
                }
            }
        }
    });

    tx
}

async fn flush_buffer(
    telemetry: &Arc<dyn TelemetryStore>,
    buffer: &mut Vec<SummaryRollupIncrement>,
) {
    if buffer.is_empty() {
        return;
    }
    let batch = std::mem::take(buffer);
    let len = batch.len();
    if let Err(err) = telemetry.record_rollups(&batch).await {
        warn!(count = len, error = %err, "failed to flush telemetry rollup batch");
    } else {
        debug!(count = len, "flushed telemetry rollup batch");
    }
}

pub fn build_router(state: IngestState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/events/v1/bulk", post(ingest_bulk))
        .route("/events/v1/client/{clientSideId}", post(ingest_client))
        .layer(RequestBodyLimitLayer::new(4 * 1024 * 1024))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn healthz() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "service": "event-ingest",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn readyz() -> impl IntoResponse {
    Json(json!({
        "status": "ready",
        "service": "event-ingest",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn ingest_bulk(
    State(state): State<IngestState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(auth_hdr) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "missing Authorization header" })),
        )
            .into_response();
    };
    let Some(resolved) = state.auth.verify_sdk_key(auth_hdr).await else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid or expired SDK key" })),
        )
            .into_response();
    };

    process_event_payload(&state, resolved, &headers, body).await
}

async fn ingest_client(
    State(state): State<IngestState>,
    Path(client_side_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(resolved) = state.auth.resolve_client_id(&client_side_id).await else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unknown clientSideId" })),
        )
            .into_response();
    };

    process_event_payload(&state, resolved, &headers, body).await
}

async fn process_event_payload(
    state: &IngestState,
    resolved: ResolvedEnvAuth,
    headers: &HeaderMap,
    body: Bytes,
) -> Response {
    // 1. Decompress transparent gzip batches if Content-Encoding: gzip
    let is_gzip = headers
        .get(header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_ascii_lowercase().contains("gzip"))
        .unwrap_or(false);

    let json_bytes = if is_gzip {
        let mut decoder = GzDecoder::new(body.as_ref());
        let mut out = Vec::new();
        // Bound decompressed payload at 8 MiB to prevent zip-bomb OOM
        let mut limited = decoder.by_ref().take(8 * 1024 * 1024 + 1);
        if limited.read_to_end(&mut out).is_err() || out.len() > 8 * 1024 * 1024 {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid or oversized gzip payload" })),
            )
                .into_response();
        }
        out
    } else {
        body.to_vec()
    };

    // 2. Parse SDK event batch array
    let events: Vec<SdkEvent> = match serde_json::from_slice(&json_bytes) {
        Ok(e) => e,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("malformed event batch: {err}") })),
            )
                .into_response();
        }
    };

    // 3. Enforce X-Skotos-Payload-ID idempotency
    if let Some(payload_id) = headers
        .get("x-skotos-payload-id")
        .and_then(|v| v.to_str().ok())
    {
        match state
            .telemetry
            .claim_payload_id(resolved.env_id, payload_id)
            .await
        {
            Ok(false) => {
                // Already processed — idempotent 202 Accepted
                return (
                    StatusCode::ACCEPTED,
                    Json(json!({ "accepted": true, "deduplicated": true })),
                )
                    .into_response();
            }
            Ok(true) => {}
            Err(err) => {
                warn!(error = %err, "payload idempotency check failed; proceeding best-effort");
            }
        }
    }

    // 4. Extract hourly summary rollups and enqueue for async flush
    let increments = extract_rollup_increments(resolved.org_id, resolved.env_id, &events);
    let accepted_events = events.len();

    // 5. Phase 3 Live Events Debugger: publish ephemeral debug events onto core NATS (zero storage cost)
    if let Some(nats) = &state.nats {
        for inc in &increments {
            let dbg = skotos_domain::LiveEvalDebugEvent {
                timestamp: inc.evaluated_at,
                env_id: resolved.env_id,
                kind: "summary".to_string(),
                flag_key: Some(inc.flag_key.clone()),
                variation_index: Some(inc.variation_index),
                value: None,
                context_kinds: inc.context_kinds.clone(),
                count: inc.count,
            };
            let _ = nats.publish_debug_event(resolved.env_id, &dbg).await;
        }
    }

    if !increments.is_empty() {
        if let Err(err) = state.tx.try_send(increments) {
            warn!(error = %err, "event-ingest queue full; dropping summary batch to protect memory");
        }
    }

    (
        StatusCode::ACCEPTED,
        Json(json!({ "accepted": true, "events": accepted_events })),
    )
        .into_response()
}
