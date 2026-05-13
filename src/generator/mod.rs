//! Forward generation: lemma → inflected surface forms.
//!
//! `Speller::generate(lemma)` walks every accepting path through the
//! lexicon whose **input tape** starts with the supplied lemma symbols
//! and yields the corresponding *output* tape (the surface form),
//! paired with the morphological tag input symbols trailing the lemma.
//!
//! # FST orientation assumed
//!
//! This module assumes a **generator FST** in the giellaLT/HFST sense:
//! input tape = lemma + multichar morphology tags, output tape =
//! surface form. (e.g. `generator-gt-norm.hfstol` from giellaLT.) The
//! compiled inverse of an analyser; analyser FSTs (input = surface,
//! output = lemma+tags, e.g. `analyser-gt-norm.hfstol`) are the wrong
//! shape for this module — `analyze_input` already inverts those.
//!
//! Calling `generate` against a *speller acceptor* (input == output,
//! e.g. `acceptor.default.thfst` inside a `.bhfst` bundle) silently
//! yields just the citation form. There's no morphology on the output
//! tape to walk against.
//!
//! # Limitations of v0
//!
//! - **Cycles** are bounded by [`GeneratorConfig::max_depth`],
//!   [`GeneratorConfig::max_weight`], and the hard-stop
//!   [`GeneratorConfig::max_iterations`] cap. The walker is naive
//!   DFS-with-pruning, not best-first; very deep budgets can run for
//!   a long time even with the iteration cap.
//! - **Identity / unknown** symbols are not substituted; raw symbol
//!   passes through to the surface form.

use hashbrown::HashSet;
use smol_str::SmolStr;
use unic_segment::Graphemes;

use crate::transducer::Transducer;
use crate::types::{
    FlagDiacriticOperation, FlagDiacriticOperator, SymbolNumber, TransitionTableIndex, ValueNumber,
    Weight,
};

/// One generated surface form for a lemma.
#[derive(Debug, Clone)]
pub struct GenerationResult {
    /// The inflected surface form (decoded from the FST's output tape).
    pub surface: SmolStr,
    /// The morphological analysis: the multichar tag symbols trailing
    /// the lemma on the FST's input tape, joined into a single string
    /// (e.g. `"+V+Inf"`, `"+V+Action+Acc+Sg"`).
    pub analysis: SmolStr,
    /// Cumulative weight of the path producing this form.
    pub weight: Weight,
}

/// Configuration for [`Speller::generate_with_config`](crate::speller::Speller::generate_with_config).
#[derive(Debug, Clone, Copy)]
pub struct GeneratorConfig {
    /// Maximum number of transitions to follow on a single path.
    pub max_depth: usize,
    /// Maximum cumulative path weight; paths exceeding this are pruned.
    pub max_weight: Weight,
    /// Maximum number of generation results to return; further paths
    /// are not emitted once this is reached. `None` = unbounded.
    pub max_results: Option<usize>,
    /// Hard cap on total recursive walk steps. Guarantees termination.
    /// `None` = unbounded.
    pub max_iterations: Option<u64>,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self {
            max_depth: 32,
            max_weight: Weight(40.0),
            max_results: Some(512),
            max_iterations: Some(200_000),
        }
    }
}

/// Run forward generation against `lexicon` (a generator FST) and
/// return every reachable surface form whose input-tape prefix
/// equals `lemma`.
pub(crate) fn generate_from_lexicon<T: Transducer>(
    lexicon: &T,
    lemma: &str,
    config: &GeneratorConfig,
) -> Vec<GenerationResult> {
    let Some(lemma_syms) = tokenise_lemma_in(lexicon, lemma) else {
        return Vec::new();
    };

    let flag_size = lexicon.alphabet().state_size().0 as usize;
    let mut state = WalkState {
        lemma_syms: &lemma_syms,
        input_acc: Vec::with_capacity(32),
        output_acc: Vec::with_capacity(32),
        flag_state: vec![ValueNumber::ZERO; flag_size],
        results: Vec::new(),
        seen: HashSet::new(),
        on_path: HashSet::new(),
        iterations: 0,
        config: *config,
    };

    walk(
        lexicon,
        TransitionTableIndex(0),
        0,
        Weight::ZERO,
        0,
        &mut state,
    );
    state.results
}

