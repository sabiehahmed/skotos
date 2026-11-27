use std::collections::BTreeMap;
use std::convert::Infallible;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, on, MethodFilter};
use axum::{Json, Router};
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use bytes::Bytes;
use futures::stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use skotos_domain::{Context, EnvironmentKeyKind, EvaluationReason};
use skotos_eval::evaluate;
use tokio::sync::broadcast::error::RecvError;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::auth::AuthRegistry;
use crate::cache::EnvironmentCache;

#[derive(Clone)]
pub struct EdgeState {
    pub auth: AuthRegistry,
    pub cache: EnvironmentCache,
    pub keepalive_interval: Duration,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalxQuery {
    #[serde(default)]
    pub rev: Option<i64>,
    #[serde(default)]
    pub with_reasons: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientEvaluatedFlag {
    pub value: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variation_index: Option<usize>,
    pub version: i64,
    pub flag_version: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<EvaluationReason>,
}

pub fn build_router(state: EdgeState) -> Router {
    let report_method = Method::from_bytes(b"REPORT").expect("valid REPORT HTTP method");

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::OPTIONS,
            report_method.clone(),
        ])
        .allow_headers(Any)
        .expose_headers([header::ETAG, header::CACHE_CONTROL]);

    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        // Server-Side SDK Streaming (SSE) & Polling (ETag + pre-compressed gzip/zstd)
        .route("/stream/v1/all", get(stream_server_all))
        .route("/sdk/v1/latest-all", get(poll_server_latest_all))
        // Client-Side & Mobile Ping-to-Re-evaluate SSE + Stateless evalx
        .route("/stream/v1/eval/{clientSideId}", get(stream_client_ping))
        .route(
            "/sdk/v1/evalx/{clientSideId}/contexts/{encodedContext}",
            get(evalx_get),
        )
        .route(
            "/sdk/v1/evalx/{clientSideId}/context",
            on(MethodFilter::POST, evalx_report_or_post).fallback(evalx_report_fallback),
        )
        .route(
            "/sdk/v1/evalx/mobile/context",
            on(MethodFilter::POST, evalx_mobile_context).fallback(evalx_mobile_fallback),
        )
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn healthz() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "service": "delivery-edge",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn readyz() -> impl IntoResponse {
    Json(json!({
        "status": "ready",
        "service": "delivery-edge",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// `GET /stream/v1/all` — High-density Server-Side SDK SSE stream.
async fn stream_server_all(State(state): State<EdgeState>, headers: HeaderMap) -> Response {
    let Some(auth_hdr) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return unauthorized("missing Authorization header with SDK key");
    };

    let Some(resolved) = state
        .auth
        .verify_secret_key(auth_hdr, EnvironmentKeyKind::Sdk)
        .await
    else {
        return unauthorized("invalid or expired SDK key");
    };

    let channel = match state.cache.get_or_load(resolved.env_id).await {
        Ok(c) => c,
        Err(err) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": err.to_string() })),
            )
                .into_response();
        }
    };

    // Subscribe BEFORE reading initial put snapshot so no delta between put and stream is missed.
    let rx = channel.server_tx.subscribe();
    let initial_put = channel
        .data
        .read()
        .await
        .precompressed
        .sse_put_bytes
        .clone();
    let keepalive = state.keepalive_interval;

    sse_byte_response(initial_put, rx, keepalive)
}

/// `GET /sdk/v1/latest-all` — Server-Side SDK polling with strong ETag & pre-compressed bytes.
async fn poll_server_latest_all(State(state): State<EdgeState>, headers: HeaderMap) -> Response {
    let Some(auth_hdr) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return unauthorized("missing Authorization header with SDK key");
    };

    let Some(resolved) = state
        .auth
        .verify_secret_key(auth_hdr, EnvironmentKeyKind::Sdk)
        .await
    else {
        return unauthorized("invalid or expired SDK key");
    };

    let channel = match state.cache.get_or_load(resolved.env_id).await {
        Ok(c) => c,
        Err(err) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": err.to_string() })),
            )
                .into_response();
        }
    };

    let cached = channel.data.read().await.clone();
    let etag = &cached.precompressed.etag;

    if let Some(inm) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    {
        if inm.trim() == etag {
            return Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .header(header::ETAG, etag)
                .header(header::CACHE_CONTROL, "public, max-age=1, must-revalidate")
                .body(Body::empty())
                .unwrap();
        }
    }

    let accept_encoding = headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ETAG, etag)
        .header(header::VARY, "Accept-Encoding")
        .header(header::CACHE_CONTROL, "public, max-age=1, must-revalidate");

    let body_bytes = if accept_encoding.contains("zstd") {
        builder = builder.header(header::CONTENT_ENCODING, "zstd");
        cached.precompressed.zstd_bytes.clone()
    } else if accept_encoding.contains("gzip") {
        builder = builder.header(header::CONTENT_ENCODING, "gzip");
        cached.precompressed.gzip_bytes.clone()
    } else {
        cached.precompressed.json_bytes.clone()
    };

    builder.body(Body::from(body_bytes)).unwrap()
}

