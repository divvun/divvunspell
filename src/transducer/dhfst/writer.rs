//! Writing DHFST files from an existing error model.
//!
//! The writer finds, for every state, at most one default arc per kind and a
//! fallback state whose row it mostly repeats, then stores only what is left.
//! It is a post-processing step: the automaton, its states and its weights are
//! the ones it was given, bit for bit.
//!
//! Nothing is written on trust. Every `(state, pair)` of the encoding is
//! resolved and compared with the source before serialising, and the bytes are
//! then read back through [`DhfstTransducer`] and every `(state, input)` it
//! answers is compared with the source again. Any difference is an error.
//!
//! The search for defaults and fallbacks is heuristic (the largest
//! single-valued group per kind; fallback candidates from MinHash buckets over
//! the rows plus each state's most frequent targets; a greedy assignment that
//! rejects cycles and chains deeper than the bound). A better optimiser can
//! only write a smaller file; it cannot change what the file means.

use std::collections::HashMap;

use crate::transducer::dhfst::{
    DEFAULT_BASE, DefaultKind, DhfstTransducer, FLAG_DEFAULTS, FLAG_FALLBACK, FLAG_TROPICAL,
    HEADER_LEN, MAGIC, NONE, SECTION_ENTRY_LEN, VERSION, is_regular_name, tag,
};
use crate::transducer::{Transducer, TransducerError};
use crate::types::{SymbolNumber, TransitionTableIndex};

/// Why a model could not be written.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WriteError {
    /// The source transducer has something the format cannot hold, or the
    /// search has never read.
    #[error("the source error model cannot be written as DHFST: {0}")]
    Unsupported(String),
    /// The encoding or the written bytes do not answer what the source does.
    /// This is a writer bug; nothing was written.
    #[error("DHFST self-check failed: {0}")]
    Verification(String),
    /// Reading back the written bytes failed.
    #[error("the written DHFST file does not load")]
    ReadBack(#[source] TransducerError),
}

/// One arc of the source model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceArc {
    /// input symbol
    pub input: u16,
    /// output symbol
    pub output: u16,
    /// target state
    pub target: u32,
    /// weight
    pub weight: f32,
}

/// One state of the source model.
#[derive(Clone, Debug, Default)]
pub struct SourceState {
    /// final weight, if final
    pub final_weight: Option<f32>,
    /// arcs, in any order
    pub arcs: Vec<SourceArc>,
}

/// An error model as plain states and arcs, state 0 being the start, each
/// state's arcs sorted by `(input, output, target, weight)` with exact
/// duplicates dropped.
#[derive(Clone, Debug)]
pub struct SourceModel {
    symbols: Vec<String>,
    states: Vec<SourceState>,
    duplicate_arcs: u64,
}

impl SourceModel {
    /// A model from plain states, state 0 being the start. Arcs are sorted
    /// and exact duplicates dropped; targets and symbols are checked.
    pub fn new(
        symbols: Vec<String>,
        mut states: Vec<SourceState>,
    ) -> Result<SourceModel, WriteError> {
        let n_symbols = symbols.len();
        if n_symbols == 0 || n_symbols > DEFAULT_BASE as usize {
            return Err(WriteError::Unsupported(format!(
                "{n_symbols} symbols is out of range"
            )));
        }
        if symbols[0] != "@_EPSILON_SYMBOL_@" {
            return Err(WriteError::Unsupported(
                "symbol 0 must be @_EPSILON_SYMBOL_@".into(),
            ));
        }
        if states.is_empty() {
            return Err(WriteError::Unsupported("the model has no states".into()));
        }
        let n_states = states.len();
        let mut duplicate_arcs = 0u64;
        for (q, state) in states.iter_mut().enumerate() {
            for arc in &state.arcs {
                if arc.weight.is_nan() {
                    return Err(WriteError::Unsupported(format!(
                        "state {q} has a NaN weight"
                    )));
                }
                if arc.input as usize >= n_symbols || arc.output as usize >= n_symbols {
                    return Err(WriteError::Unsupported(format!(
                        "state {q} has an arc {}:{} outside the alphabet",
                        arc.input, arc.output
                    )));
                }
                if arc.target as usize >= n_states {
                    return Err(WriteError::Unsupported(format!(
                        "state {q} has an arc to missing state {}",
                        arc.target
                    )));
                }
            }
            if state
                .final_weight
                .is_some_and(|w| w.is_nan() || w == f32::NEG_INFINITY)
            {
                return Err(WriteError::Unsupported(format!(
                    "state {q} has an invalid final weight"
                )));
            }
            let before = state.arcs.len();
            state
                .arcs
                .sort_by_key(|a| (a.input, a.output, a.target, a.weight.to_bits()));
            state
                .arcs
                .dedup_by_key(|a| (a.input, a.output, a.target, a.weight.to_bits()));
            duplicate_arcs += (before - state.arcs.len()) as u64;
        }
        Ok(SourceModel {
            symbols,
            states,
            duplicate_arcs,
        })
    }

