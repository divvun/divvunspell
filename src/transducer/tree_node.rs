use lifeguard::{Pool, Recycled};
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

use super::symbol_transition::SymbolTransition;
use crate::types::{
    FlagDiacriticOperation, FlagDiacriticOperator, FlagDiacriticState, InputIndex, SymbolNumber,
    TransitionTableIndex, ValueNumber, Weight,
};

#[derive(Debug, Clone)]
pub(crate) struct TreeNode {
    pub(crate) lexicon_state: TransitionTableIndex,
    pub(crate) mutator_state: TransitionTableIndex,
    pub(crate) input_state: InputIndex,
    pub(crate) weight: Weight,
    pub(crate) flag_state: FlagDiacriticState,
    pub(crate) string: Vec<SymbolNumber>,
}

impl std::cmp::PartialEq for TreeNode {
    fn eq(&self, other: &TreeNode) -> bool {
        self.lexicon_state == other.lexicon_state
            && self.mutator_state == other.mutator_state
            && self.input_state == other.input_state
            && self.weight == other.weight
            && self.flag_state == other.flag_state
            && self.string == other.string
    }
}

impl std::cmp::Ord for TreeNode {
    #[allow(clippy::comparison_chain)]
    fn cmp(&self, other: &Self) -> Ordering {
        if self.weight < other.weight {
            Ordering::Less
        } else if self.weight > other.weight {
            Ordering::Greater
        } else {
            self.string.cmp(&other.string)
        }
    }
}

impl std::cmp::PartialOrd for TreeNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl std::cmp::Eq for TreeNode {}

impl Hash for TreeNode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.input_state.hash(state);
        self.mutator_state.hash(state);
        self.lexicon_state.hash(state);
    }
}

impl lifeguard::Recycleable for TreeNode {
    fn new() -> Self {
        TreeNode {
            string: Vec::with_capacity(1),
            input_state: InputIndex(0),
            mutator_state: TransitionTableIndex(0),
            lexicon_state: TransitionTableIndex(0),
            flag_state: vec![],
            weight: Weight(0.0),
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
            self.flag_state
                .extend_from_slice(&source.flag_state.as_slice());
        }

        self.weight = source.weight;
    }
}

impl TreeNode {
    #[inline(always)]
    pub fn empty<'a>(
        pool: &'a Pool<TreeNode>,
        start_state: FlagDiacriticState,
    ) -> Recycled<'a, TreeNode> {
        pool.attach(TreeNode {
            string: vec![],
            input_state: InputIndex(0),
            mutator_state: TransitionTableIndex(0),
            lexicon_state: TransitionTableIndex(0),
            flag_state: start_state,
            weight: Weight(0.0),
        })
    }

    #[inline(always)]
    pub fn weight(&self) -> Weight {
        self.weight
    }

    #[inline(always)]
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
            if value.0 != 0 {
                node.string.push(value);
            }
        }

        node.input_state = self.input_state;
        node.mutator_state = self.mutator_state;
        node.lexicon_state = transition.target().unwrap();

        if node.flag_state != self.flag_state {
            node.flag_state.truncate(0);
            node.flag_state
                .extend_from_slice(&self.flag_state.as_slice());
        }

        node.weight = self.weight + transition.weight().unwrap();

        node
    }

    #[inline(always)]
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
            node.flag_state
                .extend_from_slice(&self.flag_state.as_slice());
        }

        node.weight = self.weight + transition.weight().unwrap();
        node
    }

    #[inline(always)]
    pub fn update<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        output_symbol: SymbolNumber,
        next_input: Option<InputIndex>,
        next_mutator: TransitionTableIndex,
        next_lexicon: TransitionTableIndex,
        weight: Weight,
    ) -> Recycled<'a, TreeNode> {
        let mut node = pool.new();

        if node.string != self.string {
            node.string.truncate(0);
            node.string.extend(&self.string);
        }

        if output_symbol.0 != 0 {
            node.string.push(output_symbol);
        }

        node.mutator_state = next_mutator;
        node.lexicon_state = next_lexicon;

        if node.flag_state != self.flag_state {
            node.flag_state.truncate(0);
            node.flag_state
                .extend_from_slice(&self.flag_state.as_slice());
        }

        node.weight = self.weight + weight;

        if let Some(input) = next_input {
            node.input_state = input;
        } else {
            node.input_state = self.input_state;
        }

        node
    }

    #[inline(always)]
    fn update_flag<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        feature: SymbolNumber,
        value: ValueNumber,
        transition: &SymbolTransition,
    ) -> Recycled<'a, TreeNode> {
        let mut node = self.apply_transition(pool, transition);
        node.flag_state[feature.0 as usize] = value;
        node
    }

    #[inline(always)]
    pub fn apply_transition<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        transition: &SymbolTransition,
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
            node.flag_state
                .extend_from_slice(&self.flag_state.as_slice());
        }

        node.weight = self.weight + transition.weight().unwrap();
        node
    }

    #[inline(always)]
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
                Some(self.update_flag(pool, op.feature, op.value.invert(), transition))
            }
            FlagDiacriticOperator::Require => {
                let res = if op.value.0 == 0 {
                    self.flag_state[op.feature.0 as usize] != ValueNumber(0)
                } else {
                    self.flag_state[op.feature.0 as usize] == op.value
                };

                if res {
                    Some(self.apply_transition(pool, transition))
                } else {
                    None
                }
            }
            FlagDiacriticOperator::Disallow => {
                let res = if op.value.0 == 0 {
                    self.flag_state[op.feature.0 as usize] == ValueNumber(0)
                } else {
                    self.flag_state[op.feature.0 as usize] != op.value
                };

                if res {
                    Some(self.apply_transition(pool, transition))
                } else {
                    None
                }
            }
            FlagDiacriticOperator::Clear => {
                Some(self.update_flag(pool, op.feature, ValueNumber(0), transition))
            }
            FlagDiacriticOperator::Unification => {
                // if the feature is unset OR the feature is to this value already OR
                // the feature is negatively set to something else than this value
                let f = self.flag_state[op.feature.0 as usize];

                if f.0 == 0 || f == op.value || (f.0 < 0 && f.invert() != op.value) {
                    Some(self.update_flag(pool, op.feature, op.value, transition))
                } else {
                    None
                }
            }
        }
    }
}
