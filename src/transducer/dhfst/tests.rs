//! The reader's validation, the default-arc and fallback semantics against a
//! literal reading of the lookup rule, and the writer end to end.

use std::path::Path;
use std::sync::Arc;

use super::writer::{
    ContextSpec, EditStageSpec, SourceArc, SourceModel, SourceState, StageSpec, StagesSpec,
    TableSpec, WriteOptions, write,
};
use super::*;
use crate::speller::{HfstSpeller, Speller, SpellerConfig};
use crate::transducer::thfst::MmapThfstTransducer;
use crate::transducer::{ErrorModel, TransducerLoader};
use crate::vfs::Fs;

/// A file assembled by hand from its parts, without the writer.
#[derive(Clone)]
struct Raw {
    symbols: Vec<String>,
    classes: Vec<Vec<u16>>,
    pairs: Vec<(u16, u16)>,
    states: Vec<RawState>,
    start: u32,
    max_depth: u32,
    flags: Option<u32>,
    extra: Vec<([u8; 4], Vec<u8>)>,
    drop: Vec<[u8; 4]>,
    version: u8,
}

#[derive(Clone)]
struct RawState {
    /// `(input, output, target, weight)`, default records included
    entries: Vec<(u16, u16, u32, f32)>,
    fallback: u32,
    final_weight: f32,
}

impl RawState {
    fn new() -> RawState {
        RawState {
            entries: Vec::new(),
            fallback: NONE,
            final_weight: f32::INFINITY,
        }
    }
}

const ID: u16 = DEFAULT_BASE;
const SUB: u16 = DEFAULT_BASE + 1;
const DEL: u16 = DEFAULT_BASE + 2;
const INS: u16 = DEFAULT_BASE + 3;

