//! Acceptors (lexicons) in the DHFST format, type 2.
//!
//! The suggestion search asks a lexicon "does this state have an arc on this
//! symbol" about 200 million times per 1000 words, and the answer is almost
//! always no. HFST optimized lookup answers it with one load from a table
//! indexed by state plus symbol, whose entries say which symbol they belong
//! to. This format keeps that one load and one compare, for hits and misses
//! alike, and spends as few bytes on it as the lexicon allows. The symbol
//! checks are a column of bytes of their own, so that the probes of one state
//! read a few consecutive cache lines; a lexicon whose labels do not fit in a
//! byte has a second column with the high bytes, read only when the low byte
//! matches. The arcs sit in a parallel column of records of a few bytes, a
//! target and a weight's index, and the epsilon and flag diacritic arcs are
//! found through the state's own slot.
//!
//! # What a state means
//!
//! A state is a number `q` below `n_ids`, the start state 0; its arcs are
//! found at slots `q + x`. States are numbered so that no two share a number
//! and no two hold the same slot. Slot `q + s`, for a regular symbol `s` (not
//! epsilon, not a flag diacritic), holds the arcs of `q` on `s` when its
//! check is `s`. Slot `q` holds the free arcs of `q` (input epsilon or a flag
//! diacritic) when its check is 0, as epsilon's symbol is, and its record is
//! not zero. As no two states share a number, a slot whose check is `s`
//! belongs to the state `s` below it and to no other. Every arc's output is
//! its input.
//!
//! # Layout (type 2, version 1)
//!
//! Every multi-byte field is little-endian.
//!
//! ```text
//! offset  size  field
//! 0       5     "DHFST"
//! 5       1     type = 2
//! 6       1     version = 1
//! 7       1     reserved, zero
//! 8       4     flags: bit 0 tropical f32 weights (required)
//! 12      4     number of sections
//! 16      8     reserved, zero
//! 24      24*n  section table: tag[4], flags u32 (zero), offset u64, length u64
//! ...           sections, each at a multiple of 8, zero-padded to one
//! ```
//!
//! A section whose tag starts with an upper-case letter is critical: a reader
//! that does not know it refuses the file. A lower-case tag is ancillary and
//! is skipped. Each array below is followed by at least 8 zero bytes inside
//! its section, so that a reader may load 8 bytes at any element. A *record*
//! is an unsigned integer of `record_bytes` bytes: its low `target_bits` bits
//! are its *first* field, the next `index_bits` bits its *second*, and any
//! bits above them zero. `I = 2^index_bits - 1`.
//!
//! * `SYMS` (critical): `u32 n; u32 offsets[n + 1]; u8 names[]`, UTF-8, as in
//!   type 1. Symbol 0 is `@_EPSILON_SYMBOL_@`.
//! * `CHCK` (critical): `u32 n_slots; u32 n_ids; u16 label_max; u8
//!   check_bytes; u8 0[5]`, then the low byte of every slot's check followed
//!   by 8 zero bytes and, when `check_bytes` is 2, the high byte of every
//!   slot's check followed by 8 zero bytes. `label_max` is the greatest
//!   regular label, below `2^(8 check_bytes)`; `n_slots >= n_ids +
//!   label_max`.
//! * `SLOT` (critical): `u32 n_slots; u8 record_bytes; u8 target_bits; u8
//!   index_bits; u8 0`, then one record per slot.
//! * `LIST` (critical): `u32 n; u8 record_bytes; u8 0[3]`, then `n` records.
//! * `FINL` (critical): `u32 n; u32 0`, then for every 64 state numbers, from
//!   `64k`, a 12-byte entry `u64 final; u32 before`, then `f32 weights[n]`.
//!   Bit `i` of `final` is set when state `64k + i` is final, and `before`
//!   counts the bits set in all earlier entries; bits past `n_ids` are zero.
//!   The weights are the final states' final weights, in the order of their
//!   numbers.
//! * `FREE` (critical): `u32 n; u32 0; { u16 symbol; u16 0; f32 weight }[n]`,
//!   the distinct symbol and weight pairs of the free arcs.
//! * `WGHT` (critical): `u32 n; u32 0; f32 weights[n]`, the regular arcs'
//!   weights (zero included), most frequent first; at most `I` of them.
//! * `DIST` (critical, optional): `u32 n_ids; u32 0; f32 distances[n_ids]`.
//!   For every state number, a lower bound on the weight of any path from
//!   that state to a final state, final weight included, `+inf` when there is
//!   none: what the search's lookahead adds to a path's weight to order its
//!   queue. Without `DIST` every state's distance is 0.
//! * `meta` (ancillary): UTF-8 JSON describing how the file was written.
//!
//! What slot `p` holds, by its check `c` and its record:
//!
//! * `c = 0`, record zero: nothing.
//! * `c = 0`, record not zero: the free arcs of state `p`, `p < n_ids`. The
//!   first field is their position in `LIST` and the second their number (at
//!   least one); each listed record holds a target first and an index into
//!   `FREE` second, in the source's order.
//! * `1 <= c <= label_max`: the arcs on `c` of the state `p - c`. If the
//!   record's second field is below `I`, one arc: target the first field,
//!   weight `WGHT[second]`. If it is `I`, the first field is a position `k` in
//!   `LIST`: `LIST[k]`'s first field is the number of arcs `n >= 2` and its
//!   second is zero, and `LIST[k + 1 ..= k + n]` are the arcs, each target
//!   first and weight index second, in the source's order.
//!
//! # Lookups
//!
//! * Does state `q` have an arc on symbol `s`? Refuse `s` outside `1 ..=
//!   label_max` and `q` past the last state number, then compare the low byte
//!   of the check of slot `q + s` with `s`'s (and, for two-byte checks, the
//!   high byte when that matches). That is one load and one compare, hit or
//!   miss.
//! * Its arcs: the record of the same slot, or the list it names.
//! * Free arcs: the check of slot `q` is 0, and its record names them.
//! * Is `q` final? Bit `q mod 64` of `FINL` entry `q / 64`: one load. Its
//!   final weight is `weights[before + popcount(final below q)]`.
//! * Its distance to a final state: `distances[q]`, or 0 without `DIST`.
//!
//! Loading validates every section, check, record, list, count, weight and
//! distance, so that no lookup after a successful load can read outside the
//! file. The lookups then read the mapped file in place through `word()`, the
//! one unchecked load, and nothing on the heap grows with the lexicon.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use memmap2::Mmap;

