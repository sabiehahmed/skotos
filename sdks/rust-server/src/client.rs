use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use skotos_domain::{
    Context, EvalFlag, EvalSegment, EvaluationDetail, EvaluationErrorKind, EvaluationReason,
};
use skotos_eval::{bool_variation, evaluate, number_variation, string_variation, RedactionConfig};
use tokio::sync::{broadcast, watch, Notify};
use tracing::{debug, warn};

use crate::events::EventProcessor;
use crate::store::{StoreSnapshotEval, TestData, VersionedFeatureStore};

/// Configuration for [`SkotosClient`].
#[derive(Debug, Clone)]
pub struct SkotosConfig {
    pub sdk_key: String,
    pub stream_url: String,
    pub poll_url: String,
    pub events_url: Option<String>,
    pub offline: bool,
    pub use_polling: bool,
    pub poll_interval: Duration,
    pub flush_interval: Duration,
    pub all_attributes_private: bool,
    pub global_private_attributes: Vec<String>,
}

impl SkotosConfig {
    pub fn new(sdk_key: impl Into<String>, delivery_base_url: impl Into<String>) -> Self {
        let base = delivery_base_url.into();
        let trimmed = base.trim_end_matches('/').to_string();
        Self {
            sdk_key: sdk_key.into(),
            stream_url: trimmed.clone(),
            poll_url: trimmed,
            events_url: None,
            offline: false,
            use_polling: false,
            poll_interval: Duration::from_secs(30),
            flush_interval: Duration::from_secs(5),
            all_attributes_private: false,
            global_private_attributes: Vec::new(),
        }
    }

    pub fn with_events_url(mut self, url: impl Into<String>) -> Self {
        self.events_url = Some(url.into());
        self
    }

    pub fn with_polling(mut self, interval: Duration) -> Self {
        self.use_polling = true;
        self.poll_interval = interval;
        self
    }

    pub fn offline(mut self) -> Self {
        self.offline = true;
        self
    }
}

/// Event emitted when a flag changes in the local feature store.
#[derive(Debug, Clone)]
pub struct FlagChangeEvent {
    pub flag_key: String,
}

/// Production Rust Server-Side SDK client reusing `crates/skotos-eval` directly (ADR-0004).
#[derive(Clone)]
pub struct SkotosClient {
    store: Arc<VersionedFeatureStore>,
    events: EventProcessor,
    ready_notify: Arc<Notify>,
    change_tx: broadcast::Sender<FlagChangeEvent>,
    shutdown_tx: Arc<watch::Sender<bool>>,
    offline: bool,
}

impl SkotosClient {
    /// Starts a live SDK client connecting via SSE streaming (or ETag polling if configured).
    pub fn new(config: SkotosConfig) -> Self {
        let store = Arc::new(VersionedFeatureStore::new());
        let redaction = RedactionConfig {
            all_attributes_private: config.all_attributes_private,
            global_private_attributes: config.global_private_attributes.clone(),
        };
        let events = EventProcessor::new(
            config.sdk_key.clone(),
            if config.offline {
                None
            } else {
                config.events_url.clone()
            },
            redaction,
        );
        let ready_notify = Arc::new(Notify::new());
        let (change_tx, _) = broadcast::channel(512);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let client = Self {
            store: store.clone(),
            events: events.clone(),
            ready_notify: ready_notify.clone(),
            change_tx: change_tx.clone(),
            shutdown_tx: Arc::new(shutdown_tx),
            offline: config.offline,
        };

        if !config.offline {
            if config.use_polling {
                tokio::spawn(run_poll_loop(
                    config.clone(),
                    store.clone(),
                    ready_notify.clone(),
                    change_tx.clone(),
                    shutdown_rx.clone(),
                ));
            } else {
                tokio::spawn(run_stream_loop(
                    config.clone(),
                    store.clone(),
                    ready_notify.clone(),
                    change_tx.clone(),
                    shutdown_rx.clone(),
                ));
            }

            if config.events_url.is_some() {
                let flush_interval = config.flush_interval;
                let mut flush_shutdown = shutdown_rx;
                tokio::spawn(async move {
                    let mut ticker = tokio::time::interval(flush_interval);
                    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    loop {
                        tokio::select! {
                            _ = ticker.tick() => {
                                let _ = events.flush().await;
                            }
                            _ = flush_shutdown.changed() => {
                                if *flush_shutdown.borrow() {
                                    let _ = events.flush().await;
                                    break;
                                }
                            }
                        }
                    }
                });
            }
        }

        client
    }

