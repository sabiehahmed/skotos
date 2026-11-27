use std::collections::{BTreeSet, HashMap};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde_json::Value;
use skotos_domain::Context;
use skotos_eval::{redact_context, RedactionConfig};
use skotos_telemetry::{SdkEvent, SummaryCounter, SummaryFeatureBucket};
use tracing::warn;
use uuid::Uuid;

const MAX_BUFFERED_EVENTS: usize = 5_000;

type SummaryKey = (String, Option<i64>, Option<i32>);
type SummaryValue = (i64, Option<Value>, Option<Value>);

#[derive(Debug)]
struct EventState {
    start_ms: i64,
    /// `(flag_key, version, variation)` -> `(count, value, default)`
    summary: HashMap<SummaryKey, SummaryValue>,
    flag_context_kinds: HashMap<String, BTreeSet<String>>,
    raw_events: Vec<SdkEvent>,
}

impl Default for EventState {
    fn default() -> Self {
        Self {
            start_ms: Utc::now().timestamp_millis(),
            summary: HashMap::new(),
            flag_context_kinds: HashMap::new(),
            raw_events: Vec::new(),
        }
    }
}

/// Bounded in-memory event aggregator that collapses evaluations into 5-second `summary`
/// buckets, redacts private attributes, and flushes gzipped batches to `services/event-ingest`.
#[derive(Clone)]
pub struct EventProcessor {
    state: Arc<Mutex<EventState>>,
    sdk_key: String,
    events_url: Option<String>,
    http: reqwest::Client,
    redaction: RedactionConfig,
}

impl EventProcessor {
    pub fn new(sdk_key: String, events_url: Option<String>, redaction: RedactionConfig) -> Self {
        Self {
            state: Arc::new(Mutex::new(EventState::default())),
            sdk_key,
            events_url,
            http: reqwest::Client::new(),
            redaction,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_evaluation(
        &self,
        flag_key: &str,
        version: Option<i64>,
        variation: Option<usize>,
        value: Value,
        default_val: Value,
        context: &Context,
        track_detailed: bool,
    ) {
        let Ok(mut guard) = self.state.lock() else {
            return;
        };
        let var_i32 = variation.map(|v| v as i32);
        let entry = guard
            .summary
            .entry((flag_key.to_string(), version, var_i32))
            .or_insert_with(|| (0, Some(value.clone()), Some(default_val.clone())));
        entry.0 += 1;

        let kinds_entry = guard
            .flag_context_kinds
            .entry(flag_key.to_string())
            .or_default();
        for (kind, _) in context.kinds() {
            kinds_entry.insert(kind.to_string());
        }

        if track_detailed && guard.raw_events.len() < MAX_BUFFERED_EVENTS {
            let mut context_keys = HashMap::new();
            for (kind, single) in context.kinds() {
                context_keys.insert(kind.to_string(), single.key.clone());
            }
            guard.raw_events.push(SdkEvent::Feature {
                creation_date: Utc::now().timestamp_millis(),
                key: flag_key.to_string(),
                version,
                variation: var_i32,
                value: Some(value),
                default: Some(default_val),
                context_keys,
            });
        }
    }

    pub fn record_identify(&self, context: &Context) {
        let Ok(mut guard) = self.state.lock() else {
            return;
        };
        if guard.raw_events.len() >= MAX_BUFFERED_EVENTS {
            return;
        }
        let redacted = redact_context(context, &self.redaction);
        guard.raw_events.push(SdkEvent::Identify {
            creation_date: Utc::now().timestamp_millis(),
            context: Some(redacted),
        });
    }

    pub fn record_custom(
        &self,
        event_key: &str,
        context: &Context,
        data: Option<Value>,
        metric_value: Option<f64>,
    ) {
        let Ok(mut guard) = self.state.lock() else {
            return;
        };
        if guard.raw_events.len() >= MAX_BUFFERED_EVENTS {
            return;
        }
        let mut context_keys = HashMap::new();
        for (kind, single) in context.kinds() {
            context_keys.insert(kind.to_string(), single.key.clone());
        }
        guard.raw_events.push(SdkEvent::Custom {
            creation_date: Utc::now().timestamp_millis(),
            key: event_key.to_string(),
            context_keys,
            data,
            metric_value,
        });
    }

    /// Drains current buffered summary & raw events into an `SdkEvent` payload batch.
    pub fn drain_batch(&self) -> Vec<SdkEvent> {
        let Ok(mut guard) = self.state.lock() else {
            return Vec::new();
        };
        let end_ms = Utc::now().timestamp_millis();
        let start_ms = guard.start_ms;
        guard.start_ms = end_ms;

        let summary_map = std::mem::take(&mut guard.summary);
        let kinds_map = std::mem::take(&mut guard.flag_context_kinds);
        let mut out = std::mem::take(&mut guard.raw_events);

        if !summary_map.is_empty() {
            let mut features: HashMap<String, SummaryFeatureBucket> = HashMap::new();
            for ((flag_key, version, variation), (count, value, default_val)) in summary_map {
                let bucket =
                    features
                        .entry(flag_key.clone())
                        .or_insert_with(|| SummaryFeatureBucket {
                            default: default_val,
                            context_kinds: kinds_map
                                .get(&flag_key)
                                .map(|s| s.iter().cloned().collect())
                                .unwrap_or_default(),
                            counters: Vec::new(),
                        });
                bucket.counters.push(SummaryCounter {
                    variation,
                    version,
                    value,
                    unknown: version.is_none(),
                    count,
                });
            }
            out.push(SdkEvent::Summary {
                start_date: start_ms,
                end_date: end_ms,
                features,
            });
        }

        out
    }

    /// Flushes buffered events gzip-compressed to `POST {events_url}/events/v1/bulk`.
    pub async fn flush(&self) -> Result<usize, reqwest::Error> {
        let batch = self.drain_batch();
        if batch.is_empty() {
            return Ok(0);
        }
        let Some(base) = &self.events_url else {
            return Ok(batch.len());
        };
        let Ok(json_bytes) = serde_json::to_vec(&batch) else {
            return Ok(0);
        };
        let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
        if gz.write_all(&json_bytes).is_err() {
            return Ok(0);
        }
        let Ok(compressed) = gz.finish() else {
            return Ok(0);
        };

        let url = format!("{}/events/v1/bulk", base.trim_end_matches('/'));
        let payload_id = Uuid::now_v7().to_string();
        let count = batch.len();

        let resp = self
            .http
            .post(&url)
            .timeout(Duration::from_secs(5))
            .header("authorization", &self.sdk_key)
            .header("content-type", "application/json")
            .header("content-encoding", "gzip")
            .header("x-skotos-payload-id", payload_id)
            .body(compressed)
            .send()
            .await?;

        if !resp.status().is_success() {
            warn!(status = %resp.status(), "skotos event-ingest rejected batch");
        }
        Ok(count)
    }
}
