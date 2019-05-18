use hashbrown::{HashMap, HashSet};
use std::f32;
use std::sync::Arc;

use lifeguard::{Pool, Recycled};

use super::{SpellerConfig, Speller};
use crate::transducer::Transducer;
use crate::transducer::tree_node::TreeNode;
use crate::speller::suggestion::Suggestion;
use crate::types::{SymbolNumber, Weight, SpellerWorkerMode};

fn speller_start_node(pool: &Pool<TreeNode>, size: usize) -> Vec<Recycled<TreeNode>> {
    let start_node = TreeNode::empty(pool, vec![0; size]);
    let mut nodes = Vec::with_capacity(256);
    nodes.push(start_node);
    nodes
}

fn speller_max_weight(config: &SpellerConfig) -> Weight {
    config.max_weight.unwrap_or(f32::INFINITY)
}

pub struct SpellerWorker<T: Transducer> {
    speller: Arc<Speller<T>>,
    input: Vec<SymbolNumber>,
    mode: SpellerWorkerMode,
    config: SpellerConfig
}

impl<T: Transducer> SpellerWorker<T> {
    pub fn new(
        speller: Arc<Speller<T>>,
        mode: SpellerWorkerMode,
        input: Vec<SymbolNumber>,
        config: SpellerConfig
    ) -> Arc<SpellerWorker<T>> {
        Arc::new(SpellerWorker {
            speller: speller,
            input: input,
            mode: mode,
            config: config,
        })
    }