impl Raw {
    fn new(symbols: &[&str]) -> Raw {
        Raw {
            symbols: symbols.iter().map(|s| s.to_string()).collect(),
            classes: Vec::new(),
            pairs: Vec::new(),
            states: Vec::new(),
            start: 0,
            max_depth: 0,
            flags: None,
            extra: Vec::new(),
            drop: Vec::new(),
            version: VERSION,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        let mut syms = Vec::new();
        syms.extend_from_slice(&(self.symbols.len() as u32).to_le_bytes());
        let mut off = 0u32;
        let mut blob = Vec::new();
        for s in &self.symbols {
            syms.extend_from_slice(&off.to_le_bytes());
            blob.extend_from_slice(s.as_bytes());
            off += s.len() as u32;
        }
        syms.extend_from_slice(&off.to_le_bytes());
        syms.extend_from_slice(&blob);

        let words = self.symbols.len().div_ceil(64);
        let mut clas = Vec::new();
        clas.extend_from_slice(&(words as u32).to_le_bytes());
        clas.extend_from_slice(&(self.classes.len() as u32).to_le_bytes());
        for c in &self.classes {
            let mut bits = vec![0u64; words];
            for s in c {
                bits[*s as usize / 64] |= 1 << (*s as usize % 64);
            }
            for b in bits {
                clas.extend_from_slice(&b.to_le_bytes());
            }
        }
        let mut cpai = Vec::new();
        cpai.extend_from_slice(&(self.pairs.len() as u32).to_le_bytes());
        cpai.extend_from_slice(&0u32.to_le_bytes());
        for (a, b) in &self.pairs {
            cpai.extend_from_slice(&a.to_le_bytes());
            cpai.extend_from_slice(&b.to_le_bytes());
        }
        let mut stat = Vec::new();
        stat.extend_from_slice(&(self.states.len() as u32).to_le_bytes());
        stat.extend_from_slice(&self.start.to_le_bytes());
        let mut entr_body = Vec::new();
        let mut n = 0u32;
        let mut any_fallback = false;
        let mut any_default = false;
        for s in &self.states {
            stat.extend_from_slice(&n.to_le_bytes());
            stat.extend_from_slice(&(s.entries.len() as u32).to_le_bytes());
            stat.extend_from_slice(&s.fallback.to_le_bytes());
            stat.extend_from_slice(&s.final_weight.to_bits().to_le_bytes());
            any_fallback |= s.fallback != NONE;
            for (i, o, t, w) in &s.entries {
                any_default |= *i >= DEFAULT_BASE;
                entr_body.extend_from_slice(&i.to_le_bytes());
                entr_body.extend_from_slice(&o.to_le_bytes());
                entr_body.extend_from_slice(&t.to_le_bytes());
                entr_body.extend_from_slice(&w.to_bits().to_le_bytes());
                n += 1;
            }
        }
        let mut entr = Vec::new();
        entr.extend_from_slice(&n.to_le_bytes());
        entr.extend_from_slice(&0u32.to_le_bytes());
        entr.extend_from_slice(&entr_body);

        let flags = self.flags.unwrap_or(
            FLAG_TROPICAL
                | if any_fallback { FLAG_FALLBACK } else { 0 }
                | if any_default { FLAG_DEFAULTS } else { 0 },
        );

        let mut sections: Vec<([u8; 4], Vec<u8>)> = vec![
            (tag::SYMS, syms),
            (tag::CLAS, clas),
            (tag::CPAI, cpai),
            (tag::STAT, stat),
            (tag::ENTR, entr),
        ];
        sections.extend(self.extra.iter().cloned());
        sections.retain(|(t, _)| !self.drop.contains(t));

        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.push(self.version);
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&(sections.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.max_depth.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        let mut offset = (HEADER_LEN + SECTION_ENTRY_LEN * sections.len()).div_ceil(8) * 8;
        for (t, body) in &sections {
            out.extend_from_slice(t);
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&(offset as u64).to_le_bytes());
            out.extend_from_slice(&(body.len() as u64).to_le_bytes());
            offset = (offset + body.len()).div_ceil(8) * 8;
        }
        while out.len() % 8 != 0 {
            out.push(0);
        }
        for (_, body) in &sections {
            out.extend_from_slice(body);
            while out.len() % 8 != 0 {
                out.push(0);
            }
        }
        out
    }

    fn load(&self) -> Result<DhfstTransducer, TransducerError> {
        DhfstTransducer::from_bytes(&self.bytes(), "test.dhfst")
    }

    fn regular(&self, s: u16) -> bool {
        is_regular_name(s as usize, &self.symbols[s as usize])
    }

    /// The lookup rule, read literally: explicit entries, then the level's
    /// default of the pair's kind, then the fallback. In stored state
    /// numbers.
    fn resolve(&self, q: u32, x: u16, o: u16) -> Vec<(u32, u32)> {
        let kind = pair_kind(x, o, self.regular(x), self.regular(o));
        let mut s = q;
        loop {
            let st = &self.states[s as usize];
            let explicit: Vec<&(u16, u16, u32, f32)> = st
                .entries
                .iter()
                .filter(|e| e.0 < DEFAULT_BASE && e.0 == x && e.1 == o)
                .collect();
            if !explicit.is_empty() {
                return explicit
                    .iter()
                    .filter(|e| e.2 != NONE)
                    .map(|e| (e.2, e.3.to_bits()))
                    .collect();
            }
            if let Some(kind) = kind {
                for e in st.entries.iter().filter(|e| e.0 >= DEFAULT_BASE) {
                    if DefaultKind::from_record(e.0) != Some(kind) {
                        continue;
                    }
                    let in_span = match kind {
                        DefaultKind::Identity | DefaultKind::Deletion => {
                            self.classes[e.1 as usize].contains(&x)
                        }
                        DefaultKind::Insertion => self.classes[e.1 as usize].contains(&o),
                        DefaultKind::Substitution => {
                            let (a, b) = self.pairs[e.1 as usize];
                            self.classes[a as usize].contains(&x)
                                && self.classes[b as usize].contains(&o)
                        }
                    };
                    if in_span {
                        return vec![(e.2, e.3.to_bits())];
                    }
                }
            }
            if st.fallback == NONE {
                return Vec::new();
            }
            s = st.fallback;
        }
    }

    /// Callers see the stored start as state 0: the two trade numbers.
    fn swap(&self, state: u32) -> u32 {
        if state == self.start {
            0
        } else if state == 0 {
            self.start
        } else {
            state
        }
    }

    /// Every arc of a state as callers number it, on `x`, by the literal
    /// rule, sorted.
    fn expected(&self, external: u32, x: u16) -> Vec<(u16, u32, u32)> {
        let stored = self.swap(external);
        let mut out = Vec::new();
        for o in 0..self.symbols.len() as u16 {
            for (t, w) in self.resolve(stored, x, o) {
                out.push((o, self.swap(t), w));
            }
        }
        out.sort_unstable();
        out
    }
}

fn arcs(t: &DhfstTransducer, q: u32, x: u16) -> Vec<(u16, u32, u32)> {
    let mut out = Vec::new();
    t.for_each_arc(TransitionTableIndex(q), SymbolNumber(x), |o, t, w| {
        out.push((o.0, t.0, w.0.to_bits()))
    });
    out.sort_unstable();
    out
}

/// A model with every kind of entry in it:
///
/// * state 0 (start, final): `a:b` explicit, `a:c` blocked, identity marker
///   explicit, and all four defaults;
/// * state 1 falls back to 0 and adds `b:a`;
/// * state 2 falls back to 1, has a substitution default of its own and
///   blocks `c:d`;
/// * states 3 and 4 are final targets.
fn sampler() -> Raw {
    let mut raw = Raw::new(&[
        "@_EPSILON_SYMBOL_@",
        "@_IDENTITY_SYMBOL_@",
        "a",
        "b",
        "c",
        "d",
    ]);
    raw.classes = vec![vec![2, 3, 4, 5]];
    raw.pairs = vec![(0, 0)];
    let mut s0 = RawState::new();
    s0.entries = vec![
        (1, 1, 0, 0.0),
        (2, 3, 1, 1.0),
        (2, 4, NONE, 0.0),
        (ID, 0, 0, 0.0),
        (SUB, 0, 2, 5.0),
        (DEL, 0, 3, 7.0),
        (INS, 0, 4, 9.0),
    ];
    s0.final_weight = 0.0;
    let mut s1 = RawState::new();
    s1.entries = vec![(3, 2, 2, 2.0)];
    s1.fallback = 0;
    let mut s2 = RawState::new();
    s2.entries = vec![(4, 5, NONE, 0.0), (SUB, 0, 3, 6.0)];
    s2.fallback = 1;
    let mut s3 = RawState::new();
    s3.final_weight = 1.5;
    let mut s4 = RawState::new();
    s4.final_weight = 2.5;
    raw.states = vec![s0, s1, s2, s3, s4];
    raw.max_depth = 2;
    raw
}

#[test]
fn the_header_says_which_format_a_file_is() {
    let path = Path::new("x");
    assert_eq!(
        TransducerFormat::detect(b"HFST\0\x10\0\0", path).ok(),
        Some(TransducerFormat::Hfst)
    );
    assert_eq!(
        TransducerFormat::detect(b"DHFST\x01\0\0", path).ok(),
        Some(TransducerFormat::Dhfst { version: 1 })
    );
    for bad in [
        &b"DHFST\x02\0\0"[..],
        b"DHFST\x00",
        b"DHFST",
        b"HFSX\0\0\0\0",
        b"",
    ] {
        assert!(
            matches!(
                TransducerFormat::detect(bad, path),
                Err(TransducerError::UnrecognisedFormat { .. })
            ),
            "{bad:?} was recognised"
        );
    }
}

#[test]
fn a_dhfst_file_is_never_read_as_optimized_lookup() {
    let bytes = sampler().bytes();
    let parsed = crate::transducer::hfst::header::TransducerHeader::parse(&bytes, Path::new("x"));
    assert!(matches!(
        parsed,
        Err(TransducerError::UnrecognisedFormat { .. })
    ));

    let mut map = memmap2::MmapMut::map_anon(bytes.len()).expect("anonymous map");
    map.copy_from_slice(&bytes);
    let map = Arc::new(map.make_read_only().expect("read-only map"));
    assert!(crate::transducer::hfst::HfstTransducer::from_mapped_memory(map.clone(), "x").is_err());
    assert!(matches!(
        ErrorModel::from_mapped_memory(map, "x"),
        Ok(ErrorModel::Dhfst(_))
    ));
}

#[test]
fn a_well_formed_file_loads() {
    let t = sampler().load().expect("the sampler is well formed");
    assert_eq!(t.state_count(), 5);
    assert_eq!(t.class_count(), 1);
    assert_eq!(t.max_fallback_depth(), 2);
    // The alphabet is the one an HFST file with these symbols gives.
    assert_eq!(t.alphabet().key_table()[0], "");
    assert_eq!(t.alphabet().identity(), Some(SymbolNumber(1)));
    assert_eq!(
        t.alphabet().string_to_symbol().get("c").copied(),
        Some(SymbolNumber(4))
    );
}

#[test]
fn corrupt_files_are_refused() {
    let good = sampler();
    good.load().expect("the sampler is well formed");

    let mut cases: Vec<(&str, Raw)> = Vec::new();
    let mut r = good.clone();
    r.version = 2;
    cases.push(("unknown version", r));
    let mut r = good.clone();
    r.flags = Some(FLAG_TROPICAL | FLAG_FALLBACK | FLAG_DEFAULTS | FLAG_STAGES);
    cases.push(("stages flag without a STAG section", r));
    let mut r = good.clone();
    r.drop.push(tag::STAT);
    cases.push(("no STAT", r));
    let mut r = good.clone();
    r.drop.push(tag::CLAS);
    cases.push(("defaults without CLAS", r));
    let mut r = good.clone();
    r.extra.push((*b"XTRA", vec![0; 8]));
    cases.push(("unknown critical section", r));
    let mut r = good.clone();
    r.extra.push((tag::RULE, vec![0; 8]));
    cases.push(("RULE section", r));
    let mut r = good.clone();
    r.flags = Some(FLAG_TROPICAL | FLAG_FALLBACK | FLAG_DEFAULTS | FLAG_RULES);
    cases.push(("RULE flag", r));
    let mut r = good.clone();
    r.flags = Some(FLAG_TROPICAL | FLAG_DEFAULTS);
    cases.push(("undeclared fallbacks", r));
    let mut r = good.clone();
    r.flags = Some(FLAG_TROPICAL | FLAG_FALLBACK);
    cases.push(("undeclared defaults", r));
    let mut r = good.clone();
    r.flags = Some(FLAG_FALLBACK | FLAG_DEFAULTS);
    cases.push(("weights not tropical", r));
    let mut r = good.clone();
    r.states[1].entries[0].2 = 9;
    cases.push(("target out of range", r));
    let mut r = good.clone();
    r.states[1].entries[0].0 = 9;
    cases.push(("input out of range", r));
    let mut r = good.clone();
    r.classes[0].push(1);
    cases.push(("identity marker in a class", r));
    let mut r = good.clone();
    r.classes[0].push(0);
    cases.push(("epsilon in a class", r));
    let mut r = good.clone();
    r.states[0].fallback = 2;
    cases.push(("fallback cycle", r));
    let mut r = good.clone();
    r.states[3].fallback = 3;
    cases.push(("fallback to itself", r));
    let mut r = good.clone();
    r.max_depth = 1;
    cases.push(("chain deeper than declared", r));
    let mut r = good.clone();
    r.states[0].entries.swap(1, 2);
    cases.push(("unsorted entries", r));
    let mut r = good.clone();
    r.states[0].entries.insert(2, (2, 4, 1, 1.0));
    cases.push(("blocker sharing its pair", r));
    let mut r = good.clone();
    r.states[0].entries.swap(3, 4);
    cases.push(("defaults out of order", r));
    let mut r = good.clone();
    r.states[0].entries.push((SUB, 0, 2, 5.0));
    cases.push(("two defaults of a kind", r));
    let mut r = good.clone();
    r.states[0].entries.push((1, 1, 0, 0.0));
    cases.push(("explicit entry after a default", r));
    let mut r = good.clone();
    r.states[0].entries[5].1 = 3;
    cases.push(("missing class", r));
    let mut r = good.clone();
    r.states[0].entries[4].1 = 3;
    cases.push(("missing class pair", r));
    let mut r = good.clone();
    r.states[0].entries[4].0 = DEFAULT_BASE + 7;
    cases.push(("reserved record kind", r));
    let mut r = good.clone();
    r.start = 5;
    cases.push(("start out of range", r));
    let mut r = good.clone();
    r.states[3].final_weight = f32::NAN;
    cases.push(("NaN final weight", r));
    let mut r = good.clone();
    r.symbols[0] = "x".into();
    cases.push(("symbol 0 not epsilon", r));

    for (name, raw) in cases {
        assert!(raw.load().is_err(), "{name}: a corrupt file loaded");
    }

    let mut bytes = good.bytes();
    bytes.truncate(bytes.len() - 16);
    assert!(
        DhfstTransducer::from_bytes(&bytes, "x").is_err(),
        "a truncated file loaded"
    );
    let mut bytes = good.bytes();
    bytes[0] = b'X';
    assert!(
        DhfstTransducer::from_bytes(&bytes, "x").is_err(),
        "bad magic loaded"
    );
    let mut bytes = good.bytes();
    bytes[6] = 1;
    assert!(
        DhfstTransducer::from_bytes(&bytes, "x").is_err(),
        "a reserved header byte was ignored"
    );

    let mut ancillary = good.clone();
    ancillary.extra.push((*b"xtra", vec![1, 2, 3]));
    assert!(
        ancillary.load().is_ok(),
        "an unknown ancillary section must be skipped"
    );
}

#[test]
fn defaults_answer_what_explicit_entries_leave() {
    let t = sampler().load().expect("the sampler is well formed");
    let w = |x: f32| x.to_bits();

    // a at state 0: the identity default gives a:a; a:b is explicit, which
    // takes b out of the substitution default and nothing else; a:c is
    // blocked; a:d comes from the substitution default; a:ε from the
    // deletion default.
    assert_eq!(
        arcs(&t, 0, 2),
        vec![
            (0, 3, w(7.0)),
            (2, 0, w(0.0)),
            (3, 1, w(1.0)),
            (5, 2, w(5.0))
        ]
    );

    // The substitution default reaches a caller as one group, holding only
    // what nothing else answered.
    let mut groups = Vec::new();
    t.for_each_arc_group(TransitionTableIndex(0), SymbolNumber(2), |g| {
        if let ArcGroup::Each {
            outputs,
            target,
            weight,
        } = g
        {
            groups.push((
                outputs.iter().map(|s| s.0).collect::<Vec<_>>(),
                target.0,
                weight.0,
            ));
        }
    });
    assert_eq!(groups, vec![(vec![5], 2, 5.0)]);

    // Epsilon input at state 0: the insertion default, all four letters.
    assert_eq!(
        arcs(&t, 0, 0),
        vec![
            (2, 4, w(9.0)),
            (3, 4, w(9.0)),
            (4, 4, w(9.0)),
            (5, 4, w(9.0))
        ]
    );

    // b at state 1: its own b:a; everything else from state 0.
    assert_eq!(
        arcs(&t, 1, 3),
        vec![
            (0, 3, w(7.0)),
            (2, 2, w(2.0)),
            (3, 0, w(0.0)),
            (4, 2, w(5.0)),
            (5, 2, w(5.0))
        ]
    );

    // c at state 2: its own substitution default gives c:a and c:b, its
    // blocker hides c:d (which state 0's default would have given), and
    // identity and deletion come from state 0, two levels down.
    assert_eq!(
        arcs(&t, 2, 4),
        vec![
            (0, 3, w(7.0)),
            (2, 3, w(6.0)),
            (3, 3, w(6.0)),
            (4, 0, w(0.0))
        ]
    );

    // a at state 2: the first level with an entry or a default for a pair
    // answers it, so state 2's substitution default answers a:b, a:c and
    // a:d ahead of state 0's explicit a:b and blocked a:c.
    assert_eq!(
        arcs(&t, 2, 2),
        vec![
            (0, 3, w(7.0)),
            (2, 0, w(0.0)),
            (3, 3, w(6.0)),
            (4, 3, w(6.0)),
            (5, 3, w(6.0))
        ]
    );

    // The identity marker takes explicit entries only, here two levels down.
    assert_eq!(arcs(&t, 2, 1), vec![(1, 0, w(0.0))]);

    // Finality is never inherited.
    assert_eq!(t.final_weight(TransitionTableIndex(0)), Some(Weight(0.0)));
    assert_eq!(t.final_weight(TransitionTableIndex(1)), None);
    assert_eq!(t.final_weight(TransitionTableIndex(2)), None);
    assert!(!t.is_final(TransitionTableIndex(2)));
    assert_eq!(t.final_weight(TransitionTableIndex(3)), Some(Weight(1.5)));
}

#[test]
fn the_stored_start_state_is_state_zero_to_callers() {
    let mut raw = sampler();
    // Store state 0 as 3 and state 3 as 0, and say the start is at 3.
    let trade = |s: u32| match s {
        0 => 3,
        3 => 0,
        s => s,
    };
    raw.states.swap(0, 3);
    for s in &mut raw.states {
        for e in &mut s.entries {
            e.2 = trade(e.2);
        }
        s.fallback = trade(s.fallback);
    }
    raw.start = 3;
    let swapped = raw.load().expect("well formed");
    let original = sampler().load().expect("well formed");
    for q in 0..5 {
        for x in 0..6 {
            assert_eq!(
                arcs(&swapped, q, x),
                arcs(&original, q, x),
                "state {q} input {x}"
            );
        }
        assert_eq!(
            swapped.final_weight(TransitionTableIndex(q)),
            original.final_weight(TransitionTableIndex(q))
        );
    }
}

/// A small xorshift generator, so the random tests need no dependency and
/// replay the same models every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, n: u64) -> bool {
        self.below(n) == 0
    }
    fn weight(&mut self) -> f32 {
        [0.0, 1.0, 2.5, 10.0][self.below(4) as usize]
    }
}

