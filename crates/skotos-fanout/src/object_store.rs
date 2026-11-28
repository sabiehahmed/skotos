use std::sync::Arc;

use bytes::Bytes;
use chrono::Utc;
use hmac::{Hmac, Mac};
use object_store::aws::AmazonS3Builder;
use object_store::memory::InMemory;
use object_store::path::Path as ObjPath;
use object_store::{ObjectStore, PutPayload};
use sha2::{Digest, Sha256};
use skotos_domain::EnvironmentAuthRecord;
use uuid::Uuid;

use crate::protocol::{
    AuthIndex, EnvironmentSnapshot, FanoutError, PrecompressedSnapshot, SnapshotManifest,
};

type HmacSha256 = Hmac<Sha256>;

/// S3 / Cloudflare R2 / MinIO object storage configuration.
#[derive(Debug, Clone)]
pub struct S3Config {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub force_path_style: bool,
}

impl S3Config {
    pub fn from_env() -> Self {
        Self {
            endpoint: std::env::var("S3_ENDPOINT")
                .unwrap_or_else(|_| "http://127.0.0.1:9000".to_string()),
            region: std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
            bucket: std::env::var("S3_BUCKET").unwrap_or_else(|_| "skotos-snapshots".to_string()),
            access_key_id: std::env::var("S3_ACCESS_KEY_ID")
                .unwrap_or_else(|_| "skotos".to_string()),
            secret_access_key: std::env::var("S3_SECRET_ACCESS_KEY")
                .unwrap_or_else(|_| "skotos-dev-secret".to_string()),
            force_path_style: std::env::var("S3_FORCE_PATH_STYLE")
                .map(|v| v != "false" && v != "0")
                .unwrap_or(true),
        }
    }
}

/// Pre-compressed snapshot & DB-free auth registry object storage wrapper (R2 / S3 / MinIO / InMemory).
#[derive(Clone)]
pub struct SnapshotStore {
    inner: Arc<dyn ObjectStore>,
}

impl SnapshotStore {
    pub fn in_memory() -> Self {
        Self {
            inner: Arc::new(InMemory::new()),
        }
    }

    pub fn from_s3_config(cfg: &S3Config) -> Result<Self, FanoutError> {
        let mut builder = AmazonS3Builder::new()
            .with_endpoint(&cfg.endpoint)
            .with_region(&cfg.region)
            .with_bucket_name(&cfg.bucket)
            .with_access_key_id(&cfg.access_key_id)
            .with_secret_access_key(&cfg.secret_access_key)
            .with_virtual_hosted_style_request(!cfg.force_path_style);

        if cfg.endpoint.starts_with("http://") {
            builder = builder.with_allow_http(true);
        }

        let store = builder.build()?;
        Ok(Self {
            inner: Arc::new(store),
        })
    }

