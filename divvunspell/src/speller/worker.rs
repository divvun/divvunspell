use hashbrown::HashMap;
use smol_str::SmolStr;
use std::f32;
use std::sync::Arc;

use lifeguard::{Pool, Recycled};

use super::{HfstSpeller, SpellerConfig};
use crate::speller::suggestion::Suggestion;
use crate::transducer::tree_node::TreeNode;
use crate::transducer::Transducer;
use crate::types::{SymbolNumber, Weight};

#[inline(always)]
fn speller_start_node(pool: &Pool<TreeNode>, size: usize) -> Vec<Recycled<TreeNode>> {
    let start_node = TreeNode::empty(pool, vec![0; size]);
    let mut nodes = Vec::with_capacity(256);
    nodes.push(start_node);
    nodes
}

#[inline(always)]
fn speller_max_weight(config: &SpellerConfig) -> Weight {
    config.max_weight.unwrap_or(f32::MAX)
}

pub struct SpellerWorker<F: crate::vfs::File, T: Transducer<F>, U: Transducer<F>> {
    speller: Arc<HfstSpeller<F, T, U>>,
    input: Vec<SymbolNumber>,
    config: SpellerConfig,
}

#[allow(clippy::too_many_arguments)]
impl<'t, F, T: Transducer<F> + 't, U: Transducer<F> + 't> SpellerWorker<F, T, U>
where
    F: crate::vfs::File,
    T: Transducer<F>,
    U: Transducer<F>,
{
    #[inline(always)]
    pub(crate) fn new(
        speller: Arc<HfstSpeller<F, T, U>>,
        input: Vec<SymbolNumber>,
        config: SpellerConfig,
    ) -> SpellerWorker<F, T, U> {
        SpellerWorker {
            speller,
            input,
            config,
        }
    }

    #[inline(always)]
    fn lexicon_epsilons<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        max_weight: Weight,
        next_node: &TreeNode,
        output_nodes: &mut Vec<Recycled<'a, TreeNode>>,
    ) {
        let lexicon = self.speller.lexicon();
        let operations = lexicon.alphabet().operations();

        if !lexicon.has_epsilons_or_flags(next_node.lexicon_state + 1) {
            return;
        }

        let mut next = lexicon.next(next_node.lexicon_state, 0).unwrap();

        while let Some(transition) = lexicon.take_epsilons_and_flags(next) {
            if let Some(sym) = lexicon.transition_input_symbol(next) {
                let transition_weight = transition.weight().unwrap();

                if sym == 0 {
                    if self
                        .is_under_weight_limit(max_weight, next_node.weight() + transition_weight)
                    {
                        let new_node = next_node.update_lexicon(pool, transition);
                        output_nodes.push(new_node);
                    }
                } else {
                    let operation = operations.get(&sym);

                    if let Some(op) = operation {
                        if !self.is_under_weight_limit(max_weight, transition_weight) {
                            next += 1;
                            continue;
                        }

                        if let Some(applied_node) = next_node.apply_operation(pool, op, &transition)
                        {
                            output_nodes.push(applied_node);
                        }
                    }
                }
            }

            next += 1;
        }
    }

    #[inline(always)]
    fn mutator_epsilons<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        max_weight: Weight,
        next_node: &TreeNode,
        output_nodes: &mut Vec<Recycled<'a, TreeNode>>,
    ) {
        let mutator = self.speller.mutator();
        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();

        if !mutator.has_transitions(next_node.mutator_state + 1, Some(0)) {
            return;
        }

        let mut next_m = mutator.next(next_node.mutator_state, 0).unwrap();

        while let Some(transition) = mutator.take_epsilons(next_m) {
            if let Some(0) = transition.symbol() {
                if self.is_under_weight_limit(
                    max_weight,
                    next_node.weight() + transition.weight().unwrap(),
                ) {
                    let new_node = next_node.update_mutator(pool, transition);
                    output_nodes.push(new_node);
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
                            self.queue_lexicon_arcs(
                                pool,
                                max_weight,
                                &next_node,
                                lexicon.alphabet().unknown().unwrap(),
                                transition.target().unwrap(),
                                transition.weight().unwrap(),
                                0,
                                output_nodes,
                            );
                        }

                        if lexicon.has_transitions(
                            next_node.lexicon_state + 1,
                            lexicon.alphabet().identity(),
                        ) {
                            self.queue_lexicon_arcs(
                                pool,
                                max_weight,
                                &next_node,
                                lexicon.alphabet().identity().unwrap(),
                                transition.target().unwrap(),
                                transition.weight().unwrap(),
                                0,
                                output_nodes,
                            );
                        }
                    }

                    next_m += 1;
                    continue;
                }

                self.queue_lexicon_arcs(
                    pool,
                    max_weight,
                    &next_node,
                    trans_sym,
                    transition.target().unwrap(),
                    transition.weight().unwrap(),
                    0,
                    output_nodes,
                );
            }

            next_m += 1;
        }
    }

    #[inline(always)]
    fn queue_lexicon_arcs<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        max_weight: Weight,
        next_node: &TreeNode,
        input_sym: SymbolNumber,
        mutator_state: u32,
        mutator_weight: Weight,
        input_increment: i16,
        output_nodes: &mut Vec<Recycled<'a, TreeNode>>,
    ) {
        let lexicon = self.speller.lexicon();
        let identity = lexicon.alphabet().identity();
        let mut next = lexicon.next(next_node.lexicon_state, input_sym).unwrap();

        // TODO: Potential infinite loop!

        while let Some(noneps_trans) = lexicon.take_non_epsilons(next, input_sym) {
            if let Some(mut sym) = noneps_trans.symbol() {
                // Symbol replacement here is unfortunate but necessary.
                if let Some(id) = identity {
                    if sym == id {
                        sym = self.input[next_node.input_state as usize];
                    }
                }

                let is_under_weight_limit = self.is_under_weight_limit(
                    max_weight,
                    next_node.weight() + noneps_trans.weight().unwrap() + mutator_weight,
                );

                if is_under_weight_limit {
                    let new_node = next_node.update(
                        pool,
                        sym,
                        Some(next_node.input_state + input_increment as u32),
                        mutator_state,
                        noneps_trans.target().unwrap(),
                        noneps_trans.weight().unwrap() + mutator_weight,
                    );

                    output_nodes.push(new_node);
                }
            }

            next += 1;
        }
    }

    #[inline(always)]
    fn queue_mutator_arcs<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        max_weight: Weight,
        next_node: &TreeNode,
        input_sym: SymbolNumber,
        output_nodes: &mut Vec<Recycled<'a, TreeNode>>,
    ) {
        let mutator = self.speller.mutator();
        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();

        let mut next_m = mutator.next(next_node.mutator_state, input_sym).unwrap();

        while let Some(transition) = mutator.take_non_epsilons(next_m, input_sym) {
            let symbol = transition.symbol();

            if let Some(0) = symbol {
                let transition_weight = transition.weight().unwrap();
                if self.is_under_weight_limit(max_weight, next_node.weight() + transition_weight) {
                    let new_node = next_node.update(
                        pool,
                        0,
                        Some(next_node.input_state + 1),
                        transition.target().unwrap(),
                        next_node.lexicon_state,
                        transition_weight,
                    );

                    output_nodes.push(new_node);
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
                            self.queue_lexicon_arcs(
                                pool,
                                max_weight,
                                &next_node,
                                lexicon.alphabet().unknown().unwrap(),
                                transition.target().unwrap(),
                                transition.weight().unwrap(),
                                1,
                                output_nodes,
                            );
                        }
                        if lexicon.has_transitions(
                            next_node.lexicon_state + 1,
                            lexicon.alphabet().identity(),
                        ) {
                            self.queue_lexicon_arcs(
                                pool,
                                max_weight,
                                &next_node,
                                lexicon.alphabet().identity().unwrap(),
                                transition.target().unwrap(),
                                transition.weight().unwrap(),
                                1,
                                output_nodes,
                            );
                        }
                    }
                    next_m += 1;
                    continue;
                }

                self.queue_lexicon_arcs(
                    pool,
                    max_weight,
                    &next_node,
                    trans_sym,
                    transition.target().unwrap(),
                    transition.weight().unwrap(),
                    1,
                    output_nodes,
                );

                next_m += 1;
            }
        }
    }

    #[inline(always)]
    fn consume_input<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        max_weight: Weight,
        next_node: &TreeNode,
        output_nodes: &mut Vec<Recycled<'a, TreeNode>>,
    ) {
        let mutator = self.speller.mutator();
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
                    self.queue_mutator_arcs(
                        pool,
                        max_weight,
                        &next_node,
                        mutator.alphabet().identity().unwrap(),
                        output_nodes,
                    );
                }

                // Check for unknown transition
                if mutator
                    .has_transitions(next_node.mutator_state + 1, mutator.alphabet().unknown())
                {
                    self.queue_mutator_arcs(
                        pool,
                        max_weight,
                        &next_node,
                        mutator.alphabet().unknown().unwrap(),
                        output_nodes,
                    );
                }
            }
        } else {
            self.queue_mutator_arcs(pool, max_weight, &next_node, input_sym, output_nodes)
        }
    }

    #[inline(always)]
    fn lexicon_consume<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        max_weight: Weight,
        next_node: &TreeNode,
        output_nodes: &mut Vec<Recycled<'a, TreeNode>>,
    ) {
        let mutator = self.speller.mutator();
        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();
        let input_state = next_node.input_state as usize;

        if input_state >= self.input.len() {
            return;
        }

        let input_sym = alphabet_translator[self.input[input_state as usize] as usize];
        let next_lexicon_state = next_node.lexicon_state + 1;

        if !lexicon.has_transitions(next_lexicon_state, Some(input_sym)) {
            // we have no regular transitions for this
            if input_sym >= lexicon.alphabet().initial_symbol_count() {
                let identity = mutator.alphabet().identity();
                if lexicon.has_transitions(next_lexicon_state, identity) {
                    self.queue_lexicon_arcs(
                        pool,
                        max_weight,
                        &next_node,
                        identity.unwrap(),
                        next_node.mutator_state,
                        0.0,
                        1,
                        output_nodes,
                    );
                }

                let unknown = mutator.alphabet().unknown();
                if lexicon.has_transitions(next_lexicon_state, unknown) {
                    self.queue_lexicon_arcs(
                        pool,
                        max_weight,
                        &next_node,
                        unknown.unwrap(),
                        next_node.mutator_state,
                        0.0,
                        1,
                        output_nodes,
                    );
                }
            }

            return;
        }

        self.queue_lexicon_arcs(
            pool,
            max_weight,
            &next_node,
            input_sym,
            next_node.mutator_state,
            0.0,
            1,
            output_nodes,
        );
    }

    #[inline(always)]
    fn update_weight_limit(&self, best_weight: Weight, suggestions: &[Suggestion]) -> Weight {
        use std::cmp::Ordering::{Equal, Less};

        let c = &self.config;
        let mut max_weight = c.max_weight.unwrap_or(f32::MAX);

        if let Some(beam) = c.beam {
            let candidate_weight = best_weight + beam;

            max_weight = match max_weight.partial_cmp(&candidate_weight).unwrap_or(Equal) {
                Less => max_weight,
                _ => candidate_weight,
            };
        }

        if c.n_best.is_some() && suggestions.len() >= c.n_best.unwrap() {
            if let Some(sugg) = suggestions.last() {
                return sugg.weight();
            }
        }

        max_weight
    }

    #[inline(always)]
    fn is_under_weight_limit(&self, max_weight: Weight, w: Weight) -> bool {
        w <= max_weight
    }

    #[inline(always)]
    fn state_size(&self) -> usize {
        self.speller.lexicon().alphabet().state_size() as usize
    }

    pub(crate) fn is_correct(&self) -> bool {
        let max_weight = speller_max_weight(&self.config);
        let pool = Pool::with_size_and_max(0, 0);
        let mut nodes = speller_start_node(&pool, self.state_size() as usize);

        while let Some(next_node) = nodes.pop() {
            if next_node.input_state as usize == self.input.len()
                && self.speller.lexicon().is_final(next_node.lexicon_state)
            {
                return true;
            }

            self.lexicon_epsilons(&pool, max_weight, &next_node, &mut nodes);
            self.lexicon_consume(&pool, max_weight, &next_node, &mut nodes);
        }

        false
    }

    pub(crate) fn suggest(&self) -> Vec<Suggestion> {
        log::trace!("Beginning suggest");

        let pool = Pool::with_size_and_max(self.config.node_pool_size, self.config.node_pool_size);
        let mut nodes = speller_start_node(&pool, self.state_size() as usize);
        let mut corrections = HashMap::new();
        let mut suggestions: Vec<Suggestion> = vec![];
        let mut best_weight = self.config.max_weight.unwrap_or(f32::MAX);
        let key_table = self.speller.mutator().alphabet().key_table();

        let mut iteration_count = 0usize;

        while let Some(next_node) = nodes.pop() {
            iteration_count += 1;

            let max_weight = self.update_weight_limit(best_weight, &suggestions);

            if iteration_count >= 10_000_000 {
                let name: SmolStr = self
                    .input
                    .iter()
                    .map(|s| &*key_table[*s as usize])
                    .collect();
                log::warn!("{}: iteration count at {}", name, iteration_count);
                log::warn!("Node count: {}", nodes.len());
                log::warn!("Node weight: {}", next_node.weight());
                break;
            }

            if !self.is_under_weight_limit(max_weight, next_node.weight()) {
                continue;
            }

            self.lexicon_epsilons(&pool, max_weight, &next_node, &mut nodes);
            self.mutator_epsilons(&pool, max_weight, &next_node, &mut nodes);

            if next_node.input_state as usize != self.input.len() {
                self.consume_input(&pool, max_weight, &next_node, &mut nodes);
                continue;
            }

            if !self.speller.mutator().is_final(next_node.mutator_state)
                || !self.speller.lexicon().is_final(next_node.lexicon_state)
            {
                continue;
            }

            let weight = next_node.weight()
                + self
                    .speller
                    .lexicon()
                    .final_weight(next_node.lexicon_state)
                    .unwrap()
                + self
                    .speller
                    .mutator()
                    .final_weight(next_node.mutator_state)
                    .unwrap();

            if !self.is_under_weight_limit(max_weight, weight) {
                continue;
            }

            let string = self
                .speller
                .lexicon()
                .alphabet()
                .string_from_symbols(&next_node.string);

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

    fn generate_sorted_suggestions(
        &self,
        corrections: &HashMap<SmolStr, Weight>,
    ) -> Vec<Suggestion> {
        let mut c: Vec<Suggestion> = corrections
            .into_iter()
            .map(|x| Suggestion::new(x.0.clone(), *x.1))
            .collect();

        c.sort();

        if let Some(n) = self.config.n_best {
            c.truncate(n);
        }

        c
    }
}
