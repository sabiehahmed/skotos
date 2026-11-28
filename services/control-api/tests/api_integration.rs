use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use control_api::{build_router, AppState};
use serde_json::{json, Value};
use skotos_auth::{
    hash_password, ApiTokenVerifier, Authorizer, BootstrapAdminConfig, SelfHostedAuthAdapter,
};
use skotos_domain::{BuiltInRole, PlanTier};
use skotos_storage::{run_migrations, AuditActor, Storage};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use uuid::Uuid;

async fn setup_test_app() -> (axum::Router, Storage, String, Uuid) {
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://skotos:skotos@localhost:5432/skotos".to_string());
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .expect("connect to local Docker Postgres");

    run_migrations(&pool).await.expect("run migrations");
    let storage = Storage::new(pool);

    let unique = Uuid::new_v4().simple().to_string();
    let org_slug = format!("org-{}", &unique[..10]);
    let admin_email = format!("owner-{unique}@skotos.test");
    let admin_password = "TestPassword123!";
    let session_secret = "integration-test-session-secret-at-least-32-bytes".to_string();

    let adapter = SelfHostedAuthAdapter::new(storage.clone(), session_secret).unwrap();
    adapter
        .ensure_bootstrap_admin(&BootstrapAdminConfig {
            org_slug: org_slug.clone(),
            org_name: format!("Test Org {unique}"),
            admin_email: admin_email.clone(),
            admin_password: admin_password.to_string(),
        })
        .await
        .expect("bootstrap admin");

    let org = storage
        .get_organization_by_slug(&org_slug)
        .await
        .unwrap()
        .expect("org exists");

    let state = AppState {
        api_token_verifier: ApiTokenVerifier::new(storage.clone()),
        telemetry: Arc::new(skotos_telemetry::PostgresTelemetryStore::new(
            storage.pool().clone(),
        )),
        snapshots: None,
        nats: None,
        storage: storage.clone(),
        identity_provider: Arc::new(adapter),
        authorizer: Authorizer,
        cookie_secure: false,
    };

    let router = build_router(state, &["http://localhost:5173".to_string()]);
    (router, storage, admin_email, org.id)
}

async fn send_json(
    app: &axum::Router,
    method: Method,
    uri: &str,
    cookie: Option<&str>,
    bearer: Option<&str>,
    if_match: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(c) = cookie {
        builder = builder.header(header::COOKIE, c);
    }
    if let Some(b) = bearer {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {b}"));
    }
    if let Some(im) = if_match {
        builder = builder.header(header::IF_MATCH, im);
    }
    let req = if let Some(b) = body {
        builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&b).unwrap()))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = axum::body::to_bytes(resp.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let val = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()))
    };
    (status, headers, val)
}

