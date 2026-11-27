//! Canonical pure synchronous Skotos feature flag evaluation engine.
//!
//! Full evaluation pipeline and golden suite are implemented in Milestone 1B (M2).

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
