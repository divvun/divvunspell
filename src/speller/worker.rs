use std::collections::BinaryHeap;

use hashbrown::{HashMap, HashSet};
use smol_str::SmolStr;
use std::sync::Arc;

use lifeguard::{Pool, Recycled};

use super::subset::{MutatorSubsets, SubsetStats};
use super::{HfstSpeller, OutputMode, SpellerConfig};
use crate::speller::suggestion::{Suggestion, WeightDetails};
use crate::transducer::Transducer;
use crate::transducer::tree_node::TreeNode;
use crate::types::{SymbolNumber, TransitionTableIndex, ValueNumber, Weight};

#[inline(always)]
fn speller_start_node(pool: &Pool<TreeNode>, size: usize) -> Vec<Recycled<'_, TreeNode>> {
    let start_node = TreeNode::empty(pool, vec![ValueNumber::ZERO; size]);
    let mut nodes = Vec::with_capacity(256);
    nodes.push(start_node);
    nodes
}

/// Min-order wrapper so `BinaryHeap` (a max-heap) pops the most promising node
/// first.
///
/// The order is A*'s `f = g + h`: `g` is the weight accumulated so far and `h`
/// is [`Transducer::distance_to_final`] summed over the two transducers — a
/// lower bound on what finishing must still cost. Plain best-first (`h = 0`)
/// has no lookahead and drowns in shallow, cheap, hopeless paths before any
/// final state tightens the cutoff; `h` prices the rest of the word in.
struct OrderedNode<'a> {
    /// `g + h`. Never overestimates the weight of any completion of this node,
    /// which is what makes it safe to both prune and stop on.
    estimate: Weight,
    node: Recycled<'a, TreeNode>,
}

impl PartialEq for OrderedNode<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.estimate == other.estimate && self.node.weight() == other.node.weight()
    }
}
impl Eq for OrderedNode<'_> {}
impl PartialOrd for OrderedNode<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for OrderedNode<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reversed: cheapest estimate first out of the max-heap. Ties go to the
        // node that has already travelled further, which reaches a complete
        // correction sooner and so tightens the cutoff sooner.
        other
            .estimate
            .cmp(&self.estimate)
            .then_with(|| self.node.weight().cmp(&other.node.weight()))
    }
}

/// Opt-in accounting of what the suggestion search spends its iterations on,
/// switched on with `DIVVUNSPELL_SEARCH_STATS=1` and written to stderr as one
/// `SEARCHSTATS` line per search.
///
/// An iteration count on its own cannot say *why* a search is expensive. The
/// two questions that tell a genuinely large error model apart from a badly
/// shaped one are both answered here:
///
/// * `distinct_sigs` against `pops` — is the search reaching new
///   configurations, or re-walking the same ones along different paths?
/// * `live_mutator_states` — how many error-model states are alive for one and
///   the same partial correction. Near 1 means the model behaves like a DFA;
///   well above 1 means it is an NFA and the search pays for it on every node.
///
/// Everything accumulated here is allocated only when it is switched on.
static SEARCH_STATS: std::sync::LazyLock<bool> =
    std::sync::LazyLock::new(|| std::env::var_os("DIVVUNSPELL_SEARCH_STATS").is_some());

#[derive(Default)]
struct SearchStats {
    pops: u64,
    /// Pops of a triple already popped at no greater weight — pure path
    /// redundancy, and exactly what a visited-set would eliminate.
    redundant_pops: u64,
    push_lexicon_epsilons: u64,
    push_mutator_epsilons: u64,
    push_consume_input: u64,
    pushes_kept: u64,
    max_queue: usize,
    corrections: u64,
    first_correction_pop: Option<u64>,
    seen: HashMap<(u32, u32, u32), (Weight, u32)>,
    /// Distinct `(triple, output-so-far)` signatures. A pop whose signature has
    /// been seen before cannot contribute a correction the earlier one could
    /// not, so this separates "the model is big" from "the search is walking
    /// the same partial correction over and over".
    signatures: HashSet<(u32, u32, u32, u64)>,
    /// Signatures with the mutator state dropped. `signatures / this` is the
    /// average number of mutator states alive for one and the same partial
    /// correction — the price of running a non-determinised error model as an
    /// NFA, and the ceiling on what on-the-fly determinisation could recover.
    signatures_no_mutator: HashSet<(u32, u32, u64)>,
    mutator_states: HashSet<u32>,
    lexicon_states: HashSet<u32>,
    /// What the on-the-fly determinisation of the error model cost, when it is
    /// the one being walked.
    subsets: Option<SubsetStats>,
}

impl SearchStats {
    fn record_pop(&mut self, node: &TreeNode) {
        use std::hash::{BuildHasher, Hash, Hasher};

        self.pops += 1;
        let key = (
            node.input_state.0,
            node.mutator_state.0,
            node.lexicon_state.0,
        );
        match self.seen.get_mut(&key) {
            Some((best, count)) => {
                *count += 1;
                if *best <= node.weight() {
                    self.redundant_pops += 1;
                } else {
                    *best = node.weight();
                }
            }
            None => {
                self.seen.insert(key, (node.weight(), 1));
            }
        }

        let mut hasher = self.signatures.hasher().build_hasher();
        node.string.hash(&mut hasher);
        for value in &node.flag_state {
            value.0.hash(&mut hasher);
        }
        let output = hasher.finish();
        self.signatures.insert((key.0, key.1, key.2, output));
        self.signatures_no_mutator.insert((key.0, key.2, output));

        self.mutator_states.insert(node.mutator_state.0);
        self.lexicon_states.insert(node.lexicon_state.0);
    }