use super::{DhfstType, PREFIX_LEN, u16_at, u32_at, u64_at};
use crate::transducer::alphabet::TransducerAlphabet;
use crate::transducer::hfst::alphabet::TransducerAlphabetParser;
use crate::transducer::symbol_transition::SymbolTransition;
use crate::transducer::{Transducer, TransducerError, TransducerFormat, TransducerLoader};
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
use crate::vfs::{self, Filesystem};

/// Bytes before the section table.
pub const HEADER_LEN: usize = 24;
/// Bytes per section table entry.
pub const SECTION_ENTRY_LEN: usize = 24;
/// Zero bytes after every array, so that 8 bytes may be loaded at any element.
pub const PADDING: usize = 8;
/// State numbers per `FINL` entry.
pub const FINL_STATES: usize = 64;
/// Bytes per `FINL` entry.
pub const FINL_ENTRY_LEN: usize = 12;
/// Bytes of the `CHCK` head.
pub const CHCK_HEAD_LEN: usize = 16;
/// Bytes of the `SLOT` and `LIST` heads.
pub const RECORD_HEAD_LEN: usize = 8;

/// Header flag: weights are tropical `f32`.
pub const FLAG_TROPICAL: u32 = 1 << 0;
const KNOWN_FLAGS: u32 = FLAG_TROPICAL;

/// Section tags.
pub mod tag {
    /// symbol table
    pub const SYMS: [u8; 4] = *b"SYMS";
    /// the check of every slot
    pub const CHCK: [u8; 4] = *b"CHCK";
    /// the record of every slot
    pub const SLOT: [u8; 4] = *b"SLOT";
    /// arcs that do not fit in their slot
    pub const LIST: [u8; 4] = *b"LIST";
    /// final states
    pub const FINL: [u8; 4] = *b"FINL";
    /// free arc symbols and weights
    pub const FREE: [u8; 4] = *b"FREE";
    /// regular arc weights
    pub const WGHT: [u8; 4] = *b"WGHT";
    /// every state's distance to a final state
    pub const DIST: [u8; 4] = *b"DIST";
    /// writer metadata
    pub const META: [u8; 4] = *b"meta";
}

/// The 8 bytes at `at`, which the load-time validation put inside the file.
#[inline(always)]
fn word(b: &[u8], at: usize) -> u64 {
    debug_assert!(at + 8 <= b.len());
    // SAFETY: every offset the lookups pass was checked against the file's
    // length when it loaded.
    u64::from_le_bytes(unsafe { *(b.as_ptr().add(at) as *const [u8; 8]) })
}

#[inline(always)]
fn byte_mask(bytes: usize) -> u64 {
    if bytes >= 8 {
        u64::MAX
    } else {
        (1u64 << (8 * bytes)) - 1
    }
}

/// Where the sections of a validated file are, and how wide its fields.
#[derive(Clone, Copy, Debug)]
struct Layout {
    /// The greatest regular label.
    label_max: u32,
    n_ids: u32,
    /// The low byte of the check of slot `k` is at `check_lo + k + 1`.
    check_lo: usize,
    /// Whether the checks have a high byte, at `check_hi + k + 1`.
    wide: bool,
    check_hi: usize,
    records: usize,
    list: usize,
    record_bytes: usize,
    record_mask: u64,
    target_bits: u32,
    target_mask: u64,
    index_mask: u64,
    n_slots: u32,
    n_list: u32,
    finals: usize,
    final_weights: usize,
    n_finals: u32,
    free: usize,
    n_free: u32,
    weights: usize,
    n_weights: u32,
    n_arcs: u64,
    n_free_arcs: u64,
    used_slots: u32,
    flags: u32,
    version: u8,
    /// The distances, one per state number; 0 of them without `DIST`.
    distances: usize,
    n_distances: u32,
}

/// A validated DHFST acceptor, read in place from a memory map.
pub struct DhfstAcceptor {
    buf: Mmap,
    layout: Layout,
    alphabet: TransducerAlphabet,
    symbol_names: Vec<String>,
    meta: Option<String>,
    least_weight: Option<Weight>,
}

impl std::fmt::Debug for DhfstAcceptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DhfstAcceptor")
            .field("bytes", &self.buf.len())
            .field("ids", &self.layout.n_ids)
            .field("slots", &self.layout.n_slots)
            .field("arcs", &self.layout.n_arcs)
            .finish()
    }
}

/// How a file stores its arcs, for the stats tools.
#[derive(Clone, Copy, Debug)]
pub struct AcceptorInfo {
    /// state numbers
    pub ids: u32,
    /// slots in the table
    pub slots: u32,
    /// slots that hold arcs, free or regular
    pub used_slots: u32,
    /// bytes per check
    pub check_bytes: usize,
    /// the greatest regular label
    pub label_max: u32,
    /// bytes per record
    pub record_bytes: usize,
    /// bits of a record's first field
    pub target_bits: u32,
    /// bits of a record's second field
    pub index_bits: u32,
    /// records in `LIST`
    pub list_records: u32,
    /// arcs, free and regular
    pub arcs: u64,
    /// free arcs
    pub free_arcs: u64,
    /// distinct symbol and weight pairs of the free arcs
    pub free_pairs: u32,
    /// distinct regular arc weights
    pub weights: u32,
    /// final states
    pub finals: u32,
    /// whether the file stores distances to a final state
    pub distances: bool,
}

