use chrono::Utc;
use serde_json::{json, Value};
use skotos_domain::WebhookIntegrationKind;
use uuid::Uuid;

/// Formats an outbound webhook event payload according to the target integration protocol
/// (Generic JSON, Slack Block Kit, MS Teams Adaptive Card, Datadog Events, PagerDuty v2, Jira/Linear).
pub fn format_integration_payload(
    kind: WebhookIntegrationKind,
    org_id: Uuid,
    event_id: Uuid,
    event_type: &str,
    env_key: Option<&str>,
    payload: &Value,
) -> Value {
    let env_label = env_key.unwrap_or("org");
    let flag_key = payload
        .get("flagKey")
        .or_else(|| payload.get("segmentKey"))
        .and_then(Value::as_str)
        .unwrap_or("resource");
    let summary = format!("[Skotos] {event_type} on '{flag_key}' ({env_label})");

    match kind {
        WebhookIntegrationKind::Generic => json!({
            "id": event_id,
            "eventType": event_type,
            "orgId": org_id,
            "environmentKey": env_key,
            "timestamp": Utc::now(),
            "data": payload,
        }),
        WebhookIntegrationKind::Slack => json!({
            "text": summary,
            "blocks": [
                {
                    "type": "header",
                    "text": { "type": "plain_text", "text": summary }
                },
                {
                    "type": "section",
                    "fields": [
                        { "type": "mrkdwn", "text": format!("*Event:*\n`{event_type}`") },
                        { "type": "mrkdwn", "text": format!("*Environment:*\n`{env_label}`") },
                        { "type": "mrkdwn", "text": format!("*Resource:*\n`{flag_key}`") }
                    ]
                },
                {
                    "type": "context",
                    "elements": [
                        { "type": "mrkdwn", "text": format!("Event ID: `{event_id}`") }
                    ]
                }
            ]
        }),
        WebhookIntegrationKind::Teams => json!({
            "type": "message",
            "attachments": [{
                "contentType": "application/vnd.microsoft.card.adaptive",
                "content": {
                    "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
                    "type": "AdaptiveCard",
                    "version": "1.4",
                    "body": [
                        { "type": "TextBlock", "size": "Medium", "weight": "Bolder", "text": summary },
                        {
                            "type": "FactSet",
                            "facts": [
                                { "title": "Event", "value": event_type },
                                { "title": "Environment", "value": env_label },
                                { "title": "Resource", "value": flag_key }
                            ]
                        }
                    ]
                }
            }]
        }),
        WebhookIntegrationKind::Datadog => json!({
            "title": summary,
            "text": format!("Skotos flag governance event `{event_type}` in `{env_label}` for `{flag_key}`."),
            "alert_type": if event_type.contains("deleted") { "warning" } else { "info" },
            "source_type_name": "skotos",
            "tags": [
                format!("skotos_event:{event_type}"),
                format!("env:{env_label}"),
                format!("flag:{flag_key}")
            ]
        }),
        WebhookIntegrationKind::PagerDuty => json!({
            "event_action": "trigger",
            "dedup_key": format!("skotos-{event_id}"),
            "payload": {
                "summary": summary,
                "severity": "info",
                "source": format!("skotos:{env_label}"),
                "component": flag_key,
                "custom_details": payload
            }
        }),
        WebhookIntegrationKind::Jira | WebhookIntegrationKind::Linear => json!({
            "integration": kind.as_str(),
            "title": summary,
            "body": format!("Automated Skotos change notification: `{event_type}` on `{flag_key}` in `{env_label}`."),
            "eventId": event_id,
            "data": payload
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_slack_datadog_and_pagerduty_payloads() {
        let org_id = Uuid::new_v4();
        let ev_id = Uuid::new_v4();
        let data = json!({ "flagKey": "checkout-v2" });

        let slack = format_integration_payload(
            WebhookIntegrationKind::Slack,
            org_id,
            ev_id,
            "flag.updated",
            Some("production"),
            &data,
        );
        assert!(slack.get("blocks").and_then(Value::as_array).is_some());

        let dd = format_integration_payload(
            WebhookIntegrationKind::Datadog,
            org_id,
            ev_id,
            "flag.updated",
            Some("production"),
            &data,
        );
        assert_eq!(dd["source_type_name"], "skotos");

        let pd = format_integration_payload(
            WebhookIntegrationKind::PagerDuty,
            org_id,
            ev_id,
            "flag.updated",
            Some("production"),
            &data,
        );
        assert_eq!(pd["event_action"], "trigger");
    }
}