    fn report(&self, word: &str, queue_len: usize) {
        let hottest = self.seen.values().map(|(_, c)| *c).max().unwrap_or(0);
        let subsets = match self.subsets {
            Some(s) => format!(
                "\tsubsets={}\tsubset_members={}\tsubset_avg_size={:.2}\t\
                 subset_lookups={}\tsubset_misses={}\tsubset_hit_pct={:.1}",
                s.subsets,
                s.members,
                s.members as f64 / s.subsets.max(1) as f64,
                s.lookups,
                s.misses,
                100.0 * (s.lookups - s.misses) as f64 / s.lookups.max(1) as f64,
            ),
            None => String::new(),
        };
        eprintln!(
            "SEARCHSTATS\tword={word}\tpops={}\tdistinct_triples={}\tdistinct_sigs={}\t\
             sigs_no_mutator={}\tlive_mutator_states={:.2}\t\
             hottest_triple_pops={}\tredundant_pops={}\t\
             redundant_pct={:.1}\tmutator_states={}\tlexicon_states={}\t\
             push_lex_eps={}\tpush_mut_eps={}\tpush_consume={}\tpushes_kept={}\t\
             max_queue={}\tqueue_left={}\tcorrections={}\tfirst_correction_pop={}{}",
            self.pops,
            self.seen.len(),
            self.signatures.len(),
            self.signatures_no_mutator.len(),
            self.signatures.len() as f64 / self.signatures_no_mutator.len().max(1) as f64,
            hottest,
            self.redundant_pops,
            100.0 * self.redundant_pops as f64 / (self.pops.max(1)) as f64,
            self.mutator_states.len(),
            self.lexicon_states.len(),
            self.push_lexicon_epsilons,
            self.push_mutator_epsilons,
            self.push_consume_input,
            self.pushes_kept,
            self.max_queue,
            queue_len,
            self.corrections,
            self.first_correction_pop
                .map(|p| p.to_string())
                .unwrap_or_else(|| "-".to_string()),
            subsets,
        );
    }
}

/// The set of search states already reached, and the cheapest way found to
/// each — what turns the suggestion search from a walk of *paths* into a walk
/// of *states*.
///
/// Two nodes that agree on input position, mutator state, lexicon state, flag
/// state **and the output spelled so far** are interchangeable: every
/// completion of one is a completion of the other, producing the same
/// correction string at a weight that differs by exactly the difference
/// between the two. So the dearer of the pair can only ever yield a
/// worse-weighted copy of what the cheaper one yields, and dropping it loses
/// no correction and no best weight.
///
/// Keying on the output string is what makes that argument hold, and is what
/// separates this from the usual product-state visited set. Two paths that
/// reach the same pair of transducer states having spelled *different* words
/// stay apart, so distinct corrections are never collapsed into one — the
/// failure mode that makes a naive visited set unusable here.
///
/// Weight-pushed error models make this matter enormously. An error model that
/// has been determinised (the "expanded" build) offers roughly one path per
/// state, so the distinction is invisible. One that has not — a plain union of
/// components, which is 19x smaller on disk — offers combinatorially many
/// paths to the same state, and a path-walk drowns in them.
struct Closed {
    /// Keys live in `arena`; a table entry only points at one. Interning them
    /// this way keeps the whole structure to a handful of growing allocations
    /// instead of one per state reached, which on an easy word is most of what
    /// tracking states would otherwise cost.
    table: hashbrown::HashTable<ClosedEntry>,
    /// Concatenated keys, each `[input, mutator, lexicon, flags.., output..]`.
    /// Flag state has a fixed width for the whole search, so the layout needs
    /// no separator.
    arena: Vec<u16>,
    /// The key being looked up, rebuilt per query so lookups never allocate.
    scratch: Vec<u16>,
    hasher: hashbrown::DefaultHashBuilder,
}

struct ClosedEntry {
    start: u32,
    len: u32,
    hash: u64,
    weight: Weight,
}

impl Closed {
    fn new() -> Closed {
        Closed {
            table: hashbrown::HashTable::new(),
            arena: Vec::new(),
            scratch: Vec::with_capacity(64),
            hasher: hashbrown::DefaultHashBuilder::default(),
        }
    }

    /// Flatten a node's search state into `scratch` and hash it.
    ///
    /// State indices are `u32` and everything else is 16 bits wide, so the key
    /// is built out of `u16` halves — half the bytes to copy and to hash
    /// compared with widening everything to `u32`.
    #[inline(always)]
    fn build_key(&mut self, node: &TreeNode) -> u64 {
        use std::hash::BuildHasher;

        self.scratch.clear();
        for index in [
            node.input_state.0,
            node.mutator_state.0,
            node.lexicon_state.0,
        ] {
            self.scratch.push(index as u16);
            self.scratch.push((index >> 16) as u16);
        }
        self.scratch
            .extend(node.flag_state.iter().map(|value| value.0 as u16));
        self.scratch.extend(node.string.iter().map(|sym| sym.0));

        self.hasher.hash_one(self.scratch.as_slice())
    }

    /// Whether this node is worth queueing: true unless some path already
    /// reached the same state at no greater weight.
    #[inline(always)]
    fn admit(&mut self, node: &TreeNode) -> bool {
        let hash = self.build_key(node);
        // Split the borrow so the equality test can read the arena while the
        // table is held mutably, keeping this to a single lookup.
        let Closed {
            table,
            arena,
            scratch,
            ..
        } = self;

        if let Some(entry) = table.find_mut(hash, |entry| {
            entry.hash == hash && arena[entry.start as usize..][..entry.len as usize] == scratch[..]
        }) {
            if entry.weight <= node.weight() {
                return false;
            }
            entry.weight = node.weight();
            return true;
        }

        let start = arena.len() as u32;
        arena.extend_from_slice(scratch);
        table.insert_unique(
            hash,
            ClosedEntry {
                start,
                len: scratch.len() as u32,
                hash,
                weight: node.weight(),
            },
            |entry| entry.hash,
        );
        true
    }

    /// Whether this node still carries the best known weight for its state, or
    /// has been superseded by a cheaper path queued after it.
    #[inline(always)]
    fn is_current(&mut self, node: &TreeNode) -> bool {
        let hash = self.build_key(node);
        let Closed {
            table,
            arena,
            scratch,
            ..
        } = self;

        table
            .find(hash, |entry| {
                entry.hash == hash
                    && arena[entry.start as usize..][..entry.len as usize] == scratch[..]
            })
            .is_none_or(|entry| entry.weight >= node.weight())
    }
}

