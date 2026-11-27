#[cfg(feature = "workers")]
use std::time::Duration;

#[cfg(feature = "workers")]
use chrono::Utc;
use hmac::{Hmac, Mac};
#[cfg(feature = "workers")]
use serde_json::{json, Value};
use sha2::Sha256;
#[cfg(feature = "workers")]
use skotos_storage::Storage;
use subtle::ConstantTimeEq;
#[cfg(feature = "workers")]
use tracing::warn;
#[cfg(feature = "workers")]
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

/// Computes the canonical `X-Skotos-Signature` header (`sha256=<hex>`).
pub fn sign_webhook_payload(secret: &str, body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC key valid");
    mac.update(body);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

/// Constant-time verification of an incoming `X-Skotos-Signature` header.
pub fn verify_webhook_signature(secret: &str, body: &[u8], header_value: &str) -> bool {
    let expected = sign_webhook_payload(secret, body);
    if expected.len() != header_value.len() {
        return false;
    }
    bool::from(expected.as_bytes().ct_eq(header_value.as_bytes()))
}

/// Dispatches signed outbound HTTP POST notifications to all matching enabled webhooks
/// for an organization, retrying transient failures up to 3 attempts and recording each
/// attempt in `webhook_deliveries`.
#[cfg(feature = "workers")]
pub async fn dispatch_webhooks_for_event(
    storage: &Storage,
    http: &reqwest::Client,
    org_id: Uuid,
    event_id: Uuid,
    event_type: &str,
    env_key: Option<&str>,
    payload: &Value,
) {
    let webhooks = match storage.list_active_webhooks_with_secret(org_id).await {
        Ok(list) => list,
        Err(err) => {
            warn!(error = %err, %org_id, "failed to query webhooks for outbox dispatch");
            return;
        }
    };

    if webhooks.is_empty() {
        return;
    }

    let envelope = json!({
        "id": event_id,
        "eventType": event_type,
        "orgId": org_id,
        "environmentKey": env_key,
        "timestamp": Utc::now(),
        "data": payload,
    });
    let Ok(body_bytes) = serde_json::to_vec(&envelope) else {
        return;
    };

    for wh in webhooks {
        let event_matches = wh.webhook.events.is_empty()
            || wh
                .webhook
                .events
                .iter()
                .any(|e| e == "*" || e == event_type);
        if !event_matches {
            continue;
        }
        let env_matches = wh.webhook.env_filter.is_empty()
            || env_key
                .map(|ek| wh.webhook.env_filter.iter().any(|f| f == "*" || f == ek))
                .unwrap_or(true);
        if !env_matches {
            continue;
        }

        let signature = sign_webhook_payload(&wh.secret, &body_bytes);
        let delivery_id = Uuid::now_v7();

        for attempt in 1..=3_i32 {
            let res = http
                .post(&wh.webhook.url)
                .timeout(Duration::from_secs(5))
                .header("content-type", "application/json")
                .header("user-agent", "Skotos-Webhook/0.1")
                .header("x-skotos-event", event_type)
                .header("x-skotos-delivery", delivery_id.to_string())
                .header("x-skotos-signature", &signature)
                .body(body_bytes.clone())
                .send()
                .await;

            match res {
                Ok(resp) => {
                    let code = i32::from(resp.status().as_u16());
                    let ok = resp.status().is_success();
                    let err_msg = if ok {
                        None
                    } else {
                        Some(format!("HTTP {code}"))
                    };
                    let _ = storage
                        .record_webhook_delivery(
                            org_id,
                            wh.webhook.id,
                            event_type,
                            Some(code),
                            attempt,
                            ok,
                            err_msg.as_deref(),
                        )
                        .await;
                    if ok || code < 500 {
                        break;
                    }
                }
                Err(err) => {
                    let msg = err.to_string();
                    let _ = storage
                        .record_webhook_delivery(
                            org_id,
                            wh.webhook.id,
                            event_type,
                            None,
                            attempt,
                            false,
                            Some(&msg),
                        )
                        .await;
                }
            }

            if attempt < 3 {
                tokio::time::sleep(Duration::from_millis(100 * u64::from(attempt as u32))).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_and_verifies_hmac_sha256_webhook_signature() {
        let body = br#"{"eventType":"flag.updated"}"#;
        let sig = sign_webhook_payload("sk_whsec_test_secret", body);
        assert!(sig.starts_with("sha256="));
        assert!(verify_webhook_signature("sk_whsec_test_secret", body, &sig));
        assert!(!verify_webhook_signature("wrong_secret", body, &sig));
        assert!(!verify_webhook_signature(
            "sk_whsec_test_secret",
            b"tampered",
            &sig
        ));
    }
}
