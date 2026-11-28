//! PostgreSQL 16+ storage repositories, embedded SQLx migrations, and transactional outbox.

pub mod error;
pub mod keys;
pub mod phase2;
pub mod phase3;
pub mod phase4;
pub mod repo;

pub use error::StorageError;
pub use keys::{
    generate_client_side_id, generate_prefixed_secret, generate_salt, sha256_hex,
    GeneratedSecretKey,
};
pub use phase2::{
    ClaimedOutboxEvent, CreateSegmentInput, CreatedTriggerOutcome, CreatedWebhookOutcome,
    DueScheduledChange, EnvironmentAuthRecord, EnvironmentCompileBundle, EnvironmentKeyHashRecord,
    ExecutedTriggerOutcome, PatchSegmentOutcome, WebhookWithSecret,
};
pub use phase3::{DueScheduledSegmentChange, ResolvedMemberRbac};
pub use phase4::{ActiveGuardedRolloutContext, CreatedSyncedSegmentOutcome, ResolvedSyncedSegment};
pub use repo::{
    run_migrations, AuditActor, AuditLogFilter, AuditLogPage, CreateFlagInput,
    CreatedApiTokenOutcome, FlagWithEnvironments, MemberWithPassword, PatchFlagOutcome,
    RotatedKeyOutcome, Storage, MIGRATOR,
};

/// Canonical crate identifier for workspace wiring verification.
pub const CRATE_NAME: &str = "skotos-storage";

/// Returns the linked domain crate identifier.
pub fn domain_crate_name() -> &'static str {
    skotos_domain::CRATE_NAME
}

#[cfg(test)]
mod tests {
    use super::{domain_crate_name, CRATE_NAME};

    #[test]
    fn links_domain_crate() {
        assert_eq!(CRATE_NAME, "skotos-storage");
        assert_eq!(domain_crate_name(), "skotos-domain");
    }
}