/// The free arcs of one state, as [`Transducer::free_arcs`] hands them out.
pub struct FreeArcs<'a> {
    acceptor: &'a DhfstAcceptor,
    at: usize,
    end: usize,
}

impl Iterator for FreeArcs<'_> {
    type Item = (SymbolNumber, SymbolTransition);

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.at >= self.end {
            return None;
        }
        let t = self.acceptor;
        let l = &t.layout;
        let record = t.record(self.at);
        self.at += l.record_bytes;
        let (symbol, weight) = t.free_pair(((record >> l.target_bits) & l.index_mask) as usize);
        Some((
            symbol,
            SymbolTransition::new(
                Some(TransitionTableIndex((record & l.target_mask) as u32)),
                Some(symbol),
                Some(Weight(weight)),
            ),
        ))
    }
}

/// The regular arcs of one state on one symbol, as
/// [`Transducer::transitions`] hands them out.
pub struct Transitions<'a> {
    acceptor: &'a DhfstAcceptor,
    symbol: SymbolNumber,
    at: usize,
    end: usize,
}

impl Iterator for Transitions<'_> {
    type Item = SymbolTransition;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.at >= self.end {
            return None;
        }
        let t = self.acceptor;
        let l = &t.layout;
        let record = t.record(self.at);
        self.at += l.record_bytes;
        let weight = t.weight(((record >> l.target_bits) & l.index_mask) as usize);
        Some(SymbolTransition::new(
            Some(TransitionTableIndex((record & l.target_mask) as u32)),
            Some(self.symbol),
            Some(Weight(weight)),
        ))
    }
}

impl DhfstAcceptor {
    /// Parse and validate an acceptor out of a memory map.
    ///
    /// `path` is used only for error reporting. Every section, check, record
    /// and list is checked here, so once this succeeds no later lookup can
    /// read outside the buffer.
    pub fn from_mmap(
        buf: Mmap,
        path: impl Into<PathBuf>,
    ) -> Result<DhfstAcceptor, TransducerError> {
        let path = path.into();
        let parsed = parse(&buf, &path)?;
        Ok(DhfstAcceptor {
            buf,
            layout: parsed.layout,
            alphabet: parsed.alphabet,
            symbol_names: parsed.names,
            meta: parsed.meta,
            least_weight: parsed.least_weight,
        })
    }

    /// Load an acceptor held in memory, by copying it into an anonymous
    /// mapping.
    pub fn from_bytes(
        bytes: &[u8],
        path: impl Into<PathBuf>,
    ) -> Result<DhfstAcceptor, TransducerError> {
        let path = path.into();
        if bytes.is_empty() {
            return Err(TransducerError::CorruptHeader { path, offset: 0 });
        }
        let mut map =
            memmap2::MmapMut::map_anon(bytes.len()).map_err(|source| TransducerError::Memmap {
                path: path.clone(),
                source,
            })?;
        map.copy_from_slice(bytes);
        let map = map
            .make_read_only()
            .map_err(|source| TransducerError::Memmap {
                path: path.clone(),
                source,
            })?;
        DhfstAcceptor::from_mmap(map, path)
    }

    /// The raw bytes of the file.
    pub fn buffer(&self) -> &[u8] {
        &self.buf
    }

    /// The number of state numbers: every state is numbered below it.
    pub fn id_count(&self) -> u32 {
        self.layout.n_ids
    }

    /// The header flags.
    pub fn flags(&self) -> u32 {
        self.layout.flags
    }

    /// The format version the file declares.
    pub fn version(&self) -> u8 {
        self.layout.version
    }

    /// Symbol names as stored, `@_EPSILON_SYMBOL_@` first.
    pub fn symbol_names(&self) -> &[String] {
        &self.symbol_names
    }

    /// The writer's `meta` section, if the file carries one.
    pub fn meta(&self) -> Option<&str> {
        self.meta.as_deref()
    }

    /// How the file stores its arcs, for the stats tools.
    pub fn info(&self) -> AcceptorInfo {
        let l = &self.layout;
        AcceptorInfo {
            ids: l.n_ids,
            slots: l.n_slots,
            used_slots: l.used_slots,
            check_bytes: 1 + l.wide as usize,
            label_max: l.label_max,
            record_bytes: l.record_bytes,
            target_bits: l.target_bits,
            index_bits: l.index_mask.count_ones(),
            list_records: l.n_list,
            arcs: l.n_arcs,
            free_arcs: l.n_free_arcs,
            free_pairs: l.n_free,
            weights: l.n_weights,
            finals: l.n_finals,
            distances: l.n_distances > 0,
        }
    }

    /// Bytes per section, by tag, for the stats tools.
    pub fn section_sizes(&self) -> Vec<(String, u64)> {
        let b: &[u8] = &self.buf;
        let n = u32_at(b, 12) as usize;
        (0..n)
            .map(|s| {
                let at = HEADER_LEN + SECTION_ENTRY_LEN * s;
                (
                    String::from_utf8_lossy(&b[at..at + 4]).into_owned(),
                    u64_at(b, at + 16),
                )
            })
            .collect()
    }

    /// The check of slot `at - 1`, `at >= 1`.
    #[inline(always)]
    fn check_before(&self, at: usize) -> u32 {
        let l = &self.layout;
        let low = word(&self.buf, l.check_lo + at) as u8 as u32;
        if l.wide {
            low | (word(&self.buf, l.check_hi + at) as u8 as u32) << 8
        } else {
            low
        }
    }

    /// Whether the check of slot `at - 1`, `at >= 1`, is `s`: its low byte
    /// first, and the high byte only when that matches.
    #[inline(always)]
    fn checks_as(&self, at: usize, s: u32) -> bool {
        let l = &self.layout;
        word(&self.buf, l.check_lo + at) as u8 == s as u8
            && (!l.wide || word(&self.buf, l.check_hi + at) as u8 == (s >> 8) as u8)
    }

    /// The record at byte `at`.
    #[inline(always)]
    fn record(&self, at: usize) -> u64 {
        word(&self.buf, at) & self.layout.record_mask
    }

