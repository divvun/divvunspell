//! Backward shortest distance ("distance to a final state") over a transducer.
//!
//! The suggestion search in [`crate::speller::worker`] is a best-first walk of
//! the product of the error model and the lexicon. Ordering that walk by the
//! weight accumulated *so far* has no lookahead: the queue fills up with cheap
//! partial paths that can never finish cheaply, and the cutoff only tightens
//! once a complete correction is finally reached.
//!
//! This module precomputes, for every state of a transducer, the minimum weight
//! of any path from that state to a final state, final weight included. That
//! value is an admissible heuristic: adding it to a partial path's weight can
//! never overshoot the cheapest completion of that path, because the completion
//! is itself such a path (the search additionally has to match the remaining
//! input, which can only cost more). Ordering the queue by `g + h` therefore
//! preserves the result set while pulling promising paths forward.
//!
//! # How it is computed
//!
//! The value is a single-source shortest distance in the *reverse* transition
//! graph, sourced at every final state (seeded with its final weight). Arc
//! weights are non-negative — bar the rounding residue weight pushing leaves,
//! which is floored and compensated for — so Dijkstra applies.
//!
//! A transducer whose weights have been pushed towards the initial state, which
//! is what shipped Divvun spellers carry, can finish from anywhere for nothing:
//! every reachable state scores zero and the table has nothing to say. That
//! case is detected and the table dropped, so the search does not pay a memory
//! access per node for an answer that is always zero.
//!
//! Enumerating the graph means reading the HFST optimised-lookup tables
//! directly, since the [`crate::transducer::Transducer`] traversal API answers
//! "what can I do from here with this symbol" rather than "what states are
//! there". The layout, per the format's writer:
//!
//! * An **index-table state** occupies position `i` (its finality marker) plus
//!   one entry per outgoing input symbol `s` at `i + 1 + s`, tagged with `s` —
//!   rows of different states may interleave, and the tag is what disambiguates
//!   them. So a linear sweep of the index table recovers every (state, symbol)
//!   row exactly: the owner of a tagged entry `j` is `j - s - 1`. The entry's
//!   target points at the first arc *of that state* carrying `s` in the
//!   transition table.
//! * A **transition-table state** is addressed as `p + TARGET_TABLE`, where
//!   record `p` is the state's head (finality marker) and its arcs run from
//!   `p + 1` until the next head record (`input_symbol == None`). Such states
//!   are discovered here by following arc targets.
//!
//! Over-counting edges is harmless — an extra edge can only make the value
//! smaller, and a smaller value is still a lower bound. Missing an edge is not,
//! so the walk deliberately errs wide: it takes whole blocks for
//! transition-table states, and follows the epsilon row through the flag
//! diacritic arcs that share the head of a block.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::constants::TARGET_TABLE;
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};

/// Raw table access needed to enumerate a transducer's states and arcs.
///
/// Implemented by each backend over its own index/transition tables. All
/// indices are raw table positions, not state addresses: the `TARGET_TABLE`
/// offset has already been stripped.
pub(crate) trait BackwardTables {
    /// Number of entries in the index table.
    fn index_len(&self) -> u32;
    /// Number of records in the transition table.
    fn trans_len(&self) -> u32;

    fn index_input_symbol(&self, i: u32) -> Option<SymbolNumber>;
    fn index_target(&self, i: u32) -> Option<TransitionTableIndex>;
    fn index_final_weight(&self, i: u32) -> Option<Weight>;
    fn index_is_final(&self, i: u32) -> bool;

    fn trans_input_symbol(&self, i: u32) -> Option<SymbolNumber>;
    fn trans_target(&self, i: u32) -> Option<TransitionTableIndex>;
    fn trans_weight(&self, i: u32) -> Option<Weight>;
    fn trans_is_final(&self, i: u32) -> bool;

    /// Whether a symbol is a flag diacritic (traversed like an epsilon).
    fn is_flag_symbol(&self, symbol: SymbolNumber) -> bool;
}

