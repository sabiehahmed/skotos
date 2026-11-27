//! Outbox & Snapshot Fanout Engine (ADR-0003).
//!
//! Compiles versioned PostgreSQL flag/segment configurations into pre-compressed
//! (`gzip` + `zstd`) Cloudflare R2 / S3 / MinIO snapshots, publishes incremental
//! delivery events onto NATS JetStream (`skotos.env.{env_id}.events`), keeps the
//! DB-free Delivery Edge auth registry synchronized, dispatches HMAC-signed
//! outbound webhooks, and executes scheduled flag changes.

pub mod nats;
pub mod object_store;
pub mod protocol;
pub mod webhooks;
#[cfg(feature = "workers")]
pub mod worker;

pub use nats::NatsFanout;
pub use object_store::{S3Config, SnapshotStore};
#[cfg(feature = "workers")]
pub use protocol::compile_environment_snapshot;
pub use protocol::{
    env_events_subject, precompress_snapshot, AuthIndex, AuthRegistryEvent, DeliveryEvent,
    EnvironmentSnapshot, FanoutError, PrecompressedSnapshot, SnapshotManifest, AUTH_KEYS_SUBJECT,
    NATS_STREAM_NAME,
};
#[cfg(feature = "workers")]
pub use webhooks::dispatch_webhooks_for_event;
pub use webhooks::{sign_webhook_payload, verify_webhook_signature};
#[cfg(feature = "workers")]
pub use worker::{OutboxWorker, OutboxWorkerConfig, SchedulerWorker};

pub const CRATE_NAME: &str = "skotos-fanout";