    /// The slot that holds the arcs on `symbol` of the state `i - 1`, as the
    /// cursor API numbers it, if it has any. Symbol 0, the check of free and
    /// empty slots, wraps around and is refused with the symbols no label
    /// holds, before the table is read. `i = 0` reads below the slots of the
    /// state numbers, where no check can be `s` (its state would be `-1`),
    /// and needs no test of its own.
    #[inline(always)]
    fn slot(&self, i: u32, symbol: SymbolNumber) -> Option<usize> {
        let l = &self.layout;
        let s = symbol.0 as u32;
        if s.wrapping_sub(1) >= l.label_max || i > l.n_ids {
            return None;
        }
        let at = i as usize + s as usize;
        self.checks_as(at, s).then_some(at - 1)
    }

    /// The records of the arcs of state `q` on `symbol`, as the bytes `at ..
    /// end`.
    #[inline(always)]
    fn arcs(&self, q: u32, symbol: SymbolNumber) -> (usize, usize) {
        let Some(slot) = q.checked_add(1).and_then(|i| self.slot(i, symbol)) else {
            return (0, 0);
        };
        let l = &self.layout;
        let at = l.records + l.record_bytes * slot;
        let record = self.record(at);
        if (record >> l.target_bits) & l.index_mask != l.index_mask {
            return (at, at + l.record_bytes);
        }
        let head = l.list + l.record_bytes * (record & l.target_mask) as usize;
        let n = (self.record(head) & l.target_mask) as usize;
        (head + l.record_bytes, head + l.record_bytes * (n + 1))
    }

    /// The records of the free arcs of state `q`, as the bytes `at .. end`:
    /// none when its slot is empty or holds another state's arcs.
    #[inline(always)]
    fn free_run(&self, q: u32) -> (usize, usize) {
        let l = &self.layout;
        if q >= l.n_ids || self.check_before(q as usize + 1) != 0 {
            return (0, 0);
        }
        let record = self.record(l.records + l.record_bytes * q as usize);
        let at = l.list + l.record_bytes * (record & l.target_mask) as usize;
        let n = ((record >> l.target_bits) & l.index_mask) as usize;
        (at, at + l.record_bytes * n)
    }

    /// The symbol and weight of the free pair `pair`.
    #[inline(always)]
    fn free_pair(&self, pair: usize) -> (SymbolNumber, f32) {
        let entry = word(&self.buf, self.layout.free + 8 * pair);
        (
            SymbolNumber(entry as u16),
            f32::from_bits((entry >> 32) as u32),
        )
    }

    /// The regular arc weight `index`.
    #[inline(always)]
    fn weight(&self, index: usize) -> f32 {
        f32::from_bits(word(&self.buf, self.layout.weights + 4 * index) as u32)
    }

    /// The `FINL` entry of state `q` and `q`'s bit in it, if `q` is a state
    /// number.
    #[inline(always)]
    fn final_entry(&self, q: u32) -> Option<(usize, u64, u32)> {
        let l = &self.layout;
        if q >= l.n_ids {
            return None;
        }
        let entry = l.finals + FINL_ENTRY_LEN * (q as usize / FINL_STATES);
        Some((entry, word(&self.buf, entry), q % FINL_STATES as u32))
    }

    /// The final weight of state `q`, if it is final.
    #[inline(always)]
    fn final_of(&self, q: u32) -> Option<f32> {
        let (entry, bits, bit) = self.final_entry(q)?;
        if (bits >> bit) & 1 == 0 {
            return None;
        }
        let b: &[u8] = &self.buf;
        let rank = word(b, entry + 8) as u32 + (bits & ((1u64 << bit) - 1)).count_ones();
        Some(f32::from_bits(
            word(b, self.layout.final_weights + 4 * rank as usize) as u32,
        ))
    }
}

/// A file parsed and validated, before it becomes an acceptor.
struct Parsed {
    layout: Layout,
    alphabet: TransducerAlphabet,
    names: Vec<String>,
    meta: Option<String>,
    least_weight: Option<Weight>,
}

fn corrupt(path: &Path, detail: impl Into<Cow<'static, str>>) -> TransducerError {
    TransducerError::CorruptTables {
        path: path.to_path_buf(),
        detail: detail.into(),
    }
}

/// A section's extent in the file, `start .. end`.
type Extent = (usize, usize);

/// The header and section table of an acceptor file, checked.
struct Sections {
    /// the format version the header declares
    version: u8,
    /// the header flags
    flags: u32,
    list: Vec<([u8; 4], usize, usize)>,
}

impl Sections {
    /// The extent of the section `wanted`, if the file has one.
    fn find(&self, wanted: [u8; 4]) -> Option<Extent> {
        self.list
            .iter()
            .find(|(t, _, _)| *t == wanted)
            .map(|(_, start, end)| (*start, *end))
    }

    /// The extent of the section `wanted`, which the file must have.
    fn need(&self, wanted: [u8; 4], path: &Path) -> Result<Extent, TransducerError> {
        self.find(wanted).ok_or_else(|| {
            corrupt(
                path,
                format!(
                    "required section {} is missing",
                    String::from_utf8_lossy(&wanted)
                ),
            )
        })
    }
}

