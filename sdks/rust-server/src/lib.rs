//! Official Skotos Rust Server-Side SDK & OpenFeature Provider (ADR-0004).
//!
//! Reuses `crates/skotos-eval` natively for zero-latency (`< 1 ms`) in-process
//! evaluation with 100% golden test suite parity.

pub mod client;
pub mod events;
pub mod openfeature;
pub mod store;

pub use client::{FlagChangeEvent, SkotosClient, SkotosConfig};
pub use events::EventProcessor;
pub use openfeature::{OpenFeatureContext, ResolutionDetails, SkotosOpenFeatureProvider};
pub use skotos_domain::{Context, EvaluationDetail, EvaluationReason, SingleContext};
pub use store::{TestData, VersionedFeatureStore};

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn offline_test_data_and_openfeature_provider_work_end_to_end() {
        let td = TestData::new();
        td.set_bool_flag("new-nav", true);

        let client = SkotosClient::with_test_data(&td);
        assert!(client.is_initialized());

        let ctx = Context::single(SingleContext::new("user", "u1"));
        assert!(client.bool_variation("new-nav", &ctx, false));
        assert!(!client.bool_variation("missing-flag", &ctx, false));

        let provider = SkotosOpenFeatureProvider::new(client);
        let of_ctx = OpenFeatureContext::with_targeting_key("u1").with_attr("plan", json!("pro"));
        let res = provider.resolve_bool_value("new-nav", &of_ctx, false);
        assert!(res.value);
        assert_eq!(res.variant.as_deref(), Some("0"));
        assert_eq!(res.reason, "DEFAULT");
        assert!(res.error_code.is_none());

        let missing = provider.resolve_bool_value("unknown-flag", &of_ctx, false);
        assert!(!missing.value);
        assert_eq!(missing.error_code.as_deref(), Some("FLAG_NOT_FOUND"));
    }
}