fn random_raw(rng: &mut Rng) -> Raw {
    let mut raw = Raw::new(&[
        "@_EPSILON_SYMBOL_@",
        "@_IDENTITY_SYMBOL_@",
        "@_UNKNOWN_SYMBOL_@",
        "a",
        "b",
        "c",
        "d",
        "e",
    ]);
    for _ in 0..4 {
        let mut c: Vec<u16> = (3..8u16).filter(|_| rng.chance(2)).collect();
        if c.is_empty() {
            c.push(3);
        }
        raw.classes.push(c);
    }
    for _ in 0..3 {
        raw.pairs.push((rng.below(4) as u16, rng.below(4) as u16));
    }
    let n = 1 + rng.below(6) as u32;
    let mut depth = vec![0u32; n as usize];
    for q in 0..n {
        let mut st = RawState::new();
        let mut explicit: Vec<(u16, u16, u32, f32)> = Vec::new();
        for _ in 0..rng.below(8) {
            let (x, o) = (rng.below(8) as u16, rng.below(8) as u16);
            if explicit.iter().any(|e| (e.0, e.1) == (x, o)) {
                continue;
            }
            if rng.chance(5) {
                explicit.push((x, o, NONE, 0.0));
            } else {
                for _ in 0..1 + rng.below(2) {
                    let e = (x, o, rng.below(n as u64) as u32, rng.weight());
                    if !explicit.contains(&e) {
                        explicit.push(e);
                    }
                }
            }
        }
        explicit.sort_by(|a, b| {
            (a.0, a.1, a.2)
                .cmp(&(b.0, b.1, b.2))
                .then(a.3.total_cmp(&b.3))
        });
        st.entries = explicit;
        for kind in [ID, SUB, DEL, INS] {
            if rng.chance(2) {
                let class = if kind == SUB {
                    rng.below(3) as u16
                } else {
                    rng.below(4) as u16
                };
                st.entries
                    .push((kind, class, rng.below(n as u64) as u32, rng.weight()));
            }
        }
        if q > 0 && rng.chance(2) {
            st.fallback = rng.below(q as u64) as u32;
            depth[q as usize] = depth[st.fallback as usize] + 1;
        }
        if rng.chance(3) {
            st.final_weight = rng.weight();
        }
        raw.states.push(st);
    }
    raw.max_depth = depth.iter().copied().max().unwrap_or(0);
    raw.start = rng.below(n as u64) as u32;
    raw
}

