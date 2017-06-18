pub mod suggestion;

use std::cell::RefCell;
use std::collections::{BinaryHeap, BTreeMap};
use std::cmp::{Ordering};
use std::cmp::Ordering::Equal;

use transducer::Transducer;
use transducer::tree_node::TreeNode;
use speller::suggestion::Suggestion;
use transducer::symbol_transition::SymbolTransition;
use types::{SymbolNumber, Weight, FlagDiacriticOperation};

type OperationMap = BTreeMap<SymbolNumber, FlagDiacriticOperation>;

#[derive(Debug, Clone)]
pub struct Speller<'a> {
    mutator: Transducer<'a>,
    lexicon: Transducer<'a>,
    operations: OperationMap,
    alphabet_translator: Vec<SymbolNumber>
}

struct SpellerWorker<'a, 'b> where 'a: 'b {
    speller: &'b Speller<'a>,
    input: Vec<SymbolNumber>,
    nodes: RefCell<Vec<TreeNode>>
}

impl<'a, 'b> SpellerWorker<'a, 'b> where 'a: 'b {
    fn new(speller: &'b Speller<'a>) -> SpellerWorker<'a, 'b> {
        SpellerWorker {
            speller: speller,
            input: vec![],
            nodes: RefCell::new(vec![])
        }
    }

    fn lexicon_epsilons(&'b self, next_node: &TreeNode) {
        let lexicon: &'b Transducer<'a> = &self.speller.lexicon;
        let operations = self.speller.operations();

        if !lexicon.has_epsilons_or_flags(next_node.lexicon_state + 1) {
            return
        }

        let mut next = lexicon.next(next_node.lexicon_state, 0).unwrap();

        while let Some(transition) = lexicon.take_epsilons_and_flags(next) {
            // TODO: re-add weight limit checks
            if let Some(sym) = lexicon.transition_table().input_symbol(next) {
                if sym == 0 {
                    // TODO: handle mode (Correct appears to force epsilon here)
                    let mut nodes = self.nodes.borrow_mut();
                    nodes.push(next_node.update_lexicon(transition));
                } else if let Some(op) = operations.get(&sym) {
                    let (is_success, applied_node) = next_node.apply_operation(op);

                    if is_success {
                        let epsilon_transition = transition.clone_with_epsilon_target();
                        let mut nodes = self.nodes.borrow_mut();
                        nodes.push(applied_node.update_lexicon(epsilon_transition));
                    }
                }
            }

            next += 1;
        }
    }

    fn mutator_epsilons(&self, next_node: &TreeNode) {
        let mutator = self.speller.mutator();
        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();

        if !mutator.has_transitions(next_node.mutator_state + 1, Some(0)) {
            return
        }

        let mut next_m = mutator.next(next_node.mutator_state, 0).unwrap();

        while let Some(transition) = mutator.take_epsilons(next_m) {
            if let Some(0) = transition.symbol() {
                // TODO weight limit
                let mut nodes = self.nodes.borrow_mut();
                nodes.push(next_node.update_mutator(transition));
                next_m += 1;
                continue;
            }

            if let Some(sym) = transition.symbol() {
                let trans_sym = alphabet_translator[sym as usize];

                if !lexicon.has_transitions(next_node.lexicon_state + 1, Some(trans_sym)) {
                    // we have no regular transitions for this
                    if trans_sym >= lexicon.alphabet().initial_symbol_count() {
                        // this input was not originally in the alphabet, so unknown or identity
                        // may apply
                        if lexicon.has_transitions(next_node.lexicon_state + 1, lexicon.alphabet().unknown()) {
                            self.queue_lexicon_arcs(&next_node, lexicon.alphabet().unknown().unwrap(),
                                    transition.target().unwrap(), transition.weight().unwrap(), 0)
                        }

                        if lexicon.has_transitions(next_node.lexicon_state + 1, lexicon.alphabet().identity()) {
                            self.queue_lexicon_arcs(&next_node, lexicon.alphabet().identity().unwrap(),
                                    transition.target().unwrap(), transition.weight().unwrap(), 0)
                        }
                    }

                    next_m += 1;
                    continue;
                }

                self.queue_lexicon_arcs(&next_node, trans_sym,
                        transition.target().unwrap(), transition.weight().unwrap(), 0);
                next_m += 1;
            }
        }
    }