#[tokio::test]
async fn full_control_plane_acid_rbac_concurrency_and_tenant_isolation() {
    let (app, storage, admin_email, org_id) = setup_test_app().await;

    // 1. Health & readiness probes
    let (st, _, _) = send_json(&app, Method::GET, "/healthz", None, None, None, None).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _, _) = send_json(&app, Method::GET, "/readyz", None, None, None, None).await;
    assert_eq!(st, StatusCode::OK);

    // 2. Unauthenticated request returns RFC 9457 application/problem+json
    let (st, hdrs, err_body) = send_json(
        &app,
        Method::GET,
        "/api/v1/projects",
        None,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    assert_eq!(
        hdrs.get(header::CONTENT_TYPE).unwrap(),
        "application/problem+json"
    );
    assert_eq!(err_body["code"], "unauthenticated");

    // 3. Login sets HttpOnly session cookie
    let (st, hdrs, me) = send_json(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        None,
        None,
        None,
        Some(json!({ "email": admin_email, "password": "TestPassword123!" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(me["role"], "owner");
    let set_cookie = hdrs.get(header::SET_COOKIE).unwrap().to_str().unwrap();
    assert!(set_cookie.contains("skotos_session="));
    assert!(set_cookie.contains("HttpOnly"));
    let cookie_pair = set_cookie.split(';').next().unwrap().to_string();

    // 4. Create project with seeded development & production environments
    let (st, _, proj_resp) = send_json(
        &app,
        Method::POST,
        "/api/v1/projects",
        Some(&cookie_pair),
        None,
        None,
        Some(json!({ "key": "checkout", "name": "Checkout Service" })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    assert_eq!(proj_resp["environments"].as_array().unwrap().len(), 2);

    // 5. Create boolean flag
    let (st, _, flag_resp) = send_json(
        &app,
        Method::POST,
        "/api/v1/projects/checkout/flags",
        Some(&cookie_pair),
        None,
        None,
        Some(json!({
            "key": "express-pay",
            "name": "Express Pay",
            "kind": "boolean",
            "variations": [
                { "value": true, "name": "Enabled" },
                { "value": false, "name": "Disabled" }
            ]
        })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let on_var_id = flag_resp["variations"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // 6. Protected production environment rejects patch without comment
    let (st, _, err_body) = send_json(
        &app,
        Method::PATCH,
        "/api/v1/projects/checkout/flags/express-pay",
        Some(&cookie_pair),
        None,
        None,
        Some(json!({
            "environmentKey": "production",
            "expectedVersion": 1,
            "instructions": [{ "kind": "turnFlagOn" }]
        })),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert_eq!(err_body["code"], "comment_required");

    // 7. Patch development flag with semantic rule + turnFlagOn
    let (st, _, patch_resp) = send_json(
        &app,
        Method::PATCH,
        "/api/v1/projects/checkout/flags/express-pay",
        Some(&cookie_pair),
        None,
        Some("\"1\""),
        Some(json!({
            "environmentKey": "development",
            "expectedVersion": 1,
            "comment": "Enable for beta users",
            "instructions": [
                { "kind": "turnFlagOn" },
                {
                    "kind": "addRule",
                    "id": "r-beta",
                    "clauses": [{
                        "contextKind": "user",
                        "attribute": "email",
                        "op": "endsWith",
                        "values": ["@skotos.dev"],
                        "negate": false
                    }],
                    "variationId": on_var_id
                }
            ]
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(patch_resp["envConfig"]["version"], 2);
    assert_eq!(patch_resp["envConfig"]["on"], true);

    // 8. Stale expectedVersion returns 409 Conflict with currentVersion
    let (st, _, conflict_body) = send_json(
        &app,
        Method::PATCH,
        "/api/v1/projects/checkout/flags/express-pay",
        Some(&cookie_pair),
        None,
        None,
        Some(json!({
            "environmentKey": "development",
            "expectedVersion": 1,
            "instructions": [{ "kind": "turnFlagOff" }]
        })),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_eq!(conflict_body["code"], "version_conflict");
    assert_eq!(conflict_body["currentVersion"], 2);

    // 9. Disagreeing If-Match and expectedVersion returns 400
    let (st, _, mismatch_body) = send_json(
        &app,
        Method::PATCH,
        "/api/v1/projects/checkout/flags/express-pay",
        Some(&cookie_pair),
        None,
        Some("99"),
        Some(json!({
            "environmentKey": "development",
            "expectedVersion": 2,
            "instructions": [{ "kind": "turnFlagOff" }]
        })),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert_eq!(mismatch_body["code"], "version_header_mismatch");

    // 10. Simulate saved vs draft rules via /simulate
    let (st, _, sim_saved) = send_json(
        &app,
        Method::POST,
        "/api/v1/projects/checkout/flags/express-pay/environments/development/simulate",
        Some(&cookie_pair),
        None,
        None,
        Some(json!({
            "context": { "kind": "user", "key": "u1", "email": "ada@skotos.dev" }
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(sim_saved["value"], true);
    assert_eq!(sim_saved["variationId"], on_var_id);
    assert_eq!(sim_saved["reason"]["kind"], "RULE_MATCH");
    assert_eq!(sim_saved["usedDraft"], false);

    let (st, _, sim_draft) = send_json(
        &app,
        Method::POST,
        "/api/v1/projects/checkout/flags/express-pay/environments/development/simulate",
        Some(&cookie_pair),
        None,
        None,
        Some(json!({
            "context": { "kind": "user", "key": "u1", "email": "ada@skotos.dev" },
            "draftInstructions": [{ "kind": "turnFlagOff" }]
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(sim_draft["value"], false);
    assert_eq!(sim_draft["reason"]["kind"], "OFF");
    assert_eq!(sim_draft["usedDraft"], true);

    // 11. Revision history + monotonic restore (restoring v1 creates v3!)
    let (st, _, revs) = send_json(
        &app,
        Method::GET,
        "/api/v1/projects/checkout/flags/express-pay/environments/development/revisions",
        Some(&cookie_pair),
        None,
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(revs["items"].as_array().unwrap().len(), 2);

    let (st, _, restored) = send_json(
        &app,
        Method::POST,
        "/api/v1/projects/checkout/flags/express-pay/environments/development/restore/1",
        Some(&cookie_pair),
        None,
        None,
        Some(json!({ "comment": "Rollback to v1" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(restored["version"], 3);
    assert_eq!(restored["on"], false);

    // 12. Outbox events & cursor-paginated Audit Log recorded atomically
    let outbox = storage.list_outbox_events(org_id, 50).await.unwrap();
    assert!(outbox.iter().any(|e| e.event_type == "flag.updated"));
    assert!(outbox.iter().any(|e| e.event_type == "flag.restored"));

    let (st, _, audit_page1) = send_json(
        &app,
        Method::GET,
        "/api/v1/audit-log?limit=2",
        Some(&cookie_pair),
        None,
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(audit_page1["items"].as_array().unwrap().len(), 2);
    assert_eq!(audit_page1["hasMore"], true);
    let cursor = audit_page1["nextCursor"].as_str().unwrap();
    let (st, _, audit_page2) = send_json(
        &app,
        Method::GET,
        &format!("/api/v1/audit-log?limit=2&cursor={cursor}"),
        Some(&cookie_pair),
        None,
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(!audit_page2["items"].as_array().unwrap().is_empty());

    // 13. API token RBAC enforcement (Reader token can GET/simulate but gets 403 on PATCH)
    let (st, _, tok_resp) = send_json(
        &app,
        Method::POST,
        "/api/v1/tokens",
        Some(&cookie_pair),
        None,
        None,
        Some(json!({ "name": "CI Reader", "role": "reader" })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let reader_token = tok_resp["plaintextToken"].as_str().unwrap().to_string();
    assert!(reader_token.starts_with("sk_api_"));

    let (st, _, _) = send_json(
        &app,
        Method::GET,
        "/api/v1/projects/checkout/flags",
        None,
        Some(&reader_token),
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (st, _, _) = send_json(
        &app,
        Method::PATCH,
        "/api/v1/projects/checkout/flags/express-pay",
        None,
        Some(&reader_token),
        None,
        Some(json!({
            "environmentKey": "development",
            "instructions": [{ "kind": "turnFlagOn" }]
        })),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // 14. Multi-tenant isolation: another org cannot see `checkout` project
    let other_org = storage
        .ensure_organization(
            &format!("other-{}", &Uuid::new_v4().simple().to_string()[..8]),
            "Other Tenant",
            None,
            PlanTier::Free,
        )
        .await
        .unwrap();
    let other_email = format!("other-{}@skotos.test", Uuid::new_v4().simple());
    let pwd_hash = hash_password("OtherPass123!").unwrap();
    storage
        .create_member(
            other_org.id,
            &other_email,
            "Other Owner",
            BuiltInRole::Owner,
            Some(&pwd_hash),
            None,
            &AuditActor::system(),
        )
        .await
        .unwrap();
    let (st, other_hdrs, _) = send_json(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        None,
        None,
        None,
        Some(json!({ "email": other_email, "password": "OtherPass123!" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let other_cookie = other_hdrs
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let (st, _, _) = send_json(
        &app,
        Method::GET,
        "/api/v1/projects/checkout",
        Some(&other_cookie),
        None,
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // 15. Zero-downtime key rotation
    let (st, _, rot) = send_json(
        &app,
        Method::POST,
        "/api/v1/projects/checkout/environments/production/keys/rotate",
        Some(&cookie_pair),
        None,
        None,
        Some(json!({ "kind": "sdk", "gracePeriodSeconds": 3600 })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(rot["plaintextKey"].as_str().unwrap().starts_with("sk_sdk_"));
}
