//! On-the-fly weighted determinisation of the error model.
//!
//! The suggestion search walks the product of the error model and the lexicon.
//! On the lexicon side it is walking an acceptor and there is one state to be
//! in; on the error-model side there need not be. A model built as a plain
//! union of components — one for substitutions, one for transpositions, one for
//! the authored word list — offers several routes carrying the very same
//! `input:output` label sequence, so the search stands in several model states
//! at once for one and the same partial correction, and pays the whole product
//! walk once per state. Measured on the Northern Sámi model that is 6.03 states
//! per partial correction against 1.12 for the same relation determinised ahead
//! of time, and it is the entire distance between the two builds' search times.
//!
//! Determinising the model ahead of time costs nineteen times the disk. This
//! module recovers the same collapse at run time and keeps nothing but what the
//! search actually reached: the model side of a search node becomes an interned
//! *subset* of model states, each carrying the residual weight a path into the
//! subset still owes for having gone through that state, normalised so the
//! cheapest residual is zero and the difference rides on the node's own weight.
//! That is the standard weighted subset construction, done lazily and memoised,
//! so the search pays for each `(subset, input symbol)` transition once and
//! reads it back on every later node that needs it.
//!
//! Nothing a construction holds depends on the word that built it, so it
//! outlives the search: [`crate::speller::HfstSpeller`] keeps a pool of them
//! and a search borrows one. The Northern Sámi model's reachable
//! determinisation settles at around four thousand subsets, which a few hundred
//! words are enough to reach — after that every transition is a memo hit and
//! determinising costs nothing at all.
//!
//! The alphabet being determinised over is the *pair* `input:output`, not the
//! input alone: the two symbols go to two different places (the input tape and
//! the lexicon), so two model states may only be merged when they agree on
//! both. Subsets therefore fan out per output symbol, exactly as an encoded
//! determinisation would.
//!
//! The one pair that is *not* a label is `ε:ε`, which moves the model without
//! touching either tape and so is an epsilon of the product even though it is
//! an ordinary arc of the model. Those arcs are the glue a union and a
//! concatenation compile down to, which makes them precisely what holds the
//! parallel routes apart: left as labels they split a subset in two on every
//! hop and merge nothing. Every subset is therefore closed over them before it
//! is interned, and they are dropped from the epsilon-input transition. The
//! Northern Sámi model carries 2,703 of them; the determinised build of the
//! same relation carries none, and that is the whole distance between standing
//! in six model states per partial correction and standing in one.
//!
//! # Why the results are unchanged
//!
//! For one label sequence, the merged arc weight is the minimum over the model
//! paths carrying it, and the residuals carry the rest — so the weight of every
//! completed correction is the minimum over the same set of paths the NFA walk
//! enumerates one at a time. Node weights along the way are minima too, hence
//! never above the NFA's, so the weight cutoff can only prune later, never
//! earlier. The heuristic is the minimum over members of residual plus that
//! member's distance to a final state, which is still a lower bound on
//! finishing. Nothing here reads the flag-diacritic state: flags live on the
//! lexicon side, and the search has never traversed an error model's flag arcs.
//!
//! # Termination
//!
//! Nothing about a subset construction has to terminate on an arbitrary
//! transducer, so it is capped in three directions — how many states one subset
//! may hold, how many subsets may be interned, and how many relaxations one
//! epsilon closure may take. A breach abandons the subset walk for that word
//! and the search is re-run as the plain NFA walk, which costs time and no
//! correctness.

use std::hash::{BuildHasher, BuildHasherDefault, Hash, Hasher};

use hashbrown::{HashMap, HashTable};

use crate::transducer::Transducer;
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};

/// How many model states one subset may hold before the construction gives up.
///
/// A determinised model produces singletons and the Northern Sámi union of
/// components averages eleven; anything approaching this is a transducer this
/// construction has no business running on.
const MAX_SUBSET_MEMBERS: usize = 256;

/// How many subsets may be interned for one word before the construction gives
/// up. Each is a handful of words of arena, so this is a memory bound rather
/// than a plausibility one.
const MAX_SUBSETS: usize = 1 << 20;

/// How many relaxations one epsilon closure may take. A closure over
/// non-negative arcs settles in well under this; the cap is what stops a
/// negatively-weighted cycle from spinning.
const MAX_CLOSURE_STEPS: usize = 8192;