    /// Read every state reachable from the start of `transducer`, through
    /// [`Transducer::for_each_arc`] — that is, exactly the arcs the suggestion
    /// search walks, no more and no fewer.
    ///
    /// `symbols` are the transducer's symbol names as stored in its file.
    /// Refuses a model with flag diacritic arcs: the search has never
    /// traversed them, and dropping them silently would be a lie about what
    /// was converted.
    pub fn from_transducer<T: Transducer>(
        transducer: &T,
        symbols: Vec<String>,
    ) -> Result<SourceModel, WriteError> {
        let n_symbols = symbols.len();
        if n_symbols == 0 || n_symbols > DEFAULT_BASE as usize {
            return Err(WriteError::Unsupported(format!(
                "{n_symbols} symbols is out of range"
            )));
        }
        let alphabet = transducer.alphabet();

        let mut ids: HashMap<u32, u32> = HashMap::new();
        let mut order: Vec<u32> = vec![0];
        ids.insert(0, 0);
        let mut states: Vec<SourceState> = Vec::new();
        let mut cursor = 0usize;

        while cursor < order.len() {
            let address = TransitionTableIndex(order[cursor]);
            cursor += 1;

            // Flag diacritic arcs sit in the epsilon run of optimized lookup
            // and are skipped by the search; refuse rather than drop them.
            if transducer.has_epsilons_or_flags(address.incr())
                && let Some(mut next) = transducer.next(address, SymbolNumber::ZERO)
            {
                while transducer.take_epsilons_and_flags(next).is_some() {
                    if let Some(symbol) = transducer.transition_input_symbol(next)
                        && alphabet.is_flag(symbol)
                    {
                        return Err(WriteError::Unsupported(format!(
                            "state at {} has a flag diacritic arc ({}); the suggestion search never follows those",
                            address.0,
                            symbols
                                .get(symbol.0 as usize)
                                .map(String::as_str)
                                .unwrap_or("?")
                        )));
                    }
                    next = next.incr();
                }
            }

            let mut arcs: Vec<SourceArc> = Vec::new();
            for input in 0..n_symbols as u16 {
                transducer.for_each_arc(address, SymbolNumber(input), |output, target, weight| {
                    let next_id = order.len() as u32;
                    let id = *ids.entry(target.0).or_insert_with(|| {
                        order.push(target.0);
                        next_id
                    });
                    arcs.push(SourceArc {
                        input,
                        output: output.0,
                        target: id,
                        weight: weight.0,
                    });
                });
            }

            let final_weight = if transducer.is_final(address) {
                transducer.final_weight(address).map(|w| w.0)
            } else {
                None
            };
            states.push(SourceState { final_weight, arcs });
        }

        SourceModel::new(symbols, states)
    }

    /// Total arcs.
    pub fn arc_count(&self) -> u64 {
        self.states.iter().map(|s| s.arcs.len() as u64).sum()
    }

    /// Symbol names, `@_EPSILON_SYMBOL_@` first.
    pub fn symbols(&self) -> &[String] {
        &self.symbols
    }

    /// States; arc targets index this.
    pub fn states(&self) -> &[SourceState] {
        &self.states
    }

    /// Arcs dropped because an identical arc (same state, pair, target and
    /// weight) was already there.
    pub fn duplicate_arcs(&self) -> u64 {
        self.duplicate_arcs
    }
}

/// How to write.
#[derive(Clone, Debug)]
pub struct WriteOptions {
    /// Longest fallback chain to allow; `None` for no bound.
    pub max_fallback_depth: Option<u32>,
    /// Worker threads for the search and the checks; `0` for all cores.
    pub threads: usize,
    /// A name for the source, recorded in the `meta` section.
    pub source_name: String,
}

impl Default for WriteOptions {
    fn default() -> Self {
        WriteOptions {
            max_fallback_depth: Some(4),
            threads: 0,
            source_name: String::new(),
        }
    }
}

/// What the encoding came to.
#[derive(Clone, Debug, Default)]
pub struct WriteReport {
    /// states
    pub states: usize,
    /// arcs in the source
    pub source_arcs: u64,
    /// explicit arcs written
    pub explicit_arcs: u64,
    /// blockers written
    pub blockers: u64,
    /// default records written, per kind (identity, substitution, deletion,
    /// insertion)
    pub defaults: [u64; 4],
    /// distinct classes
    pub classes: usize,
    /// distinct class pairs
    pub class_pairs: usize,
    /// states with a fallback
    pub states_with_fallback: usize,
    /// states per fallback chain length
    pub depth_histogram: Vec<usize>,
    /// longest fallback chain written
    pub max_depth: u32,
    /// source arcs by what answers them: own explicit, own default (identity,
    /// substitution, deletion, insertion), inherited explicit, inherited
    /// default
    pub answered_by: [u64; 7],
    /// `(state, pair)` resolutions checked on the encoding
    pub pairs_checked: u64,
    /// `(state, input)` queries checked on the written bytes
    pub queries_checked: u64,
    /// arcs compared on the written bytes
    pub arcs_checked: u64,
}

/// A written file and what it holds.
pub struct Written {
    /// the file
    pub bytes: Vec<u8>,
    /// what the encoding came to
    pub report: WriteReport,
}

