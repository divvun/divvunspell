//! Search nodes, and the per-search tables their output strings and flag
//! states live in.
//!
//! A node names its output string and its flag state by number rather than
//! owning them. Both tables intern: a string is its parent string and one more
//! symbol, looked up so that equal strings get the same number, and a flag
//! state is looked up whole. Two nodes therefore carry equal strings exactly
//! when they carry equal numbers, and likewise for flag states, so everything
//! that compares them — the search's visited set, the corrections it collects
//! — sees what it saw when each node held its own copies. A node is a few
//! words that copy for nothing, and making a child costs one table lookup when
//! it writes a symbol instead of copying the whole string.

use std::cell::RefCell;
use std::hash::BuildHasher;

use hashbrown::{HashMap, HashTable};

use super::symbol_transition::SymbolTransition;
use crate::types::{
    FlagDiacriticOperation, FlagDiacriticOperator, InputIndex, SymbolNumber, TransitionTableIndex,
    ValueNumber, Weight,
};

/// The string with no symbols, and the flag state with every feature unset.
const ROOT: u32 = 0;

/// Output strings as a tree: string `i` is string `parent[i]` with
/// `symbol[i]` appended. String 0 is the empty string.
struct Strings {
    parent: Vec<u32>,
    symbol: Vec<SymbolNumber>,
    /// `(parent << 16) | symbol` to the string it makes.
    children: HashMap<u64, u32>,
}

/// Flag states, `width` values each, concatenated. State 0 has every feature
/// unset.
struct Flags {
    width: usize,
    values: Vec<ValueNumber>,
    index: HashTable<u32>,
    hasher: hashbrown::DefaultHashBuilder,
    /// The state being built, so a lookup never allocates.
    scratch: Vec<ValueNumber>,
    /// `(state << 32) | (feature << 16) | value` to the state it makes, so a
    /// change made before costs one integer lookup.
    changes: HashMap<u64, u32>,
}

/// The tables one search's nodes number their strings and flag states in.
pub(crate) struct NodeTables {
    strings: RefCell<Strings>,
    flags: RefCell<Flags>,
}

impl NodeTables {
    /// Tables holding the empty string and the unset flag state, for flag
    /// states of `width` features.
    pub(crate) fn new(width: usize) -> NodeTables {
        let mut flags = Flags {
            width,
            values: vec![ValueNumber::ZERO; width],
            index: HashTable::new(),
            hasher: hashbrown::DefaultHashBuilder::default(),
            scratch: Vec::with_capacity(width),
            changes: HashMap::new(),
        };
        let hash = flags.hasher.hash_one(&flags.values[..]);
        flags.index.insert_unique(hash, ROOT, |_| hash);
        NodeTables {
            strings: RefCell::new(Strings {
                parent: vec![ROOT],
                symbol: vec![SymbolNumber::ZERO],
                children: HashMap::new(),
            }),
            flags: RefCell::new(flags),
        }
    }

    /// Empty the tables for another search, keeping their capacity.
    pub(crate) fn reset(&mut self, width: usize) {
        let strings = self.strings.get_mut();
        strings.parent.truncate(1);
        strings.symbol.truncate(1);
        strings.children.clear();
        let flags = self.flags.get_mut();
        flags.width = width;
        flags.values.clear();
        flags.values.resize(width, ValueNumber::ZERO);
        flags.index.clear();
        flags.changes.clear();
        let hash = flags.hasher.hash_one(&flags.values[..]);
        flags.index.insert_unique(hash, ROOT, |_| hash);
    }

    /// How many strings the tables have room for.
    pub(crate) fn capacity(&self) -> usize {
        self.strings.borrow().children.capacity()
    }

    /// The number of `string` with `symbol` appended.
    #[inline]
    fn push(&self, string: u32, symbol: SymbolNumber) -> u32 {
        let mut strings = self.strings.borrow_mut();
        let key = ((string as u64) << 16) | symbol.0 as u64;
        if let Some(&id) = strings.children.get(&key) {
            return id;
        }
        let id = strings.parent.len() as u32;
        strings.parent.push(string);
        strings.symbol.push(symbol);
        strings.children.insert(key, id);
        id
    }

