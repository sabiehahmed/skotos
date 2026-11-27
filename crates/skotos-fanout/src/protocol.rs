use std::collections::BTreeMap;
#[cfg(feature = "workers")]
use std::collections::HashMap;
use std::io::Write;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(feature = "workers")]
use skotos_domain::{compile_eval_flag_with_flags, compile_eval_segment, FeatureFlag};
use skotos_domain::{EnvironmentAuthRecord, EvalFlag, EvalSegment};
#[cfg(feature = "workers")]
use skotos_storage::EnvironmentCompileBundle;
use thiserror::Error;
use uuid::Uuid;

pub const NATS_STREAM_NAME: &str = "SKOTOS_EVENTS";
pub const AUTH_KEYS_SUBJECT: &str = "skotos.auth.keys";

/// Returns the canonical NATS JetStream subject for an environment's flag/segment events.
pub fn env_events_subject(env_id: Uuid) -> String {
    format!("skotos.env.{env_id}.events")
}

#[derive(Debug, Error)]
pub enum FanoutError {
    #[cfg(feature = "workers")]
    #[error("storage error: {0}")]
    Storage(#[from] skotos_storage::StorageError),
    #[error("domain error: {0}")]
    Domain(#[from] skotos_domain::DomainError),
    #[error("object store error: {0}")]
    ObjectStore(#[from] object_store::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("compression error: {0}")]
    Io(#[from] std::io::Error),
    #[error("nats error: {0}")]
    Nats(String),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
}

/// Full compiled environment ruleset serialized into pre-compressed R2/S3 snapshots
/// and delivered in Server-Side SDK `put` / `/sdk/v1/latest-all` responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSnapshot {
    pub environment_id: Uuid,
    pub revision: i64,
    pub generated_at: DateTime<Utc>,
    pub flags: BTreeMap<String, EvalFlag>,
    pub segments: BTreeMap<String, EvalSegment>,
}

/// Lightweight manifest stored alongside compressed snapshots in R2/S3.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotManifest {
    pub environment_id: Uuid,
    pub revision: i64,
    pub etag: String,
    pub updated_at: DateTime<Utc>,
}

/// Pre-serialized and pre-compressed environment snapshot bytes ready for zero-copy streaming.
#[derive(Debug, Clone)]
pub struct PrecompressedSnapshot {
    pub environment_id: Uuid,
    pub revision: i64,
    pub etag: String,
    pub json_bytes: Bytes,
    pub gzip_bytes: Bytes,
    pub zstd_bytes: Bytes,
    /// Pre-framed SSE `event: put\ndata: {"path":"/","data":...}\n\n` bytes.
    pub sse_put_bytes: Bytes,
}

/// Incremental fanout event published on `skotos.env.{env_id}.events`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DeliveryEvent {
    #[serde(rename_all = "camelCase")]
    Put {
        revision: i64,
        etag: String,
        client_side_affected: bool,
    },
    #[serde(rename_all = "camelCase")]
    Patch {
        revision: i64,
        etag: String,
        path: String,
        version: i64,
        data: Value,
        client_side_affected: bool,
    },
    #[serde(rename_all = "camelCase")]
    Delete {
        revision: i64,
        etag: String,
        path: String,
        version: i64,
        client_side_affected: bool,
    },
}

impl DeliveryEvent {
    pub fn revision(&self) -> i64 {
        match self {
            Self::Put { revision, .. }
            | Self::Patch { revision, .. }
            | Self::Delete { revision, .. } => *revision,
        }
    }

    pub fn client_side_affected(&self) -> bool {
        match self {
            Self::Put {
                client_side_affected,
                ..
            }
            | Self::Patch {
                client_side_affected,
                ..
            }
            | Self::Delete {
                client_side_affected,
                ..
            } => *client_side_affected,
        }
    }
}

/// Auth registry synchronization event published on `skotos.auth.keys`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum AuthRegistryEvent {
    Upsert { record: EnvironmentAuthRecord },
    Delete { env_id: Uuid },
}

