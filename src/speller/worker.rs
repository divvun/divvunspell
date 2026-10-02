use std::collections::BinaryHeap;

use hashbrown::{HashMap, HashSet};
use smol_str::SmolStr;
use std::sync::Arc;

use super::subset::{MutatorSubsets, SubsetStats};
use super::{HfstSpeller, OutputMode, SpellerConfig};
use crate::speller::suggestion::{Suggestion, WeightDetails};
use crate::transducer::tree_node::{NodeTables, TreeNode};
use crate::transducer::{ArcGroup, SymbolSet, Transducer};
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};

#[inline(always)]
fn speller_start_node() -> Vec<TreeNode> {
    let mut nodes = Vec::with_capacity(256);
    nodes.push(TreeNode::empty());
    nodes
}

/// A node with its place in the search order.
///
/// The order is A*'s `f = g + h`: `g` is the weight accumulated so far and `h`
/// is [`Transducer::distance_to_final`] summed over the two transducers — a
/// lower bound on what finishing must still cost. Plain best-first (`h = 0`)
/// has no lookahead and drowns in shallow, cheap, hopeless paths before any
/// final state tightens the cutoff; `h` prices the rest of the word in.
struct OrderedNode {
    /// `g + h`. Never overestimates the weight of any completion of this node,
    /// which is what makes it safe to both prune and stop on.
    estimate: Weight,
    node: TreeNode,
}

/// What the queue holds for one node: its place in the order and where the
/// node itself is parked.
///
/// The heap moves its elements on every push and pop, so it holds only these
/// sixteen bytes and the nodes stay put in a [`Parked`] slab.
#[derive(Clone, Copy)]
struct QueueEntry {
    /// The order as one integer: the cheapest estimate first out of the
    /// max-heap, and among equal estimates the node that has already
    /// travelled further, which reaches a complete correction sooner and so
    /// tightens the cutoff sooner. It ranks exactly as comparing the estimate
    /// and then the weight with [`Weight`]'s own total order does, so the heap
    /// pops the nodes in exactly the same sequence.
    key: u64,
    estimate: Weight,
    slot: u32,
}

/// `f32::total_cmp`'s order as an unsigned integer.
#[inline(always)]
fn total_order_key(w: Weight) -> u32 {
    let bits = w.0.to_bits();
    if bits & 0x8000_0000 != 0 {
        !bits
    } else {
        bits | 0x8000_0000
    }
}

impl QueueEntry {
    #[inline(always)]
    fn new(estimate: Weight, weight: Weight, slot: u32) -> Self {
        QueueEntry {
            key: ((!total_order_key(estimate) as u64) << 32) | total_order_key(weight) as u64,
            estimate,
            slot,
        }
    }
}

impl PartialEq for QueueEntry {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl Eq for QueueEntry {}
impl PartialOrd for QueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for QueueEntry {
    #[inline(always)]
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.key.cmp(&other.key)
    }
}

/// The queued nodes, by slot, with the free slots kept for reuse.
struct Parked {
    slots: Vec<TreeNode>,
    free: Vec<u32>,
}

impl Parked {
    fn new() -> Self {
        Parked {
            slots: Vec::with_capacity(256),
            free: Vec::new(),
        }
    }

    /// Park a node and answer its queue entry.
    #[inline(always)]
    fn park(&mut self, ordered: OrderedNode) -> QueueEntry {
        let weight = ordered.node.weight();
        let slot = match self.free.pop() {
            Some(slot) => {
                self.slots[slot as usize] = ordered.node;
                slot
            }
            None => {
                self.slots.push(ordered.node);
                (self.slots.len() - 1) as u32
            }
        };
        QueueEntry::new(ordered.estimate, weight, slot)
    }

