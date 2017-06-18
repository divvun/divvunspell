use types::{SymbolNumber, ValueNumber, FlagDiacriticOperator, FlagDiacriticOperation, Weight};
use std::collections::BTreeMap;
use std::f32;

struct Transition {
    input_symbol: Option<SymbolNumber>,
    output_symbol: Option<SymbolNumber>,
    target_index: Option<TransitionTableIndex>,
    transition_weight: Weight
}

impl Transition {
    pub fn new(input: SymbolNumber, output: SymbolNumber, target: TransitionTableIndex, weight: Weight) -> Transition {
        Transition {
            input_symbol: Some(input),
            output_symbol: Some(output),
            target_index: Some(target),
            transition_weight: weight
        }
    }

    pub fn empty() -> Transition {
        Transition {
            input_symbol: None,
            output_symbol: None,
            target_index: None,
            transition_weight: f32::INFINITY
        }
    }

    pub fn target(&self) -> Option<TransitionTableIndex> {
        self.target_index
    }

    pub fn output(&self) -> Option<SymbolNumber> {
        self.output_symbol
    }

    pub fn input(&self) -> Option<SymbolNumber> {
        self.input_symbol
    }

    pub fn weight(&self) -> Weight {
        self.transition_weight
    }

    pub fn is_final(&self) -> bool {
        self.input_symbol == None &&
            self.output_symbol == None &&
            self.target_index == Some(1)
    }
}