/// One model state inside a subset, and what a path into the subset still owes
/// for having arrived there through that state.
///
/// The residual is kept as raw bits so subsets hash and compare bit-exactly;
/// two subsets that differ in the last place of one residual stay distinct,
/// which merges less than it could and never merges more.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Member {
    state: u32,
    residual: u32,
}

impl Member {
    #[inline(always)]
    fn residual(&self) -> f32 {
        f32::from_bits(self.residual)
    }
}

/// One transition out of a subset: the output symbol it writes, the subset it
/// lands in, and the weight common to every model path it stands for.
#[derive(Clone, Copy)]
pub(crate) struct SubsetArc {
    pub(crate) symbol: SymbolNumber,
    pub(crate) target: TransitionTableIndex,
    pub(crate) weight: Weight,
}

/// An interned subset, keyed by the exact member sequence.
struct InternEntry {
    id: u32,
    hash: u64,
}

/// A model successor before grouping: which output symbol it was reached by,
/// which state it is, and the absolute weight of getting there.
struct Successor {
    symbol: u16,
    state: u32,
    weight: f32,
}

/// Hasher for the `(subset, symbol)` transition memo, whose keys are already
/// dense integers and need spreading rather than digesting.
#[derive(Default)]
struct IntHasher(u64);

impl Hasher for IntHasher {
    #[inline(always)]
    fn finish(&self) -> u64 {
        self.0
    }

    #[inline(always)]
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ *byte as u64).wrapping_mul(0x0100_0000_01b3);
        }
    }

    #[inline(always)]
    fn write_u64(&mut self, value: u64) {
        let mixed = value.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.0 = mixed ^ (mixed >> 32);
    }
}

/// Record `state` at `weight` in a closure under construction, answering
/// whether that improved on what was already known and so needs following up.
#[inline]
fn relax(closure: &mut Vec<(u32, f32)>, state: u32, weight: f32) -> bool {
    match closure.iter_mut().find(|(member, _)| *member == state) {
        Some((_, best)) => {
            if weight < *best {
                *best = weight;
                true
            } else {
                false
            }
        }
        None => {
            closure.push((state, weight));
            true
        }
    }
}

/// What the subset construction spent, for `DIVVUNSPELL_SEARCH_STATS`.
#[derive(Clone, Copy, Default)]
pub(crate) struct SubsetStats {
    pub(crate) subsets: usize,
    pub(crate) members: usize,
    pub(crate) lookups: u64,
    pub(crate) misses: u64,
}

/// The lazily-built determinisation of one error model.
///
/// Nothing in here is specific to the word being searched, so a construction
/// warmed up on one word answers the next one's transitions from memo — which
/// is where most of the benefit is, the reachable determinisation being a few
/// thousand subsets against ten thousand words.
pub(crate) struct MutatorSubsets {
    /// Members of every interned subset, concatenated; `spans` slices it.
    members: Vec<Member>,
    /// `(start, len)` into `members`, indexed by subset id.
    spans: Vec<(u32, u32)>,
    /// Merged final weight per subset, `None` when no member is final.
    finals: Vec<Option<Weight>>,
    /// Merged distance-to-final per subset. Empty unless the A* heuristic is
    /// switched on, since asking a transducer for it builds a table the search
    /// otherwise never wants.
    distances: Vec<Weight>,
    /// Subset ids by member sequence.
    intern: HashTable<InternEntry>,
    /// Arcs of every computed transition, concatenated; `arc_spans` slices it.
    arcs: Vec<SubsetArc>,
    /// `(start, len)` into `arcs` per `(subset, input symbol)`.
    arc_spans: HashMap<u64, (u32, u32), BuildHasherDefault<IntHasher>>,
    hasher: hashbrown::DefaultHashBuilder,
    track_distance: bool,
    max_members: usize,
    max_subsets: usize,
    /// Reused across `compute` calls so a transition costs no allocation.
    successors: Vec<Successor>,
    /// One output symbol's group of successors, as `(state, absolute weight)`.
    group: Vec<(u32, f32)>,
    /// That group once closed and normalised, ready to intern.
    normalised: Vec<Member>,
    /// The subset being closed, as `(state, absolute weight)`.
    closure: Vec<(u32, f32)>,
    /// Closure members whose weight has dropped and whose arcs therefore need
    /// re-reading.
    pending: Vec<u32>,
    stats: SubsetStats,
}

