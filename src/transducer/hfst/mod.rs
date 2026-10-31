//! Finite-state automaton in HFST format.
//!
//! HFST (Helsinki Finite-State Technology) is a widely-used format for representing
//! finite-state transducers in morphological analysis and spell-checking. This module
//! provides support for reading and traversing HFST transducers.
pub mod alphabet;
pub mod header;
pub mod index_table;
#[cfg(test)]
pub(crate) mod test_support;
pub mod transition_table;

use std::borrow::Cow;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

#[cfg(feature = "fs")]
use memmap2::Mmap;

use self::alphabet::TransducerAlphabetParser;
use self::header::TransducerHeader;
use super::alphabet::TransducerAlphabet;
use super::symbol_transition::SymbolTransition;
use super::{Transducer, TransducerError, TransducerLoader};
use crate::constants::{INDEX_TABLE_SIZE, TARGET_TABLE, TRANS_TABLE_SIZE};
use crate::transducer::heuristic::BackwardDistance;
use crate::transducer::hfst::index_table::MappedIndexTable;
use crate::transducer::hfst::transition_table::MappedTransitionTable;
use crate::types::{HeaderFlag, SymbolNumber, TransitionTableIndex, Weight};
use crate::vfs::{self, Filesystem};

pub struct HfstTransducer {
    buf: Arc<vfs::Map>,
    header: TransducerHeader,
    alphabet: TransducerAlphabet,
    pub(crate) index_table: MappedIndexTable,
    pub(crate) transition_table: MappedTransitionTable,
    /// Backward shortest distances, computed on first use by the suggestion
    /// search and kept for the transducer's lifetime.
    distances: OnceLock<BackwardDistance>,
    /// The least transition weight, found on first use.
    least_weight: OnceLock<Option<Weight>>,
}

impl crate::transducer::heuristic::BackwardTables for HfstTransducer {
    fn index_len(&self) -> u32 {
        self.index_table.size.0
    }

    fn trans_len(&self) -> u32 {
        self.transition_table.size.0
    }

    fn index_input_symbol(&self, i: u32) -> Option<SymbolNumber> {
        self.index_table.input_symbol(TransitionTableIndex(i))
    }

    fn index_target(&self, i: u32) -> Option<TransitionTableIndex> {
        self.index_table.target(TransitionTableIndex(i))
    }

    fn index_final_weight(&self, i: u32) -> Option<Weight> {
        self.index_table.final_weight(TransitionTableIndex(i))
    }

    fn index_is_final(&self, i: u32) -> bool {
        self.index_table.is_final(TransitionTableIndex(i))
    }

    fn trans_input_symbol(&self, i: u32) -> Option<SymbolNumber> {
        self.transition_table.input_symbol(TransitionTableIndex(i))
    }

    fn trans_target(&self, i: u32) -> Option<TransitionTableIndex> {
        self.transition_table.target(TransitionTableIndex(i))
    }

    fn trans_weight(&self, i: u32) -> Option<Weight> {
        self.transition_table.weight(TransitionTableIndex(i))
    }

    fn trans_is_final(&self, i: u32) -> bool {
        self.transition_table.is_final(TransitionTableIndex(i))
    }

    fn is_flag_symbol(&self, symbol: SymbolNumber) -> bool {
        self.alphabet.is_flag(symbol)
    }
}

impl fmt::Debug for HfstTransducer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{:?}", self.header)?;
        writeln!(f, "{:?}", self.alphabet)?;
        writeln!(f, "{:?}", self.index_table)?;
        writeln!(f, "{:?}", self.transition_table)?;
        Ok(())
    }
}

impl HfstTransducer {
    /// Parse a HFST transducer out of a memory-mapped buffer.
    ///
    /// `path` is used only for error reporting — it should identify the file
    /// or archive member the buffer came from.
    ///
    /// Returns a [`TransducerError`] if the header is truncated, the alphabet
    /// is malformed, or the header-declared index/transition table sizes would
    /// read past the end of the buffer. Once this function succeeds, all
    /// subsequent per-state lookups are guaranteed to stay in bounds.
    #[cfg(feature = "fs")]
    pub fn from_mapped_memory(
        buf: Arc<Mmap>,
        path: impl Into<PathBuf>,
    ) -> Result<HfstTransducer, TransducerError> {
        HfstTransducer::from_map(buf, path)
    }

