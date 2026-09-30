//! Compact error models in the DHFST format.
//!
//! An error model built the usual way is determinised ahead of time, and every
//! state then carries the full fan of `x:y` substitutions over the alphabet:
//! the Northern Sámi model is 86 million arcs, 94% of them substitutions, most
//! of which say "any other letter, to the same state, at the same weight". This
//! format stores that fan once per state, as a *default* arc, and lets a state
//! borrow the rest of its row from another state it mostly agrees with.
//!
//! # What a state means
//!
//! A state holds explicit entries, at most one default arc per kind, and
//! optionally a *fallback* state. The arcs of state `q` for the pair `x:o` are
//! resolved as follows, level by level starting at `q`:
//!
//! 1. If the level has explicit entries for `x:o`, they are the answer. A
//!    *blocker* entry means the pair has no arc, and nothing further down is
//!    asked.
//! 2. Otherwise, if the level has a default arc of the pair's kind whose span
//!    holds `x:o`, its single target and weight is the answer.
//! 3. Otherwise the level's fallback state is asked, from step 1. Without a
//!    fallback, the pair has no arc.
//!
//! The kinds are identity `x:x`, substitution `x:y` with `x ≠ y`, deletion
//! `x:ε` and insertion `ε:y`, all over *regular* symbols (not epsilon, not a
//! flag diacritic, not `@_IDENTITY_SYMBOL_@` or `@_UNKNOWN_SYMBOL_@`). Every
//! other pair is answered by explicit entries only. The fallback is a storage
//! device and not a transition: it consumes nothing, costs nothing, and the
//! targets it hands back are the ones stored where the answer was found.
//! Finality is never inherited.
//!
//! A substitution default says "any output in this class other than the
//! input", so it is handed to the caller as an [`ArcGroup::Each`]: the
//! suggestion search intersects it with what the lexicon can continue with,
//! the same way it treats an `@_UNKNOWN_SYMBOL_@` output.
//!
//! # Layout (version 1)
//!
//! Every multi-byte field is little-endian. The reader reads through byte
//! slices, so a mapping at any alignment is read correctly.
//!
//! ```text
//! offset  size  field
//! 0       5     "DHFST"
//! 5       1     version = 1
//! 6       2     reserved, zero
//! 8       4     flags: bit 0 tropical f32 weights (required), bit 1 fallback
//!               rows used, bit 2 default records used, bit 3 RULE section
//! 12      4     number of sections
//! 16      4     max_fallback_depth: no fallback chain is longer
//! 20      4     reserved, zero
//! 24      24*n  section table: tag[4], flags u32, offset u64, length u64
//! ...           sections, each at a multiple of 8, zero-padded to one
//! ```
//!
//! A section whose tag starts with an upper-case letter is critical: a reader
//! that does not know it refuses the file. A lower-case tag is ancillary and is
//! skipped.
//!
//! * `SYMS` (critical): `u32 n; u32 offsets[n + 1]; u8 names[]`, UTF-8. Symbol
//!   0 is `@_EPSILON_SYMBOL_@`; symbols are classified by name, as in HFST.
//! * `CLAS` (critical when defaults are used): `u32 words_per_class; u32 n;
//!   u64 bits[n * words_per_class]`; symbol `s` of class `c` is word
//!   `c * words_per_class + s / 64`, bit `s % 64`. Only regular symbols.
//! * `CPAI` (critical when substitution defaults are used): `u32 n; u32 0;
//!   { u16 input_class; u16 output_class }[n]`.
//! * `STAT` (critical): `u32 n; u32 start; { u32 first_entry; u32 n_entries;
//!   u32 fallback; f32 final_weight }[n]`, fallback `0xFFFFFFFF` for none and
//!   final weight `+inf` for a state that is not final.
//! * `ENTR` (critical): `u32 n; u32 0; { u16 input; u16 output; u32 target;
//!   f32 weight }[n]`. A state's entries are its explicit entries sorted by
//!   `(input, output, target)`, a blocker having target `0xFFFFFFFF`, followed
//!   by at most one default record per kind, sorted by kind. A default record
//!   has input `0xFFF0 + kind` (0 identity, 1 substitution, 2 deletion, 3
//!   insertion) and, as output, the class it covers: a class for identity and
//!   deletion (inputs) and insertion (outputs), a class pair for substitution.
//! * `meta` (ancillary): UTF-8 JSON describing how the file was written.
//! * `RULE` (critical, reserved for rule tries): not read by this version.
//!
//! Loading validates every section, run, class, target and fallback chain, so
//! nothing read after a successful load can fall outside the file.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use memmap2::Mmap;

