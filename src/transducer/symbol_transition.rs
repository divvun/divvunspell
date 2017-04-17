use types::{
    TransitionTableIndex,
    SymbolNumber,
    Weight
};

#[derive(Clone)]
pub struct SymbolTransition {
    pub index: TransitionTableIndex,
    pub symbol: Option<SymbolNumber>,
    pub weight: Option<Weight>
}

impl SymbolTransition {
    pub fn empty() -> SymbolTransition {
        SymbolTransition {
            index: 0,
            symbol: None,
            weight: None
        }
    }
}