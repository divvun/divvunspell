pub mod suggestion;

use hashbrown::{HashMap, HashSet};
use std::f32;
use std::sync::Arc;

use crate::COUNTER;

use crate::transducer::Transducer;
use crate::transducer::tree_node::TreeNode;
use crate::speller::suggestion::Suggestion;
use crate::types::{SymbolNumber, Weight, SpellerWorkerMode, FlagDiacriticOperator};

// pub fn debug_incr(key: &'static str) {
//     // debug!("{}", key);
//     use COUNTER;
//     let mut c = COUNTER.lock().unwrap();
//     let mut entry = c.entry(key).or_insert(0);
//     *entry += 1;
// }

#[derive(Clone, Debug)]
pub struct SpellerConfig {
    pub n_best: Option<usize>,
    pub max_weight: Option<Weight>,
    pub beam: Option<Weight>,
}

impl SpellerConfig {
    pub fn default() -> SpellerConfig {
        SpellerConfig {
            n_best: None,
            max_weight: None,
            beam: None
        }
    }
}

#[derive(Debug)]
pub struct Speller {
    mutator: Transducer,
    lexicon: Transducer,
    alphabet_translator: Vec<SymbolNumber>,
}

struct SpellerWorker {
    speller: Arc<Speller>,
    input: Vec<SymbolNumber>,
    mode: SpellerWorkerMode,
    config: SpellerConfig
}

// struct SpellerState {
//     nodes: Vec<TreeNode>,
//     max_weight: Weight
// }

fn speller_start_node(size: usize) -> Vec<TreeNode> {
    let start_node = TreeNode::empty(vec![0; size]);
    let mut nodes = Vec::with_capacity(256);
    nodes.push(start_node);
    nodes
}

fn speller_max_weight(config: &SpellerConfig) -> Weight {
    config.max_weight.unwrap_or(f32::INFINITY)
}

// impl SpellerState {
//     pub fn new(size: usize, config: &SpellerConfig) -> SpellerState {
//         let start_node = TreeNode::empty(vec![0; size]);
//         let mut nodes = Vec::with_capacity(256);
//         nodes.push(start_node);

//         SpellerState {
//             nodes: nodes,
//             max_weight: config.max_weight.unwrap_or(f32::INFINITY)
//         }
//     }
// }

impl SpellerWorker {
    fn new(
        speller: Arc<Speller>,
        mode: SpellerWorkerMode,
        input: Vec<SymbolNumber>,
        config: &SpellerConfig
    ) -> Arc<SpellerWorker> {
        Arc::new(SpellerWorker {
            speller: speller,
            input: input,
            mode: mode,
            config: config.clone()
        })
    }

