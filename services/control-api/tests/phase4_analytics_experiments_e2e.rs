use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State as AxumState;
use axum::routing::post;
use axum::Router;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use delivery_edge::{build_router as build_edge_router, AuthRegistry, EdgeState, EnvironmentCache};
use event_ingest::{
    build_router as build_ingest_router, spawn_rollup_flusher, IngestAuthCache, IngestState,
};
use serde_json::{json, Value};
use skotos_auth::{AuthMode, BootstrapAdminConfig};
use skotos_domain::{MetricAggregation, MetricDefinition, MetricKind, MetricSuccessDirection};
use skotos_fanout::{
    NatsFanout, OutboxWorker, OutboxWorkerConfig, S3Config, SchedulerWorker, SnapshotStore,
};
use skotos_telemetry::{
    ClickHouseConfig, ClickHouseTelemetryStore, CustomEventRecord, ExposureRecord,
    MigrationOpRecord, TelemetryStore,
};
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

/// Full Phase 4 live stack end-to-end test verifying:
/// 1. Metrics, Funnels, Mutual Exclusion Layers, and A/B Experiment (Bayesian + Frequentist + SRM)
/// 2. Guarded Rollouts automated regression detection & TurnFlagOff rollback via SchedulerWorker
/// 3. Feature Monitoring per-variation exposures/errors/latency
/// 4. 2/4/6-Stage Migration Flags + `migration_op` consistency/latency telemetry
/// 5. Synced Segments reverse-ETL push -> Big Segment Zstd snapshot -> DB-free Delivery Edge evalx
/// 6. Real-Time Data Export Sinks (Zstd NDJSON flush to MinIO) & Engineering Insights (DORA)
/// 7. ClickHouse HTTP telemetry store driver (DDL, JSONEachRow inserts, JSON analytical queries)
#[tokio::test]
async fn phase4_analytics_experiments_guardrails_migrations_synced_segments_and_clickhouse_e2e() {
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
    let slug = format!("p4-e2e-{}", Uuid::new_v4().simple());
    let owner_email = format!("owner@{slug}.test");
    let config = AppConfig {
        listen_addr: "127.0.0.1:0".to_string(),
        database_url: database_url.clone(),
        database_max_connections: 8,
        run_migrations: true,
        log_format: "pretty".to_string(),
        cors_origins: vec!["http://localhost:5173".to_string()],
        auth_mode: AuthMode::SelfHosted,
        session_secret: "test-session-secret-phase4-at-least-32-bytes".to_string(),
        cookie_secure: false,
        bootstrap: BootstrapAdminConfig {
            org_slug: slug.clone(),
            org_name: "Phase 4 Analytics E2E Org".to_string(),
            admin_email: owner_email.clone(),
            admin_password: "Phase4Password123!".to_string(),
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

    sqlx::query("UPDATE outbox_events SET published_at = now() WHERE published_at IS NULL")
        .execute(cp_state.storage.pool())
        .await
        .unwrap();

    // 2. Start OutboxWorker & SchedulerWorker (with TelemetryStore for Guarded Rollouts)
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

    let scheduler = SchedulerWorker::new(cp_state.storage.clone(), Duration::from_millis(50))
        .with_telemetry(cp_state.telemetry.clone());

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

    // 4. Start Event Ingest Service
    let ingest_auth = IngestAuthCache::new(Some(snapshots.clone()), Some(cp_state.storage.clone()));
    let ingest_tx = spawn_rollup_flusher(
        cp_state.telemetry.clone(),
        Duration::from_millis(50),
        shutdown_tx.subscribe(),
    );
    let ingest_router = build_ingest_router(IngestState {
        auth: ingest_auth.clone(),
        telemetry: cp_state.telemetry.clone(),
        tx: ingest_tx,
        nats: Some(nats.clone()),
    });
    let ingest_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ingest_addr = ingest_listener.local_addr().unwrap();
    let ingest_base = format!("http://{ingest_addr}");
    let mut ingest_http_rx = shutdown_tx.subscribe();
    tokio::spawn(async move {
        axum::serve(ingest_listener, ingest_router)
            .with_graceful_shutdown(async move {
                let _ = ingest_http_rx.changed().await;
            })
            .await
            .unwrap();
    });

    // Sign in as bootstrap owner and obtain development SDK + ClientSide keys
    let client = login_with_cookie(&cp_base, &owner_email, "Phase4Password123!").await;

    let envs_resp: Value = client
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
    let client_side_id = dev_env["clientSideId"].as_str().unwrap().to_string();

    let rot_resp: Value = client
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
    let sdk_key = rot_resp["plaintextKey"].as_str().unwrap().to_string();
    ingest_auth.refresh().await;

    // Create test flags
    let flag_resp = client
        .post(format!("{cp_base}/api/v1/projects/default/flags"))
        .json(&json!({
            "key": "checkout-exp-flag",
            "name": "Checkout Experiment Flag",
            "kind": "boolean",
            "variations": [
                { "value": false, "name": "Control" },
                { "value": true, "name": "Treatment" }
            ],
            "onVariationIndex": 1,
            "offVariationIndex": 0,
            "clientSideAvailability": { "usingEnvironmentId": true, "usingMobileKey": true }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(flag_resp.status(), 201);

    // Turn flag ON so Guarded Rollout can auto-rollback it later
    let patch_on = client
        .patch(format!(
            "{cp_base}/api/v1/projects/default/flags/checkout-exp-flag"
        ))
        .json(&json!({
            "environmentKey": "development",
            "instructions": [{ "kind": "turnFlagOn" }],
            "comment": "Enable flag for Phase 4 experiment and guarded rollout"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(patch_on.status(), 200);

    // =========================================================================
    // 1. Metrics, Funnels, Mutual Exclusion Layer & Bayesian A/B Experiment
    // =========================================================================
    let m1 = client
        .post(format!("{cp_base}/api/v1/projects/default/metrics"))
        .json(&json!({
            "key": "checkout-conv",
            "name": "Checkout Conversion",
            "kind": "conversion",
            "eventKey": "checkout_completed",
            "successDirection": "higher_is_better",
            "unit": "%"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(m1.status(), 201);

    let m2 = client
        .post(format!("{cp_base}/api/v1/projects/default/metrics"))
        .json(&json!({
            "key": "checkout-err",
            "name": "Checkout Error Rate",
            "kind": "error_rate",
            "eventKey": "error",
            "successDirection": "lower_is_better",
            "unit": "%"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(m2.status(), 201);

    let mg = client
        .post(format!("{cp_base}/api/v1/projects/default/metric-groups"))
        .json(&json!({
            "key": "checkout-funnel",
            "name": "Checkout Funnel",
            "kind": "funnel",
            "metricKeys": ["checkout-conv", "checkout-err"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(mg.status(), 201);

    let layer_resp = client
        .post(format!("{cp_base}/api/v1/projects/default/layers"))
        .json(&json!({
            "key": "checkout-surface",
            "name": "Checkout Surface Layer",
            "description": "Isolate concurrent checkout tests"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(layer_resp.status(), 201);

    let exp_resp = client
        .post(format!(
            "{cp_base}/api/v1/projects/default/environments/development/experiments"
        ))
        .json(&json!({
            "key": "exp-checkout-lift",
            "name": "Checkout Conversion Lift",
            "hypothesis": "One-click increases checkout conversion",
            "flagKey": "checkout-exp-flag",
            "mode": "ab_test",
            "engine": "bayesian",
            "controlVariationIndex": 0,
            "primaryMetricKey": "checkout-conv",
            "secondaryMetricKeys": ["checkout-err"],
            "metricGroupKey": "checkout-funnel",
            "layerKey": "checkout-surface",
            "layerReservationBuckets": 25000,
            "allocations": [
                { "variationIndex": 0, "weight": 50000 },
                { "variationIndex": 1, "weight": 50000 }
            ],
            "alpha": 0.05
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(exp_resp.status(), 201);
    let exp_body: Value = exp_resp.json().await.unwrap();
    assert_eq!(exp_body["layerBucketStart"], 0);
    assert_eq!(exp_body["layerBucketEnd"], 25000);

    let start_resp = client
        .post(format!(
            "{cp_base}/api/v1/projects/default/environments/development/experiments/exp-checkout-lift/start"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(start_resp.status(), 200);

    // Ingest 240 balanced exposures + conversion & error custom events via event-ingest
    let now_ms = Utc::now().timestamp_millis() - 5_000;
    let mut bulk_events: Vec<Value> = Vec::new();
    for i in 0..120 {
        let ctrl_user = format!("ctrl-u-{i}");
        bulk_events.push(json!({
            "kind": "feature",
            "creationDate": now_ms,
            "key": "checkout-exp-flag",
            "variation": 0,
            "value": false,
            "version": 2,
            "contextKeys": { "user": ctrl_user }
        }));
        if i < 12 {
            bulk_events.push(json!({
                "kind": "custom",
                "creationDate": now_ms + 500,
                "key": "checkout_completed",
                "contextKeys": { "user": ctrl_user },
                "metricValue": 1.0
            }));
        }
        if i == 0 {
            bulk_events.push(json!({
                "kind": "custom",
                "creationDate": now_ms + 600,
                "key": "error",
                "contextKeys": { "user": ctrl_user },
                "metricValue": 1.0
            }));
        }

        let trt_user = format!("trt-u-{i}");
        bulk_events.push(json!({
            "kind": "feature",
            "creationDate": now_ms,
            "key": "checkout-exp-flag",
            "variation": 1,
            "value": true,
            "version": 2,
            "contextKeys": { "user": trt_user }
        }));
        if i < 54 {
            bulk_events.push(json!({
                "kind": "custom",
                "creationDate": now_ms + 500,
                "key": "checkout_completed",
                "contextKeys": { "user": trt_user },
                "metricValue": 1.0
            }));
        }
        if i < 42 {
            // Heavy error spike in Treatment (35% vs 0.8%) to trigger Guarded Rollout rollback!
            bulk_events.push(json!({
                "kind": "custom",
                "creationDate": now_ms + 600,
                "key": "error",
                "contextKeys": { "user": trt_user },
                "metricValue": 1.0
            }));
        }
    }

    let ingest_client = reqwest::Client::new();
    let ing_resp = ingest_client
        .post(format!("{ingest_base}/events/v1/bulk"))
        .header("Authorization", &sdk_key)
        .json(&bulk_events)
        .send()
        .await
        .unwrap();
    assert_eq!(ing_resp.status(), 202);

    // Query Experiment Results
    let results: Value = client
        .get(format!(
            "{cp_base}/api/v1/projects/default/environments/development/experiments/exp-checkout-lift/results"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(results["totalExposures"], 240);
    assert_eq!(results["srm"]["srmDetected"], false);
    let trt = results["primary"]["variations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["variationIndex"] == 1)
        .unwrap();
    assert_eq!(trt["winner"], true);
    assert_eq!(trt["statisticallySignificant"], true);
    assert!(trt["probabilityToBeatControl"].as_f64().unwrap() > 0.99);
    assert!(trt["pValue"].as_f64().unwrap() < 0.001);

    // =========================================================================
    // 2. Guarded Rollout Automated Rollback via SchedulerWorker
    // =========================================================================
    let gr_resp = client
        .post(format!(
            "{cp_base}/api/v1/projects/default/environments/development/guarded-rollouts"
        ))
        .json(&json!({
            "flagKey": "checkout-exp-flag",
            "name": "Checkout Error Guardrail",
            "controlVariationIndex": 0,
            "treatmentVariationIndex": 1,
            "currentTrafficPct": 50,
            "autoRollback": true,
            "guardrails": [
                {
                    "metricKey": "checkout-err",
                    "maxRegressionPct": 5.0,
                    "minSampleSize": 20,
                    "alpha": 0.05
                }
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(gr_resp.status(), 201);

    // Drive background SchedulerWorker guardrail cycle
    let processed = scheduler.process_due_once().await.expect("scheduler cycle");
    assert!(
        processed >= 1,
        "scheduler must evaluate monitoring guarded rollout"
    );

    // Verify flag was automatically turned OFF and rollout marked rolled_back
    let flag_after: Value = client
        .get(format!(
            "{cp_base}/api/v1/projects/default/flags/checkout-exp-flag"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        flag_after["environments"]["development"]["on"], false,
        "Guarded Rollout must auto-turn flag OFF on error-rate regression"
    );

    let gr_list: Value = client
        .get(format!(
            "{cp_base}/api/v1/projects/default/environments/development/guarded-rollouts"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(gr_list["items"][0]["status"], "rolled_back");

    // =========================================================================
    // 3. Feature Monitoring (Per-Variation Exposures & Error Rates)
    // =========================================================================
    let mon: Value = client
        .get(format!(
            "{cp_base}/api/v1/projects/default/flags/checkout-exp-flag/environments/development/monitoring?hours=24"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mon_vars = mon["variations"].as_array().unwrap();
    assert_eq!(mon_vars.len(), 2);
    assert_eq!(mon_vars[1]["exposures"], 120);
    assert!(mon_vars[1]["errorRate"].as_f64().unwrap() > 0.30);

    // =========================================================================
    // 4. 2/4/6-Stage Migration Flag + `migration_op` Telemetry
    // =========================================================================
    let mig_put = client
        .put(format!(
            "{cp_base}/api/v1/projects/default/flags/checkout-exp-flag/environments/development/migration"
        ))
        .json(&json!({
            "template": "six_stage",
            "currentStage": "shadow",
            "checkRatio": 1,
            "readExecutionOrder": "parallel"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(mig_put.status(), 200);

    let mut mig_events = Vec::new();
    for i in 0..10 {
        mig_events.push(json!({
            "kind": "migration_op",
            "creationDate": now_ms,
            "operation": "read",
            "key": "checkout-exp-flag",
            "stage": "shadow",
            "oldInvoked": true,
            "newInvoked": true,
            "consistent": i < 9,
            "oldLatencyMs": 14.0,
            "newLatencyMs": 8.5,
            "oldError": false,
            "newError": false
        }));
    }
    let mig_ing = ingest_client
        .post(format!("{ingest_base}/events/v1/bulk"))
        .header("Authorization", &sdk_key)
        .json(&mig_events)
        .send()
        .await
        .unwrap();
    assert_eq!(mig_ing.status(), 202);

    let mig_tel: Value = client
        .get(format!(
            "{cp_base}/api/v1/projects/default/flags/checkout-exp-flag/environments/development/migration/telemetry?hours=24"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let stage_row = &mig_tel["stages"][0];
    assert_eq!(stage_row["stage"], "shadow");
    assert_eq!(stage_row["oldInvocations"], 10);
    assert_eq!(stage_row["newInvocations"], 10);
    assert_eq!(stage_row["consistencyMatches"], 9);
    assert_eq!(stage_row["consistencyMismatches"], 1);
    assert!((stage_row["consistencyRatePct"].as_f64().unwrap() - 90.0).abs() < 1e-4);

    // =========================================================================
    // 5. Synced Segments Reverse-ETL Push -> Big Segment -> DB-Free Delivery Edge
    // =========================================================================
    let seg_resp = client
        .post(format!(
            "{cp_base}/api/v1/projects/default/environments/development/segments"
        ))
        .json(&json!({
            "key": "warehouse-vips",
            "name": "Warehouse VIPs",
            "kind": "big",
            "unbounded": true,
            "unboundedContextKind": "user"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(seg_resp.status(), 201);

    let sync_create: Value = client
        .post(format!(
            "{cp_base}/api/v1/projects/default/environments/development/synced-segments"
        ))
        .json(&json!({
            "segmentKey": "warehouse-vips",
            "provider": "hightouch",
            "externalCohortId": "cohort_vip_2026",
            "contextKind": "user"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let ingest_path = sync_create["ingestPath"].as_str().unwrap().to_string();

    // Reverse-ETL unauthenticated token push (adds vip-synced-42)
    let push_resp = reqwest::Client::new()
        .post(format!("{cp_base}{ingest_path}"))
        .json(&json!({ "addKeys": ["vip-synced-42", "vip-synced-99"], "removeKeys": [] }))
        .send()
        .await
        .unwrap();
    assert_eq!(push_resp.status(), 200);
    let push_body: Value = push_resp.json().await.unwrap();
    assert_eq!(push_body["includedCount"], 2);

    // Create flag targeting warehouse-vips and verify via DB-free Delivery Edge
    let sf_resp = client
        .post(format!("{cp_base}/api/v1/projects/default/flags"))
        .json(&json!({
            "key": "synced-vip-flag",
            "name": "Synced VIP Flag",
            "kind": "boolean",
            "variations": [
                { "value": true, "name": "VIP" },
                { "value": false, "name": "Standard" }
            ],
            "onVariationIndex": 1,
            "offVariationIndex": 1,
            "clientSideAvailability": { "usingEnvironmentId": true, "usingMobileKey": true }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(sf_resp.status(), 201);
    let sf_json: Value = sf_resp.json().await.unwrap();
    let vip_var_id = sf_json["variations"][0]["id"].as_str().unwrap();
    let std_var_id = sf_json["variations"][1]["id"].as_str().unwrap();

    let patch_sf = client
        .patch(format!(
            "{cp_base}/api/v1/projects/default/flags/synced-vip-flag"
        ))
        .json(&json!({
            "environmentKey": "development",
            "instructions": [
                { "kind": "turnFlagOn" },
                { "kind": "updateFallthrough", "variationId": std_var_id },
                {
                    "kind": "addRule",
                    "description": "Synced VIP match",
                    "clauses": [{
                        "contextKind": "user",
                        "attribute": "key",
                        "op": "segmentMatch",
                        "values": ["warehouse-vips"],
                        "negate": false
                    }],
                    "variationId": vip_var_id
                }
            ],
            "comment": "Target Hightouch synced Big Segment"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(patch_sf.status(), 200);

    // Wait deterministically for OutboxWorker + NATS to sync snapshot + Big Segment to MinIO & Delivery Edge
    let vip_ctx_b64 = URL_SAFE_NO_PAD.encode(r#"{"kind":"user","key":"vip-synced-42"}"#);
    let sync_deadline = std::time::Instant::now() + Duration::from_secs(5);
    let eval_client = reqwest::Client::new();
    let vip_eval: Value = loop {
        let candidate: Value = eval_client
            .get(format!(
                "{edge_base}/sdk/v1/evalx/{client_side_id}/contexts/{vip_ctx_b64}"
            ))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if candidate["synced-vip-flag"]["value"] == true {
            break candidate;
        }
        assert!(
            std::time::Instant::now() < sync_deadline,
            "synced-vip-flag Big Segment snapshot did not reach delivery-edge within 5s, last: {candidate}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert_eq!(vip_eval["synced-vip-flag"]["value"], true);

    let std_ctx_b64 = URL_SAFE_NO_PAD.encode(r#"{"kind":"user","key":"normal-user"}"#);
    let std_eval: Value = reqwest::Client::new()
        .get(format!(
            "{edge_base}/sdk/v1/evalx/{client_side_id}/contexts/{std_ctx_b64}"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(std_eval["synced-vip-flag"]["value"], false);

    // =========================================================================
    // 6. Real-Time Data Export Sink (MinIO Zstd NDJSON) & Engineering Insights
    // =========================================================================
    let sink_resp: Value = client
        .post(format!("{cp_base}/api/v1/data-export/sinks"))
        .json(&json!({
            "name": "Phase 4 MinIO Data Lake",
            "kind": "s3_parquet",
            "destination": "s3://skotos-snapshots/exports/phase4"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let sink_id = sink_resp["id"].as_str().unwrap();

    let flush_resp: Value = client
        .post(format!(
            "{cp_base}/api/v1/data-export/sinks/{sink_id}/flush"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(flush_resp["eventsExported"].as_i64().unwrap() >= 1);
    assert!(flush_resp["location"]
        .as_str()
        .unwrap()
        .contains("exports/"));

    let insights: Value = client
        .get(format!(
            "{cp_base}/api/v1/projects/default/insights?days=30"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(insights["flagDeploymentsCount"].as_i64().unwrap() >= 2);
    assert!(insights["rolledBackGuardrails"].as_i64().unwrap() >= 1);
    assert!(insights["activeExperiments"].as_i64().unwrap() >= 1);

    // =========================================================================
    // 7. ClickHouse HTTP Telemetry Driver Verification (Mock ClickHouse HTTP)
    // =========================================================================
    let recorded_queries: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let ch_router = Router::new()
        .route(
            "/",
            post(
                |AxumState(log): AxumState<Arc<Mutex<Vec<String>>>>, body: String| async move {
                    log.lock().unwrap().push(body.clone());
                    if body.contains("FORMAT JSON") && body.contains("FROM flag_eval_rollups") {
                        return json!({
                            "data": [{
                                "variation_index": 0,
                                "total_count": "42",
                                "max_eval_at": "2026-11-27T12:00:00Z",
                                "kinds": ["user"]
                            }]
                        })
                        .to_string();
                    }
                    if body.contains("FORMAT JSON") && body.contains("FROM experiment_exposures") {
                        return json!({
                            "data": [{
                                "variation_index": 1,
                                "exposures": "100",
                                "conversions": "35",
                                "sum_val": 35.0,
                                "sum_sq": 35.0
                            }]
                        })
                        .to_string();
                    }
                    "OK".to_string()
                },
            ),
        )
        .with_state(recorded_queries.clone());

    let ch_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ch_addr = ch_listener.local_addr().unwrap();
    let mut ch_rx = shutdown_tx.subscribe();
    tokio::spawn(async move {
        axum::serve(ch_listener, ch_router)
            .with_graceful_shutdown(async move {
                let _ = ch_rx.changed().await;
            })
            .await
            .unwrap();
    });

    let ch_store = ClickHouseTelemetryStore::new(ClickHouseConfig {
        url: format!("http://{ch_addr}"),
        database: "skotos_test".to_string(),
        username: None,
        password: None,
    });
    ch_store.ensure_schema().await.expect("CH DDL");
    let org_id = Uuid::now_v7();
    let env_id = Uuid::now_v7();
    ch_store
        .record_exposures(&[ExposureRecord {
            org_id,
            env_id,
            flag_key: "f1".to_string(),
            variation_index: 1,
            context_kind: "user".to_string(),
            context_key: "u-1".to_string(),
            exposed_at: Utc::now(),
        }])
        .await
        .expect("CH exposures");
    ch_store
        .record_custom_events(&[CustomEventRecord {
            org_id,
            env_id,
            event_key: "checkout_completed".to_string(),
            context_kind: "user".to_string(),
            context_key: "u-1".to_string(),
            metric_value: 1.0,
            occurred_at: Utc::now(),
        }])
        .await
        .expect("CH custom events");
    ch_store
        .record_migration_ops(&[MigrationOpRecord {
            org_id,
            env_id,
            flag_key: "f1".to_string(),
            stage: "shadow".to_string(),
            operation: "read".to_string(),
            old_invoked: true,
            new_invoked: true,
            consistent: Some(true),
            old_latency_ms: 10.0,
            new_latency_ms: 8.0,
            old_error: false,
            new_error: false,
            occurred_at: Utc::now(),
        }])
        .await
        .expect("CH migration ops");

    let ch_usage = ch_store
        .query_flag_usage(org_id, env_id, "f1", 30)
        .await
        .expect("CH usage query");
    assert_eq!(ch_usage.total_evaluations, 42);

    let dummy_metric = MetricDefinition {
        id: Uuid::now_v7(),
        org_id,
        project_id: Uuid::now_v7(),
        key: "checkout-conv".to_string(),
        name: "Checkout Conv".to_string(),
        description: String::new(),
        kind: MetricKind::Conversion,
        event_key: "checkout_completed".to_string(),
        success_direction: MetricSuccessDirection::HigherIsBetter,
        aggregation: MetricAggregation::Average,
        unit: "%".to_string(),
        randomization_unit: "user".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let ch_stats = ch_store
        .query_sufficient_stats(
            org_id,
            env_id,
            "f1",
            &dummy_metric,
            Utc::now() - chrono::Duration::days(1),
            Utc::now(),
        )
        .await
        .expect("CH stats query");
    assert_eq!(ch_stats.len(), 1);
    assert_eq!(ch_stats[0].exposures, 100);
    assert_eq!(ch_stats[0].conversions, 35);

    let queries = recorded_queries.lock().unwrap();
    assert!(queries
        .iter()
        .any(|q| q.contains("CODEC(Delta, ZSTD(3))") && q.contains("experiment_exposures")));

    let _ = shutdown_tx.send(true);
}
