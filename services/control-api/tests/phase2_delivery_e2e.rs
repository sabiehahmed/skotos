use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::Router;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use delivery_edge::{build_router as build_edge_router, AuthRegistry, EdgeState, EnvironmentCache};
use event_ingest::{
    build_router as build_ingest_router, spawn_rollup_flusher, IngestAuthCache, IngestState,
};
use flate2::read::GzDecoder;
use futures::StreamExt;
use serde_json::{json, Value};
use skotos_auth::{AuthMode, BootstrapAdminConfig};
use skotos_domain::{Context, SingleContext};
use skotos_fanout::{
    verify_webhook_signature, NatsFanout, OutboxWorker, OutboxWorkerConfig, S3Config,
    SchedulerWorker, SnapshotStore,
};
use skotos_server_sdk::{SkotosClient, SkotosConfig};
use tokio::net::TcpListener;
use tokio::sync::watch;
use uuid::Uuid;

use control_api::{init_app, AppConfig};

/// End-to-end Phase 2 integration test verifying the entire live pipeline across
/// PostgreSQL 16 + MinIO (S3/R2) + NATS JetStream + `control-api` + `OutboxWorker` +
/// `SchedulerWorker` + `delivery-edge` (DB-free) + `event-ingest` + live Rust Server SDK +
/// Client Ping-to-Re-evaluate + Signed Webhooks + Flag Triggers + DB outage survivability.
#[tokio::test]
async fn phase2_full_delivery_pipeline_sdk_telemetry_automation_and_outage_isolation() {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://skotos:skotos@127.0.0.1:5432/skotos".to_string());
    let nats_url =
        std::env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_string());
    let s3_cfg = S3Config::from_env();

    // Ensure MinIO bucket exists and connect to NATS JetStream
    SnapshotStore::ensure_s3_bucket_exists(&s3_cfg)
        .await
        .expect("MinIO bucket ready");
    let snapshots = SnapshotStore::from_s3_config(&s3_cfg).expect("MinIO snapshot store");
    let nats = NatsFanout::connect(&nats_url)
        .await
        .expect("NATS JetStream connected");

    // 1. Start Control Plane API
    let slug = format!("p2-e2e-{}", Uuid::new_v4().simple());
    let admin_email = format!("owner@{slug}.test");
    let config = AppConfig {
        listen_addr: "127.0.0.1:0".to_string(),
        database_url: database_url.clone(),
        database_max_connections: 8,
        run_migrations: true,
        log_format: "pretty".to_string(),
        cors_origins: vec!["http://localhost:5173".to_string()],
        auth_mode: AuthMode::SelfHosted,
        session_secret: "test-session-secret-phase2-at-least-32-bytes".to_string(),
        cookie_secure: false,
        bootstrap: BootstrapAdminConfig {
            org_slug: slug.clone(),
            org_name: "Phase 2 E2E Org".to_string(),
            admin_email: admin_email.clone(),
            admin_password: "Phase2Password123!".to_string(),
        },
        external_oidc: None,
    };

    let (cp_state, cp_router) = init_app(&config).await.expect("control-api init");
    let cp_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cp_addr = cp_listener.local_addr().unwrap();
    let cp_base = format!("http://{cp_addr}");

    let (cp_shutdown_tx, mut cp_shutdown_rx) = watch::channel(false);
    let cp_task = tokio::spawn(async move {
        axum::serve(cp_listener, cp_router)
            .with_graceful_shutdown(async move {
                let _ = cp_shutdown_rx.changed().await;
            })
            .await
            .unwrap();
    });

    // Mark any leftover unpublished outbox rows from prior test runs so we measure live Phase 2 propagation
    sqlx::query("UPDATE outbox_events SET published_at = now() WHERE published_at IS NULL")
        .execute(cp_state.storage.pool())
        .await
        .unwrap();

    // 2. Start Outbox & Scheduler Workers
    let (worker_shutdown_tx, worker_shutdown_rx) = watch::channel(false);
    let outbox_worker = OutboxWorker::new(
        cp_state.storage.clone(),
        snapshots.clone(),
        Some(nats.clone()),
        OutboxWorkerConfig {
            poll_interval: Duration::from_millis(15),
            batch_size: 50,
            lease_seconds: 30,
        },
    );
    outbox_worker
        .sync_all_environments()
        .await
        .expect("warmup sync");
    let outbox_handle = tokio::spawn(
        outbox_worker
            .clone()
            .run_until_shutdown(worker_shutdown_rx.clone()),
    );
    let scheduler = SchedulerWorker::new(cp_state.storage.clone(), Duration::from_millis(50));

    // 3. Start Delivery Edge (ZERO Postgres connection!)
    let (edge_shutdown_tx, edge_shutdown_rx) = watch::channel(false);
    let edge_auth = AuthRegistry::new(snapshots.clone());
    edge_auth.refresh_from_object_store().await;
    edge_auth
        .spawn_nats_sync(nats.clone(), edge_shutdown_rx.clone())
        .await;
    let edge_cache = EnvironmentCache::new(
        snapshots.clone(),
        Some(nats.clone()),
        edge_shutdown_rx.clone(),
    );
    let edge_router = build_edge_router(EdgeState {
        auth: edge_auth,
        cache: edge_cache,
        keepalive_interval: Duration::from_secs(30),
    });
    let edge_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let edge_addr = edge_listener.local_addr().unwrap();
    let edge_base = format!("http://{edge_addr}");
    let mut edge_http_shutdown = edge_shutdown_rx.clone();
    tokio::spawn(async move {
        axum::serve(edge_listener, edge_router)
            .with_graceful_shutdown(async move {
                let _ = edge_http_shutdown.changed().await;
            })
            .await
            .unwrap();
    });

    // 4. Start Event Ingest Service
    let ingest_auth = IngestAuthCache::new(Some(snapshots.clone()), Some(cp_state.storage.clone()));
    ingest_auth.refresh().await;
    let ingest_tx = spawn_rollup_flusher(
        cp_state.telemetry.clone(),
        Duration::from_millis(100),
        edge_shutdown_rx.clone(),
    );
    let ingest_router = build_ingest_router(IngestState {
        auth: ingest_auth,
        telemetry: cp_state.telemetry.clone(),
        tx: ingest_tx,
        nats: Some(nats.clone()),
    });
    let ingest_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ingest_addr = ingest_listener.local_addr().unwrap();
    let ingest_base = format!("http://{ingest_addr}");
    let mut ingest_http_shutdown = edge_shutdown_rx.clone();
    tokio::spawn(async move {
        axum::serve(ingest_listener, ingest_router)
            .with_graceful_shutdown(async move {
                let _ = ingest_http_shutdown.changed().await;
            })
            .await
            .unwrap();
    });

    // 5. Start Local Webhook Receiver to verify HMAC-SHA256 signatures
    let received_webhooks: Arc<Mutex<Vec<(String, String, Bytes)>>> =
        Arc::new(Mutex::new(Vec::new()));
    let wh_store = received_webhooks.clone();
    let wh_router = Router::new().route(
        "/hook",
        post(move |headers: HeaderMap, body: Bytes| {
            let store = wh_store.clone();
            async move {
                let ev = headers
                    .get("x-skotos-event")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                let sig = headers
                    .get("x-skotos-signature")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                store.lock().unwrap().push((ev, sig, body));
                axum::http::StatusCode::OK
            }
        }),
    );
    let wh_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let wh_addr = wh_listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(wh_listener, wh_router).await.unwrap();
    });

    // 6. Authenticate via control-api & create an API token + signed webhook
    let http = reqwest::Client::new();
    let login_resp = http
        .post(format!("{cp_base}/api/v1/auth/login"))
        .json(&json!({ "email": admin_email, "password": "Phase2Password123!" }))
        .send()
        .await
        .unwrap();
    assert_eq!(login_resp.status(), 200);
    let cookie = login_resp
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    // Rotate SDK key for default/development to obtain plaintext SDK key
    let rot_resp: Value = http
        .post(format!(
            "{cp_base}/api/v1/projects/default/environments/development/keys/rotate"
        ))
        .header("cookie", &cookie)
        .json(&json!({ "kind": "sdk" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let sdk_key = rot_resp["plaintextKey"].as_str().unwrap().to_string();

    let env_resp: Value = http
        .get(format!(
            "{cp_base}/api/v1/projects/default/environments/development"
        ))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let client_side_id = env_resp["clientSideId"].as_str().unwrap().to_string();

    // Create signed outbound webhook
    let wh_secret = "sk_whsec_phase2_verify_secret";
    let wh_create = http
        .post(format!("{cp_base}/api/v1/webhooks"))
        .header("cookie", &cookie)
        .json(&json!({
            "name": "Phase 2 Test Hook",
            "url": format!("http://{wh_addr}/hook"),
            "secret": wh_secret,
            "events": ["flag.updated", "segment.created"],
            "envFilter": ["development"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(wh_create.status(), 201);

    // 7. Create a segment ("beta-users") & a client-side boolean flag referencing that segment
    let seg_resp = http
        .post(format!(
            "{cp_base}/api/v1/projects/default/environments/development/segments"
        ))
        .header("cookie", &cookie)
        .json(&json!({
            "key": "beta-users",
            "name": "Beta Users",
            "included": ["user-beta"],
            "rules": [{
                "id": "staff-rule",
                "clauses": [{
                    "contextKind": "user",
                    "attribute": "email",
                    "op": "endsWith",
                    "values": ["@skotos.dev"],
                    "negate": false
                }]
            }]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(seg_resp.status(), 201);

    let flag_resp: Value = http
        .post(format!("{cp_base}/api/v1/projects/default/flags"))
        .header("cookie", &cookie)
        .json(&json!({
            "key": "live-streaming-flag",
            "name": "Live Streaming Flag",
            "kind": "boolean",
            "variations": [
                { "value": true, "name": "On" },
                { "value": false, "name": "Off" }
            ],
            "clientSideAvailability": { "usingEnvironmentId": true, "usingMobileKey": true }
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let on_var = flag_resp["variations"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let off_var = flag_resp["variations"][1]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Give outbox worker a moment to publish initial key + flag + segment
    tokio::time::sleep(Duration::from_millis(180)).await;

    // 8. Connect live Rust Server SDK via SSE to delivery-edge + open Client Ping SSE stream
    let sdk =
        SkotosClient::new(SkotosConfig::new(&sdk_key, &edge_base).with_events_url(&ingest_base));
    assert!(
        sdk.wait_for_initialization(Duration::from_secs(5)).await,
        "Rust SDK initialized via live SSE put"
    );
    let mut sdk_changes = sdk.subscribe_changes();

    let beta_ctx = Context::single(SingleContext::new("user", "user-beta"));
    let other_ctx = Context::single(SingleContext::new("user", "user-regular"));
    assert!(!sdk.bool_variation("live-streaming-flag", &beta_ctx, false));

    // Open live Client-Side Ping-to-Re-evaluate SSE stream
    let ping_resp = http
        .get(format!("{edge_base}/stream/v1/eval/{client_side_id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(ping_resp.status(), 200);
    let mut ping_stream = ping_resp.bytes_stream();
    // Consume initial ping frame
    let initial_ping = tokio::time::timeout(Duration::from_secs(2), ping_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&initial_ping).contains("event: ping"));

    // 9. PATCH flag ON with segmentMatch rule -> Measure < 200ms end-to-end streaming latency!
    let t0 = Instant::now();
    let patch_resp = http
        .patch(format!(
            "{cp_base}/api/v1/projects/default/flags/live-streaming-flag"
        ))
        .header("cookie", &cookie)
        .json(&json!({
            "environmentKey": "development",
            "expectedVersion": 1,
            "comment": "Turn on for beta-users segment",
            "instructions": [
                { "kind": "turnFlagOn" },
                { "kind": "updateFallthrough", "variationId": off_var },
                {
                    "kind": "addRule",
                    "id": "beta-segment-rule",
                    "clauses": [{
                        "contextKind": "user",
                        "attribute": "key",
                        "op": "segmentMatch",
                        "values": ["beta-users"],
                        "negate": false
                    }],
                    "variationId": on_var
                }
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(patch_resp.status(), 200);

    // Wait for live SSE patch notification in Rust Server SDK
    let change_ev = tokio::time::timeout(Duration::from_secs(2), sdk_changes.recv())
        .await
        .expect("received change event promptly")
        .expect("broadcast ok");
    let propagation_ms = t0.elapsed().as_millis();
    println!("Measured live PATCH -> Outbox -> MinIO -> NATS -> delivery-edge -> Rust SDK latency: {propagation_ms} ms");
    assert_eq!(change_ev.flag_key, "live-streaming-flag");
    assert!(
        propagation_ms < 200,
        "expected < 200ms p95 propagation SLA, got {propagation_ms}ms"
    );

    // Verify Server SDK evaluates segment rule locally (< 1ms)
    assert!(sdk.bool_variation("live-streaming-flag", &beta_ctx, false));
    assert!(!sdk.bool_variation("live-streaming-flag", &other_ctx, true));

    // Verify Client-Side Ping-to-Re-evaluate SSE received ping frame!
    let update_ping = tokio::time::timeout(Duration::from_secs(2), ping_stream.next())
        .await
        .expect("client SSE ping received")
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&update_ping).contains("event: ping"));

    // Verify Client-Side GET /evalx (base64url) AND REPORT /evalx (body)
    let encoded_beta = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&beta_ctx).unwrap());
    let evalx_get = http
        .get(format!(
            "{edge_base}/sdk/v1/evalx/{client_side_id}/contexts/{encoded_beta}?withReasons=true"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(evalx_get.status(), 200);
    let evalx_etag = evalx_get
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let evalx_body: Value = evalx_get.json().await.unwrap();
    assert_eq!(evalx_body["live-streaming-flag"]["value"], json!(true));
    assert_eq!(
        evalx_body["live-streaming-flag"]["reason"]["kind"],
        json!("RULE_MATCH")
    );

    // Verify 304 Not Modified on evalx ETag
    let evalx_304 = http
        .get(format!(
            "{edge_base}/sdk/v1/evalx/{client_side_id}/contexts/{encoded_beta}?withReasons=true"
        ))
        .header("if-none-match", &evalx_etag)
        .send()
        .await
        .unwrap();
    assert_eq!(evalx_304.status(), 304);

    // Verify REPORT /sdk/v1/evalx/{cid}/context
    let report_method = reqwest::Method::from_bytes(b"REPORT").unwrap();
    let evalx_report: Value = http
        .request(
            report_method,
            format!("{edge_base}/sdk/v1/evalx/{client_side_id}/context"),
        )
        .json(&json!({ "kind": "user", "key": "staff-1", "email": "ada@skotos.dev" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(evalx_report["live-streaming-flag"]["value"], json!(true));

    // 10. Verify Polling /sdk/v1/latest-all (gzip, zstd, and ETag 304)
    let poll_gz = http
        .get(format!("{edge_base}/sdk/v1/latest-all"))
        .header("authorization", &sdk_key)
        .header("accept-encoding", "gzip")
        .send()
        .await
        .unwrap();
    assert_eq!(poll_gz.status(), 200);
    assert_eq!(poll_gz.headers().get("content-encoding").unwrap(), "gzip");
    let poll_etag = poll_gz
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let gz_bytes = poll_gz.bytes().await.unwrap();
    let mut dec = GzDecoder::new(gz_bytes.as_ref());
    let mut decoded_json = String::new();
    dec.read_to_string(&mut decoded_json).unwrap();
    assert!(decoded_json.contains("live-streaming-flag"));

    let poll_zst = http
        .get(format!("{edge_base}/sdk/v1/latest-all"))
        .header("authorization", &sdk_key)
        .header("accept-encoding", "zstd")
        .send()
        .await
        .unwrap();
    assert_eq!(poll_zst.headers().get("content-encoding").unwrap(), "zstd");
    let zst_bytes = poll_zst.bytes().await.unwrap();
    let decompressed_zst = zstd::bulk::decompress(&zst_bytes, 4 * 1024 * 1024).unwrap();
    assert!(String::from_utf8(decompressed_zst)
        .unwrap()
        .contains("beta-users"));

    let poll_304 = http
        .get(format!("{edge_base}/sdk/v1/latest-all"))
        .header("authorization", &sdk_key)
        .header("if-none-match", &poll_etag)
        .send()
        .await
        .unwrap();
    assert_eq!(poll_304.status(), 304);

    // 11. Flush SDK summary rollups -> event-ingest -> PostgreSQL flag_eval_rollups -> Usage API
    let flushed = sdk.flush().await;
    assert!(flushed > 0, "SDK flushed summary batch");
    let usage_deadline = Instant::now() + Duration::from_secs(5);
    let usage: Value = loop {
        let candidate: Value = http
            .get(format!(
                "{cp_base}/api/v1/projects/default/flags/live-streaming-flag/environments/development/usage"
            ))
            .header("cookie", &cookie)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if candidate["totalEvaluations"].as_i64().unwrap_or(0) >= 3
            && candidate["lastEvaluatedAt"].is_string()
        {
            break candidate;
        }
        assert!(
            Instant::now() < usage_deadline,
            "expected >= 3 recorded evaluations within 5s, got {candidate}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert!(usage["totalEvaluations"].as_i64().unwrap_or(0) >= 3);
    assert!(usage["lastEvaluatedAt"].is_string());

    // 12. Verify Signed Outbound Webhook received valid HMAC-SHA256 signature
    let webhook_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let delivered = {
            let hooks = received_webhooks.lock().unwrap();
            hooks.iter().any(|(ev, sig, body)| {
                ev == "flag.updated" && verify_webhook_signature(wh_secret, body, sig)
            })
        };
        if delivered {
            break;
        }
        assert!(
            Instant::now() < webhook_deadline,
            "expected valid HMAC-SHA256 signed flag.updated webhook delivery within 5s"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    // 13. Verify Flag Trigger (instant APM rollback -> turns flag OFF)
    let trig_resp: Value = http
        .post(format!(
            "{cp_base}/api/v1/projects/default/flags/live-streaming-flag/environments/development/triggers"
        ))
        .header("cookie", &cookie)
        .json(&json!({ "action": "turnFlagOff", "description": "Datadog P1 rollback" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let trigger_path = trig_resp["triggerPath"].as_str().unwrap();
    let exec_trig: Value = http
        .post(format!("{cp_base}{trigger_path}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(exec_trig["on"], json!(false));

    // 14. Verify Scheduled Change execution (turns flag back ON)
    let sched_resp = http
        .post(format!(
            "{cp_base}/api/v1/projects/default/flags/live-streaming-flag/environments/development/schedules"
        ))
        .header("cookie", &cookie)
        .json(&json!({
            "executeAt": Utc::now() - chrono::Duration::seconds(1),
            "comment": "Scheduled re-enable",
            "instructions": [{ "kind": "turnFlagOn" }]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(sched_resp.status(), 201);
    let processed = scheduler.process_due_once().await.unwrap();
    assert!(processed >= 1);

    // 15. Verify Environment Copy (development -> production)
    let copy_resp = http
        .post(format!(
            "{cp_base}/api/v1/projects/default/flags/live-streaming-flag/copy"
        ))
        .header("cookie", &cookie)
        .json(&json!({
            "sourceEnvironmentKey": "development",
            "targetEnvironmentKey": "production",
            "copyOnState": true,
            "comment": "Promote development targeting to production"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(copy_resp.status(), 200);
    let copied: Value = copy_resp.json().await.unwrap();
    assert_eq!(copied["on"], json!(true));

    // Wait deterministically until the scheduled re-enable snapshot reaches delivery-edge and SDK
    // before shutting down the control plane and outbox worker for the outage survivability check.
    let reenable_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let evalx_ready = http
            .get(format!(
                "{edge_base}/sdk/v1/evalx/{client_side_id}/contexts/{encoded_beta}"
            ))
            .send()
            .await
            .ok();
        let evalx_is_true = match evalx_ready {
            Some(resp) if resp.status().is_success() => resp
                .json::<Value>()
                .await
                .ok()
                .is_some_and(|v| v["live-streaming-flag"]["value"] == json!(true)),
            _ => false,
        };
        if evalx_is_true && sdk.bool_variation("live-streaming-flag", &beta_ctx, false) {
            break;
        }
        assert!(
            Instant::now() < reenable_deadline,
            "scheduled re-enable snapshot did not reach delivery-edge and SDK within 5s"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    // 16. Verify 100% Control Plane / DB Outage Survivability!
    // Shut down control-api AND outbox worker completely; delivery-edge + SDK must remain 100% live!
    let _ = worker_shutdown_tx.send(true);
    let _ = cp_shutdown_tx.send(true);
    let _ = outbox_handle.await;
    let _ = cp_task.await;

    // Newly polling SDK and client evalx MUST succeed with Control Plane completely stopped
    let outage_poll = http
        .get(format!("{edge_base}/sdk/v1/latest-all"))
        .header("authorization", &sdk_key)
        .send()
        .await
        .unwrap();
    assert_eq!(outage_poll.status(), 200);

    let outage_evalx: Value = http
        .get(format!(
            "{edge_base}/sdk/v1/evalx/{client_side_id}/contexts/{encoded_beta}"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(outage_evalx["live-streaming-flag"]["value"], json!(true));
    assert!(sdk.bool_variation("live-streaming-flag", &beta_ctx, false));

    sdk.close().await;
    let _ = edge_shutdown_tx.send(true);
}