#[test]
fn random_models_answer_as_the_lookup_rule_says() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for round in 0..500 {
        let raw = random_raw(&mut rng);
        let t = raw.load().unwrap_or_else(|e| panic!("round {round}: {e}"));
        for q in 0..raw.states.len() as u32 {
            for x in 0..raw.symbols.len() as u16 {
                let want = raw.expected(q, x);
                assert_eq!(arcs(&t, q, x), want, "round {round}, state {q}, input {x}");

                // Grouped: the same arcs, each output once, no group empty.
                let mut grouped = Vec::new();
                t.for_each_arc_group(TransitionTableIndex(q), SymbolNumber(x), |g| match g {
                    ArcGroup::One {
                        output,
                        target,
                        weight,
                    } => grouped.push((output.0, target.0, weight.0.to_bits())),
                    ArcGroup::Each {
                        outputs,
                        target,
                        weight,
                    } => {
                        assert!(!outputs.is_empty(), "round {round}: an empty group");
                        for o in outputs.iter() {
                            grouped.push((o.0, target.0, weight.0.to_bits()));
                        }
                    }
                });
                grouped.sort_unstable();
                assert_eq!(grouped, want, "round {round}: groups differ");
            }
            let stored_final = raw.states[raw.swap(q) as usize].final_weight;
            assert_eq!(
                t.final_weight(TransitionTableIndex(q))
                    .map(|w| w.0.to_bits()),
                stored_final.is_finite().then_some(stored_final.to_bits()),
                "round {round}, state {q}: final weight"
            );
        }
    }
}

