#[derive(Debug, Clone, PartialEq)]
pub enum FlagDiacriticOperator {
    PositiveSet,
    NegativeSet,
    Require,
    Disallow,
    Clear,
    Unification,
}

impl FlagDiacriticOperator {
    pub fn from_str(key: &str) -> Option<FlagDiacriticOperator> {
        match key {
            "P" => Some(FlagDiacriticOperator::PositiveSet),
            "N" => Some(FlagDiacriticOperator::NegativeSet),
            "R" => Some(FlagDiacriticOperator::Require),
            "D" => Some(FlagDiacriticOperator::Disallow),
            "C" => Some(FlagDiacriticOperator::Clear),
            "U" => Some(FlagDiacriticOperator::Unification),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum HeaderFlag {
    Weighted,
    Deterministic,
    InputDeterministic,
    Minimized,
    Cyclic,
    HasEpsilonEpsilonTransitions,
    HasInputEpsilonTransitions,
    HasInputEpsilonCycles,
    HasUnweightedInputEpsilonCycles,
}

#[derive(Debug)]
pub struct FlagDiacriticOperation {
    pub operation: FlagDiacriticOperator,
    pub feature: SymbolNumber,
    pub value: ValueNumber,
}

pub type SymbolNumber = u16;
pub type ValueNumber = i16;
pub type TransitionTableIndex = u32;
pub type Weight = f32;
pub type FlagDiacriticState = Vec<i16>;