    /// Take a node back out of its slot.
    #[inline(always)]
    fn take(&mut self, slot: u32) -> Option<TreeNode> {
        let node = *self.slots.get(slot as usize)?;
        self.free.push(slot);
        Some(node)
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

/// Why a search stopped with nodes still queued.
///
/// Both stops hand back the corrections found so far rather than nothing. The
/// queue is ordered by `weight + heuristic`, so everything already collected is
/// cheaper than anything still open: cutting a best-first search short costs
/// the dearest candidates, never the best one. What such a cut must not be is
/// quiet — a truncated search that reports like an exhausted one is
/// indistinguishable from a model that genuinely had nothing more to offer, and
/// the truncation point moves with the shape of the transducers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchStop {
    /// `SpellerConfig::search_budget` was reached — the caller asked for a
    /// bound on the work and got it.
    Budget,
    /// [`HARD_ITERATION_CAP`] was reached: nothing configured this, and no word
    /// is expected to.
    HardCap,
}

/// Backstop against a search that would otherwise run without end.
///
/// This is not a tuning knob — it is far beyond anything a real word reaches,
/// and a search that hits it has found something pathological in the
/// transducers rather than merely a hard word. Bounding the work on purpose is
/// what `SpellerConfig::search_budget` is for.
const HARD_ITERATION_CAP: u64 = 10_000_000;

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
    /// Searches cut short by the configured budget, and by the backstop. Either
    /// being non-zero says the result set is a prefix of the answer rather than
    /// the answer.
    budget_stops: u64,
    hard_cap_stops: u64,
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

        let output = ((node.string as u64) << 32) | node.flags as u64;
        self.signatures.insert((key.0, key.1, key.2, output));
        self.signatures_no_mutator.insert((key.0, key.2, output));

        self.mutator_states.insert(node.mutator_state.0);
        self.lexicon_states.insert(node.lexicon_state.0);
    }

    fn record_stop(&mut self, stop: SearchStop) {
        match stop {
            SearchStop::Budget => self.budget_stops += 1,
            SearchStop::HardCap => self.hard_cap_stops += 1,
        }
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
             max_queue={}\tqueue_left={}\tcorrections={}\tfirst_correction_pop={}\t\
             budget_stops={}\thard_cap_stops={}{}",
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
            self.budget_stops,
            self.hard_cap_stops,
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
    /// Best weight seen per search state: the three transducer positions, the
    /// flag state and the output so far. Flag states and outputs are numbered
    /// by the search's [`NodeTables`], which give equal ones equal numbers, so
    /// the key is five words however long the word grows.
    table: HashMap<[u32; 5], Weight>,
}

impl Closed {
    fn new() -> Closed {
        Closed {
            table: HashMap::new(),
        }
    }

    #[inline(always)]
    fn key(node: &TreeNode) -> [u32; 5] {
        [
            node.input_state.0,
            node.mutator_state.0,
            node.lexicon_state.0,
            node.flags,
            node.string,
        ]
    }

    /// Whether this node is worth queueing: true unless some path already
    /// reached the same state at no greater weight.
    #[inline(always)]
    fn admit(&mut self, node: &TreeNode) -> bool {
        match self.table.entry(Self::key(node)) {
            hashbrown::hash_map::Entry::Occupied(mut entry) => {
                if *entry.get() <= node.weight() {
                    return false;
                }
                entry.insert(node.weight());
                true
            }
            hashbrown::hash_map::Entry::Vacant(entry) => {
                entry.insert(node.weight());
                true
            }
        }
    }

