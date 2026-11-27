//! Core Skotos domain entities, validation, and semantic flag patch logic.
//!
//! Full domain models are implemented in Milestone 1B (M2).

/// Canonical crate identifier for workspace wiring verification.
pub const CRATE_NAME: &str = "skotos-domain";

/// Returns the semantic version of the `skotos-domain` crate.
pub fn crate_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::{crate_version, CRATE_NAME};

    #[test]
    fn exposes_crate_metadata() {
        assert_eq!(CRATE_NAME, "skotos-domain");
        assert!(!crate_version().is_empty());
    }
}