use crate::transducer::alphabet::TransducerAlphabet;
use crate::transducer::hfst::alphabet::TransducerAlphabetParser;
use crate::transducer::symbol_transition::SymbolTransition;
use crate::transducer::{
    ArcGroup, SymbolSet, Transducer, TransducerError, TransducerFormat, TransducerLoader,
};
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
use crate::vfs::{self, Filesystem};

/// The first five bytes of a DHFST file.
pub const MAGIC: &[u8; 5] = b"DHFST";
/// The format version this reader reads and the writer writes.
pub const VERSION: u8 = 1;
/// Bytes before the section table.
pub const HEADER_LEN: usize = 24;
/// Bytes per section table entry.
pub const SECTION_ENTRY_LEN: usize = 24;
/// Bytes per state record.
pub const STATE_LEN: usize = 16;
/// Bytes per entry.
pub const ENTRY_LEN: usize = 12;
/// A fallback of no state, and the target of a blocker.
pub const NONE: u32 = u32::MAX;
/// Input field of the first default record kind; kinds follow in order.
pub const DEFAULT_BASE: u16 = 0xFFF0;
/// Symbol numbers from here on are reserved for default records.
pub const MAX_SYMBOLS: u32 = DEFAULT_BASE as u32;

/// Header flag: weights are tropical `f32`.
pub const FLAG_TROPICAL: u32 = 1 << 0;
/// Header flag: some state has a fallback.
pub const FLAG_FALLBACK: u32 = 1 << 1;
/// Header flag: some state has a default record.
pub const FLAG_DEFAULTS: u32 = 1 << 2;
/// Header flag: a `RULE` section is present.
pub const FLAG_RULES: u32 = 1 << 3;
const KNOWN_FLAGS: u32 = FLAG_TROPICAL | FLAG_FALLBACK | FLAG_DEFAULTS | FLAG_RULES;

/// Section tags.
pub mod tag {
    /// symbol table
    pub const SYMS: [u8; 4] = *b"SYMS";
    /// symbol classes
    pub const CLAS: [u8; 4] = *b"CLAS";
    /// class pairs
    pub const CPAI: [u8; 4] = *b"CPAI";
    /// states
    pub const STAT: [u8; 4] = *b"STAT";
    /// entries
    pub const ENTR: [u8; 4] = *b"ENTR";
    /// rule tries (reserved)
    pub const RULE: [u8; 4] = *b"RULE";
    /// writer metadata
    pub const META: [u8; 4] = *b"meta";
}

/// The kind of a default record, and of a pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum DefaultKind {
    /// `x:x`
    Identity = 0,
    /// `x:y`, `x ≠ y`
    Substitution = 1,
    /// `x:ε`
    Deletion = 2,
    /// `ε:y`
    Insertion = 3,
}

impl DefaultKind {
    /// Every kind, in record order.
    pub const ALL: [DefaultKind; 4] = [
        DefaultKind::Identity,
        DefaultKind::Substitution,
        DefaultKind::Deletion,
        DefaultKind::Insertion,
    ];

    /// The kind a default record's input field names.
    pub fn from_record(input: u16) -> Option<DefaultKind> {
        match input.checked_sub(DEFAULT_BASE)? {
            0 => Some(DefaultKind::Identity),
            1 => Some(DefaultKind::Substitution),
            2 => Some(DefaultKind::Deletion),
            3 => Some(DefaultKind::Insertion),
            _ => None,
        }
    }

    /// The input field of a default record of this kind.
    pub fn record(self) -> u16 {
        DEFAULT_BASE + self as u16
    }
}

/// Whether a symbol name denotes a regular symbol, one a default arc may
/// cover: anything but epsilon (symbol 0) and the `@…@` special symbols, which
/// are flag diacritics, the identity and unknown wildcards, and anything else
/// HFST reserves that way.
pub fn is_regular_name(symbol: usize, name: &str) -> bool {
    symbol != 0 && !(name.len() > 1 && name.starts_with('@') && name.ends_with('@'))
}

/// The kind of the pair `input:output`, or `None` for a pair no default arc
/// may cover.
#[inline(always)]
pub fn pair_kind(
    input: u16,
    output: u16,
    input_regular: bool,
    output_regular: bool,
) -> Option<DefaultKind> {
    match (input_regular, output_regular) {
        (true, true) if input == output => Some(DefaultKind::Identity),
        (true, true) => Some(DefaultKind::Substitution),
        (true, false) if output == 0 => Some(DefaultKind::Deletion),
        (false, true) if input == 0 => Some(DefaultKind::Insertion),
        _ => None,
    }
}

#[inline(always)]
fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

#[inline(always)]
fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

#[inline(always)]
fn u64_at(b: &[u8], at: usize) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(bytes)
}