    fn lexicon_epsilons(&self, max_weight: Weight, next_node: &TreeNode, nodes: &HashSet<TreeNode>) -> Vec<TreeNode> {
        // debug_incr("lexicon_epsilons");

        // debug!("Begin lexicon epsilons");

        let lexicon = self.speller.lexicon();
        let operations = lexicon.alphabet().operations();
        let mut output_nodes = Vec::new();

        if !lexicon.has_epsilons_or_flags(next_node.lexicon_state + 1) {
            // debug!("Has no epsilons or flags, returning");
            return output_nodes;
        }

        let mut next = lexicon.next(next_node.lexicon_state, 0).unwrap();

        while let Some(transition) = lexicon.take_epsilons_and_flags(next) {
            if let Some(sym) = lexicon.transition_table().input_symbol(next) {
                let transition_weight = transition.weight().unwrap();

                if sym == 0 {
                    if self.is_under_weight_limit(max_weight, next_node.weight() + transition_weight) {
                        if let SpellerWorkerMode::Correct = self.mode {
                            let epsilon_transition = transition.clone_with_epsilon_symbol();
                            // debug_incr(
                            //     "lexicon_epsilons push node epsilon_transition CORRECT MODE",
                            // );

                            let new_node = next_node.update_lexicon(epsilon_transition);

                            if !nodes.contains(&new_node) {
                                output_nodes.push(new_node);
                            }
                        } else {
                            // debug_incr("lexicon_epsilons push node transition");
                            let new_node = next_node.update_lexicon(transition);

                            if !nodes.contains(&new_node) {
                                output_nodes.push(new_node);
                            }
                            // output_nodes.push(next_node.update_lexicon(transition));
                        }
                    }
                   
                } else {
                    let operation = operations.get(&sym);

                    if let Some(op) = operation {
                        //println!("{:?}", op);

                        // let is_skippable = match op.operation {
                        //     FlagDiacriticOperator::PositiveSet => true,
                        //     FlagDiacriticOperator::NegativeSet => true,
                        //     FlagDiacriticOperator::Require => true,
                        //     FlagDiacriticOperator::Disallow => false,
                        //     FlagDiacriticOperator::Clear => false,
                        //     FlagDiacriticOperator::Unification => true
                        // };
                        
                        if !self.is_under_weight_limit(max_weight, transition_weight) { //next_node.weight) {//} + transition.weight().unwrap()) {
                            // println!("{}+{} {:?}", next_node.weight, transition.weight().unwrap(), &op);
                            next += 1;
                            continue;
                        }
                        
                        let (is_success, mut applied_node) = next_node.apply_operation(op);

                        if is_success {
                            let epsilon_transition = transition.clone_with_epsilon_symbol();
                            applied_node.update_lexicon_mut(epsilon_transition);

                            if !nodes.contains(&applied_node) {
                                output_nodes.push(applied_node);
                            }
                        }
                    }
                }
            }

            next += 1;
        }

        // debug!(
        //     "lexicon epsilons, nodes length: {}",
        //     state.nodes.len()
        // );

        output_nodes
    }

    fn mutator_epsilons(&self, max_weight: Weight, next_node: &TreeNode, nodes: &HashSet<TreeNode>) -> Vec<TreeNode> {
        // debug_incr("mutator_epsilons");
        // debug!("Begin mutator epsilons");

        let mutator = self.speller.mutator();
        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();
        let mut output_nodes = Vec::new();

        if !mutator.has_transitions(next_node.mutator_state + 1, Some(0)) {
            // debug!("Mutator has no transitions, skipping");
            return output_nodes;
        }

        let mut next_m = mutator.next(next_node.mutator_state, 0).unwrap();

        while let Some(transition) = mutator.take_epsilons(next_m) {
            //debug!("trans mut next: {}", next_m);
            //debug!("{}", next_node.weight);
            // debug!("Current taken epsilon: {}", next_m);

            if let Some(0) = transition.symbol() {
                if self.is_under_weight_limit(max_weight, next_node.weight() + transition.weight().unwrap()) {
                    // debug_incr("mutator_epsilons push node update_mutator transition");
                    let new_node = next_node.update_mutator(transition);
                    if !nodes.contains(&new_node) {
                        output_nodes.push(new_node);
                    } 
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
                            // debug_incr("qla unknown mutator_eps");
                            output_nodes.append(&mut self.queue_lexicon_arcs(
                                max_weight,
                                &next_node,
                                nodes,
                                lexicon.alphabet().unknown().unwrap(),
                                transition.target().unwrap(),
                                transition.weight().unwrap(),
                                0,
                            ));
                        }

                        if lexicon.has_transitions(
                            next_node.lexicon_state + 1,
                            lexicon.alphabet().identity(),
                        ) {
                            // debug_incr("qla identity mutator_eps");
                            output_nodes.append(&mut self.queue_lexicon_arcs(
                                max_weight,
                                &next_node,
                                nodes,
                                lexicon.alphabet().identity().unwrap(),
                                transition.target().unwrap(),
                                transition.weight().unwrap(),
                                0,
                            ));
                        }
                    }

                    next_m += 1;
                    continue;
                }

                // debug_incr("qla alpha_trans mutator_eps");
                output_nodes.append(&mut self.queue_lexicon_arcs(
                    max_weight,
                    &next_node,
                    nodes,
                    trans_sym,
                    transition.target().unwrap(),
                    transition.weight().unwrap(),
                    0,
                ));
            }