/// Encode `model`, check the encoding against it, serialise, read the bytes
/// back and check them against it again.
pub fn write(model: &SourceModel, options: &WriteOptions) -> Result<Written, WriteError> {
    let threads = match options.threads {
        0 => std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4),
        n => n,
    };
    let max_depth = options
        .max_fallback_depth
        .map(|d| d as usize)
        .unwrap_or(usize::MAX);

    let m = Model::build(model);
    let groups: Vec<[Option<Group>; 4]> = (0..m.n_states).map(|q| groups_of(&m, q)).collect();
    let cands = candidates(&m, model, 16, 6, threads);
    let scored = score_candidates(&m, &groups, &cands, threads);
    let enc = assign(&m, &groups, &scored, max_depth);
    let (answered_by, pairs_checked) = verify_encoding(&m, &groups, &enc, threads)?;
    let (bytes, mut report) = serialise(model, &m, &groups, &enc, options);
    report.answered_by = answered_by;
    report.pairs_checked = pairs_checked;

    let reader =
        DhfstTransducer::from_bytes(&bytes, "(written DHFST)").map_err(WriteError::ReadBack)?;
    let (queries, arcs) = verify_reader(model, &reader, threads)?;
    report.queries_checked = queries;
    report.arcs_checked = arcs;

    Ok(Written { bytes, report })
}

/// Check that `reader` answers every `(state, input)` of `model` with exactly
/// the model's arcs, each once. Answers `(queries, arcs)` compared.
pub fn verify_reader(
    model: &SourceModel,
    reader: &DhfstTransducer,
    threads: usize,
) -> Result<(u64, u64), WriteError> {
    let n_states = model.states.len();
    let n_symbols = model.symbols.len();
    if reader.state_count() as usize != n_states {
        return Err(WriteError::Verification(format!(
            "reader has {} states, source has {n_states}",
            reader.state_count()
        )));
    }
    if reader.symbol_names() != model.symbols.as_slice() {
        return Err(WriteError::Verification(
            "reader's symbol table differs from the source's".into(),
        ));
    }
    let threads = threads.max(1);
    let chunk = n_states.div_ceil(threads);
    let results: Vec<Result<(u64, u64), String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                scope.spawn(move || {
                    let mut got: Vec<(u16, u32, u32)> = Vec::new();
                    let mut queries = 0u64;
                    let mut arcs = 0u64;
                    for q in t * chunk..((t + 1) * chunk).min(n_states) {
                        let state = &model.states[q];
                        let want_final = state.final_weight.map(f32::to_bits);
                        let got_final = reader
                            .final_weight(TransitionTableIndex(q as u32))
                            .map(|w| w.0.to_bits());
                        if want_final != got_final {
                            return Err(format!("state {q}: final weight differs"));
                        }
                        let mut at = 0usize;
                        for x in 0..n_symbols as u16 {
                            let start = at;
                            while at < state.arcs.len() && state.arcs[at].input == x {
                                at += 1;
                            }
                            let want = &state.arcs[start..at];
                            got.clear();
                            reader.for_each_arc(
                                TransitionTableIndex(q as u32),
                                SymbolNumber(x),
                                |o, t, w| got.push((o.0, t.0, w.0.to_bits())),
                            );
                            got.sort_unstable();
                            queries += 1;
                            arcs += got.len() as u64;
                            let same = got.len() == want.len()
                                && got
                                    .iter()
                                    .zip(want)
                                    .all(|(g, w)| *g == (w.output, w.target, w.weight.to_bits()));
                            if !same {
                                return Err(format!(
                                    "state {q} input {x}: reader answers {} arcs, source has {}",
                                    got.len(),
                                    want.len()
                                ));
                            }
                        }
                    }
                    Ok((queries, arcs))
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err("a checker thread panicked".to_string()))
            })
            .collect()
    });
    let mut total = (0u64, 0u64);
    for r in results {
        let (q, a) = r.map_err(WriteError::Verification)?;
        total.0 += q;
        total.1 += a;
    }
    Ok(total)
}

/// Pair kinds, with "special" for pairs only explicit entries may answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Default(DefaultKind),
    Special,
}

fn kind_index(k: DefaultKind) -> usize {
    k as usize
}

/// The value id of a pair with no arcs.
const EMPTY: u32 = 0;

/// Dense per-state rows over every `(input, output)` pair. Each cell is a
/// value id naming the sorted set of `(target, weight)` the pair leads to.
struct Model {
    n: usize,
    np: usize,
    dense: Vec<u32>,
    values: Vec<Vec<u64>>,
    pair_kind: Vec<Kind>,
    n_states: usize,
}

#[inline(always)]
fn target_weight(target: u32, weight: f32) -> u64 {
    ((target as u64) << 32) | weight.to_bits() as u64
}