/// One entry of a state's run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry {
    /// input symbol, or a default record's kind
    pub input: u16,
    /// output symbol, or a default record's class or class pair
    pub output: u16,
    /// target state, [`NONE`] for a blocker
    pub target: u32,
    /// weight
    pub weight: f32,
}

/// Where the sections of a validated file are.
#[derive(Clone, Copy, Debug)]
struct Layout {
    n_symbols: u32,
    words: usize,
    clas: usize,
    n_classes: u32,
    cpai: usize,
    n_pairs: u32,
    stat: usize,
    n_states: u32,
    start: u32,
    entr: usize,
    n_entries: u32,
    max_fallback_depth: u32,
    flags: u32,
}

/// A validated DHFST error model, read in place from a memory map.
pub struct DhfstTransducer {
    buf: Arc<Mmap>,
    layout: Layout,
    /// Regular symbols, as a bitset.
    regular: Vec<u64>,
    alphabet: TransducerAlphabet,
    symbol_names: Vec<String>,
    meta: Option<String>,
}

impl std::fmt::Debug for DhfstTransducer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DhfstTransducer")
            .field("bytes", &self.buf.len())
            .field("symbols", &self.layout.n_symbols)
            .field("states", &self.layout.n_states)
            .field("entries", &self.layout.n_entries)
            .field("classes", &self.layout.n_classes)
            .field("max_fallback_depth", &self.layout.max_fallback_depth)
            .finish()
    }
}

/// How many bitset words fit inline before the arc walk allocates.
const INLINE_WORDS: usize = 8;

impl DhfstTransducer {
    /// Parse and validate a DHFST file out of a memory-mapped buffer.
    ///
    /// `path` is used only for error reporting. Every section, state run,
    /// class, target and fallback chain is checked here, so once this
    /// succeeds no later lookup can read outside the buffer.
    pub fn from_mapped_memory(
        buf: Arc<Mmap>,
        path: impl Into<PathBuf>,
    ) -> Result<DhfstTransducer, TransducerError> {
        let path = path.into();
        let parsed = Parsed::parse(&buf, &path)?;
        let alphabet = parsed.alphabet(&path)?;

        Ok(DhfstTransducer {
            layout: parsed.layout,
            regular: parsed.regular,
            symbol_names: parsed.names,
            meta: parsed.meta,
            alphabet,
            buf,
        })
    }

    /// Load a DHFST file held in memory, by copying it into an anonymous
    /// mapping.
    pub fn from_bytes(
        bytes: &[u8],
        path: impl Into<PathBuf>,
    ) -> Result<DhfstTransducer, TransducerError> {
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
        DhfstTransducer::from_mapped_memory(Arc::new(map), path)
    }

    /// The raw bytes of the file.
    pub fn buffer(&self) -> &[u8] {
        &self.buf
    }

    /// Number of states.
    pub fn state_count(&self) -> u32 {
        self.layout.n_states
    }

    /// Number of entries, default records included.
    pub fn entry_count(&self) -> u32 {
        self.layout.n_entries
    }

    /// Number of symbol classes.
    pub fn class_count(&self) -> u32 {
        self.layout.n_classes
    }

    /// Number of class pairs.
    pub fn class_pair_count(&self) -> u32 {
        self.layout.n_pairs
    }

    /// The longest fallback chain the file admits.
    pub fn max_fallback_depth(&self) -> u32 {
        self.layout.max_fallback_depth
    }

    /// The header flags.
    pub fn flags(&self) -> u32 {
        self.layout.flags
    }

    /// Symbol names as stored, `@_EPSILON_SYMBOL_@` first.
    pub fn symbol_names(&self) -> &[String] {
        &self.symbol_names
    }

    /// The writer's `meta` section, if the file carries one.
    pub fn meta(&self) -> Option<&str> {
        self.meta.as_deref()
    }

    /// The start state as stored. The transducer presents it to callers as
    /// state 0, as the search expects, by swapping it with state 0.
    pub fn stored_start(&self) -> u32 {
        self.layout.start
    }

    /// Whether `symbol` is regular.
    #[inline(always)]
    pub fn is_regular(&self, symbol: u16) -> bool {
        let s = symbol as usize;
        self.regular
            .get(s / 64)
            .is_some_and(|word| word & (1u64 << (s % 64)) != 0)
    }

    /// The stored state behind the state number callers see, and back: state
    /// 0 and the stored start state trade places, so a search that starts in
    /// state 0 starts where the file says.
    #[inline(always)]
    fn swap_start(&self, state: u32) -> u32 {
        let start = self.layout.start;
        if state == 0 {
            start
        } else if state == start {
            0
        } else {
            state
        }
    }