impl MutatorSubsets {
    /// Start the construction at the closure of the model's start state, which
    /// is interned as subset zero so a fresh
    /// [`crate::transducer::tree_node::TreeNode`] needs no special casing.
    ///
    /// `None` when even that first closure breaches a cap, which is a model the
    /// caller should walk as an NFA instead.
    pub(crate) fn new<T: Transducer>(mutator: &T, track_distance: bool) -> Option<MutatorSubsets> {
        MutatorSubsets::with_caps(mutator, track_distance, MAX_SUBSET_MEMBERS, MAX_SUBSETS)
    }

    fn with_caps<T: Transducer>(
        mutator: &T,
        track_distance: bool,
        max_members: usize,
        max_subsets: usize,
    ) -> Option<MutatorSubsets> {
        let mut subsets = MutatorSubsets {
            members: Vec::new(),
            spans: Vec::new(),
            finals: Vec::new(),
            distances: Vec::new(),
            intern: HashTable::new(),
            arcs: Vec::new(),
            arc_spans: HashMap::default(),
            hasher: hashbrown::DefaultHashBuilder::default(),
            track_distance,
            max_members,
            max_subsets,
            successors: Vec::new(),
            group: Vec::new(),
            normalised: Vec::new(),
            closure: Vec::new(),
            pending: Vec::new(),
            stats: SubsetStats::default(),
        };

        // A fresh search node carries weight zero, so the start closure has to
        // normalise to zero for the node to be telling the truth. It does
        // whenever the model's free moves cost nothing to make, which is every
        // error model; a model that pays you to move bows out here and is
        // walked as an NFA instead of being answered slightly wrong.
        match subsets.intern_closed(mutator, &[(0, 0.0)]) {
            Some((_, 0.0)) => Some(subsets),
            _ => None,
        }
    }

    /// Whether this construction was built to answer distance-to-final, which
    /// decides whether a warm one can be handed to a search that wants it.
    pub(crate) fn tracks_distance(&self) -> bool {
        self.track_distance
    }

    /// How much of the determinisation has been built, for deciding whether a
    /// warm construction is still worth keeping around.
    pub(crate) fn len(&self) -> usize {
        self.spans.len()
    }

    pub(crate) fn stats(&self) -> SubsetStats {
        SubsetStats {
            subsets: self.spans.len(),
            members: self.members.len(),
            ..self.stats
        }
    }

    /// The weight of finishing in this subset, or `None` when no member of it
    /// is a final model state.
    #[inline(always)]
    pub(crate) fn final_weight(&self, subset: TransitionTableIndex) -> Option<Weight> {
        self.finals[subset.0 as usize]
    }

    /// Lower bound on what the model still has to pay from this subset.
    #[inline(always)]
    pub(crate) fn distance_to_final(&self, subset: TransitionTableIndex) -> Weight {
        self.distances
            .get(subset.0 as usize)
            .copied()
            .unwrap_or(Weight::ZERO)
    }

    /// One arc of a span returned by [`MutatorSubsets::transitions`].
    #[inline(always)]
    pub(crate) fn arc(&self, index: u32) -> SubsetArc {
        self.arcs[index as usize]
    }

    /// The transitions leaving `subset` on `symbol`, as a `(start, len)` span
    /// into the arc arena, computed on first ask and read back thereafter.
    ///
    /// `None` means a cap was breached and the caller must abandon the subset
    /// walk.
    #[inline]
    pub(crate) fn transitions<T: Transducer>(
        &mut self,
        mutator: &T,
        subset: TransitionTableIndex,
        symbol: SymbolNumber,
    ) -> Option<(u32, u32)> {
        let key = ((subset.0 as u64) << 16) | symbol.0 as u64;
        self.stats.lookups += 1;

        if let Some(span) = self.arc_spans.get(&key) {
            return Some(*span);
        }

        self.stats.misses += 1;
        let span = self.compute(mutator, subset, symbol)?;
        self.arc_spans.insert(key, span);
        Some(span)
    }

