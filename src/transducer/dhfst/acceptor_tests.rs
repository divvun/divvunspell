//! The acceptor writer end to end, the reader's validation, and the speller
//! with a DHFST acceptor against the same speller with the source.

use std::path::Path;

use super::acceptor::DhfstAcceptor;
use super::acceptor_writer::{AcceptorOptions, Placement, WrittenAcceptor, write_acceptor};
use crate::speller::{HfstSpeller, Speller, SpellerConfig};
use crate::transducer::alphabet::TransducerAlphabet;
use crate::transducer::hfst::alphabet::TransducerAlphabetParser;
use crate::transducer::symbol_transition::SymbolTransition;
use crate::transducer::thfst::MmapThfstTransducer;
use crate::transducer::{Transducer, TransducerLoader};
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
use crate::vfs::Fs;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures")).join(name)
}

fn thfst(name: &str) -> MmapThfstTransducer {
    MmapThfstTransducer::from_path(&Fs, fixture(name)).expect("fixture loads")
}

/// Symbol names of a THFST fixture as its file stores them.
fn names(t: &MmapThfstTransducer) -> Vec<String> {
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

fn options(placement: Placement) -> AcceptorOptions {
    AcceptorOptions {
        placement,
        threads: 1,
        source_name: "fixture".into(),
    }
}

fn written(name: &str, placement: Placement) -> WrittenAcceptor {
    let t = thfst(name);
    write_acceptor(&t, &names(&t), &options(placement)).expect("fixture writes and checks")
}

const LEXICONS: [&str; 5] = [
    "lexicon.thfst",
    "flag-lexicon.thfst",
    "identity-lexicon.thfst",
    "reorder-lexicon.thfst",
    "unknown-out-lexicon.thfst",
];

/// Every fixture lexicon writes, reads back and checks, in either placement
/// order, and writes the same bytes every time.
#[test]
fn fixture_lexicons_write_and_check() {
    for name in LEXICONS {
        for placement in [Placement::FanOut, Placement::DepthFirst] {
            let w = written(name, placement);
            assert_eq!(w.bytes, written(name, placement).bytes, "{name}");
            let reader = DhfstAcceptor::from_bytes(&w.bytes, name).expect("loads");
            let info = reader.info();
            assert_eq!(info.check_bytes, 1, "{name}");
            assert_eq!(info.arcs, w.arcs, "{name}");
            assert_eq!(info.distances, w.distances, "{name}");
            assert_eq!(reader.id_count(), w.ids, "{name}");
            assert_eq!(w.ids_of[0], 0, "{name}: the start state is not 0");
        }
    }
}

/// A lexicon with an arc whose output is not its input is no acceptor, and
/// the writer says so.
#[test]
fn a_transducer_is_refused() {
    let t = thfst("eps-lexicon.thfst");
    assert!(matches!(
        write_acceptor(&t, &names(&t), &options(Placement::DepthFirst)),
        Err(super::writer::WriteError::Unsupported(_))
    ));
}

/// Every state's distance to a final state is the source's, bit for bit;
/// a file stores the distances when the source has any but 0, and answers
/// 0 for every state when it does not.
#[test]
fn distances_are_the_sources() {
    let mut with = 0;
    for name in LEXICONS {
        let t = thfst(name);
        let w = written(name, Placement::DepthFirst);
        let reader = DhfstAcceptor::from_bytes(&w.bytes, name).expect("loads");
        let mut nonzero = false;
        for (q, s) in w.order.iter().enumerate() {
            let want = t.distance_to_final(TransitionTableIndex(*s));
            let got = reader.distance_to_final(TransitionTableIndex(w.ids_of[q]));
            assert_eq!(got.0.to_bits(), want.0.to_bits(), "{name}, state {s}");
            nonzero |= want.0.to_bits() != 0;
        }
        assert_eq!(reader.info().distances, nonzero, "{name}");
        with += usize::from(nonzero);
    }
    assert!(with > 0, "no fixture has a distance other than 0");

    let mut source = synthetic();
    source.distances.clear();
    let w = write_acceptor(&source, &source.names, &options(Placement::DepthFirst))
        .expect("writes and checks");
    let reader = DhfstAcceptor::from_bytes(&w.bytes, "flat").expect("loads");
    assert!(!w.distances && !reader.info().distances);
    for q in 0..reader.id_count() + 2 {
        assert_eq!(
            reader
                .distance_to_final(TransitionTableIndex(q))
                .0
                .to_bits(),
            0
        );
    }
}

/// A lexicon given directly by its states, for alphabets no fixture has.
struct Synthetic {
    alphabet: TransducerAlphabet,
    names: Vec<String>,
    states: Vec<SyntheticState>,
    /// each state's distance to a final state; none for 0 everywhere
    distances: Vec<f32>,
}

/// Arcs are `(symbol, target, weight)`, regular arcs sorted by symbol.
#[derive(Default)]
struct SyntheticState {
    final_weight: Option<f32>,
    free: Vec<(u16, u32, f32)>,
    regular: Vec<(u16, u32, f32)>,
}

impl Synthetic {
    fn state(&self, q: u32) -> Option<&SyntheticState> {
        self.states.get(q as usize)
    }

    fn arc(&self, (symbol, target, weight): (u16, u32, f32)) -> SymbolTransition {
        SymbolTransition::new(
            Some(TransitionTableIndex(target)),
            Some(SymbolNumber(symbol)),
            Some(Weight(weight)),
        )
    }

    /// The least weight of a path from each state to a final state, final
    /// weight included, by relaxing every arc until nothing changes.
    fn shortest_distances(&self) -> Vec<f32> {
        let mut d: Vec<f32> = self
            .states
            .iter()
            .map(|s| s.final_weight.unwrap_or(f32::INFINITY))
            .collect();
        loop {
            let mut changed = false;
            for (q, state) in self.states.iter().enumerate() {
                for (_, target, weight) in state.free.iter().chain(state.regular.iter()) {
                    let through = weight.max(0.0) + d[*target as usize];
                    if through < d[q] {
                        d[q] = through;
                        changed = true;
                    }
                }
            }
            if !changed {
                return d;
            }
        }
    }
}

impl Transducer for Synthetic {
    const FILE_EXT: &'static str = "";
    const CURSOR: bool = false;

    fn alphabet(&self) -> &TransducerAlphabet {
        &self.alphabet
    }

    fn alphabet_mut(&mut self) -> &mut TransducerAlphabet {
        &mut self.alphabet
    }

    fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.state(i.0).is_some_and(|s| s.final_weight.is_some())
    }

    fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        self.state(i.0).and_then(|s| s.final_weight).map(Weight)
    }

    fn distance_to_final(&self, i: TransitionTableIndex) -> Weight {
        Weight(self.distances.get(i.0 as usize).copied().unwrap_or(0.0))
    }

    fn has_transitions(&self, i: TransitionTableIndex, s: Option<SymbolNumber>) -> bool {
        let (Some(q), Some(s)) = (i.0.checked_sub(1), s) else {
            return false;
        };
        self.state(q)
            .is_some_and(|state| state.regular.iter().any(|a| a.0 == s.0))
    }

    fn has_epsilons_or_flags(&self, i: TransitionTableIndex) -> bool {
        i.0.checked_sub(1)
            .and_then(|q| self.state(q))
            .is_some_and(|state| !state.free.is_empty())
    }

    fn free_arcs(
        &self,
        state: TransitionTableIndex,
    ) -> impl Iterator<Item = (SymbolNumber, SymbolTransition)> + '_ {
        self.state(state.0)
            .into_iter()
            .flat_map(|s| s.free.iter())
            .map(|a| (SymbolNumber(a.0), self.arc(*a)))
    }

    fn transitions(
        &self,
        state: TransitionTableIndex,
        input: SymbolNumber,
    ) -> impl Iterator<Item = SymbolTransition> + '_ {
        self.state(state.0)
            .into_iter()
            .flat_map(|s| s.regular.iter())
            .filter(move |a| a.0 == input.0)
            .map(|a| self.arc(*a))
    }

    fn transition_input_symbol(&self, _i: TransitionTableIndex) -> Option<SymbolNumber> {
        None
    }

    fn next(&self, _i: TransitionTableIndex, _s: SymbolNumber) -> Option<TransitionTableIndex> {
        None
    }

    fn take_epsilons_and_flags(&self, _i: TransitionTableIndex) -> Option<SymbolTransition> {
        None
    }

    fn take_epsilons(&self, _i: TransitionTableIndex) -> Option<SymbolTransition> {
        None
    }

    fn take_non_epsilons(
        &self,
        _i: TransitionTableIndex,
        _s: SymbolNumber,
    ) -> Option<SymbolTransition> {
        None
    }
}

