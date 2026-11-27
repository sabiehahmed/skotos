//! PostgreSQL storage repositories, embedded SQLx migrations, and transactional outbox.
//!
//! Full PostgreSQL schema and repositories are implemented in Milestone 1C (M4).

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
