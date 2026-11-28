use std::time::Duration;

use chrono::Utc;
use serde_json::Value;
use skotos_domain::{FlagPatchInstruction, ScheduledChangeStatus};
use skotos_storage::{AuditActor, ClaimedOutboxEvent, Storage};
use tokio::sync::watch;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::nats::NatsFanout;
use crate::object_store::SnapshotStore;
use crate::protocol::{
    compile_environment_snapshot, precompress_snapshot, AuthIndex, AuthRegistryEvent,
    DeliveryEvent, EnvironmentSnapshot, FanoutError,
};
use crate::webhooks::dispatch_webhooks_for_event;

#[derive(Debug, Clone)]
pub struct OutboxWorkerConfig {
    pub poll_interval: Duration,
    pub batch_size: i64,
    pub lease_seconds: i64,
}

impl Default for OutboxWorkerConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(50),
            batch_size: 50,
            lease_seconds: 30,
        }
    }
}

/// Claims committed PostgreSQL `outbox_events`, compiles & pre-compresses R2/S3 snapshots,
/// publishes incremental NATS JetStream events, syncs the DB-free auth registry, and dispatches
/// signed outbound webhooks.
#[derive(Clone)]
pub struct OutboxWorker {
    storage: Storage,
    snapshots: SnapshotStore,
    nats: Option<NatsFanout>,
    http: reqwest::Client,
    config: OutboxWorkerConfig,
}

impl OutboxWorker {
    pub fn new(
        storage: Storage,
        snapshots: SnapshotStore,
        nats: Option<NatsFanout>,
        config: OutboxWorkerConfig,
    ) -> Self {
        Self {
            storage,
            snapshots,
            nats,
            http: reqwest::Client::new(),
            config,
        }
    }

    /// Cold-start warmup: compiles snapshots for every existing environment in PostgreSQL,
    /// writes `snapshots/{env_id}/latest.json{.gz,.zst}` and `auth/index.json` to R2/S3/MinIO,
    /// and notifies connected `delivery-edge` nodes via NATS.
    pub async fn sync_all_environments(&self) -> Result<usize, FanoutError> {
        let auth_records = self.storage.list_all_environment_auth_records().await?;
        let count = auth_records.len();

        for rec in &auth_records {
            let bundle = self
                .storage
                .load_environment_compile_bundle_by_id(rec.env_id)
                .await?;
            let snapshot = compile_environment_snapshot(&bundle)?;
            let pre = precompress_snapshot(&snapshot)?;
            self.snapshots.put_snapshot(&pre).await?;
            self.snapshots.put_auth_record(rec).await?;
            if let Some(nats) = &self.nats {
                let _ = nats
                    .publish_auth_event(&AuthRegistryEvent::Upsert {
                        record: rec.clone(),
                    })
                    .await;
            }
        }

        let index = AuthIndex {
            generated_at: Utc::now(),
            environments: auth_records,
        };
        self.snapshots.put_auth_index(&index).await?;
        info!(
            environments = count,
            "synced environment snapshots and auth index to object storage"
        );
        Ok(count)
    }

    /// Processes a single batch of claimed outbox events. Returns the number of events processed.
    pub async fn process_once(&self) -> Result<usize, FanoutError> {
        let claimed = self
            .storage
            .claim_outbox_events(self.config.batch_size, self.config.lease_seconds)
            .await?;
        if claimed.is_empty() {
            return Ok(0);
        }

        let count = claimed.len();
        for item in claimed {
            match self.handle_claimed_event(&item).await {
                Ok(()) => {
                    self.storage.mark_outbox_published(item.event.id).await?;
                }
                Err(err) => {
                    let msg = err.to_string();
                    warn!(
                        event_id = %item.event.id,
                        event_type = %item.event.event_type,
                        attempts = item.attempts,
                        error = %msg,
                        "outbox event publish failed; scheduling retry with backoff"
                    );
                    let _ = self
                        .storage
                        .mark_outbox_failed(item.event.id, item.attempts, &msg)
                        .await;
                }
            }
        }
        Ok(count)
    }