    /// Load a HFST transducer held in memory, from a copy of `bytes`: an
    /// anonymous mapping with the `fs` feature, bytes on the heap without it.
    pub fn from_bytes(
        bytes: &[u8],
        path: impl Into<PathBuf>,
    ) -> Result<HfstTransducer, TransducerError> {
        let path = path.into();
        if bytes.is_empty() {
            return Err(TransducerError::CorruptHeader { path, offset: 0 });
        }
        let map = vfs::copy_to_map(bytes).map_err(|source| TransducerError::Memmap {
            path: path.clone(),
            source,
        })?;
        HfstTransducer::from_map(Arc::new(map), path)
    }

    pub(crate) fn from_map(
        buf: Arc<vfs::Map>,
        path: impl Into<PathBuf>,
    ) -> Result<HfstTransducer, TransducerError> {
        let path = path.into();
        let header = TransducerHeader::parse(&buf, &path)?;
        let alphabet_offset = header.len();

        let alphabet_slice =
            buf.get(alphabet_offset..)
                .ok_or_else(|| TransducerError::CorruptHeader {
                    path: path.clone(),
                    offset: alphabet_offset,
                })?;

        let mut alphabet =
            TransducerAlphabetParser::parse(alphabet_slice, header.symbol_count(), &path)?;
        alphabet.input_symbol_count = Some(header.input_symbol_count());

        let index_table_offset = alphabet_offset + alphabet.len();
        let index_bytes = INDEX_TABLE_SIZE
            .checked_mul(header.index_table_size().0 as usize)
            .ok_or_else(|| TransducerError::CorruptTables {
                path: path.clone(),
                detail: Cow::Borrowed("index table size overflows usize"),
            })?;
        let index_table_end = index_table_offset.checked_add(index_bytes).ok_or_else(|| {
            TransducerError::CorruptTables {
                path: path.clone(),
                detail: Cow::Borrowed("index table extent overflows usize"),
            }
        })?;
        if index_table_end > buf.len() {
            return Err(TransducerError::CorruptTables {
                path,
                detail: Cow::Owned(format!(
                    "index table extends to offset {index_table_end}, but mapped buffer is only {} bytes",
                    buf.len()
                )),
            });
        }

        let weighted = header.has_flag(HeaderFlag::Weighted);
        let record = if weighted {
            TRANS_TABLE_SIZE
        } else {
            UNWEIGHTED_TRANS_TABLE_SIZE
        };
        let trans_bytes = record
            .checked_mul(header.target_table_size().0 as usize)
            .ok_or_else(|| TransducerError::CorruptTables {
                path: path.clone(),
                detail: Cow::Borrowed("transition table size overflows usize"),
            })?;
        let trans_table_end = index_table_end.checked_add(trans_bytes).ok_or_else(|| {
            TransducerError::CorruptTables {
                path: path.clone(),
                detail: Cow::Borrowed("transition table extent overflows usize"),
            }
        })?;
        if trans_table_end > buf.len() {
            return Err(TransducerError::CorruptTables {
                path,
                detail: Cow::Owned(format!(
                    "transition table extends to offset {trans_table_end}, but mapped buffer is only {} bytes",
                    buf.len()
                )),
            });
        }

        let buf = if weighted {
            buf
        } else {
            let tables = Tables {
                index: index_table_offset..index_table_end,
                transitions: index_table_end..trans_table_end,
            };
            let copy = vfs::copy_to_map(&with_weights(&buf, tables)).map_err(|source| {
                TransducerError::Memmap {
                    path: path.clone(),
                    source,
                }
            })?;
            Arc::new(copy)
        };

        let index_table = MappedIndexTable::new(
            buf.clone(),
            index_table_offset,
            index_table_end,
            header.index_table_size(),
        );

        let trans_table =
            MappedTransitionTable::new(buf.clone(), index_table_end, header.target_table_size());

        Ok(HfstTransducer {
            buf,
            header,
            alphabet,
            index_table,
            transition_table: trans_table,
            distances: OnceLock::new(),
            least_weight: OnceLock::new(),
        })
    }

    /// Where the run of `state`'s arcs on `input` starts in the transition
    /// table, which is where the cursor API's `has_transitions` and `next`
    /// lead. A state in the transition table has its arcs after its own
    /// record, and has the run when `opens` admits the first one's input; a
    /// state in the index table has it when its entry for `input` is its own.
    #[inline(always)]
    fn run_start(
        &self,
        state: TransitionTableIndex,
        input: SymbolNumber,
        opens: impl Fn(u16) -> bool,
    ) -> Option<TransitionTableIndex> {
        if state >= TARGET_TABLE {
            let first = state - TARGET_TABLE + TransitionTableIndex::ONE;
            let record = self.transition_table.record(first)?;
            opens(record.input).then_some(first)
        } else {
            let (symbol, target) = self
                .index_table
                .entry(state + TransitionTableIndex(input.0 as u32 + 1))?;
            (symbol == input.0 && target != u32::MAX)
                .then(|| TransitionTableIndex(target) - TARGET_TABLE)
        }
    }