            next_m += 1;
        }

        output_nodes

        // debug!(
        //     "mutator epsilons, nodes length: {}",
        //     state.nodes.len()
        // );

        // debug!("End mutator epsilons");
    }

    pub fn queue_lexicon_arcs(
        &self,
        max_weight: Weight,
        next_node: &TreeNode,
        nodes: &HashSet<TreeNode>,
        input_sym: SymbolNumber,
        mutator_state: u32,
        mutator_weight: Weight,
        input_increment: i16,
    ) -> Vec<TreeNode> {
        // debug!("Begin queue lexicon arcs");

        let lexicon = self.speller.lexicon();
        let identity = lexicon.alphabet().identity();
        let mut next = lexicon.next(next_node.lexicon_state, input_sym).unwrap();
        let mut output_nodes = Vec::new();

        while let Some(noneps_trans) = lexicon.take_non_epsilons(next, input_sym) {
            //debug!("noneps next: {:?}", &noneps_trans);
            // debug!(
            //     "qla noneps input_sym:{}, next: {}, t:{} s:{} w:{}",
            //     input_sym,
            //     next,
            //     noneps_trans.target().unwrap(),
            //     noneps_trans.symbol().unwrap(),
            //     noneps_trans.weight().unwrap()
            // );

            if let Some(mut sym) = noneps_trans.symbol() {
                // TODO: wtf?
                if let Some(id) = identity {
                    if sym == id {
                        sym = self.input[next_node.input_state as usize];
                    }
                }

                //debug!("{}: {} {} {} n:{}", next, sym, next_node.weight, mutator_weight, nodes.len());

                let next_sym = match self.mode {
                    SpellerWorkerMode::Correct => input_sym,
                    _ => sym,
                };

                let can_push = match self.mode {
                    SpellerWorkerMode::Correct => {
                        // println!("{} vs {}+{}+{} = {}", state.max_weight, next_node.weight, noneps_trans.weight().unwrap(), mutator_weight, next_node.weight + noneps_trans.weight().unwrap() + mutator_weight);
                        self.is_under_weight_limit(
                            max_weight,
                            noneps_trans.weight().unwrap() + mutator_weight
                        )
                    },
                    _ => self.is_under_weight_limit(
                        max_weight,
                        next_node.weight() + noneps_trans.weight().unwrap() + mutator_weight,
                    )
                };

                if can_push {
                    // debug_incr("queue_lexicon_arcs push node update");

                    let new_node = next_node.update(
                        next_sym,
                        Some(next_node.input_state + input_increment as u32),
                        mutator_state,
                        noneps_trans.target().unwrap(),
                        noneps_trans.weight().unwrap() + mutator_weight,
                    );

                    if !nodes.contains(&new_node) {
                        output_nodes.push(new_node);
                    }
                }
            }

            next += 1;
            // debug!("qla noneps NEXT input_sym:{}, next: {}", input_sym, next);
        }

        // debug!("qla, nodes length: {}", state.nodes.len());
        // debug!("--- qla noneps end ---");

        //debug!("End lexicon arcs");
        output_nodes
    }

    fn queue_mutator_arcs(&self, max_weight: Weight, next_node: &TreeNode, nodes: &HashSet<TreeNode>, input_sym: SymbolNumber) -> Vec<TreeNode> {
        //debug!("Mutator arcs");
        let mutator = self.speller.mutator();
        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();
        let mut output_nodes = Vec::new();

        let mut next_m = mutator.next(next_node.mutator_state, input_sym).unwrap();

        while let Some(transition) = mutator.take_non_epsilons(next_m, input_sym) {
            //debug!("mut arc loop: {}", next_m);
            let symbol = transition.symbol();

            if let Some(0) = symbol {
                // println!("{}, {}", next_node.weight, transition.weight().unwrap());
                // TODO: this line causes a great speed up but also breaks accuracy _a lot_
                let transition_weight = transition.weight().unwrap();
                if self.is_under_weight_limit(max_weight, next_node.weight() + transition_weight) {
                    // debug_incr("queue_mutator_arcs push node update");
                    let new_node = next_node.update(
                        0,
                        Some(next_node.input_state + 1),
                        transition.target().unwrap(),
                        next_node.lexicon_state,
                        transition_weight,
                    );

                    if !nodes.contains(&new_node) {
                        output_nodes.push(new_node);
                    }
                }

                next_m += 1;
                continue;
            }

            if let Some(sym) = symbol {
                let trans_sym = alphabet_translator[sym as usize];

                if !lexicon.has_transitions(next_node.lexicon_state + 1, Some(trans_sym)) {
                    if trans_sym >= lexicon.alphabet().initial_symbol_count() {
                        if lexicon.has_transitions(
                            next_node.lexicon_state + 1,
                            lexicon.alphabet().unknown(),
                        ) {
                            // debug_incr("qla unknown qma");
                            output_nodes.append(&mut self.queue_lexicon_arcs(
                                max_weight,
                                &next_node,
                                nodes,
                                lexicon.alphabet().unknown().unwrap(),
                                transition.target().unwrap(),
                                transition.weight().unwrap(),
                                1,
                            ));
                        }
                        if lexicon.has_transitions(
                            next_node.lexicon_state + 1,
                            lexicon.alphabet().identity(),
                        ) {
                            // debug_incr("qla identity qma");
                            output_nodes.append(&mut self.queue_lexicon_arcs(
                                max_weight,
                                &next_node,
                                nodes,
                                lexicon.alphabet().identity().unwrap(),
                                transition.target().unwrap(),
                                transition.weight().unwrap(),
                                1,
                            ));
                        }
                    }
                    next_m += 1;
                    continue;
                }

                // debug_incr("qla alpha_trans qma");
                output_nodes.append(&mut self.queue_lexicon_arcs(
                    max_weight,
                    &next_node,
                    nodes,
                    trans_sym,
                    transition.target().unwrap(),
                    transition.weight().unwrap(),
                    1,
                ));

                next_m += 1;
            }
        }

        output_nodes

        // debug!("qma, nodes length: {}", state.nodes.len());
    }

    fn consume_input(&self, max_weight: Weight, next_node: &TreeNode, nodes: &HashSet<TreeNode>) -> Vec<TreeNode> {
        let mutator = self.speller.mutator();
        let input_state = next_node.input_state as usize;
        let mut output_nodes = Vec::new();

        if input_state >= self.input.len() {
            return output_nodes;
        }

        let input_sym = self.input[input_state];

        if !mutator.has_transitions(next_node.mutator_state + 1, Some(input_sym)) {
            // we have no regular transitions for this
            if input_sym >= mutator.alphabet().initial_symbol_count() {
                if mutator
                    .has_transitions(next_node.mutator_state + 1, mutator.alphabet().identity())
                {
                    output_nodes.append(&mut self.queue_mutator_arcs(max_weight, &next_node, nodes, mutator.alphabet().identity().unwrap()));
                }

                // Check for unknown transition
                if mutator
                    .has_transitions(next_node.mutator_state + 1, mutator.alphabet().unknown())
                {
                    output_nodes.append(&mut self.queue_mutator_arcs(max_weight, &next_node, nodes, mutator.alphabet().unknown().unwrap()));
                }
            }
        } else {
            output_nodes.append(&mut self.queue_mutator_arcs(max_weight, &next_node, nodes, input_sym));
        }

        // debug!("finish consume input");
        output_nodes
    }

    fn lexicon_consume(&self, max_weight: Weight, next_node: &TreeNode, nodes: &HashSet<TreeNode>) -> Vec<TreeNode> {
        let mutator = self.speller.mutator();
        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();
        let input_state = next_node.input_state as usize;
        let mut output_nodes = Vec::new();

        if input_state >= self.input.len() {
            return output_nodes;
        }

        // TODO handle nullable mutator
        let input_sym = alphabet_translator[self.input[input_state as usize] as usize];
        let next_lexicon_state = next_node.lexicon_state + 1;

        if !lexicon.has_transitions(next_lexicon_state, Some(input_sym)) {
            // we have no regular transitions for this
            if input_sym >= lexicon.alphabet().initial_symbol_count() {
                let identity = mutator.alphabet().identity();
                if lexicon.has_transitions(next_lexicon_state, identity) {
                    output_nodes.append(&mut self.queue_lexicon_arcs(
                        max_weight,
                        &next_node,
                        nodes,
                        identity.unwrap(),
                        next_node.mutator_state,
                        0.0,
                        1,
                    ));
                }

                let unknown = mutator.alphabet().unknown();
                if lexicon.has_transitions(next_lexicon_state, unknown) {
                    output_nodes.append(&mut self.queue_lexicon_arcs(
                        max_weight,
                        &next_node,
                        nodes,
                        unknown.unwrap(),
                        next_node.mutator_state,
                        0.0,
                        1,
                    ));
                }
            }

            return output_nodes;
        }

        output_nodes.append(&mut self.queue_lexicon_arcs(max_weight, &next_node, nodes, input_sym, next_node.mutator_state, 0.0, 1));
        output_nodes
    }

    fn update_weight_limit(&self, best_weight: Weight, suggestions: &Vec<Suggestion>) -> Weight {
        use std::cmp::Ordering::{Less, Equal};

        let c = &self.config;
        let mut max_weight = c.max_weight.unwrap_or(f32::INFINITY);

        if let Some(beam) = c.beam {
            let candidate_weight = best_weight + beam;

            max_weight = match max_weight.partial_cmp(&candidate_weight).unwrap_or(Equal) {
                Less => max_weight,
                _ => candidate_weight
            };

            // if old_max != state.max_weight {
            //     println!("Max old: {} beam new: {}", old_max, state.max_weight);
            // }
        }

        if let Some(n) = c.n_best {
            if suggestions.len() >= n {
                if let Some(sugg) = suggestions.last() {
                    return sugg.weight();

                    // if old_max != state.max_weight {
                    //     println!("Max old: {} n-best new: {}", old_max, state.max_weight);
                    // }
                }
            }
        }

        max_weight
    }

    #[inline]
    fn is_under_weight_limit(&self, max_weight: Weight, w: Weight) -> bool {
        w <= max_weight
    }

    fn state_size(&self) -> usize {
        self.speller.lexicon().alphabet().state_size() as usize
    }

    pub fn is_correct(&self) -> bool {
        let mut max_weight = speller_max_weight(&self.config);
        let mut nodes = speller_start_node(self.state_size() as usize);

        let mut seen_nodes: HashSet<TreeNode> = HashSet::default();

        while let Some(next_node) = nodes.pop() {
            seen_nodes.insert(next_node.clone());
            
            if next_node.input_state as usize == self.input.len() &&
                self.speller.lexicon().is_final(next_node.lexicon_state)
            {
                return true;
            }

            self.lexicon_epsilons(max_weight, &next_node, &seen_nodes);
            self.lexicon_consume(max_weight, &next_node, &seen_nodes);
        }

        false
    }

    fn suggest(self: Arc<Self>) -> Vec<Suggestion> {
        let mut max_weight = speller_max_weight(&self.config);
        let mut nodes = speller_start_node(self.state_size() as usize);
        let mut corrections = HashMap::new();//fxhash::FxHashMap::<String, Weight>::default();
        let mut suggestions: Vec<Suggestion> = vec![];
        let mut best_weight = self.config.max_weight.unwrap_or(f32::INFINITY);

        let mut seen_nodes: HashSet<TreeNode> = HashSet::default();

        // COUNTER.lock().unwrap().insert("c", 0);

        loop {
        // while let Some(next_node) = nodes.pop() {
            // COUNTER.lock().unwrap().entry("c").and_modify(|e| { *e += 1; });

            let next_node = {
                match nodes.pop() {
                    Some(v) => v,
                    None => break
                }
            };

            seen_nodes.insert(next_node.clone());

            // if all_nodes.contains(&next_node) {
            //     // println!("DO YOU SEE");
            //     continue;
            // }

            // let x = all_nodes.iter().filter(|&x| *x == next_node).count();
            // if x > 5 {
            //     println!("{}", x);
            // }
            // all_nodes.push(next_node.clone());
            

            max_weight = self.update_weight_limit(best_weight, &suggestions);
            
            // debug_incr("Worker node loop count");
            // debug!("{:?}", next_node);

            // debug!(
            //     "sugloop next_node: is:{} w:{} ms:{} ls:{}",
            //     next_node.input_state,
            //     next_node.weight,
            //     next_node.mutator_state,
            //     next_node.lexicon_state
            // );

            if !self.is_under_weight_limit(max_weight, next_node.weight()) {
                continue
            }

            // let next_node = Arc::new(next_node);
            // let state = Arc::new(state);p
            
            nodes.append(&mut self.lexicon_epsilons(max_weight, &next_node, &seen_nodes));
            nodes.append(&mut self.mutator_epsilons(max_weight, &next_node, &seen_nodes));

            // println!("{:?}", state.nodes);

            if next_node.input_state as usize == self.input.len() {
                // // debug_incr("input_state eq input size");
                // debug!(
                //     "is_final ms:{} ls:{}",
                //     next_node.mutator_state,
                //     next_node.lexicon_state
                // );
                if self.speller.mutator().is_final(next_node.mutator_state) &&
                    self.speller.lexicon().is_final(next_node.lexicon_state)
                {
                    // debug_incr("is_final");

                    //debug!("string: {}", string);

                    let weight = next_node.weight() +
                        self.speller
                            .lexicon()
                            .final_weight(next_node.lexicon_state)
                            .unwrap() +
                        self.speller
                            .mutator()
                            .final_weight(next_node.mutator_state)
                            .unwrap();

                    if !self.is_under_weight_limit(max_weight, weight) {
                        continue;
                    }

                    let key_table = self.speller.lexicon().alphabet().key_table();
                    let string: String = next_node
                        .string
                        .iter()
                        .map(|&s| key_table[s as usize].to_string())
                        .collect();

                    if weight < best_weight {
                        best_weight = weight;
                    }
                    
                    {
                        let entry = corrections.entry(string).or_insert(weight);

                        if *entry > weight {
                            *entry = weight;
                        }
                    }

                    suggestions = self.generate_sorted_suggestions(&corrections);
                }
            } else {
                nodes.append(&mut self.consume_input(max_weight, &next_node, &seen_nodes));
            }
        }

        // debug!("Here we go!");
        // println!("{}", COUNTER.lock().unwrap()["c"]);

        suggestions
    }

    fn generate_sorted_suggestions(&self, corrections: &HashMap<String, Weight>) -> Vec<Suggestion> {
        let mut c: Vec<Suggestion> = corrections
            .into_iter()
            .map(|x| Suggestion::new(x.0.to_string(), *x.1))
            .collect();

        c.sort();

        if let Some(n) = self.config.n_best {
            c.truncate(n);
        }

        c
    }
}

