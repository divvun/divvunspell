use types::{SymbolNumber, ValueNumber, FlagDiacriticOperator, FlagDiacriticOperation, Weight};
use std::collections::BTreeMap;
use std::f32;

type TransitionTableIndex = u32;

struct TransitionIndex {
    input_symbol: Option<SymbolNumber>,
    first_transition_index: Option<TransitionField>
}

enum TransitionField {
    Weight(Weight),
    TransitionTableIndex(TransitionTableIndex)
}

impl TransitionIndex {
    pub fn new(input: SymbolNumber, first_transition: TransitionField) -> TransitionIndex {
        TransitionIndex {
            input_symbol: Some(input),
            first_transition_index: Some(first_transition)
        }
    }

    // Originally final()
    pub fn is_final(&self) -> bool {
        self.input_symbol == None &&
            self.first_transition_index != None
    }

    pub fn target(&self) -> TransitionTableIndex {
        match self.first_transition_index {
            Weight(_) => panic!("Got weight for transition index field"),
            TransitionTableIndex(i) => i
        }
    }

    pub fn final_weight(&self) -> Weight {
        match self.first_transition_index {
            Weight(w) => w,
            TransitionTableIndex(_) => panic!("Got transition index for weight field")
        }
    }

    // Was get_input
    pub fn input(&self) -> SymbolNumber {
        self.input_symbol
    }
}