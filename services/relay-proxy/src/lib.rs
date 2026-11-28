//! Open-source Skotos Relay Proxy (Phase 3).
//!
//! Zero-DB, single-binary Rust proxy for customer VPCs and air-gapped clusters:
//! - **Proxy Mode**: Maintains a single upstream SSE connection per configured environment
//!   to `services/delivery-edge` and re-broadcasts `/stream/v1/all`, `/sdk/v1/latest-all`,
//!   `/stream/v1/eval/{cid}`, and stateless `/sdk/v1/evalx/{cid}/...` locally, forwarding
//!   `/events/v1/bulk` upstream to `services/event-ingest`.
//! - **Offline / File-Snapshot Mode**: Serves air-gapped environments directly from a
//!   local JSON snapshot file without any outbound internet connectivity.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, on, post, MethodFilter};
use axum::{Json, Router};
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use bytes::Bytes;
use chrono::Utc;
use dashmap::DashMap;
use futures::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use skotos_domain::{Context, EvalFlag, EvalSegment, EvaluationReason};
use skotos_eval::{evaluate, InMemoryEvalStore};
use skotos_fanout::{precompress_snapshot, EnvironmentSnapshot, PrecompressedSnapshot};
use tokio::sync::{broadcast, watch, RwLock};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{debug, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayEnvConfig {
    pub name: String,
    pub sdk_key: String,
    #[serde(default)]
    pub client_side_id: Option<String>,
    #[serde(default)]
    pub mobile_key: Option<String>,
}

#[derive(Debug, Clone)]
pub enum RelayMode {
    Proxy {
        upstream_stream_url: String,
        upstream_events_url: Option<String>,
    },
    OfflineFile {
        snapshot_path: String,
    },
}

#[derive(Clone)]
struct RelayEnvData {
    snapshot: EnvironmentSnapshot,
    precompressed: PrecompressedSnapshot,
    eval_store: InMemoryEvalStore,
    connected: bool,
}

struct RelayEnvSlot {
    cfg: RelayEnvConfig,
    data: RwLock<Arc<RelayEnvData>>,
    server_tx: broadcast::Sender<Bytes>,
    client_tx: broadcast::Sender<Bytes>,
}

#[derive(Clone)]
pub struct RelayState {
    mode_label: String,
    upstream_events_url: Option<String>,
    by_sdk_key: Arc<DashMap<String, Arc<RelayEnvSlot>>>,
    by_client_id: Arc<DashMap<String, Arc<RelayEnvSlot>>>,
    by_mobile_key: Arc<DashMap<String, Arc<RelayEnvSlot>>>,
    http: reqwest::Client,
    keepalive_interval: Duration,
}

impl RelayState {
    pub fn new(
        envs: Vec<RelayEnvConfig>,
        mode: &RelayMode,
        keepalive_interval: Duration,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let by_sdk_key = Arc::new(DashMap::new());
        let by_client_id = Arc::new(DashMap::new());
        let by_mobile_key = Arc::new(DashMap::new());

        let (mode_label, upstream_events_url, offline_snap) = match mode {
            RelayMode::Proxy {
                upstream_events_url,
                ..
            } => ("proxy".to_string(), upstream_events_url.clone(), None),
            RelayMode::OfflineFile { snapshot_path } => {
                let raw = std::fs::read(Path::new(snapshot_path))?;
                let snap: EnvironmentSnapshot = serde_json::from_slice(&raw)?;
                ("offline_file".to_string(), None, Some(snap))
            }
        };

        for cfg in envs {
            let snap = offline_snap.clone().unwrap_or_else(|| EnvironmentSnapshot {
                environment_id: Uuid::nil(),
                revision: 0,
                generated_at: Utc::now(),
                flags: BTreeMap::new(),
                segments: BTreeMap::new(),
            });
            let data = build_env_data(snap, offline_snap.is_some())?;
            let (server_tx, _) = broadcast::channel(256);
            let (client_tx, _) = broadcast::channel(256);
            let slot = Arc::new(RelayEnvSlot {
                cfg: cfg.clone(),
                data: RwLock::new(Arc::new(data)),
                server_tx,
                client_tx,
            });
            by_sdk_key.insert(cfg.sdk_key.clone(), slot.clone());
            if let Some(cid) = &cfg.client_side_id {
                by_client_id.insert(cid.clone(), slot.clone());
            }
            if let Some(mk) = &cfg.mobile_key {
                by_mobile_key.insert(mk.clone(), slot.clone());
            }
        }

        Ok(Self {
            mode_label,
            upstream_events_url,
            by_sdk_key,
            by_client_id,
            by_mobile_key,
            http: reqwest::Client::new(),
            keepalive_interval,
        })
    }

    /// Spawns background upstream SSE stream listeners for every configured environment in Proxy Mode.
    pub fn spawn_upstream_sync(
        &self,
        upstream_stream_url: String,
        shutdown: watch::Receiver<bool>,
    ) {
        let base = upstream_stream_url.trim_end_matches('/').to_string();
        for entry in self.by_sdk_key.iter() {
            let slot = entry.value().clone();
            let http = self.http.clone();
            let url = format!("{base}/stream/v1/all");
            let mut rx_shutdown = shutdown.clone();
            tokio::spawn(async move {
                let mut backoff = Duration::from_millis(200);
                loop {
                    if *rx_shutdown.borrow() {
                        break;
                    }
                    let req = http
                        .get(&url)
                        .header(header::AUTHORIZATION, &slot.cfg.sdk_key)
                        .header(header::ACCEPT, "text/event-stream");
                    match req.send().await {
                        Ok(resp) if resp.status().is_success() => {
                            backoff = Duration::from_millis(200);
                            info!(env = %slot.cfg.name, "relay-proxy connected to upstream SSE stream");
                            let mut bytes_stream = resp.bytes_stream();
                            let mut buf = String::new();
                            loop {
                                tokio::select! {
                                    chunk = bytes_stream.next() => {
                                        match chunk {
                                            Some(Ok(bytes)) => {
                                                buf.push_str(&String::from_utf8_lossy(&bytes));
                                                while let Some(idx) = buf.find("\n\n") {
                                                    let block = buf[..idx].to_string();
                                                    buf.drain(..idx + 2);
                                                    apply_upstream_sse_block(&slot, &block).await;
                                                }
                                            }
                                            Some(Err(err)) => {
                                                warn!(env = %slot.cfg.name, error = %err, "relay-proxy upstream SSE error");
                                                break;
                                            }
                                            None => break,
                                        }
                                    }
                                    _ = rx_shutdown.changed() => {
                                        if *rx_shutdown.borrow() {
                                            return;
                                        }
                                    }
                                }
                            }
                        }
                        Ok(resp) => {
                            warn!(env = %slot.cfg.name, status = %resp.status(), "relay-proxy upstream rejected stream");
                        }
                        Err(err) => {
                            warn!(env = %slot.cfg.name, error = %err, "relay-proxy upstream connect failed");
                        }
                    }

                    tokio::select! {
                        _ = tokio::time::sleep(backoff) => {}
                        _ = rx_shutdown.changed() => {
                            if *rx_shutdown.borrow() {
                                break;
                            }
                        }
                    }
                    backoff = (backoff * 2).min(Duration::from_secs(10));
                }
            });
        }
    }
}

fn build_env_data(
    snapshot: EnvironmentSnapshot,
    connected: bool,
) -> Result<RelayEnvData, Box<dyn std::error::Error + Send + Sync>> {
    let precompressed = precompress_snapshot(&snapshot)?;
    let mut eval_store = InMemoryEvalStore::new();
    for f in snapshot.flags.values() {
        eval_store.insert_flag(f.clone());
    }
    for s in snapshot.segments.values() {
        eval_store.insert_segment(s.clone());
    }
    Ok(RelayEnvData {
        snapshot,
        precompressed,
        eval_store,
        connected,
    })
}

async fn apply_upstream_sse_block(slot: &RelayEnvSlot, block: &str) {
    let mut event_name = "message";
    let mut data_lines = Vec::new();
    for line in block.lines() {
        if let Some(ev) = line.strip_prefix("event:") {
            event_name = ev.trim();
        } else if let Some(d) = line.strip_prefix("data:") {
            data_lines.push(d.trim());
        }
    }
    if data_lines.is_empty() {
        return;
    }
    let data_str = data_lines.join("\n");
    let Ok(val) = serde_json::from_str::<Value>(&data_str) else {
        return;
    };

    let mut snap = slot.data.read().await.snapshot.clone();
    if let Some(rev) = val.get("revision").and_then(Value::as_i64) {
        snap.revision = snap.revision.max(rev);
    }
    snap.generated_at = Utc::now();

    let server_frame = match event_name {
        "put" => {
            if let Some(data_obj) = val.get("data") {
                if let Some(flags_val) = data_obj.get("flags") {
                    if let Ok(flags) =
                        serde_json::from_value::<BTreeMap<String, EvalFlag>>(flags_val.clone())
                    {
                        snap.flags = flags;
                    }
                }
                if let Some(segs_val) = data_obj.get("segments") {
                    if let Ok(segs) =
                        serde_json::from_value::<BTreeMap<String, EvalSegment>>(segs_val.clone())
                    {
                        snap.segments = segs;
                    }
                }
            }
            let Ok(next) = build_env_data(snap, true) else {
                return;
            };
            let frame = next.precompressed.sse_put_bytes.clone();
            *slot.data.write().await = Arc::new(next);
            frame
        }
        "patch" => {
            if let Some(path) = val.get("path").and_then(Value::as_str) {
                if let Some(flag_key) = path.strip_prefix("/flags/") {
                    if let Some(d) = val.get("data") {
                        if let Ok(flag) = serde_json::from_value::<EvalFlag>(d.clone()) {
                            snap.flags.insert(flag_key.to_string(), flag);
                        }
                    }
                } else if let Some(seg_key) = path.strip_prefix("/segments/") {
                    if let Some(d) = val.get("data") {
                        if let Ok(seg) = serde_json::from_value::<EvalSegment>(d.clone()) {
                            snap.segments.insert(seg_key.to_string(), seg);
                        }
                    }
                }
            }
            let Ok(next) = build_env_data(snap, true) else {
                return;
            };
            *slot.data.write().await = Arc::new(next);
            Bytes::from(format!("event: patch\ndata: {data_str}\n\n"))
        }
        "delete" => {
            if let Some(path) = val.get("path").and_then(Value::as_str) {
                if let Some(flag_key) = path.strip_prefix("/flags/") {
                    snap.flags.remove(flag_key);
                } else if let Some(seg_key) = path.strip_prefix("/segments/") {
                    snap.segments.remove(seg_key);
                }
            }
            let Ok(next) = build_env_data(snap, true) else {
                return;
            };
            *slot.data.write().await = Arc::new(next);
            Bytes::from(format!("event: delete\ndata: {data_str}\n\n"))
        }
        _ => return,
    };

    debug!(env = %slot.cfg.name, event = event_name, "relay-proxy updated local cache & fanning out");
    let _ = slot.server_tx.send(server_frame);
    let rev = slot.data.read().await.snapshot.revision;
    let _ = slot.client_tx.send(Bytes::from(format!(
        "event: ping\ndata: {{\"rev\":{rev}}}\n\n"
    )));
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EvalxQuery {
    #[serde(default)]
    with_reasons: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClientEvalFlag {
    value: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    variation_index: Option<usize>,
    version: i64,
    flag_version: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<EvaluationReason>,
}

pub fn build_router(state: RelayState) -> Router {
    let report_method = Method::from_bytes(b"REPORT").expect("valid REPORT method");
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS, report_method])
        .allow_headers(Any)
        .expose_headers([header::ETAG, header::CACHE_CONTROL]);

    Router::new()
        .route("/healthz", get(status))
        .route("/status", get(status))
        .route("/stream/v1/all", get(stream_server_all))
        .route("/sdk/v1/latest-all", get(poll_latest_all))
        .route("/stream/v1/eval/{clientSideId}", get(stream_client_ping))
        .route(
            "/sdk/v1/evalx/{clientSideId}/contexts/{encodedContext}",
            get(evalx_get),
        )
        .route(
            "/sdk/v1/evalx/{clientSideId}/context",
            on(MethodFilter::POST, evalx_post),
        )
        .route("/events/v1/bulk", post(forward_events_bulk))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn status(State(state): State<RelayState>) -> impl IntoResponse {
    let mut envs = serde_json::Map::new();
    let mut all_connected = true;
    for entry in state.by_sdk_key.iter() {
        let slot = entry.value();
        let data = slot.data.read().await;
        if !data.connected {
            all_connected = false;
        }
        envs.insert(
            slot.cfg.name.clone(),
            json!({
                "status": if data.connected { "connected" } else { "initializing" },
                "revision": data.snapshot.revision,
                "flagsCount": data.snapshot.flags.len(),
                "segmentsCount": data.snapshot.segments.len(),
            }),
        );
    }
    Json(json!({
        "status": if all_connected { "healthy" } else { "degraded" },
        "service": "skotos-relay-proxy",
        "mode": state.mode_label,
        "version": env!("CARGO_PKG_VERSION"),
        "environments": envs,
    }))
}

async fn stream_server_all(State(state): State<RelayState>, headers: HeaderMap) -> Response {
    let Some(key) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return (StatusCode::UNAUTHORIZED, "missing SDK key").into_response();
    };
    let Some(slot) = state.by_sdk_key.get(key) else {
        return (StatusCode::UNAUTHORIZED, "unknown SDK key").into_response();
    };
    let rx = slot.server_tx.subscribe();
    let initial = slot.data.read().await.precompressed.sse_put_bytes.clone();
    sse_response(initial, rx, state.keepalive_interval)
}

async fn poll_latest_all(State(state): State<RelayState>, headers: HeaderMap) -> Response {
    let Some(key) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return (StatusCode::UNAUTHORIZED, "missing SDK key").into_response();
    };
    let Some(slot) = state.by_sdk_key.get(key) else {
        return (StatusCode::UNAUTHORIZED, "unknown SDK key").into_response();
    };
    let data = slot.data.read().await.clone();
    let etag = &data.precompressed.etag;
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        == Some(etag.as_str())
    {
        return Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(header::ETAG, etag)
            .body(Body::empty())
            .unwrap();
    }

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ETAG, etag)
        .body(Body::from(data.precompressed.json_bytes.clone()))
        .unwrap()
}