/// A lexicon whose letters are numbered past 300 symbols that pad the
/// alphabet, so that its labels need two bytes: words with weights, a start
/// state with an arc on every padding symbol, two arcs on one symbol, a flag
/// pair, an epsilon arc and a state with free and regular arcs. Its states'
/// distances to a final state are their shortest distances.
fn synthetic() -> Synthetic {
    const PADS: u16 = 300;
    let mut names = vec!["@_EPSILON_SYMBOL_@".to_string()];
    names.extend((0..PADS).map(|i| format!("<pad{i}>")));
    names.extend(["c", "a", "t", "r", "e", "ä"].map(String::from));
    names.extend(["@P.F.A@", "@R.F.A@"].map(String::from));
    let symbol = |name: &str| names.iter().position(|n| n == name).expect("named") as u16;
    let mut states: Vec<SyntheticState> = vec![SyntheticState::default()];
    let add = |states: &mut Vec<SyntheticState>| {
        states.push(SyntheticState::default());
        states.len() as u32 - 1
    };

    // A trie of words; the first arc of each word carries its weight.
    for (word, weight) in [
        ("cat", 0.0f32),
        ("car", 1.5),
        ("cart", 2.0),
        ("ca", 1.0),
        ("tac", 0.5),
        ("ät", 3.25),
        ("rät", -3.8e-6),
    ] {
        let mut q = 0u32;
        for (i, ch) in word.chars().enumerate() {
            let s = symbol(&ch.to_string());
            let found = states[q as usize]
                .regular
                .iter()
                .find(|a| a.0 == s)
                .map(|a| a.1);
            q = match found {
                Some(t) => t,
                None => {
                    let t = add(&mut states);
                    let w = if i == 0 { weight } else { 0.0 };
                    states[q as usize].regular.push((s, t, w));
                    t
                }
            };
        }
        states[q as usize].final_weight = Some(weight.abs() / 2.0);
    }
    let walk = |states: &Vec<SyntheticState>, word: &str| {
        word.chars().fold(0u32, |q, ch| {
            let s = symbol(&ch.to_string());
            states[q as usize]
                .regular
                .iter()
                .find(|a| a.0 == s)
                .map(|a| a.1)
                .expect("in the trie")
        })
    };
    let (r, ca) = (walk(&states, "r"), walk(&states, "ca"));
    // A second arc on "c", to "ce" only.
    let c2 = add(&mut states);
    let ce = add(&mut states);
    states[0].regular.push((symbol("c"), c2, 4.0));
    states[c2 as usize].regular.push((symbol("e"), ce, 0.25));
    states[ce as usize].final_weight = Some(0.0);
    // "re" between a flag pair, and an epsilon arc out of a state that
    // also has a regular arc.
    let (p, e, end) = (add(&mut states), add(&mut states), add(&mut states));
    states[r as usize].free.push((symbol("@P.F.A@"), p, 0.0));
    states[p as usize].regular.push((symbol("e"), e, 0.75));
    states[e as usize]
        .free
        .push((symbol("@R.F.A@"), end, 0.125));
    states[end as usize].final_weight = Some(0.0);
    states[r as usize].free.push((0, ce, 6.0));
    // Every padding symbol out of the start state and out of "ca".
    let dead = add(&mut states);
    states[dead as usize].final_weight = Some(9.0);
    for x in 1..=PADS {
        states[0].regular.push((x, dead, x as f32));
        states[ca as usize].regular.push((x, dead, 0.0));
    }
    for state in &mut states {
        state.regular.sort_by_key(|a| a.0);
    }

    let mut buf = Vec::new();
    for name in &names {
        buf.extend_from_slice(name.as_bytes());
        buf.push(0);
    }
    let alphabet = TransducerAlphabetParser::parse(
        &buf,
        SymbolNumber(names.len() as u16),
        Path::new("synthetic"),
    )
    .expect("alphabet parses");
    let mut synthetic = Synthetic {
        alphabet,
        names,
        states,
        distances: Vec::new(),
    };
    synthetic.distances = synthetic.shortest_distances();
    synthetic
}

