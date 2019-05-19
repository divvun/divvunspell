use std::hash::{Hash, Hasher};
use lifeguard::{Recycled, Pool};

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

#[derive(Eq, Debug, Clone)]
pub struct TreeNode {
    pub string: Vec<SymbolNumber>,
    pub flag_state: FlagDiacriticState,
    pub weight: EqWeight,
    pub input_state: u32,
    pub mutator_state: TransitionTableIndex,
    pub lexicon_state: TransitionTableIndex,
}

impl std::cmp::PartialEq for TreeNode {
    // This equality implementation is purposely not entirely correct. It is much faster this way.
    // The idea is that the seen_nodes hashset has to do a lot less work, and even if we miss a bunch,
    // memory pressure is significantly lowers
    fn eq(&self, other: &TreeNode) -> bool {
        self.lexicon_state == other.lexicon_state &&
            self.mutator_state == other.mutator_state &&
            self.input_state == other.input_state &&
            self.string == other.string
    }
}

impl Hash for TreeNode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u32(self.input_state);
        state.write_u32(self.mutator_state);
        state.write_u32(self.lexicon_state);
        self.string.hash(state);
    }
}

impl lifeguard::Recycleable for TreeNode {
    fn new() -> Self {
        TreeNode {
            string: Vec::with_capacity(1),
            input_state: 0,
            mutator_state: 0,
            lexicon_state: 0,
            flag_state: vec![],
            weight: EqWeight(0.0),
        }
    }
    
    fn reset(&mut self) {
        // There is nothing done to reset it.
        // Implementers must reset any fields where used!
    }
}


impl lifeguard::InitializeWith<&TreeNode> for TreeNode {
    fn initialize_with(&mut self, source: &TreeNode) {
        self.string.truncate(0);
        self.flag_state.truncate(0);
        self.string.extend(&source.string);
        self.input_state = source.input_state;
        self.mutator_state = source.mutator_state;
        self.lexicon_state = source.lexicon_state;
        self.flag_state.extend(&source.flag_state);
        self.weight = source.weight;
    }
}

impl TreeNode {
    pub fn empty<'a>(pool: &'a Pool<TreeNode>, start_state: FlagDiacriticState) -> Recycled<'a, TreeNode> {
        pool.attach(TreeNode {
            string: vec![],
            input_state: 0,
            mutator_state: 0,
            lexicon_state: 0,
            flag_state: start_state,
            weight: EqWeight(0.0)
        })
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

    pub fn update_lexicon<'a>(&self, pool: &'a Pool<TreeNode>, transition: SymbolTransition) -> Recycled<'a, TreeNode> {
        let mut node = pool.new();
        node.string.truncate(0);
        node.string.extend(&self.string);

        match transition.symbol() {
            Some(value) if value != 0 => {
                node.string.push(value);
            }
            _ => {},
        }

        node.input_state = self.input_state;
        node.mutator_state = self.mutator_state;
        node.lexicon_state = transition.target().unwrap();
        node.flag_state.truncate(0);
        node.flag_state.extend(&self.flag_state);
        node.weight = EqWeight(self.weight.0 + transition.weight().unwrap());

        node
    }

    pub fn update_mutator<'a>(&self, pool: &'a Pool<TreeNode>, transition: SymbolTransition) -> Recycled<'a, TreeNode> {
        let mut node = pool.new();
        node.string.truncate(0);
        node.string.extend(&self.string);
        node.input_state = self.input_state;
        node.mutator_state = transition.target().unwrap();
        node.lexicon_state = self.lexicon_state;
        node.flag_state.truncate(0);
        node.flag_state.extend(&self.flag_state);
        node.weight = EqWeight(self.weight.0 + transition.weight().unwrap());
        node
    }

    pub fn update<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        output_symbol: SymbolNumber,
        next_input: Option<u32>,
        next_mutator: TransitionTableIndex,
        next_lexicon: TransitionTableIndex,
        weight: Weight,
    ) -> Recycled<'a, TreeNode> {
        let mut node = pool.new();
        node.string.truncate(0);
        node.string.extend(&self.string);

        if output_symbol != 0 {
            node.string.push(output_symbol);
        }

        node.mutator_state = next_mutator;
        node.lexicon_state = next_lexicon;
        node.flag_state.truncate(0);
        node.flag_state.extend(&self.flag_state);
        node.weight = EqWeight(self.weight.0 + weight);

        if let Some(input) = next_input {
            node.input_state = input;
        } else {
            node.input_state = self.input_state;
        }

        node
    }

    fn update_flag<'a>(&self, pool: &'a Pool<TreeNode>, feature: SymbolNumber, value: i16) -> Recycled<'a, TreeNode> {
        let mut node = pool.new();
        node.string.truncate(0);
        node.string.extend(&self.string);
        node.input_state = self.input_state;
        node.mutator_state = self.mutator_state;
        node.lexicon_state = self.lexicon_state;

        node.flag_state.truncate(0);
        node.flag_state.extend(&self.flag_state);
        node.flag_state[feature as usize] = value;

        node.weight = self.weight;

        node
    }

    pub fn apply_operation<'a>(&self, pool: &'a Pool<TreeNode>, op: &FlagDiacriticOperation) -> (bool, Recycled<'a, TreeNode>) {
        match op.operation {
            FlagDiacriticOperator::PositiveSet => (true, self.update_flag(pool, op.feature, op.value)),
            FlagDiacriticOperator::NegativeSet => (
                true,
                self.update_flag(pool, op.feature, -1 * op.value),
            ),
            FlagDiacriticOperator::Require => {
                let res = if op.value == 0 {
                    self.flag_state[op.feature as usize] != 0
                } else {
                    self.flag_state[op.feature as usize] == op.value
                };

                (res, pool.new_from(self))
            }
            FlagDiacriticOperator::Disallow => {
                let res = if op.value == 0 {
                    self.flag_state[op.feature as usize] == 0
                } else {
                    self.flag_state[op.feature as usize] != op.value
                };

                (res, pool.new_from(self))
            }
            FlagDiacriticOperator::Clear => (true, self.update_flag(pool, op.feature, 0)),
            FlagDiacriticOperator::Unification => {
                // if the feature is unset OR the feature is to this value already OR
                // the feature is negatively set to something else than this value
                let f = self.flag_state[op.feature as usize];

                if f == 0 || f == op.value || (f < 0 && f * -1 != op.value) {
                    (true, self.update_flag(pool, op.feature, op.value))
                } else {
                    (false, pool.new_from(self))
                }
            }
        }
    }
}