/// Which records belong to the run of arcs being scanned.
#[derive(Clone, Copy)]
enum RunMode {
    /// Consecutive records carrying exactly this input symbol.
    Symbol(SymbolNumber),
    /// Consecutive epsilon or flag-diacritic records, which the format writes
    /// at the head of a state's block and which the search walks as one run.
    EpsilonAndFlags,
    /// Every record until the next head record — a transition-table state's
    /// whole block.
    WholeBlock,
}

/// Minimum weight of a path from each state to a final state.
///
/// Indexed by raw table position: index-table states by their own position,
/// transition-table states by the position of their head record.
pub(crate) struct BackwardDistance {
    index: Box<[Weight]>,
    trans: Box<[Weight]>,
}

impl std::fmt::Debug for BackwardDistance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackwardDistance")
            .field("index_states", &self.index.len())
            .field("transition_states", &self.trans.len())
            .finish()
    }
}

impl BackwardDistance {
    /// A table that answers zero for everything, at no memory cost.
    ///
    /// Zero is a valid lower bound for any state, so this is the answer when
    /// the transducer is empty, when its weights cannot be reasoned about
    /// (NaN, or negative by more than rounding residue), or when the computed
    /// table turns out to be flat and therefore not worth consulting.
    pub(crate) fn disabled() -> BackwardDistance {
        BackwardDistance {
            index: Box::new([]),
            trans: Box::new([]),
        }
    }

    /// Lower bound on the weight of any path from state `i` to a final state.
    ///
    /// [`Weight::INFINITE`] means no final state is reachable from `i`.
    /// Positions outside the tables answer [`Weight::ZERO`], which is a valid
    /// (if useless) lower bound.
    #[inline(always)]
    pub(crate) fn get(&self, i: TransitionTableIndex) -> Weight {
        let slot = if i >= TARGET_TABLE {
            self.trans.get((i - TARGET_TABLE).0 as usize)
        } else {
            self.index.get(i.0 as usize)
        };

        slot.copied().unwrap_or(Weight::ZERO)
    }