    /// Creates an offline client backed by [`TestData`] with zero network I/O.
    pub fn with_test_data(test_data: &TestData) -> Self {
        let ready_notify = Arc::new(Notify::new());
        let (change_tx, _) = broadcast::channel(128);
        let (shutdown_tx, _) = watch::channel(false);
        Self {
            store: test_data.store(),
            events: EventProcessor::new("offline".to_string(), None, RedactionConfig::default()),
            ready_notify,
            change_tx,
            shutdown_tx: Arc::new(shutdown_tx),
            offline: true,
        }
    }

    pub fn is_initialized(&self) -> bool {
        self.offline || self.store.is_initialized()
    }

    /// Waits up to `timeout` for initial ruleset hydration (`put` or first poll). Never panics or hangs.
    pub async fn wait_for_initialization(&self, timeout: Duration) -> bool {
        if self.is_initialized() {
            return true;
        }
        let _ = tokio::time::timeout(timeout, self.ready_notify.notified()).await;
        self.is_initialized()
    }

    pub fn subscribe_changes(&self) -> broadcast::Receiver<FlagChangeEvent> {
        self.change_tx.subscribe()
    }

    pub fn bool_variation(&self, flag_key: &str, context: &Context, default: bool) -> bool {
        self.bool_variation_detail(flag_key, context, default)
            .value
            .as_bool()
            .unwrap_or(default)
    }

    pub fn bool_variation_detail(
        &self,
        flag_key: &str,
        context: &Context,
        default: bool,
    ) -> EvaluationDetail {
        if !self.is_initialized() {
            return not_ready_detail(Value::Bool(default));
        }
        let snap = StoreSnapshotEval::from_store(&self.store);
        let detail = bool_variation(&snap, flag_key, context, default);
        self.record_eval(flag_key, context, Value::Bool(default), &detail);
        detail
    }

    pub fn string_variation(&self, flag_key: &str, context: &Context, default: &str) -> String {
        self.string_variation_detail(flag_key, context, default)
            .value
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| default.to_string())
    }

    pub fn string_variation_detail(
        &self,
        flag_key: &str,
        context: &Context,
        default: &str,
    ) -> EvaluationDetail {
        if !self.is_initialized() {
            return not_ready_detail(Value::String(default.to_string()));
        }
        let snap = StoreSnapshotEval::from_store(&self.store);
        let detail = string_variation(&snap, flag_key, context, default);
        self.record_eval(
            flag_key,
            context,
            Value::String(default.to_string()),
            &detail,
        );
        detail
    }

    pub fn f64_variation(&self, flag_key: &str, context: &Context, default: f64) -> f64 {
        self.f64_variation_detail(flag_key, context, default)
            .value
            .as_f64()
            .unwrap_or(default)
    }

    pub fn f64_variation_detail(
        &self,
        flag_key: &str,
        context: &Context,
        default: f64,
    ) -> EvaluationDetail {
        if !self.is_initialized() {
            return not_ready_detail(Value::from(default));
        }
        let snap = StoreSnapshotEval::from_store(&self.store);
        let detail = number_variation(&snap, flag_key, context, default);
        self.record_eval(flag_key, context, Value::from(default), &detail);
        detail
    }

    pub fn json_variation(&self, flag_key: &str, context: &Context, default: Value) -> Value {
        self.json_variation_detail(flag_key, context, default).value
    }

    pub fn json_variation_detail(
        &self,
        flag_key: &str,
        context: &Context,
        default: Value,
    ) -> EvaluationDetail {
        if !self.is_initialized() {
            return not_ready_detail(default);
        }
        let snap = StoreSnapshotEval::from_store(&self.store);
        let detail = evaluate(&snap, flag_key, context, default.clone());
        self.record_eval(flag_key, context, default, &detail);
        detail
    }

    pub fn all_flags_state(&self, context: &Context) -> HashMap<String, Value> {
        let mut out = HashMap::new();
        if !self.is_initialized() {
            return out;
        }
        let snap = StoreSnapshotEval::from_store(&self.store);
        for key in self.store.all_flag_keys() {
            let detail = evaluate(&snap, &key, context, Value::Null);
            out.insert(key, detail.value);
        }
        out
    }

    pub fn identify(&self, context: &Context) {
        self.events.record_identify(context);
    }

    pub fn track(
        &self,
        event_key: &str,
        context: &Context,
        data: Option<Value>,
        metric_value: Option<f64>,
    ) {
        self.events
            .record_custom(event_key, context, data, metric_value);
    }

    pub async fn flush(&self) -> usize {
        self.events.flush().await.unwrap_or(0)
    }

    pub async fn close(&self) {
        let _ = self.shutdown_tx.send(true);
        let _ = self.events.flush().await;
    }

    fn record_eval(
        &self,
        flag_key: &str,
        context: &Context,
        default_val: Value,
        detail: &EvaluationDetail,
    ) {
        let meta = self.store.get_flag_cloned(flag_key);
        let (version, track) = meta
            .map(|f| (Some(f.version), f.track_events))
            .unwrap_or((None, false));
        self.events.record_evaluation(
            flag_key,
            version,
            detail.variation_index,
            detail.value.clone(),
            default_val,
            context,
            track,
        );
    }
}

