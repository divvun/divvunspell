//! Writing DHFST acceptors (type 2) from an existing lexicon.
//!
//! The writer reads the source exactly as the suggestion search does, through
//! [`Transducer::free_arcs`], [`Transducer::has_transitions`] and
//! [`Transducer::transitions`], so what it stores is what the search sees:
//! the same arcs, in the same order, with the same weights, bit for bit. Each
//! state's distance to a final state is the source's
//! [`Transducer::distance_to_final`], bit for bit, so that the search's
//! lookahead orders its queue the same way; when the source answers 0 for
//! every state, the file leaves the distances out.
//!
//! States are placed in the slot table first fit: each state takes the
//! least number that is no other state's and whose slots (`q` for its free
//! arcs, `q + s` for its arcs on `s`) are all empty. The start state goes
//! first, at 0. The order the others are placed in is a writer choice; it
//! changes the size and the locality, not the meaning. Depth-first order, the
//! default, fills the table as well as placing the states with most slots
//! first does, or better, and keeps the states along a word close together.
//! Placement is deterministic.
//!
//! Nothing is written on trust. The bytes are read back through
//! [`DhfstAcceptor`] and every state's finality, distance, free arcs and
//! answer for every symbol of the alphabet are compared with the source's.
//! Any difference is an error.

use std::collections::HashMap;

use crate::transducer::Transducer;
use crate::transducer::dhfst::PREFIX_LEN;
use crate::transducer::dhfst::acceptor::{
    DhfstAcceptor, FINL_STATES, FLAG_TROPICAL, HEADER_LEN, PADDING, SECTION_ENTRY_LEN, prefix, tag,
};
use crate::transducer::dhfst::writer::WriteError;
use crate::types::{SymbolNumber, TransitionTableIndex};

/// The order states are placed in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Placement {
    /// Depth-first pre-order from the start state.
    #[default]
    DepthFirst,
    /// States that need the most slots first, ties in depth-first order: the
    /// big states fill an empty table and the small ones its gaps.
    FanOut,
}

/// How to write an acceptor.
#[derive(Clone, Debug, Default)]
pub struct AcceptorOptions {
    /// The order states are placed in.
    pub placement: Placement,
    /// Worker threads for the check; `0` for all cores.
    pub threads: usize,
    /// A name for the source, recorded in the `meta` section.
    pub source_name: String,
}

/// What the writer wrote.
#[derive(Clone, Debug)]
pub struct WrittenAcceptor {
    /// the file
    pub bytes: Vec<u8>,
    /// states written
    pub states: u32,
    /// state numbers: every state is numbered below this
    pub ids: u32,
    /// slots in the table
    pub slots: u32,
    /// slots that hold arcs, free or regular
    pub used_slots: u32,
    /// records in `LIST`
    pub list_records: u32,
    /// arcs written, free and regular
    pub arcs: u64,
    /// free (epsilon and flag) arcs written
    pub free_arcs: u64,
    /// distinct regular arc weights
    pub weights: usize,
    /// distinct free symbol and weight pairs
    pub free_pairs: usize,
    /// whether the file stores distances to a final state
    pub distances: bool,
    /// `(state, symbol)` answers compared against the source
    pub checked: u64,
    /// free symbols on which the source's regular lookup answers yes: a
    /// quirk of the optimized-lookup cursor that the search never asks
    pub free_symbol_quirks: u64,
    /// the source state behind each state, in depth-first order
    pub order: Vec<u32>,
    /// the number of each state of `order`
    pub ids_of: Vec<u32>,
}

/// One arc read from the source: symbol, source target, weight bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Arc {
    symbol: u16,
    target: u32,
    weight: u32,
}

/// A source state as the search reads it.
#[derive(Clone, Debug, Default)]
struct State {
    source: u32,
    final_weight: Option<u32>,
    /// the bits of the source's distance to a final state
    distance: u32,
    free: Vec<Arc>,
    regular: Vec<Arc>,
}

fn unsupported(detail: impl Into<String>) -> WriteError {
    WriteError::Unsupported(detail.into())
}

