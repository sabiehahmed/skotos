use std::collections::BTreeMap;
use std::sync::Arc;

use bytes::Bytes;
use chrono::Utc;
use dashmap::DashMap;
use futures::StreamExt;
use serde_json::json;
use skotos_domain::{EvalFlag, EvalSegment};
use skotos_eval::InMemoryEvalStore;
use skotos_fanout::{
    precompress_snapshot, DeliveryEvent, EnvironmentSnapshot, FanoutError, NatsFanout,
    PrecompressedSnapshot, SnapshotStore,
};
use tokio::sync::{broadcast, watch, RwLock};
use tracing::{debug, warn};
use uuid::Uuid;

/// Bounded per-environment broadcast queue; slow SSE readers lag and disconnect cleanly.
const BROADCAST_CAPACITY: usize = 256;

/// Snapshot state held in RAM for an active environment (pre-compressed wire buffers + parsed eval store).
pub struct CachedEnvData {
    pub snapshot: EnvironmentSnapshot,
    pub precompressed: PrecompressedSnapshot,
    pub eval_store: InMemoryEvalStore,
}

impl CachedEnvData {
    pub fn from_snapshot(snapshot: EnvironmentSnapshot) -> Result<Self, FanoutError> {
        let precompressed = precompress_snapshot(&snapshot)?;
        let eval_store = build_eval_store(&snapshot);
        Ok(Self {
            snapshot,
            precompressed,
            eval_store,
        })
    }
}

fn build_eval_store(snapshot: &EnvironmentSnapshot) -> InMemoryEvalStore {
    let mut store = InMemoryEvalStore::new();
    for flag in snapshot.flags.values() {
        store.insert_flag(flag.clone());
    }
    for seg in snapshot.segments.values() {
        store.insert_segment(seg.clone());
    }
    store
}

/// Active environment state: single NATS subscriber fanning out to all local SSE connections.
pub struct EnvChannel {
    pub data: RwLock<Arc<CachedEnvData>>,
    pub server_tx: broadcast::Sender<Bytes>,
    pub client_tx: broadcast::Sender<Bytes>,
}

/// High-density in-memory environment ruleset cache with 1 NATS subscription per active `env_id`.
#[derive(Clone)]
pub struct EnvironmentCache {
    envs: Arc<DashMap<Uuid, Arc<EnvChannel>>>,
    snapshots: SnapshotStore,
    nats: Option<NatsFanout>,
    shutdown: watch::Receiver<bool>,
}

impl EnvironmentCache {
    pub fn new(
        snapshots: SnapshotStore,
        nats: Option<NatsFanout>,
        shutdown: watch::Receiver<bool>,
    ) -> Self {
        Self {
            envs: Arc::new(DashMap::new()),
            snapshots,
            nats,
            shutdown,
        }
    }