/// Check the header of an acceptor file and its section table, whose
/// critical sections must all be among `known`.
fn sections(b: &[u8], path: &Path, known: &[[u8; 4]]) -> Result<Sections, TransducerError> {
    let wanted = DhfstType::Acceptor;
    let version = match TransducerFormat::detect(b, path)? {
        TransducerFormat::Dhfst { kind, version } if kind == wanted => version,
        TransducerFormat::Dhfst { kind, .. } => {
            return Err(TransducerError::WrongDhfstType {
                path: path.to_path_buf(),
                found: kind,
                wanted,
            });
        }
        TransducerFormat::Hfst => {
            return Err(TransducerError::UnrecognisedFormat {
                path: path.to_path_buf(),
                detail: Cow::Borrowed("the file is HFST optimized lookup, not DHFST"),
            });
        }
    };
    if b.len() < HEADER_LEN {
        return Err(TransducerError::CorruptHeader {
            path: path.to_path_buf(),
            offset: b.len(),
        });
    }
    let flags = u32_at(b, 8);
    let n_sections = u32_at(b, 12) as usize;
    if u64_at(b, 16) != 0 {
        return Err(TransducerError::CorruptHeader {
            path: path.to_path_buf(),
            offset: 16,
        });
    }
    if flags & !KNOWN_FLAGS != 0 {
        return Err(corrupt(path, format!("unknown header flags {flags:#x}")));
    }
    if flags & FLAG_TROPICAL == 0 {
        return Err(corrupt(path, "weights are not declared tropical f32"));
    }

    let table_end = n_sections
        .checked_mul(SECTION_ENTRY_LEN)
        .and_then(|n| n.checked_add(HEADER_LEN))
        .filter(|end| *end <= b.len())
        .ok_or_else(|| corrupt(path, "section table runs past the end of the file"))?;
    let mut sections: Vec<([u8; 4], usize, usize)> = Vec::with_capacity(n_sections);
    for s in 0..n_sections {
        let at = HEADER_LEN + SECTION_ENTRY_LEN * s;
        let tag = [b[at], b[at + 1], b[at + 2], b[at + 3]];
        let offset = u64_at(b, at + 8);
        let length = u64_at(b, at + 16);
        let end = offset.checked_add(length);
        let (Ok(offset), Some(Ok(end))) = (usize::try_from(offset), end.map(usize::try_from))
        else {
            return Err(corrupt(path, "section extent overflows"));
        };
        if offset % 8 != 0 || offset < table_end || end > b.len() || u32_at(b, at + 4) != 0 {
            return Err(corrupt(
                path,
                format!(
                    "section {} at {offset}..{end} is misplaced in a {} byte file",
                    String::from_utf8_lossy(&tag),
                    b.len()
                ),
            ));
        }
        if sections.iter().any(|(t, _, _)| *t == tag) {
            return Err(corrupt(
                path,
                format!("section {} appears twice", String::from_utf8_lossy(&tag)),
            ));
        }
        if tag[0].is_ascii_uppercase() && !known.contains(&tag) {
            return Err(corrupt(
                path,
                format!(
                    "critical section {} is not known to this reader",
                    String::from_utf8_lossy(&tag)
                ),
            ));
        }
        sections.push((tag, offset, end));
    }
    Ok(Sections {
        version,
        flags,
        list: sections,
    })
}

/// Check a `SYMS` section: the symbol names, and the alphabet they make.
fn symbols(
    b: &[u8],
    (syms, syms_end): Extent,
    path: &Path,
) -> Result<(Vec<String>, TransducerAlphabet), TransducerError> {
    if syms_end - syms < 4 {
        return Err(corrupt(path, "SYMS is truncated"));
    }
    let n_symbols = u32_at(b, syms);
    if n_symbols == 0 || n_symbols > u16::MAX as u32 {
        return Err(corrupt(
            path,
            format!("{n_symbols} symbols is out of range"),
        ));
    }
    let blob = syms + 4 + 4 * (n_symbols as usize + 1);
    if blob > syms_end {
        return Err(corrupt(path, "SYMS offsets run past the section"));
    }
    let mut names = Vec::with_capacity(n_symbols as usize);
    let mut previous = 0usize;
    for s in 0..n_symbols as usize {
        let start = u32_at(b, syms + 4 + 4 * s) as usize;
        let end = u32_at(b, syms + 8 + 4 * s) as usize;
        if start != previous || end < start || blob + end > syms_end {
            return Err(corrupt(
                path,
                format!("SYMS offsets of symbol {s} are invalid"),
            ));
        }
        previous = end;
        let name = std::str::from_utf8(&b[blob + start..blob + end])
            .map_err(|_| corrupt(path, format!("symbol {s} is not UTF-8")))?;
        if name.contains('\0') {
            return Err(corrupt(path, format!("symbol {s} contains NUL")));
        }
        names.push(name.to_string());
    }
    if names[0] != "@_EPSILON_SYMBOL_@" {
        return Err(corrupt(path, "symbol 0 is not @_EPSILON_SYMBOL_@"));
    }
    let alphabet = {
        let mut buf = Vec::new();
        for name in &names {
            buf.extend_from_slice(name.as_bytes());
            buf.push(0);
        }
        TransducerAlphabetParser::parse(&buf, SymbolNumber(n_symbols as u16), path)?
    };
    Ok((names, alphabet))
}

/// Check a `FINL` section over `n_ids` state numbers: where its entries
/// start, where its weights start, and how many final states it marks.
fn finals(
    b: &[u8],
    (finl, finl_end): Extent,
    n_ids: u32,
    path: &Path,
) -> Result<(usize, usize, u32), TransducerError> {
    if finl_end - finl < 8 || u32_at(b, finl + 4) != 0 {
        return Err(corrupt(path, "FINL is truncated"));
    }
    let n_finals = u32_at(b, finl);
    let finals = finl + 8;
    let final_entries = (n_ids as usize).div_ceil(FINL_STATES);
    let final_weights = finals + FINL_ENTRY_LEN * final_entries;
    if (n_finals as usize)
        .checked_mul(4)
        .and_then(|len| len.checked_add(final_weights + PADDING))
        .is_none_or(|e| e > finl_end)
    {
        return Err(corrupt(path, "FINL runs past its section"));
    }
    let mut final_total = 0u32;
    for e in 0..final_entries {
        let at = finals + FINL_ENTRY_LEN * e;
        let bits = u64_at(b, at);
        let valid = (n_ids as usize - e * FINL_STATES).min(FINL_STATES);
        if u32_at(b, at + 8) != final_total || (valid < 64 && bits >> valid != 0) {
            return Err(corrupt(path, format!("FINL entry {e} miscounts")));
        }
        final_total += bits.count_ones();
    }
    if final_total != n_finals {
        return Err(corrupt(
            path,
            format!("FINL marks {final_total} final states and holds {n_finals} weights"),
        ));
    }
    for k in 0..n_finals as usize {
        let w = f32::from_bits(u32_at(b, final_weights + 4 * k));
        if !w.is_finite() {
            return Err(corrupt(path, format!("final weight {k} is {w}")));
        }
    }
    Ok((finals, final_weights, n_finals))
}