    pub fn queue_lexicon_arcs(&self, next_node: &TreeNode, input_sym: SymbolNumber, mutator_state: u32, mutator_weight: Weight, input_increment: i16) {
        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();

        let mut next = lexicon.next(next_node.lexicon_state, input_sym).unwrap();

        while let Some(noneps_trans) = lexicon.take_non_epsilons(next, input_sym) {
            if let Some(mut sym) = noneps_trans.symbol() {
                // TODO: wtf?
                if lexicon.alphabet().identity() == noneps_trans.symbol() {
                    sym = self.input[next_node.input_state as usize];
                }

                // TODO: weight limit
                // TODO: handle Correct mode
                let mut nodes = self.nodes.borrow_mut();
                nodes.push(next_node.update(
                    sym,
                    Some(next_node.input_state + input_increment as u32),
                    mutator_state,
                    noneps_trans.target().unwrap(),
                    noneps_trans.weight().unwrap()))
            }

            next += 1
        }
    }

    fn queue_mutator_arcs(&self, next_node: &TreeNode, input_sym: SymbolNumber) {
        let mutator = self.speller.mutator();
        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();

        let mut next_m = mutator.next(next_node.mutator_state, input_sym).unwrap();

        while let Some(transition) = mutator.take_non_epsilons(next_m, input_sym) {
            if let Some(0) = transition.symbol() {
                let mut nodes = self.nodes.borrow_mut();
                nodes.push(next_node.update(
                        0,
                        Some(next_node.input_state + 1),
                        transition.target().unwrap(),
                        next_node.lexicon_state,
                        transition.weight().unwrap()));
                next_m += 1;
                continue;
            }

            if let Some(sym) = transition.symbol() {
                let trans_sym = alphabet_translator[sym as usize];

                if !lexicon.has_transitions(next_node.lexicon_state + 1, Some(trans_sym)) {
                    if trans_sym >= lexicon.alphabet().initial_symbol_count() {
                        if lexicon.has_transitions(next_node.lexicon_state + 1, lexicon.alphabet().unknown()) {
                            self.queue_lexicon_arcs(&next_node, lexicon.alphabet().unknown().unwrap(),
                                    transition.target().unwrap(), transition.weight().unwrap(), 1);
                        }
                        if lexicon.has_transitions(next_node.lexicon_state + 1, lexicon.alphabet().identity()) {
                            self.queue_lexicon_arcs(&next_node, lexicon.alphabet().identity().unwrap(),
                                    transition.target().unwrap(), transition.weight().unwrap(), 1);
                        }
                    }
                    next_m += 1;
                    continue;
                }

                self.queue_lexicon_arcs(&next_node, trans_sym,
                        transition.target().unwrap(), transition.weight().unwrap(), 1);
                next_m += 1;
            }


            // TODO: weight limit

        }
    }

    fn consume_input(&self, next_node: &TreeNode) {
        let mutator = self.speller.mutator();
        let lexicon = self.speller.lexicon();
        let input_state = next_node.input_state as usize;

        if input_state >= self.input.len() {
            return;
        }

        let input_sym = self.input[input_state];

        if !mutator.has_transitions(next_node.mutator_state + 1, Some(input_sym)) {
            // we have no regular transitions for this
            if input_sym >= mutator.alphabet().initial_symbol_count() {
                if mutator.has_transitions(next_node.mutator_state + 1, mutator.alphabet().identity()) {
                    self.queue_mutator_arcs(&next_node, mutator.alphabet().identity().unwrap());
                }
                if mutator.has_transitions(next_node.mutator_state + 1, mutator.alphabet().unknown()) {
                    self.queue_mutator_arcs(&next_node, mutator.alphabet().unknown().unwrap());
                }
            }
        } else {
            self.queue_mutator_arcs(&next_node, input_sym);
        }
    }
}