impl Speller {
    pub fn new(mutator: Transducer, mut lexicon: Transducer) -> Arc<Speller> {
        let alphabet_translator = lexicon.mut_alphabet().create_translator_from(&mutator);

        Arc::new(Speller {
            mutator: mutator,
            lexicon: lexicon,
            alphabet_translator: alphabet_translator,
        })
    }

    pub fn mutator(&self) -> &Transducer {
        &self.mutator
    }

    pub fn lexicon(&self) -> &Transducer {
        &self.lexicon
    }

    fn alphabet_translator(&self) -> &Vec<SymbolNumber> {
        &self.alphabet_translator
    }
    
    fn to_input_vec(&self, word: &str) -> Vec<SymbolNumber> {
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

    pub fn is_correct(self: Arc<Self>, word: &str) -> bool {
        let worker = SpellerWorker::new(
            self.clone(),
            SpellerWorkerMode::Unknown,
            self.to_input_vec(word),
            &SpellerConfig::default()
        );

        worker.is_correct()
    }

    pub fn suggest(self: Arc<Self>, word: &str) -> Vec<Suggestion> {
        self.suggest_with_config(word, &SpellerConfig::default())
    }

    pub fn suggest_with_config(self: Arc<Self>, word: &str, config: &SpellerConfig) -> Vec<Suggestion> {
        let worker = SpellerWorker::new(
            self.clone(),
            SpellerWorkerMode::Correct,
            self.to_input_vec(word),
            config
        );

        worker.suggest()
    }
}