    /// `(first entry, entry count, fallback, final weight)` of a stored state.
    #[inline(always)]
    fn state_record(&self, state: u32) -> (u32, u32, u32, f32) {
        let b: &[u8] = &self.buf;
        let at = self.layout.stat + STATE_LEN * state as usize;
        (
            u32_at(b, at),
            u32_at(b, at + 4),
            u32_at(b, at + 8),
            f32::from_bits(u32_at(b, at + 12)),
        )
    }

    /// One entry.
    #[inline(always)]
    pub fn entry(&self, index: u32) -> Entry {
        let b: &[u8] = &self.buf;
        let at = self.layout.entr + ENTRY_LEN * index as usize;
        Entry {
            input: u16_at(b, at),
            output: u16_at(b, at + 2),
            target: u32_at(b, at + 4),
            weight: f32::from_bits(u32_at(b, at + 8)),
        }
    }

    /// How many of the run's entries are explicit: default records sort last
    /// and there are at most four of them.
    #[inline(always)]
    fn explicit_len(&self, first: u32, len: u32) -> u32 {
        let mut explicit = len;
        while explicit > 0 && self.entry(first + explicit - 1).input >= DEFAULT_BASE {
            explicit -= 1;
        }
        explicit
    }

    /// Word `word` of class `class`.
    #[inline(always)]
    fn class_word(&self, class: u16, word: usize) -> u64 {
        u64_at(
            &self.buf,
            self.layout.clas + 8 * (class as usize * self.layout.words + word),
        )
    }

    #[inline(always)]
    fn class_has(&self, class: u16, symbol: u16) -> bool {
        let s = symbol as usize;
        self.class_word(class, s / 64) & (1u64 << (s % 64)) != 0
    }

    /// The input and output class of a class pair.
    #[inline(always)]
    fn class_pair(&self, pair: u16) -> (u16, u16) {
        let at = self.layout.cpai + 8 + 4 * pair as usize;
        (u16_at(&self.buf, at), u16_at(&self.buf, at + 2))
    }

    /// Stored state, entries of its run and fallback, for the stats tools.
    pub fn stored_state(&self, state: u32) -> Option<StoredState> {
        if state >= self.layout.n_states {
            return None;
        }
        let (first, len, fallback, final_weight) = self.state_record(state);
        Some(StoredState {
            first,
            len,
            explicit: self.explicit_len(first, len),
            fallback: (fallback != NONE).then_some(fallback),
            final_weight: final_weight.is_finite().then_some(final_weight),
        })
    }