fn walk<T: Transducer>(
    lexicon: &T,
    node_state: TransitionTableIndex,
    lemma_idx: usize,
    weight: Weight,
    depth: usize,
    ws: &mut WalkState<'_>,
) {
    ws.iterations += 1;
    if let Some(max) = ws.config.max_iterations {
        if ws.iterations > max {
            return;
        }
    }
    if depth > ws.config.max_depth {
        return;
    }
    if weight > ws.config.max_weight {
        return;
    }
    if let Some(max) = ws.config.max_results {
        if ws.results.len() >= max {
            return;
        }
    }

    // Emit if we're at a final state and the lemma is fully consumed
    // on the input tape. Multiple paths can yield identical
    // (surface, analysis) pairs; dedup at emit time.
    if lemma_idx >= ws.lemma_syms.len() && lexicon.is_final(node_state) {
        let final_w = lexicon.final_weight(node_state).unwrap_or(Weight::ZERO);
        let total = weight + final_w;
        if total <= ws.config.max_weight {
            let alphabet = lexicon.alphabet();
            // surface = output tape; analysis = input tape *after* the
            // lemma chars (i.e. the morphology tag symbols).
            let surface = alphabet.string_from_symbols(&ws.output_acc);
            let analysis_syms = &ws.input_acc[ws.lemma_syms.len()..];
            let analysis = alphabet.string_from_symbols(analysis_syms);
            let key = (surface.clone(), analysis.clone());
            if ws.seen.insert(key) {
                ws.results.push(GenerationResult {
                    surface,
                    analysis,
                    weight: total,
                });
            }
        }
    }

    // 1. Free input moves: epsilon-input arcs and flag diacritics.
    //
    //    A state's eps slot lives at `state + 1` (slot 0 is the
    //    final-state marker), so pass `state.incr()` to the check.
    if lexicon.has_epsilons_or_flags(node_state.incr()) {
        if let Some(mut pos) = lexicon.next(node_state, SymbolNumber::ZERO) {
            let operations = lexicon.alphabet().operations();
            while let Some(trans) = lexicon.take_epsilons_and_flags(pos) {
                let input_sym = lexicon
                    .transition_input_symbol(pos)
                    .unwrap_or(SymbolNumber::ZERO);
                if input_sym == SymbolNumber::ZERO {
                    try_advance(lexicon, input_sym, trans, lemma_idx, weight, depth, ws);
                } else if let Some(op) = operations.get(&input_sym) {
                    try_advance_with_flag(
                        lexicon, input_sym, op, trans, lemma_idx, weight, depth, ws,
                    );
                }
                pos = pos.incr();
            }
        }
    }

    // 2. Symbol input moves.
    //
    //    Phase A: while we're still consuming lemma symbols, only
    //    follow the *exact* next lemma symbol on the input tape
    //    (input-side walk, like the speller). This is O(1) per step.
    //
    //    Phase B: once the lemma is consumed, enumerate all remaining
    //    input arcs — these are the morphology-tag arcs that carry
    //    multichar input symbols like `+V`, `+Sg`, etc.
    //
    //    `has_transitions(state.incr(), Some(sym))` MUST be checked
    //    before `next(state, sym)` — see same caveat in input-side
    //    callers in `speller/worker.rs`.
    if lemma_idx < ws.lemma_syms.len() {
        let sym = ws.lemma_syms[lemma_idx];
        if lexicon.has_transitions(node_state.incr(), Some(sym)) {
            if let Some(mut pos) = lexicon.next(node_state, sym) {
                while let Some(trans) = lexicon.take_non_epsilons(pos, sym) {
                    try_advance(lexicon, sym, trans, lemma_idx, weight, depth, ws);
                    pos = pos.incr();
                }
            }
        }
    } else {
        let alpha_len = lexicon.alphabet().len();
        for sym_raw in 1u32..(alpha_len as u32) {
            let sym = SymbolNumber(sym_raw as u16);
            if !lexicon.has_transitions(node_state.incr(), Some(sym)) {
                continue;
            }
            let Some(mut pos) = lexicon.next(node_state, sym) else {
                continue;
            };
            while let Some(trans) = lexicon.take_non_epsilons(pos, sym) {
                try_advance(lexicon, sym, trans, lemma_idx, weight, depth, ws);
                pos = pos.incr();
            }
        }
    }
}

/// Take a single transition: push input/output symbols, advance lemma
/// index if we matched the next expected lemma symbol on the input
/// tape, recurse, then undo on backtrack.
#[inline]
fn try_advance<T: Transducer>(
    lexicon: &T,
    input_sym: SymbolNumber,
    trans: crate::transducer::symbol_transition::SymbolTransition,
    lemma_idx: usize,
    weight: Weight,
    depth: usize,
    ws: &mut WalkState<'_>,
) {
    let target = match trans.target() {
        Some(t) => t,
        None => return,
    };
    let output_sym = trans.symbol().unwrap_or(SymbolNumber::ZERO);
    let trans_w = trans.weight().unwrap_or(Weight::ZERO);

    let alphabet = lexicon.alphabet();
    let input_is_real = input_sym != SymbolNumber::ZERO && !alphabet.is_flag(input_sym);
    let output_is_real = output_sym != SymbolNumber::ZERO && !alphabet.is_flag(output_sym);

    // Lemma matching on the input tape: while there are unmatched
    // lemma symbols left, a real (non-eps, non-flag) input must equal
    // the next expected lemma symbol. Once the lemma is consumed,
    // any further real input must be a multichar tag — a single-
    // grapheme input would mean we're walking through a longer lemma
    // that has the requested lemma as a prefix.
    let new_lemma_idx = if lemma_idx < ws.lemma_syms.len() && input_is_real {
        if input_sym == ws.lemma_syms[lemma_idx] {
            lemma_idx + 1
        } else {
            return;
        }
    } else if input_is_real && lemma_idx >= ws.lemma_syms.len() {
        let key = &alphabet.key_table()[input_sym.0 as usize];
        if key.chars().count() <= 1 {
            return;
        }
        lemma_idx
    } else {
        lemma_idx
    };

    // Path-local cycle detection. Without it, flag-mediated cycles
    // and tag-cycles (compound-forming, etc.) blow up the search
    // exponentially. Different DAG-paths to the same target are still
    // explored: emission-time dedup by (surface, analysis) folds
    // duplicates afterwards.
    if !ws.on_path.insert(target) {
        return;
    }

    if input_is_real {
        ws.input_acc.push(input_sym);
    }
    if output_is_real {
        ws.output_acc.push(output_sym);
    }

    walk(
        lexicon,
        target,
        new_lemma_idx,
        weight + trans_w,
        depth + 1,
        ws,
    );

    if output_is_real {
        ws.output_acc.pop();
    }
    if input_is_real {
        ws.input_acc.pop();
    }
    ws.on_path.remove(&target);
}

