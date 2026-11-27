use std::collections::HashMap;
use std::sync::RwLock;

use serde_json::Value;
use skotos_domain::{EvalFlag, EvalSegment, EvalVariationOrRollout};
use skotos_eval::EvalStore;

/// Thread-safe, monotonically version-checked feature store (`put` replaces all;
/// `patch`/`delete` apply only when `incoming.version > stored.version`).
#[derive(Debug, Default)]
pub struct VersionedFeatureStore {
    inner: RwLock<StoreInner>,
}

#[derive(Debug, Default)]
struct StoreInner {
    initialized: bool,
    flags: HashMap<String, StoredItem<EvalFlag>>,
    segments: HashMap<String, StoredItem<EvalSegment>>,
}

#[derive(Debug, Clone)]
struct StoredItem<T> {
    version: i64,
    item: Option<T>, // None == tombstone
}

impl VersionedFeatureStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_initialized(&self) -> bool {
        self.inner.read().map(|g| g.initialized).unwrap_or(false)
    }

    pub fn put(
        &self,
        flags: HashMap<String, EvalFlag>,
        segments: HashMap<String, EvalSegment>,
    ) -> Vec<String> {
        let Ok(mut guard) = self.inner.write() else {
            return Vec::new();
        };
        let mut changed_keys: Vec<String> = Vec::new();
        for (k, v) in &flags {
            let changed = guard
                .flags
                .get(k)
                .map(|old| old.version != v.version || old.item.is_none())
                .unwrap_or(true);
            if changed {
                changed_keys.push(k.clone());
            }
        }
        guard.flags = flags
            .into_iter()
            .map(|(k, f)| {
                let version = f.version;
                (
                    k,
                    StoredItem {
                        version,
                        item: Some(f),
                    },
                )
            })
            .collect();
        guard.segments = segments
            .into_iter()
            .map(|(k, s)| {
                let version = s.version;
                (
                    k,
                    StoredItem {
                        version,
                        item: Some(s),
                    },
                )
            })
            .collect();
        guard.initialized = true;
        changed_keys
    }

    pub fn upsert_flag(&self, flag: EvalFlag) -> bool {
        let Ok(mut guard) = self.inner.write() else {
            return false;
        };
        let should_apply = guard
            .flags
            .get(&flag.key)
            .map(|cur| flag.version > cur.version)
            .unwrap_or(true);
        if should_apply {
            let version = flag.version;
            guard.flags.insert(
                flag.key.clone(),
                StoredItem {
                    version,
                    item: Some(flag),
                },
            );
            guard.initialized = true;
        }
        should_apply
    }

    pub fn delete_flag(&self, key: &str, version: i64) -> bool {
        let Ok(mut guard) = self.inner.write() else {
            return false;
        };
        let should_apply = guard
            .flags
            .get(key)
            .map(|cur| version > cur.version)
            .unwrap_or(true);
        if should_apply {
            guard.flags.insert(
                key.to_string(),
                StoredItem {
                    version,
                    item: None,
                },
            );
        }
        should_apply
    }

    pub fn upsert_segment(&self, seg: EvalSegment) -> bool {
        let Ok(mut guard) = self.inner.write() else {
            return false;
        };
        let should_apply = guard
            .segments
            .get(&seg.key)
            .map(|cur| seg.version > cur.version)
            .unwrap_or(true);
        if should_apply {
            let version = seg.version;
            guard.segments.insert(
                seg.key.clone(),
                StoredItem {
                    version,
                    item: Some(seg),
                },
            );
        }
        should_apply
    }

    pub fn delete_segment(&self, key: &str, version: i64) -> bool {
        let Ok(mut guard) = self.inner.write() else {
            return false;
        };
        let should_apply = guard
            .segments
            .get(key)
            .map(|cur| version > cur.version)
            .unwrap_or(true);
        if should_apply {
            guard.segments.insert(
                key.to_string(),
                StoredItem {
                    version,
                    item: None,
                },
            );
        }
        should_apply
    }

    pub fn all_flag_keys(&self) -> Vec<String> {
        let Ok(guard) = self.inner.read() else {
            return Vec::new();
        };
        guard
            .flags
            .iter()
            .filter_map(|(k, v)| v.item.as_ref().map(|_| k.clone()))
            .collect()
    }

    /// Snapshots all live flags & segments into an `InMemoryEvalStore` for lock-free evaluation.
    pub fn snapshot_eval_store(&self) -> skotos_eval::InMemoryEvalStore {
        let mut out = skotos_eval::InMemoryEvalStore::new();
        if let Ok(guard) = self.inner.read() {
            for entry in guard.flags.values() {
                if let Some(flag) = &entry.item {
                    out.insert_flag(flag.clone());
                }
            }
            for entry in guard.segments.values() {
                if let Some(seg) = &entry.item {
                    out.insert_segment(seg.clone());
                }
            }
        }
        out
    }

    pub fn get_flag_cloned(&self, key: &str) -> Option<EvalFlag> {
        let guard = self.inner.read().ok()?;
        guard.flags.get(key).and_then(|e| e.item.clone())
    }
}

/// Offline in-memory mock data source for unit & integration tests (zero network I/O).
#[derive(Debug, Clone, Default)]
pub struct TestData {
    store: std::sync::Arc<VersionedFeatureStore>,
}

impl TestData {
    pub fn new() -> Self {
        let td = Self::default();
        td.store.put(HashMap::new(), HashMap::new());
        td
    }

    pub fn store(&self) -> std::sync::Arc<VersionedFeatureStore> {
        self.store.clone()
    }

    pub fn set_bool_flag(&self, key: &str, value: bool) -> &Self {
        let next_ver = self
            .store
            .get_flag_cloned(key)
            .map(|f| f.version + 1)
            .unwrap_or(1);
        self.store.upsert_flag(EvalFlag {
            key: key.to_string(),
            version: next_ver,
            on: true,
            salt: "test-salt".to_string(),
            variations: vec![Value::Bool(true), Value::Bool(false)],
            off_variation: Some(1),
            prerequisites: vec![],
            targets: vec![],
            rules: vec![],
            fallthrough: EvalVariationOrRollout {
                variation: Some(if value { 0 } else { 1 }),
                rollout: None,
            },
            client_side_availability: Default::default(),
            track_events: false,
            track_events_fallthrough: false,
        });
        self
    }

    pub fn set_flag(&self, flag: EvalFlag) -> &Self {
        self.store.upsert_flag(flag);
        self
    }

    pub fn set_segment(&self, segment: EvalSegment) -> &Self {
        self.store.upsert_segment(segment);
        self
    }
}

/// Wrapper allowing direct synchronous `skotos_eval::evaluate` against a snapshot.
pub struct StoreSnapshotEval {
    inner: skotos_eval::InMemoryEvalStore,
}

impl StoreSnapshotEval {
    pub fn from_store(store: &VersionedFeatureStore) -> Self {
        Self {
            inner: store.snapshot_eval_store(),
        }
    }
}

impl EvalStore for StoreSnapshotEval {
    fn get_flag(&self, key: &str) -> Option<&EvalFlag> {
        self.inner.get_flag(key)
    }

    fn get_segment(&self, key: &str) -> Option<&EvalSegment> {
        self.inner.get_segment(key)
    }
}