    /// Take one step of the subset construction: read every member's arcs for
    /// `symbol`, group them by the output symbol they write, and intern one
    /// successor subset per group.
    fn compute<T: Transducer>(
        &mut self,
        mutator: &T,
        subset: TransitionTableIndex,
        symbol: SymbolNumber,
    ) -> Option<(u32, u32)> {
        let mut successors = std::mem::take(&mut self.successors);
        successors.clear();

        let (start, len) = self.spans[subset.0 as usize];
        for slot in start..start + len {
            let member = self.members[slot as usize];
            let state = TransitionTableIndex(member.state);

            if !mutator.has_transitions(state.incr(), Some(symbol)) {
                continue;
            }
            let Some(mut next) = mutator.next(state, symbol) else {
                continue;
            };

            let residual = member.residual();
            loop {
                let transition = if symbol == SymbolNumber::ZERO {
                    mutator.take_epsilons(next)
                } else {
                    mutator.take_non_epsilons(next, symbol)
                };
                let Some(transition) = transition else {
                    break;
                };

                if let (Some(output), Some(target), Some(weight)) = (
                    transition.symbol(),
                    transition.target(),
                    transition.weight(),
                ) {
                    // `ε:ε` moves the model without touching either tape, so it
                    // is not a transition of the product at all — every subset
                    // is closed over those arcs instead, which is what merges
                    // the routes a union is glued together from.
                    if symbol != SymbolNumber::ZERO || output != SymbolNumber::ZERO {
                        successors.push(Successor {
                            symbol: output.0,
                            state: target.0,
                            weight: residual + weight.0,
                        });
                    }
                }

                next = next.incr();
            }
        }

        // Grouping is by output symbol: sorting on it puts each group in one
        // run. States inside a group need no ordering here, the closure sorts.
        successors.sort_unstable_by_key(|s| s.symbol);

        let mut group = std::mem::take(&mut self.group);
        let arc_start = self.arcs.len() as u32;
        let mut arc_len = 0u32;
        let mut overflowed = false;
        let mut cursor = 0usize;

        while cursor < successors.len() {
            let output = successors[cursor].symbol;
            group.clear();

            while cursor < successors.len() && successors[cursor].symbol == output {
                group.push((successors[cursor].state, successors[cursor].weight));
                cursor += 1;
            }

            match self.intern_closed(mutator, &group) {
                Some((target, weight)) => {
                    self.arcs.push(SubsetArc {
                        symbol: SymbolNumber(output),
                        target: TransitionTableIndex(target),
                        weight: Weight(weight),
                    });
                    arc_len += 1;
                }
                // Every successor of an unreachable group weighs more than any
                // cutoff can admit, so the NFA walk drops it too.
                None if !group.iter().any(|(_, weight)| weight.is_finite()) => {}
                None => {
                    overflowed = true;
                    break;
                }
            }
        }

        self.group = group;
        self.successors = successors;

        if overflowed {
            self.arcs.truncate(arc_start as usize);
            return None;
        }

        Some((arc_start, arc_len))
    }

    /// Close `seeds` over the model's `ε:ε` arcs, normalise the result so its
    /// cheapest member owes nothing, and give it a subset id.
    ///
    /// Answers the id together with the weight taken out in normalising, which
    /// is what the arc into this subset charges. `None` on a cap breach.
    fn intern_closed<T: Transducer>(
        &mut self,
        mutator: &T,
        seeds: &[(u32, f32)],
    ) -> Option<(u32, f32)> {
        let mut closure = std::mem::take(&mut self.closure);
        let mut pending = std::mem::take(&mut self.pending);
        let closed = self.close(mutator, seeds, &mut closure, &mut pending);
        self.pending = pending;

        let interned = closed.then(|| {
            // Non-negative arcs would leave the cheapest seed cheapest, but a
            // weight-pushed model carries rounding residue, so take the minimum
            // over what the closure actually produced.
            let base = closure
                .iter()
                .map(|(_, weight)| *weight)
                .fold(f32::INFINITY, f32::min);

            if !base.is_finite() {
                return None;
            }

            closure.sort_unstable_by_key(|(state, _)| *state);
            let mut normalised = std::mem::take(&mut self.normalised);
            normalised.clear();
            normalised.extend(closure.iter().map(|(state, weight)| Member {
                state: *state,
                residual: (weight - base).to_bits(),
            }));

            let id = self.intern(mutator, &normalised);
            self.normalised = normalised;
            id.map(|id| (id, base))
        });

        self.closure = closure;
        interned.flatten()
    }