    /// Reverse-Dijkstra the whole transducer.
    ///
    /// Peak transient cost is roughly `4 * (states) + 8 * (arcs)` bytes on top
    /// of the retained `4 * (states)`; for a 3.6M-arc lexicon that is under
    /// 100MB transient and 25MB retained.
    pub(crate) fn compute<T: BackwardTables>(tables: &T) -> BackwardDistance {
        let n_index = tables.index_len() as usize;
        let n_trans = tables.trans_len() as usize;
        let n = n_index + n_trans;

        // State ids have to stay addressable as `u32`.
        if n == 0 || n > u32::MAX as usize {
            return BackwardDistance::disabled();
        }

        // Weight pushing leaves rounding residue behind: real acceptors carry a
        // scattering of arcs weighing about -1e-6. Dijkstra needs non-negative
        // arcs, so those are floored at zero and the total amount floored is
        // remembered. A minimum-weight path is simple (cycles cannot be
        // negative overall, or no minimum would exist), so it can have absorbed
        // at most that much: subtracting it at the end restores the lower bound.
        let mut slack = 0.0f32;
        let mut poisoned = false;

        // Seed: every final state, at its own final weight.
        let mut dist = vec![Weight::INFINITE; n];
        for (i, slot) in dist.iter_mut().enumerate().take(n_index) {
            if tables.index_is_final(i as u32)
                && let Some(w) = tables.index_final_weight(i as u32)
            {
                tally(w, &mut slack, &mut poisoned);
                *slot = clamp(w);
            }
        }
        for (p, slot) in dist.iter_mut().skip(n_index).enumerate() {
            if tables.trans_is_final(p as u32)
                && let Some(w) = tables.trans_weight(p as u32)
            {
                tally(w, &mut slack, &mut poisoned);
                *slot = clamp(w);
            }
        }

        // Reverse adjacency in CSR form, built in two identical walks: the
        // first counts (and tallies the flooring slack), the second fills.
        let mut offsets = vec![0u32; n + 1];
        let mut edge_count: usize = 0;
        walk_edges(tables, &mut |_src, dst, w| {
            tally(w, &mut slack, &mut poisoned);
            offsets[dst as usize + 1] += 1;
            edge_count += 1;
        });

        // Slack this large is not rounding residue, it is a transducer whose
        // weights this computation cannot be trusted on.
        if poisoned || slack > MAX_SLACK || edge_count > u32::MAX as usize {
            return BackwardDistance::disabled();
        }

        for i in 0..n {
            offsets[i + 1] += offsets[i];
        }

        // Filling advances `offsets[dst]` past each edge written, which leaves
        // `offsets[i]` holding the *end* of bucket `i`; the start is then
        // `offsets[i - 1]` (or zero for the first bucket).
        let mut edges: Vec<(u32, Weight)> = vec![(0, Weight::ZERO); edge_count];
        walk_edges(tables, &mut |src, dst, w| {
            let slot = &mut offsets[dst as usize];
            edges[*slot as usize] = (src, clamp(w));
            *slot += 1;
        });

        let mut queue: BinaryHeap<Reverse<(Weight, u32)>> = BinaryHeap::new();
        for (id, d) in dist.iter().enumerate() {
            if *d != Weight::INFINITE {
                queue.push(Reverse((*d, id as u32)));
            }
        }

        while let Some(Reverse((d, node))) = queue.pop() {
            if d > dist[node as usize] {
                continue;
            }

            let end = offsets[node as usize] as usize;
            let start = if node == 0 {
                0
            } else {
                offsets[node as usize - 1] as usize
            };

            for &(predecessor, weight) in &edges[start..end] {
                let relaxed = d + weight;
                if relaxed < dist[predecessor as usize] {
                    dist[predecessor as usize] = relaxed;
                    queue.push(Reverse((relaxed, predecessor)));
                }
            }
        }

        // Give back whatever the flooring may have added along the way.
        if slack > 0.0 {
            for d in dist.iter_mut() {
                if d.0.is_finite() {
                    d.0 -= slack;
                }
            }
        }

        // A weight-pushed transducer — which is what `hfst-push-weights` leaves
        // behind, and what real Divvun spellers ship — can finish from anywhere
        // for nothing, so every reachable state scores zero and the table
        // cannot reorder anything. Consulting it anyway costs two random reads
        // into tens of megabytes on every node expanded, so a table with
        // nothing to say is dropped instead of consulted.
        //
        // "Reachable" is read off the reverse graph: a state nothing points at
        // (other than the start) is a table position that is not a state at all.
        let mut reachable = 0usize;
        let mut informative = 0usize;
        for (id, d) in dist.iter().enumerate() {
            let bucket_start = if id == 0 { 0 } else { offsets[id - 1] };
            if id != 0 && offsets[id] == bucket_start {
                continue;
            }
            reachable += 1;
            if d.0 > FLAT_TOLERANCE {
                informative += 1;
            }
        }

        tracing::debug!(
            states = n,
            edges = edge_count,
            reachable,
            informative,
            slack,
            "backward distances computed"
        );

        if informative * INFORMATIVE_IN < reachable {
            return BackwardDistance::disabled();
        }

        let trans = dist.split_off(n_index);
        BackwardDistance {
            index: dist.into_boxed_slice(),
            trans: trans.into_boxed_slice(),
        }
    }
}

/// How much total flooring of negative weights is still credible as rounding
/// residue. Beyond this the transducer is genuinely negatively weighted and the
/// heuristic bows out.
const MAX_SLACK: f32 = 0.01;

/// Distance below which a state is treated as saying nothing.
const FLAT_TOLERANCE: f32 = 1e-3;

/// The table has to have something to say about at least one reachable state in
/// this many to be worth a memory access per node expanded.
const INFORMATIVE_IN: usize = 1000;