    /// Ensures the environment is cached in memory (loading from R2/S3/MinIO if cold)
    /// and opens exactly one NATS subscription for `skotos.env.{env_id}.events`.
    pub async fn get_or_load(&self, env_id: Uuid) -> Result<Arc<EnvChannel>, FanoutError> {
        if let Some(existing) = self.envs.get(&env_id) {
            return Ok(existing.clone());
        }

        let snapshot = self
            .snapshots
            .get_snapshot(env_id)
            .await?
            .unwrap_or_else(|| EnvironmentSnapshot {
                environment_id: env_id,
                revision: 1,
                generated_at: Utc::now(),
                flags: BTreeMap::new(),
                segments: BTreeMap::new(),
            });
        let cached = Arc::new(CachedEnvData::from_snapshot(snapshot)?);
        let (server_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        let (client_tx, _) = broadcast::channel(BROADCAST_CAPACITY);

        let channel = Arc::new(EnvChannel {
            data: RwLock::new(cached),
            server_tx,
            client_tx,
        });

        // Atomic insert so only the winning caller spawns the single NATS subscriber for this env_id
        let mut won_insert = false;
        let resolved = self
            .envs
            .entry(env_id)
            .or_insert_with(|| {
                won_insert = true;
                channel.clone()
            })
            .clone();

        if won_insert {
            if let Some(nats) = self.nats.clone() {
                self.spawn_env_nats_subscriber(env_id, resolved.clone(), nats)
                    .await;
            }
        }

        Ok(resolved)
    }

    /// Manually injects an environment snapshot (useful in unit tests or push warmups).
    pub async fn seed_snapshot(
        &self,
        snapshot: EnvironmentSnapshot,
    ) -> Result<Arc<EnvChannel>, FanoutError> {
        let env_id = snapshot.environment_id;
        let cached = Arc::new(CachedEnvData::from_snapshot(snapshot)?);
        if let Some(existing) = self.envs.get(&env_id) {
            *existing.data.write().await = cached;
            return Ok(existing.clone());
        }
        let (server_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        let (client_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        let channel = Arc::new(EnvChannel {
            data: RwLock::new(cached),
            server_tx,
            client_tx,
        });
        self.envs.insert(env_id, channel.clone());
        Ok(channel)
    }

    async fn spawn_env_nats_subscriber(
        &self,
        env_id: Uuid,
        channel: Arc<EnvChannel>,
        nats: NatsFanout,
    ) {
        let mut sub = match nats.subscribe_env(env_id).await {
            Ok(s) => s,
            Err(err) => {
                warn!(%env_id, error = %err, "failed to subscribe to env NATS subject");
                return;
            }
        };
        let snapshots = self.snapshots.clone();
        let mut shutdown = self.shutdown.clone();

        tokio::spawn(async move {
            debug!(%env_id, "opened single NATS fanout subscription for environment");
            loop {
                tokio::select! {
                    maybe_msg = sub.next() => {
                        let Some(msg) = maybe_msg else { break; };
                        match serde_json::from_slice::<DeliveryEvent>(&msg.payload) {
                            Ok(event) => {
                                if let Err(err) = apply_delivery_event(env_id, &channel, &snapshots, event).await {
                                    warn!(%env_id, error = %err, "failed applying NATS delivery event");
                                }
                            }
                            Err(err) => {
                                warn!(%env_id, error = %err, "malformed DeliveryEvent payload on NATS");
                            }
                        }
                    }
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() {
                            break;
                        }
                    }
                }
            }
            debug!(%env_id, "closed NATS fanout subscription for environment");
        });
    }
}

async fn apply_delivery_event(
    env_id: Uuid,
    channel: &EnvChannel,
    snapshots: &SnapshotStore,
    event: DeliveryEvent,
) -> Result<(), FanoutError> {
    let client_affected = event.client_side_affected();
    let incoming_rev = event.revision();

    let sse_server_frame: Bytes = match event {
        DeliveryEvent::Patch {
            revision,
            path,
            version,
            data,
            ..
        } => {
            let mut next_snap = channel.data.read().await.snapshot.clone();
            next_snap.revision = next_snap.revision.max(revision);
            next_snap.generated_at = Utc::now();

            if let Some(flag_key) = path.strip_prefix("/flags/") {
                let flag: EvalFlag = serde_json::from_value(data.clone())?;
                let should_apply = next_snap
                    .flags
                    .get(flag_key)
                    .map(|cur| flag.version > cur.version)
                    .unwrap_or(true);
                if should_apply {
                    next_snap.flags.insert(flag_key.to_string(), flag);
                }
            } else if let Some(seg_key) = path.strip_prefix("/segments/") {
                let seg: EvalSegment = serde_json::from_value(data.clone())?;
                let should_apply = next_snap
                    .segments
                    .get(seg_key)
                    .map(|cur| seg.version > cur.version)
                    .unwrap_or(true);
                if should_apply {
                    next_snap.segments.insert(seg_key.to_string(), seg);
                }
            }

            let next_cached = Arc::new(CachedEnvData::from_snapshot(next_snap)?);
            *channel.data.write().await = next_cached;

            let frame_json = serde_json::to_string(&json!({
                "path": path,
                "version": version,
                "revision": revision,
                "data": data,
            }))?;
            Bytes::from(format!("event: patch\ndata: {frame_json}\n\n"))
        }
        DeliveryEvent::Delete {
            revision,
            path,
            version,
            ..
        } => {
            let mut next_snap = channel.data.read().await.snapshot.clone();
            next_snap.revision = next_snap.revision.max(revision);
            next_snap.generated_at = Utc::now();

            if let Some(flag_key) = path.strip_prefix("/flags/") {
                let should_remove = next_snap
                    .flags
                    .get(flag_key)
                    .map(|cur| version > cur.version)
                    .unwrap_or(true);
                if should_remove {
                    next_snap.flags.remove(flag_key);
                }
            } else if let Some(seg_key) = path.strip_prefix("/segments/") {
                let should_remove = next_snap
                    .segments
                    .get(seg_key)
                    .map(|cur| version > cur.version)
                    .unwrap_or(true);
                if should_remove {
                    next_snap.segments.remove(seg_key);
                }
            }

            let next_cached = Arc::new(CachedEnvData::from_snapshot(next_snap)?);
            *channel.data.write().await = next_cached;

            let frame_json = serde_json::to_string(&json!({
                "path": path,
                "version": version,
                "revision": revision,
            }))?;
            Bytes::from(format!("event: delete\ndata: {frame_json}\n\n"))
        }
        DeliveryEvent::Put { .. } => {
            if let Some(fresh) = snapshots.get_snapshot(env_id).await? {
                let next_cached = Arc::new(CachedEnvData::from_snapshot(fresh)?);
                let frame = next_cached.precompressed.sse_put_bytes.clone();
                *channel.data.write().await = next_cached;
                frame
            } else {
                channel
                    .data
                    .read()
                    .await
                    .precompressed
                    .sse_put_bytes
                    .clone()
            }
        }
    };

    // Broadcast to Server-Side SDK SSE listeners (ignore error if 0 connected)
    let _ = channel.server_tx.send(sse_server_frame);

    // Ping-to-Re-evaluate: ONLY ping Client-Side SSE sockets when client-side flags/segments changed
    if client_affected {
        let ping = Bytes::from(format!("event: ping\ndata: {{\"rev\":{incoming_rev}}}\n\n"));
        let _ = channel.client_tx.send(ping);
    }

    Ok(())
}