fn random_source(rng: &mut Rng) -> SourceModel {
    let symbols: Vec<String> = [
        "@_EPSILON_SYMBOL_@",
        "@_IDENTITY_SYMBOL_@",
        "@_UNKNOWN_SYMBOL_@",
        "a",
        "b",
        "c",
        "d",
        "e",
        "f",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let n = 1 + rng.below(12) as u32;
    // Fans of one target and weight, so that defaults have something to
    // find.
    let mut states = Vec::new();
    for _ in 0..n {
        let mut arcs = Vec::new();
        let fan_target = rng.below(n as u64) as u32;
        let fan_weight = rng.weight();
        for x in 3..9u16 {
            for o in 3..9u16 {
                if x != o && !rng.chance(6) {
                    arcs.push(SourceArc {
                        input: x,
                        output: o,
                        target: fan_target,
                        weight: fan_weight,
                    });
                }
            }
            if !rng.chance(4) {
                arcs.push(SourceArc {
                    input: x,
                    output: x,
                    target: 0,
                    weight: 0.0,
                });
            }
            if rng.chance(2) {
                arcs.push(SourceArc {
                    input: x,
                    output: 0,
                    target: rng.below(n as u64) as u32,
                    weight: rng.weight(),
                });
            }
        }
        for _ in 0..rng.below(6) {
            arcs.push(SourceArc {
                input: rng.below(9) as u16,
                output: rng.below(9) as u16,
                target: rng.below(n as u64) as u32,
                weight: rng.weight(),
            });
        }
        let final_weight = rng.chance(3).then(|| rng.weight());
        states.push(SourceState { final_weight, arcs });
    }
    // Near-copies of other states' rows, which is what fallbacks are for.
    for q in 1..n as usize {
        if rng.chance(2) {
            let from = rng.below(q as u64) as usize;
            let mut arcs = states[from].arcs.clone();
            arcs.retain(|_| !rng.chance(10));
            states[q].arcs = arcs;
        }
    }
    SourceModel::new(symbols, states).expect("the random model is valid")
}

#[test]
fn the_writer_writes_what_it_was_given() {
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    for round in 0..200 {
        let model = random_source(&mut rng);
        for depth in [Some(0), Some(1), Some(4), None] {
            let options = WriteOptions {
                max_fallback_depth: depth,
                threads: 2,
                source_name: "random".into(),
                stages: None,
            };
            // `write` checks every (state, pair) of the encoding and every
            // (state, input) of the bytes against the model.
            let written = write(&model, &options)
                .unwrap_or_else(|e| panic!("round {round}, depth {depth:?}: {e}"));
            assert!(
                written.report.max_depth <= depth.unwrap_or(u32::MAX),
                "round {round}: chain deeper than the bound"
            );
            let again = write(&model, &options).expect("second write");
            assert_eq!(
                written.bytes, again.bytes,
                "round {round}: not deterministic"
            );
        }
    }
}

#[test]
fn the_writer_refuses_what_it_cannot_hold() {
    let symbols: Vec<String> = ["@_EPSILON_SYMBOL_@", "a"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let arc = |target, weight| SourceArc {
        input: 1,
        output: 1,
        target,
        weight,
    };
    let state = |arcs| SourceState {
        final_weight: None,
        arcs,
    };
    assert!(SourceModel::new(symbols.clone(), vec![state(vec![arc(1, 0.0)])]).is_err());
    assert!(SourceModel::new(symbols.clone(), vec![state(vec![arc(0, f32::NAN)])]).is_err());
    assert!(SourceModel::new(symbols.clone(), Vec::new()).is_err());
    assert!(SourceModel::new(vec!["a".into()], vec![state(Vec::new())]).is_err());
}

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures")).join(name)
}

/// Symbol names of a THFST fixture as its file stores them.
fn thfst_names(t: &MmapThfstTransducer) -> Vec<String> {
    t.alphabet()
        .key_table()
        .iter()
        .enumerate()
        .map(|(i, k)| {
            if i == 0 && k.is_empty() {
                "@_EPSILON_SYMBOL_@".to_string()
            } else {
                k.to_string()
            }
        })
        .collect()
}

pub(crate) fn dhfst_from_fixture(name: &str) -> Vec<u8> {
    let thfst = MmapThfstTransducer::from_path(&Fs, fixture(name)).expect("fixture loads");
    let model = SourceModel::from_transducer(&thfst, thfst_names(&thfst)).expect("fixture reads");
    write(
        &model,
        &WriteOptions {
            threads: 2,
            ..WriteOptions::default()
        },
    )
    .expect("fixture writes")
    .bytes
}

type Row = (String, u32, Option<bool>, Option<(u32, u32, u32, u32, u32)>);

fn rows(suggestions: Vec<crate::speller::suggestion::Suggestion>) -> Vec<Row> {
    suggestions
        .into_iter()
        .map(|s| {
            (
                s.value.to_string(),
                s.weight.0.to_bits(),
                s.completed,
                s.weight_details.map(|d| {
                    (
                        d.lexicon_weight.0.to_bits(),
                        d.mutator_weight.0.to_bits(),
                        d.reweight_start.to_bits(),
                        d.reweight_mid.to_bits(),
                        d.reweight_end.to_bits(),
                    )
                }),
            )
        })
        .collect()
}

/// A speller whose error model was converted to DHFST suggests exactly what
/// the original suggests, walking the model as an NFA (where default arcs
/// reach the search as groups) and with the subset construction.
#[test]
fn a_converted_error_model_suggests_the_same() {
    let pairs = [
        ("lexicon.thfst", "mutator.thfst"),
        ("eps-lexicon.thfst", "eps-mutator.thfst"),
        ("flag-lexicon.thfst", "flag-mutator.thfst"),
        ("identity-lexicon.thfst", "identity-mutator.thfst"),
        ("reorder-lexicon.thfst", "reorder-mutator.thfst"),
        (
            "unknown-out-lexicon.thfst",
            "unknown-out-compact-mutator.thfst",
        ),
        (
            "unknown-out-lexicon.thfst",
            "unknown-out-expanded-mutator.thfst",
        ),
        ("lexicon.thfst", "wildcard-compact-mutator.thfst"),
        ("lexicon.thfst", "wildcard-expanded-mutator.thfst"),
    ];
    let words = [
        "cat", "kat", "cet", "car", "cart", "kar", "katt", "cae", "ät", "cät", "cZt", "ct", "catt",
        "ca", "c", "tac", "cxt", "cbt", "abc", "bac", "xab", "cäät", "cöt", "caZ", "re",
    ];
    let mut configs = Vec::new();
    for subsets in [true, false] {
        let mut config = SpellerConfig::default();
        config.n_best = None;
        config.mutator_subsets = subsets;
        config.verbose = true;
        configs.push(config);
    }

    for (lexicon, mutator) in pairs {
        let original = HfstSpeller::new(
            MmapThfstTransducer::from_path(&Fs, fixture(mutator)).expect("mutator loads"),
            MmapThfstTransducer::from_path(&Fs, fixture(lexicon)).expect("lexicon loads"),
        );
        let converted = HfstSpeller::new(
            DhfstTransducer::from_bytes(&dhfst_from_fixture(mutator), mutator)
                .expect("written file loads"),
            MmapThfstTransducer::from_path(&Fs, fixture(lexicon)).expect("lexicon loads"),
        );
        let mut compared = 0;
        for config in &configs {
            for word in words {
                let want = rows(original.clone().suggest_with_config(word, config));
                let got = rows(converted.clone().suggest_with_config(word, config));
                assert_eq!(
                    got, want,
                    "{mutator} with {lexicon}, subsets {}, word {word}",
                    config.mutator_subsets
                );
                compared += want.len();
            }
        }
        assert!(compared > 0, "{mutator}: no suggestions to compare");
    }
}

/// The converted fixture models do carry default arcs and fallbacks, so the
/// parity above exercises them.
#[test]
fn converted_fixtures_use_defaults() {
    let t = DhfstTransducer::from_bytes(&dhfst_from_fixture("mutator.thfst"), "mutator")
        .expect("loads");
    assert!(t.flags() & FLAG_DEFAULTS != 0);
}

/// Symbols of the stage tests: the fixture lexicon's letters, and for the
/// staged model a call pair past them.
fn stage_symbols(with_calls: bool) -> Vec<String> {
    let mut symbols: Vec<String> = ["@_EPSILON_SYMBOL_@", "c", "a", "t", "r", "e"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    if with_calls {
        symbols.push("@DHFST_CALL_1_IN@".into());
        symbols.push("@DHFST_CALL_1_OUT@".into());
    }
    symbols
}

const LETTERS: [u16; 5] = [1, 2, 3, 4, 5];

/// At most one edit anywhere: substitution 5 (a:e 2), deletion 7 (t 3),
/// insertion 9, transposition 4 starting at `a` or `r`.
fn edit_table() -> TableSpec {
    let mut table = TableSpec {
        target: 1,
        ..TableSpec::default()
    };
    for &x in &LETTERS {
        for &y in &LETTERS {
            if x != y {
                table
                    .sub
                    .push((x, y, if (x, y) == (2, 5) { 2.0 } else { 5.0 }));
            }
        }
        table.del.push((x, if x == 3 { 3.0 } else { 7.0 }));
        table.ins.push((x, 9.0));
        if x == 2 || x == 4 {
            for &y in &LETTERS {
                table.swap.push((x, y, 4.0));
            }
        }
    }
    table
}

/// The same relation stored: identity loops round an edit.
fn stored_edits() -> SourceModel {
    let table = edit_table();
    let mut states = vec![
        SourceState {
            final_weight: Some(0.0),
            arcs: Vec::new(),
        },
        SourceState {
            final_weight: Some(0.0),
            arcs: Vec::new(),
        },
    ];
    for &x in &LETTERS {
        for q in 0..2u32 {
            states[q as usize].arcs.push(SourceArc {
                input: x,
                output: x,
                target: q,
                weight: 0.0,
            });
        }
    }
    for &(x, y, w) in &table.sub {
        states[0].arcs.push(SourceArc {
            input: x,
            output: y,
            target: 1,
            weight: w,
        });
    }
    for &(x, w) in &table.del {
        states[0].arcs.push(SourceArc {
            input: x,
            output: 0,
            target: 1,
            weight: w,
        });
    }
    for &(y, w) in &table.ins {
        states[0].arcs.push(SourceArc {
            input: 0,
            output: y,
            target: 1,
            weight: w,
        });
    }
    let mut pending: Vec<(u16, u32)> = Vec::new();
    for &(x, y, w) in &table.swap {
        let m1 = match pending.iter().find(|p| p.0 == x) {
            Some(p) => p.1,
            None => {
                let m1 = states.len() as u32;
                states.push(SourceState::default());
                states.push(SourceState::default());
                states[0].arcs.push(SourceArc {
                    input: x,
                    output: 0,
                    target: m1,
                    weight: 0.0,
                });
                states[m1 as usize + 1].arcs.push(SourceArc {
                    input: 0,
                    output: x,
                    target: 1,
                    weight: 0.0,
                });
                pending.push((x, m1));
                m1
            }
        };
        states[m1 as usize].arcs.push(SourceArc {
            input: y,
            output: y,
            target: m1 + 1,
            weight: w,
        });
    }
    SourceModel::new(stage_symbols(false), states).expect("valid model")
}

/// The same relation as one call arc into an edit-table stage.
fn staged_edits() -> (SourceModel, StagesSpec) {
    let model = SourceModel::new(
        stage_symbols(true),
        vec![
            SourceState {
                final_weight: None,
                arcs: vec![SourceArc {
                    input: 6,
                    output: 7,
                    target: 1,
                    weight: 0.0,
                }],
            },
            SourceState {
                final_weight: Some(0.0),
                arcs: Vec::new(),
            },
        ],
    )
    .expect("valid model");
    let stage = EditStageSpec {
        contexts: vec![
            ContextSpec {
                final_weight: Some(0.0),
                ident: vec![(LETTERS.to_vec(), 0)],
                table: Some(0),
            },
            ContextSpec {
                final_weight: Some(0.0),
                ident: vec![(LETTERS.to_vec(), 1)],
                table: None,
            },
        ],
        start: 0,
        tables: vec![edit_table()],
    };
    (
        model,
        StagesSpec {
            n_alphabet: 6,
            stages: vec![StageSpec {
                call_input: 6,
                call_output: 7,
                table: stage,
            }],
        },
    )
}

#[test]
fn an_edit_table_stage_suggests_what_the_stored_edits_suggest() {
    let options = WriteOptions {
        threads: 1,
        ..WriteOptions::default()
    };
    let stored = write(&stored_edits(), &options).expect("stored model writes");
    let (model, stages) = staged_edits();
    let staged = write(
        &model,
        &WriteOptions {
            stages: Some(stages),
            ..options.clone()
        },
    )
    .expect("staged model writes and checks");
    assert!(staged.report.stage_queries_checked > 0);
    let staged_t = DhfstTransducer::from_bytes(&staged.bytes, "staged").expect("loads");
    assert_eq!(staged_t.version(), VERSION);
    assert_eq!(staged_t.alphabet_len(), 6);
    assert_eq!(
        staged_t.alphabet().key_table().len(),
        6,
        "call symbols are not alphabet"
    );

    let lexicon = || MmapThfstTransducer::from_path(&Fs, fixture("lexicon.thfst")).expect("loads");
    let a = HfstSpeller::new(
        DhfstTransducer::from_bytes(&stored.bytes, "stored").expect("loads"),
        lexicon(),
    );
    let b = HfstSpeller::new(staged_t, lexicon());
    let mut compared = 0;
    for subsets in [true, false] {
        let mut config = SpellerConfig::default();
        config.n_best = None;
        config.mutator_subsets = subsets;
        config.verbose = true;
        for word in [
            "cat", "cet", "cta", "acr", "ca", "catt", "car", "cra", "caer", "crae", "tac", "ccat",
            "re",
        ] {
            let want = rows(a.clone().suggest_with_config(word, &config));
            let got = rows(b.clone().suggest_with_config(word, &config));
            assert_eq!(got, want, "word {word}, subsets {subsets}");
            compared += want.len();
        }
    }
    assert!(compared > 10, "too few suggestions to compare");
}

#[test]
fn corrupt_stages_are_refused() {
    let (model, stages) = staged_edits();
    let bytes = write(
        &model,
        &WriteOptions {
            threads: 1,
            stages: Some(stages.clone()),
            ..WriteOptions::default()
        },
    )
    .expect("writes")
    .bytes;
    DhfstTransducer::from_bytes(&bytes, "x").expect("the written file loads");

    // The stages flag cleared while the file has a STAG section.
    let mut b = bytes.clone();
    b[8] &= !(FLAG_STAGES as u8);
    assert!(
        DhfstTransducer::from_bytes(&b, "x").is_err(),
        "a STAG section without the stages flag loaded"
    );

    // A call symbol inside the alphabet.
    let mut bad = stages.clone();
    bad.stages[0].call_input = 3;
    assert!(
        write(
            &model,
            &WriteOptions {
                threads: 1,
                stages: Some(bad),
                ..WriteOptions::default()
            }
        )
        .is_err(),
        "a call symbol inside the alphabet was written"
    );

    // Every byte of the STAG section flipped in turn: never a panic, and a
    // load that succeeds must still answer every query.
    let reader = DhfstTransducer::from_bytes(&bytes, "x").expect("loads");
    let stag = {
        let n = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
        (0..n)
            .map(|s| HEADER_LEN + SECTION_ENTRY_LEN * s)
            .find(|at| bytes[*at..*at + 4] == tag::STAG)
            .map(|at| {
                let off = u64::from_le_bytes(bytes[at + 8..at + 16].try_into().expect("8"));
                let len = u64::from_le_bytes(bytes[at + 16..at + 24].try_into().expect("8"));
                (off as usize, len as usize)
            })
            .expect("a STAG section")
    };
    drop(reader);
    for at in stag.0..stag.0 + stag.1 {
        let mut b = bytes.clone();
        b[at] ^= 0x5a;
        if let Ok(t) = DhfstTransducer::from_bytes(&b, "x") {
            let n = t.state_count();
            for q in 0..n + 64 {
                for x in 0..8u16 {
                    t.for_each_arc(TransitionTableIndex(q), SymbolNumber(x), |_, _, _| {});
                }
            }
        }
    }
}

/// Exactly one edit, as the stored relation and as a weight-pushed stage: the
/// call arc carries the least edit cost, the cells what is left, and a
/// transposition its row's least cost when it starts.
#[test]
fn a_pushed_stage_suggests_what_the_stored_edits_suggest() {
    let mut stored = stored_edits();
    let mut states: Vec<SourceState> = stored.states().to_vec();
    states[0].final_weight = None;
    stored = SourceModel::new(stage_symbols(false), states).expect("valid");

    let table = edit_table();
    let swap_min = 4.0f32;
    let m = table
        .sub
        .iter()
        .map(|c| c.2)
        .chain(table.del.iter().map(|c| c.1))
        .chain(table.ins.iter().map(|c| c.1))
        .chain(std::iter::once(swap_min))
        .fold(f32::INFINITY, f32::min);
    let pushed = TableSpec {
        target: 1,
        sub: table.sub.iter().map(|&(x, y, w)| (x, y, w - m)).collect(),
        del: table.del.iter().map(|&(x, w)| (x, w - m)).collect(),
        ins: table.ins.iter().map(|&(x, w)| (x, w - m)).collect(),
        swap: table
            .swap
            .iter()
            .map(|&(x, y, w)| (x, y, w - swap_min))
            .collect(),
        swap_entry: vec![(2, swap_min - m), (4, swap_min - m)],
    };
    let (mut model, mut stages) = staged_edits();
    let mut states: Vec<SourceState> = model.states().to_vec();
    states[0].arcs[0].weight = m;
    model = SourceModel::new(stage_symbols(true), states).expect("valid");
    stages.stages[0].table.contexts[0].final_weight = None;
    stages.stages[0].table.tables = vec![pushed];

    let options = WriteOptions {
        threads: 1,
        ..WriteOptions::default()
    };
    let a_bytes = write(&stored, &options).expect("writes").bytes;
    let b_bytes = write(
        &model,
        &WriteOptions {
            stages: Some(stages),
            ..options
        },
    )
    .expect("writes and checks")
    .bytes;
    let lexicon = || MmapThfstTransducer::from_path(&Fs, fixture("lexicon.thfst")).expect("loads");
    let a = HfstSpeller::new(
        DhfstTransducer::from_bytes(&a_bytes, "a").expect("loads"),
        lexicon(),
    );
    let b = HfstSpeller::new(
        DhfstTransducer::from_bytes(&b_bytes, "b").expect("loads"),
        lexicon(),
    );
    let mut compared = 0;
    for subsets in [true, false] {
        let mut config = SpellerConfig::default();
        config.n_best = None;
        config.mutator_subsets = subsets;
        config.verbose = true;
        for word in [
            "cat", "cet", "cta", "acr", "ca", "catt", "car", "cra", "caer", "crae", "tac", "re",
        ] {
            let want = rows(a.clone().suggest_with_config(word, &config));
            let got = rows(b.clone().suggest_with_config(word, &config));
            assert_eq!(got, want, "word {word}, subsets {subsets}");
            compared += want.len();
        }
    }
    assert!(compared > 10, "too few suggestions to compare");
}
