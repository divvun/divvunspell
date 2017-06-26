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
    alphabet_translator: Vec<SymbolNumber>
}

struct SpellerWorker<'data, 'a> where 'data: 'a {
    speller: &'a Speller<'data>,
    input: Vec<SymbolNumber>,
    nodes: Rc<RefCell<Vec<TreeNode>>>,
    mode: SpellerWorkerMode
}

fn debug_incr(key: &'static str) {
    debug!("{}", key);
    use COUNTER;
    let mut c = COUNTER.lock().unwrap();
    let mut entry = c.entry(key).or_insert(0);
    *entry += 1;
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
        debug_incr("lexicon_epsilons");
        
        debug!("Begin lexicon epsilons");

        let lexicon: &'a Transducer<'data> = &self.speller.lexicon;
        let operations = lexicon.alphabet().operations();

        if !lexicon.has_epsilons_or_flags(next_node.lexicon_state + 1) {
            debug!("Has no epsilons or flags, returning");
            return
        }

        let mut next = lexicon.next(next_node.lexicon_state, 0).unwrap();

        while let Some(transition) = lexicon.take_epsilons_and_flags(next) {
            if self.is_under_weight_limit(next_node.weight + transition.weight().unwrap()) {
                if let Some(sym) = lexicon.transition_table().input_symbol(next) {
                    if sym == 0 {
                        let mut nodes = self.nodes.borrow_mut();
                        if let SpellerWorkerMode::Correct = self.mode {
                            let epsilon_transition = transition.clone_with_epsilon_symbol();
                            debug_incr("lexicon_epsilons push node epsilon_transition CORRECT MODE");
                            nodes.push(next_node.update_lexicon(epsilon_transition));
                        } else {
                            debug_incr("lexicon_epsilons push node transition");
                            nodes.push(next_node.update_lexicon(transition));
                        }
                    } else {
                        let operation = operations.get(&sym);

                        if let Some(op) = operation {
                            let (is_success, applied_node) = next_node.apply_operation(op);

                            debug_incr(if is_success { "is_success" } else { "isnt_success" });

                            if is_success {
                                let mut nodes = self.nodes.borrow_mut();
                                let epsilon_transition = transition.clone_with_epsilon_symbol();
                                debug_incr("lexicon_epsilons push node cloned with eps target epsilon_transition");
                                nodes.push(applied_node.update_lexicon(epsilon_transition));
                            }
                        }
                    }
                }
            }

            next += 1;
        }

        debug!("lexicon epsilons, nodes length: {}", self.nodes.borrow().len());
    }

    fn mutator_epsilons(&self, next_node: &TreeNode) {
        debug_incr("mutator_epsilons");
        // debug!("Begin mutator epsilons");

        let mutator = self.speller.mutator();
        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();

        if !mutator.has_transitions(next_node.mutator_state + 1, Some(0)) {
            debug!("Mutator has no transitions, skipping");
            return
        }

        let mut next_m = mutator.next(next_node.mutator_state, 0).unwrap();

        while let Some(transition) = mutator.take_epsilons(next_m) {
            //debug!("trans mut next: {}", next_m);
            //debug!("{}", next_node.weight);
            // debug!("Current taken epsilon: {}", next_m);

            if let Some(0) = transition.symbol() {
                if self.is_under_weight_limit(next_node.weight + transition.weight().unwrap()) {
                    let mut nodes = self.nodes.borrow_mut();
                    debug_incr("mutator_epsilons push node update_mutator transition");
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
                            debug_incr("qla unknown mutator_eps");
                            self.queue_lexicon_arcs(&next_node, lexicon.alphabet().unknown().unwrap(),
                                    transition.target().unwrap(), transition.weight().unwrap(), 0)
                        }

                        if lexicon.has_transitions(next_node.lexicon_state + 1, lexicon.alphabet().identity()) {
                            debug_incr("qla identity mutator_eps");
                            self.queue_lexicon_arcs(&next_node, lexicon.alphabet().identity().unwrap(),
                                    transition.target().unwrap(), transition.weight().unwrap(), 0)
                        }
                    }

                    next_m += 1;
                    continue;
                }

                debug_incr("qla alpha_trans mutator_eps");
                self.queue_lexicon_arcs(&next_node, trans_sym,
                        transition.target().unwrap(), transition.weight().unwrap(), 0);
            }

            next_m += 1;
        }

        debug!("mutator epsilons, nodes length: {}", self.nodes.borrow().len());

        // debug!("End mutator epsilons");
    }

    pub fn queue_lexicon_arcs(&self, next_node: &TreeNode, input_sym: SymbolNumber, mutator_state: u32, mutator_weight: Weight, input_increment: i16) {
        debug!("Begin queue lexicon arcs");
        //debug!("next_node lexstate:{}", next_node.lexicon_state);

        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();

        let mut next = lexicon.next(next_node.lexicon_state, input_sym).unwrap();
        //debug!("next: {}", next);

        let identity = lexicon.alphabet().identity();

        while let Some(noneps_trans) = lexicon.take_non_epsilons(next, input_sym) {
            //debug!("noneps next: {:?}", &noneps_trans);
            debug!("qla noneps input_sym:{}, next: {}, t:{} s:{} w:{}", input_sym, next, noneps_trans.target().unwrap(), noneps_trans.symbol().unwrap(), noneps_trans.weight().unwrap());

            if let Some(mut sym) = noneps_trans.symbol() {
                // TODO: wtf?
                if let Some(id) = identity {
                    if sym == id {
                        sym = self.input[next_node.input_state as usize];
                    }
                }

                //debug!("{}: {} {} {} n:{}", next, sym, next_node.weight, mutator_weight, self.nodes.borrow().len());

                let next_sym = match &self.mode {
                    Correct => input_sym,
                    _ => sym
                };

                if self.is_under_weight_limit(next_node.weight + noneps_trans.weight().unwrap() + mutator_weight) {
                    let mut nodes = self.nodes.borrow_mut();
                    debug_incr("queue_lexicon_arcs push node update");
                    nodes.push(next_node.update(
                        next_sym,
                        Some(next_node.input_state + input_increment as u32),
                        mutator_state,
                        noneps_trans.target().unwrap(),
                        noneps_trans.weight().unwrap() + mutator_weight))
                }
            }

            next += 1;
            debug!("qla noneps NEXT input_sym:{}, next: {}", input_sym, next);
        }

        debug!("qla, nodes length: {}", self.nodes.borrow().len());
        debug!("--- qla noneps end ---");

        //debug!("End lexicon arcs");
    }

    fn queue_mutator_arcs(&self, next_node: &TreeNode, input_sym: SymbolNumber) {
        //debug!("Mutator arcs");
        let mutator = self.speller.mutator();
        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();

        let mut next_m = mutator.next(next_node.mutator_state, input_sym).unwrap();

        while let Some(transition) = mutator.take_non_epsilons(next_m, input_sym) {
            //debug!("mut arc loop: {}", next_m);

            if let Some(0) = transition.symbol() {
                if self.is_under_weight_limit(next_node.weight + transition.weight().unwrap()) {
                    let mut nodes = self.nodes.borrow_mut();
                    debug_incr("queue_mutator_arcs push node update");
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
                            debug_incr("qla unknown qma");
                            self.queue_lexicon_arcs(&next_node, lexicon.alphabet().unknown().unwrap(),
                                    transition.target().unwrap(), transition.weight().unwrap(), 1);
                        }
                        if lexicon.has_transitions(next_node.lexicon_state + 1, lexicon.alphabet().identity()) {
                            debug_incr("qla identity qma");
                            self.queue_lexicon_arcs(&next_node, lexicon.alphabet().identity().unwrap(),
                                    transition.target().unwrap(), transition.weight().unwrap(), 1);
                        }
                    }
                    next_m += 1;
                    continue;
                }

                debug_incr("qla alpha_trans qma");
                self.queue_lexicon_arcs(&next_node, trans_sym,
                        transition.target().unwrap(), transition.weight().unwrap(), 1);
            }

            next_m += 1;


            // TODO: weight limit

        }

        debug!("qma, nodes length: {}", self.nodes.borrow().len());
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

        debug!("finish consume input");
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
    }

    fn is_under_weight_limit(&self, w: Weight) -> bool {
        use std::f32;
        w < f32::MAX
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
            alphabet_translator: alphabet_translator
        }
    }

    pub fn mutator(&'a self) -> &'a Transducer<'data> {
        &self.mutator
    }

    pub fn lexicon(&'a self) -> &'a Transducer<'data> {
        &self.lexicon
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

        //debug!("kt: {:?}; word: {}", key_table, word);

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

        let start_node = TreeNode::empty(vec![0; self.state_size() as usize]);
        worker.nodes.borrow_mut().push(start_node);

        while worker.nodes.borrow().len() > 0 {
            let next_node = worker.nodes.borrow_mut().pop().unwrap();

            if next_node.input_state as usize == worker.input.len() && 
                self.lexicon().is_final(next_node.lexicon_state) {
                return true;
            }

            debug!("lexicon_epsilons");
            worker.lexicon_epsilons(&next_node);
            debug!("lexicon_consume");
            worker.lexicon_consume(&next_node);
        }
        
        false
    }

    // Known as Speller::correct in C++
    pub fn suggest(&'a self, word: &str) -> Vec<Suggestion> {
        let mut input = self.to_input_vec(word);
        let mut worker = SpellerWorker::new(&self, SpellerWorkerMode::Correct, input);

        let start_node = TreeNode::empty(vec![0; self.state_size() as usize]);
        worker.nodes.borrow_mut().push(start_node);

        let mut corrections = BTreeMap::<String, Weight>::new();

        while worker.nodes.borrow().len() > 0 {
            debug_incr("Worker node loop count");

            let next_node = worker.nodes.borrow_mut().pop().unwrap();
            debug!("Suggest loop");
            debug!("{:?}", next_node);

            debug!("sugloop next_node: is:{} w:{} ms:{} ls:{}", next_node.input_state, next_node.weight, next_node.mutator_state, next_node.lexicon_state);

            if !worker.is_under_weight_limit(next_node.weight) {
            //    continue
            }

            worker.lexicon_epsilons(&next_node);
            worker.mutator_epsilons(&next_node);

            if next_node.input_state as usize == worker.input.len() {
                debug_incr("input_state eq input size");
                debug!("is_final ms:{} ls:{}", next_node.mutator_state, next_node.lexicon_state);
                if self.mutator().is_final(next_node.mutator_state) && self.lexicon().is_final(next_node.lexicon_state) {
                    debug_incr("is_final");

                    let key_table = self.lexicon().alphabet().key_table();
                    let string: String = next_node.string.iter().map(|&s| key_table[s as usize].to_string()).collect();

                    //debug!("string: {}", string);

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
        }

        debug!("Here we go!");

        let mut c: Vec<Suggestion> = corrections
            .into_iter()
            .map(|x| Suggestion::new(x.0, x.1))
            .collect();

        c.sort();

        c
    }
}

