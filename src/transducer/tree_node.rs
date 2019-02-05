use std::hash::{Hash, Hasher};

use crate::types::{TransitionTableIndex, SymbolNumber, FlagDiacriticState, FlagDiacriticOperator,
            FlagDiacriticOperation, Weight};
use super::symbol_transition::SymbolTransition;

#[derive(Debug, Clone, Copy)]
pub struct EqWeight(pub Weight);

impl std::cmp::PartialEq for EqWeight {
    fn eq(&self, other: &EqWeight) -> bool {
        self.0.is_finite() && other.0.is_finite() && self.0 == other.0
    }
}

impl Hash for EqWeight {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u32(unsafe { std::mem::transmute::<f32, u32>(self.0) });
    }
}

impl std::cmp::Eq for EqWeight {}

#[derive(PartialEq, Eq, Debug, Clone, Hash)]
pub struct TreeNode {
    pub string: Vec<SymbolNumber>,
    pub flag_state: FlagDiacriticState,
    pub weight: EqWeight,
    pub input_state: u32,
    pub mutator_state: TransitionTableIndex,
    pub lexicon_state: TransitionTableIndex,
}

impl TreeNode {
    pub fn empty(start_state: FlagDiacriticState) -> TreeNode {
        TreeNode {
            string: Vec::with_capacity(1),
            input_state: 0,
            mutator_state: 0,
            lexicon_state: 0,
            flag_state: start_state,
            weight: EqWeight(0.0),
        }
    }

    pub fn weight(&self) -> Weight {
        self.weight.0
    }

    pub fn flag_state(&self) -> &FlagDiacriticState {
        &self.flag_state
    }

    pub fn update_lexicon_mut(&mut self, transition: SymbolTransition) {
        if let Some(value) = transition.symbol() {
            if value != 0 {
                self.string.push(value);
            }
        };

        self.lexicon_state = transition.target().unwrap();
        self.weight = EqWeight(self.weight.0 + transition.weight().unwrap());
    }

    pub fn update_lexicon(&self, transition: SymbolTransition) -> TreeNode {
        let string = match transition.symbol() {
            Some(value) if value != 0 => {
                let mut string = Vec::with_capacity(self.string.len() + 1);
                string.extend(&self.string);
                string.push(value);
                string
            }
            _ => self.string.clone(),
        };

        TreeNode {
            string: string,
            input_state: self.input_state,
            mutator_state: self.mutator_state,
            lexicon_state: transition.target().unwrap(),
            flag_state: self.flag_state.clone(),
            weight: EqWeight(self.weight.0 + transition.weight().unwrap())
        }
    }

    pub fn update_mutator(&self, transition: SymbolTransition) -> TreeNode {
        TreeNode {
            string: self.string.clone(),
            input_state: self.input_state,
            mutator_state: transition.target().unwrap(),
            lexicon_state: self.lexicon_state,
            flag_state: self.flag_state.clone(),
            weight: EqWeight(self.weight.0 + transition.weight().unwrap())
        }
    }

    pub fn update(
        &self,
        output_symbol: SymbolNumber,
        next_input: Option<u32>,
        next_mutator: TransitionTableIndex,
        next_lexicon: TransitionTableIndex,
        weight: Weight,
    ) -> TreeNode {
        let string = if output_symbol != 0 {
            let mut string = Vec::with_capacity(self.string.len() + 1);
            string.extend(&self.string);
            string.push(output_symbol);
            string
        } else {
            self.string.clone()
        };

        let mut node = TreeNode {
            string: string,
            input_state: self.input_state,
            mutator_state: next_mutator,
            lexicon_state: next_lexicon,
            flag_state: self.flag_state.clone(),
            weight: EqWeight(self.weight.0 + weight),
            ..self.clone()
        };

        if let Some(input) = next_input {
            node.input_state = input;
        }

        node
    }

    fn update_flag(&self, feature: SymbolNumber, value: i16) -> TreeNode {
        let mut vec = self.flag_state.clone();

        vec[feature as usize] = value;

        TreeNode {
            string: self.string.clone(),
            input_state: self.input_state,
            mutator_state: self.mutator_state,
            lexicon_state: self.lexicon_state,
            flag_state: vec,
            weight: self.weight
        }
    }

    pub fn apply_operation(&self, op: &FlagDiacriticOperation) -> (bool, TreeNode) {
        match op.operation {
            FlagDiacriticOperator::PositiveSet => (true, self.update_flag(op.feature, op.value)),
            FlagDiacriticOperator::NegativeSet => (
                true,
                self.update_flag(op.feature, -1 * op.value),
            ),
            FlagDiacriticOperator::Require => {
                let res = if op.value == 0 {
                    self.flag_state[op.feature as usize] != 0
                } else {
                    self.flag_state[op.feature as usize] == op.value
                };

                (res, self.clone())
            }
            FlagDiacriticOperator::Disallow => {
                let res = if op.value == 0 {
                    self.flag_state[op.feature as usize] == 0
                } else {
                    self.flag_state[op.feature as usize] != op.value
                };

                (res, self.clone())
            }
            FlagDiacriticOperator::Clear => (true, self.update_flag(op.feature, 0)),
            FlagDiacriticOperator::Unification => {
                // if the feature is unset OR the feature is to this value already OR
                // the feature is negatively set to something else than this value
                let f = self.flag_state[op.feature as usize];

                if f == 0 || f == op.value || (f < 0 && f * -1 != op.value) {
                    (true, self.update_flag(op.feature, op.value))
                } else {
                    (false, self.clone())
                }
            }
        }
    }
}
