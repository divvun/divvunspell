//! The reader's validation, and the default-arc and fallback semantics against
//! a literal reading of the lookup rule.

use std::path::Path;
use std::sync::Arc;

use super::*;
use crate::transducer::ErrorModel;

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
    for bad in [&b"DHFST\x02\0\0"[..], b"DHFST", b"HFSX\0\0\0\0", b""] {
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
