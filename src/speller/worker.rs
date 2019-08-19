use hashbrown::{HashMap};
use std::f32;
use std::sync::Arc;
use smol_str::SmolStr;

use lifeguard::{Pool, Recycled};

use super::{Speller, SpellerConfig};
use crate::speller::suggestion::Suggestion;
use crate::transducer::tree_node::TreeNode;
use crate::transducer::Transducer;
use crate::types::{SpellerWorkerMode, SymbolNumber, Weight};

use ahash::ABuildHasher;
use std::hash::{Hash, Hasher, BuildHasher};


const PRIMES: &[u8] = &[
    1, 2, 3,  // 5,  7, 11, 13, 17, 19, 23, 29, //7487, 10627, 15569, 20149
//    31,  37,  41,  43,  47,  53,  59,  61,  67,  71,
//    73,  79,  83,  89,  97, 101, 103, 107, 109, 113,
//   127, 131, 137, 139, 149, 151, 157, 163, 167, 173,
//   179, 181, 191, 193, 197, 199, 211, 223, 227, 229
];


pub struct InverseBloomFilter<T> {
    array: Vec<Option<T>>,
    build_hasher: ABuildHasher,
    capacity: u64
}

impl<T: Hash + Eq> InverseBloomFilter<T> {
    pub fn new() -> InverseBloomFilter<T> {
        InverseBloomFilter::with_capacity(1_048_576)
    }

    pub fn with_capacity(capacity: u64) -> InverseBloomFilter<T> {
        InverseBloomFilter {
            array: std::iter::from_fn(|| Some(None)).take(capacity as usize).collect(),
            build_hasher: ABuildHasher::new(),
            capacity
        }
    }

    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    pub fn add(&mut self, item: T) {
        let index = self.index_for_hash(&item) as usize;
        self.array[index] = Some(item);
    }

    pub fn test(&self, item: &T) -> bool {
        let index = self.index_for_hash(item) as usize;
        match self.array[index] {
            None => false,
            Some(ref v) => v == item
        }
    }

    pub fn test_and_add(&mut self, item: T) -> bool {
        let (old_item, new_item) = self.get_and_set(self.index_for_hash(&item) as usize, Some(item));
        &old_item == new_item
    }

    #[inline(always)]
    fn index_for_hash(&self, item: &T) -> u64 {
        let mut hasher = self.build_hasher.build_hasher();
        item.hash(&mut hasher);
        hasher.finish() % self.capacity
    }

    #[inline(always)]
    fn get_and_set(&mut self, index: usize, item: Option<T>) -> (Option<T>, &Option<T>) {
        let old_item = std::mem::replace(&mut self.array[index], item);
        (old_item, &self.array[index])
    }
}

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
    config: SpellerConfig,
}

impl<'t, T: Transducer + 't> SpellerWorker<T> {
    pub fn new(
        speller: Arc<Speller<T>>,
        mode: SpellerWorkerMode,
        input: Vec<SymbolNumber>,
        config: SpellerConfig,
    ) -> Arc<SpellerWorker<T>> {
        Arc::new(SpellerWorker {
            speller,
            input,
            mode,
            config,
        })
    }