fn is_free<T: Transducer>(source: &T, symbol: u16) -> bool {
    symbol == 0 || source.alphabet().is_flag(SymbolNumber(symbol))
}

/// The free arcs of `state` as the search reads them.
fn free_arcs<T: Transducer>(source: &T, state: u32) -> Result<Vec<Arc>, WriteError> {
    let mut arcs = Vec::new();
    for (input, t) in source.free_arcs(TransitionTableIndex(state)) {
        match (t.symbol(), t.target(), t.weight()) {
            (Some(output), Some(target), Some(weight)) if output == input => arcs.push(Arc {
                symbol: input.0,
                target: target.0,
                weight: weight.0.to_bits(),
            }),
            _ => {
                return Err(unsupported(format!(
                    "free arc {t:?} of state {state} is not an acceptor arc"
                )));
            }
        }
    }
    Ok(arcs)
}

/// The regular arcs of `state` on `symbol` as the search reads them.
fn regular_arcs<T: Transducer>(
    source: &T,
    state: u32,
    symbol: u16,
    arcs: &mut Vec<Arc>,
) -> Result<(), WriteError> {
    for t in source.transitions(TransitionTableIndex(state), SymbolNumber(symbol)) {
        match (t.symbol(), t.target(), t.weight()) {
            (Some(output), Some(target), Some(weight)) if output.0 == symbol => arcs.push(Arc {
                symbol,
                target: target.0,
                weight: weight.0.to_bits(),
            }),
            _ => {
                return Err(unsupported(format!(
                    "arc {t:?} of state {state} on symbol {symbol} is not an acceptor arc"
                )));
            }
        }
    }
    Ok(())
}

fn read_state<T: Transducer>(source: &T, state: u32, n_symbols: u16) -> Result<State, WriteError> {
    let at = TransitionTableIndex(state);
    let final_weight = if source.is_final(at) {
        let w = source
            .final_weight(at)
            .ok_or_else(|| unsupported(format!("final state {state} has no final weight")))?;
        if !w.0.is_finite() {
            return Err(unsupported(format!(
                "state {state} has final weight {}",
                w.0
            )));
        }
        Some(w.0.to_bits())
    } else {
        None
    };
    let distance = source.distance_to_final(at).0;
    if distance.is_nan() || distance == f32::NEG_INFINITY {
        return Err(unsupported(format!(
            "state {state} has distance {distance} to a final state"
        )));
    }
    let free = free_arcs(source, state)?;
    let mut regular = Vec::new();
    for symbol in 1..n_symbols {
        if is_free(source, symbol) || !source.has_transitions(at.incr(), Some(SymbolNumber(symbol)))
        {
            continue;
        }
        regular_arcs(source, state, symbol, &mut regular)?;
    }
    for arc in free.iter().chain(regular.iter()) {
        let w = f32::from_bits(arc.weight);
        if w.is_nan() || w == f32::NEG_INFINITY {
            return Err(unsupported(format!(
                "state {state} has an arc weighing {w}"
            )));
        }
    }
    Ok(State {
        source: state,
        final_weight,
        distance: distance.to_bits(),
        free,
        regular,
    })
}

/// Bits needed to hold every value in `0..=max`.
fn bits(max: u64) -> u32 {
    64 - max.leading_zeros()
}

/// A dictionary of `values` by descending frequency (ties by value), and each
/// value's index in it.
fn dictionary<V>(values: impl Iterator<Item = V>) -> (Vec<V>, HashMap<V, u32>)
where
    V: Copy + Ord + std::hash::Hash,
{
    let mut count: HashMap<V, u64> = HashMap::new();
    for v in values {
        *count.entry(v).or_default() += 1;
    }
    let mut sorted: Vec<(V, u64)> = count.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let index = sorted
        .iter()
        .enumerate()
        .map(|(i, (v, _))| (*v, i as u32))
        .collect();
    (sorted.into_iter().map(|(v, _)| v).collect(), index)
}