    async fn handle_claimed_event(&self, item: &ClaimedOutboxEvent) -> Result<(), FanoutError> {
        let ev = &item.event;
        let mut env_key_for_webhook: Option<String> = None;

        if let Some(env_id) = ev.env_id {
            // 1. Keep auth registry synced on environment creation or key rotation
            if matches!(
                ev.event_type.as_str(),
                "environment.created" | "environment.key_rotated"
            ) {
                self.sync_auth_for_env(env_id).await?;
            }

            // 2. Compile & upload pre-compressed environment snapshot to R2/S3/MinIO
            let bundle = self
                .storage
                .load_environment_compile_bundle_by_id(env_id)
                .await?;
            env_key_for_webhook = Some(bundle.environment.key.clone());
            let snapshot = compile_environment_snapshot(&bundle)?;
            let pre = precompress_snapshot(&snapshot)?;
            self.snapshots.put_snapshot(&pre).await?;

            // 2b. Sync Zstd-compressed Big Segment membership snapshot when mutated
            if ev.event_type == "big_segment.updated" {
                if let Some(seg_key) = ev.payload.get("segmentKey").and_then(Value::as_str) {
                    let ck = ev
                        .payload
                        .get("contextKind")
                        .and_then(Value::as_str)
                        .unwrap_or("user");
                    if let Some(bs) = self
                        .storage
                        .build_big_segment_snapshot(env_id, seg_key, ck)
                        .await?
                    {
                        self.snapshots.put_big_segment_snapshot(&bs).await?;
                    }
                }
            }

            // 3. Build incremental DeliveryEvent and publish to NATS JetStream
            let delivery_event =
                build_delivery_event(&ev.event_type, &ev.payload, &snapshot, &pre.etag)?;
            if let Some(nats) = &self.nats {
                nats.publish_delivery_event(env_id, &delivery_event).await?;
            }
            debug!(
                env_id = %env_id,
                revision = snapshot.revision,
                event_type = %ev.event_type,
                "published environment snapshot and delivery event"
            );
        }

        // 4. Dispatch matching HMAC-signed outbound webhooks asynchronously so slow
        // customer endpoints never block sub-200ms real-time SDK flag delivery.
        let wh_storage = self.storage.clone();
        let wh_http = self.http.clone();
        let wh_org_id = ev.org_id;
        let wh_event_id = ev.id;
        let wh_event_type = ev.event_type.clone();
        let wh_payload = ev.payload.clone();
        tokio::spawn(async move {
            dispatch_webhooks_for_event(
                &wh_storage,
                &wh_http,
                wh_org_id,
                wh_event_id,
                &wh_event_type,
                env_key_for_webhook.as_deref(),
                &wh_payload,
            )
            .await;
        });

        Ok(())
    }

    async fn sync_auth_for_env(&self, env_id: Uuid) -> Result<(), FanoutError> {
        let record = self.storage.load_environment_auth_record(env_id).await?;
        self.snapshots.put_auth_record(&record).await?;

        let all = self.storage.list_all_environment_auth_records().await?;
        let index = AuthIndex {
            generated_at: Utc::now(),
            environments: all,
        };
        self.snapshots.put_auth_index(&index).await?;

        if let Some(nats) = &self.nats {
            nats.publish_auth_event(&AuthRegistryEvent::Upsert { record })
                .await?;
        }
        Ok(())
    }

    /// Runs the outbox polling loop until `shutdown` transitions to `true`.
    pub async fn run_until_shutdown(self, mut shutdown: watch::Receiver<bool>) {
        info!("starting Skotos Outbox & Snapshot Fanout worker");
        loop {
            if *shutdown.borrow() {
                break;
            }
            match self.process_once().await {
                Ok(n) if n > 0 => {
                    // Drain immediately if more events might be queued
                    continue;
                }
                Ok(_) => {}
                Err(err) => {
                    warn!(error = %err, "outbox worker poll error");
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(self.config.poll_interval) => {}
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        break;
                    }
                }
            }
        }
        info!("Skotos Outbox & Snapshot Fanout worker stopped");
    }
}