/// Check a `FREE` section: where its pairs start and how many there are.
fn free_pairs(
    b: &[u8],
    (free, free_end): Extent,
    n_symbols: u32,
    is_free: impl Fn(u16) -> bool,
    path: &Path,
) -> Result<(usize, u32), TransducerError> {
    if free_end - free < 8 || u32_at(b, free + 4) != 0 {
        return Err(corrupt(path, "FREE is truncated"));
    }
    let n_free = u32_at(b, free);
    let free = free + 8;
    if (n_free as usize)
        .checked_mul(8)
        .and_then(|len| len.checked_add(free + PADDING))
        .is_none_or(|e| e > free_end)
    {
        return Err(corrupt(path, "FREE runs past its section"));
    }
    for k in 0..n_free as usize {
        let symbol = u16_at(b, free + 8 * k);
        let w = f32::from_bits(u32_at(b, free + 8 * k + 4));
        if symbol as u32 >= n_symbols
            || !is_free(symbol)
            || u16_at(b, free + 8 * k + 2) != 0
            || w.is_nan()
            || w == f32::NEG_INFINITY
        {
            return Err(corrupt(path, format!("free pair {k} is invalid")));
        }
    }
    Ok((free, n_free))
}

/// Check a section of `u32 n; u32 0; f32 values[n]` and its padding, named
/// `name`, whose values may not be NaN or `-inf`: where the values start, how
/// many there are, and the least of them (`+inf` for none).
fn floats(
    b: &[u8],
    (start, end): Extent,
    name: &str,
    path: &Path,
) -> Result<(usize, u32, f32), TransducerError> {
    if end - start < 8 || u32_at(b, start + 4) != 0 {
        return Err(corrupt(path, format!("{name} is truncated")));
    }
    let n = u32_at(b, start);
    if (n as usize)
        .checked_mul(4)
        .and_then(|len| len.checked_add(start + 8 + PADDING))
        .is_none_or(|e| e > end)
    {
        return Err(corrupt(path, format!("{name} runs past its section")));
    }
    let mut least = f32::INFINITY;
    for k in 0..n as usize {
        let w = f32::from_bits(u32_at(b, start + 8 + 4 * k));
        if w.is_nan() || w == f32::NEG_INFINITY {
            return Err(corrupt(path, format!("{name} value {k} is {w}")));
        }
        least = least.min(w);
    }
    Ok((start + 8, n, least))
}