    /// The bytes the tables are read from: the file's own, or for an
    /// unweighted file a copy of it whose transitions carry weights.
    #[inline(always)]
    pub fn buffer(&self) -> &[u8] {
        &self.buf
    }

    #[inline(always)]
    pub fn is_weighted(&self) -> bool {
        self.header.has_flag(HeaderFlag::Weighted)
    }

    #[inline(always)]
    pub fn header(&self) -> &TransducerHeader {
        &self.header
    }
}

/// Bytes of an unweighted transition record: input, output and target, the
/// weight being left out.
const UNWEIGHTED_TRANS_TABLE_SIZE: usize = 8;

/// Where the tables of an optimized-lookup file are.
struct Tables {
    index: std::ops::Range<usize>,
    transitions: std::ops::Range<usize>,
}

/// An unweighted optimized-lookup file `buf` laid out as a weighted one, every
/// weight zero, so that it is read by the weighted tables.
///
/// The header and alphabet are copied as they are. An unweighted index table
/// marks a final state with target 1 where a weighted one stores the final
/// weight; a transition record gains a zero weight, which is also the final
/// weight of a final state's record in the transition table.
fn with_weights(buf: &[u8], tables: Tables) -> Vec<u8> {
    let count = tables.transitions.len() / UNWEIGHTED_TRANS_TABLE_SIZE;
    let mut out = Vec::with_capacity(tables.transitions.start + count * TRANS_TABLE_SIZE);
    out.extend_from_slice(&buf[..tables.transitions.start]);
    for at in tables.index.step_by(INDEX_TABLE_SIZE) {
        let input = u16::from_le_bytes([out[at], out[at + 1]]);
        let target = &mut out[at + 2..at + INDEX_TABLE_SIZE];
        if input == u16::MAX && target != [0xFF; 4] {
            target.copy_from_slice(&0f32.to_le_bytes());
        }
    }
    let (records, _) = buf[tables.transitions].as_chunks::<UNWEIGHTED_TRANS_TABLE_SIZE>();
    for record in records {
        out.extend_from_slice(record);
        out.extend_from_slice(&0f32.to_le_bytes());
    }
    out
}

impl Transducer for HfstTransducer {
    const FILE_EXT: &'static str = "hfst";

    #[inline(always)]
    fn distance_to_final(&self, i: TransitionTableIndex) -> Weight {
        self.distances
            .get_or_init(|| BackwardDistance::compute(self))
            .get(i)
    }

    fn least_arc_weight(&self) -> Option<Weight> {
        *self
            .least_weight
            .get_or_init(|| crate::transducer::heuristic::least_arc_weight(self))
    }

    #[inline(always)]
    fn is_final(&self, i: TransitionTableIndex) -> bool {
        if i >= TARGET_TABLE {
            self.transition_table.is_final(i - TARGET_TABLE)
        } else {
            self.index_table.is_final(i)
        }
    }