type Row = (String, u32, Option<bool>, Option<(u32, u32)>);

fn rows(suggestions: Vec<crate::speller::suggestion::Suggestion>) -> Vec<Row> {
    suggestions
        .into_iter()
        .map(|s| {
            (
                s.value.to_string(),
                s.weight.0.to_bits(),
                s.completed,
                s.weight_details
                    .map(|d| (d.lexicon_weight.0.to_bits(), d.mutator_weight.0.to_bits())),
            )
        })
        .collect()
}

const WORDS: [&str; 25] = [
    "cat", "kat", "cet", "car", "cart", "kar", "katt", "cae", "ät", "cät", "cZt", "ct", "catt",
    "ca", "c", "tac", "cxt", "cbt", "abc", "bac", "xab", "cäät", "cöt", "caZ", "re",
];

/// Both search modes, each with the lookahead off and on.
fn configs() -> Vec<SpellerConfig> {
    let mut configs = Vec::new();
    for subsets in [true, false] {
        for lookahead in [false, true] {
            let mut config = SpellerConfig::default();
            config.n_best = None;
            config.mutator_subsets = subsets;
            config.astar_lookahead = lookahead;
            config.verbose = true;
            configs.push(config);
        }
    }
    configs
}

/// Suggestions, acceptance and analyses of every word of [`WORDS`] agree
/// between two spellers.
fn same_answers<A, B>(a: &std::sync::Arc<A>, b: &std::sync::Arc<B>, what: &str)
where
    A: Speller,
    B: Speller,
{
    let mut compared = 0;
    for config in &configs() {
        for word in WORDS {
            let want = rows(a.clone().suggest_with_config(word, config));
            let got = rows(b.clone().suggest_with_config(word, config));
            assert_eq!(
                got, want,
                "{what}, word {word}, lookahead {}",
                config.astar_lookahead
            );
            compared += want.len();
            assert_eq!(
                b.clone().is_correct_with_config(word, config),
                a.clone().is_correct_with_config(word, config),
                "{what}, word {word}"
            );
        }
    }
    assert!(compared > 0, "{what}: nothing to compare");
}