fn build_delivery_event(
    event_type: &str,
    payload: &Value,
    snapshot: &EnvironmentSnapshot,
    etag: &str,
) -> Result<DeliveryEvent, FanoutError> {
    let client_side_affected = payload
        .get("clientSideAffected")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    match event_type {
        "flag.created" | "flag.updated" | "flag.restored" => {
            if let Some(flag_key) = payload.get("flagKey").and_then(Value::as_str) {
                if let Some(eval_flag) = snapshot.flags.get(flag_key) {
                    return Ok(DeliveryEvent::Patch {
                        revision: snapshot.revision,
                        etag: etag.to_string(),
                        path: format!("/flags/{flag_key}"),
                        version: eval_flag.version,
                        data: serde_json::to_value(eval_flag)?,
                        client_side_affected: client_side_affected
                            || eval_flag.client_side_availability.any_enabled(),
                    });
                } else {
                    // Flag was archived — emit delete tombstone so SDKs drop archived flags cleanly
                    let ver = payload
                        .get("version")
                        .and_then(Value::as_i64)
                        .unwrap_or(snapshot.revision);
                    return Ok(DeliveryEvent::Delete {
                        revision: snapshot.revision,
                        etag: etag.to_string(),
                        path: format!("/flags/{flag_key}"),
                        version: ver,
                        client_side_affected,
                    });
                }
            }
        }
        "flag.deleted" => {
            if let Some(flag_key) = payload.get("flagKey").and_then(Value::as_str) {
                let ver = payload
                    .get("version")
                    .and_then(Value::as_i64)
                    .unwrap_or(snapshot.revision);
                return Ok(DeliveryEvent::Delete {
                    revision: snapshot.revision,
                    etag: etag.to_string(),
                    path: format!("/flags/{flag_key}"),
                    version: ver,
                    client_side_affected,
                });
            }
        }
        "segment.created" | "segment.updated" => {
            if let Some(seg_key) = payload.get("segmentKey").and_then(Value::as_str) {
                if let Some(eval_seg) = snapshot.segments.get(seg_key) {
                    return Ok(DeliveryEvent::Patch {
                        revision: snapshot.revision,
                        etag: etag.to_string(),
                        path: format!("/segments/{seg_key}"),
                        version: eval_seg.version,
                        data: serde_json::to_value(eval_seg)?,
                        client_side_affected: true,
                    });
                }
            }
        }
        "segment.deleted" => {
            if let Some(seg_key) = payload.get("segmentKey").and_then(Value::as_str) {
                let ver = payload
                    .get("version")
                    .and_then(Value::as_i64)
                    .unwrap_or(snapshot.revision);
                return Ok(DeliveryEvent::Delete {
                    revision: snapshot.revision,
                    etag: etag.to_string(),
                    path: format!("/segments/{seg_key}"),
                    version: ver,
                    client_side_affected: true,
                });
            }
        }
        _ => {}
    }

    Ok(DeliveryEvent::Put {
        revision: snapshot.revision,
        etag: etag.to_string(),
        client_side_affected,
    })
}

/// Periodically claims due `scheduled_changes` and executes their semantic `FlagPatchInstruction`s.
#[derive(Clone)]
pub struct SchedulerWorker {
    storage: Storage,
    poll_interval: Duration,
}

impl SchedulerWorker {
    pub fn new(storage: Storage, poll_interval: Duration) -> Self {
        Self {
            storage,
            poll_interval,
        }
    }