impl Model {
    fn build(model: &SourceModel) -> Model {
        let n = model.symbols.len();
        let np = n * n;
        let regular: Vec<bool> = model
            .symbols
            .iter()
            .enumerate()
            .map(|(s, name)| is_regular_name(s, name))
            .collect();
        let mut pair_kind = vec![Kind::Special; np];
        for i in 0..n {
            for o in 0..n {
                if let Some(k) =
                    crate::transducer::dhfst::pair_kind(i as u16, o as u16, regular[i], regular[o])
                {
                    pair_kind[i * n + o] = Kind::Default(k);
                }
            }
        }

        let mut values: Vec<Vec<u64>> = vec![Vec::new()];
        let mut intern: HashMap<Vec<u64>, u32> = HashMap::new();
        let ns = model.states.len();
        let mut dense = vec![EMPTY; ns * np];
        for (q, state) in model.states.iter().enumerate() {
            // Arcs arrive sorted by (input, output, target, weight): each
            // pair's set is one run.
            let mut at = 0usize;
            while at < state.arcs.len() {
                let (i, o) = (state.arcs[at].input, state.arcs[at].output);
                let mut set: Vec<u64> = Vec::new();
                while at < state.arcs.len()
                    && (state.arcs[at].input, state.arcs[at].output) == (i, o)
                {
                    set.push(target_weight(state.arcs[at].target, state.arcs[at].weight));
                    at += 1;
                }
                set.sort_unstable();
                set.dedup();
                let id = match intern.get(&set) {
                    Some(id) => *id,
                    None => {
                        let id = values.len() as u32;
                        values.push(set.clone());
                        intern.insert(set, id);
                        id
                    }
                };
                dense[q * np + i as usize * n + o as usize] = id;
            }
        }
        Model {
            n,
            np,
            dense,
            values,
            pair_kind,
            n_states: ns,
        }
    }

    fn row(&self, q: usize) -> &[u32] {
        &self.dense[q * self.np..(q + 1) * self.np]
    }

    fn cost(&self, v: u32) -> u64 {
        if v == EMPTY {
            1
        } else {
            self.values[v as usize].len() as u64
        }
    }

    fn single(&self, v: u32) -> Option<u64> {
        if v == EMPTY {
            return None;
        }
        let s = &self.values[v as usize];
        if s.len() == 1 { Some(s[0]) } else { None }
    }
}

/// The largest single-valued group of one kind at one state. Its members are
/// the pairs of that kind whose only arc is `tw`; its span is the members for
/// identity, deletion and insertion, and `A × B` less the diagonal for
/// substitution.
#[derive(Clone)]
struct Group {
    tw: u64,
    a: Vec<u16>,
    b: Vec<u16>,
    a_mask: Vec<bool>,
    b_mask: Vec<bool>,
}

impl Group {
    #[inline(always)]
    fn in_span(&self, kind: DefaultKind, n: usize, p: usize) -> bool {
        let (i, o) = (p / n, p % n);
        match kind {
            DefaultKind::Identity => i == o && self.a_mask[i],
            DefaultKind::Substitution => i != o && self.a_mask[i] && self.b_mask[o],
            DefaultKind::Deletion => o == 0 && self.a_mask[i],
            DefaultKind::Insertion => i == 0 && self.b_mask[o],
        }
    }
}

fn groups_of(m: &Model, q: usize) -> [Option<Group>; 4] {
    let row = m.row(q);
    let mut by: [HashMap<u64, Vec<u32>>; 4] = Default::default();
    for (p, v) in row.iter().enumerate() {
        let Kind::Default(k) = m.pair_kind[p] else {
            continue;
        };
        if let Some(tw) = m.single(*v) {
            by[kind_index(k)].entry(tw).or_default().push(p as u32);
        }
    }
    let mut out: [Option<Group>; 4] = [None, None, None, None];
    for k in DefaultKind::ALL {
        let ki = kind_index(k);
        let Some((tw, members)) = by[ki]
            .iter()
            .max_by_key(|(tw, v)| (v.len(), std::cmp::Reverse(**tw)))
        else {
            continue;
        };
        if members.len() < 2 {
            continue;
        }
        let mut a_mask = vec![false; m.n];
        let mut b_mask = vec![false; m.n];
        for p in members {
            a_mask[*p as usize / m.n] = true;
            b_mask[*p as usize % m.n] = true;
        }
        let a: Vec<u16> = (0..m.n as u16).filter(|s| a_mask[*s as usize]).collect();
        let b: Vec<u16> = (0..m.n as u16).filter(|s| b_mask[*s as usize]).collect();
        out[ki] = Some(Group {
            tw: *tw,
            a,
            b,
            a_mask,
            b_mask,
        });
    }
    out
}

/// Per-kind cost of state `q` given fallback `f`, without and with that
/// kind's default, plus the special pairs' cost.
fn kind_costs(
    m: &Model,
    q: usize,
    f: Option<usize>,
    groups: &[Option<Group>; 4],
) -> ([u64; 4], [u64; 4], u64) {
    let row = m.row(q);
    let frow = f.map(|f| m.row(f));
    let mut without = [0u64; 4];
    let mut with = [0u64; 4];
    let mut special = 0u64;
    for p in 0..m.np {
        let v = row[p];
        let fv = frow.map_or(EMPTY, |r| r[p]);
        let differs = v != fv;
        let Kind::Default(k) = m.pair_kind[p] else {
            if differs {
                special += m.cost(v);
            }
            continue;
        };
        let ki = kind_index(k);
        if differs {
            without[ki] += m.cost(v);
        }
        if let Some(g) = &groups[ki] {
            let in_group = m.single(v) == Some(g.tw);
            if in_group {
                // answered by the default
            } else if g.in_span(k, m.n, p) || differs {
                with[ki] += m.cost(v);
            }
        }
    }
    for ki in 0..4 {
        if groups[ki].is_some() {
            with[ki] += 1;
        } else {
            with[ki] = u64::MAX;
        }
    }
    (without, with, special)
}

