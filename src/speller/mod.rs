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

#[derive(Debug)]
pub struct Speller<'a> {
    mutator: Transducer<'a>,
    lexicon: Transducer<'a>,
    operations: OperationMap,
    alphabet_translator: Vec<SymbolNumber>
}

struct SpellerWorker<'a> {
    speller: &'a Speller<'a>,
    input: Vec<SymbolNumber>,
    nodes: RefCell<Vec<TreeNode>>
}

impl<'a> SpellerWorker<'a> {
    fn lexicon_epsilons(&self, next_node: &TreeNode) {
        let lexicon = self.speller.lexicon();
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
        unimplemented!();
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

        /*
        TransitionTableIndex next_m = mutator->next(next_node.mutator_state, input_sym);
        STransition mutator_i_s = mutator->take_non_epsilons(next_m, input_sym);
        while (mutator_i_s.symbol != NO_SYMBOL) {
            if (mutator_i_s.symbol == 0) {
                if (is_under_weight_limit(next_node.weight + mutator_i_s.weight)) {
                    queue.push_back(next_node.update(0, next_node.input_state + 1,
                                                    mutator_i_s.index,
                                                    next_node.lexicon_state,
                                                    mutator_i_s.weight));
                }
                ++next_m;
                mutator_i_s = mutator->take_non_epsilons(next_m, input_sym);
                continue;
            } else if (!lexicon->has_transitions(next_node.lexicon_state + 1, alphabet_translator[mutator_i_s.symbol])) {
                // we have no regular transitions for this
                if (alphabet_translator[mutator_i_s.symbol] >= lexicon->get_alphabet()->get_orig_symbol_count()) {
                    // this input was not originally in the alphabet, so unknown or identity
                    // may apply
                    if (lexicon->get_unknown() != NO_SYMBOL && lexicon->has_transitions(next_node.lexicon_state + 1,  lexicon->get_unknown())) {
                        queue_lexicon_arcs(lexicon->get_unknown(), mutator_i_s.index, mutator_i_s.weight, 1);
                    }
                    if (lexicon->get_identity() != NO_SYMBOL && lexicon->has_transitions(next_node.lexicon_state + 1, lexicon->get_identity())) {
                        queue_lexicon_arcs(lexicon->get_identity(), mutator_i_s.index, mutator_i_s.weight, 1);
                    }
                }
                ++next_m;
                mutator_i_s = mutator->take_non_epsilons(next_m, input_sym);
                continue;
            }
            queue_lexicon_arcs(alphabet_translator[mutator_i_s.symbol], mutator_i_s.index, mutator_i_s.weight, 1);
            ++next_m;
            mutator_i_s = mutator->take_non_epsilons(next_m, input_sym);
        }
        */
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

impl<'a> Speller<'a> {
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

    pub fn mutator(&self) -> &'a Transducer {
        &self.mutator
    }

    pub fn lexicon(&self) -> &'a Transducer {
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
    fn to_input_vec(word: &str) -> Vec<SymbolNumber> {
        unimplemented!()
    }

    // Known as Speller::correct in C++    
    pub fn suggest(&self, word: &str) -> Vec<String> {
        unimplemented!()
        // let correction_queue = BinaryHeap::<StringWeightPair>::new();
        // let start_node = TreeNode::empty(vec![self.state_size() as i16, 0]);

        // let mut nodes = self.nodes.borrow_mut();
        // nodes.clear();
        // nodes.push(start_node);

        // let corrections = BTreeMap::<String, Weight>::new();

        // let mut input = Speller::to_input_vec(word);
        
        // while nodes.len() > 0 {
        //     let next_node = nodes.pop().unwrap();
        //     let lexicon = self.lexicon();
        //     let mutator = self.mutator();
            
        //     Speller::lexicon_epsilons(lexicon, &self.operations.borrow(), &mut nodes, &next_node);
        //     Speller::mutator_epsilons(mutator, lexicon, &self.alphabet_translator, &mut nodes, &next_node);
        
        //     if next_node.input_state as usize != input.len() {
        //         self.consume_input(mutator, &next_node, &input);
        //         continue;
        //     }

        //     let m_final = mutator.is_final(next_node.mutator_state());
        //     let l_final = lexicon.is_final(next_node.lexicon_state());
            
        //     if !(m_final && l_final) {
        //         continue;
        //     }

        //     let weight = next_node.weight() + 
        //         lexicon.final_weight(next_node.lexicon_state()) + 
        //         mutator.final_weight(next_node.mutator_state());
            
        //     // if weight > limit { }

        //     let string = to_string(lexicon.key_table(), next_node.string());

        //     corrections.entry(string).or_insert(weight);

        //     if corrections[string] > weight {
        //         corrections[string] = weight;
        //     }
        // }

        // for (string, weight) in corrections {
        //     correction_queue.push(StringWeightPair(string, weight));
        // }

        // return correction_queue.into_sorted_vec();
    }
}

// TODO: what was I thinking here?
// fn to_string(key_table: &Vec<String>, symbols: &Vec<SymbolNumber>) -> String {
//     symbols.iter().map(|s| key_table[s]).collect()
// }

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
        // println!("Dropped: {:?}", self);
    }
}