/// `GET /stream/v1/eval/{clientSideId}` — Client-Side Ping-to-Re-evaluate SSE (zero per-user RAM).
async fn stream_client_ping(
    State(state): State<EdgeState>,
    Path(client_side_id): Path<String>,
) -> Response {
    let Some(resolved) = state.auth.resolve_client_side_id(&client_side_id).await else {
        return unauthorized("unknown clientSideId");
    };

    let channel = match state.cache.get_or_load(resolved.env_id).await {
        Ok(c) => c,
        Err(err) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": err.to_string() })),
            )
                .into_response();
        }
    };

    let rx = channel.client_tx.subscribe();
    let rev = channel.data.read().await.snapshot.revision;
    let initial_ping = Bytes::from(format!("event: ping\ndata: {{\"rev\":{rev}}}\n\n"));

    sse_byte_response(initial_ping, rx, state.keepalive_interval)
}

/// `GET /sdk/v1/evalx/{clientSideId}/contexts/{base64urlContext}` — CDN-cacheable stateless client eval.
async fn evalx_get(
    State(state): State<EdgeState>,
    Path((client_side_id, encoded_context)): Path<(String, String)>,
    Query(query): Query<EvalxQuery>,
    headers: HeaderMap,
) -> Response {
    let Some(resolved) = state.auth.resolve_client_side_id(&client_side_id).await else {
        return unauthorized("unknown clientSideId");
    };

    let decoded = URL_SAFE_NO_PAD
        .decode(encoded_context.as_bytes())
        .or_else(|_| URL_SAFE.decode(encoded_context.as_bytes()));
    let Ok(ctx_bytes) = decoded else {
        return bad_request("invalid base64url context encoding");
    };
    let Ok(context) = serde_json::from_slice::<Context>(&ctx_bytes) else {
        return bad_request("invalid Skotos Context JSON");
    };

    evaluate_client_context(
        &state,
        resolved.env_id,
        &context,
        query.with_reasons,
        false,
        Some(&headers),
    )
    .await
}

/// `POST /sdk/v1/evalx/{clientSideId}/context` — Body-based client evaluation.
async fn evalx_report_or_post(
    State(state): State<EdgeState>,
    Path(client_side_id): Path<String>,
    Query(query): Query<EvalxQuery>,
    Json(context): Json<Context>,
) -> Response {
    let Some(resolved) = state.auth.resolve_client_side_id(&client_side_id).await else {
        return unauthorized("unknown clientSideId");
    };

    evaluate_client_context(
        &state,
        resolved.env_id,
        &context,
        query.with_reasons,
        false,
        None,
    )
    .await
}

/// Handles `REPORT /sdk/v1/evalx/{clientSideId}/context` (Axum custom HTTP method support).
async fn evalx_report_fallback(
    State(state): State<EdgeState>,
    Path(client_side_id): Path<String>,
    Query(query): Query<EvalxQuery>,
    method: Method,
    body: Bytes,
) -> Response {
    if method.as_str() != "REPORT" {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let Some(resolved) = state.auth.resolve_client_side_id(&client_side_id).await else {
        return unauthorized("unknown clientSideId");
    };
    let Ok(context) = serde_json::from_slice::<Context>(&body) else {
        return bad_request("invalid Skotos Context JSON");
    };
    evaluate_client_context(
        &state,
        resolved.env_id,
        &context,
        query.with_reasons,
        false,
        None,
    )
    .await
}

/// `POST /sdk/v1/evalx/mobile/context` — Mobile Key authenticated evaluation (`usingMobileKey = true`).
async fn evalx_mobile_context(
    State(state): State<EdgeState>,
    Query(query): Query<EvalxQuery>,
    headers: HeaderMap,
    Json(context): Json<Context>,
) -> Response {
    let Some(auth_hdr) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return unauthorized("missing Authorization header with Mobile key");
    };
    let Some(resolved) = state
        .auth
        .verify_secret_key(auth_hdr, EnvironmentKeyKind::Mobile)
        .await
    else {
        return unauthorized("invalid or expired Mobile key");
    };
    evaluate_client_context(
        &state,
        resolved.env_id,
        &context,
        query.with_reasons,
        true,
        None,
    )
    .await
}