#[derive(Clone)]
enum EncEntry {
    /// explicit arcs for a pair (value id), or a blocker when `EMPTY`
    Pair(u32, u32),
    /// a default of this kind
    Default(DefaultKind),
}

struct Encoded {
    fallback: Vec<Option<usize>>,
    entries: Vec<Vec<EncEntry>>,
}

fn encode_state(
    m: &Model,
    q: usize,
    f: Option<usize>,
    groups: &[Option<Group>; 4],
    use_default: [bool; 4],
) -> Vec<EncEntry> {
    let row = m.row(q);
    let frow = f.map(|f| m.row(f));
    let mut out = Vec::new();
    for k in DefaultKind::ALL {
        if use_default[kind_index(k)] {
            out.push(EncEntry::Default(k));
        }
    }
    for p in 0..m.np {
        let v = row[p];
        let fv = frow.map_or(EMPTY, |r| r[p]);
        let differs = v != fv;
        let need = match m.pair_kind[p] {
            Kind::Special => differs,
            Kind::Default(k) => {
                let ki = kind_index(k);
                match (&groups[ki], use_default[ki]) {
                    (Some(g), true) => {
                        m.single(v) != Some(g.tw) && (g.in_span(k, m.n, p) || differs)
                    }
                    _ => differs,
                }
            }
        };
        if need {
            out.push(EncEntry::Pair(p as u32, v));
        }
    }
    out
}

/// Fallback candidates: states sharing a MinHash bucket over `(pair, value)`,
/// plus the state's most frequent targets.
fn candidates(
    m: &Model,
    model: &SourceModel,
    k: usize,
    per_bucket: usize,
    threads: usize,
) -> Vec<Vec<usize>> {
    let ns = m.n_states;
    let seeds: Vec<u64> = (0..k as u64)
        .map(|i| 0x9E37_79B9_7F4A_7C15u64.wrapping_mul(i + 1) ^ 0xD1B5_4A32_D192_ED03)
        .collect();
    let mut sig = vec![u64::MAX; ns * k];
    std::thread::scope(|scope| {
        let chunk = ns.div_ceil(threads).max(1);
        for (ci, part) in sig.chunks_mut(chunk * k).enumerate() {
            let seeds = &seeds;
            scope.spawn(move || {
                for (local, s) in part.chunks_mut(k).enumerate() {
                    let q = ci * chunk + local;
                    for (p, v) in m.row(q).iter().enumerate() {
                        if *v == EMPTY {
                            continue;
                        }
                        let base = ((p as u64) << 32) | *v as u64;
                        for (j, seed) in seeds.iter().enumerate() {
                            let mut h = base ^ seed;
                            h ^= h >> 33;
                            h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
                            h ^= h >> 33;
                            h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
                            h ^= h >> 33;
                            if h < s[j] {
                                s[j] = h;
                            }
                        }
                    }
                }
            });
        }
    });
    let mut cands: Vec<Vec<usize>> = vec![Vec::new(); ns];
    for j in 0..k {
        let mut buckets: HashMap<u64, Vec<usize>> = HashMap::new();
        for q in 0..ns {
            buckets.entry(sig[q * k + j]).or_default().push(q);
        }
        for members in buckets.values() {
            if members.len() < 2 {
                continue;
            }
            for (idx, q) in members.iter().enumerate() {
                for d in 1..=per_bucket {
                    let other = members[(idx + d) % members.len()];
                    if other != *q {
                        cands[*q].push(other);
                    }
                }
            }
        }
    }
    for (q, state) in model.states.iter().enumerate() {
        let mut freq: HashMap<u32, u32> = HashMap::new();
        for a in &state.arcs {
            *freq.entry(a.target).or_default() += 1;
        }
        let mut f: Vec<_> = freq.into_iter().collect();
        f.sort_by_key(|(t, c)| (std::cmp::Reverse(*c), *t));
        for (t, _) in f.into_iter().take(8) {
            if t as usize != q {
                cands[q].push(t as usize);
            }
        }
        cands[q].sort_unstable();
        cands[q].dedup();
    }
    cands
}

