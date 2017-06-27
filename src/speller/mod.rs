pub mod suggestion;

use std::collections::{BinaryHeap, BTreeMap};
use std::cmp::Ordering;
use std::cmp::Ordering::Equal;

use transducer::Transducer;
use transducer::tree_node::TreeNode;
use speller::suggestion::Suggestion;
use transducer::symbol_transition::SymbolTransition;
use types::{SymbolNumber, Weight, FlagDiacriticOperation, SpellerWorkerMode};

type OperationMap = BTreeMap<SymbolNumber, FlagDiacriticOperation>;

#[derive(Debug)]
pub struct Speller<'data> {
    mutator: Transducer<'data>,
    lexicon: Transducer<'data>,
    alphabet_translator: Vec<SymbolNumber>,
}

struct SpellerWorker<'data, 'a>
where
    'data: 'a,
{
    speller: &'a Speller<'data>,
    input: Vec<SymbolNumber>,
    mode: SpellerWorkerMode,
}

fn debug_incr(key: &'static str) {
    debug!("{}", key);
    use COUNTER;
    let mut c = COUNTER.lock().unwrap();
    let mut entry = c.entry(key).or_insert(0);
    *entry += 1;
}

impl<'data, 'a> SpellerWorker<'data, 'a>
where
    'data: 'a,
{
    fn new(
        speller: &'a Speller<'data>,
        mode: SpellerWorkerMode,
        input: Vec<SymbolNumber>,
    ) -> SpellerWorker<'data, 'a> {
        SpellerWorker {
            speller: speller,
            input: input,
            mode: mode,
        }
    }

    fn lexicon_epsilons(&self, nodes: &mut Vec<TreeNode>, next_node: &TreeNode) {
        debug_incr("lexicon_epsilons");

        debug!("Begin lexicon epsilons");

        let lexicon = self.speller.lexicon();
        let operations = lexicon.alphabet().operations();

        if !lexicon.has_epsilons_or_flags(next_node.lexicon_state + 1) {
            debug!("Has no epsilons or flags, returning");
            return;
        }

        let mut next = lexicon.next(next_node.lexicon_state, 0).unwrap();

        while let Some(transition) = lexicon.take_epsilons_and_flags(next) {
            if self.is_under_weight_limit(next_node.weight + transition.weight().unwrap()) {
                if let Some(sym) = lexicon.transition_table().input_symbol(next) {
                    if sym == 0 {
                        if let SpellerWorkerMode::Correct = self.mode {
                            let epsilon_transition = transition.clone_with_epsilon_symbol();
                            debug_incr(
                                "lexicon_epsilons push node epsilon_transition CORRECT MODE",
                            );
                            nodes.push(next_node.update_lexicon(epsilon_transition));
                        } else {
                            debug_incr("lexicon_epsilons push node transition");
                            nodes.push(next_node.update_lexicon(transition));
                        }
                    } else {
                        let operation = operations.get(&sym);

                        if let Some(op) = operation {
                            let (is_success, applied_node) = next_node.apply_operation(op);

                            debug_incr(if is_success {
                                "is_success"
                            } else {
                                "isnt_success"
                            });

                            if is_success {
                                let epsilon_transition = transition.clone_with_epsilon_symbol();
                                debug_incr(
                                    "lexicon_epsilons push node cloned with eps target epsilon_transition",
                                );
                                nodes.push(applied_node.update_lexicon(epsilon_transition));
                            }
                        }
                    }
                }
            }

            next += 1;
        }

        debug!(
            "lexicon epsilons, nodes length: {}",
            nodes.len()
        );
    }

    fn mutator_epsilons(&self, nodes: &mut Vec<TreeNode>, next_node: &TreeNode) {
        debug_incr("mutator_epsilons");
        // debug!("Begin mutator epsilons");

        let mutator = self.speller.mutator();
        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();

        if !mutator.has_transitions(next_node.mutator_state + 1, Some(0)) {
            debug!("Mutator has no transitions, skipping");
            return;
        }

        let mut next_m = mutator.next(next_node.mutator_state, 0).unwrap();

        while let Some(transition) = mutator.take_epsilons(next_m) {
            //debug!("trans mut next: {}", next_m);
            //debug!("{}", next_node.weight);
            // debug!("Current taken epsilon: {}", next_m);

            if let Some(0) = transition.symbol() {
                if self.is_under_weight_limit(next_node.weight + transition.weight().unwrap()) {
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
                        if lexicon.has_transitions(
                            next_node.lexicon_state + 1,
                            lexicon.alphabet().unknown(),
                        ) {
                            debug_incr("qla unknown mutator_eps");
                            self.queue_lexicon_arcs(
                                nodes,
                                &next_node,
                                lexicon.alphabet().unknown().unwrap(),
                                transition.target().unwrap(),
                                transition.weight().unwrap(),
                                0,
                            )
                        }

                        if lexicon.has_transitions(
                            next_node.lexicon_state + 1,
                            lexicon.alphabet().identity(),
                        ) {
                            debug_incr("qla identity mutator_eps");
                            self.queue_lexicon_arcs(
                                nodes,
                                &next_node,
                                lexicon.alphabet().identity().unwrap(),
                                transition.target().unwrap(),
                                transition.weight().unwrap(),
                                0,
                            )
                        }
                    }

                    next_m += 1;
                    continue;
                }

                debug_incr("qla alpha_trans mutator_eps");
                self.queue_lexicon_arcs(
                    nodes,
                    &next_node,
                    trans_sym,
                    transition.target().unwrap(),
                    transition.weight().unwrap(),
                    0,
                );
            }

            next_m += 1;
        }

        debug!(
            "mutator epsilons, nodes length: {}",
            nodes.len()
        );

        // debug!("End mutator epsilons");
    }

    pub fn queue_lexicon_arcs(
        &self,
        nodes: &mut Vec<TreeNode>,
        next_node: &TreeNode,
        input_sym: SymbolNumber,
        mutator_state: u32,
        mutator_weight: Weight,
        input_increment: i16,
    ) {
        debug!("Begin queue lexicon arcs");
        //debug!("next_node lexstate:{}", next_node.lexicon_state);

        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();

        let mut next = lexicon.next(next_node.lexicon_state, input_sym).unwrap();
        //debug!("next: {}", next);

        let identity = lexicon.alphabet().identity();

        while let Some(noneps_trans) = lexicon.take_non_epsilons(next, input_sym) {
            //debug!("noneps next: {:?}", &noneps_trans);
            debug!(
                "qla noneps input_sym:{}, next: {}, t:{} s:{} w:{}",
                input_sym,
                next,
                noneps_trans.target().unwrap(),
                noneps_trans.symbol().unwrap(),
                noneps_trans.weight().unwrap()
            );

            if let Some(mut sym) = noneps_trans.symbol() {
                // TODO: wtf?
                if let Some(id) = identity {
                    if sym == id {
                        sym = self.input[next_node.input_state as usize];
                    }
                }

                //debug!("{}: {} {} {} n:{}", next, sym, next_node.weight, mutator_weight, nodes.len());

                let next_sym = match &self.mode {
                    Correct => input_sym,
                    _ => sym,
                };

                if self.is_under_weight_limit(
                    next_node.weight + noneps_trans.weight().unwrap() + mutator_weight,
                ) {
                    debug_incr("queue_lexicon_arcs push node update");
                    nodes.push(next_node.update(
                        next_sym,
                        Some(next_node.input_state + input_increment as u32),
                        mutator_state,
                        noneps_trans.target().unwrap(),
                        noneps_trans.weight().unwrap() + mutator_weight,
                    ))
                }
            }

            next += 1;
            debug!("qla noneps NEXT input_sym:{}, next: {}", input_sym, next);
        }

        debug!("qla, nodes length: {}", nodes.len());
        debug!("--- qla noneps end ---");

        //debug!("End lexicon arcs");
    }

    fn queue_mutator_arcs(&self, nodes: &mut Vec<TreeNode>, next_node: &TreeNode, input_sym: SymbolNumber) {
        //debug!("Mutator arcs");
        let mutator = self.speller.mutator();
        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();

        let mut next_m = mutator.next(next_node.mutator_state, input_sym).unwrap();

        while let Some(transition) = mutator.take_non_epsilons(next_m, input_sym) {
            //debug!("mut arc loop: {}", next_m);

            if let Some(0) = transition.symbol() {
                if self.is_under_weight_limit(next_node.weight + transition.weight().unwrap()) {
                    debug_incr("queue_mutator_arcs push node update");
                    nodes.push(next_node.update(
                        0,
                        Some(next_node.input_state + 1),
                        transition.target().unwrap(),
                        next_node.lexicon_state,
                        transition.weight().unwrap(),
                    ));
                }

                next_m += 1;
                continue;
            }

            if let Some(sym) = transition.symbol() {
                let trans_sym = alphabet_translator[sym as usize];

                if !lexicon.has_transitions(next_node.lexicon_state + 1, Some(trans_sym)) {
                    if trans_sym >= lexicon.alphabet().initial_symbol_count() {
                        if lexicon.has_transitions(
                            next_node.lexicon_state + 1,
                            lexicon.alphabet().unknown(),
                        ) {
                            debug_incr("qla unknown qma");
                            self.queue_lexicon_arcs(
                                nodes,
                                &next_node,
                                lexicon.alphabet().unknown().unwrap(),
                                transition.target().unwrap(),
                                transition.weight().unwrap(),
                                1,
                            );
                        }
                        if lexicon.has_transitions(
                            next_node.lexicon_state + 1,
                            lexicon.alphabet().identity(),
                        ) {
                            debug_incr("qla identity qma");
                            self.queue_lexicon_arcs(
                                nodes,
                                &next_node,
                                lexicon.alphabet().identity().unwrap(),
                                transition.target().unwrap(),
                                transition.weight().unwrap(),
                                1,
                            );
                        }
                    }
                    next_m += 1;
                    continue;
                }

                debug_incr("qla alpha_trans qma");
                self.queue_lexicon_arcs(
                    nodes,
                    &next_node,
                    trans_sym,
                    transition.target().unwrap(),
                    transition.weight().unwrap(),
                    1,
                );
            }

            next_m += 1;


            // TODO: weight limit

        }

        debug!("qma, nodes length: {}", nodes.len());
    }

    fn consume_input(&self, nodes: &mut Vec<TreeNode>, next_node: &TreeNode) {
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
                if mutator
                    .has_transitions(next_node.mutator_state + 1, mutator.alphabet().identity())
                {
                    self.queue_mutator_arcs(nodes, &next_node, mutator.alphabet().identity().unwrap());
                }
                if mutator
                    .has_transitions(next_node.mutator_state + 1, mutator.alphabet().unknown())
                {
                    self.queue_mutator_arcs(nodes, &next_node, mutator.alphabet().unknown().unwrap());
                }
            }
        } else {
            self.queue_mutator_arcs(nodes, &next_node, input_sym);
        }

        debug!("finish consume input");
    }

    fn lexicon_consume(&self, nodes: &mut Vec<TreeNode>, next_node: &TreeNode) {
        let mutator = self.speller.mutator();
        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();
        let input_state = next_node.input_state as usize;

        if input_state >= self.input.len() {
            return;
        }

        // TODO handle nullable mutator
        let input_sym = alphabet_translator[self.input[input_state as usize] as usize];
        let next_lexicon_state = next_node.lexicon_state + 1;

        if !lexicon.has_transitions(next_lexicon_state, Some(input_sym)) {
            // we have no regular transitions for this
            if input_sym >= lexicon.alphabet().initial_symbol_count() {
                if lexicon.has_transitions(next_lexicon_state, mutator.alphabet().identity()) {
                    self.queue_lexicon_arcs(
                        nodes,
                        &next_node,
                        lexicon.alphabet().identity().unwrap(),
                        next_node.mutator_state,
                        0.0,
                        1,
                    );
                }
                if lexicon.has_transitions(next_lexicon_state, mutator.alphabet().unknown()) {
                    self.queue_lexicon_arcs(
                        nodes,
                        &next_node,
                        lexicon.alphabet().unknown().unwrap(),
                        next_node.mutator_state,
                        0.0,
                        1,
                    );
                }
            }

            return;
        }

        self.queue_lexicon_arcs(nodes, &next_node, input_sym, next_node.mutator_state, 0.0, 1);
    }

    fn is_under_weight_limit(&self, w: Weight) -> bool {
        use std::f32;
        w < f32::MAX
    }

    fn state_size(&self) -> usize {
        self.speller.lexicon().alphabet().state_size() as usize
    }

    fn suggest_one(&self) -> Vec<Suggestion> {
        let start_node = TreeNode::empty(vec![0; self.state_size() as usize]);
        let mut nodes = vec![start_node];

        let mut corrections = BTreeMap::<String, Weight>::new();

        while let Some(next_node) = nodes.pop() {
            debug_incr("Worker node loop count");
            debug!("{:?}", next_node);

            debug!(
                "sugloop next_node: is:{} w:{} ms:{} ls:{}",
                next_node.input_state,
                next_node.weight,
                next_node.mutator_state,
                next_node.lexicon_state
            );

            if !self.is_under_weight_limit(next_node.weight) {
                //    continue
            }

            self.lexicon_epsilons(&mut nodes, &next_node);
            self.mutator_epsilons(&mut nodes, &next_node);

            if next_node.input_state as usize == self.input.len() {
                debug_incr("input_state eq input size");
                debug!(
                    "is_final ms:{} ls:{}",
                    next_node.mutator_state,
                    next_node.lexicon_state
                );
                if self.speller.mutator().is_final(next_node.mutator_state) &&
                    self.speller.lexicon().is_final(next_node.lexicon_state)
                {
                    debug_incr("is_final");

                    let key_table = self.speller.lexicon().alphabet().key_table();
                    let string: String = next_node
                        .string
                        .iter()
                        .map(|&s| key_table[s as usize].to_string())
                        .collect();

                    //debug!("string: {}", string);

                    let weight = next_node.weight +
                        self.speller
                            .lexicon()
                            .final_weight(next_node.lexicon_state)
                            .unwrap() +
                        self.speller
                            .mutator()
                            .final_weight(next_node.mutator_state)
                            .unwrap();
                    let entry = corrections.entry(string).or_insert(weight);

                    if *entry > weight {
                        *entry = weight;
                    }
                }
            } else {
                self.consume_input(&mut nodes, &next_node);
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

    pub fn is_correct(&self) -> bool {
        let start_node = TreeNode::empty(vec![0; self.state_size() as usize]);
        let mut nodes = vec![start_node];

        while let Some(next_node) = nodes.pop() {
            if next_node.input_state as usize == self.input.len() &&
                self.speller.lexicon().is_final(next_node.lexicon_state)
            {
                return true;
            }

            self.lexicon_epsilons(&mut nodes, &next_node);
            self.lexicon_consume(&mut nodes, &next_node);
        }

        false
    }
}

impl<'data, 'a> Speller<'data>
where
    'data: 'a,
{
    pub fn new(mutator: Transducer<'data>, mut lexicon: Transducer<'data>) -> Speller<'data> {
        let alphabet_translator = lexicon.mut_alphabet().create_translator_from(&mutator);

        Speller {
            mutator: mutator,
            lexicon: lexicon,
            alphabet_translator: alphabet_translator,
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
    
    fn to_input_vec(&'a self, word: &str) -> Vec<SymbolNumber> {
        // TODO: refactor for when mutator is optional
        let key_table = self.mutator().alphabet().key_table();

        word.chars()
            .filter_map(|ch| {
                let s = ch.to_string();
                key_table.iter().position(|x| x == &s)
            })
            .map(|x| x as u16)
            .collect()
    }

    pub fn is_correct(&'a self, word: &str) -> bool {
        let mut input = self.to_input_vec(word);
        let mut worker = SpellerWorker::new(&self, SpellerWorkerMode::Unknown, input);

        worker.is_correct()
    }    
    
    pub fn suggest(&'a self, words: &[&str]) -> Vec<Vec<Suggestion>> {
        words.iter().map(|w| self.suggest_one(w)).collect()
    }

    pub fn suggest_one(&'a self, word: &str) -> Vec<Suggestion> {
        let mut input = self.to_input_vec(word);
        let mut worker = SpellerWorker::new(&self, SpellerWorkerMode::Correct, input);

        worker.suggest_one()
    }
}