    /// Whether this node still carries the best known weight for its state, or
    /// has been superseded by a cheaper path queued after it.
    #[inline(always)]
    fn is_current(&mut self, node: &TreeNode) -> bool {
        self.table
            .get(&Self::key(node))
            .is_none_or(|weight| *weight >= node.weight())
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
    /// No lexicon arc weighs less than this, so no step into the lexicon costs
    /// less. See [`Transducer::least_arc_weight`].
    lexicon_least: Option<Weight>,
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
        let lexicon_least = speller.lexicon().least_arc_weight();
        SpellerWorker {
            lexicon_least,
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
        let lexicon_least = speller.lexicon().least_arc_weight();
        SpellerWorker {
            lexicon_least,
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
    fn lexicon_epsilons(
        &self,
        tables: &NodeTables,
        max_weight: Weight,
        next_node: &TreeNode,
        output_nodes: &mut Vec<TreeNode>,
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
                                .update_lexicon(tables, transition.clone_with_epsilon_symbol()),
                            OutputMode::WithTags => next_node.update_lexicon(tables, transition),
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

                        if let Some(applied_node) =
                            next_node.apply_operation(tables, op, &transition)
                        {
                            output_nodes.push(applied_node);
                        }
                    }
                }
            }

            next = next.incr();
        }
    }

    /// Hand `visit` every error-model arc leaving `state` on `input_sym`.
    ///
    /// `state` names a model state when the search walks the model as an NFA
    /// and an interned subset when it determinises the model on the fly. The
    /// two agree on everything downstream of this call — an arc is an output
    /// symbol, a successor and a weight either way — which is what lets the
    /// product walk below be written once.
    ///
    /// Walking the model as an NFA, a default arc of a compact error model
    /// arrives as one [`ArcGroup::Each`] rather than one arc per output; the
    /// subset construction expands it into the memo like any other arcs.
    ///
    /// False means the subset construction breached a cap; the caller must
    /// abandon the search and redo it as the NFA walk.
    #[inline(always)]
    fn for_each_mutator_arc(
        &self,
        subsets: Option<&mut MutatorSubsets>,
        state: TransitionTableIndex,
        input_sym: SymbolNumber,
        mut visit: impl FnMut(ArcGroup<'_>),
    ) -> bool {
        let mutator = self.speller.mutator();

        let Some(subsets) = subsets else {
            mutator.for_each_arc_group(state, input_sym, visit);
            return true;
        };

        let Some((start, len)) = subsets.transitions(mutator, state, input_sym) else {
            return false;
        };

        for index in start..start + len {
            let arc = subsets.arc(index);
            visit(ArcGroup::One {
                output: arc.symbol,
                target: arc.target,
                weight: arc.weight,
            });
        }

        true
    }

    /// Queue what the lexicon can do with a default arc of the error model:
    /// one arc for each symbol in `outputs`, all to `target` at `weight`.
    ///
    /// The set is not a list of nodes to make. Each candidate goes through
    /// [`queue_mutator_output`](Self::queue_mutator_output), which asks the
    /// lexicon whether it can continue with that symbol at this node and
    /// queues nothing when it cannot — so what reaches the queue is the part
    /// of the set the lexicon offers here, found by the same test an explicit
    /// arc's output gets. The search therefore reaches exactly the nodes it
    /// would reach over the arcs the default stands for.
    #[inline]
    fn queue_mutator_output_set(
        &self,
        tables: &NodeTables,
        max_weight: Weight,
        next_node: &TreeNode,
        outputs: SymbolSet<'_>,
        target: TransitionTableIndex,
        weight: Weight,
        input_increment: i16,
        input_lexicon_sym: Option<SymbolNumber>,
        output_nodes: &mut Vec<TreeNode>,
    ) {
        if self.no_lexicon_step_fits(max_weight, next_node, weight) {
            return;
        }
        for sym in outputs.iter() {
            self.queue_mutator_output(
                tables,
                max_weight,
                next_node,
                sym,
                target,
                weight,
                input_increment,
                input_lexicon_sym,
                output_nodes,
            );
        }
    }

    /// Queue what the lexicon can do with one error-model output symbol.
    ///
    /// Shared by the two ways the model produces one: against an epsilon input
    /// (an insertion) and against a consumed input character.
    ///
    /// `input_lexicon_sym` is the input character's lexicon symbol when there
    /// is an input character being consumed. It names what `@_IDENTITY_@`
    /// writes and what `@_UNKNOWN_@` may not.
    /// Whether no single lexicon arc taken from `node` with an error-model
    /// step of `mutator_weight` could pass the weight test
    /// [`queue_lexicon_arcs`](Self::queue_lexicon_arcs) puts every such arc
    /// to. That test sums `node + lexicon + mutator` in that order, and float
    /// addition never decreases when an operand grows, so the sum with the
    /// lexicon's least arc weight bounds every arc's from below: when it fails
    /// the test, so does every arc, and asking the lexicon changes nothing.
    #[inline(always)]
    fn no_lexicon_step_fits(
        &self,
        max_weight: Weight,
        node: &TreeNode,
        mutator_weight: Weight,
    ) -> bool {
        self.lexicon_least.is_some_and(|least| {
            !self.is_under_weight_limit(max_weight, node.weight() + least + mutator_weight)
        })
    }

    #[inline(always)]
    fn queue_mutator_output(
        &self,
        tables: &NodeTables,
        max_weight: Weight,
        next_node: &TreeNode,
        sym: SymbolNumber,
        target: TransitionTableIndex,
        weight: Weight,
        input_increment: i16,
        input_lexicon_sym: Option<SymbolNumber>,
        output_nodes: &mut Vec<TreeNode>,
    ) {
        let mutator = self.speller.mutator();
        let lexicon = self.speller.lexicon();

        // No lexicon step from here can come in under the cutoff, so there is
        // nothing to ask the lexicon.
        if self.no_lexicon_step_fits(max_weight, next_node, weight) {
            return;
        }

        let alphabet_translator = self.speller.alphabet_translator();
        let mut_alpha = mutator.alphabet();

        // `@_UNKNOWN_@` on the output tape is not a character to write: it
        // stands for some symbol outside the model's alphabet, and the lexicon
        // says which ones are available here.
        if mut_alpha.unknown() == Some(sym) {
            self.queue_unknown_output_arcs(
                tables,
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
                        tables,
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
                        tables,
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
            tables,
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
    fn mutator_epsilons(
        &self,
        tables: &NodeTables,
        max_weight: Weight,
        next_node: &TreeNode,
        subsets: Option<&mut MutatorSubsets>,
        output_nodes: &mut Vec<TreeNode>,
    ) -> bool {
        self.for_each_mutator_arc(
            subsets,
            next_node.mutator_state,
            SymbolNumber::ZERO,
            |group| match group {
                ArcGroup::One {
                    output: sym,
                    target,
                    weight,
                } => {
                    if sym == SymbolNumber::ZERO {
                        if self.is_under_weight_limit(max_weight, next_node.weight() + weight) {
                            output_nodes.push(next_node.update_mutator(target, weight));
                        }
                        return;
                    }

                    // An `@_UNKNOWN_@` output against an epsilon input inserts
                    // "some symbol outside the alphabet" — no character in
                    // particular, and none to exclude either, since no input
                    // character is being consumed here.
                    self.queue_mutator_output(
                        tables,
                        max_weight,
                        next_node,
                        sym,
                        target,
                        weight,
                        0,
                        None,
                        output_nodes,
                    );
                }
                // An insertion default: any of these symbols, inserted.
                ArcGroup::Each {
                    outputs,
                    target,
                    weight,
                } => self.queue_mutator_output_set(
                    tables,
                    max_weight,
                    next_node,
                    outputs,
                    target,
                    weight,
                    0,
                    None,
                    output_nodes,
                ),
            },
        )
    }

    #[inline(always)]
    fn queue_lexicon_arcs(
        &self,
        tables: &NodeTables,
        max_weight: Weight,
        next_node: &TreeNode,
        input_sym: SymbolNumber,
        mutator_state: TransitionTableIndex,
        mutator_weight: Weight,
        input_increment: i16,
        output_nodes: &mut Vec<TreeNode>,
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
                            tables,
                            input_sym,
                            Some(next_node.input_state.incr(input_increment as u32)),
                            mutator_state,
                            noneps_trans.target().unwrap(),
                            noneps_trans.weight().unwrap() + mutator_weight,
                            mutator_weight,
                        ),
                        OutputMode::WithTags => next_node.update(
                            tables,
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
    fn queue_unknown_output_arcs(
        &self,
        tables: &NodeTables,
        max_weight: Weight,
        next_node: &TreeNode,
        mutator_state: TransitionTableIndex,
        mutator_weight: Weight,
        input_increment: i16,
        exclude: Option<SymbolNumber>,
        output_nodes: &mut Vec<TreeNode>,
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
                tables,
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
    fn queue_mutator_arcs(
        &self,
        tables: &NodeTables,
        max_weight: Weight,
        next_node: &TreeNode,
        subsets: Option<&mut MutatorSubsets>,
        input_sym: SymbolNumber,
        output_nodes: &mut Vec<TreeNode>,
    ) -> bool {
        let input_lexicon_sym = self
            .lexicon_input
            .get(next_node.input_state.0 as usize)
            .copied();

        self.for_each_mutator_arc(
            subsets,
            next_node.mutator_state,
            input_sym,
            |group| match group {
                ArcGroup::One {
                    output: sym,
                    target,
                    weight,
                } => {
                    if sym == SymbolNumber::ZERO {
                        if self.is_under_weight_limit(max_weight, next_node.weight() + weight) {
                            output_nodes.push(next_node.update(
                                tables,
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
                        tables,
                        max_weight,
                        next_node,
                        sym,
                        target,
                        weight,
                        1,
                        input_lexicon_sym,
                        output_nodes,
                    );
                }
                // A substitution default: the input becomes any of these.
                ArcGroup::Each {
                    outputs,
                    target,
                    weight,
                } => self.queue_mutator_output_set(
                    tables,
                    max_weight,
                    next_node,
                    outputs,
                    target,
                    weight,
                    1,
                    input_lexicon_sym,
                    output_nodes,
                ),
            },
        )
    }

    #[inline(always)]
    fn consume_input(
        &self,
        tables: &NodeTables,
        max_weight: Weight,
        next_node: &TreeNode,
        mut subsets: Option<&mut MutatorSubsets>,
        output_nodes: &mut Vec<TreeNode>,
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
                tables,
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
            tables,
            max_weight,
            next_node,
            subsets,
            input_sym,
            output_nodes,
        )
    }

    #[inline(always)]
    fn lexicon_consume(
        &self,
        tables: &NodeTables,
        max_weight: Weight,
        next_node: &TreeNode,
        output_nodes: &mut Vec<TreeNode>,
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
                        tables,
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
                        tables,
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
            tables,
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
        let tables = NodeTables::new(self.state_size());
        let mut nodes = speller_start_node();
        tracing::trace!("beginning is_correct {:?}?", self.input);
        while let Some(next_node) = nodes.pop() {
            if next_node.input_state.0 as usize == self.input.len()
                && self.speller.lexicon().is_final(next_node.lexicon_state)
            {
                return true;
            }

            self.lexicon_epsilons(&tables, Weight::INFINITE, &next_node, &mut nodes);
            self.lexicon_consume(&tables, Weight::INFINITE, &next_node, &mut nodes);
        }

        false
    }

    /// The cheapest lexicon-only path that accepts the whole input.
    ///
    /// [`is_correct`](Self::is_correct) with the weight kept, and without
    /// [`analyze`](Self::analyze)'s output forms. A rejection costs the same as
    /// `is_correct`, since either way the walk has to be exhausted to know.
    pub(crate) fn accepting_weight(&self) -> Option<Weight> {
        tracing::trace!("accepting_weight");
        let tables = NodeTables::new(self.state_size());
        let mut nodes = speller_start_node();
        let mut best: Option<Weight> = None;

        while let Some(next_node) = nodes.pop() {
            if next_node.input_state.0 as usize == self.input.len()
                && self.speller.lexicon().is_final(next_node.lexicon_state)
            {
                let weight = next_node.weight()
                    + self
                        .speller
                        .lexicon()
                        .final_weight(next_node.lexicon_state)
                        .expect("a final lexicon state has a final weight");

                best = match best {
                    Some(best) if best <= weight => Some(best),
                    _ => Some(weight),
                };
            }

            self.lexicon_epsilons(&tables, Weight::INFINITE, &next_node, &mut nodes);
            self.lexicon_consume(&tables, Weight::INFINITE, &next_node, &mut nodes);
        }

        best
    }

    pub(crate) fn analyze(&self) -> Vec<Suggestion> {
        tracing::trace!("Beginning analyze");
        let tables = NodeTables::new(self.state_size());
        let mut nodes = speller_start_node();
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
                    .string_from_symbols(&tables.symbols(next_node.string));
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
            self.lexicon_epsilons(&tables, Weight::INFINITE, &next_node, &mut nodes);
            self.lexicon_consume(&tables, Weight::INFINITE, &next_node, &mut nodes);
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
    fn ordered(&self, subsets: Option<&MutatorSubsets>, node: TreeNode) -> OrderedNode {
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

        let tables = NodeTables::new(self.state_size());
        // A*: always expand the node with the cheapest `weight + heuristic`.
        // Arc weights are non-negative and the heuristic is admissible, so the
        // first time a final configuration is reached it is via a least-weight
        // path, the n-best heap fills with good candidates early (tightening
        // the cutoff), and the whole search can stop when the cheapest open
        // estimate exceeds the cutoff.
        let mut queue: BinaryHeap<QueueEntry> = BinaryHeap::with_capacity(256);
        let mut parked = Parked::new();
        queue.extend(
            speller_start_node()
                .into_iter()
                .map(|node| parked.park(self.ordered(subsets.as_deref(), node))),
        );
        let mut scratch: Vec<TreeNode> = Vec::with_capacity(256);
        // Key on symbol sequences to avoid string_from_symbols in the hot loop.
        // Converted to SmolStr once after the loop.
        // Total weight and the error model's share of it, keyed by output form.
        let mut corrections: HashMap<u32, (Weight, Weight)> = HashMap::new();
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

        let mut iteration_count = 0u64;
        let mut stats = SEARCH_STATS.then(SearchStats::default);
        let mut closed = self.config.search_dedup.then(Closed::new);
        // Every node the search takes off the queue is counted, whichever kind
        // of expansion put it there, so the budget bounds the work the search
        // does rather than one variety of it.
        let budget = self.config.search_budget.unwrap_or(u64::MAX);
        let mut stop: Option<SearchStop> = None;

        while let Some(QueueEntry { estimate, slot, .. }) = queue.pop() {
            let Some(next_node) = parked.take(slot) else {
                continue;
            };
            iteration_count += 1;
            if let Some(s) = stats.as_mut() {
                s.record_pop(&next_node);
            }

            // Checked on the pop rather than on the expansion, so a node the
            // dedup below discards still costs what it cost to reach and queue.
            if iteration_count >= budget {
                stop = Some(SearchStop::Budget);
                break;
            }
            if iteration_count >= HARD_ITERATION_CAP {
                stop = Some(SearchStop::HardCap);
                break;
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

            if !self.is_under_weight_limit(max_weight, estimate) {
                // No completion of the most promising open node can come in
                // under the cutoff, and the cutoff only ever tightens — so the
                // same holds for every other open node. Done.
                break;
            }

            // `scratch` is drained at the end of every iteration, so these marks
            // attribute each child to the expansion that produced it.
            self.lexicon_epsilons(&tables, max_weight, &next_node, &mut scratch);
            let lexicon_eps_mark = scratch.len();
            if !self.mutator_epsilons(
                &tables,
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
                    &tables,
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
                    })
                    .map(|queued| parked.park(queued)),
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
            if let Some(entry) = corrections.get_mut(&next_node.string) {
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
                        let value = alphabet.string_from_symbols(&tables.symbols(next_node.string));
                        weight + ctx.additional_weight_for(&value, mutator_weight, &mut dl_buf)
                    }
                    None => weight,
                };
                corrections.insert(next_node.string, (weight, mutator_weight));
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

        // A stop leaves the queue non-empty, so say so: the corrections below
        // are the ones the search got to, not the ones the model has.
        if let Some(stop) = stop {
            let word: SmolStr = self
                .input
                .iter()
                .map(|sym| &*key_table[sym.0 as usize])
                .collect();
            match stop {
                SearchStop::Budget => tracing::debug!(
                    word = %word,
                    pops = iteration_count,
                    queued = queue.len(),
                    corrections = corrections.len(),
                    "search budget reached; returning the corrections found so far"
                ),
                SearchStop::HardCap => tracing::warn!(
                    word = %word,
                    pops = iteration_count,
                    queued = queue.len(),
                    corrections = corrections.len(),
                    "search hit the hard iteration cap; returning the corrections found so far"
                ),
            }
            if let Some(s) = stats.as_mut() {
                s.record_stop(stop);
            }
        }

        tracing::debug!(
            heuristic = self.config.astar_lookahead,
            iterations = iteration_count,
            queued = queue.len(),
            stopped = ?stop,
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
            .map(|(string, w)| (alphabet.string_from_symbols(&tables.symbols(string)), w))
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
        let tables = NodeTables::new(self.state_size());
        let lexicon = self.speller.lexicon();
        let mut nodes = speller_start_node();
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
            temp_worker.lexicon_epsilons(&tables, Weight::INFINITE, &next_node, &mut nodes);
            temp_worker.lexicon_consume(&tables, Weight::INFINITE, &next_node, &mut nodes);
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

#[cfg(test)]
mod queue_order_tests {
    use super::*;

    /// The integer key ranks every pair of entries as comparing the estimate
    /// (reversed) and then the weight with `Weight`'s total order does.
    #[test]
    fn the_queue_key_ranks_as_the_weights_do() {
        let values = [
            f32::NEG_INFINITY,
            -3.5,
            -1.0,
            -f32::MIN_POSITIVE,
            -0.0,
            0.0,
            f32::MIN_POSITIVE,
            1e-7,
            0.5,
            1.0,
            1.0000001,
            12.0,
            1e30,
            f32::MAX,
            f32::INFINITY,
        ];
        for &e1 in &values {
            for &w1 in &values {
                for &e2 in &values {
                    for &w2 in &values {
                        let a = QueueEntry::new(Weight(e1), Weight(w1), 0);
                        let b = QueueEntry::new(Weight(e2), Weight(w2), 1);
                        let want = Weight(e2)
                            .cmp(&Weight(e1))
                            .then_with(|| Weight(w1).cmp(&Weight(w2)));
                        assert_eq!(a.cmp(&b), want, "({e1}, {w1}) against ({e2}, {w2})");
                    }
                }
            }
        }
    }
}