impl<'a, 'b> Speller<'a> where 'a: 'b {
    pub fn new(mutator: Transducer<'a>, mut lexicon: Transducer<'a>) -> Speller<'a> {
        // TODO: review why this i16 -> u16 is happening
        let size = lexicon.alphabet().state_size() as i16;
        let alphabet_translator = lexicon.mut_alphabet().create_translator_from(&mutator);

        Speller {
            mutator: mutator,
            lexicon: lexicon,
            operations: BTreeMap::new(),
            alphabet_translator: alphabet_translator
        }
    }

    pub fn mutator(&'b self) -> &'b Transducer<'a> {
        &self.mutator
    }

    pub fn lexicon(&'b self) -> &'b Transducer<'a> {
        &self.lexicon
    }

    pub fn operations(&self) -> &OperationMap {
        &self.operations
    }

    fn alphabet_translator(&self) -> &Vec<SymbolNumber> {
        &self.alphabet_translator
    }

    // TODO: this passthrough function really doesn't need to exist surely
    // Rename to lexicon_state_size?
    fn state_size(&self) -> SymbolNumber {
        self.lexicon.alphabet().state_size()
    }

    // orig: init_input
    fn to_input_vec(&'b self, word: &str) -> Vec<SymbolNumber> {
        // TODO: refactor for when mutator is optional
        let key_table = self.mutator().alphabet().key_table();

        word.chars().filter_map(|ch| {
            let s = ch.to_string();
            key_table.iter().position(|x| x == &s)
        }).map(|x| x as u16).collect()
    }

    pub fn test(&self) -> String {
        String::from("Hello")
    }

    // Known as Speller::correct in C++
    pub fn suggest(&'b self, word: &str) -> Vec<String> {
        let lexicon = self.lexicon();
        let mutator = self.mutator();

        let mut worker = SpellerWorker::new(&self);

        let start_node = TreeNode::empty(vec![self.state_size() as i16, 0]);
        let mut nodes = worker.nodes.borrow_mut();
        nodes.push(start_node);

        let mut corrections = BTreeMap::<String, Weight>::new();
        let mut input = self.to_input_vec(word);

        while nodes.len() > 0 {
            let next_node = nodes.pop().unwrap();

            worker.lexicon_epsilons(&next_node);
            worker.mutator_epsilons(&next_node);

            if next_node.input_state as usize != input.len() {
                worker.consume_input(&next_node);
                continue;
            }

            let m_final = self.mutator().is_final(next_node.mutator_state);
            let l_final = self.lexicon().is_final(next_node.lexicon_state);

            if !(m_final && l_final) {
                continue;
            }

            let weight = next_node.weight +
                self.lexicon().final_weight(next_node.lexicon_state).unwrap() +
                self.mutator().final_weight(next_node.mutator_state).unwrap();

            // if weight > limit { }
            let key_table = self.lexicon().alphabet().key_table();
            let string: String = next_node.string.iter().map(|&s| key_table[s as usize].to_string()).collect();

            let entry = corrections.entry(string).or_insert(weight);

            if *entry > weight {
                *entry = weight;
            }
        }

        let mut c: Vec<StringWeightPair> = corrections
            .into_iter()
            .map(|x| StringWeightPair(x.0, x.1))
            .collect();

        c.sort();

        c.into_iter().map(|x| x.0).collect()
    }
}

struct StringWeightPair(String, Weight);

impl Eq for StringWeightPair {}

impl Ord for StringWeightPair {
    fn cmp(&self, other: &StringWeightPair) -> Ordering {
        self.1.partial_cmp(&other.1).unwrap_or(Equal)
    }
}

impl PartialOrd for StringWeightPair {
    fn partial_cmp(&self, other: &StringWeightPair) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for StringWeightPair {
    fn eq(&self, other: &StringWeightPair) -> bool {
        self.1 == other.1
    }
}

impl<'a> Drop for Speller<'a> {
    fn drop(&mut self) {
        println!("Dropped: {:?}", self);
    }
}
