use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FlagDiacriticOperator {
    PositiveSet,
    NegativeSet,
    Require,
    Disallow,
    Clear,
    Unification,
}

impl std::str::FromStr for FlagDiacriticOperator {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "P" => Ok(FlagDiacriticOperator::PositiveSet),
            "N" => Ok(FlagDiacriticOperator::NegativeSet),
            "R" => Ok(FlagDiacriticOperator::Require),
            "D" => Ok(FlagDiacriticOperator::Disallow),
            "C" => Ok(FlagDiacriticOperator::Clear),
            "U" => Ok(FlagDiacriticOperator::Unification),
            _ => Err(()),
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

#[derive(Debug, Serialize, Deserialize)]
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
pub type OperationsMap = hashbrown::HashMap<SymbolNumber, FlagDiacriticOperation>;