fn push_uint(out: &mut Vec<u8>, value: u64, bytes: usize) {
    out.extend_from_slice(&value.to_le_bytes()[..bytes]);
}

fn pad8(out: &mut Vec<u8>) {
    while !out.len().is_multiple_of(8) {
        out.push(0);
    }
}

/// Every state reachable from the start state of `source`, as the search
/// reads it, in depth-first pre-order with successors in stored order, and
/// each source state's position in that order.
fn read_states<T: Transducer>(
    source: &T,
    n_symbols: u16,
) -> Result<(Vec<State>, HashMap<u32, u32>), WriteError> {
    let mut states: Vec<State> = Vec::new();
    let mut number: HashMap<u32, u32> = HashMap::new();
    let mut stack = vec![0u32];
    while let Some(s) = stack.pop() {
        if number.contains_key(&s) {
            continue;
        }
        number.insert(s, states.len() as u32);
        let state = read_state(source, s, n_symbols)?;
        for arc in state.free.iter().chain(state.regular.iter()).rev() {
            if !number.contains_key(&arc.target) {
                stack.push(arc.target);
            }
        }
        states.push(state);
    }
    Ok((states, number))
}

/// The `SYMS` section body for the symbols `names`.
fn symbol_section(names: &[String]) -> Vec<u8> {
    let mut syms = Vec::new();
    syms.extend_from_slice(&(names.len() as u32).to_le_bytes());
    let mut offset = 0u32;
    syms.extend_from_slice(&offset.to_le_bytes());
    for name in names {
        offset += name.len() as u32;
        syms.extend_from_slice(&offset.to_le_bytes());
    }
    for name in names {
        syms.extend_from_slice(name.as_bytes());
    }
    syms
}

/// The `FINL` section body: one final weight's bits, or `None`, per state
/// number.
fn final_section(finals: &[Option<u32>]) -> Vec<u8> {
    let mut finl = Vec::new();
    let final_weights: Vec<u32> = finals.iter().filter_map(|w| *w).collect();
    finl.extend_from_slice(&(final_weights.len() as u32).to_le_bytes());
    finl.extend_from_slice(&0u32.to_le_bytes());
    let mut before = 0u32;
    for chunk in finals.chunks(FINL_STATES) {
        let bits = chunk
            .iter()
            .enumerate()
            .filter(|(_, w)| w.is_some())
            .fold(0u64, |w, (i, _)| w | (1 << i));
        finl.extend_from_slice(&bits.to_le_bytes());
        finl.extend_from_slice(&before.to_le_bytes());
        before += bits.count_ones();
    }
    for w in &final_weights {
        finl.extend_from_slice(&w.to_le_bytes());
    }
    finl.extend_from_slice(&[0u8; PADDING]);
    finl
}

/// The `FREE` section body: the free pairs, each a symbol in the upper and
/// a weight's bits in the lower 32 bits.
fn free_section(pairs: &[u64]) -> Vec<u8> {
    let mut free = Vec::new();
    free.extend_from_slice(&(pairs.len() as u32).to_le_bytes());
    free.extend_from_slice(&0u32.to_le_bytes());
    for key in pairs {
        free.extend_from_slice(&((key >> 32) as u16).to_le_bytes());
        free.extend_from_slice(&0u16.to_le_bytes());
        free.extend_from_slice(&(*key as u32).to_le_bytes());
    }
    free.extend_from_slice(&[0u8; PADDING]);
    free
}

/// A section body of `u32 n; u32 0; f32 values[n]` and its padding, from the
/// values' bits: the `WGHT` and `DIST` sections.
fn float_section(values: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(values.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&[0u8; PADDING]);
    out
}