    /// The symbols of string `string`.
    pub(crate) fn symbols(&self, string: u32) -> Vec<SymbolNumber> {
        let strings = self.strings.borrow();
        let mut out = Vec::new();
        let mut at = string;
        while at != ROOT {
            out.push(strings.symbol[at as usize]);
            at = strings.parent[at as usize];
        }
        out.reverse();
        out
    }

    /// The value of `feature` in flag state `flags`.
    #[inline]
    fn flag(&self, flags: u32, feature: SymbolNumber) -> ValueNumber {
        let table = self.flags.borrow();
        table.values[flags as usize * table.width + feature.0 as usize]
    }

    /// The number of flag state `flags` with `feature` set to `value`.
    fn with_flag(&self, flags: u32, feature: SymbolNumber, value: ValueNumber) -> u32 {
        let mut table = self.flags.borrow_mut();
        let change = ((flags as u64) << 32) | ((feature.0 as u64) << 16) | value.0 as u16 as u64;
        if let Some(&id) = table.changes.get(&change) {
            return id;
        }
        let id = Self::find_or_add(&mut table, flags, feature, value);
        table.changes.insert(change, id);
        id
    }

    /// The number of flag state `flags` with `feature` set to `value`, looked
    /// up whole.
    fn find_or_add(
        table: &mut Flags,
        flags: u32,
        feature: SymbolNumber,
        value: ValueNumber,
    ) -> u32 {
        let Flags {
            width,
            values,
            index,
            hasher,
            scratch,
            ..
        } = table;
        let width = *width;
        scratch.clear();
        scratch.extend_from_slice(&values[flags as usize * width..][..width]);
        scratch[feature.0 as usize] = value;
        let hash = hasher.hash_one(&scratch[..]);
        if let Some(&id) = index.find(hash, |&id| {
            values[id as usize * width..][..width] == scratch[..]
        }) {
            return id;
        }
        let id = (values.len() / width) as u32;
        values.extend_from_slice(scratch);
        let hasher = &*hasher;
        index.insert_unique(hash, id, |&other| {
            hasher.hash_one(&values[other as usize * width..][..width])
        });
        id
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TreeNode {
    pub(crate) lexicon_state: TransitionTableIndex,
    pub(crate) mutator_state: TransitionTableIndex,
    pub(crate) input_state: InputIndex,
    pub(crate) weight: Weight,
    /// The error model's share of `weight`.
    ///
    /// Kept apart from the total so the caller can tell what the model charged
    /// for this correction from what the lexicon charged for the result. A
    /// whole-word entry authored in `words.default.txt` costs whatever its
    /// author wrote, however far the two strings are apart; reweighting keys
    /// off that rather than off the string distance.
    pub(crate) mutator_weight: Weight,
    /// The flag state, numbered in the search's [`NodeTables`].
    pub(crate) flags: u32,
    /// The output so far, numbered in the search's [`NodeTables`].
    pub(crate) string: u32,
}

impl TreeNode {
    /// The start node: no input read, no output written, every flag unset.
    #[inline(always)]
    pub fn empty() -> TreeNode {
        TreeNode {
            lexicon_state: TransitionTableIndex(0),
            mutator_state: TransitionTableIndex(0),
            input_state: InputIndex(0),
            weight: Weight(0.0),
            mutator_weight: Weight(0.0),
            flags: ROOT,
            string: ROOT,
        }
    }

    #[inline(always)]
    pub fn weight(&self) -> Weight {
        self.weight
    }

    #[inline(always)]
    pub fn update_lexicon(&self, tables: &NodeTables, transition: SymbolTransition) -> TreeNode {
        let string = match transition.symbol() {
            Some(value) if value.0 != 0 => tables.push(self.string, value),
            _ => self.string,
        };
        TreeNode {
            lexicon_state: transition.target().unwrap(),
            weight: self.weight + transition.weight().unwrap(),
            string,
            ..*self
        }
    }

    #[inline(always)]
    pub fn update_mutator(&self, target: TransitionTableIndex, weight: Weight) -> TreeNode {
        TreeNode {
            mutator_state: target,
            weight: self.weight + weight,
            mutator_weight: self.mutator_weight + weight,
            ..*self
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    pub fn update(
        &self,
        tables: &NodeTables,
        output_symbol: SymbolNumber,
        next_input: Option<InputIndex>,
        next_mutator: TransitionTableIndex,
        next_lexicon: TransitionTableIndex,
        weight: Weight,
        mutator_weight: Weight,
    ) -> TreeNode {
        let string = if output_symbol.0 != 0 {
            tables.push(self.string, output_symbol)
        } else {
            self.string
        };
        TreeNode {
            lexicon_state: next_lexicon,
            mutator_state: next_mutator,
            input_state: next_input.unwrap_or(self.input_state),
            weight: self.weight + weight,
            mutator_weight: self.mutator_weight + mutator_weight,
            flags: self.flags,
            string,
        }
    }

    #[inline(always)]
    fn update_flag(
        &self,
        tables: &NodeTables,
        feature: SymbolNumber,
        value: ValueNumber,
        transition: &SymbolTransition,
    ) -> TreeNode {
        TreeNode {
            flags: tables.with_flag(self.flags, feature, value),
            ..self.apply_transition(transition)
        }
    }

    #[inline(always)]
    pub fn apply_transition(&self, transition: &SymbolTransition) -> TreeNode {
        TreeNode {
            lexicon_state: transition.target().unwrap(),
            weight: self.weight + transition.weight().unwrap(),
            ..*self
        }
    }

    #[inline(always)]
    pub fn apply_operation(
        &self,
        tables: &NodeTables,
        op: &FlagDiacriticOperation,
        transition: &SymbolTransition,
    ) -> Option<TreeNode> {
        match op.operation {
            FlagDiacriticOperator::PositiveSet => {
                Some(self.update_flag(tables, op.feature, op.value, transition))
            }
            FlagDiacriticOperator::NegativeSet => {
                Some(self.update_flag(tables, op.feature, op.value.invert(), transition))
            }
            FlagDiacriticOperator::Require => {
                let f = tables.flag(self.flags, op.feature);
                let res = if op.value.0 == 0 {
                    f != ValueNumber(0)
                } else {
                    f == op.value
                };

                if res {
                    Some(self.apply_transition(transition))
                } else {
                    None
                }
            }
            FlagDiacriticOperator::Disallow => {
                let f = tables.flag(self.flags, op.feature);
                let res = if op.value.0 == 0 {
                    f == ValueNumber(0)
                } else {
                    f != op.value
                };

                if res {
                    Some(self.apply_transition(transition))
                } else {
                    None
                }
            }
            FlagDiacriticOperator::Clear => {
                Some(self.update_flag(tables, op.feature, ValueNumber(0), transition))
            }
            FlagDiacriticOperator::Unification => {
                // if the feature is unset OR the feature is to this value already OR
                // the feature is negatively set to something else than this value
                let f = tables.flag(self.flags, op.feature);

                if f.0 == 0 || f == op.value || (f.0 < 0 && f.invert() != op.value) {
                    Some(self.update_flag(tables, op.feature, op.value, transition))
                } else {
                    None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_strings_get_equal_numbers() {
        let tables = NodeTables::new(0);
        let (a, b, c) = (SymbolNumber(1), SymbolNumber(2), SymbolNumber(3));
        let ab = tables.push(tables.push(ROOT, a), b);
        let ab2 = tables.push(tables.push(ROOT, a), b);
        let ac = tables.push(tables.push(ROOT, a), c);
        assert_eq!(ab, ab2);
        assert_ne!(ab, ac);
        assert_eq!(tables.symbols(ab), vec![a, b]);
        assert_eq!(tables.symbols(ROOT), Vec::<SymbolNumber>::new());
    }

    #[test]
    fn equal_flag_states_get_equal_numbers() {
        let tables = NodeTables::new(3);
        let (f0, f2) = (SymbolNumber(0), SymbolNumber(2));
        let set = tables.with_flag(ROOT, f2, ValueNumber(5));
        assert_ne!(set, ROOT);
        assert_eq!(tables.with_flag(ROOT, f2, ValueNumber(5)), set);
        assert_eq!(tables.flag(set, f2), ValueNumber(5));
        assert_eq!(tables.flag(set, f0), ValueNumber(0));
        // Unsetting the feature again finds the start state.
        assert_eq!(tables.with_flag(set, f2, ValueNumber(0)), ROOT);
        let both = tables.with_flag(set, f0, ValueNumber(-1));
        let other_order = tables.with_flag(
            tables.with_flag(ROOT, f0, ValueNumber(-1)),
            f2,
            ValueNumber(5),
        );
        assert_eq!(other_order, both);
    }
}
