use crate::types::{SymbolNumber, TransitionTableIndex, Weight};

/// Represents a transition in a finite-state transducer.
///
/// A transition connects states in the FST and carries a symbol and weight.
#[derive(Debug, Clone)]
pub struct SymbolTransition {
    /// Target state index, or None if this is a final state
    pub target: Option<TransitionTableIndex>,
    /// Input/output symbol number
    pub symbol: Option<SymbolNumber>,
    /// Transition weight
    pub weight: Option<Weight>,
}

impl SymbolTransition {
    pub fn new(
        target: Option<TransitionTableIndex>,
        symbol: Option<SymbolNumber>,
        weight: Option<Weight>,
    ) -> SymbolTransition {
        SymbolTransition {
            target,
            symbol,
            weight,
        }
    }

    #[inline(always)]
    pub fn target(&self) -> Option<TransitionTableIndex> {
        self.target
    }

    #[inline(always)]
    pub fn symbol(&self) -> Option<SymbolNumber> {
        self.symbol
    }

    #[inline(always)]
    pub fn weight(&self) -> Option<Weight> {
        self.weight
    }

    #[inline(always)]
    pub fn clone_with_epsilon_symbol(&self) -> SymbolTransition {
        SymbolTransition {
            target: self.target,
            symbol: Some(0),
            weight: self.weight,
        }
    }
}