    #[inline(always)]
    fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= TARGET_TABLE {
            self.transition_table.weight(i - TARGET_TABLE)
        } else {
            self.index_table.final_weight(i)
        }
    }

    #[inline(always)]
    fn has_transitions(&self, i: TransitionTableIndex, s: Option<SymbolNumber>) -> bool {
        let sym = match s {
            Some(v) => v,
            None => return false,
        };

        if i >= TARGET_TABLE {
            match self.transition_table.input_symbol(i - TARGET_TABLE) {
                Some(res) => sym == res,
                None => false,
            }
        } else {
            match self
                .index_table
                .input_symbol(i + TransitionTableIndex(sym.0 as u32))
            {
                Some(res) => sym == res,
                None => false,
            }
        }
    }

    #[inline(always)]
    fn has_epsilons_or_flags(&self, i: TransitionTableIndex) -> bool {
        if i >= TARGET_TABLE {
            match self.transition_table.input_symbol(i - TARGET_TABLE) {
                Some(sym) => sym == SymbolNumber::ZERO || self.alphabet.is_flag(sym),
                None => false,
            }
        } else if let Some(SymbolNumber::ZERO) = self.index_table.input_symbol(i) {
            true
        } else {
            false
        }
    }

    #[inline(always)]
    fn take_epsilons(&self, i: TransitionTableIndex) -> Option<SymbolTransition> {
        if let Some(SymbolNumber::ZERO) = self.transition_table.input_symbol(i) {
            Some(self.transition_table.symbol_transition(i))
        } else {
            None
        }
    }

    #[inline(always)]
    fn take_epsilons_and_flags(&self, i: TransitionTableIndex) -> Option<SymbolTransition> {
        if let Some(sym) = self.transition_table.input_symbol(i) {
            if sym != SymbolNumber::ZERO && !self.alphabet.is_flag(sym) {
                None
            } else {
                Some(self.transition_table.symbol_transition(i))
            }
        } else {
            None
        }
    }

    #[inline(always)]
    fn take_non_epsilons(
        &self,
        i: TransitionTableIndex,
        symbol: SymbolNumber,
    ) -> Option<SymbolTransition> {
        if let Some(input_sym) = self.transition_table.input_symbol(i) {
            if input_sym != symbol {
                None
            } else {
                Some(self.transition_table.symbol_transition(i))
            }
        } else {
            None
        }
    }

    #[inline(always)]
    fn next(&self, i: TransitionTableIndex, symbol: SymbolNumber) -> Option<TransitionTableIndex> {
        if i >= TARGET_TABLE {
            Some(i - TARGET_TABLE + TransitionTableIndex::ONE)
        } else if let Some(v) = self
            .index_table
            .target(i + TransitionTableIndex(symbol.0 as u32 + 1))
        {
            Some(v - TARGET_TABLE)
        } else {
            None
        }
    }

    #[inline(always)]
    fn transition_input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        self.transition_table.input_symbol(i)
    }

    /// The arcs the cursor walk finds, each record read whole.
    #[inline(always)]
    fn for_each_arc<V>(&self, state: TransitionTableIndex, input: SymbolNumber, mut visit: V)
    where
        V: FnMut(SymbolNumber, TransitionTableIndex, Weight),
    {
        let Some(mut at) = self.run_start(state, input, |symbol| symbol == input.0) else {
            return;
        };
        while let Some(record) = self.transition_table.record(at)
            && record.input == input.0
        {
            if record.output != u16::MAX && record.target != u32::MAX {
                visit(
                    SymbolNumber(record.output),
                    TransitionTableIndex(record.target),
                    Weight(record.weight),
                );
            }
            at = at.incr();
        }
    }

    /// The epsilon arcs and the flag diacritic arcs after them, in one run.
    #[inline(always)]
    fn for_each_free_arc<V>(&self, state: TransitionTableIndex, mut visit: V)
    where
        V: FnMut(SymbolNumber, SymbolNumber, TransitionTableIndex, Weight),
    {
        let free = |symbol: u16| symbol == 0 || self.alphabet.is_flag(SymbolNumber(symbol));
        let Some(mut at) = self.run_start(state, SymbolNumber::ZERO, free) else {
            return;
        };
        while let Some(record) = self.transition_table.record(at)
            && free(record.input)
        {
            let target = TransitionTableIndex(record.target);
            let weight = Weight(record.weight);
            if record.input != 0 && record.target != u32::MAX {
                let flag = SymbolNumber(record.input);
                visit(flag, flag, target, weight);
            } else if record.output != u16::MAX && record.target != u32::MAX {
                visit(
                    SymbolNumber::ZERO,
                    SymbolNumber(record.output),
                    target,
                    weight,
                );
            }
            at = at.incr();
        }
    }

    #[inline(always)]
    fn alphabet(&self) -> &TransducerAlphabet {
        &self.alphabet
    }

    #[inline(always)]
    fn alphabet_mut(&mut self) -> &mut TransducerAlphabet {
        &mut self.alphabet
    }
}