async fn stream_client_ping(
    State(state): State<RelayState>,
    AxumPath(cid): AxumPath<String>,
) -> Response {
    let slot = state
        .by_client_id
        .get(&cid)
        .or_else(|| state.by_mobile_key.get(&cid));
    let Some(slot) = slot else {
        return (StatusCode::UNAUTHORIZED, "unknown clientSideId").into_response();
    };
    let rx = slot.client_tx.subscribe();
    let rev = slot.data.read().await.snapshot.revision;
    let init = Bytes::from(format!("event: ping\ndata: {{\"rev\":{rev}}}\n\n"));
    sse_response(init, rx, state.keepalive_interval)
}

async fn evalx_get(
    State(state): State<RelayState>,
    AxumPath((cid, encoded)): AxumPath<(String, String)>,
    Query(query): Query<EvalxQuery>,
) -> Response {
    let raw = URL_SAFE_NO_PAD
        .decode(&encoded)
        .or_else(|_| URL_SAFE.decode(&encoded));
    let Ok(bytes) = raw else {
        return (StatusCode::BAD_REQUEST, "invalid base64url context").into_response();
    };
    let Ok(ctx) = serde_json::from_slice::<Context>(&bytes) else {
        return (StatusCode::BAD_REQUEST, "invalid context json").into_response();
    };
    evaluate_for_client(&state, &cid, &ctx, query.with_reasons).await
}