fn parse(b: &[u8], path: &Path) -> Result<Parsed, TransducerError> {
    let known = [
        tag::SYMS,
        tag::CHCK,
        tag::SLOT,
        tag::LIST,
        tag::FINL,
        tag::FREE,
        tag::WGHT,
        tag::DIST,
        tag::META,
    ];
    let sections = sections(b, path, &known)?;
    let need = |wanted: [u8; 4]| sections.need(wanted, path);

    // SYMS
    let (names, alphabet) = symbols(b, need(tag::SYMS)?, path)?;
    let n_symbols = names.len() as u32;
    let is_free = |s: u16| s == 0 || alphabet.is_flag(SymbolNumber(s));

    // CHCK
    let (chck, chck_end) = need(tag::CHCK)?;
    if chck_end - chck < CHCK_HEAD_LEN || b[chck + 11..chck + CHCK_HEAD_LEN] != [0; 5] {
        return Err(corrupt(path, "CHCK is truncated"));
    }
    let n_slots = u32_at(b, chck);
    let n_ids = u32_at(b, chck + 4);
    let label_max = u16_at(b, chck + 8) as u32;
    let check_bytes = b[chck + 10] as usize;
    let checks = chck + CHCK_HEAD_LEN;
    if !(check_bytes == 1 || check_bytes == 2) {
        return Err(corrupt(path, format!("checks of {check_bytes} bytes")));
    }
    let check_mask = (1u32 << (8 * check_bytes)) - 1;
    if label_max > check_mask || label_max >= n_symbols {
        return Err(corrupt(
            path,
            format!("label {label_max} does not fit the checks or the alphabet"),
        ));
    }
    if n_ids == 0 || (n_slots as u64) < n_ids as u64 + label_max as u64 {
        return Err(corrupt(
            path,
            format!("{n_slots} slots cannot hold {n_ids} states with labels up to {label_max}"),
        ));
    }
    // One plane of low bytes, and one of high bytes after it if the checks
    // have two, each followed by its padding.
    let plane = n_slots as usize + PADDING;
    if plane
        .checked_mul(check_bytes)
        .and_then(|len| len.checked_add(checks))
        .is_none_or(|e| e > chck_end)
    {
        return Err(corrupt(path, "CHCK runs past its section"));
    }

    // SLOT
    let (slot, slot_end) = need(tag::SLOT)?;
    if slot_end - slot < RECORD_HEAD_LEN || b[slot + 7] != 0 || u32_at(b, slot) != n_slots {
        return Err(corrupt(path, "SLOT does not cover the slots"));
    }
    let record_bytes = b[slot + 4] as usize;
    let target_bits = b[slot + 5] as u32;
    let index_bits = b[slot + 6] as u32;
    let records = slot + RECORD_HEAD_LEN;
    if !(1..=8).contains(&record_bytes)
        || !(1..=32).contains(&target_bits)
        || !(1..=32).contains(&index_bits)
        || target_bits + index_bits > 8 * record_bytes as u32
    {
        return Err(corrupt(
            path,
            format!(
                "records of {record_bytes} bytes cannot hold fields of {target_bits} and {index_bits} bits"
            ),
        ));
    }
    if (n_slots as usize)
        .checked_mul(record_bytes)
        .and_then(|len| len.checked_add(records + PADDING))
        .is_none_or(|e| e > slot_end)
    {
        return Err(corrupt(path, "SLOT runs past its section"));
    }

    // LIST
    let (list, list_end) = need(tag::LIST)?;
    if list_end - list < RECORD_HEAD_LEN
        || b[list + 4] as usize != record_bytes
        || b[list + 5..list + 8] != [0, 0, 0]
    {
        return Err(corrupt(path, "LIST is truncated"));
    }
    let n_list = u32_at(b, list);
    let list = list + RECORD_HEAD_LEN;
    if (n_list as usize)
        .checked_mul(record_bytes)
        .and_then(|len| len.checked_add(list + PADDING))
        .is_none_or(|e| e > list_end)
    {
        return Err(corrupt(path, "LIST runs past its section"));
    }

    // FINL, FREE and WGHT
    let (finals, final_weights, n_finals) = finals(b, need(tag::FINL)?, n_ids, path)?;
    let (free, n_free) = free_pairs(b, need(tag::FREE)?, n_symbols, is_free, path)?;
    let (weights, n_weights, mut least) = floats(b, need(tag::WGHT)?, "WGHT", path)?;
    for k in 0..n_free as usize {
        least = least.min(f32::from_bits(u32_at(b, free + 8 * k + 4)));
    }
    let index_mask = (1u64 << index_bits) - 1;
    if n_weights as u64 > index_mask {
        return Err(corrupt(
            path,
            format!("{n_weights} weights do not fit indices of {index_bits} bits"),
        ));
    }

    // DIST
    let (distances, n_distances) = match sections.find(tag::DIST) {
        None => (0, 0),
        Some(extent) => {
            let (distances, n, _) = floats(b, extent, "DIST", path)?;
            if n != n_ids {
                return Err(corrupt(
                    path,
                    format!("DIST holds {n} distances for {n_ids} state numbers"),
                ));
            }
            (distances, n)
        }
    };

    let layout = Layout {
        label_max,
        n_ids,
        check_lo: checks - 1,
        wide: check_bytes == 2,
        check_hi: checks + plane - 1,
        records,
        list,
        record_bytes,
        record_mask: byte_mask(record_bytes),
        target_bits,
        target_mask: (1u64 << target_bits) - 1,
        index_mask,
        n_slots,
        n_list,
        finals,
        final_weights,
        n_finals,
        free,
        n_free,
        weights,
        n_weights,
        n_arcs: 0,
        n_free_arcs: 0,
        used_slots: 0,
        flags: sections.flags,
        version: sections.version,
        distances,
        n_distances,
    };
    let (n_arcs, n_free_arcs, used_slots) = check_slots(b, &layout, &is_free, path)?;

    let meta = sections
        .find(tag::META)
        .and_then(|(start, end)| std::str::from_utf8(&b[start..end]).ok())
        .map(|s| s.trim_end_matches('\0').to_string());

    Ok(Parsed {
        layout: Layout {
            n_arcs,
            n_free_arcs,
            used_slots,
            ..layout
        },
        alphabet,
        names,
        meta,
        least_weight: (n_arcs > 0 && least.is_finite()).then_some(Weight(least)),
    })
}

/// A slot that does not validate.
#[cold]
#[inline(never)]
fn bad_slot(path: &Path, p: usize, what: &'static str) -> TransducerError {
    corrupt(path, format!("slot {p}: {what}"))
}

/// Check every slot, and every record of `LIST` a slot names. Returns the
/// number of arcs, of free arcs, and of slots that hold any. The extents of
/// `CHCK`, `SLOT` and `LIST` are checked before this is called, so the reads
/// below stay inside them.
fn check_slots(
    b: &[u8],
    l: &Layout,
    is_free: &impl Fn(u16) -> bool,
    path: &Path,
) -> Result<(u64, u64, u32), TransducerError> {
    let fields = l.target_bits + l.index_mask.count_ones();
    let stray = if fields >= 64 {
        0
    } else {
        l.record_mask & !((1u64 << fields) - 1)
    };
    let record = |at: usize| word(b, at) & l.record_mask;
    let n_slots = l.n_slots as usize;
    let low = &b[l.check_lo + 1..l.check_lo + 1 + n_slots];
    let high = if l.wide {
        &b[l.check_hi + 1..l.check_hi + 1 + n_slots]
    } else {
        low
    };
    let regular: Vec<bool> = (0..=l.label_max)
        .map(|s| s != 0 && !is_free(s as u16))
        .collect();
    let (n_ids, n_weights) = (l.n_ids as u64, l.n_weights as u64);
    // Whether the listed records `k .. k + n` each hold a target and an
    // index below `limit`.
    let listed = |k: u64, n: u64, limit: u64| {
        k + n <= l.n_list as u64
            && (k..k + n).all(|k| {
                let r = record(l.list + l.record_bytes * k as usize);
                r & stray == 0
                    && r & l.target_mask < n_ids
                    && (r >> l.target_bits) & l.index_mask < limit
            })
    };

    let (mut arcs, mut free_arcs, mut used) = (0u64, 0u64, 0u32);
    for p in 0..n_slots {
        let c = if l.wide {
            low[p] as u32 | (high[p] as u32) << 8
        } else {
            low[p] as u32
        };
        let r = record(l.records + l.record_bytes * p);
        let (first, second) = (r & l.target_mask, (r >> l.target_bits) & l.index_mask);
        // Whether the slot's label is regular and its state a state number.
        let owned = regular.get(c as usize).copied().unwrap_or(false)
            & ((p as u64).wrapping_sub(c as u64) < n_ids);
        // Most slots hold one regular arc: tested in one go. A weight index
        // below the number of weights is not the list mark.
        if owned & (r & stray == 0) & (first < n_ids) & (second < n_weights) {
            used += 1;
            arcs += 1;
            continue;
        }
        if r == 0 && c == 0 {
            continue;
        }
        if r & stray != 0 {
            return Err(bad_slot(path, p, "its record has stray bits"));
        }
        used += 1;
        if c == 0 {
            if p as u64 >= n_ids || second == 0 || !listed(first, second, l.n_free as u64) {
                return Err(bad_slot(path, p, "its free arcs are out of range"));
            }
            free_arcs += second;
            arcs += second;
        } else if owned && second == l.index_mask && first < l.n_list as u64 {
            let head = record(l.list + l.record_bytes * first as usize);
            let n = head & l.target_mask;
            if head & !l.target_mask != 0 || n < 2 || !listed(first + 1, n, n_weights) {
                return Err(bad_slot(path, p, "its arcs are miscounted or out of range"));
            }
            arcs += n;
        } else {
            return Err(bad_slot(path, p, "its check or its arc is out of range"));
        }
    }
    Ok((arcs, free_arcs, used))
}

