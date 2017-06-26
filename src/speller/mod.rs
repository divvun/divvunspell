pub mod suggestion;

use std::cell::RefCell;
use std::collections::{BinaryHeap, BTreeMap};
use std::cmp::{Ordering};
use std::cmp::Ordering::Equal;

use transducer::Transducer;
use transducer::tree_node::TreeNode;
use speller::suggestion::Suggestion;
use transducer::symbol_transition::SymbolTransition;
use types::{SymbolNumber, Weight, FlagDiacriticOperation, SpellerWorkerMode};
use std::rc::Rc;

type OperationMap = BTreeMap<SymbolNumber, FlagDiacriticOperation>;

#[derive(Debug)]
pub struct Speller<'data> {
    mutator: Transducer<'data>,
    lexicon: Transducer<'data>,
    operations: OperationMap,
    alphabet_translator: Vec<SymbolNumber>
}

struct SpellerWorker<'data, 'a> where 'data: 'a {
    speller: &'a Speller<'data>,
    input: Vec<SymbolNumber>,
    nodes: Rc<RefCell<Vec<TreeNode>>>,
    mode: SpellerWorkerMode
}

impl<'data, 'a> SpellerWorker<'data, 'a> where 'data: 'a {
    fn new(speller: &'a Speller<'data>, mode: SpellerWorkerMode, input: Vec<SymbolNumber>) -> SpellerWorker<'data, 'a> {
        SpellerWorker {
            speller: speller,
            input: input,
            nodes: Rc::new(RefCell::new(vec![])),
            mode: mode
        }
    }