fn not_ready_detail(default: Value) -> EvaluationDetail {
    EvaluationDetail {
        value: default,
        variation_index: None,
        reason: EvaluationReason::Error {
            error_kind: EvaluationErrorKind::ClientNotReady,
        },
    }
}

#[derive(Debug, Deserialize)]
struct PutEnvelope {
    data: PutData,
}

#[derive(Debug, Deserialize)]
struct PutData {
    #[serde(default)]
    flags: HashMap<String, EvalFlag>,
    #[serde(default)]
    segments: HashMap<String, EvalSegment>,
}

#[derive(Debug, Deserialize)]
struct PatchEnvelope {
    path: String,
    data: Value,
}

#[derive(Debug, Deserialize)]
struct DeleteEnvelope {
    path: String,
    version: i64,
}

async fn run_stream_loop(
    config: SkotosConfig,
    store: Arc<VersionedFeatureStore>,
    ready: Arc<Notify>,
    change_tx: broadcast::Sender<FlagChangeEvent>,
    mut shutdown: watch::Receiver<bool>,
) {
    let http = reqwest::Client::new();
    let url = format!("{}/stream/v1/all", config.stream_url.trim_end_matches('/'));
    let mut backoff_ms: u64 = 250;

    loop {
        if *shutdown.borrow() {
            break;
        }
        let resp_res = http
            .get(&url)
            .header("authorization", &config.sdk_key)
            .header("accept", "text/event-stream")
            .send()
            .await;

        match resp_res {
            Ok(resp) => {
                let status = resp.status();
                if status.as_u16() == 401 || status.as_u16() == 403 {
                    warn!(%status, "skotos SDK received terminal auth error; stopping stream retries");
                    break;
                }
                if !status.is_success() {
                    warn!(%status, "skotos SSE stream non-200; backing off");
                } else {
                    backoff_ms = 250;
                    let mut stream = resp.bytes_stream();
                    let mut buf = String::new();
                    let mut current_event = String::new();
                    let mut current_data = String::new();

                    loop {
                        tokio::select! {
                            maybe_chunk = stream.next() => {
                                match maybe_chunk {
                                    Some(Ok(bytes)) => {
                                        buf.push_str(&String::from_utf8_lossy(&bytes));
                                        while let Some(idx) = buf.find('\n') {
                                            let line = buf[..idx].trim_end_matches('\r').to_string();
                                            buf.drain(..=idx);
                                            if line.is_empty() {
                                                if !current_data.is_empty() {
                                                    apply_sse_event(
                                                        &current_event,
                                                        &current_data,
                                                        &store,
                                                        &ready,
                                                        &change_tx,
                                                    );
                                                }
                                                current_event.clear();
                                                current_data.clear();
                                            } else if let Some(ev) = line.strip_prefix("event:") {
                                                current_event = ev.trim().to_string();
                                            } else if let Some(data) = line.strip_prefix("data:") {
                                                if !current_data.is_empty() {
                                                    current_data.push('\n');
                                                }
                                                current_data.push_str(data.trim_start());
                                            }
                                        }
                                    }
                                    Some(Err(err)) => {
                                        debug!(error = %err, "SSE stream read interrupted; reconnecting");
                                        break;
                                    }
                                    None => break,
                                }
                            }
                            _ = shutdown.changed() => {
                                if *shutdown.borrow() {
                                    return;
                                }
                            }
                        }
                    }
                }
            }
            Err(err) => {
                debug!(error = %err, "SSE connect error; backing off");
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(backoff_ms)) => {
                backoff_ms = (backoff_ms * 2).min(30_000);
            }
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    break;
                }
            }
        }
    }
}