    fn lexicon_epsilons<'a>(&self, pool: &'a Pool<TreeNode>, max_weight: Weight, next_node: &TreeNode, nodes: &HashSet<TreeNode>) -> Vec<Recycled<'a, TreeNode>> {
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
            if let Some(sym) = lexicon.transition_input_symbol(next) {
                let transition_weight = transition.weight().unwrap();

                if sym == 0 {
                    if self.is_under_weight_limit(max_weight, next_node.weight() + transition_weight) {
                        if let SpellerWorkerMode::Correct = self.mode {
                            let epsilon_transition = transition.clone_with_epsilon_symbol();
                            // debug_incr(
                            //     "lexicon_epsilons push node epsilon_transition CORRECT MODE",
                            // );

                            let new_node = next_node.update_lexicon(pool, epsilon_transition);

                            if !nodes.contains(&new_node) {
                                output_nodes.push(new_node);
                            }
                        } else {
                            // debug_incr("lexicon_epsilons push node transition");
                            let new_node = next_node.update_lexicon(pool, transition);

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
                        
                        let (is_success, mut applied_node) = next_node.apply_operation(pool, op);

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

    fn mutator_epsilons<'a>(&self, pool: &'a Pool<TreeNode>, max_weight: Weight, next_node: &TreeNode, nodes: &HashSet<TreeNode>) -> Vec<Recycled<'a, TreeNode>> {
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
            // println!("trans mut next: {} {}", next_m, next_node.weight.0);
            //debug!("{}", next_node.weight);
            // debug!("Current taken epsilon: {}", next_m);

            if let Some(0) = transition.symbol() {
                if self.is_under_weight_limit(max_weight, next_node.weight() + transition.weight().unwrap()) {
                    // debug_incr("mutator_epsilons push node update_mutator transition");
                    let new_node = next_node.update_mutator(pool, transition);
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
                                pool,
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
                                pool,
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
                    pool,
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

    pub fn queue_lexicon_arcs<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        max_weight: Weight,
        next_node: &TreeNode,
        nodes: &HashSet<TreeNode>,
        input_sym: SymbolNumber,
        mutator_state: u32,
        mutator_weight: Weight,
        input_increment: i16,
    ) -> Vec<Recycled<'a, TreeNode>> {
        // debug!("Begin queue lexicon arcs");

        let lexicon = self.speller.lexicon();
        let identity = lexicon.alphabet().identity();
        let mut next = lexicon.next(next_node.lexicon_state, input_sym).unwrap();
        let mut output_nodes: Vec<Recycled<'a, TreeNode>> = Vec::new();

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
                        pool,
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

    fn queue_mutator_arcs<'a>(&self, pool: &'a Pool<TreeNode>, max_weight: Weight, next_node: &TreeNode, nodes: &HashSet<TreeNode>, input_sym: SymbolNumber) -> Vec<Recycled<'a, TreeNode>> {
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
                        pool,
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
                                pool,
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
                                pool,
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
                    pool,
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

    fn consume_input<'a>(&self, pool: &'a Pool<TreeNode>, max_weight: Weight, next_node: &TreeNode, nodes: &HashSet<TreeNode>) -> Vec<Recycled<'a, TreeNode>> {
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
                    output_nodes.append(&mut self.queue_mutator_arcs(pool, max_weight, &next_node, nodes, mutator.alphabet().identity().unwrap()));
                }

                // Check for unknown transition
                if mutator
                    .has_transitions(next_node.mutator_state + 1, mutator.alphabet().unknown())
                {
                    output_nodes.append(&mut self.queue_mutator_arcs(pool, max_weight, &next_node, nodes, mutator.alphabet().unknown().unwrap()));
                }
            }
        } else {
            output_nodes.append(&mut self.queue_mutator_arcs(pool, max_weight, &next_node, nodes, input_sym));
        }

        // debug!("finish consume input");
        output_nodes
    }

    fn lexicon_consume<'a>(&self, pool: &'a Pool<TreeNode>, max_weight: Weight, next_node: &TreeNode, nodes: &HashSet<TreeNode>) -> Vec<Recycled<'a, TreeNode>> {
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
                        pool,
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
                        pool,
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

        output_nodes.append(&mut self.queue_lexicon_arcs(pool, max_weight, &next_node, nodes, input_sym, next_node.mutator_state, 0.0, 1));
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
        let pool = Pool::with_size(128);
        let mut nodes = speller_start_node(&pool, self.state_size() as usize);

        let mut seen_nodes: HashSet<TreeNode> = HashSet::default();

        while let Some(next_node) = nodes.pop() {
            seen_nodes.insert((*next_node).clone());
            
            if next_node.input_state as usize == self.input.len() &&
                self.speller.lexicon().is_final(next_node.lexicon_state)
            {
                return true;
            }

            nodes.append(&mut self.lexicon_epsilons(&pool, max_weight, &next_node, &seen_nodes));
            nodes.append(&mut self.lexicon_consume(&pool, max_weight, &next_node, &seen_nodes));
        }

        false
    }

    pub fn suggest(self: Arc<Self>) -> Vec<Suggestion> {
        let mut max_weight = speller_max_weight(&self.config);
        let pool = Pool::with_size(1024);
        let mut nodes = speller_start_node(&pool, self.state_size() as usize);
        let mut corrections = HashMap::new();
        let mut suggestions: Vec<Suggestion> = vec![];
        let mut best_weight = self.config.max_weight.unwrap_or(f32::INFINITY);

        use std::io::Write;
        // let mut out = std::fs::File::create("log.txt").unwrap();

        let mut seen_nodes: HashSet<TreeNode> = HashSet::default();
        loop {
            let next_node = {
                match nodes.pop() {
                    Some(v) => v,
                    None => break
                }
            };

            // writeln!(out, "{:?}", next_node).unwrap();

            seen_nodes.insert((*next_node).clone());
            
            max_weight = self.update_weight_limit(best_weight, &suggestions);

            if !self.is_under_weight_limit(max_weight, next_node.weight()) {
                continue
            }
            
            nodes.append(&mut self.lexicon_epsilons(&pool, max_weight, &next_node, &seen_nodes));
            nodes.append(&mut self.mutator_epsilons(&pool, max_weight, &next_node, &seen_nodes));

            if next_node.input_state as usize != self.input.len() {
                nodes.append(&mut self.consume_input(&pool, max_weight, &next_node, &seen_nodes));
                continue;
            }

            if !self.speller.mutator().is_final(next_node.mutator_state) ||
                !self.speller.lexicon().is_final(next_node.lexicon_state)
            {
                continue;
            }

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
                .map(|s| &*key_table[*s as usize])
                .collect();

            // writeln!(out, "Selected: {}", &string).unwrap();

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