/// Complete auth registry snapshot stored in R2/S3 at `auth/index.json` for DB-free edge cold starts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthIndex {
    pub generated_at: DateTime<Utc>,
    pub environments: Vec<EnvironmentAuthRecord>,
}

/// Compiles a PostgreSQL `EnvironmentCompileBundle` into an `EnvironmentSnapshot`.
#[cfg(feature = "workers")]
pub fn compile_environment_snapshot(
    bundle: &EnvironmentCompileBundle,
) -> Result<EnvironmentSnapshot, FanoutError> {
    let flag_lookup: HashMap<String, &FeatureFlag> = bundle
        .flags
        .iter()
        .map(|(f, _)| (f.key.clone(), f))
        .collect();

    let mut flags = BTreeMap::new();
    for (flag, env_cfg) in &bundle.flags {
        let compiled = compile_eval_flag_with_flags(flag, env_cfg, &flag_lookup)?;
        flags.insert(flag.key.clone(), compiled);
    }

    let mut segments = BTreeMap::new();
    for seg in &bundle.segments {
        segments.insert(seg.key.clone(), compile_eval_segment(seg));
    }

    Ok(EnvironmentSnapshot {
        environment_id: bundle.environment.id,
        revision: bundle.environment.revision,
        generated_at: Utc::now(),
        flags,
        segments,
    })
}

/// Serializes and pre-compresses an `EnvironmentSnapshot` once (JSON + gzip + zstd + SSE `put`).
pub fn precompress_snapshot(
    snapshot: &EnvironmentSnapshot,
) -> Result<PrecompressedSnapshot, FanoutError> {
    let json_vec = serde_json::to_vec(snapshot)?;
    let etag = format!("\"rev-{}\"", snapshot.revision);

    let mut gz = GzEncoder::new(Vec::with_capacity(json_vec.len() / 2), Compression::fast());
    gz.write_all(&json_vec)?;
    let gzip_vec = gz.finish()?;

    let zstd_vec = zstd::bulk::compress(&json_vec, 3)?;

    let put_payload = json!({
        "path": "/",
        "revision": snapshot.revision,
        "data": {
            "flags": snapshot.flags,
            "segments": snapshot.segments,
        }
    });
    let put_json = serde_json::to_string(&put_payload)?;
    let sse_frame = format!("event: put\ndata: {put_json}\n\n");

    Ok(PrecompressedSnapshot {
        environment_id: snapshot.environment_id,
        revision: snapshot.revision,
        etag,
        json_bytes: Bytes::from(json_vec),
        gzip_bytes: Bytes::from(gzip_vec),
        zstd_bytes: Bytes::from(zstd_vec),
        sse_put_bytes: Bytes::from(sse_frame),
    })
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use flate2::read::GzDecoder;

    use super::*;

    #[test]
    fn precompresses_snapshot_into_valid_json_gzip_zstd_and_sse_put() {
        let env_id = Uuid::new_v4();
        let snap = EnvironmentSnapshot {
            environment_id: env_id,
            revision: 7,
            generated_at: Utc::now(),
            flags: BTreeMap::new(),
            segments: BTreeMap::new(),
        };
        let pre = precompress_snapshot(&snap).expect("precompress");
        assert_eq!(pre.etag, "\"rev-7\"");

        // Verify gzip roundtrip
        let mut decoder = GzDecoder::new(pre.gzip_bytes.as_ref());
        let mut gunzipped = Vec::new();
        decoder.read_to_end(&mut gunzipped).expect("gunzip");
        assert_eq!(gunzipped, pre.json_bytes.as_ref());

        // Verify zstd roundtrip
        let decompressed =
            zstd::bulk::decompress(pre.zstd_bytes.as_ref(), 1024 * 1024).expect("zstd decompress");
        assert_eq!(decompressed, pre.json_bytes.as_ref());

        // Verify SSE put framing
        let sse = std::str::from_utf8(pre.sse_put_bytes.as_ref()).expect("utf8");
        assert!(sse.starts_with("event: put\ndata: "));
        assert!(sse.ends_with("\n\n"));
        assert!(sse.contains("\"revision\":7"));
    }
}