    fn lexicon_epsilons<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        max_weight: Weight,
        next_node: &TreeNode,
        nodes: &InverseBloomFilter<TreeNode>,
        output_nodes: &mut Vec<Recycled<'a, TreeNode>>,
    ) {
        let lexicon = self.speller.lexicon();
        let operations = lexicon.alphabet().operations();
        // let mut output_nodes = Vec::new();

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
                        if let SpellerWorkerMode::Correct = self.mode {
                            let epsilon_transition = transition.clone_with_epsilon_symbol();

                            let new_node = next_node.update_lexicon(pool, epsilon_transition);

                            if !nodes.test(&new_node) {
                                output_nodes.push(new_node);
                            }
                        } else {
                            let new_node = next_node.update_lexicon(pool, transition);

                            if !nodes.test(&new_node) {
                                output_nodes.push(new_node);
                            }
                        }
                    }
                } else {
                    let operation = operations.get(&sym);

                    if let Some(op) = operation {
                        if !self.is_under_weight_limit(max_weight, transition_weight) {
                            next += 1;
                            continue;
                        }

                        if let Some(applied_node) = next_node.apply_operation(pool, op, &transition) {
                            // applied_node.update_lexicon_mut(&transition);

                            if !nodes.test(&applied_node) {
                                output_nodes.push(applied_node);
                            }
                        }
                    }
                }
            }

            next += 1;
        }
    }

    fn mutator_epsilons<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        max_weight: Weight,
        next_node: &TreeNode,
        nodes: &InverseBloomFilter<TreeNode>,
        output_nodes: &mut Vec<Recycled<'a, TreeNode>>,
    ) {
        let mutator = self.speller.mutator();
        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();
        // let mut output_nodes = Vec::new();

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
                    if !nodes.test(&new_node) {
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
                            self.queue_lexicon_arcs(
                                pool,
                                max_weight,
                                &next_node,
                                nodes,
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
                                nodes,
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
                    nodes,
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

    pub fn queue_lexicon_arcs<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        max_weight: Weight,
        next_node: &TreeNode,
        nodes: &InverseBloomFilter<TreeNode>,
        input_sym: SymbolNumber,
        mutator_state: u32,
        mutator_weight: Weight,
        input_increment: i16,
        output_nodes: &mut Vec<Recycled<'a, TreeNode>>,
    ) {
        let lexicon = self.speller.lexicon();
        let identity = lexicon.alphabet().identity();
        let mut next = lexicon.next(next_node.lexicon_state, input_sym).unwrap();
        // let mut output_nodes: Vec<Recycled<'a, TreeNode>> = Vec::new();

        while let Some(noneps_trans) = lexicon.take_non_epsilons(next, input_sym) {
            if let Some(mut sym) = noneps_trans.symbol() {
                // Symbol replacement here is unfortunate but necessary.
                if let Some(id) = identity {
                    if sym == id {
                        sym = self.input[next_node.input_state as usize];
                    }
                }

                let next_sym = match self.mode {
                    SpellerWorkerMode::Correct => input_sym,
                    _ => sym,
                };

                let can_push = match self.mode {
                    SpellerWorkerMode::Correct => self.is_under_weight_limit(
                        max_weight,
                        noneps_trans.weight().unwrap() + mutator_weight,
                    ),
                    _ => self.is_under_weight_limit(
                        max_weight,
                        next_node.weight() + noneps_trans.weight().unwrap() + mutator_weight,
                    ),
                };

                if can_push {
                    let new_node = next_node.update(
                        pool,
                        next_sym,
                        Some(next_node.input_state + input_increment as u32),
                        mutator_state,
                        noneps_trans.target().unwrap(),
                        noneps_trans.weight().unwrap() + mutator_weight,
                    );

                    if !nodes.test(&new_node) {
                        output_nodes.push(new_node);
                    }
                }
            }

            next += 1;
        }
    }

    fn queue_mutator_arcs<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        max_weight: Weight,
        next_node: &TreeNode,
        nodes: &InverseBloomFilter<TreeNode>,
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

                    if !nodes.test(&new_node) {
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
                            self.queue_lexicon_arcs(
                                pool,
                                max_weight,
                                &next_node,
                                nodes,
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
                                nodes,
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
                    nodes,
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

    fn consume_input<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        max_weight: Weight,
        next_node: &TreeNode,
        nodes: &InverseBloomFilter<TreeNode>,
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
                        nodes,
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
                        nodes,
                        mutator.alphabet().unknown().unwrap(),
                        output_nodes,
                    );
                }
            }
        } else {
            self.queue_mutator_arcs(pool, max_weight, &next_node, nodes, input_sym, output_nodes)
        }
    }

    fn lexicon_consume<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        max_weight: Weight,
        next_node: &TreeNode,
        nodes: &InverseBloomFilter<TreeNode>,
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
                        nodes,
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
                        nodes,
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
            nodes,
            input_sym,
            next_node.mutator_state,
            0.0,
            1,
            output_nodes,
        );
    }

    fn update_weight_limit(&self, best_weight: Weight, suggestions: &[Suggestion]) -> Weight {
        use std::cmp::Ordering::{Equal, Less};

        let c = &self.config;
        let mut max_weight = c.max_weight.unwrap_or(f32::INFINITY);

        if let Some(beam) = c.beam {
            let candidate_weight = best_weight + beam;

            max_weight = match max_weight.partial_cmp(&candidate_weight).unwrap_or(Equal) {
                Less => max_weight,
                _ => candidate_weight,
            };
        }

        if let Some(n) = c.n_best {
            if suggestions.len() >= n {
                if let Some(sugg) = suggestions.last() {
                    return sugg.weight();
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
        let max_weight = speller_max_weight(&self.config);
        let pool = Pool::with_size_and_max(0, 0);
        let mut nodes = speller_start_node(&pool, self.state_size() as usize);

        let mut seen_nodes: InverseBloomFilter<TreeNode> = InverseBloomFilter::with_capacity(1_000_000);

        while let Some(next_node) = nodes.pop() {
            if next_node.input_state as usize == self.input.len()
                && self.speller.lexicon().is_final(next_node.lexicon_state)
            {
                return true;
            }

            self.lexicon_epsilons(&pool, max_weight, &next_node, &seen_nodes, &mut nodes);
            self.lexicon_consume(&pool, max_weight, &next_node, &seen_nodes, &mut nodes);

            seen_nodes.add(next_node.key());
        }

        false
    }

    pub fn suggest(self: Arc<Self>) -> Vec<Suggestion> {
        let pool = Pool::with_size_and_max(self.config.pool_start, self.config.pool_max);
        let mut nodes = speller_start_node(&pool, self.state_size() as usize);
        let mut corrections = HashMap::new();
        let mut suggestions: Vec<Suggestion> = vec![];
        let mut best_weight = self.config.max_weight.unwrap_or(f32::INFINITY);

        let mut seen_nodes: InverseBloomFilter<TreeNode> = InverseBloomFilter::with_capacity(2u64.pow(u32::from(self.config.seen_node_sample_rate)));
        let mut next_rando = PRIMES.iter();
        let mut max_rando = next_rando.next().copied().unwrap();
        let mut rando = 0;

        // let mut rando = self.config.seen_node_sample_rate;
        while let Some(next_node) = nodes.pop() {
            if rando == max_rando {
                seen_nodes.add(next_node.key());
                rando = 0;
                max_rando = next_rando.next().copied().unwrap_or(1);
            } else {
                rando += 1;
            }

            let max_weight = self.update_weight_limit(best_weight, &suggestions);

            if !self.is_under_weight_limit(max_weight, next_node.weight()) {
                continue;
            }

            self.lexicon_epsilons(&pool, max_weight, &next_node, &seen_nodes, &mut nodes);
            self.mutator_epsilons(&pool, max_weight, &next_node, &seen_nodes, &mut nodes);

            if next_node.input_state as usize != self.input.len() {
                self.consume_input(&pool, max_weight, &next_node, &seen_nodes, &mut nodes);
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

            let key_table = self.speller.lexicon().alphabet().key_table();
            let string: SmolStr = next_node
                .string
                .iter()
                .map(|s| &*key_table[*s as usize])
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