pub struct SpellerWorker<'c, T: Transducer, U: Transducer> {
    speller: Arc<HfstSpeller<T, U>>,
    input: Vec<SymbolNumber>,
    /// Lexicon-alphabet copy of the input, one symbol per grapheme.
    ///
    /// For the suggest path (`input` is mutator-alphabet), this lets
    /// `queue_mutator_arcs` substitute the real lexicon symbol when the
    /// mutator passes an input through via identity/unknown. Without it,
    /// a character like "Z" that is outside the mutator's alphabet would
    /// map to the mutator's UNKNOWN marker, translate into a synthetic
    /// lexicon symbol, and fail to match the lexicon's explicit `Z` arcs
    /// (lang-sma#160).
    ///
    /// For `is_correct`/`analyze` workers this is just a copy of `input`.
    lexicon_input: Vec<SymbolNumber>,
    config: &'c SpellerConfig,
    output_mode: OutputMode,
    /// When true, `input` already holds lexicon-alphabet symbols, so
    /// `lexicon_consume` skips the mutator-to-lexicon translator step.
    input_is_lexicon_alphabet: bool,
    /// Prices candidates the way `suggest_case` will after the search, so the
    /// n-best cutoff prunes in final (post-reweight) order. `None` on the
    /// lexicon-only paths (`is_correct`/`analyze`), which never reweight.
    reweight_ctx: Option<super::ReweightContext>,
}

