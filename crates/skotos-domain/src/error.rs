use thiserror::Error;
use uuid::Uuid;

/// Errors raised by domain validation, compilation, and semantic patch application.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DomainError {
    #[error("invalid key '{key}': {reason}")]
    InvalidKey { key: String, reason: String },

    #[error("invalid flag variation: {0}")]
    InvalidVariation(String),

    #[error("variation '{0}' not found on flag")]
    VariationNotFound(Uuid),

    #[error("duplicate variation id '{0}'")]
    DuplicateVariationId(Uuid),

    #[error("variation '{variation_id}' is still referenced by: {}", references.join(", "))]
    VariationInUse {
        variation_id: Uuid,
        references: Vec<String>,
    },

    #[error("invalid rollout weights: {0}")]
    InvalidRolloutWeights(String),

    #[error("invalid targeting clause: {0}")]
    InvalidClause(String),

    #[error("invalid regex pattern '{pattern}': {reason}")]
    InvalidRegex { pattern: String, reason: String },

    #[error("invalid evaluation context: {0}")]
    InvalidContext(String),

    #[error("invalid attribute reference '{reference}': {reason}")]
    InvalidAttributeRef { reference: String, reason: String },

    #[error("targeting rule '{0}' not found")]
    RuleNotFound(String),

    #[error("prerequisite cycle detected: {}", path.join(" -> "))]
    PrerequisiteCycle { path: Vec<String> },

    #[error("flag '{0}' cannot reference itself as a prerequisite")]
    SelfPrerequisite(String),

    #[error("prerequisite flag '{0}' not found")]
    PrerequisiteFlagNotFound(String),

    #[error("patch must contain at least one instruction")]
    EmptyInstructions,

    #[error("invalid patch instruction: {0}")]
    InvalidPatch(String),

    #[error("validation error: {0}")]
    Validation(String),
}