fn apply_sse_event(
    event_name: &str,
    data: &str,
    store: &VersionedFeatureStore,
    ready: &Notify,
    change_tx: &broadcast::Sender<FlagChangeEvent>,
) {
    match event_name {
        "put" => {
            if let Ok(env) = serde_json::from_str::<PutEnvelope>(data) {
                let changed = store.put(env.data.flags, env.data.segments);
                ready.notify_waiters();
                for flag_key in changed {
                    let _ = change_tx.send(FlagChangeEvent { flag_key });
                }
            }
        }
        "patch" => {
            if let Ok(env) = serde_json::from_str::<PatchEnvelope>(data) {
                if env.path.starts_with("/flags/") {
                    if let Ok(flag) = serde_json::from_value::<EvalFlag>(env.data) {
                        let key = flag.key.clone();
                        if store.upsert_flag(flag) {
                            let _ = change_tx.send(FlagChangeEvent { flag_key: key });
                        }
                    }
                } else if env.path.starts_with("/segments/") {
                    if let Ok(seg) = serde_json::from_value::<EvalSegment>(env.data) {
                        store.upsert_segment(seg);
                    }
                }
            }
        }
        "delete" => {
            if let Ok(env) = serde_json::from_str::<DeleteEnvelope>(data) {
                if let Some(flag_key) = env.path.strip_prefix("/flags/") {
                    if store.delete_flag(flag_key, env.version) {
                        let _ = change_tx.send(FlagChangeEvent {
                            flag_key: flag_key.to_string(),
                        });
                    }
                } else if let Some(seg_key) = env.path.strip_prefix("/segments/") {
                    store.delete_segment(seg_key, env.version);
                }
            }
        }
        _ => {}
    }
}

#[derive(Debug, Deserialize)]
struct PollSnapshot {
    #[serde(default)]
    flags: HashMap<String, EvalFlag>,
    #[serde(default)]
    segments: HashMap<String, EvalSegment>,
}

async fn run_poll_loop(
    config: SkotosConfig,
    store: Arc<VersionedFeatureStore>,
    ready: Arc<Notify>,
    change_tx: broadcast::Sender<FlagChangeEvent>,
    mut shutdown: watch::Receiver<bool>,
) {
    let http = reqwest::Client::new();
    let url = format!(
        "{}/sdk/v1/latest-all",
        config.poll_url.trim_end_matches('/')
    );
    let mut etag: Option<String> = None;

    loop {
        if *shutdown.borrow() {
            break;
        }
        let mut req = http.get(&url).header("authorization", &config.sdk_key);
        if let Some(tag) = &etag {
            req = req.header("if-none-match", tag);
        }
        if let Ok(resp) = req.send().await {
            let status = resp.status();
            if status.as_u16() == 401 || status.as_u16() == 403 {
                warn!(%status, "skotos polling received terminal auth error; stopping");
                break;
            }
            if status.as_u16() == 200 {
                if let Some(tag) = resp.headers().get("etag").and_then(|v| v.to_str().ok()) {
                    etag = Some(tag.to_string());
                }
                if let Ok(snap) = resp.json::<PollSnapshot>().await {
                    let changed = store.put(snap.flags, snap.segments);
                    ready.notify_waiters();
                    for flag_key in changed {
                        let _ = change_tx.send(FlagChangeEvent { flag_key });
                    }
                }
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(config.poll_interval) => {}
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    break;
                }
            }
        }
    }
}