#[allow(clippy::too_many_arguments)]
impl<'c, T: Transducer, U: Transducer> SpellerWorker<'c, T, U>
where
    T: Transducer,
    U: Transducer,
{
    /// Construct a worker whose `input` is in the **mutator** alphabet.
    ///
    /// Use this for the suggest path, where `consume_input` and
    /// `queue_mutator_arcs` walk the mutator transducer directly.
    /// `lexicon_consume` translates via `alphabet_translator` when it needs
    /// to query the lexicon.
    ///
    /// `lexicon_input` must be the same word tokenised through the **lexicon**
    /// alphabet (via `HfstSpeller::to_input_vec_lexicon`); the lexicon walk
    /// falls back to it when the mutator passes a grapheme through via
    /// identity/unknown.
    #[inline(always)]
    pub(crate) fn new_mutator_input(
        speller: Arc<HfstSpeller<T, U>>,
        input: Vec<SymbolNumber>,
        lexicon_input: Vec<SymbolNumber>,
        config: &'c SpellerConfig,
        output_mode: OutputMode,
    ) -> SpellerWorker<'c, T, U> {
        debug_assert_eq!(input.len(), lexicon_input.len());
        SpellerWorker {
            speller,
            input,
            lexicon_input,
            config,
            output_mode,
            input_is_lexicon_alphabet: false,
            reweight_ctx: None,
        }
    }

    pub(crate) fn with_reweight_ctx(mut self, ctx: super::ReweightContext) -> Self {
        self.reweight_ctx = Some(ctx);
        self
    }

    /// Construct a worker whose `input` is already in the **lexicon** alphabet.
    ///
    /// Use this for lexicon-only traversals (`is_correct`, `analyze`) where
    /// `input` came from `HfstSpeller::to_input_vec_lexicon`. `lexicon_consume`
    /// skips translator indirection for these workers.
    #[inline(always)]
    pub(crate) fn new_lexicon_input(
        speller: Arc<HfstSpeller<T, U>>,
        input: Vec<SymbolNumber>,
        config: &'c SpellerConfig,
        output_mode: OutputMode,
    ) -> SpellerWorker<'c, T, U> {
        SpellerWorker {
            speller,
            lexicon_input: input.clone(),
            input,
            config,
            output_mode,
            input_is_lexicon_alphabet: true,
            reweight_ctx: None,
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

        if !lexicon.has_epsilons_or_flags(next_node.lexicon_state.incr()) {
            return;
        }

        let mut next = lexicon
            .next(next_node.lexicon_state, SymbolNumber::ZERO)
            .unwrap();

        while let Some(transition) = lexicon.take_epsilons_and_flags(next) {
            if let Some(sym) = lexicon.transition_input_symbol(next) {
                let transition_weight = transition.weight().unwrap();

                if sym == SymbolNumber::ZERO {
                    if self
                        .is_under_weight_limit(max_weight, next_node.weight() + transition_weight)
                    {
                        let new_node = match self.output_mode {
                            OutputMode::WithoutTags => next_node
                                .update_lexicon(pool, transition.clone_with_epsilon_symbol()),
                            OutputMode::WithTags => next_node.update_lexicon(pool, transition),
                        };
                        output_nodes.push(new_node);
                    }
                } else {
                    let operation = operations.get(&sym);

                    if let Some(op) = operation {
                        if !self.is_under_weight_limit(max_weight, transition_weight) {
                            next = next.incr();
                            continue;
                        }

                        if let Some(applied_node) = next_node.apply_operation(pool, op, &transition)
                        {
                            output_nodes.push(applied_node);
                        }
                    }
                }
            }

            next = next.incr();
        }
    }

    /// Hand `visit` every error-model arc leaving `state` on `input_sym`, as
    /// `(output symbol, next model state, weight)`.
    ///
    /// `state` names a model state when the search walks the model as an NFA
    /// and an interned subset when it determinises the model on the fly. The
    /// two agree on everything downstream of this call — an arc is an output
    /// symbol, a successor and a weight either way — which is what lets the
    /// product walk below be written once.
    ///
    /// False means the subset construction breached a cap; the caller must
    /// abandon the search and redo it as the NFA walk.
    #[inline(always)]
    fn for_each_mutator_arc(
        &self,
        subsets: Option<&mut MutatorSubsets>,
        state: TransitionTableIndex,
        input_sym: SymbolNumber,
        mut visit: impl FnMut(SymbolNumber, TransitionTableIndex, Weight),
    ) -> bool {
        let mutator = self.speller.mutator();

        let Some(subsets) = subsets else {
            if !mutator.has_transitions(state.incr(), Some(input_sym)) {
                return true;
            }
            let Some(mut next) = mutator.next(state, input_sym) else {
                return true;
            };

            loop {
                let transition = if input_sym == SymbolNumber::ZERO {
                    mutator.take_epsilons(next)
                } else {
                    mutator.take_non_epsilons(next, input_sym)
                };
                let Some(transition) = transition else {
                    break;
                };

                if let (Some(symbol), Some(target), Some(weight)) = (
                    transition.symbol(),
                    transition.target(),
                    transition.weight(),
                ) {
                    visit(symbol, target, weight);
                }

                next = next.incr();
            }

            return true;
        };

        let Some((start, len)) = subsets.transitions(mutator, state, input_sym) else {
            return false;
        };

        for index in start..start + len {
            let arc = subsets.arc(index);
            visit(arc.symbol, arc.target, arc.weight);
        }

        true
    }

    /// Queue what the lexicon can do with one error-model output symbol.
    ///
    /// Shared by the two ways the model produces one: against an epsilon input
    /// (an insertion) and against a consumed input character.
    ///
    /// `input_lexicon_sym` is the input character's lexicon symbol when there
    /// is an input character being consumed. It names what `@_IDENTITY_@`
    /// writes and what `@_UNKNOWN_@` may not.
    #[inline(always)]
    fn queue_mutator_output<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        max_weight: Weight,
        next_node: &TreeNode,
        sym: SymbolNumber,
        target: TransitionTableIndex,
        weight: Weight,
        input_increment: i16,
        input_lexicon_sym: Option<SymbolNumber>,
        output_nodes: &mut Vec<Recycled<'a, TreeNode>>,
    ) {
        let mutator = self.speller.mutator();
        let lexicon = self.speller.lexicon();
        let alphabet_translator = self.speller.alphabet_translator();
        let mut_alpha = mutator.alphabet();

        // `@_UNKNOWN_@` on the output tape is not a character to write: it
        // stands for some symbol outside the model's alphabet, and the lexicon
        // says which ones are available here.
        if mut_alpha.unknown() == Some(sym) {
            self.queue_unknown_output_arcs(
                pool,
                max_weight,
                next_node,
                target,
                weight,
                input_increment,
                input_lexicon_sym,
                output_nodes,
            );
            return;
        }

        // `@_IDENTITY_@` on the output tape does name a character: the input
        // one, unchanged. Encode it in the lexicon alphabet so the lexicon walk
        // can match explicit arcs for it — without this, out-of-model-alphabet
        // characters like "Z" in the festschrift model silently dead-end when
        // the lexicon has no identity arcs but does have an explicit "Z" arc
        // (lang-sma#160).
        let trans_sym = match input_lexicon_sym {
            Some(lexicon_sym) if mut_alpha.identity() == Some(sym) => lexicon_sym,
            _ => alphabet_translator[sym.0 as usize],
        };

        let lookup = next_node.lexicon_state.incr();

        if !lexicon.has_transitions(lookup, Some(trans_sym)) {
            // No regular transitions for this: an input outside the lexicon's
            // original alphabet may still travel on unknown or identity.
            if trans_sym >= lexicon.alphabet().initial_symbol_count() {
                if let Some(unknown) = lexicon.alphabet().unknown()
                    && lexicon.has_transitions(lookup, Some(unknown))
                {
                    self.queue_lexicon_arcs(
                        pool,
                        max_weight,
                        next_node,
                        unknown,
                        target,
                        weight,
                        input_increment,
                        output_nodes,
                    );
                }

                if let Some(identity) = lexicon.alphabet().identity()
                    && lexicon.has_transitions(lookup, Some(identity))
                {
                    self.queue_lexicon_arcs(
                        pool,
                        max_weight,
                        next_node,
                        identity,
                        target,
                        weight,
                        input_increment,
                        output_nodes,
                    );
                }
            }

            return;
        }

        self.queue_lexicon_arcs(
            pool,
            max_weight,
            next_node,
            trans_sym,
            target,
            weight,
            input_increment,
            output_nodes,
        );
    }

    #[inline(always)]
    fn mutator_epsilons<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        max_weight: Weight,
        next_node: &TreeNode,
        subsets: Option<&mut MutatorSubsets>,
        output_nodes: &mut Vec<Recycled<'a, TreeNode>>,
    ) -> bool {
        self.for_each_mutator_arc(
            subsets,
            next_node.mutator_state,
            SymbolNumber::ZERO,
            |sym, target, weight| {
                if sym == SymbolNumber::ZERO {
                    if self.is_under_weight_limit(max_weight, next_node.weight() + weight) {
                        output_nodes.push(next_node.update_mutator(pool, target, weight));
                    }
                    return;
                }

                // An `@_UNKNOWN_@` output against an epsilon input inserts
                // "some symbol outside the alphabet" — no character in
                // particular, and none to exclude either, since no input
                // character is being consumed here.
                self.queue_mutator_output(
                    pool,
                    max_weight,
                    next_node,
                    sym,
                    target,
                    weight,
                    0,
                    None,
                    output_nodes,
                );
            },
        )
    }

    #[inline(always)]
    fn queue_lexicon_arcs<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        max_weight: Weight,
        next_node: &TreeNode,
        input_sym: SymbolNumber,
        mutator_state: TransitionTableIndex,
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
                        sym = self.input[next_node.input_state.0 as usize];
                    }
                }

                let is_under_weight_limit = self.is_under_weight_limit(
                    max_weight,
                    next_node.weight() + noneps_trans.weight().unwrap() + mutator_weight,
                );

                if is_under_weight_limit {
                    let new_node = match self.output_mode {
                        OutputMode::WithoutTags => next_node.update(
                            pool,
                            input_sym,
                            Some(next_node.input_state.incr(input_increment as u32)),
                            mutator_state,
                            noneps_trans.target().unwrap(),
                            noneps_trans.weight().unwrap() + mutator_weight,
                            mutator_weight,
                        ),
                        OutputMode::WithTags => next_node.update(
                            pool,
                            sym,
                            Some(next_node.input_state.incr(input_increment as u32)),
                            mutator_state,
                            noneps_trans.target().unwrap(),
                            noneps_trans.weight().unwrap() + mutator_weight,
                            mutator_weight,
                        ),
                    };
                    output_nodes.push(new_node);
                }
            }

            next = next.incr();
        }
    }

    /// Queue the lexicon arcs an `@_UNKNOWN_@` on the mutator's *output* tape
    /// stands for.
    ///
    /// The marker is not a character the correction can contain. It denotes
    /// "some symbol outside the mutator's alphabet", and which symbols those
    /// are is settled by the transducer it is composed with: the candidates are
    /// the ones the lexicon offers an arc for at this very state and the
    /// mutator's alphabet cannot name. Enumerating the mutator's own alphabet
    /// instead would let the model write, for free, characters it has explicit
    /// (and priced) arcs for.
    ///
    /// `exclude` is the input character's lexicon symbol, and dropping it is
    /// the whole difference between the two wildcard classes: `@_UNKNOWN_@`
    /// means a *different* out-of-alphabet symbol, and leaving the character
    /// alone is `@_IDENTITY_@`'s reading, at the identity arc's own weight.
    /// For an `x:@_UNKNOWN_@` arc the exclusion costs nothing — an `x` the
    /// mutator can name is outside the domain already.
    #[inline]
    fn queue_unknown_output_arcs<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        max_weight: Weight,
        next_node: &TreeNode,
        mutator_state: TransitionTableIndex,
        mutator_weight: Weight,
        input_increment: i16,
        exclude: Option<SymbolNumber>,
        output_nodes: &mut Vec<Recycled<'a, TreeNode>>,
    ) {
        // Every candidate is charged this arc plus a lexicon arc, and lexicon
        // weights are non-negative, so an arc already over the cutoff cannot
        // produce anything under it — worth checking once instead of once per
        // candidate.
        if !self.is_under_weight_limit(max_weight, next_node.weight() + mutator_weight) {
            return;
        }

        let lexicon = self.speller.lexicon();
        let lookup = next_node.lexicon_state.incr();

        for &candidate in self.speller.unknown_output_domain() {
            if Some(candidate) == exclude {
                continue;
            }

            if !lexicon.has_transitions(lookup, Some(candidate)) {
                continue;
            }

            self.queue_lexicon_arcs(
                pool,
                max_weight,
                next_node,
                candidate,
                mutator_state,
                mutator_weight,
                input_increment,
                output_nodes,
            );
        }
    }

    #[inline(always)]
    fn queue_mutator_arcs<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        max_weight: Weight,
        next_node: &TreeNode,
        subsets: Option<&mut MutatorSubsets>,
        input_sym: SymbolNumber,
        output_nodes: &mut Vec<Recycled<'a, TreeNode>>,
    ) -> bool {
        let input_lexicon_sym = self
            .lexicon_input
            .get(next_node.input_state.0 as usize)
            .copied();

        self.for_each_mutator_arc(
            subsets,
            next_node.mutator_state,
            input_sym,
            |sym, target, weight| {
                if sym == SymbolNumber::ZERO {
                    if self.is_under_weight_limit(max_weight, next_node.weight() + weight) {
                        output_nodes.push(next_node.update(
                            pool,
                            SymbolNumber::ZERO,
                            Some(next_node.input_state.incr(1)),
                            target,
                            next_node.lexicon_state,
                            weight,
                            weight,
                        ));
                    }
                    return;
                }

                self.queue_mutator_output(
                    pool,
                    max_weight,
                    next_node,
                    sym,
                    target,
                    weight,
                    1,
                    input_lexicon_sym,
                    output_nodes,
                );
            },
        )
    }

    #[inline(always)]
    fn consume_input<'a>(
        &self,
        pool: &'a Pool<TreeNode>,
        max_weight: Weight,
        next_node: &TreeNode,
        mut subsets: Option<&mut MutatorSubsets>,
        output_nodes: &mut Vec<Recycled<'a, TreeNode>>,
    ) -> bool {
        let mutator = self.speller.mutator();
        let input_state = next_node.input_state.0 as usize;

        if input_state >= self.input.len() {
            return true;
        }

        let input_sym = self.input[input_state];
        let alphabet = mutator.alphabet();

        // A grapheme the error model has never seen was replaced by the model's
        // UNKNOWN marker in `to_input_vec` (or by epsilon, for a model with no
        // UNKNOWN symbol at all). That marker is not a symbol to match
        // literally: it stands for "some character outside the alphabet", and
        // *both* of the model's wildcard arc classes apply to it —
        // `@_IDENTITY_@` passes the character through unchanged, `@_UNKNOWN_@`
        // replaces it with a different one.
        //
        // Matching the marker literally happens to hit exactly the
        // `@_UNKNOWN_@` arcs, so treating that as "the" transition and stopping
        // there silently drops every pass-through path. Which class a model
        // offers at a given state is an artefact of how it was compiled: a
        // determinised error model floats both up to its start state, where the
        // literal match then wins and the free pass-through is lost — while the
        // same relation left as a union of components offers only identity
        // there and keeps it. Same relation, different suggestions, and the
        // determinised build charges a substitution for a character it should
        // have passed through for nothing. Explore both classes and neither
        // compilation shows.
        let input_is_out_of_alphabet = match alphabet.unknown() {
            Some(unknown) => input_sym == unknown,
            None => input_sym == SymbolNumber::ZERO,
        };

        if input_is_out_of_alphabet
            && let Some(identity) = alphabet.identity()
            && !self.queue_mutator_arcs(
                pool,
                max_weight,
                next_node,
                subsets.as_deref_mut(),
                identity,
                output_nodes,
            )
        {
            return false;
        }

        self.queue_mutator_arcs(
            pool,
            max_weight,
            next_node,
            subsets,
            input_sym,
            output_nodes,
        )
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
        let input_state = next_node.input_state.0 as usize;

        if input_state >= self.input.len() {
            return;
        }

        let input_sym = if self.input_is_lexicon_alphabet {
            self.input[input_state]
        } else {
            let alphabet_translator = self.speller.alphabet_translator();
            alphabet_translator[self.input[input_state].0 as usize]
        };
        let next_lexicon_state = next_node.lexicon_state.incr();
        //        tracing::trace!(
        //            "lexicon consuming {}: {}",
        //            input_sym,
        //            self.speller
        //                .lexicon
        //                .alphabet()
        //                .string_from_symbols(&[input_sym])
        //        );

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
                        Weight::ZERO,
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
                        Weight::ZERO,
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
            Weight::ZERO,
            1,
            output_nodes,
        );
    }

    #[inline(always)]
    fn update_weight_limit(&self, best_weight: Weight, nth_best_weight: Option<Weight>) -> Weight {
        use std::cmp::Ordering::{Equal, Less};

        let c = &self.config;
        let mut max_weight = c.max_weight.unwrap_or(Weight::MAX);

        // beam == 0 means disabled, matching `apply_weight_limits` and FFI
        // behaviour. Under best-first traversal an active beam of zero would
        // otherwise end the search the moment the best path is found.
        if let Some(beam) = c.beam.filter(|beam| *beam > Weight::ZERO) {
            let candidate_weight = best_weight + beam;

            max_weight = match max_weight.partial_cmp(&candidate_weight).unwrap_or(Equal) {
                Less => max_weight,
                _ => candidate_weight,
            };
        }

        if let Some(w) = nth_best_weight {
            if w < max_weight {
                return w;
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
        self.speller.lexicon().alphabet().state_size().0 as usize
    }

    pub(crate) fn is_correct(&self) -> bool {
        tracing::trace!("is_correct");
        // let max_weight = speller_max_weight(&self.config);
        let pool = Pool::with_size_and_max(0, 0);
        let mut nodes = speller_start_node(&pool, self.state_size() as usize);
        tracing::trace!("beginning is_correct {:?}?", self.input);
        while let Some(next_node) = nodes.pop() {
            if next_node.input_state.0 as usize == self.input.len()
                && self.speller.lexicon().is_final(next_node.lexicon_state)
            {
                return true;
            }

            self.lexicon_epsilons(&pool, Weight::INFINITE, &next_node, &mut nodes);
            self.lexicon_consume(&pool, Weight::INFINITE, &next_node, &mut nodes);
        }

        false
    }

    pub(crate) fn analyze(&self) -> Vec<Suggestion> {
        tracing::trace!("Beginning analyze");
        let pool = Pool::with_size_and_max(0, 0);
        let mut nodes = speller_start_node(&pool, self.state_size() as usize);
        tracing::trace!("beginning analyze {:?}", self.input);
        let mut lookups = HashMap::new();
        while let Some(next_node) = nodes.pop() {
            if next_node.input_state.0 as usize == self.input.len()
                && self.speller.lexicon().is_final(next_node.lexicon_state)
            {
                let string = self
                    .speller
                    .lexicon()
                    .alphabet()
                    .string_from_symbols(&next_node.string);
                let weight = next_node.weight()
                    + self
                        .speller
                        .lexicon()
                        .final_weight(next_node.lexicon_state)
                        .unwrap();
                let entry = lookups.entry(string).or_insert(weight);
                if *entry > weight {
                    *entry = weight;
                }
            }
            self.lexicon_epsilons(&pool, Weight::INFINITE, &next_node, &mut nodes);
            self.lexicon_consume(&pool, Weight::INFINITE, &next_node, &mut nodes);
        }
        self.generate_sorted_suggestions_basic(&lookups)
    }

    fn generate_sorted_suggestions_basic(
        &self,
        lookups: &HashMap<SmolStr, Weight>,
    ) -> Vec<Suggestion> {
        // A lexicon-only traversal: the whole weight is the lexicon's, so the
        // tie-break in `Suggestion::cmp` can never separate two entries here.
        let mut c: Vec<Suggestion>;
        if let Some(s) = &self.config.completion_marker {
            c = lookups
                .into_iter()
                .map(|x| {
                    Suggestion::new(x.0.clone(), *x.1, Some(!x.0.ends_with(s)))
                        .with_lexicon_weight(*x.1)
                })
                .collect();
        } else {
            c = lookups
                .into_iter()
                .map(|x| Suggestion::new(x.0.clone(), *x.1, None).with_lexicon_weight(*x.1))
                .collect();
        }
        c.sort();

        if let Some(n) = self.config.n_best {
            c.truncate(n);
        }
        c
    }

    /// Lower bound on what a node still has to pay to become a correction.
    ///
    /// Both transducers must end in a final state for the node to be accepted,
    /// and the weight of getting there is charged to the path, so the two
    /// backward distances add. Neither accounts for the remaining input, which
    /// can only make the real cost higher — so this never overestimates, and
    /// ordering, pruning and stopping on `weight + heuristic` all stay sound.
    #[inline(always)]
    fn heuristic(&self, subsets: Option<&MutatorSubsets>, node: &TreeNode) -> Weight {
        if !self.config.astar_lookahead {
            return Weight::ZERO;
        }

        let lexicon = self.speller.lexicon().distance_to_final(node.lexicon_state);
        let mutator = match subsets {
            // The cheapest member of the subset bounds the whole of it, which
            // is what keeps this a lower bound on finishing.
            Some(subsets) => subsets.distance_to_final(node.mutator_state),
            None => self.speller.mutator().distance_to_final(node.mutator_state),
        };

        // A state that cannot reach a final state at all poisons the sum: the
        // node is a dead end, sorts last, and gets pruned by the cutoff.
        if lexicon == Weight::INFINITE || mutator == Weight::INFINITE {
            Weight::INFINITE
        } else {
            lexicon + mutator
        }
    }

    #[inline(always)]
    fn ordered<'a>(
        &self,
        subsets: Option<&MutatorSubsets>,
        node: Recycled<'a, TreeNode>,
    ) -> OrderedNode<'a> {
        let estimate = node.weight() + self.heuristic(subsets, &node);
        OrderedNode { estimate, node }
    }

    /// Search with the error model determinised on the fly, falling back to
    /// walking it as an NFA if the construction breaches a cap.
    ///
    /// The fallback is a whole second search rather than a per-node retreat: a
    /// node's `mutator_state` means a model state in one walk and a subset in
    /// the other, so the two cannot be mixed inside one queue. Nothing that
    /// ships reaches the caps, and paying twice for a transducer that does is
    /// the right trade against answering it wrongly.
    pub(crate) fn suggest(&self) -> Vec<Suggestion> {
        if self.config.mutator_subsets
            && let Some(mut subsets) = self.speller.take_subsets(self.config.astar_lookahead)
        {
            match self.search(Some(&mut subsets)) {
                Some(suggestions) => {
                    self.speller.give_subsets(subsets);
                    return suggestions;
                }
                // A construction that has breached a cap stays breached, so it
                // is dropped rather than handed back for the next word to
                // stumble over.
                None => {
                    tracing::debug!(
                        "subset construction hit a cap; redoing this word as an NFA walk"
                    );
                    if *SEARCH_STATS {
                        eprintln!("SEARCHFALLBACK\tsubsets={}", subsets.stats().subsets);
                    }
                }
            }
        }

        self.search(None)
            .expect("the NFA walk has no subset caps to breach")
    }

    fn search(&self, mut subsets: Option<&mut MutatorSubsets>) -> Option<Vec<Suggestion>> {
        tracing::trace!("Beginning suggest");

        let pool = Pool::with_size_and_max(self.config.node_pool_size, self.config.node_pool_size);
        // A*: always expand the node with the cheapest `weight + heuristic`.
        // Arc weights are non-negative and the heuristic is admissible, so the
        // first time a final configuration is reached it is via a least-weight
        // path, the n-best heap fills with good candidates early (tightening
        // the cutoff), and the whole search can stop when the cheapest open
        // estimate exceeds the cutoff.
        let mut queue: BinaryHeap<OrderedNode> = BinaryHeap::with_capacity(256);
        queue.extend(
            speller_start_node(&pool, self.state_size() as usize)
                .into_iter()
                .map(|node| self.ordered(subsets.as_deref(), node)),
        );
        let mut scratch: Vec<Recycled<TreeNode>> = Vec::with_capacity(256);
        // Key on symbol sequences to avoid string_from_symbols in the hot loop.
        // Converted to SmolStr once after the loop.
        // Total weight and the error model's share of it, keyed by output form.
        let mut corrections: HashMap<Vec<SymbolNumber>, (Weight, Weight)> = HashMap::new();
        let mut best_weight = Weight::MAX;
        let key_table = self.speller.mutator().alphabet().key_table();
        let alphabet = self.speller.lexicon().alphabet();
        let n_best = self.config.n_best.unwrap_or(usize::MAX);

        // Max-heap tracking the n-best POST-REWEIGHT weights of distinct
        // corrections. The peek (max) is the cutoff. Raw path weights may be
        // compared against it because reweight penalties are non-negative:
        // a partial path whose raw weight already exceeds the n-th best final
        // weight cannot finish above it. Keying this heap on raw weights
        // instead used to prune candidates that reweighting would have
        // promoted into the n best.
        let mut weight_heap: BinaryHeap<Weight> = BinaryHeap::with_capacity(n_best.min(64));
        let mut dl_buf: Vec<usize> = Vec::new();

        let mut iteration_count = 0usize;
        let mut stats = SEARCH_STATS.then(SearchStats::default);
        let mut closed = self.config.search_dedup.then(Closed::new);

        while let Some(OrderedNode {
            estimate,
            node: next_node,
        }) = queue.pop()
        {
            iteration_count += 1;
            if let Some(s) = stats.as_mut() {
                s.record_pop(&next_node);
            }

            // A cheaper path to this exact state was queued after this node
            // was; that one carries everything this one could contribute.
            if let Some(c) = closed.as_mut()
                && !c.is_current(&next_node)
            {
                continue;
            }

            let nth_best = if weight_heap.len() >= n_best {
                weight_heap.peek().copied()
            } else {
                None
            };
            let max_weight = self.update_weight_limit(best_weight, nth_best);

            if iteration_count >= 10_000_000 {
                let name: SmolStr = self
                    .input
                    .iter()
                    .map(|s| &*key_table[s.0 as usize])
                    .collect();
                tracing::warn!("{}: iteration count at {}", name, iteration_count);
                tracing::warn!("Node count: {}", queue.len());
                tracing::warn!("Node weight: {}", next_node.weight());
                break;
            }

            if !self.is_under_weight_limit(max_weight, estimate) {
                // No completion of the most promising open node can come in
                // under the cutoff, and the cutoff only ever tightens — so the
                // same holds for every other open node. Done.
                break;
            }

            // `scratch` is drained at the end of every iteration, so these marks
            // attribute each child to the expansion that produced it.
            self.lexicon_epsilons(&pool, max_weight, &next_node, &mut scratch);
            let lexicon_eps_mark = scratch.len();
            if !self.mutator_epsilons(
                &pool,
                max_weight,
                &next_node,
                subsets.as_deref_mut(),
                &mut scratch,
            ) {
                return None;
            }
            let mutator_eps_mark = scratch.len();
            if let Some(s) = stats.as_mut() {
                s.push_lexicon_epsilons += lexicon_eps_mark as u64;
                s.push_mutator_epsilons += (mutator_eps_mark - lexicon_eps_mark) as u64;
            }

            let at_input_end = next_node.input_state.0 as usize == self.input.len();
            if !at_input_end
                && !self.consume_input(
                    &pool,
                    max_weight,
                    &next_node,
                    subsets.as_deref_mut(),
                    &mut scratch,
                )
            {
                return None;
            }
            if let Some(s) = stats.as_mut() {
                s.push_consume_input += (scratch.len() - mutator_eps_mark) as u64;
            }
            let queue_before = queue.len();
            // Children were filtered on their weight alone; the estimate also
            // prices what they still owe, which drops dead ends outright. What
            // survives that is queued only if it reaches a state no cheaper
            // path has already reached.
            let heuristic_subsets = subsets.as_deref();
            queue.extend(
                scratch
                    .drain(..)
                    .map(|node| self.ordered(heuristic_subsets, node))
                    .filter(|queued| self.is_under_weight_limit(max_weight, queued.estimate))
                    .filter(|queued| {
                        closed
                            .as_mut()
                            .is_none_or(|closed| closed.admit(&queued.node))
                    }),
            );
            if let Some(s) = stats.as_mut() {
                s.pushes_kept += (queue.len() - queue_before) as u64;
                s.max_queue = s.max_queue.max(queue.len());
            }
            if !at_input_end {
                continue;
            }

            if !self.speller.lexicon().is_final(next_node.lexicon_state) {
                continue;
            }

            // A subset is final when any member of it is, at the cheapest
            // member's price — the same weight the NFA walk would reach by the
            // cheapest of the paths the subset stands for.
            let mutator_final = match subsets.as_deref() {
                Some(subsets) => subsets.final_weight(next_node.mutator_state),
                None => {
                    let mutator = self.speller.mutator();
                    match mutator.is_final(next_node.mutator_state) {
                        true => mutator.final_weight(next_node.mutator_state),
                        false => None,
                    }
                }
            };
            let Some(mutator_final) = mutator_final else {
                continue;
            };

            let node_weight = next_node.weight();
            let lexicon_final = self
                .speller
                .lexicon()
                .final_weight(next_node.lexicon_state)
                .expect("a final lexicon state has a final weight");
            let weight = node_weight + lexicon_final + mutator_final;
            let mutator_weight = next_node.mutator_weight + mutator_final;

            if !self.is_under_weight_limit(max_weight, weight) {
                continue;
            }

            if weight < best_weight {
                best_weight = weight;
            }

            // Dedup by symbol sequence — avoid string conversion in the hot loop.
            // On hit: just compare/update weight. On miss: clone the symbol vec.
            if let Some(entry) = corrections.get_mut(next_node.string.as_slice()) {
                if entry.0 > weight {
                    *entry = (weight, mutator_weight);
                }
                // The heap entry for this correction is left at its older,
                // higher weight: a stale-high entry only loosens the cutoff,
                // never over-tightens it. A second heap slot here would let one
                // correction occupy two of the n, over-tightening the cutoff
                // below the n-th best *distinct* correction.
            } else {
                let final_weight = match &self.reweight_ctx {
                    Some(ctx) => {
                        let value = alphabet.string_from_symbols(&next_node.string);
                        weight + ctx.additional_weight_for(&value, mutator_weight, &mut dl_buf)
                    }
                    None => weight,
                };
                corrections.insert(next_node.string.clone(), (weight, mutator_weight));
                if let Some(s) = stats.as_mut() {
                    s.corrections += 1;
                    s.first_correction_pop.get_or_insert(s.pops);
                }

                if weight_heap.len() < n_best {
                    weight_heap.push(final_weight);
                } else if let Some(&worst) = weight_heap.peek() {
                    if final_weight < worst {
                        weight_heap.pop();
                        weight_heap.push(final_weight);
                    }
                }
            }
        }

        tracing::debug!(
            heuristic = self.config.astar_lookahead,
            iterations = iteration_count,
            queued = queue.len(),
            "suggest search finished"
        );

        if let Some(s) = stats.as_mut() {
            s.subsets = subsets.as_deref().map(MutatorSubsets::stats);
            let word: SmolStr = self
                .input
                .iter()
                .map(|sym| &*key_table[sym.0 as usize])
                .collect();
            s.report(&word, queue.len());
        }

        // Convert symbol sequences to strings and build final suggestions
        let string_corrections: HashMap<SmolStr, (Weight, Weight)> = corrections
            .into_iter()
            .map(|(syms, w)| (alphabet.string_from_symbols(&syms), w))
            .collect();

        Some(self.generate_sorted_suggestions(&string_corrections))
    }

    // Analyze an output form using only the lexicon to get its weight
    fn analyze_output_form(&self, form: &str) -> Weight {
        use unic_segment::Graphemes;

        let lexicon_alphabet = self.speller.lexicon().alphabet();
        let string_to_symbol = lexicon_alphabet.string_to_symbol();

        let temp_input: Vec<SymbolNumber> = Graphemes::new(form)
            .map(|ch| {
                string_to_symbol
                    .get(ch)
                    .copied()
                    .unwrap_or_else(|| lexicon_alphabet.unknown().unwrap_or(SymbolNumber::ZERO))
            })
            .collect();

        if temp_input.is_empty() {
            return Weight(0.0);
        }

        // Manually traverse lexicon-only (like analyze() does)
        let pool = Pool::with_size_and_max(0, 0);
        let lexicon = self.speller.lexicon();
        let mut nodes = speller_start_node(&pool, self.state_size() as usize);
        let mut best_weight = Weight::MAX;

        // Create a temporary config without verbose mode to avoid infinite recursion
        let temp_config = SpellerConfig {
            verbose: false,
            ..self.config.clone()
        };

        let temp_worker = SpellerWorker::new_lexicon_input(
            self.speller.clone(),
            temp_input,
            &temp_config,
            OutputMode::WithoutTags,
        );

        while let Some(next_node) = nodes.pop() {
            if next_node.input_state.0 as usize == temp_worker.input.len()
                && lexicon.is_final(next_node.lexicon_state)
            {
                let weight =
                    next_node.weight() + lexicon.final_weight(next_node.lexicon_state).unwrap();
                if weight < best_weight {
                    best_weight = weight;
                }
            }
            temp_worker.lexicon_epsilons(&pool, Weight::INFINITE, &next_node, &mut nodes);
            temp_worker.lexicon_consume(&pool, Weight::INFINITE, &next_node, &mut nodes);
        }

        if best_weight == Weight::MAX {
            Weight(0.0)
        } else {
            best_weight
        }
    }

    /// Build suggestions, splitting each total into what the lexicon charged
    /// for the result and what the error model charged for getting there.
    ///
    /// `mutator_weight` always comes from the path taken, so it is exact. In
    /// verbose mode `lexicon_weight` is instead the best lexicon-only analysis
    /// of the output form, which is the figure cgspell's `<WA:>` is defined
    /// against (#73); the two are therefore measured differently and need not
    /// sum to the total. Otherwise it is the path's own lexicon share.
    fn generate_sorted_suggestions(
        &self,
        corrections: &HashMap<SmolStr, (Weight, Weight)>,
    ) -> Vec<Suggestion> {
        let mut c: Vec<Suggestion> = corrections
            .iter()
            .map(|(value, (weight, mutator_weight))| {
                let lexicon_weight = if self.config.verbose {
                    self.analyze_output_form(value.as_str())
                } else {
                    *weight - *mutator_weight
                };

                let completed = self
                    .config
                    .completion_marker
                    .as_ref()
                    .map(|marker| !value.ends_with(marker.as_str()));

                Suggestion::new_with_details(
                    value.clone(),
                    *weight,
                    completed,
                    WeightDetails {
                        lexicon_weight,
                        mutator_weight: *mutator_weight,
                        reweight_start: 0.0,
                        reweight_mid: 0.0,
                        reweight_end: 0.0,
                    },
                )
                // Always the path's own lexicon share, never the verbose
                // figure: the tie-break this feeds must not depend on a
                // debugging flag.
                .with_lexicon_weight(*weight - *mutator_weight)
            })
            .collect();

        c.sort();

        // No n-best truncation here: these weights are pre-reweight, and
        // cutting on them drops candidates the reweight step would promote
        // into the n best. `suggest_case` truncates after reweighting.
        c
    }
}