    fn lexicon_epsilons(&'a self, next_node: &TreeNode) {
        let lexicon: &'a Transducer<'data> = &self.speller.lexicon;
        let operations = self.speller.operations();

        if !lexicon.has_epsilons_or_flags(next_node.lexicon_state + 1) {
            println!("No epsilons or flags, bye!");
            return
        }

        println!("lexicon_eps next: {}", next_node.lexicon_state);
        let mut next = lexicon.next(next_node.lexicon_state, 0).unwrap();

        while let Some(transition) = lexicon.take_epsilons_and_flags(next) {
            if self.is_under_weight_limit(next_node.weight + transition.weight().unwrap()) {
                if let Some(sym) = lexicon.transition_table().input_symbol(next) {
                    if sym == 0 {
                        let mut nodes = self.nodes.borrow_mut();
                        if let SpellerWorkerMode::Correct = self.mode {
                            let epsilon_transition = transition.clone_with_epsilon_target();
                            nodes.push(next_node.update_lexicon(epsilon_transition));
                        } else {
                            nodes.push(next_node.update_lexicon(transition));
                        }
                    } else if let Some(op) = operations.get(&sym) {
                        let (is_success, applied_node) = next_node.apply_operation(op);

                        if is_success {
                            let mut nodes = self.nodes.borrow_mut();
                            let epsilon_transition = transition.clone_with_epsilon_target();
                            nodes.push(applied_node.update_lexicon(epsilon_transition));
                        }
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
            //println!("trans mut next: {}", next_m);
            //println!("{}", next_node.weight);

            if let Some(0) = transition.symbol() {
                if self.is_under_weight_limit(next_node.weight + transition.weight().unwrap()) {
                    let mut nodes = self.nodes.borrow_mut();
                    nodes.push(next_node.update_mutator(transition));
                }
                
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
            }

            next_m += 1;
        }
    }

    pub fn queue_lexicon_arcs(&self, next_node: &TreeNode, input_sym: SymbolNumber, mutator_state: u32, mutator_weight: Weight, input_increment: i16) {
        println!("next_node lexstate:{}", next_node.lexicon_state);

        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();

        let mut next = lexicon.next(next_node.lexicon_state, input_sym).unwrap();
        println!("next: {}", next);

        let identity = lexicon.alphabet().identity();

        while let Some(noneps_trans) = lexicon.take_non_epsilons(next, input_sym) {
            println!("noneps next: {:?}", &noneps_trans);
            if let Some(mut sym) = noneps_trans.symbol() {
                // TODO: wtf?
                if let Some(id) = identity {
                    if (sym == id) {
                        sym = self.input[next_node.input_state as usize];
                    }
                }

                //println!("{}: {} {} {} n:{}", next, sym, next_node.weight, mutator_weight, self.nodes.borrow().len());

                let next_sym = if let SpellerWorkerMode::Correct = self.mode {
                    input_sym
                } else {
                    sym
                };

                if self.is_under_weight_limit(next_node.weight + noneps_trans.weight().unwrap() + mutator_weight) {
                    let mut nodes = self.nodes.borrow_mut();
                    nodes.push(next_node.update(
                        next_sym,
                        Some(next_node.input_state + input_increment as u32),
                        mutator_state,
                        noneps_trans.target().unwrap(),
                        noneps_trans.weight().unwrap() + mutator_weight))
                }
            }

            next += 1
        }

        //println!("End lexicon arcs");
    }

    fn queue_mutator_arcs(&self, next_node: &TreeNode, input_sym: SymbolNumber) {
        //println!("Mutator arcs");
        let mutator = self.speller.mutator();
        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();

        let mut next_m = mutator.next(next_node.mutator_state, input_sym).unwrap();

        while let Some(transition) = mutator.take_non_epsilons(next_m, input_sym) {
            //println!("mut arc loop: {}", next_m);

            if let Some(0) = transition.symbol() {
                if self.is_under_weight_limit(next_node.weight + transition.weight().unwrap()) {
                    let mut nodes = self.nodes.borrow_mut();
                    nodes.push(next_node.update(
                            0,
                            Some(next_node.input_state + 1),
                            transition.target().unwrap(),
                            next_node.lexicon_state,
                            transition.weight().unwrap()));
                }
                
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
            }

            next_m += 1;


            // TODO: weight limit

        }
        println!("End mutator arcs");
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

        println!("finish consume input");
    }

    fn lexicon_consume(&self, next_node: &TreeNode) {
        let mutator = self.speller.mutator();
        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();
        let input_state = next_node.input_state as usize;

        if input_state >= self.input.len() {
            return
        }

        // TODO handle nullable mutator
        let input_sym = alphabet_translator[self.input[input_state as usize] as usize];
        let next_lexicon_state = next_node.lexicon_state + 1;

        if !lexicon.has_transitions(next_lexicon_state, Some(input_sym)) {
            // we have no regular transitions for this
            if input_sym >= lexicon.alphabet().initial_symbol_count() {
                if lexicon.has_transitions(next_lexicon_state, mutator.alphabet().identity()) {
                    self.queue_lexicon_arcs(&next_node, lexicon.alphabet().identity().unwrap(), next_node.mutator_state, 0.0, 1);
                }
                if lexicon.has_transitions(next_lexicon_state, mutator.alphabet().unknown()) {
                    self.queue_lexicon_arcs(&next_node, lexicon.alphabet().unknown().unwrap(), next_node.mutator_state, 0.0, 1);
                }
            }

            return;
        }   

        self.queue_lexicon_arcs(&next_node, input_sym, next_node.mutator_state, 0.0, 1);

        // unsigned int input_state = next_node.input_state;
        // if (input_state >= input.size()) {
        //     // no more input
        //     return;
        // }
        // SymbolNumber this_input;
        // if (mutator != NULL) {
        //     this_input = alphabet_translator[input[input_state]];
        // } else {
        //     // To support zhfst spellers without error models, we allow
        //     // for the case with plain lexicon symbols
        //     this_input = input[input_state];
        // }
        // if(!lexicon->has_transitions(
        //     next_node.lexicon_state + 1, this_input)) {
        //     // we have no regular transitions for this
        //     if (this_input >= lexicon->get_alphabet()->get_orig_symbol_count()) {
        //         // this input was not originally in the alphabet, so unknown or identity
        //         // may apply
        //         if (lexicon->get_unknown() != NO_SYMBOL &&
        //             lexicon->has_transitions(next_node.lexicon_state + 1,
        //                                     lexicon->get_unknown())) {
        //             queue_lexicon_arcs(lexicon->get_unknown(),
        //                             next_node.mutator_state,
        //                             0.0, 1);
        //         }
        //         if (lexicon->get_identity() != NO_SYMBOL &&
        //             lexicon->has_transitions(next_node.lexicon_state + 1,
        //                                     lexicon->get_identity())) {
        //             queue_lexicon_arcs(lexicon->get_identity(),
        //                             next_node.mutator_state,
        //                             0.0, 1);
        //         }
        //     }
        //     return;
        // }
        // queue_lexicon_arcs(this_input,
        //                 next_node.mutator_state, 0.0, 1);
    }

    fn is_under_weight_limit(&self, w: Weight) -> bool {
        w < 10.0
    }
}

impl<'data, 'a> Speller<'data> where 'data: 'a {
    pub fn new(mutator: Transducer<'data>, mut lexicon: Transducer<'data>) -> Speller<'data> {
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

    pub fn mutator(&'a self) -> &'a Transducer<'data> {
        &self.mutator
    }

    pub fn lexicon(&'a self) -> &'a Transducer<'data> {
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
    fn to_input_vec(&'a self, word: &str) -> Vec<SymbolNumber> {
        // TODO: refactor for when mutator is optional
        let key_table = self.mutator().alphabet().key_table();

        //println!("kt: {:?}; word: {}", key_table, word);

        word.chars().filter_map(|ch| {
            let s = ch.to_string();
            key_table.iter().position(|x| x == &s)
        }).map(|x| x as u16).collect()
    }

    // pub fn analyze(&'a self, word: &str) -> Vec<String> {
    //     unimplemented!()
    // }

    pub fn check(&'a self, word: &str) -> bool {
        let mut input = self.to_input_vec(word);
        let mut worker = SpellerWorker::new(&self, SpellerWorkerMode::Unknown, input);

        let start_node = TreeNode::empty(vec![self.state_size() as i16, 0]);
        worker.nodes.borrow_mut().push(start_node);

        while worker.nodes.borrow().len() > 0 {
            let next_node = worker.nodes.borrow_mut().pop().unwrap();

            if next_node.input_state as usize == worker.input.len() && 
                self.lexicon().is_final(next_node.lexicon_state) {
                return true;
            }

            println!("lexicon_epsilons");
            worker.lexicon_epsilons(&next_node);
            println!("lexicon_consume");
            worker.lexicon_consume(&next_node);
        }
        
        false
    }

    // Known as Speller::correct in C++
    pub fn suggest(&'a self, word: &str) -> Vec<String> {
        use std::io;

        println!("suggest");
        let mut input = self.to_input_vec(word);
        println!("{:?}", &input);
        let mut worker = SpellerWorker::new(&self, SpellerWorkerMode::Correct, input);

        let start_node = TreeNode::empty(vec![self.state_size() as i16, 0]);
        worker.nodes.borrow_mut().push(start_node);

        let mut corrections = BTreeMap::<String, Weight>::new();

        while worker.nodes.borrow().len() > 0 {
            //println!("Worker nodes: {}", worker.nodes.borrow().len());
            let next_node = worker.nodes.borrow_mut().pop().unwrap();
            println!("sugloop node: is:{}", next_node.input_state);

            if !worker.is_under_weight_limit(next_node.weight) {
            //    continue
            }

            println!("LEX");
            worker.lexicon_epsilons(&next_node);
            println!("MUT");
            worker.mutator_epsilons(&next_node);

            if next_node.input_state as usize == worker.input.len() {
                if self.mutator().is_final(next_node.mutator_state) && self.lexicon().is_final(next_node.lexicon_state) {
                    //panic!();
                    let key_table = self.lexicon().alphabet().key_table();
                    let string: String = next_node.string.iter().map(|&s| key_table[s as usize].to_string()).collect();

                    //println!("string: {}", string);

                    let weight = next_node.weight +
                        self.lexicon().final_weight(next_node.lexicon_state).unwrap() +
                        self.mutator().final_weight(next_node.mutator_state).unwrap();
                    let entry = corrections.entry(string).or_insert(weight);

                    if *entry > weight {
                        *entry = weight;
                    }
                }
            } else {
                worker.consume_input(&next_node);
            }

            // if next_node.input_state as usize != worker.input.len() {
            //     //println!("CONSUME");
            //     worker.consume_input(&next_node);
            //     continue;
            // }

            // let m_final = self.mutator().is_final(next_node.mutator_state);
            // let l_final = self.lexicon().is_final(next_node.lexicon_state);

            // if !(m_final && l_final) {
            //     println!("is not final!");
            //     continue;
            // }

            // let weight = next_node.weight +
            //     self.lexicon().final_weight(next_node.lexicon_state).unwrap() +
            //     self.mutator().final_weight(next_node.mutator_state).unwrap();

            // // if weight > limit { }
            // let key_table = self.lexicon().alphabet().key_table();
            // let string: String = next_node.string.iter().map(|&s| key_table[s as usize].to_string()).collect();

            // println!("string: {}", string);

            // let entry = corrections.entry(string).or_insert(weight);

            // if *entry > weight {
            //     *entry = weight;
            // }
        }

        println!("Here we go!");

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

impl<'data> Drop for Speller<'data> {
    fn drop(&mut self) {
        //println!("Dropped: {:?}", self);
    }
}