    /// Relax `seeds` over the model's `ε:ε` arcs until nothing improves.
    ///
    /// False means a cap was breached. Arc weights are non-negative bar
    /// rounding residue, so this settles quickly; the step cap is what bounds
    /// it on a transducer where they are not.
    fn close<T: Transducer>(
        &self,
        mutator: &T,
        seeds: &[(u32, f32)],
        closure: &mut Vec<(u32, f32)>,
        pending: &mut Vec<u32>,
    ) -> bool {
        closure.clear();
        pending.clear();

        for &(state, weight) in seeds {
            if relax(closure, state, weight) {
                pending.push(state);
            }
        }

        let mut steps = 0usize;
        while let Some(state) = pending.pop() {
            steps += 1;
            if steps > MAX_CLOSURE_STEPS || closure.len() > self.max_members {
                return false;
            }

            let Some(weight) = closure
                .iter()
                .find(|(member, _)| *member == state)
                .map(|(_, weight)| *weight)
            else {
                continue;
            };

            let state = TransitionTableIndex(state);
            if !mutator.has_transitions(state.incr(), Some(SymbolNumber::ZERO)) {
                continue;
            }
            let Some(mut next) = mutator.next(state, SymbolNumber::ZERO) else {
                continue;
            };

            while let Some(transition) = mutator.take_epsilons(next) {
                if let (Some(SymbolNumber::ZERO), Some(target), Some(arc)) = (
                    transition.symbol(),
                    transition.target(),
                    transition.weight(),
                ) && relax(closure, target.0, weight + arc.0)
                {
                    pending.push(target.0);
                }

                next = next.incr();
            }
        }

        true
    }

    /// Give `members` its subset id, allocating one if this exact subset has
    /// not been seen. `None` on a cap breach.
    fn intern<T: Transducer>(&mut self, mutator: &T, members: &[Member]) -> Option<u32> {
        if members.is_empty() || members.len() > self.max_members {
            return None;
        }

        let hash = self.hasher.hash_one(members);

        // Split the borrow so the member comparison can read the arena while
        // the table is held, keeping this to a single lookup.
        let MutatorSubsets {
            intern,
            spans,
            members: arena,
            ..
        } = self;
        if let Some(entry) = intern.find(hash, |entry| {
            entry.hash == hash && {
                let (start, len) = spans[entry.id as usize];
                arena[start as usize..][..len as usize] == *members
            }
        }) {
            return Some(entry.id);
        }

        if self.spans.len() >= self.max_subsets {
            return None;
        }

        let id = self.spans.len() as u32;
        let start = self.members.len() as u32;
        self.members.extend_from_slice(members);
        self.spans.push((start, members.len() as u32));

        let mut final_weight: Option<Weight> = None;
        for member in members {
            let state = TransitionTableIndex(member.state);
            if mutator.is_final(state)
                && let Some(weight) = mutator.final_weight(state)
            {
                let total = Weight(member.residual() + weight.0);
                final_weight = Some(match final_weight {
                    Some(best) if best <= total => best,
                    _ => total,
                });
            }
        }
        self.finals.push(final_weight);

        if self.track_distance {
            let mut best = Weight::INFINITE;
            for member in members {
                let distance = mutator.distance_to_final(TransitionTableIndex(member.state));
                if distance != Weight::INFINITE {
                    let total = Weight(member.residual() + distance.0);
                    if total < best {
                        best = total;
                    }
                }
            }
            self.distances.push(best);
        }

        self.intern
            .insert_unique(hash, InternEntry { id, hash }, |entry| entry.hash);

        Some(id)
    }

    #[cfg(test)]
    fn size(&self, subset: TransitionTableIndex) -> usize {
        self.spans[subset.0 as usize].1 as usize
    }
}