/// Every candidate's cost, cheapest first; `usize::MAX` stands for no
/// fallback.
fn score_candidates(
    m: &Model,
    groups: &[[Option<Group>; 4]],
    cands: &[Vec<usize>],
    threads: usize,
) -> Vec<Vec<(u64, usize)>> {
    let mut out: Vec<Vec<(u64, usize)>> = vec![Vec::new(); m.n_states];
    std::thread::scope(|scope| {
        let chunk = m.n_states.div_ceil(threads).max(1);
        for (ci, part) in out.chunks_mut(chunk).enumerate() {
            scope.spawn(move || {
                for (local, slot) in part.iter_mut().enumerate() {
                    let q = ci * chunk + local;
                    let score = |f: Option<usize>| -> u64 {
                        let (without, with, special) = kind_costs(m, q, f, &groups[q]);
                        let mut c = special;
                        for ki in 0..4 {
                            c += without[ki].min(with[ki]);
                        }
                        c
                    };
                    let mut scored: Vec<(u64, usize)> =
                        cands[q].iter().map(|f| (score(Some(*f)), *f)).collect();
                    scored.push((score(None), usize::MAX));
                    scored.sort_unstable();
                    *slot = scored;
                }
            });
        }
    });
    out
}

/// Greedy fallback assignment: the states that save most choose first; a
/// choice that closes a cycle or makes a chain deeper than `max_depth` is
/// passed over.
fn assign(
    m: &Model,
    groups: &[[Option<Group>; 4]],
    scored: &[Vec<(u64, usize)>],
    max_depth: usize,
) -> Encoded {
    let mut order: Vec<usize> = (0..m.n_states).collect();
    let saving = |q: usize| -> u64 {
        let none = scored[q]
            .iter()
            .find(|(_, f)| *f == usize::MAX)
            .map(|x| x.0)
            .unwrap_or(0);
        none.saturating_sub(scored[q][0].0)
    };
    order.sort_by_key(|q| std::cmp::Reverse(saving(*q)));
    let mut fallback: Vec<Option<usize>> = vec![None; m.n_states];
    let mut height: Vec<usize> = vec![0; m.n_states];
    let depth_of = |fallback: &Vec<Option<usize>>, mut x: usize| -> usize {
        let mut d = 0;
        while let Some(y) = fallback[x] {
            d += 1;
            x = y;
        }
        d
    };
    for &q in &order {
        for (_, f) in &scored[q] {
            if *f == usize::MAX {
                break;
            }
            let mut x = *f;
            let mut cyclic = false;
            loop {
                if x == q {
                    cyclic = true;
                    break;
                }
                match fallback[x] {
                    Some(y) => x = y,
                    None => break,
                }
            }
            if cyclic {
                continue;
            }
            if max_depth != usize::MAX && depth_of(&fallback, *f) + 1 + height[q] > max_depth {
                continue;
            }
            fallback[q] = Some(*f);
            let mut h = height[q] + 1;
            let mut x = Some(*f);
            while let Some(y) = x {
                if height[y] >= h {
                    break;
                }
                height[y] = h;
                h += 1;
                x = fallback[y];
            }
            break;
        }
    }
    let mut entries = Vec::with_capacity(m.n_states);
    for q in 0..m.n_states {
        let f = fallback[q];
        let (without, with, _) = kind_costs(m, q, f, &groups[q]);
        let mut use_default = [false; 4];
        for ki in 0..4 {
            use_default[ki] = with[ki] < without[ki];
        }
        entries.push(encode_state(m, q, f, &groups[q], use_default));
    }
    Encoded { fallback, entries }
}