/// A whole file: the eight-byte `prefix`, the header `flags`, the section
/// table and the sections, each at a multiple of 8.
fn assemble(prefix: [u8; PREFIX_LEN], flags: u32, sections: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&prefix);
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&(sections.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());
    debug_assert_eq!(out.len(), HEADER_LEN);
    let table = out.len();
    out.resize(table + SECTION_ENTRY_LEN * sections.len(), 0);
    for (i, (section_tag, body)) in sections.iter().enumerate() {
        pad8(&mut out);
        let at = table + SECTION_ENTRY_LEN * i;
        let offset = out.len() as u64;
        out[at..at + 4].copy_from_slice(section_tag);
        out[at + 8..at + 16].copy_from_slice(&offset.to_le_bytes());
        out[at + 16..at + 24].copy_from_slice(&(body.len() as u64).to_le_bytes());
        out.extend_from_slice(body);
    }
    out
}

/// A state's arcs on one regular symbol.
struct Group<'a> {
    label: u16,
    arcs: &'a [Arc],
}

/// The regular arcs of `state`, grouped by symbol; the source reading sorts
/// them by symbol.
fn groups(state: &State) -> Vec<Group<'_>> {
    state
        .regular
        .chunk_by(|a, b| a.symbol == b.symbol)
        .map(|arcs| Group {
            label: arcs[0].symbol,
            arcs,
        })
        .collect()
}

/// A growable bit set.
#[derive(Default)]
struct Bits(Vec<u64>);

impl Bits {
    fn set(&mut self, i: usize) {
        if i / 64 >= self.0.len() {
            self.0.resize(i / 64 + 1, 0);
        }
        self.0[i / 64] |= 1 << (i % 64);
    }

    /// The least clear bit at `i` or after it.
    fn next_clear(&self, i: usize) -> usize {
        let mut w = i / 64;
        let mut word = match self.0.get(w) {
            Some(word) => word | ((1u64 << (i % 64)) - 1),
            None => return i,
        };
        loop {
            if word != u64::MAX {
                return 64 * w + word.trailing_ones() as usize;
            }
            w += 1;
            word = match self.0.get(w) {
                Some(word) => *word,
                None => return 64 * w,
            };
        }
    }

    /// Bits `i .. i + 64`, bit `i` lowest.
    fn window(&self, i: usize) -> u64 {
        let word = |w: usize| self.0.get(w).copied().unwrap_or(0);
        let (w, shift) = (i / 64, i % 64);
        if shift == 0 {
            word(w)
        } else {
            word(w) >> shift | word(w + 1) << (64 - shift)
        }
    }
}

/// The number of each state, placing the states of `order` first fit; each
/// state needs the slots `labels[state]` (ascending) above its number. A
/// state that needs none takes the least number left once the others are
/// placed.
fn place(labels: &[Vec<u16>], order: &[usize]) -> Vec<u32> {
    let mut number = vec![u32::MAX; labels.len()];
    let mut occupied = Bits::default();
    let mut taken = Bits::default();
    let mut first_clear = 0usize;
    let mut fitted: HashMap<&[u16], usize> = HashMap::new();
    for &q in order {
        let Some(&low) = labels[q].first() else {
            continue;
        };
        // No slot below `first_clear` is empty, and slots and numbers only
        // get taken: a number that did not fit a set of labels before never
        // will. Sixty-four candidate numbers at a time: those no state has
        // and whose slots are all empty.
        let tried = fitted.entry(labels[q].as_slice()).or_insert(0);
        let mut window = first_clear.saturating_sub(low as usize).max(*tried);
        let base = loop {
            let mut fits = !taken.window(window);
            for &x in &labels[q] {
                fits &= !occupied.window(window + x as usize);
            }
            if fits != 0 {
                break window + fits.trailing_zeros() as usize;
            }
            window += 64;
        };
        for &x in &labels[q] {
            occupied.set(base + x as usize);
        }
        taken.set(base);
        *tried = base + 1;
        number[q] = base as u32;
        first_clear = occupied.next_clear(first_clear);
    }
    let mut next = 0usize;
    for &q in order {
        if labels[q].is_empty() {
            next = taken.next_clear(next);
            taken.set(next);
            number[q] = next as u32;
        }
    }
    number
}