async fn evalx_post(
    State(state): State<RelayState>,
    AxumPath(cid): AxumPath<String>,
    Query(query): Query<EvalxQuery>,
    Json(ctx): Json<Context>,
) -> Response {
    evaluate_for_client(&state, &cid, &ctx, query.with_reasons).await
}

async fn evaluate_for_client(
    state: &RelayState,
    cid: &str,
    ctx: &Context,
    with_reasons: bool,
) -> Response {
    let (slot, is_mobile) = if let Some(s) = state.by_client_id.get(cid) {
        (s, false)
    } else if let Some(s) = state.by_mobile_key.get(cid) {
        (s, true)
    } else {
        return (StatusCode::UNAUTHORIZED, "unknown clientSideId").into_response();
    };
    let data = slot.data.read().await.clone();
    let mut results: BTreeMap<String, ClientEvalFlag> = BTreeMap::new();
    for (flag_key, flag) in &data.snapshot.flags {
        let allowed = if is_mobile {
            flag.client_side_availability.using_mobile_key
        } else {
            flag.client_side_availability.using_environment_id
        };
        if !allowed {
            continue;
        }
        let detail = evaluate(&data.eval_store, flag_key, ctx, Value::Null);
        results.insert(
            flag_key.clone(),
            ClientEvalFlag {
                value: detail.value,
                variation_index: detail.variation_index,
                version: flag.version,
                flag_version: flag.version,
                reason: if with_reasons {
                    Some(detail.reason)
                } else {
                    None
                },
            },
        );
    }
    Json(results).into_response()
}