    pub async fn process_due_once(&self) -> Result<usize, FanoutError> {
        let mut total = 0usize;

        // 1. Due scheduled flag changes
        let due = self.storage.list_due_scheduled_changes(25).await?;
        total += due.len();
        for item in due {
            let instructions: Result<Vec<FlagPatchInstruction>, _> =
                serde_json::from_value(item.change.instructions.clone());
            match instructions {
                Ok(instrs) => {
                    let actor = AuditActor {
                        actor_id: item.change.created_by,
                        actor_type: "scheduler".to_string(),
                        actor_email: None,
                    };
                    let comment = item
                        .change
                        .comment
                        .clone()
                        .unwrap_or_else(|| format!("Scheduled change {}", item.change.id));
                    match self
                        .storage
                        .patch_flag(
                            item.change.org_id,
                            &item.project_key,
                            &item.flag_key,
                            &item.env_key,
                            None,
                            Some(&comment),
                            &instrs,
                            &actor,
                        )
                        .await
                    {
                        Ok(_) => {
                            self.storage
                                .mark_scheduled_change_result(
                                    item.change.id,
                                    ScheduledChangeStatus::Executed,
                                    None,
                                )
                                .await?;
                        }
                        Err(err) => {
                            let msg = err.to_string();
                            self.storage
                                .mark_scheduled_change_result(
                                    item.change.id,
                                    ScheduledChangeStatus::Failed,
                                    Some(&msg),
                                )
                                .await?;
                        }
                    }
                }
                Err(err) => {
                    let msg = format!("invalid scheduled instructions: {err}");
                    self.storage
                        .mark_scheduled_change_result(
                            item.change.id,
                            ScheduledChangeStatus::Failed,
                            Some(&msg),
                        )
                        .await?;
                }
            }
        }

        // 2. Due scheduled segment changes (Phase 3)
        let due_segs = self.storage.claim_due_scheduled_segment_changes(25).await?;
        total += due_segs.len();
        for item in due_segs {
            let actor = AuditActor {
                actor_id: item.change.created_by,
                actor_type: "scheduler".to_string(),
                actor_email: None,
            };
            let comment = item
                .change
                .comment
                .clone()
                .unwrap_or_else(|| format!("Scheduled segment change {}", item.change.id));
            match self
                .storage
                .patch_segment(
                    item.change.org_id,
                    &item.project_key,
                    &item.env_key,
                    &item.segment_key,
                    None,
                    Some(&comment),
                    &item.change.instructions,
                    &actor,
                )
                .await
            {
                Ok(_) => {
                    self.storage
                        .complete_scheduled_segment_change(
                            item.change.id,
                            ScheduledChangeStatus::Executed,
                            None,
                        )
                        .await?;
                }
                Err(err) => {
                    let msg = err.to_string();
                    self.storage
                        .complete_scheduled_segment_change(
                            item.change.id,
                            ScheduledChangeStatus::Failed,
                            Some(&msg),
                        )
                        .await?;
                }
            }
        }

        // 3. Due multi-step flag workflows (Phase 3 progressive rollouts)
        let due_wfs = self.storage.list_due_workflows(25).await?;
        total += due_wfs.len();
        for wf in due_wfs {
            let actor = AuditActor {
                actor_id: wf.created_by,
                actor_type: "workflow".to_string(),
                actor_email: None,
            };
            let _ = self
                .storage
                .advance_flag_workflow(wf.org_id, wf.id, false, &actor)
                .await;
        }

        Ok(total)
    }

    pub async fn run_until_shutdown(self, mut shutdown: watch::Receiver<bool>) {
        info!("starting Skotos Scheduled Changes worker");
        loop {
            if *shutdown.borrow() {
                break;
            }
            if let Err(err) = self.process_due_once().await {
                warn!(error = %err, "scheduler worker poll error");
            }
            tokio::select! {
                _ = tokio::time::sleep(self.poll_interval) => {}
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        break;
                    }
                }
            }
        }
        info!("Skotos Scheduled Changes worker stopped");
    }
}