    /// Ensures the configured bucket exists on MinIO / S3-compatible endpoints using AWS SigV4.
    /// Treats `200 OK` and `409 Conflict` (`BucketAlreadyOwnedByYou` / `BucketAlreadyExists`) as success.
    pub async fn ensure_s3_bucket_exists(cfg: &S3Config) -> Result<(), FanoutError> {
        let endpoint = cfg.endpoint.trim_end_matches('/');
        let host = endpoint
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        let url = format!("{endpoint}/{}", cfg.bucket);
        let now = Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date_stamp = now.format("%Y%m%d").to_string();
        let payload_hash = hex::encode(Sha256::digest(b""));

        let canonical_uri = format!("/{}", cfg.bucket);
        let canonical_headers =
            format!("host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n");
        let signed_headers = "host;x-amz-content-sha256;x-amz-date";
        let canonical_request = format!(
            "PUT\n{canonical_uri}\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
        );
        let canonical_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));
        let credential_scope = format!("{date_stamp}/{}/s3/aws4_request", cfg.region);
        let string_to_sign =
            format!("AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{canonical_hash}");

        let k_date = hmac_sha256(
            format!("AWS4{}", cfg.secret_access_key).as_bytes(),
            date_stamp.as_bytes(),
        );
        let k_region = hmac_sha256(&k_date, cfg.region.as_bytes());
        let k_service = hmac_sha256(&k_region, b"s3");
        let k_signing = hmac_sha256(&k_service, b"aws4_request");
        let signature = hex::encode(hmac_sha256(&k_signing, string_to_sign.as_bytes()));

        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
            cfg.access_key_id
        );

        let client = reqwest::Client::new();
        let resp = client
            .put(&url)
            .header("host", host)
            .header("x-amz-date", &amz_date)
            .header("x-amz-content-sha256", &payload_hash)
            .header("authorization", authorization)
            .send()
            .await?;

        let status = resp.status();
        if status.is_success() || status.as_u16() == 409 {
            Ok(())
        } else {
            let text = resp.text().await.unwrap_or_default();
            Err(FanoutError::Nats(format!(
                "failed to ensure S3 bucket '{}': HTTP {} {}",
                cfg.bucket, status, text
            )))
        }
    }

    /// Uploads pre-compressed snapshot variants (`latest.json`, `latest.json.gz`, `latest.json.zst`)
    /// and `manifest.json` for an environment.
    pub async fn put_snapshot(&self, pre: &PrecompressedSnapshot) -> Result<(), FanoutError> {
        let env_id = pre.environment_id;
        let json_path = ObjPath::from(format!("snapshots/{env_id}/latest.json"));
        let gz_path = ObjPath::from(format!("snapshots/{env_id}/latest.json.gz"));
        let zst_path = ObjPath::from(format!("snapshots/{env_id}/latest.json.zst"));
        let manifest_path = ObjPath::from(format!("snapshots/{env_id}/manifest.json"));

        let manifest = SnapshotManifest {
            environment_id: env_id,
            revision: pre.revision,
            etag: pre.etag.clone(),
            updated_at: Utc::now(),
        };
        let manifest_bytes = Bytes::from(serde_json::to_vec(&manifest)?);

        tokio::try_join!(
            self.inner
                .put(&json_path, PutPayload::from(pre.json_bytes.clone())),
            self.inner
                .put(&gz_path, PutPayload::from(pre.gzip_bytes.clone())),
            self.inner
                .put(&zst_path, PutPayload::from(pre.zstd_bytes.clone())),
            self.inner
                .put(&manifest_path, PutPayload::from(manifest_bytes)),
        )?;

        Ok(())
    }

    pub async fn get_snapshot_json(&self, env_id: Uuid) -> Result<Option<Bytes>, FanoutError> {
        let path = ObjPath::from(format!("snapshots/{env_id}/latest.json"));
        match self.inner.get(&path).await {
            Ok(res) => Ok(Some(res.bytes().await?)),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(FanoutError::ObjectStore(e)),
        }
    }

    pub async fn get_snapshot(
        &self,
        env_id: Uuid,
    ) -> Result<Option<EnvironmentSnapshot>, FanoutError> {
        let Some(bytes) = self.get_snapshot_json(env_id).await? else {
            return Ok(None);
        };
        let snap: EnvironmentSnapshot = serde_json::from_slice(&bytes)?;
        Ok(Some(snap))
    }

    pub async fn get_manifest(
        &self,
        env_id: Uuid,
    ) -> Result<Option<SnapshotManifest>, FanoutError> {
        let path = ObjPath::from(format!("snapshots/{env_id}/manifest.json"));
        match self.inner.get(&path).await {
            Ok(res) => {
                let bytes = res.bytes().await?;
                Ok(Some(serde_json::from_slice(&bytes)?))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(FanoutError::ObjectStore(e)),
        }
    }

    pub async fn put_auth_record(&self, record: &EnvironmentAuthRecord) -> Result<(), FanoutError> {
        let path = ObjPath::from(format!("auth/environments/{}.json", record.env_id));
        let bytes = Bytes::from(serde_json::to_vec(record)?);
        self.inner.put(&path, PutPayload::from(bytes)).await?;
        Ok(())
    }

    pub async fn put_auth_index(&self, index: &AuthIndex) -> Result<(), FanoutError> {
        let path = ObjPath::from("auth/index.json");
        let bytes = Bytes::from(serde_json::to_vec(index)?);
        self.inner.put(&path, PutPayload::from(bytes)).await?;
        Ok(())
    }

    pub async fn get_auth_index(&self) -> Result<Option<AuthIndex>, FanoutError> {
        let path = ObjPath::from("auth/index.json");
        match self.inner.get(&path).await {
            Ok(res) => {
                let bytes = res.bytes().await?;
                Ok(Some(serde_json::from_slice(&bytes)?))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(FanoutError::ObjectStore(e)),
        }
    }

    /// Uploads a Zstd-compressed Big Segment membership snapshot (`big-segments/{env_id}/{segment_key}.zst`).
    pub async fn put_big_segment_snapshot(
        &self,
        snap: &skotos_domain::BigSegmentSnapshot,
    ) -> Result<(), FanoutError> {
        let path = ObjPath::from(format!(
            "big-segments/{}/{}.zst",
            snap.env_id, snap.segment_key
        ));
        let json_bytes = serde_json::to_vec(snap)?;
        let zstd_bytes = zstd::bulk::compress(&json_bytes, 3)?;
        self.inner
            .put(&path, PutPayload::from(Bytes::from(zstd_bytes)))
            .await?;
        Ok(())
    }

    /// Loads and decompresses a Big Segment membership snapshot from R2/S3/MinIO.
    pub async fn get_big_segment_snapshot(
        &self,
        env_id: Uuid,
        segment_key: &str,
    ) -> Result<Option<skotos_domain::BigSegmentSnapshot>, FanoutError> {
        let path = ObjPath::from(format!("big-segments/{env_id}/{segment_key}.zst"));
        match self.inner.get(&path).await {
            Ok(res) => {
                let compressed = res.bytes().await?;
                let raw = zstd::stream::decode_all(compressed.as_ref())?;
                let snap: skotos_domain::BigSegmentSnapshot = serde_json::from_slice(&raw)?;
                Ok(Some(snap))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(FanoutError::ObjectStore(e)),
        }
    }

    /// Archives immutable audit log entries as Zstd-compressed NDJSON in zero-egress R2/S3 storage.
    /// Returns `(object_key, compressed_bytes, sha256_hex)`.
    pub async fn put_audit_archive(
        &self,
        org_id: Uuid,
        archive_id: Uuid,
        entries: &[skotos_domain::AuditLogEntry],
    ) -> Result<(String, i64, String), FanoutError> {
        let mut ndjson = Vec::with_capacity(entries.len() * 256);
        for entry in entries {
            serde_json::to_writer(&mut ndjson, entry)?;
            ndjson.push(b'\n');
        }
        let compressed = zstd::bulk::compress(&ndjson, 3)?;
        let sha = hex::encode(Sha256::digest(&compressed));
        let key = format!("audit-archives/{org_id}/{archive_id}.ndjson.zst");
        let path = ObjPath::from(key.clone());
        let len = compressed.len() as i64;
        self.inner
            .put(&path, PutPayload::from(Bytes::from(compressed)))
            .await?;
        Ok((key, len, sha))
    }
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key length valid");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}