    /// Walk the arcs of `state` (as callers number it) on `input`, level by
    /// level down the fallback chain, handing each answered pair to `visit`
    /// once.
    #[inline]
    fn walk<V>(&self, state: u32, input: u16, visit: &mut V)
    where
        V: FnMut(ArcGroup<'_>),
    {
        if state >= self.layout.n_states {
            return;
        }
        let mut level = self.swap_start(state);
        let (first, len, fallback, _) = self.state_record(level);
        let explicit = self.explicit_len(first, len);

        // A state that neither borrows nor defaults answers from its own
        // entries alone, which is most states of a model that was never
        // determinised.
        if fallback == NONE && explicit == len {
            self.walk_explicit(first, explicit, input, visit);
            return;
        }

        let words = self.layout.words;
        let mut inline = [[0u64; INLINE_WORDS]; 3];
        let mut heap: Vec<u64>;
        let (decided, newly, offer): (&mut [u64], &mut [u64], &mut [u64]) = if words <= INLINE_WORDS
        {
            let [a, b, c] = &mut inline;
            (&mut a[..words], &mut b[..words], &mut c[..words])
        } else {
            heap = vec![0u64; 3 * words];
            let (a, rest) = heap.split_at_mut(words);
            let (b, c) = rest.split_at_mut(words);
            (a, b, c)
        };

        let input_regular = self.is_regular(input);
        let x = input as usize;
        let (mut first, mut len, mut fallback) = (first, len, fallback);
        let mut explicit = explicit;

        loop {
            // Explicit entries answer their pair, unless an earlier level
            // already did. Several arcs may share a pair, so the test is
            // against earlier levels only.
            let end = first + explicit;
            let mut at = self.lower_bound(first, end, input);
            while at < end {
                let entry = self.entry(at);
                if entry.input != input {
                    break;
                }
                let o = entry.output as usize;
                let bit = 1u64 << (o % 64);
                if decided[o / 64] & bit == 0 {
                    newly[o / 64] |= bit;
                    if entry.target != NONE {
                        visit(ArcGroup::One {
                            output: SymbolNumber(entry.output),
                            target: TransitionTableIndex(self.swap_start(entry.target)),
                            weight: Weight(entry.weight),
                        });
                    }
                }
                at += 1;
            }

            // Default records answer every pair of their span that neither
            // an earlier level nor this level's explicit entries answered.
            for index in end..first + len {
                let record = self.entry(index);
                let target = TransitionTableIndex(self.swap_start(record.target));
                let weight = Weight(record.weight);
                match DefaultKind::from_record(record.input) {
                    Some(DefaultKind::Identity) => {
                        let bit = 1u64 << (x % 64);
                        if input_regular
                            && self.class_has(record.output, input)
                            && (decided[x / 64] | newly[x / 64]) & bit == 0
                        {
                            newly[x / 64] |= bit;
                            visit(ArcGroup::One {
                                output: SymbolNumber(input),
                                target,
                                weight,
                            });
                        }
                    }
                    Some(DefaultKind::Substitution) => {
                        if !input_regular {
                            continue;
                        }
                        let (from, to) = self.class_pair(record.output);
                        if !self.class_has(from, input) {
                            continue;
                        }
                        let mut any = 0u64;
                        for word in 0..words {
                            let mut bits =
                                self.class_word(to, word) & !(decided[word] | newly[word]);
                            if word == x / 64 {
                                bits &= !(1u64 << (x % 64));
                            }
                            offer[word] = bits;
                            any |= bits;
                        }
                        if any != 0 {
                            for word in 0..words {
                                newly[word] |= offer[word];
                            }
                            visit(ArcGroup::Each {
                                outputs: SymbolSet::new(offer),
                                target,
                                weight,
                            });
                        }
                    }
                    Some(DefaultKind::Deletion) => {
                        if input_regular
                            && self.class_has(record.output, input)
                            && (decided[0] | newly[0]) & 1 == 0
                        {
                            newly[0] |= 1;
                            visit(ArcGroup::One {
                                output: SymbolNumber::ZERO,
                                target,
                                weight,
                            });
                        }
                    }
                    Some(DefaultKind::Insertion) => {
                        if input != 0 {
                            continue;
                        }
                        let mut any = 0u64;
                        for word in 0..words {
                            let bits = self.class_word(record.output, word)
                                & !(decided[word] | newly[word]);
                            offer[word] = bits;
                            any |= bits;
                        }
                        if any != 0 {
                            for word in 0..words {
                                newly[word] |= offer[word];
                            }
                            visit(ArcGroup::Each {
                                outputs: SymbolSet::new(offer),
                                target,
                                weight,
                            });
                        }
                    }
                    None => {}
                }
            }

            if fallback == NONE {
                return;
            }
            for word in 0..words {
                decided[word] |= newly[word];
                newly[word] = 0;
            }
            level = fallback;
            let (next_first, next_len, next_fallback, _) = self.state_record(level);
            first = next_first;
            len = next_len;
            fallback = next_fallback;
            explicit = self.explicit_len(first, len);
        }
    }

    /// The explicit arcs of a run on `input`, for a state with no defaults and
    /// no fallback.
    #[inline(always)]
    fn walk_explicit<V>(&self, first: u32, explicit: u32, input: u16, visit: &mut V)
    where
        V: FnMut(ArcGroup<'_>),
    {
        let end = first + explicit;
        let mut at = self.lower_bound(first, end, input);
        while at < end {
            let entry = self.entry(at);
            if entry.input != input {
                break;
            }
            if entry.target != NONE {
                visit(ArcGroup::One {
                    output: SymbolNumber(entry.output),
                    target: TransitionTableIndex(self.swap_start(entry.target)),
                    weight: Weight(entry.weight),
                });
            }
            at += 1;
        }
    }

    /// First entry in `[lo, hi)` whose input is not below `input`.
    #[inline(always)]
    fn lower_bound(&self, mut lo: u32, mut hi: u32, input: u16) -> u32 {
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.entry(mid).input < input {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }
}

/// A stored state, as the stats tools see it.
#[derive(Clone, Copy, Debug)]
pub struct StoredState {
    /// first entry of the run
    pub first: u32,
    /// entries in the run, default records included
    pub len: u32,
    /// explicit entries in the run
    pub explicit: u32,
    /// the state this one falls back to
    pub fallback: Option<u32>,
    /// final weight, if the state is final
    pub final_weight: Option<f32>,
}

/// A file parsed and validated, before it becomes a transducer.
struct Parsed {
    layout: Layout,
    regular: Vec<u64>,
    names: Vec<String>,
    meta: Option<String>,
}

fn corrupt(path: &Path, detail: impl Into<Cow<'static, str>>) -> TransducerError {
    TransducerError::CorruptTables {
        path: path.to_path_buf(),
        detail: detail.into(),
    }
}

impl Parsed {
    fn parse(b: &[u8], path: &Path) -> Result<Parsed, TransducerError> {
        match TransducerFormat::detect(b, path)? {
            TransducerFormat::Dhfst { .. } => {}
            TransducerFormat::Hfst => {
                return Err(TransducerError::UnrecognisedFormat {
                    path: path.to_path_buf(),
                    detail: Cow::Borrowed("the file is HFST optimized lookup, not DHFST"),
                });
            }
        }
        if b.len() < HEADER_LEN {
            return Err(TransducerError::CorruptHeader {
                path: path.to_path_buf(),
                offset: b.len(),
            });
        }

        let flags = u32_at(b, 8);
        let n_sections = u32_at(b, 12) as usize;
        let max_fallback_depth = u32_at(b, 16);
        if u16_at(b, 6) != 0 || u32_at(b, 20) != 0 {
            return Err(TransducerError::CorruptHeader {
                path: path.to_path_buf(),
                offset: 6,
            });
        }
        if flags & !KNOWN_FLAGS != 0 {
            return Err(corrupt(path, format!("unknown header flags {flags:#x}")));
        }
        if flags & FLAG_TROPICAL == 0 {
            return Err(corrupt(path, "weights are not declared tropical f32"));
        }
        if flags & FLAG_RULES != 0 {
            return Err(corrupt(
                path,
                "the file carries a RULE section, which this reader does not implement",
            ));
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
            if offset % 8 != 0 || offset < table_end || end > b.len() {
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
            let known = [
                tag::SYMS,
                tag::CLAS,
                tag::CPAI,
                tag::STAT,
                tag::ENTR,
                tag::META,
            ];
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

        let find = |wanted: [u8; 4]| {
            sections
                .iter()
                .find(|(t, _, _)| *t == wanted)
                .map(|(_, start, end)| (*start, *end))
        };
        let need = |wanted: [u8; 4]| {
            find(wanted).ok_or_else(|| {
                corrupt(
                    path,
                    format!(
                        "required section {} is missing",
                        String::from_utf8_lossy(&wanted)
                    ),
                )
            })
        };

        // SYMS
        let (syms, syms_end) = need(tag::SYMS)?;
        if syms_end - syms < 4 {
            return Err(corrupt(path, "SYMS is truncated"));
        }
        let n_symbols = u32_at(b, syms);
        if n_symbols == 0 || n_symbols > MAX_SYMBOLS {
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
        let words = (n_symbols as usize).div_ceil(64);
        let mut regular = vec![0u64; words];
        for (s, name) in names.iter().enumerate() {
            if is_regular_name(s, name) {
                regular[s / 64] |= 1u64 << (s % 64);
            }
        }

        // CLAS
        let (clas, n_classes) = match find(tag::CLAS) {
            Some((start, end)) => {
                if end - start < 8 || u32_at(b, start) as usize != words {
                    return Err(corrupt(path, "CLAS words per class do not match SYMS"));
                }
                let n = u32_at(b, start + 4);
                (n as usize)
                    .checked_mul(words * 8)
                    .and_then(|len| len.checked_add(start + 8))
                    .filter(|e| *e <= end)
                    .ok_or_else(|| corrupt(path, "CLAS runs past the section"))?;
                for c in 0..n as usize {
                    for (w, regular_word) in regular.iter().enumerate() {
                        let word = u64_at(b, start + 8 + 8 * (c * words + w));
                        if word & !regular_word != 0 {
                            return Err(corrupt(
                                path,
                                format!("class {c} holds a symbol that is not regular"),
                            ));
                        }
                    }
                }
                (start + 8, n)
            }
            None => (0, 0),
        };
        if n_classes > u16::MAX as u32 + 1 {
            return Err(corrupt(path, "too many classes"));
        }

        // CPAI
        let (cpai, n_pairs) = match find(tag::CPAI) {
            Some((start, end)) => {
                if end - start < 8 {
                    return Err(corrupt(path, "CPAI is truncated"));
                }
                let n = u32_at(b, start);
                if (n as usize) * 4 + 8 > end - start {
                    return Err(corrupt(path, "CPAI runs past the section"));
                }
                for p in 0..n as usize {
                    let at = start + 8 + 4 * p;
                    if u16_at(b, at) as u32 >= n_classes || u16_at(b, at + 2) as u32 >= n_classes {
                        return Err(corrupt(
                            path,
                            format!("class pair {p} names a missing class"),
                        ));
                    }
                }
                (start, n)
            }
            None => (0, 0),
        };
        if n_pairs > u16::MAX as u32 + 1 {
            return Err(corrupt(path, "too many class pairs"));
        }

        // ENTR
        let (entr, entr_end) = need(tag::ENTR)?;
        if entr_end - entr < 8 {
            return Err(corrupt(path, "ENTR is truncated"));
        }
        let n_entries = u32_at(b, entr);
        if (n_entries as usize)
            .checked_mul(ENTRY_LEN)
            .and_then(|len| len.checked_add(entr + 8))
            .is_none_or(|e| e > entr_end)
        {
            return Err(corrupt(path, "ENTR runs past the section"));
        }

        // STAT
        let (stat, stat_end) = need(tag::STAT)?;
        if stat_end - stat < 8 {
            return Err(corrupt(path, "STAT is truncated"));
        }
        let n_states = u32_at(b, stat);
        let start = u32_at(b, stat + 4);
        if n_states == 0 || n_states == NONE || start >= n_states {
            return Err(corrupt(path, "STAT has no states, or no valid start state"));
        }
        if (n_states as usize)
            .checked_mul(STATE_LEN)
            .and_then(|len| len.checked_add(stat + 8))
            .is_none_or(|e| e > stat_end)
        {
            return Err(corrupt(path, "STAT runs past the section"));
        }

        let layout = Layout {
            n_symbols,
            words,
            clas,
            n_classes,
            cpai,
            n_pairs,
            stat: stat + 8,
            n_states,
            start,
            entr: entr + 8,
            n_entries,
            max_fallback_depth,
            flags,
        };

        let mut any_fallback = false;
        let mut any_default = false;
        for q in 0..n_states {
            let at = layout.stat + STATE_LEN * q as usize;
            let first = u32_at(b, at);
            let len = u32_at(b, at + 4);
            let fallback = u32_at(b, at + 8);
            let final_weight = f32::from_bits(u32_at(b, at + 12));
            if final_weight.is_nan() || final_weight == f32::NEG_INFINITY {
                return Err(corrupt(
                    path,
                    format!("state {q} has an invalid final weight"),
                ));
            }
            if first.checked_add(len).is_none_or(|end| end > n_entries) {
                return Err(corrupt(path, format!("run of state {q} is out of range")));
            }
            if fallback != NONE {
                if fallback >= n_states || fallback == q {
                    return Err(corrupt(path, format!("fallback of state {q} is invalid")));
                }
                any_fallback = true;
            }
            let mut previous: Option<Entry> = None;
            let mut previous_kind: Option<DefaultKind> = None;
            for e in first..first + len {
                let at = layout.entr + ENTRY_LEN * e as usize;
                let entry = Entry {
                    input: u16_at(b, at),
                    output: u16_at(b, at + 2),
                    target: u32_at(b, at + 4),
                    weight: f32::from_bits(u32_at(b, at + 8)),
                };
                if entry.weight.is_nan() {
                    return Err(corrupt(path, format!("entry {e} has a NaN weight")));
                }
                if entry.input >= DEFAULT_BASE {
                    let kind = DefaultKind::from_record(entry.input).ok_or_else(|| {
                        corrupt(path, format!("entry {e} has a reserved input symbol"))
                    })?;
                    if previous_kind.is_some_and(|p| p >= kind) {
                        return Err(corrupt(
                            path,
                            format!("default records of state {q} are repeated or unsorted"),
                        ));
                    }
                    previous_kind = Some(kind);
                    let class_ok = match kind {
                        DefaultKind::Substitution => (entry.output as u32) < n_pairs,
                        _ => (entry.output as u32) < n_classes,
                    };
                    if !class_ok || entry.target >= n_states {
                        return Err(corrupt(
                            path,
                            format!("default record {e} names a missing class or state"),
                        ));
                    }
                    any_default = true;
                    continue;
                }
                if previous_kind.is_some() {
                    return Err(corrupt(
                        path,
                        format!("explicit entry {e} of state {q} follows a default record"),
                    ));
                }
                if entry.input as u32 >= n_symbols || entry.output as u32 >= n_symbols {
                    return Err(corrupt(path, format!("entry {e} names a missing symbol")));
                }
                if entry.target != NONE && entry.target >= n_states {
                    return Err(corrupt(path, format!("entry {e} targets a missing state")));
                }
                if let Some(p) = previous {
                    let key = (entry.input, entry.output, entry.target);
                    let previous_key = (p.input, p.output, p.target);
                    if key < previous_key {
                        return Err(corrupt(
                            path,
                            format!("explicit entries of state {q} are not sorted"),
                        ));
                    }
                    if (entry.input, entry.output) == (p.input, p.output)
                        && (entry.target == NONE || p.target == NONE)
                    {
                        return Err(corrupt(
                            path,
                            format!("a blocker of state {q} shares its pair with an arc"),
                        ));
                    }
                }
                previous = Some(entry);
            }
        }
        if any_fallback && flags & FLAG_FALLBACK == 0 {
            return Err(corrupt(path, "fallback rows are used but not declared"));
        }
        if any_default && flags & FLAG_DEFAULTS == 0 {
            return Err(corrupt(path, "default records are used but not declared"));
        }

        // Fallback chains: acyclic and no longer than the header promises.
        const UNKNOWN: u32 = u32::MAX;
        const IN_PROGRESS: u32 = u32::MAX - 1;
        let mut depth: Vec<u32> = vec![UNKNOWN; n_states as usize];
        let mut chain: Vec<u32> = Vec::new();
        for q in 0..n_states {
            if depth[q as usize] != UNKNOWN {
                continue;
            }
            chain.clear();
            let mut s = q;
            // The depth of the last state pushed: 0 when its chain ends, one
            // more than the known depth of the state it falls back to.
            let mut next_depth = loop {
                match depth[s as usize] {
                    UNKNOWN => {}
                    IN_PROGRESS => {
                        return Err(corrupt(
                            path,
                            format!("fallback chain from state {q} is cyclic"),
                        ));
                    }
                    known => break known + 1,
                }
                depth[s as usize] = IN_PROGRESS;
                chain.push(s);
                if chain.len() as u64 > max_fallback_depth as u64 + 1 {
                    return Err(corrupt(
                        path,
                        format!(
                            "fallback chain from state {q} is longer than {max_fallback_depth}"
                        ),
                    ));
                }
                let fallback = u32_at(b, layout.stat + STATE_LEN * s as usize + 8);
                if fallback == NONE {
                    break 0;
                }
                s = fallback;
            };
            for state in chain.iter().rev() {
                if next_depth > max_fallback_depth {
                    return Err(corrupt(
                        path,
                        format!(
                            "fallback chain from state {q} is longer than {max_fallback_depth}"
                        ),
                    ));
                }
                depth[*state as usize] = next_depth;
                next_depth += 1;
            }
        }

        let meta = find(tag::META)
            .and_then(|(start, end)| std::str::from_utf8(&b[start..end]).ok())
            .map(|s| s.trim_end_matches('\0').to_string());

        Ok(Parsed {
            layout,
            regular,
            names,
            meta,
        })
    }

    /// The alphabet, parsed exactly as an HFST file with the same symbol table
    /// would be, so that tokenisation and the lexicon translator see the same
    /// symbols whichever format the error model came in.
    fn alphabet(&self, path: &Path) -> Result<TransducerAlphabet, TransducerError> {
        let mut buf = Vec::new();
        for name in &self.names {
            buf.extend_from_slice(name.as_bytes());
            buf.push(0);
        }
        TransducerAlphabetParser::parse(&buf, SymbolNumber(self.names.len() as u16), path)
    }
}

impl Transducer for DhfstTransducer {
    const FILE_EXT: &'static str = "dhfst";

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
        self.final_weight(i).is_some()
    }

    #[inline(always)]
    fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i.0 >= self.layout.n_states {
            return None;
        }
        let (_, _, _, weight) = self.state_record(self.swap_start(i.0));
        weight.is_finite().then_some(Weight(weight))
    }

    #[inline(always)]
    fn for_each_arc<V>(&self, state: TransitionTableIndex, input: SymbolNumber, mut visit: V)
    where
        V: FnMut(SymbolNumber, TransitionTableIndex, Weight),
    {
        self.walk(state.0, input.0, &mut |group: ArcGroup<'_>| match group {
            ArcGroup::One {
                output,
                target,
                weight,
            } => visit(output, target, weight),
            ArcGroup::Each {
                outputs,
                target,
                weight,
            } => {
                for output in outputs.iter() {
                    visit(output, target, weight);
                }
            }
        });
    }

    #[inline(always)]
    fn for_each_arc_group<V>(&self, state: TransitionTableIndex, input: SymbolNumber, mut visit: V)
    where
        V: FnMut(ArcGroup<'_>),
    {
        self.walk(state.0, input.0, &mut visit);
    }

    // The cursor API walks one flat run of arcs per input symbol, which this
    // format does not have: a state's arcs are resolved through its defaults
    // and its fallback chain. The suggestion search reaches an error model
    // only through `for_each_arc` and `for_each_arc_group`; the cursor API
    // answers "no arcs" rather than a partial view.

    #[inline(always)]
    fn transition_input_symbol(&self, _i: TransitionTableIndex) -> Option<SymbolNumber> {
        None
    }

    #[inline(always)]
    fn has_transitions(&self, _i: TransitionTableIndex, _s: Option<SymbolNumber>) -> bool {
        false
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
    fn has_epsilons_or_flags(&self, _i: TransitionTableIndex) -> bool {
        false
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

impl<F: vfs::File> TransducerLoader<F> for DhfstTransducer {
    fn from_path<P, FS>(fs: &FS, path: P) -> Result<DhfstTransducer, TransducerError>
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
        DhfstTransducer::from_mapped_memory(Arc::new(mmap), path.to_path_buf())
    }
}

#[cfg(test)]
mod tests;
