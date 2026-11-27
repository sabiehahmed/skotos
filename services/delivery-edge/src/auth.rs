use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use futures::StreamExt;
use sha2::{Digest, Sha256};
use skotos_domain::{EnvironmentAuthRecord, EnvironmentKeyKind};
use skotos_fanout::{AuthRegistryEvent, NatsFanout, SnapshotStore};
use tokio::sync::watch;
use tracing::{debug, info, warn};
use uuid::Uuid;

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

#[derive(Debug, Clone)]
pub struct ResolvedKey {
    pub org_id: Uuid,
    pub env_id: Uuid,
    pub kind: EnvironmentKeyKind,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct ResolvedClientId {
    pub org_id: Uuid,
    pub env_id: Uuid,
}

/// DB-free in-memory credential registry synced from R2/S3 `auth/index.json` + NATS `skotos.auth.keys`.
#[derive(Clone)]
pub struct AuthRegistry {
    by_key_hash: Arc<DashMap<String, ResolvedKey>>,
    by_client_id: Arc<DashMap<String, ResolvedClientId>>,
    snapshots: SnapshotStore,
}

impl AuthRegistry {
    pub fn new(snapshots: SnapshotStore) -> Self {
        Self {
            by_key_hash: Arc::new(DashMap::new()),
            by_client_id: Arc::new(DashMap::new()),
            snapshots,
        }
    }

    /// Loads `auth/index.json` from R2/S3/MinIO into memory.
    pub async fn refresh_from_object_store(&self) -> usize {
        match self.snapshots.get_auth_index().await {
            Ok(Some(index)) => {
                let count = index.environments.len();
                for rec in index.environments {
                    self.upsert_record(&rec);
                }
                debug!(
                    environments = count,
                    "refreshed auth registry from object store"
                );
                count
            }
            Ok(None) => 0,
            Err(err) => {
                warn!(error = %err, "failed to load auth/index.json from object store");
                0
            }
        }
    }

    pub fn upsert_record(&self, rec: &EnvironmentAuthRecord) {
        self.by_client_id.insert(
            rec.client_side_id.clone(),
            ResolvedClientId {
                org_id: rec.org_id,
                env_id: rec.env_id,
            },
        );
        for key in &rec.keys {
            self.by_key_hash.insert(
                key.key_hash.clone(),
                ResolvedKey {
                    org_id: rec.org_id,
                    env_id: rec.env_id,
                    kind: key.kind,
                    expires_at: key.expires_at,
                },
            );
        }
    }

    pub fn remove_env(&self, env_id: Uuid) {
        self.by_client_id.retain(|_, v| v.env_id != env_id);
        self.by_key_hash.retain(|_, v| v.env_id != env_id);
    }

    /// Resolves a plaintext SDK or Mobile key (`sk_sdk_...` / `sk_mob_...`), refreshing
    /// once from object storage on cache miss (handles freshly rotated keys cleanly).
    pub async fn verify_secret_key(
        &self,
        raw_header: &str,
        expected_kind: EnvironmentKeyKind,
    ) -> Option<ResolvedKey> {
        let token = raw_header
            .trim()
            .strip_prefix("Bearer ")
            .or_else(|| raw_header.trim().strip_prefix("bearer "))
            .unwrap_or(raw_header.trim());
        if token.is_empty() {
            return None;
        }
        let hash = sha256_hex(token);
        if let Some(found) = self.lookup_valid_key(&hash, expected_kind) {
            return Some(found);
        }
        // Cache miss -> try one object-store refresh in case NATS message was missed
        self.refresh_from_object_store().await;
        self.lookup_valid_key(&hash, expected_kind)
    }

    fn lookup_valid_key(
        &self,
        hash: &str,
        expected_kind: EnvironmentKeyKind,
    ) -> Option<ResolvedKey> {
        let entry = self.by_key_hash.get(hash)?;
        if entry.kind != expected_kind {
            return None;
        }
        if let Some(exp) = entry.expires_at {
            if exp <= Utc::now() {
                return None;
            }
        }
        Some(entry.clone())
    }

    /// Resolves a public `clientSideId` (`sk_cid_...`), refreshing once from object store on miss.
    pub async fn resolve_client_side_id(&self, client_side_id: &str) -> Option<ResolvedClientId> {
        if let Some(found) = self.by_client_id.get(client_side_id) {
            return Some(found.clone());
        }
        self.refresh_from_object_store().await;
        self.by_client_id.get(client_side_id).map(|e| e.clone())
    }

    /// Spawns background NATS subscription updating credentials in real time.
    pub async fn spawn_nats_sync(&self, nats: NatsFanout, mut shutdown: watch::Receiver<bool>) {
        let mut sub = match nats.subscribe_auth_keys().await {
            Ok(s) => s,
            Err(err) => {
                warn!(error = %err, "failed to subscribe to skotos.auth.keys");
                return;
            }
        };
        let registry = self.clone();
        tokio::spawn(async move {
            info!("delivery-edge subscribed to skotos.auth.keys");
            loop {
                tokio::select! {
                    maybe_msg = sub.next() => {
                        let Some(msg) = maybe_msg else { break; };
                        match serde_json::from_slice::<AuthRegistryEvent>(&msg.payload) {
                            Ok(AuthRegistryEvent::Upsert { record }) => {
                                registry.upsert_record(&record);
                            }
                            Ok(AuthRegistryEvent::Delete { env_id }) => {
                                registry.remove_env(env_id);
                            }
                            Err(err) => {
                                warn!(error = %err, "malformed auth registry event on NATS");
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
        });
    }
}