/// Mutable state threaded through the recursive walk.
struct WalkState<'a> {
    lemma_syms: &'a [SymbolNumber],
    input_acc: Vec<SymbolNumber>,
    output_acc: Vec<SymbolNumber>,
    /// Flag-diacritic feature values, indexed by feature symbol number.
    /// Sized to `alphabet.state_size()`.
    flag_state: Vec<ValueNumber>,
    results: Vec<GenerationResult>,
    /// Emitted-result dedup keyed on (surface, analysis).
    seen: HashSet<(SmolStr, SmolStr)>,
    /// States currently on the root-to-here path; used to break cycles.
    on_path: HashSet<TransitionTableIndex>,
    /// Total recursive `walk` invocations so far; bounded by
    /// [`GeneratorConfig::max_iterations`].
    iterations: u64,
    config: GeneratorConfig,
}

/// Apply a flag-diacritic operation to the walker's flag state. See
/// `tree_node::TreeNode::apply_operation` in the speller for the
/// canonical implementation; this is a port that mutates in place
/// for use by the DFS walker, returning the previous value to enable
/// O(1) backtrack.
fn apply_flag(
    flag_state: &mut [ValueNumber],
    op: &FlagDiacriticOperation,
) -> Option<Option<ValueNumber>> {
    let idx = op.feature.0 as usize;
    match op.operation {
        FlagDiacriticOperator::PositiveSet => {
            let prev = flag_state[idx];
            flag_state[idx] = op.value;
            Some(Some(prev))
        }
        FlagDiacriticOperator::NegativeSet => {
            let prev = flag_state[idx];
            flag_state[idx] = op.value.invert();
            Some(Some(prev))
        }
        FlagDiacriticOperator::Require => {
            let ok = if op.value.0 == 0 {
                flag_state[idx] != ValueNumber(0)
            } else {
                flag_state[idx] == op.value
            };
            if ok { Some(None) } else { None }
        }
        FlagDiacriticOperator::Disallow => {
            let ok = if op.value.0 == 0 {
                flag_state[idx] == ValueNumber(0)
            } else {
                flag_state[idx] != op.value
            };
            if ok { Some(None) } else { None }
        }
        FlagDiacriticOperator::Clear => {
            let prev = flag_state[idx];
            flag_state[idx] = ValueNumber(0);
            Some(Some(prev))
        }
        FlagDiacriticOperator::Unification => {
            let f = flag_state[idx];
            if f.0 == 0 || f == op.value || (f.0 < 0 && f.invert() != op.value) {
                let prev = flag_state[idx];
                flag_state[idx] = op.value;
                Some(Some(prev))
            } else {
                None
            }
        }
    }
}

#[inline]
fn try_advance_with_flag<T: Transducer>(
    lexicon: &T,
    input_sym: SymbolNumber,
    op: &FlagDiacriticOperation,
    trans: crate::transducer::symbol_transition::SymbolTransition,
    lemma_idx: usize,
    weight: Weight,
    depth: usize,
    ws: &mut WalkState<'_>,
) {
    let undo = match apply_flag(&mut ws.flag_state, op) {
        Some(u) => u,
        None => return,
    };
    try_advance(lexicon, input_sym, trans, lemma_idx, weight, depth, ws);
    if let Some(prev) = undo {
        ws.flag_state[op.feature.0 as usize] = prev;
    }
}

/// Map a lemma string to a sequence of symbol numbers in the lexicon's
/// alphabet. Returns `None` if any grapheme is unknown.
fn tokenise_lemma_in<T: Transducer>(transducer: &T, lemma: &str) -> Option<Vec<SymbolNumber>> {
    let mut syms = Vec::new();
    let alphabet = transducer.alphabet();
    for grapheme in Graphemes::new(lemma) {
        let key = SmolStr::new(grapheme);
        let sym = alphabet.string_to_symbol().get(&key).copied()?;
        syms.push(sym);
    }
    Some(syms)
}