/// Write `source`, an acceptor whose symbols are named `names`, as a DHFST
/// acceptor, and check the bytes read back against it.
pub fn write_acceptor<T>(
    source: &T,
    names: &[String],
    options: &AcceptorOptions,
) -> Result<WrittenAcceptor, WriteError>
where
    T: Transducer + Sync,
{
    let n_symbols = source.alphabet().initial_symbol_count().0;
    if names.len() != n_symbols as usize {
        return Err(unsupported(format!(
            "{} symbol names for an alphabet of {n_symbols}",
            names.len()
        )));
    }
    let (states, number) = read_states(source, n_symbols)?;
    let n_states = states.len();
    let groups: Vec<Vec<Group<'_>>> = states.iter().map(groups).collect();

    // The slots each state needs above its number.
    let labels: Vec<Vec<u16>> = states
        .iter()
        .zip(&groups)
        .map(|(state, groups)| {
            let free = (!state.free.is_empty()).then_some(0u16);
            free.into_iter()
                .chain(groups.iter().map(|g| g.label))
                .collect()
        })
        .collect();
    let label_max = labels
        .iter()
        .filter_map(|l| l.last())
        .copied()
        .max()
        .unwrap_or(0);
    let check_bytes = if label_max <= 0xFF { 1 } else { 2 };

    // The start state first, at 0.
    let mut order: Vec<usize> = (1..n_states).collect();
    if options.placement == Placement::FanOut {
        order.sort_by_key(|&q| std::cmp::Reverse(labels[q].len()));
    }
    order.insert(0, 0);
    let ids_of = place(&labels, &order);
    let n_ids = ids_of.iter().copied().max().map_or(1, |m| m + 1);
    let mut by_id: Vec<Option<usize>> = vec![None; n_ids as usize];
    for (q, &id) in ids_of.iter().enumerate() {
        if by_id[id as usize].replace(q).is_some() {
            return Err(WriteError::Verification(format!(
                "two states were given number {id}"
            )));
        }
    }
    let used_end = ids_of
        .iter()
        .zip(&labels)
        .filter_map(|(id, l)| l.last().map(|x| *id as usize + *x as usize + 1))
        .max()
        .unwrap_or(0);
    let n_slots = used_end.max(n_ids as usize + label_max as usize);
    if n_slots >= u32::MAX as usize {
        return Err(unsupported("too many slots"));
    }

    // Weights and free pairs, most frequent first.
    let (weights, weight_index) = dictionary(
        states
            .iter()
            .flat_map(|s| s.regular.iter().map(|a| a.weight)),
    );
    let pair_key = |a: &Arc| (a.symbol as u64) << 32 | a.weight as u64;
    let (pairs, pair_index) = dictionary(states.iter().flat_map(|s| s.free.iter().map(pair_key)));

    // Field widths.
    let target = |a: &Arc| -> Result<u64, WriteError> {
        number
            .get(&a.target)
            .map(|q| ids_of[*q as usize] as u64)
            .ok_or_else(|| unsupported(format!("arc to unvisited state {}", a.target)))
    };
    let n_list: u64 = states
        .iter()
        .zip(&groups)
        .map(|(s, g)| {
            s.free.len() as u64
                + g.iter()
                    .filter(|g| g.arcs.len() > 1)
                    .map(|g| 1 + g.arcs.len() as u64)
                    .sum::<u64>()
        })
        .sum();
    let max_free = states.iter().map(|s| s.free.len()).max().unwrap_or(0) as u64;
    let target_bits = bits((n_ids as u64 - 1).max(n_list.saturating_sub(1))).max(1);
    let index_bits = bits(
        (weights.len() as u64)
            .max(pairs.len().saturating_sub(1) as u64)
            .max(max_free),
    )
    .max(1);
    let record_bytes = (target_bits + index_bits).div_ceil(8) as usize;
    if target_bits > 32 || index_bits > 32 || record_bytes > 8 {
        return Err(unsupported(format!(
            "records of {target_bits} and {index_bits} bits do not fit"
        )));
    }
    let list_mark = (1u64 << index_bits) - 1;
    let record = |first: u64, second: u64| first | second << target_bits;

    // Checks, slot records and lists, in slot order.
    let mut checks = vec![0u16; n_slots];
    let mut slot_records = vec![0u64; n_slots];
    let mut list: Vec<u64> = Vec::with_capacity(n_list as usize);
    for &q in by_id.iter().flatten() {
        let base = ids_of[q] as usize;
        let state = &states[q];
        if !state.free.is_empty() {
            slot_records[base] = record(list.len() as u64, state.free.len() as u64);
            for a in &state.free {
                list.push(record(target(a)?, pair_index[&pair_key(a)] as u64));
            }
        }
        for g in &groups[q] {
            let slot = base + g.label as usize;
            checks[slot] = g.label;
            let arc = |a: &Arc| -> Result<u64, WriteError> {
                Ok(record(target(a)?, weight_index[&a.weight] as u64))
            };
            if let [a] = g.arcs {
                slot_records[slot] = arc(a)?;
            } else {
                slot_records[slot] = record(list.len() as u64, list_mark);
                list.push(g.arcs.len() as u64);
                for a in g.arcs {
                    list.push(arc(a)?);
                }
            }
        }
    }
    debug_assert_eq!(list.len() as u64, n_list);

    let mut chck = Vec::new();
    chck.extend_from_slice(&(n_slots as u32).to_le_bytes());
    chck.extend_from_slice(&n_ids.to_le_bytes());
    chck.extend_from_slice(&label_max.to_le_bytes());
    chck.extend_from_slice(&[check_bytes as u8, 0, 0, 0, 0, 0]);
    for plane in 0..check_bytes {
        chck.extend(checks.iter().map(|c| (c >> (8 * plane)) as u8));
        chck.extend_from_slice(&[0u8; PADDING]);
    }

    let mut slot = Vec::new();
    slot.extend_from_slice(&(n_slots as u32).to_le_bytes());
    slot.extend_from_slice(&[record_bytes as u8, target_bits as u8, index_bits as u8, 0]);
    for r in &slot_records {
        push_uint(&mut slot, *r, record_bytes);
    }
    slot.extend_from_slice(&[0u8; PADDING]);

    let mut lst = Vec::new();
    lst.extend_from_slice(&(list.len() as u32).to_le_bytes());
    lst.extend_from_slice(&[record_bytes as u8, 0, 0, 0]);
    for r in &list {
        push_uint(&mut lst, *r, record_bytes);
    }
    lst.extend_from_slice(&[0u8; PADDING]);

    // Finality and distance by state number; a number no state has is not
    // final and is 0 from a final state.
    let mut finals = vec![None; n_ids as usize];
    let mut distances = vec![0u32; n_ids as usize];
    for (q, state) in states.iter().enumerate() {
        finals[ids_of[q] as usize] = state.final_weight;
        distances[ids_of[q] as usize] = state.distance;
    }
    let with_distances = distances.iter().any(|d| *d != 0);

    let arcs: u64 = states
        .iter()
        .map(|s| (s.free.len() + s.regular.len()) as u64)
        .sum();
    let free_arcs: u64 = states.iter().map(|s| s.free.len() as u64).sum();
    let used_slots = checks
        .iter()
        .zip(&slot_records)
        .filter(|(c, r)| **c != 0 || **r != 0)
        .count() as u32;
    let meta = serde_json::json!({
        "writer": "divvunspell dhfst acceptor writer",
        "source": options.source_name,
        "states": n_states,
        "arcs": arcs,
        "free_arcs": free_arcs,
        "placement": match options.placement {
            Placement::FanOut => "fan-out",
            Placement::DepthFirst => "depth-first",
        },
        "distances": with_distances,
    })
    .to_string()
    .into_bytes();

    let mut sections: Vec<([u8; 4], Vec<u8>)> = vec![
        (tag::SYMS, symbol_section(names)),
        (tag::CHCK, chck),
        (tag::SLOT, slot),
        (tag::LIST, lst),
        (tag::FINL, final_section(&finals)),
        (tag::FREE, free_section(&pairs)),
        (tag::WGHT, float_section(&weights)),
    ];
    if with_distances {
        sections.push((tag::DIST, float_section(&distances)));
    }
    sections.push((tag::META, meta));
    let mut written = WrittenAcceptor {
        bytes: assemble(prefix(), FLAG_TROPICAL, &sections),
        states: n_states as u32,
        ids: n_ids,
        slots: n_slots as u32,
        used_slots,
        list_records: list.len() as u32,
        arcs,
        free_arcs,
        weights: weights.len(),
        free_pairs: pairs.len(),
        distances: with_distances,
        checked: 0,
        free_symbol_quirks: 0,
        order: states.iter().map(|s| s.source).collect(),
        ids_of,
    };

    let reader = DhfstAcceptor::from_bytes(&written.bytes, "written acceptor")
        .map_err(WriteError::ReadBack)?;
    let (checked, quirks) = verify_acceptor(source, &reader, &written, options.threads)?;
    written.checked = checked;
    written.free_symbol_quirks = quirks;
    Ok(written)
}