/// Clamp a weight into the non-negative range Dijkstra needs.
#[inline(always)]
fn clamp(w: Weight) -> Weight {
    if w.0 > 0.0 { w } else { Weight::ZERO }
}

/// Account for a weight that [`clamp`] will alter: negative weights add what
/// they lose to `slack`, and a NaN — which cannot be reasoned about at all —
/// sets `poisoned`, disabling the heuristic outright rather than letting a
/// value that might overshoot through.
#[inline(always)]
fn tally(w: Weight, slack: &mut f32, poisoned: &mut bool) {
    if w.0.is_nan() {
        *poisoned = true;
    } else if w.0 < 0.0 {
        *slack -= w.0;
    }
}

/// Enumerate every arc as `(source state id, target state id, weight)`.
///
/// State ids are dense: index-table state `i` is `i`, transition-table state at
/// head record `p` is `index_len + p`. The walk is deterministic, so calling it
/// twice yields the same edges in the same order.
fn walk_edges<T: BackwardTables>(tables: &T, emit: &mut impl FnMut(u32, u32, Weight)) {
    let n_index = tables.index_len();
    let n_trans = tables.trans_len();

    // Transition-table states are only discoverable by following arcs into
    // them, so they are queued as they turn up.
    let mut seen = vec![false; n_trans as usize];
    let mut pending: Vec<u32> = Vec::new();

    for entry in 0..n_index {
        // A tagged index entry is the row of state `entry - symbol - 1` for
        // that symbol; an untagged one is a finality marker or unused padding.
        let Some(symbol) = tables.index_input_symbol(entry) else {
            continue;
        };
        let Some(target) = tables.index_target(entry) else {
            continue;
        };
        if target < TARGET_TABLE {
            continue;
        }
        let Some(source) = entry.checked_sub(symbol.0 as u32 + 1) else {
            continue;
        };

        let mode = if symbol == SymbolNumber::ZERO {
            RunMode::EpsilonAndFlags
        } else {
            RunMode::Symbol(symbol)
        };

        scan_run(
            tables,
            (target - TARGET_TABLE).0,
            mode,
            source,
            n_index,
            n_trans,
            &mut seen,
            &mut pending,
            emit,
        );
    }

    while let Some(head) = pending.pop() {
        scan_run(
            tables,
            head.saturating_add(1),
            RunMode::WholeBlock,
            n_index + head,
            n_index,
            n_trans,
            &mut seen,
            &mut pending,
            emit,
        );
    }
}

/// Emit the arcs of one run of transition records, starting at `start`.
#[allow(clippy::too_many_arguments)]
fn scan_run<T: BackwardTables>(
    tables: &T,
    start: u32,
    mode: RunMode,
    source: u32,
    n_index: u32,
    n_trans: u32,
    seen: &mut [bool],
    pending: &mut Vec<u32>,
    emit: &mut impl FnMut(u32, u32, Weight),
) {
    let mut record = start;

    while record < n_trans {
        let Some(symbol) = tables.trans_input_symbol(record) else {
            // A head record ends the run: it is the next state's marker.
            break;
        };

        let in_run = match mode {
            RunMode::Symbol(wanted) => symbol == wanted,
            RunMode::EpsilonAndFlags => {
                symbol == SymbolNumber::ZERO || tables.is_flag_symbol(symbol)
            }
            RunMode::WholeBlock => true,
        };
        if !in_run {
            break;
        }

        if let (Some(target), Some(weight)) =
            (tables.trans_target(record), tables.trans_weight(record))
        {
            if target >= TARGET_TABLE {
                let head = (target - TARGET_TABLE).0;
                if head < n_trans {
                    emit(source, n_index + head, weight);
                    if !seen[head as usize] {
                        seen[head as usize] = true;
                        pending.push(head);
                    }
                }
            } else if target.0 < n_index {
                emit(source, target.0, weight);
            }
        }

        record += 1;
    }
}
