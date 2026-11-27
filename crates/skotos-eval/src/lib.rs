//! Canonical pure synchronous Skotos feature flag evaluation engine.
//!
//! Produces deterministic evaluations verified against `spec/evaluation-tests/`
//! and powers Control Plane simulation, Delivery Edge `evalx`, Relay Proxy, and
//! the Rust Server SDK.

pub mod bucketing;
pub mod evaluator;
pub mod operators;
pub mod redaction;
pub mod segment;
pub mod store;

pub use bucketing::{
    build_hash_input, evaluate_rollout, extract_bucket_value, hash_to_bucket, RolloutResult,
    BUCKET_SCALE,
};
pub use evaluator::{bool_variation, evaluate, json_variation, number_variation, string_variation};
pub use operators::parse_semver_relaxed;
pub use redaction::{redact_context, RedactionConfig};
pub use store::{EvalStore, InMemoryEvalStore};

/// Canonical crate identifier for workspace wiring verification.
pub const CRATE_NAME: &str = "skotos-eval";

/// Returns the linked domain crate identifier.
pub fn domain_crate_name() -> &'static str {
    skotos_domain::CRATE_NAME
}

#[cfg(test)]
mod tests {
    use super::{domain_crate_name, CRATE_NAME};

    #[test]
    fn links_domain_crate() {
        assert_eq!(CRATE_NAME, "skotos-eval");
        assert_eq!(domain_crate_name(), "skotos-domain");
    }
}