impl<F: vfs::File> TransducerLoader<F> for HfstTransducer {
    fn from_path<P, FS>(fs: &FS, path: P) -> Result<HfstTransducer, TransducerError>
    where
        P: AsRef<Path>,
        FS: Filesystem<File = F>,
    {
        let path = path.as_ref();
        let file = fs.open_file(path).map_err(|source| TransducerError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        #[cfg(feature = "fs")]
        let map = unsafe { file.memory_map() };
        #[cfg(not(feature = "fs"))]
        let map = file.contents();
        let map = map.map_err(|source| TransducerError::Memmap {
            path: path.to_path_buf(),
            source,
        })?;
        HfstTransducer::from_map(Arc::new(map), path.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{OlState, errmodel, lexicon, ol_bytes_with};
    use super::*;

    type Arc = (u16, u16, u32, u32);

    /// The arcs from `state` on `input` that the cursor API leads to, walked
    /// as the trait's default `for_each_arc` walks them.
    fn cursor_arcs(
        t: &HfstTransducer,
        state: TransitionTableIndex,
        input: SymbolNumber,
    ) -> Vec<Arc> {
        let mut arcs = Vec::new();
        if !t.has_transitions(state.incr(), Some(input)) {
            return arcs;
        }
        let Some(mut next) = t.next(state, input) else {
            return arcs;
        };
        loop {
            let transition = if input == SymbolNumber::ZERO {
                t.take_epsilons(next)
            } else {
                t.take_non_epsilons(next, input)
            };
            let Some(transition) = transition else {
                break;
            };
            if let (Some(output), Some(target), Some(weight)) = (
                transition.symbol(),
                transition.target(),
                transition.weight(),
            ) {
                arcs.push((input.0, output.0, target.0, weight.0.to_bits()));
            }
            next = next.incr();
        }
        arcs
    }

    /// The free arcs from `state` as the trait's default `for_each_free_arc`
    /// finds them: the cursor walk on epsilon, then the flag arcs of
    /// `free_arcs`.
    fn cursor_free_arcs(t: &HfstTransducer, state: TransitionTableIndex) -> Vec<Arc> {
        let mut arcs = cursor_arcs(t, state, SymbolNumber::ZERO);
        for (input, transition) in t.free_arcs(state) {
            if input == SymbolNumber::ZERO {
                continue;
            }
            if let (Some(target), Some(weight)) = (transition.target(), transition.weight()) {
                arcs.push((input.0, input.0, target.0, weight.0.to_bits()));
            }
        }
        arcs
    }

    /// Epsilon arcs, flag diacritic arcs before and after them, a state whose
    /// only free arcs are flags, and arcs on several inputs.
    fn flags() -> Vec<u8> {
        let symbols = ["@_EPSILON_SYMBOL_@", "@P.F.A@", "@R.F.A@", "a", "b"];
        let states: Vec<OlState> = vec![
            (
                None,
                vec![
                    (1, 1, 1, 0.5),
                    (0, 3, 2, 0.25),
                    (3, 3, 3, 1.0),
                    (3, 4, 2, 2.0),
                ],
            ),
            (None, vec![(2, 2, 3, 0.0), (4, 4, 0, 0.0)]),
            (
                Some(0.0),
                vec![(0, 0, 0, 0.0), (0, 4, 3, 1.5), (2, 2, 1, 0.0)],
            ),
            (Some(1.0), vec![]),
        ];
        ol_bytes_with(&symbols, &states, true)
    }

    /// Every place a walk can start, as a state: each index table entry and
    /// each transition record, whether or not a state is there, so that the
    /// two walks are compared on every path through their code.
    fn places(t: &HfstTransducer) -> impl Iterator<Item = TransitionTableIndex> {
        let index = (0..t.index_table.size.0 + 2).map(TransitionTableIndex);
        let transitions =
            (0..t.transition_table.size.0 + 2).map(|i| TARGET_TABLE + TransitionTableIndex(i));
        index.chain(transitions)
    }

    #[test]
    fn records_read_whole_find_the_arcs_the_cursor_walk_finds() {
        for (name, bytes) in [
            ("flags", flags()),
            ("lexicon", lexicon()),
            ("errmodel", errmodel()),
        ] {
            let t = HfstTransducer::from_bytes(&bytes, name).expect("the fixture loads");
            let symbols = t.alphabet().len() as u16;
            for state in places(&t) {
                for input in (0..symbols + 2).map(SymbolNumber) {
                    let mut arcs = Vec::new();
                    t.for_each_arc(state, input, |output, target, weight| {
                        arcs.push((input.0, output.0, target.0, weight.0.to_bits()));
                    });
                    assert_eq!(
                        arcs,
                        cursor_arcs(&t, state, input),
                        "{name}: {state:?} on {input:?}"
                    );
                }
                let mut free = Vec::new();
                t.for_each_free_arc(state, |input, output, target, weight| {
                    free.push((input.0, output.0, target.0, weight.0.to_bits()));
                });
                assert_eq!(
                    free,
                    cursor_free_arcs(&t, state),
                    "{name}: free arcs of {state:?}"
                );
            }
        }
    }
}