/// A speller whose lexicon is a DHFST acceptor suggests, accepts and
/// analyses exactly what the same speller with the THFST lexicon does, row
/// for row, with the lookahead off and on.
#[test]
fn a_dhfst_lexicon_suggests_the_same() {
    let pairs = [
        ("lexicon.thfst", "mutator.thfst"),
        ("lexicon.thfst", "eps-mutator.thfst"),
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
    for (lexicon, mutator) in pairs {
        for placement in [Placement::FanOut, Placement::DepthFirst] {
            let original = HfstSpeller::new(thfst(mutator), thfst(lexicon));
            let converted = HfstSpeller::new(
                thfst(mutator),
                DhfstAcceptor::from_bytes(&written(lexicon, placement).bytes, lexicon)
                    .expect("loads"),
            );
            same_answers(&original, &converted, &format!("{lexicon} with {mutator}"));
        }
    }
}

/// Labels past 255 take two-byte checks, and such a lexicon writes, checks,
/// and suggests what its source does.
#[test]
fn labels_past_one_byte_take_two_byte_checks() {
    let source = synthetic();
    let w = write_acceptor(&source, &source.names, &options(Placement::DepthFirst))
        .expect("writes and checks");
    let reader = DhfstAcceptor::from_bytes(&w.bytes, "synthetic").expect("loads");
    let info = reader.info();
    assert_eq!(info.check_bytes, 2);
    assert!(info.label_max > 300, "label_max {}", info.label_max);
    assert!(info.free_arcs >= 3 && info.list_records as u64 > info.free_arcs);
    assert!(info.distances);
    let original = HfstSpeller::new(thfst("mutator.thfst"), synthetic());
    let converted = HfstSpeller::new(thfst("mutator.thfst"), reader);
    same_answers(&original, &converted, "synthetic");
}

/// Walk every state and symbol, as a search would.
fn sweep(t: &DhfstAcceptor) -> u64 {
    let mut seen = 0u64;
    let n = t.alphabet().initial_symbol_count().0 as u32 + 2;
    for q in 0..t.id_count() + 2 {
        let state = TransitionTableIndex(q);
        seen += u64::from(t.is_final(state));
        seen += t
            .final_weight(state)
            .map_or(0, |w| w.0.to_bits() as u64 & 1);
        seen += t.distance_to_final(state).0.to_bits() as u64 & 1;
        seen += u64::from(t.has_epsilons_or_flags(TransitionTableIndex(q + 1)));
        for (symbol, arc) in t.free_arcs(state) {
            seen += symbol.0 as u64 + arc.target().map_or(0, |t| t.0 as u64);
        }
        for s in 0..n {
            let symbol = SymbolNumber(s as u16);
            if t.has_transitions(TransitionTableIndex(q + 1), Some(symbol)) {
                for arc in t.transitions(state, symbol) {
                    seen += arc.target().map_or(0, |t| t.0 as u64);
                }
            }
        }
    }
    seen
}

/// A damaged file either refuses to load or, if the damage happens to leave
/// a valid file, answers every question without reading outside it; with
/// one-byte checks and with two, with distances and without.
#[test]
fn damaged_files_load_or_refuse_but_never_misread() {
    let mut flat = synthetic();
    flat.distances.clear();
    let source = synthetic();
    for bytes in [
        written("flag-lexicon.thfst", Placement::FanOut).bytes,
        write_acceptor(&source, &source.names, &options(Placement::DepthFirst))
            .expect("writes")
            .bytes,
        write_acceptor(&flat, &flat.names, &options(Placement::DepthFirst))
            .expect("writes")
            .bytes,
    ] {
        let reader = DhfstAcceptor::from_bytes(&bytes, "intact").expect("loads");
        sweep(&reader);

        let mut loaded = 0;
        let mut refused = 0;
        for at in 0..bytes.len() {
            for flip in [0x01u8, 0x80, 0xFF] {
                let mut damaged = bytes.clone();
                damaged[at] ^= flip;
                match DhfstAcceptor::from_bytes(&damaged, "damaged") {
                    Ok(t) => {
                        sweep(&t);
                        loaded += 1;
                    }
                    Err(_) => refused += 1,
                }
            }
        }
        for len in (0..bytes.len()).step_by(3) {
            assert!(
                DhfstAcceptor::from_bytes(&bytes[..len], "truncated").is_err(),
                "a file cut to {len} bytes loads"
            );
        }
        assert!(
            refused > 0 && loaded > 0,
            "{loaded} loaded, {refused} refused"
        );
    }
}

/// An acceptor is not an error model, and the acceptor reader refuses an
/// error model, a reserved type and HFST optimized lookup.
#[test]
fn the_types_do_not_mix() {
    let bytes = written("lexicon.thfst", Placement::FanOut).bytes;
    assert!(matches!(
        super::DhfstTransducer::from_bytes(&bytes, "acceptor"),
        Err(crate::transducer::TransducerError::WrongDhfstType { .. })
    ));
    let mut other = bytes.clone();
    other[5] = super::DhfstType::ErrorModel.byte();
    assert!(matches!(
        DhfstAcceptor::from_bytes(&other, "error model"),
        Err(crate::transducer::TransducerError::WrongDhfstType { .. })
    ));
    other[5] = 3;
    assert!(matches!(
        DhfstAcceptor::from_bytes(&other, "type 3"),
        Err(crate::transducer::TransducerError::UnrecognisedFormat { .. })
    ));
    let mut hfst = crate::transducer::hfst::header::MAGIC.to_vec();
    hfst.extend_from_slice(&bytes[hfst.len()..]);
    assert!(matches!(
        DhfstAcceptor::from_bytes(&hfst, "hfst"),
        Err(crate::transducer::TransducerError::UnrecognisedFormat { .. })
    ));
}
