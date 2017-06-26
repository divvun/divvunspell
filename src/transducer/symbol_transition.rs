use types::{
    TransitionTableIndex,
    SymbolNumber,
    Weight
};

#[derive(Debug, Clone)]
pub struct SymbolTransition {
    target: Option<TransitionTableIndex>,
    symbol: Option<SymbolNumber>,
    weight: Option<Weight>
}

impl SymbolTransition {
    pub fn new(target: Option<TransitionTableIndex>, symbol: Option<SymbolNumber>, weight: Option<Weight>) -> SymbolTransition {
        SymbolTransition {
            target: target,
            symbol: symbol,
            weight: weight
        }
    }

    pub fn target(&self) -> Option<TransitionTableIndex> {
        self.target
    }

    pub fn symbol(&self) -> Option<SymbolNumber> {
        self.symbol
    }

    pub fn weight(&self) -> Option<Weight> {
        self.weight
    }

    pub fn clone_with_epsilon_target(&self) -> SymbolTransition {
        SymbolTransition {
            target: Some(0),
            symbol: self.symbol,
            weight: self.weight
        }
    }
}