//! Error types for spell-checking operations.

use crate::types::TransitionTableIndex;

/// Errors that can occur during spell-checking operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SpellerError {
    /// Invalid transducer state encountered during spell-checking
    #[error("Invalid transducer state at index {0}")]
    InvalidState(TransitionTableIndex),

    /// Failed to calculate transition weight
    #[error("Failed to calculate weight for transition")]
    WeightCalculation,

    /// Transition operation failed
    #[error("Transition operation failed at index {0}")]
    TransitionFailed(TransitionTableIndex),

    /// Required symbol not found in alphabet
    #[error("Symbol not found in alphabet")]
    MissingSymbol,

    /// Unexpected None value in critical path
    #[error("Unexpected None value in {0}")]
    UnexpectedNone(&'static str),
}
