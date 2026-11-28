use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use delivery_edge::{build_router as build_edge_router, AuthRegistry, EdgeState, EnvironmentCache};
use event_ingest::{
    build_router as build_ingest_router, spawn_rollup_flusher, IngestAuthCache, IngestState,
};
use futures::StreamExt;
use relay_proxy::{build_router as build_relay_router, RelayEnvConfig, RelayMode, RelayState};
use serde_json::{json, Value};
use skotos_auth::{AuthMode, BootstrapAdminConfig};
use skotos_code_refs::{scan_directory, upload_code_refs};
use skotos_fanout::{
    NatsFanout, OutboxWorker, OutboxWorkerConfig, S3Config, SchedulerWorker, SnapshotStore,
};
use skotos_mcp::{JsonRpcRequest, McpConfig, McpServer};
use tokio::net::TcpListener;
use tokio::sync::watch;
use uuid::Uuid;

use control_api::{init_app, AppConfig};

async fn login_with_cookie(base: &str, email: &str, password: &str) -> reqwest::Client {
    let tmp = reqwest::Client::new();
    let resp = tmp
        .post(format!("{base}/api/v1/auth/login"))
        .json(&json!({ "email": email, "password": password }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "login failed for {email}");
    let cookie = resp
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .expect("set-cookie header present")
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::COOKIE,
        reqwest::header::HeaderValue::from_str(&cookie).unwrap(),
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap()
}

/// End-to-end Phase 3 Enterprise integration test verifying:
/// 1. Custom RBAC allow/deny resource-path policies (deny overrides writer) + Team role inheritance
/// 2. Mandatory peer Approval Workflows (409 guard, self-approval guard, peer review, atomic apply)
/// 3. Multi-Step Flag Workflows (SchedulerWorker auto-advance) + Release Pipelines & Flag Releases
/// 4. High-Scale Big Segments (Zstd hash-block sync to MinIO -> DB-free delivery-edge evalx)
/// 5. Open-Source DB-free Relay Proxy (upstream SSE -> local polling/evalx/event forwarding)
/// 6. Live Events Debugger (ephemeral core NATS stream from event-ingest -> control-api SSE)
/// 7. SCIM 2.0 provisioning, Audit Log Zstd archive export, skotos-code-refs, and skotos-mcp
#[tokio::test]
async fn phase3_enterprise_governance_big_segments_relay_debugger_and_devtools_e2e() {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://skotos:skotos@127.0.0.1:5432/skotos".to_string());
    let nats_url =
        std::env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_string());
    let s3_cfg = S3Config::from_env();

    SnapshotStore::ensure_s3_bucket_exists(&s3_cfg)
        .await
        .expect("MinIO bucket ready");
    let snapshots = SnapshotStore::from_s3_config(&s3_cfg).expect("MinIO snapshot store");
    let nats = NatsFanout::connect(&nats_url)
        .await
        .expect("NATS JetStream connected");

    // 1. Start Control Plane API
    let slug = format!("p3-e2e-{}", Uuid::new_v4().simple());
    let owner_email = format!("owner@{slug}.test");
    let config = AppConfig {
        listen_addr: "127.0.0.1:0".to_string(),
        database_url: database_url.clone(),
        database_max_connections: 8,
        run_migrations: true,
        log_format: "pretty".to_string(),
        cors_origins: vec!["http://localhost:5173".to_string()],
        auth_mode: AuthMode::SelfHosted,
        session_secret: "test-session-secret-phase3-at-least-32-bytes".to_string(),
        cookie_secure: false,
        bootstrap: BootstrapAdminConfig {
            org_slug: slug.clone(),
            org_name: "Phase 3 Enterprise E2E Org".to_string(),
            admin_email: owner_email.clone(),
            admin_password: "Phase3Password123!".to_string(),
        },
        external_oidc: None,
    };

    let (cp_state, cp_router) = init_app(&config).await.expect("control-api init");
    let cp_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cp_addr = cp_listener.local_addr().unwrap();
    let cp_base = format!("http://{cp_addr}");

    let (shutdown_tx, mut cp_shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        axum::serve(cp_listener, cp_router)
            .with_graceful_shutdown(async move {
                let _ = cp_shutdown_rx.changed().await;
            })
            .await
            .unwrap();
    });

    // Clear stale unpublished outbox events from other runs so worker stays snappy
    sqlx::query("UPDATE outbox_events SET published_at = now() WHERE published_at IS NULL")
        .execute(cp_state.storage.pool())
        .await
        .unwrap();

    // 2. Start OutboxWorker & SchedulerWorker
    let worker_rx = shutdown_tx.subscribe();
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
    tokio::spawn(outbox_worker.clone().run_until_shutdown(worker_rx.clone()));
    let scheduler = SchedulerWorker::new(cp_state.storage.clone(), Duration::from_millis(50));

    // 3. Start DB-Free Delivery Edge
    let edge_rx = shutdown_tx.subscribe();
    let edge_auth = AuthRegistry::new(snapshots.clone());
    edge_auth.refresh_from_object_store().await;
    edge_auth
        .spawn_nats_sync(nats.clone(), edge_rx.clone())
        .await;
    let edge_cache = EnvironmentCache::new(snapshots.clone(), Some(nats.clone()), edge_rx.clone());
    let edge_router = build_edge_router(EdgeState {
        auth: edge_auth,
        cache: edge_cache,
        keepalive_interval: Duration::from_secs(30),
    });
    let edge_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let edge_addr = edge_listener.local_addr().unwrap();
    let edge_base = format!("http://{edge_addr}");
    let mut edge_http_rx = edge_rx.clone();
    tokio::spawn(async move {
        axum::serve(edge_listener, edge_router)
            .with_graceful_shutdown(async move {
                let _ = edge_http_rx.changed().await;
            })
            .await
            .unwrap();
    });

    // 4. Start Event Ingest with Live NATS Debugger enabled
    let ingest_auth = IngestAuthCache::new(Some(snapshots.clone()), Some(cp_state.storage.clone()));
    ingest_auth.refresh().await;
    let ingest_tx = spawn_rollup_flusher(
        cp_state.telemetry.clone(),
        Duration::from_millis(100),
        edge_rx.clone(),
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
    let mut ingest_http_rx = edge_rx.clone();
    tokio::spawn(async move {
        axum::serve(ingest_listener, ingest_router)
            .with_graceful_shutdown(async move {
                let _ = ingest_http_rx.changed().await;
            })
            .await
            .unwrap();
    });

    // Helper: create authenticated HTTP client for owner
    let owner_client = login_with_cookie(&cp_base, &owner_email, "Phase3Password123!").await;

    // Mint an Owner API token for skotos-mcp and skotos-code-refs
    let tok_resp: Value = owner_client
        .post(format!("{cp_base}/api/v1/tokens"))
        .json(&json!({ "name": "Phase3 E2E Token", "role": "owner" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let owner_api_token = tok_resp["plaintextToken"].as_str().unwrap().to_string();

    // Rotate development SDK key so we know plaintext SDK key + clientSideId for Relay Proxy
    let envs_resp: Value = owner_client
        .get(format!("{cp_base}/api/v1/projects/default/environments"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let dev_env = envs_resp["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["key"] == "development")
        .unwrap();
    let dev_env_id: Uuid = dev_env["id"].as_str().unwrap().parse().unwrap();
    let dev_client_id = dev_env["clientSideId"].as_str().unwrap().to_string();

    let rot_resp: Value = owner_client
        .post(format!(
            "{cp_base}/api/v1/projects/default/environments/development/keys/rotate"
        ))
        .json(&json!({ "kind": "sdk", "gracePeriodSeconds": 60 }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let dev_sdk_key = rot_resp["plaintextKey"].as_str().unwrap().to_string();

    // Create a primary test flag with client-side availability enabled
    let create_flag_res = owner_client
        .post(format!("{cp_base}/api/v1/projects/default/flags"))
        .json(&json!({
            "key": "p3-enterprise-flag",
            "name": "Phase 3 Enterprise Flag",
            "kind": "boolean",
            "temporary": true,
            "clientSideAvailability": {
                "usingEnvironmentId": true,
                "usingMobileKey": true
            },
            "variations": [
                { "value": true, "name": "On" },
                { "value": false, "name": "Off" }
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_flag_res.status(), 201);

    // =========================================================================
    // PILLAR 1: Custom RBAC Policies (Deny overrides Writer) + Team Inheritance
    // =========================================================================
    let writer_email = format!("writer@{slug}.test");
    let writer_member: Value = owner_client
        .post(format!("{cp_base}/api/v1/members"))
        .json(&json!({
            "email": writer_email,
            "displayName": "Writer User",
            "role": "writer",
            "password": "WriterPassword123!"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let writer_id = writer_member["id"].as_str().unwrap().to_string();

    // Create Custom Role that DENIES all flag writes in production
    let role_res = owner_client
        .post(format!("{cp_base}/api/v1/roles"))
        .json(&json!({
            "key": "deny-prod-flags",
            "name": "Deny Production Flags",
            "description": "Blocks direct writes to production flags",
            "statements": [{
                "effect": "deny",
                "actions": ["*"],
                "resources": ["proj/*:env/production:flag/*"]
            }]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(role_res.status(), 201);

    // Attach role via a Team to verify Team-inherited RBAC policy evaluation!
    let team_res = owner_client
        .post(format!("{cp_base}/api/v1/teams"))
        .json(&json!({
            "key": "app-devs",
            "name": "Application Developers",
            "customRoleKeys": ["deny-prod-flags"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(team_res.status(), 201);

    let add_tm_res = owner_client
        .post(format!(
            "{cp_base}/api/v1/teams/app-devs/members/{writer_id}"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(add_tm_res.status(), 204);

    // Sign in as writer and try patching production -> MUST be 403 Forbidden (explicit deny wins!)
    let writer_client = login_with_cookie(&cp_base, &writer_email, "WriterPassword123!").await;

    let prod_denied = writer_client
        .patch(format!(
            "{cp_base}/api/v1/projects/default/flags/p3-enterprise-flag"
        ))
        .json(&json!({
            "environmentKey": "production",
            "instructions": [{ "kind": "turnFlagOn" }],
            "comment": "Should be denied by custom RBAC role"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        prod_denied.status(),
        403,
        "explicit deny custom role inherited from Team MUST override built-in writer role"
    );

    // =========================================================================
    // PILLAR 2: Mandatory Approval Workflows (409 guard + peer review + apply)
    // =========================================================================
    let set_appr = owner_client
        .put(format!(
            "{cp_base}/api/v1/projects/default/environments/development/approval-settings"
        ))
        .json(&json!({
            "required": true,
            "minApprovals": 1,
            "allowSelfApproval": false
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(set_appr.status(), 200);

    // Direct PATCH by writer in development MUST now fail with 409 Conflict (`approval_required`)
    let direct_patch = writer_client
        .patch(format!(
            "{cp_base}/api/v1/projects/default/flags/p3-enterprise-flag"
        ))
        .json(&json!({
            "environmentKey": "development",
            "instructions": [{ "kind": "turnFlagOn" }],
            "comment": "Bypass attempt"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(direct_patch.status(), 409);
    let conflict_body: Value = direct_patch.json().await.unwrap();
    assert_eq!(conflict_body["code"], "approval_required");

    // Writer submits an Approval Request
    let appr_req: Value = writer_client
        .post(format!(
            "{cp_base}/api/v1/projects/default/flags/p3-enterprise-flag/environments/development/approvals"
        ))
        .json(&json!({
            "description": "Turn on Phase 3 flag in dev",
            "comment": "Peer review requested",
            "instructions": [{ "kind": "turnFlagOn" }]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let approval_id = appr_req["id"].as_str().unwrap().to_string();
    assert_eq!(appr_req["status"], "pending");

    // Self-approval by requester MUST be rejected (400 validation error)
    let self_rev = writer_client
        .post(format!("{cp_base}/api/v1/approvals/{approval_id}/reviews"))
        .json(&json!({ "kind": "approve", "comment": "Self approving" }))
        .send()
        .await
        .unwrap();
    assert_eq!(self_rev.status(), 400);

    // Owner approves the request -> status becomes "approved"
    let peer_rev: Value = owner_client
        .post(format!("{cp_base}/api/v1/approvals/{approval_id}/reviews"))
        .json(&json!({ "kind": "approve", "comment": "LGTM!" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(peer_rev["status"], "approved");

    // Owner applies the approved request -> flag turns ON atomically!
    let applied: Value = owner_client
        .post(format!("{cp_base}/api/v1/approvals/{approval_id}/apply"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(applied["approval"]["status"], "applied");
    assert_eq!(applied["patch"]["envConfig"]["on"], true);

    // Disable approval requirement on development for subsequent workflow/big-segment steps
    owner_client
        .put(format!(
            "{cp_base}/api/v1/projects/default/environments/development/approval-settings"
        ))
        .json(&json!({
            "required": false,
            "minApprovals": 1,
            "allowSelfApproval": false
        }))
        .send()
        .await
        .unwrap();

    // =========================================================================
    // PILLAR 3: Multi-Step Flag Workflows + Release Pipelines & Flag Releases
    // =========================================================================
    let wf_res: Value = owner_client
        .post(format!(
            "{cp_base}/api/v1/projects/default/flags/p3-enterprise-flag/environments/development/workflows"
        ))
        .json(&json!({
            "name": "Auto Toggle Workflow",
            "description": "Turns flag off then back on via SchedulerWorker",
            "stages": [
                { "name": "Turn Off", "waitSeconds": 0, "instructions": [{ "kind": "turnFlagOff" }] },
                { "name": "Turn Back On", "waitSeconds": 0, "instructions": [{ "kind": "turnFlagOn" }] }
            ]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(wf_res["status"], "active");

    // Drive SchedulerWorker twice to execute both stages automatically
    scheduler.process_due_once().await.unwrap();
    scheduler.process_due_once().await.unwrap();
    let wf_list: Value = owner_client
        .get(format!(
            "{cp_base}/api/v1/projects/default/flags/p3-enterprise-flag/environments/development/workflows"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(wf_list["items"][0]["status"], "completed");

    // Create a Release Pipeline and attach + promote flag through phase 0
    let pipe_res = owner_client
        .post(format!("{cp_base}/api/v1/projects/default/release-pipelines"))
        .json(&json!({
            "key": "p3-pipeline",
            "name": "Dev to Prod Pipeline",
            "phases": [
                { "name": "Dev Phase", "environmentKey": "development", "instructions": [{ "kind": "turnFlagOn" }] },
                { "name": "Prod Phase", "environmentKey": "production", "copyFromPreviousEnv": true }
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(pipe_res.status(), 201);

    let attach_res: Value = owner_client
        .post(format!(
            "{cp_base}/api/v1/projects/default/flags/p3-enterprise-flag/release"
        ))
        .json(&json!({ "pipelineKey": "p3-pipeline" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(attach_res["pipelineKey"], "p3-pipeline");

    let start_p0: Value = owner_client
        .post(format!(
            "{cp_base}/api/v1/projects/default/flags/p3-enterprise-flag/release/phases/0/start"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(start_p0["phases"][0]["status"], "completed");

    // =========================================================================
    // PILLAR 4: High-Scale Big Segments (MinIO Zstd Hash-Block -> delivery-edge)
    // =========================================================================
    let big_seg_res = owner_client
        .post(format!(
            "{cp_base}/api/v1/projects/default/environments/development/segments"
        ))
        .json(&json!({
            "key": "enterprise-vip-bigseg",
            "name": "Enterprise VIP Big Segment",
            "kind": "big"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(big_seg_res.status(), 201);

    let bs_mut: Value = owner_client
        .post(format!(
            "{cp_base}/api/v1/projects/default/environments/development/segments/enterprise-vip-bigseg/members"
        ))
        .json(&json!({
            "contextKind": "user",
            "addIncluded": ["vip-user-1", "vip-user-2"],
            "addExcluded": ["blocked-user-9"]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(bs_mut["includedCount"], 2);
    assert_eq!(bs_mut["excludedCount"], 1);

    // Fetch flag variations to build a targeting rule referencing our Big Segment
    let flag_detail: Value = owner_client
        .get(format!(
            "{cp_base}/api/v1/projects/default/flags/p3-enterprise-flag"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let on_var_id = flag_detail["variations"][0]["id"].as_str().unwrap();
    let off_var_id = flag_detail["variations"][1]["id"].as_str().unwrap();

    // Configure flag: ON, fallthrough = false (off_var_id), rule: segmentMatch "enterprise-vip-bigseg" -> true (on_var_id)
    let patch_bs_rule = owner_client
        .patch(format!(
            "{cp_base}/api/v1/projects/default/flags/p3-enterprise-flag"
        ))
        .json(&json!({
            "environmentKey": "development",
            "instructions": [
                { "kind": "turnFlagOn" },
                { "kind": "updateFallthrough", "variationId": off_var_id },
                {
                    "kind": "addRule",
                    "description": "VIP Big Segment Rule",
                    "clauses": [{
                        "contextKind": "user",
                        "attribute": "key",
                        "op": "segmentMatch",
                        "values": ["enterprise-vip-bigseg"]
                    }],
                    "variationId": on_var_id
                }
            ],
            "comment": "Target Big Segment VIP users"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(patch_bs_rule.status(), 200);

    // Poll DB-free delivery-edge evalx until Big Segment + rule propagate from MinIO/NATS!
    let plain_http = reqwest::Client::new();
    let vip_ctx_b64 = URL_SAFE_NO_PAD.encode(r#"{"kind":"user","key":"vip-user-1"}"#);
    let blocked_ctx_b64 = URL_SAFE_NO_PAD.encode(r#"{"kind":"user","key":"blocked-user-9"}"#);
    let non_vip_b64 = URL_SAFE_NO_PAD.encode(r#"{"kind":"user","key":"regular-user-42"}"#);

    let mut big_seg_matched = false;
    for _ in 0..60 {
        let resp = plain_http
            .get(format!(
                "{edge_base}/sdk/v1/evalx/{dev_client_id}/contexts/{vip_ctx_b64}?withReasons=true"
            ))
            .send()
            .await
            .unwrap();
        if resp.status() == 200 {
            let body: Value = resp.json().await.unwrap();
            if body["p3-enterprise-flag"]["value"] == json!(true)
                && body["p3-enterprise-flag"]["reason"]["kind"] == json!("RULE_MATCH")
            {
                big_seg_matched = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        big_seg_matched,
        "DB-free delivery-edge must evaluate Big Segment Zstd snapshot via RULE_MATCH and return true for vip-user-1"
    );

    let blocked_eval: Value = plain_http
        .get(format!(
            "{edge_base}/sdk/v1/evalx/{dev_client_id}/contexts/{blocked_ctx_b64}"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(blocked_eval["p3-enterprise-flag"]["value"], json!(false));

    let non_vip_eval: Value = plain_http
        .get(format!(
            "{edge_base}/sdk/v1/evalx/{dev_client_id}/contexts/{non_vip_b64}"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(non_vip_eval["p3-enterprise-flag"]["value"], json!(false));

    // =========================================================================
    // PILLAR 5: Open-Source DB-Free Relay Proxy (Proxy Mode against delivery-edge)
    // =========================================================================
    let relay_mode = RelayMode::Proxy {
        upstream_stream_url: edge_base.clone(),
        upstream_events_url: Some(ingest_base.clone()),
    };
    let relay_state = RelayState::new(
        vec![RelayEnvConfig {
            name: "development".to_string(),
            sdk_key: dev_sdk_key.clone(),
            client_side_id: Some(dev_client_id.clone()),
            mobile_key: None,
        }],
        &relay_mode,
        Duration::from_secs(30),
    )
    .expect("relay state init");
    relay_state.spawn_upstream_sync(edge_base.clone(), shutdown_tx.subscribe());

    let relay_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let relay_addr = relay_listener.local_addr().unwrap();
    let relay_base = format!("http://{relay_addr}");
    let mut relay_http_rx = shutdown_tx.subscribe();
    tokio::spawn(async move {
        axum::serve(relay_listener, build_relay_router(relay_state))
            .with_graceful_shutdown(async move {
                let _ = relay_http_rx.changed().await;
            })
            .await
            .unwrap();
    });

    // Wait for Relay Proxy upstream SSE handshake -> /status reports status=="connected"
    let mut relay_ready = false;
    for _ in 0..40 {
        let status_val: Value = plain_http
            .get(format!("{relay_base}/status"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if status_val["environments"]["development"]["status"] == json!("connected") {
            relay_ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        relay_ready,
        "Relay Proxy must connect upstream to delivery-edge SSE"
    );

    // Verify Relay Proxy serves /sdk/v1/latest-all with our flag
    let relay_poll: Value = plain_http
        .get(format!("{relay_base}/sdk/v1/latest-all"))
        .header("Authorization", &dev_sdk_key)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(relay_poll["flags"]["p3-enterprise-flag"].is_object());

    // =========================================================================
    // PILLAR 6: Live Events Debugger (ephemeral NATS stream from event-ingest)
    // =========================================================================
    let mut debug_sub = nats
        .subscribe_debug_events(dev_env_id)
        .await
        .expect("subscribe debug events");

    // Submit summary event through Relay Proxy -> forwarded to event-ingest -> published on NATS!
    let now_ms = chrono::Utc::now().timestamp_millis();
    let fwd_res = plain_http
        .post(format!("{relay_base}/events/v1/bulk"))
        .header("Authorization", &dev_sdk_key)
        .json(&json!([{
            "kind": "summary",
            "startDate": now_ms - 1000,
            "endDate": now_ms,
            "features": {
                "p3-enterprise-flag": {
                    "contextKinds": ["user"],
                    "counters": [{ "variation": 0, "version": 1, "value": true, "count": 7 }]
                }
            }
        }]))
        .send()
        .await
        .unwrap();
    assert_eq!(fwd_res.status(), 202);

    let debug_msg = tokio::time::timeout(Duration::from_secs(3), debug_sub.next())
        .await
        .expect("received ephemeral live debug event within 3s")
        .expect("nats message");
    let debug_evt: skotos_domain::LiveEvalDebugEvent =
        serde_json::from_slice(&debug_msg.payload).unwrap();
    assert_eq!(debug_evt.flag_key.as_deref(), Some("p3-enterprise-flag"));
    assert_eq!(debug_evt.count, 7);

    // =========================================================================
    // PILLAR 7: SCIM 2.0, Audit Log Zstd Archive, Code Refs Scanner, and MCP
    // =========================================================================
    // 7a. SCIM 2.0 Provisioning
    let scim_res: Value = owner_client
        .post(format!("{cp_base}/scim/v2/Users"))
        .json(&json!({
            "userName": format!("scim-user@{slug}.test"),
            "displayName": "SCIM Provisioned Engineer",
            "active": true
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(scim_res["userName"], format!("scim-user@{slug}.test"));

    // 7b. Audit Log Zstd Archival to MinIO
    let arch_res: Value = owner_client
        .post(format!("{cp_base}/api/v1/audit-log/export"))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(arch_res["recordCount"].as_i64().unwrap() > 0);
    assert!(arch_res["compressedBytes"].as_i64().unwrap() > 0);
    assert_eq!(arch_res["sha256"].as_str().unwrap().len(), 64);

    // 7c. skotos-code-refs scanner + Control Plane upload
    let tmp_repo = std::env::temp_dir().join(format!("skotos-p3-coderefs-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&tmp_repo).unwrap();
    std::fs::write(
        tmp_repo.join("checkout.ts"),
        "export const active = client.boolVariation(\"p3-enterprise-flag\", ctx, false);\n",
    )
    .unwrap();
    let hunks = scan_directory(&tmp_repo, &["p3-enterprise-flag".to_string()]).unwrap();
    assert_eq!(hunks.len(), 1);
    upload_code_refs(
        &cp_base,
        &owner_api_token,
        "default",
        "acme/p3-repo",
        "main",
        "deadbeef12345678",
        &hunks,
    )
    .await
    .expect("upload code refs");
    let _ = std::fs::remove_dir_all(&tmp_repo);

    let refs_resp: Value = owner_client
        .get(format!(
            "{cp_base}/api/v1/projects/default/flags/p3-enterprise-flag/code-refs"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(refs_resp["items"][0]["referenceCount"], 1);
    assert_eq!(refs_resp["items"][0]["extinct"], false);

    // 7d. skotos-mcp live JSON-RPC 2.0 tool execution against control-api
    let mcp = McpServer::new(McpConfig {
        api_url: cp_base.clone(),
        api_token: Some(owner_api_token.clone()),
    });
    let mcp_resp = mcp
        .handle_request(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(42)),
            method: "tools/call".to_string(),
            params: json!({
                "name": "list_flags",
                "arguments": { "projectKey": "default" }
            }),
        })
        .await
        .expect("mcp response");
    assert!(mcp_resp.error.is_none());
    let mcp_text = mcp_resp.result.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        mcp_text.contains("p3-enterprise-flag"),
        "MCP list_flags tool must return newly created Phase 3 flag"
    );

    let _ = shutdown_tx.send(true);
}