impl std::fmt::Debug for MutatorSubsets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MutatorSubsets")
            .field("subsets", &self.spans.len())
            .field("members", &self.members.len())
            .field("arcs", &self.arcs.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transducer::TransducerLoader;
    use crate::transducer::thfst::MmapThfstTransducer;
    use crate::vfs::Fs;

    fn mutator(name: &str) -> MmapThfstTransducer {
        let path =
            std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures")).join(name);
        MmapThfstTransducer::from_path(&Fs, path).expect("the fixture is checked in")
    }

    fn subsets(name: &str) -> (MmapThfstTransducer, MutatorSubsets) {
        let mutator = mutator(name);
        let subsets = MutatorSubsets::new(&mutator, false)
            .expect("a fixture model cannot breach a cap on its start closure");
        (mutator, subsets)
    }

    /// The compact wildcard model is a union of two components, each behind its
    /// own `ε:ε` hop — the shape that has a search standing in several model
    /// states for one partial correction. Closing the start subset over those
    /// hops is what merges them, and if it did not, the parity tests would be
    /// comparing the NFA walk against itself.
    #[test]
    fn compact_layout_merges_its_components() {
        let (_mutator, subsets) = subsets("wildcard-compact-mutator.thfst");

        assert_eq!(
            subsets.size(TransitionTableIndex(0)),
            3,
            "the start state, the identity branch and the unknown branch are \
             one subset once the epsilon hops are closed over"
        );
    }

    /// An `ε:ε` arc is not a transition of the product, so nothing is left on
    /// the epsilon-input transition of a subset already closed over them.
    #[test]
    fn closed_epsilon_arcs_are_not_transitions() {
        let (mutator, mut subsets) = subsets("wildcard-compact-mutator.thfst");

        let (_, len) = subsets
            .transitions(&mutator, TransitionTableIndex(0), SymbolNumber::ZERO)
            .expect("a fixture model cannot breach a cap");

        assert_eq!(len, 0, "both epsilon hops were absorbed by the closure");
    }

    /// The same relation determinised before it shipped has one route per
    /// label and no `ε:ε` glue left, so every subset it produces is a singleton
    /// and the machinery costs a memo lookup and nothing else.
    #[test]
    fn expanded_layout_stays_singleton() {
        let (mutator, mut subsets) = subsets("wildcard-expanded-mutator.thfst");
        assert_eq!(subsets.size(TransitionTableIndex(0)), 1);

        for symbol in [1u16, 2, 3, 4, 5, 6] {
            let (start, len) = subsets
                .transitions(&mutator, TransitionTableIndex(0), SymbolNumber(symbol))
                .expect("no cap can be breached by a determinised model");
            for index in start..start + len {
                let arc = subsets.arc(index);
                assert_eq!(
                    subsets.size(arc.target),
                    1,
                    "symbol {symbol} produced a subset of more than one state"
                );
            }
        }
    }

    /// A transition is computed once and read back thereafter, which is what
    /// makes the construction cheaper than the walk it replaces.
    #[test]
    fn transitions_are_memoised() {
        let (mutator, mut subsets) = subsets("wildcard-compact-mutator.thfst");

        let first = subsets.transitions(&mutator, TransitionTableIndex(0), SymbolNumber(5));
        let after_first = subsets.stats().misses;
        let second = subsets.transitions(&mutator, TransitionTableIndex(0), SymbolNumber(5));

        assert!(first.is_some());
        assert_eq!(first, second);
        assert_eq!(subsets.stats().misses, after_first);
        assert_eq!(subsets.stats().lookups, 2);
    }

    /// A cap breach is reported rather than absorbed, so the search retreats to
    /// the NFA walk instead of answering from a truncated construction — which
    /// the parity tests then hold to the same answers.
    #[test]
    fn breaching_a_cap_is_reported() {
        let mutator = mutator("wildcard-compact-mutator.thfst");

        assert!(
            MutatorSubsets::with_caps(&mutator, false, 1, MAX_SUBSETS).is_none(),
            "a three-state start closure cannot fit a one-state cap"
        );
        assert!(
            MutatorSubsets::with_caps(&mutator, false, MAX_SUBSET_MEMBERS, 0).is_none(),
            "no subset at all fits a cap of none"
        );
        assert!(
            MutatorSubsets::with_caps(&mutator, false, MAX_SUBSET_MEMBERS, MAX_SUBSETS).is_some(),
            "the real caps admit a real model"
        );
    }
}