async fn evalx_mobile_fallback(
    State(state): State<EdgeState>,
    Query(query): Query<EvalxQuery>,
    headers: HeaderMap,
    method: Method,
    body: Bytes,
) -> Response {
    if method.as_str() != "REPORT" {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let Some(auth_hdr) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return unauthorized("missing Authorization header with Mobile key");
    };
    let Some(resolved) = state
        .auth
        .verify_secret_key(auth_hdr, EnvironmentKeyKind::Mobile)
        .await
    else {
        return unauthorized("invalid or expired Mobile key");
    };
    let Ok(context) = serde_json::from_slice::<Context>(&body) else {
        return bad_request("invalid Skotos Context JSON");
    };
    evaluate_client_context(
        &state,
        resolved.env_id,
        &context,
        query.with_reasons,
        true,
        None,
    )
    .await
}

async fn evaluate_client_context(
    state: &EdgeState,
    env_id: uuid::Uuid,
    context: &Context,
    with_reasons: bool,
    mobile_mode: bool,
    request_headers: Option<&HeaderMap>,
) -> Response {
    if let Err(err) = context.validate() {
        return bad_request(&err.to_string());
    }

    let channel = match state.cache.get_or_load(env_id).await {
        Ok(c) => c,
        Err(err) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": err.to_string() })),
            )
                .into_response();
        }
    };

    let cached = channel.data.read().await.clone();
    let mut evaluated: BTreeMap<String, ClientEvaluatedFlag> = BTreeMap::new();

    for (flag_key, flag) in &cached.snapshot.flags {
        let allowed = if mobile_mode {
            flag.client_side_availability.using_mobile_key
        } else {
            flag.client_side_availability.using_environment_id
        };
        if !allowed {
            continue;
        }
        let detail = evaluate(&cached.eval_store, flag_key, context, Value::Null);
        evaluated.insert(
            flag_key.clone(),
            ClientEvaluatedFlag {
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

    let body_bytes = match serde_json::to_vec(&evaluated) {
        Ok(b) => b,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": err.to_string() })),
            )
                .into_response();
        }
    };

    let hash_prefix = &hex::encode(Sha256::digest(&body_bytes))[..16];
    let etag = format!("\"rev-{}-{hash_prefix}\"", cached.snapshot.revision);

    if let Some(hdrs) = request_headers {
        if let Some(inm) = hdrs
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
        {
            if inm.trim() == etag {
                return Response::builder()
                    .status(StatusCode::NOT_MODIFIED)
                    .header(header::ETAG, &etag)
                    .header(header::CACHE_CONTROL, "public, max-age=5")
                    .body(Body::empty())
                    .unwrap();
            }
        }
    }

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ETAG, etag)
        .header(
            header::CACHE_CONTROL,
            if request_headers.is_some() {
                "public, max-age=5"
            } else {
                "no-store"
            },
        )
        .body(Body::from(body_bytes))
        .unwrap()
}

fn sse_byte_response(
    initial_frame: Bytes,
    rx: tokio::sync::broadcast::Receiver<Bytes>,
    keepalive_interval: Duration,
) -> Response {
    struct StreamState {
        initial: Option<Bytes>,
        rx: tokio::sync::broadcast::Receiver<Bytes>,
        keepalive: tokio::time::Interval,
    }

    let mut interval = tokio::time::interval(keepalive_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // Skip immediate first tick
    let init_state = StreamState {
        initial: Some(initial_frame),
        rx,
        keepalive: interval,
    };

    let body_stream = stream::unfold(init_state, |mut st| async move {
        if let Some(first) = st.initial.take() {
            // Advance past the instant initial interval tick once
            let _ = st.keepalive.tick().await;
            return Some((Ok::<Bytes, Infallible>(first), st));
        }
        tokio::select! {
            res = st.rx.recv() => {
                match res {
                    Ok(frame) => Some((Ok(frame), st)),
                    // Slow consumer evicted cleanly -> SDK reconnects with fresh `put`
                    Err(RecvError::Lagged(_)) | Err(RecvError::Closed) => None,
                }
            }
            _ = st.keepalive.tick() => {
                Some((Ok(Bytes::from_static(b": keepalive\n\n")), st))
            }
        }
    });

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

fn unauthorized(msg: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": msg, "code": "unauthorized" })),
    )
        .into_response()
}

fn bad_request(msg: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": msg, "code": "bad_request" })),
    )
        .into_response()
}