async fn forward_events_bulk(
    State(state): State<RelayState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(upstream) = &state.upstream_events_url else {
        return (
            StatusCode::ACCEPTED,
            Json(json!({ "accepted": true, "mode": "offline" })),
        )
            .into_response();
    };
    let url = format!("{}/events/v1/bulk", upstream.trim_end_matches('/'));
    let mut req = state.http.post(&url).body(body);
    if let Some(auth) = headers.get(header::AUTHORIZATION) {
        req = req.header(header::AUTHORIZATION, auth);
    }
    if let Some(enc) = headers.get(header::CONTENT_ENCODING) {
        req = req.header(header::CONTENT_ENCODING, enc);
    }
    if let Some(pid) = headers.get("x-skotos-payload-id") {
        req = req.header("x-skotos-payload-id", pid);
    }
    match req.send().await {
        Ok(resp) => (
            StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::ACCEPTED),
            Json(json!({ "accepted": true, "forwarded": true })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": format!("upstream event forward failed: {err}") })),
        )
            .into_response(),
    }
}

fn sse_response(
    initial: Bytes,
    rx: broadcast::Receiver<Bytes>,
    keepalive_interval: Duration,
) -> Response {
    let keepalive = tokio::time::interval(keepalive_interval);
    let body_stream = stream::unfold(
        (Some(initial), rx, keepalive),
        |(mut init, mut rx, mut ka)| async move {
            if let Some(first) = init.take() {
                let _ = ka.tick().await;
                return Some((Ok::<Bytes, Infallible>(first), (init, rx, ka)));
            }
            tokio::select! {
                res = rx.recv() => match res {
                    Ok(frame) => Some((Ok(frame), (init, rx, ka))),
                    Err(_) => None,
                },
                _ = ka.tick() => Some((Ok(Bytes::from_static(b": keepalive\n\n")), (init, rx, ka))),
            }
        },
    );

    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream; charset=utf-8"),
        )
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache, no-transform"),
        )
        .header("x-accel-buffering", HeaderValue::from_static("no"))
        .body(Body::from_stream(body_stream))
        .unwrap()
}
