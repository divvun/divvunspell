use crate::types::{SymbolNumber, TransitionTableIndex, Weight};

#[derive(Debug, Clone)]
pub struct SymbolTransition {
    target: Option<TransitionTableIndex>,
    symbol: Option<SymbolNumber>,
    weight: Option<Weight>,
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