impl Transducer for DhfstAcceptor {
    const FILE_EXT: &'static str = "dhfst";
    const CURSOR: bool = false;

    #[inline(always)]
    fn alphabet(&self) -> &TransducerAlphabet {
        &self.alphabet
    }

    #[inline(always)]
    fn alphabet_mut(&mut self) -> &mut TransducerAlphabet {
        &mut self.alphabet
    }

    #[inline(always)]
    fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.final_entry(i.0)
            .is_some_and(|(_, bits, bit)| (bits >> bit) & 1 != 0)
    }

    #[inline(always)]
    fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        self.final_of(i.0).map(Weight)
    }

    #[inline(always)]
    fn distance_to_final(&self, i: TransitionTableIndex) -> Weight {
        let l = &self.layout;
        if i.0 >= l.n_distances {
            return Weight::ZERO;
        }
        Weight(f32::from_bits(
            word(&self.buf, l.distances + 4 * i.0 as usize) as u32,
        ))
    }

    fn least_arc_weight(&self) -> Option<Weight> {
        self.least_weight
    }

    /// `i` is the state plus one, as the cursor API addresses it.
    #[inline(always)]
    fn has_transitions(&self, i: TransitionTableIndex, s: Option<SymbolNumber>) -> bool {
        match s {
            Some(symbol) => self.slot(i.0, symbol).is_some(),
            None => false,
        }
    }

    /// `i` is the state plus one, as the cursor API addresses it.
    #[inline(always)]
    fn has_epsilons_or_flags(&self, i: TransitionTableIndex) -> bool {
        let (at, end) = self.free_run(i.0.wrapping_sub(1));
        at < end
    }

    #[inline(always)]
    fn free_arcs(
        &self,
        state: TransitionTableIndex,
    ) -> impl Iterator<Item = (SymbolNumber, SymbolTransition)> + '_ {
        let (at, end) = self.free_run(state.0);
        FreeArcs {
            acceptor: self,
            at,
            end,
        }
    }

    #[inline(always)]
    fn transitions(
        &self,
        state: TransitionTableIndex,
        input: SymbolNumber,
    ) -> impl Iterator<Item = SymbolTransition> + '_ {
        let (at, end) = self.arcs(state.0, input);
        Transitions {
            acceptor: self,
            symbol: input,
            at,
            end,
        }
    }

    fn for_each_arc<V>(&self, state: TransitionTableIndex, input: SymbolNumber, mut visit: V)
    where
        V: FnMut(SymbolNumber, TransitionTableIndex, Weight),
    {
        let mut take = |transition: SymbolTransition| {
            if let (Some(target), Some(weight)) = (transition.target(), transition.weight()) {
                visit(input, target, weight);
            }
        };
        if input == SymbolNumber::ZERO {
            for (symbol, transition) in self.free_arcs(state) {
                if symbol == SymbolNumber::ZERO {
                    take(transition);
                }
            }
        } else {
            for transition in self.transitions(state, input) {
                take(transition);
            }
        }
    }

    // The cursor API walks one flat run of arcs per input symbol from a
    // position that carries no state, and this format has no such runs: a
    // state's arcs are found from the state. The suggestion search and the
    // generator reach a lexicon through `has_transitions`,
    // `has_epsilons_or_flags`, `free_arcs` and `transitions`; the cursor API
    // answers "no arcs" rather than a partial view.

    #[inline(always)]
    fn transition_input_symbol(&self, _i: TransitionTableIndex) -> Option<SymbolNumber> {
        None
    }

    #[inline(always)]
    fn next(
        &self,
        _i: TransitionTableIndex,
        _symbol: SymbolNumber,
    ) -> Option<TransitionTableIndex> {
        None
    }

    #[inline(always)]
    fn take_epsilons_and_flags(&self, _i: TransitionTableIndex) -> Option<SymbolTransition> {
        None
    }

    #[inline(always)]
    fn take_epsilons(&self, _i: TransitionTableIndex) -> Option<SymbolTransition> {
        None
    }

    #[inline(always)]
    fn take_non_epsilons(
        &self,
        _i: TransitionTableIndex,
        _symbol: SymbolNumber,
    ) -> Option<SymbolTransition> {
        None
    }
}

impl<F: vfs::File> TransducerLoader<F> for DhfstAcceptor {
    fn from_path<P, FS>(fs: &FS, path: P) -> Result<DhfstAcceptor, TransducerError>
    where
        P: AsRef<Path>,
        FS: Filesystem<File = F>,
    {
        let path = path.as_ref();
        let file = fs.open_file(path).map_err(|source| TransducerError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mmap = unsafe { file.memory_map() }.map_err(|source| TransducerError::Memmap {
            path: path.to_path_buf(),
            source,
        })?;
        DhfstAcceptor::from_mmap(mmap, path.to_path_buf())
    }
}

/// The eight bytes every DHFST acceptor starts with.
pub fn prefix() -> [u8; PREFIX_LEN] {
    DhfstType::Acceptor.prefix()
}