/// Check that `reader` answers as `source` does for every state `written`
/// wrote: its finality, its distance to a final state, its free arcs, and
/// for every symbol of the alphabet and a few past it whether it has arcs on
/// that symbol and which. Returns the number of `(state, symbol)` answers
/// compared and the number of free symbols on which the source's regular
/// lookup answered yes (which the search never asks, and the reader answers
/// no).
pub fn verify_acceptor<T>(
    source: &T,
    reader: &DhfstAcceptor,
    written: &WrittenAcceptor,
    threads: usize,
) -> Result<(u64, u64), WriteError>
where
    T: Transducer + Sync,
{
    if reader.id_count() != written.ids || written.ids_of.len() != written.order.len() {
        return Err(WriteError::Verification(format!(
            "reader numbers states below {}, the writer below {}",
            reader.id_count(),
            written.ids
        )));
    }
    verify_states(
        source,
        reader,
        &written.order,
        |q| written.ids_of[q],
        threads,
    )
}

/// [`verify_acceptor`] for the states of `order` (source state numbers),
/// the reader numbering the state at `order[q]` `id(q)`.
fn verify_states<T, R>(
    source: &T,
    reader: &R,
    order: &[u32],
    id: impl Fn(usize) -> u32 + Sync,
    threads: usize,
) -> Result<(u64, u64), WriteError>
where
    T: Transducer + Sync,
    R: Transducer + Sync,
{
    let fail = |detail: String| WriteError::Verification(detail);
    // The alphabets agree on everything the search reads. Epsilon and the
    // flag diacritics are never written out, and a THFST alphabet may name
    // them where one parsed from symbol names leaves them blank; its length
    // field is whatever the tool that wrote it put there.
    let (a, b) = (source.alphabet(), reader.alphabet());
    let named = |s: usize| s != 0 && !a.is_flag(SymbolNumber(s as u16));
    let same_operations = a.operations().len() == b.operations().len()
        && a.operations().iter().all(|(s, op)| {
            b.operations().get(s).is_some_and(|other| {
                (other.operation, other.feature, other.value)
                    == (op.operation, op.feature, op.value)
            })
        });
    let differences: Vec<&str> = [
        (
            a.key_table().len() == b.key_table().len()
                && (0..a.key_table().len())
                    .all(|s| !named(s) || a.key_table()[s] == b.key_table()[s]),
            "symbol names",
        ),
        (
            a.initial_symbol_count() == b.initial_symbol_count(),
            "symbol count",
        ),
        (a.state_size() == b.state_size(), "flag state size"),
        (a.identity() == b.identity(), "identity symbol"),
        (a.unknown() == b.unknown(), "unknown symbol"),
        (
            a.string_to_symbol() == b.string_to_symbol(),
            "string to symbol map",
        ),
        (same_operations, "flag operations"),
    ]
    .into_iter()
    .filter(|(same, _)| !same)
    .map(|(_, what)| what)
    .collect();
    if !differences.is_empty() {
        return Err(fail(format!(
            "the reader's alphabet differs from the source's in: {}",
            differences.join(", ")
        )));
    }
    let number: HashMap<u32, u32> = order.iter().enumerate().map(|(i, s)| (*s, id(i))).collect();
    let n_symbols = a.initial_symbol_count().0 as u32;
    let threads = match threads {
        0 => std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4),
        n => n,
    };
    let chunk = order.len().div_ceil(threads).max(1);

    let check = |range: std::ops::Range<usize>| -> Result<(u64, u64), WriteError> {
        let mut checked = 0u64;
        let mut quirks = 0u64;
        let map = |t: Option<TransitionTableIndex>| t.and_then(|t| number.get(&t.0).copied());
        for q in range {
            let s = TransitionTableIndex(order[q]);
            let r = TransitionTableIndex(id(q));
            let weight_bits = |w: Option<crate::types::Weight>| w.map(|w| w.0.to_bits());
            if source.is_final(s) != reader.is_final(r)
                || weight_bits(source.final_weight(s).filter(|_| source.is_final(s)))
                    != weight_bits(reader.final_weight(r))
            {
                return Err(fail(format!("finality of state {} differs", s.0)));
            }
            let (want, got) = (source.distance_to_final(s), reader.distance_to_final(r));
            if want.0.to_bits() != got.0.to_bits() {
                return Err(fail(format!(
                    "distance of state {} to a final state: source {}, reader {}",
                    s.0, want.0, got.0
                )));
            }
            if source.has_epsilons_or_flags(s.incr()) != reader.has_epsilons_or_flags(r.incr()) {
                return Err(fail(format!("free arcs of state {} differ", s.0)));
            }
            let want: Vec<_> = source
                .free_arcs(s)
                .map(|(input, t)| (input, t.symbol(), map(t.target()), weight_bits(t.weight())))
                .collect();
            let got: Vec<_> = reader
                .free_arcs(r)
                .map(|(input, t)| {
                    (
                        input,
                        t.symbol(),
                        t.target().map(|t| t.0),
                        weight_bits(t.weight()),
                    )
                })
                .collect();
            if want != got {
                return Err(fail(format!(
                    "free arcs of state {}: source {want:?}, reader {got:?}",
                    s.0
                )));
            }
            for symbol in 0..n_symbols + 3 {
                let symbol = SymbolNumber(symbol as u16);
                checked += 1;
                let source_has = source.has_transitions(s.incr(), Some(symbol));
                let reader_has = reader.has_transitions(r.incr(), Some(symbol));
                let free = symbol.0 == 0 || a.is_flag(symbol);
                if free {
                    if reader_has {
                        return Err(fail(format!(
                            "reader answers yes for free symbol {} at state {q}",
                            symbol.0
                        )));
                    }
                    if source_has {
                        quirks += 1;
                    }
                    continue;
                }
                if source_has != reader_has {
                    return Err(fail(format!(
                        "state {} on symbol {}: source {source_has}, reader {reader_has}",
                        s.0, symbol.0
                    )));
                }
                if !source_has {
                    continue;
                }
                let want: Vec<_> = source
                    .transitions(s, symbol)
                    .map(|t| (t.symbol(), map(t.target()), weight_bits(t.weight())))
                    .collect();
                let got: Vec<_> = reader
                    .transitions(r, symbol)
                    .map(|t| (t.symbol(), t.target().map(|t| t.0), weight_bits(t.weight())))
                    .collect();
                if want != got || want.is_empty() {
                    return Err(fail(format!(
                        "state {} on symbol {}: source {want:?}, reader {got:?}",
                        s.0, symbol.0
                    )));
                }
            }
        }
        Ok((checked, quirks))
    };

    let results: Vec<Result<(u64, u64), WriteError>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                let start = (t * chunk).min(order.len());
                let end = ((t + 1) * chunk).min(order.len());
                let check = &check;
                scope.spawn(move || check(start..end))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err(fail("a check thread panicked".into())))
            })
            .collect()
    });
    let mut total = (0, 0);
    for r in results {
        let (c, q) = r?;
        total.0 += c;
        total.1 += q;
    }
    Ok(total)
}
