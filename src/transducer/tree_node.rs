use lifeguard::{Pool, Recycled};
use std::hash::{Hash, Hasher};
use std::mem;
use std::borrow::Cow;

use super::symbol_transition::SymbolTransition;
use crate::types::{
    FlagDiacriticOperation, FlagDiacriticOperator, FlagDiacriticState, SymbolNumber,
    TransitionTableIndex, Weight,
};

#[derive(Debug, Clone, Copy)]
pub struct EqWeight(pub Weight);

impl std::cmp::PartialEq for EqWeight {
    fn eq(&self, other: &EqWeight) -> bool {
        self.0 == other.0
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

impl TreeNode {
    #[inline(always)]
    pub fn key(&self) -> TreeNode {
        self.clone()
    }
}

impl std::cmp::PartialEq for TreeNode {
    // This equality implementation is purposely not entirely correct. It is much faster this way.
    // The idea is that the seen_nodes hashset has to do a lot less work, and even if we miss a bunch,
    // memory pressure is significantly lowered
    fn eq(&self, other: &TreeNode) -> bool {
        self.lexicon_state == other.lexicon_state
            && self.mutator_state == other.mutator_state
            && self.input_state == other.input_state
            && self.string == other.string
    }
}

impl Hash for TreeNode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u32(self.input_state);
        state.write_u32(self.mutator_state);
        state.write_u32(self.lexicon_state);
        // self.string.hash(state);
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
        if self.string != source.string {
            self.string.truncate(0);
            self.string.extend(&source.string);
        }

        self.input_state = source.input_state;
        self.mutator_state = source.mutator_state;
        self.lexicon_state = source.lexicon_state;

        if self.flag_state != source.flag_state {
            self.flag_state.truncate(0);
            self.flag_state.extend(&source.flag_state);
        }

        self.weight = source.weight;
    }
}

impl TreeNode {
    pub fn empty<'a>(
        pool: &'a Pool<TreeNode>,
        start_state: FlagDiacriticState,
    ) -> Recycled<'a, TreeNode> {
        pool.attach(TreeNode {
            string: vec![],
            input_state: 0,
            mutator_state: 0,
            lexicon_state: 0,
            flag_state: start_state,
            weight: EqWeight(0.0),
        })
    }

    pub fn weight(&self) -> Weight {
        self.weight.0
    }

    pub fn flag_state(&self) -> &FlagDiacriticState {
        &self.flag_state
    }

    pub fn update_lexicon<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        transition: SymbolTransition,
    ) -> Recycled<'a, TreeNode> {
        let mut node = pool.new();

        if node.string != self.string {
            node.string.truncate(0);
            node.string.extend(&self.string);
        }

        if let Some(value) = transition.symbol() {
            if value != 0 {
                node.string.push(value);
            }
        }

        node.input_state = self.input_state;
        node.mutator_state = self.mutator_state;
        node.lexicon_state = transition.target().unwrap();

        if node.flag_state != self.flag_state {
            node.flag_state.truncate(0);
            node.flag_state.extend(&self.flag_state);
        }
        
        node.weight = EqWeight(self.weight.0 + transition.weight().unwrap());

        node
    }

    pub fn update_mutator<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        transition: SymbolTransition,
    ) -> Recycled<'a, TreeNode> {
        let mut node = pool.new();
        if node.string != self.string {
            node.string.truncate(0);
            node.string.extend(&self.string);
        }
        node.input_state = self.input_state;
        node.mutator_state = transition.target().unwrap();
        node.lexicon_state = self.lexicon_state;

        if node.flag_state != self.flag_state {
            node.flag_state.truncate(0);
            node.flag_state.extend(&self.flag_state);
        }
        
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
        
        if node.string != self.string {
            node.string.truncate(0);
            node.string.extend(&self.string);
        }

        if output_symbol != 0 {
            node.string.push(output_symbol);
        }

        node.mutator_state = next_mutator;
        node.lexicon_state = next_lexicon;

        if node.flag_state != self.flag_state {
            node.flag_state.truncate(0);
            node.flag_state.extend(&self.flag_state);
        }

        node.weight = EqWeight(self.weight.0 + weight);

        if let Some(input) = next_input {
            node.input_state = input;
        } else {
            node.input_state = self.input_state;
        }

        node
    }

    fn update_flag<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        feature: SymbolNumber,
        value: i16,
        transition: &SymbolTransition,
    ) -> Recycled<'a, TreeNode> {
        let mut node = self.apply_transition(pool, transition); //pool.new();

        // if node.string != self.string {
        //     node.string.truncate(0);
        //     node.string.extend(&self.string);
        // }

        // node.input_state = self.input_state;
        // node.mutator_state = self.mutator_state;
        // node.lexicon_state = transition.target().unwrap();

        // if node.flag_state != self.flag_state {
        //     node.flag_state.truncate(0);
        //     node.flag_state.extend(&self.flag_state);
        // }

        node.flag_state[feature as usize] = value;

        // node.weight = EqWeight(self.weight.0 + transition.weight().unwrap());

        node
    }

    pub fn apply_transition<'a>(
        &self, 
        pool: &'a Pool<TreeNode>,
        transition: &SymbolTransition
    ) -> Recycled<'a, TreeNode> {
        let mut node = pool.new();

        if node.string != self.string {
            node.string.truncate(0);
            node.string.extend(&self.string);
        }

        node.input_state = self.input_state;
        node.mutator_state = self.mutator_state;
        node.lexicon_state = transition.target().unwrap();

        if node.flag_state != self.flag_state {
            node.flag_state.truncate(0);
            node.flag_state.extend(&self.flag_state);
        }

        node.weight = EqWeight(self.weight.0 + transition.weight().unwrap());
        node
    }

    pub fn apply_operation<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        op: &FlagDiacriticOperation,
        transition: &SymbolTransition,
    ) -> Option<Recycled<'a, TreeNode>> {
        match op.operation {
            FlagDiacriticOperator::PositiveSet => {
                Some(self.update_flag(pool, op.feature, op.value, transition))
            }
            FlagDiacriticOperator::NegativeSet => {
                Some(self.update_flag(pool, op.feature, -1 * op.value, transition))
            }
            FlagDiacriticOperator::Require => {
                let res = if op.value == 0 {
                    self.flag_state[op.feature as usize] != 0
                } else {
                    self.flag_state[op.feature as usize] == op.value
                };

                if res {
                    Some(self.apply_transition(pool, transition))
                } else {
                    None
                }
            }
            FlagDiacriticOperator::Disallow => {
                let res = if op.value == 0 {
                    self.flag_state[op.feature as usize] == 0
                } else {
                    self.flag_state[op.feature as usize] != op.value
                };

                if res {
                    Some(self.apply_transition(pool, transition))
                } else {
                    None
                }
            }
            FlagDiacriticOperator::Clear => Some(self.update_flag(pool, op.feature, 0, transition)),
            FlagDiacriticOperator::Unification => {
                // if the feature is unset OR the feature is to this value already OR
                // the feature is negatively set to something else than this value
                let f = self.flag_state[op.feature as usize];

                if f == 0 || f == op.value || (f < 0 && f * -1 != op.value) {
                    Some(self.update_flag(pool, op.feature, op.value, transition))
                } else {
                    None
                }
            }
        }
    }
}