/// Resolve every `(state, pair)` through the encoding and compare it with the
/// source. Answers the breakdown of source arcs by what answered them, and the
/// number of resolutions checked.
fn verify_encoding(
    m: &Model,
    groups: &[[Option<Group>; 4]],
    enc: &Encoded,
    threads: usize,
) -> Result<([u64; 7], u64), WriteError> {
    // Each level: explicit pairs sorted, and which kinds default.
    type Level = (Vec<(u32, u32)>, [bool; 4]);
    let levels: Vec<Level> = enc
        .entries
        .iter()
        .map(|es| {
            let mut pairs = Vec::new();
            let mut defaults = [false; 4];
            for e in es {
                match e {
                    EncEntry::Pair(p, v) => pairs.push((*p, *v)),
                    EncEntry::Default(k) => defaults[kind_index(*k)] = true,
                }
            }
            pairs.sort_unstable();
            (pairs, defaults)
        })
        .collect();

    let results: Vec<Result<([u64; 7], u64), String>> = std::thread::scope(|scope| {
        let chunk = m.n_states.div_ceil(threads).max(1);
        let handles: Vec<_> = (0..threads)
            .map(|ci| {
                let levels = &levels;
                scope.spawn(move || {
                    let mut breakdown = [0u64; 7];
                    let mut checked = 0u64;
                    for q in ci * chunk..((ci + 1) * chunk).min(m.n_states) {
                        let row = m.row(q);
                        for p in 0..m.np {
                            // (answer, depth, default kind)
                            let mut level = Some(q);
                            let mut depth = 0usize;
                            let mut answer: Option<(bool, u64, u32, Option<usize>)> = None;
                            while let Some(l) = level {
                                if depth > m.n_states {
                                    return Err(format!("fallback cycle at state {q}"));
                                }
                                let (pairs, defaults) = &levels[l];
                                if let Ok(at) = pairs.binary_search_by_key(&(p as u32), |e| e.0) {
                                    answer = Some((false, 0, pairs[at].1, None));
                                    break;
                                }
                                if let Kind::Default(k) = m.pair_kind[p] {
                                    let ki = kind_index(k);
                                    if defaults[ki]
                                        && let Some(g) = &groups[l][ki]
                                        && g.in_span(k, m.n, p)
                                    {
                                        answer = Some((true, g.tw, 0, Some(ki)));
                                        break;
                                    }
                                }
                                level = enc.fallback[l];
                                depth += 1;
                            }
                            let ok = match answer {
                                None => row[p] == EMPTY,
                                Some((false, _, v, _)) => v == row[p],
                                Some((true, tw, _, _)) => m.single(row[p]) == Some(tw),
                            };
                            checked += 1;
                            if !ok {
                                return Err(format!("state {q} pair {p}: encoding differs"));
                            }
                            if row[p] == EMPTY {
                                continue;
                            }
                            let arcs = m.values[row[p] as usize].len() as u64;
                            let slot = match (depth, answer.and_then(|a| a.3)) {
                                (0, None) => 0,
                                (0, Some(k)) => 1 + k,
                                (_, None) => 5,
                                (_, Some(_)) => 6,
                            };
                            breakdown[slot] += arcs;
                        }
                    }
                    Ok((breakdown, checked))
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err("a checker thread panicked".to_string()))
            })
            .collect()
    });
    let mut total = [0u64; 7];
    let mut checked = 0u64;
    for r in results {
        let (b, c) = r.map_err(WriteError::Verification)?;
        for i in 0..7 {
            total[i] += b[i];
        }
        checked += c;
    }
    Ok((total, checked))
}

fn pad8(v: &mut Vec<u8>) {
    while !v.len().is_multiple_of(8) {
        v.push(0);
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn serialise(
    model: &SourceModel,
    m: &Model,
    groups: &[[Option<Group>; 4]],
    enc: &Encoded,
    options: &WriteOptions,
) -> (Vec<u8>, WriteReport) {
    let nsym = model.symbols.len();
    let words = nsym.div_ceil(64);
    let mut report = WriteReport {
        states: m.n_states,
        source_arcs: model.arc_count(),
        ..WriteReport::default()
    };

    let mut classes: Vec<Vec<u16>> = Vec::new();
    let mut class_ix: HashMap<Vec<u16>, u16> = HashMap::new();
    let mut intern_class = |c: &Vec<u16>, classes: &mut Vec<Vec<u16>>| -> u16 {
        if let Some(i) = class_ix.get(c) {
            return *i;
        }
        let i = classes.len() as u16;
        classes.push(c.clone());
        class_ix.insert(c.clone(), i);
        i
    };
    let mut pairs: Vec<(u16, u16)> = Vec::new();
    let mut pair_ix: HashMap<(u16, u16), u16> = HashMap::new();

    let mut states: Vec<u8> = Vec::with_capacity(16 * m.n_states);
    let mut entries: Vec<u8> = Vec::new();
    let mut n_entries: u32 = 0;
    for q in 0..m.n_states {
        let mut explicit: Vec<(u16, u16, u32, f32)> = Vec::new();
        let mut defaults: Vec<(u16, u16, u32, f32)> = Vec::new();
        for e in &enc.entries[q] {
            match e {
                EncEntry::Pair(p, v) => {
                    let (i, o) = ((*p as usize / m.n) as u16, (*p as usize % m.n) as u16);
                    if *v == EMPTY {
                        explicit.push((i, o, NONE, 0.0));
                        report.blockers += 1;
                    } else {
                        for tw in &m.values[*v as usize] {
                            explicit.push((i, o, (tw >> 32) as u32, f32::from_bits(*tw as u32)));
                            report.explicit_arcs += 1;
                        }
                    }
                }
                EncEntry::Default(k) => {
                    let Some(g) = &groups[q][kind_index(*k)] else {
                        continue;
                    };
                    let class = match k {
                        DefaultKind::Identity | DefaultKind::Deletion => {
                            intern_class(&g.a, &mut classes)
                        }
                        DefaultKind::Insertion => intern_class(&g.b, &mut classes),
                        DefaultKind::Substitution => {
                            let a = intern_class(&g.a, &mut classes);
                            let b = intern_class(&g.b, &mut classes);
                            *pair_ix.entry((a, b)).or_insert_with(|| {
                                pairs.push((a, b));
                                (pairs.len() - 1) as u16
                            })
                        }
                    };
                    report.defaults[kind_index(*k)] += 1;
                    defaults.push((
                        k.record(),
                        class,
                        (g.tw >> 32) as u32,
                        f32::from_bits(g.tw as u32),
                    ));
                }
            }
        }
        explicit.sort_by_key(|e| (e.0, e.1, e.2, e.3.to_bits()));
        defaults.sort_by_key(|e| e.0);
        let first = n_entries;
        for (i, o, t, w) in explicit.iter().chain(defaults.iter()) {
            entries.extend_from_slice(&i.to_le_bytes());
            entries.extend_from_slice(&o.to_le_bytes());
            entries.extend_from_slice(&t.to_le_bytes());
            entries.extend_from_slice(&w.to_bits().to_le_bytes());
            n_entries += 1;
        }
        states.extend_from_slice(&first.to_le_bytes());
        states.extend_from_slice(&((explicit.len() + defaults.len()) as u32).to_le_bytes());
        states.extend_from_slice(
            &enc.fallback[q]
                .map(|f| f as u32)
                .unwrap_or(NONE)
                .to_le_bytes(),
        );
        states.extend_from_slice(
            &model.states[q]
                .final_weight
                .unwrap_or(f32::INFINITY)
                .to_bits()
                .to_le_bytes(),
        );
    }

    // Chain depths, for the header's guarantee and the report.
    let mut depth_histogram = vec![0usize; 1];
    let mut max_depth = 0u32;
    for q in 0..m.n_states {
        let mut d = 0usize;
        let mut l = enc.fallback[q];
        if l.is_some() {
            report.states_with_fallback += 1;
        }
        while let Some(x) = l {
            d += 1;
            l = enc.fallback[x];
        }
        if depth_histogram.len() <= d {
            depth_histogram.resize(d + 1, 0);
        }
        depth_histogram[d] += 1;
        max_depth = max_depth.max(d as u32);
    }
    report.depth_histogram = depth_histogram;
    report.max_depth = max_depth;
    report.classes = classes.len();
    report.class_pairs = pairs.len();

    let mut syms: Vec<u8> = Vec::new();
    syms.extend_from_slice(&(nsym as u32).to_le_bytes());
    let mut off = 0u32;
    let mut blob: Vec<u8> = Vec::new();
    for s in &model.symbols {
        syms.extend_from_slice(&off.to_le_bytes());
        blob.extend_from_slice(s.as_bytes());
        off += s.len() as u32;
    }
    syms.extend_from_slice(&off.to_le_bytes());
    syms.extend_from_slice(&blob);

    let mut clas: Vec<u8> = Vec::new();
    clas.extend_from_slice(&(words as u32).to_le_bytes());
    clas.extend_from_slice(&(classes.len() as u32).to_le_bytes());
    for c in &classes {
        let mut bits = vec![0u64; words];
        for s in c {
            bits[*s as usize / 64] |= 1u64 << (*s as usize % 64);
        }
        for b in bits {
            clas.extend_from_slice(&b.to_le_bytes());
        }
    }
    let mut cpai: Vec<u8> = Vec::new();
    cpai.extend_from_slice(&(pairs.len() as u32).to_le_bytes());
    cpai.extend_from_slice(&0u32.to_le_bytes());
    for (a, b) in &pairs {
        cpai.extend_from_slice(&a.to_le_bytes());
        cpai.extend_from_slice(&b.to_le_bytes());
    }
    let mut stat: Vec<u8> = Vec::new();
    stat.extend_from_slice(&(m.n_states as u32).to_le_bytes());
    stat.extend_from_slice(&0u32.to_le_bytes());
    stat.extend_from_slice(&states);
    let mut entr: Vec<u8> = Vec::new();
    entr.extend_from_slice(&n_entries.to_le_bytes());
    entr.extend_from_slice(&0u32.to_le_bytes());
    entr.extend_from_slice(&entries);
    let bound = match options.max_fallback_depth {
        Some(d) => d.to_string(),
        None => "null".to_string(),
    };
    let meta = format!(
        "{{\"writer\":\"divvun-fst {}\",\"source\":\"{}\",\"max-fallback-depth-bound\":{},\"source-states\":{},\"source-arcs\":{},\"source-duplicate-arcs\":{}}}",
        env!("CARGO_PKG_VERSION"),
        json_escape(&options.source_name),
        bound,
        m.n_states,
        report.source_arcs,
        model.duplicate_arcs,
    )
    .into_bytes();

    let mut sections: Vec<([u8; 4], Vec<u8>)> = vec![(tag::SYMS, syms)];
    if !classes.is_empty() {
        sections.push((tag::CLAS, clas));
    }
    if !pairs.is_empty() {
        sections.push((tag::CPAI, cpai));
    }
    sections.push((tag::STAT, stat));
    sections.push((tag::ENTR, entr));
    sections.push((tag::META, meta));

    let mut flags = FLAG_TROPICAL;
    if report.states_with_fallback > 0 {
        flags |= FLAG_FALLBACK;
    }
    if report.defaults.iter().any(|d| *d > 0) {
        flags |= FLAG_DEFAULTS;
    }

    let table_len = sections.len() * SECTION_ENTRY_LEN;
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.extend_from_slice(&[0u8; 2]);
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&(sections.len() as u32).to_le_bytes());
    out.extend_from_slice(&max_depth.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    debug_assert_eq!(out.len(), HEADER_LEN);
    let mut offset = (HEADER_LEN + table_len).div_ceil(8) * 8;
    for (t, body) in &sections {
        out.extend_from_slice(t);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(offset as u64).to_le_bytes());
        out.extend_from_slice(&(body.len() as u64).to_le_bytes());
        offset = (offset + body.len()).div_ceil(8) * 8;
    }
    pad8(&mut out);
    for (_, body) in &sections {
        out.extend_from_slice(body);
        pad8(&mut out);
    }
    (out, report)
}
