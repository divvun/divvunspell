//! Instructions per lexicon operation, for comparing formats.
//!
//! usage: acceptor_ops (thfst DIR | dhfst FILE) WORDS MODE ROUNDS [MIN-MAX]
//!
//! Collects the states a walk along each word's letters reaches (free arcs
//! taken as epsilons), keeps those with MIN to MAX arcs when given (free arcs
//! and regular arcs on symbols 1 to 511), then runs one kind of operation
//! over them ROUNDS times: `base` (the loop alone), `probe_lo`
//! (has_transitions for symbols 1 to 255), `probe_call` (the same through a
//! function that is not inlined, so that nothing about the lexicon is kept
//! from one probe to the next, as in the search), `probe_hi`
//! (has_transitions for symbols 256 to 511), `probe_one` (has_transitions
//! for one symbol per state), `probe_hit` and `probe_miss` (has_transitions
//! as a call of its own, for the symbols 1 to 255 on which a state has arcs,
//! and for those on which it has none; each distinct state once), `arcs`
//! (the arcs of each such hit, through `transitions`, as a call of its own;
//! `base_hit` and `base_miss` are the loops over the pairs alone, to subtract
//! from these three),
//! `free` (the free arcs) or `final` (is_final, and final_weight when final).
//! Run each mode under `/usr/bin/time -l` and subtract `base` at the same
//! ROUNDS; the last line printed is the number of operations. Every mode,
//! `base` too, first sorts the pairs `probe_hit` and `probe_miss` use.

use std::collections::BTreeSet;
use std::path::Path;

use divvun_fst::transducer::dhfst::acceptor::DhfstAcceptor;
use divvun_fst::transducer::thfst::MmapThfstTransducer;
use divvun_fst::transducer::{Transducer, TransducerLoader};
use divvun_fst::types::{SymbolNumber, TransitionTableIndex};
use divvun_fst::vfs::Fs;

fn closure<T: Transducer>(t: &T, states: &mut BTreeSet<u32>) {
    let mut stack: Vec<u32> = states.iter().copied().collect();
    while let Some(s) = stack.pop() {
        for (_, tr) in t.free_arcs(TransitionTableIndex(s)) {
            if let Some(target) = tr.target()
                && states.insert(target.0)
            {
                stack.push(target.0);
            }
        }
    }
}

fn visited<T: Transducer>(t: &T, words: &[String]) -> Vec<u32> {
    let alphabet = t.alphabet();
    let mut out = Vec::new();
    for word in words {
        let mut states = BTreeSet::from([0u32]);
        closure(t, &mut states);
        for ch in word.chars() {
            out.extend(states.iter().copied());
            let Some(sym) = alphabet
                .string_to_symbol()
                .get(ch.to_string().as_str())
                .copied()
            else {
                break;
            };
            let mut next = BTreeSet::new();
            for &s in &states {
                for tr in t.transitions(TransitionTableIndex(s), sym) {
                    if let Some(target) = tr.target() {
                        next.insert(target.0);
                    }
                }
            }
            closure(t, &mut next);
            states = next;
            if states.is_empty() {
                break;
            }
        }
    }
    out
}

/// The arcs of state `s`: free, and regular on symbols 1 to 511.
fn arcs<T: Transducer>(t: &T, s: u32) -> usize {
    let q = TransitionTableIndex(s);
    t.free_arcs(q).count()
        + (1..512u16)
            .map(|x| t.transitions(q, SymbolNumber(x)).count())
            .sum::<usize>()
}

/// The states of `visited` with `min` to `max` arcs.
fn filtered<T: Transducer>(t: &T, words: &[String], range: Option<(usize, usize)>) -> Vec<u32> {
    let states = visited(t, words);
    match range {
        None => states,
        Some((min, max)) => states
            .into_iter()
            .filter(|&s| (min..=max).contains(&arcs(t, s)))
            .collect(),
    }
}

/// Calls `op` on every state, `rounds` times; `op` returns how many
/// operations it made and adds what it read to the sink.
#[inline(always)]
fn each(states: &[u32], rounds: usize, mut op: impl FnMut(u32, &mut u64) -> u64) -> u64 {
    let mut ops = 0u64;
    let mut sink = 0u64;
    for _ in 0..rounds {
        for &s in states {
            ops += op(s, &mut sink);
        }
    }
    std::hint::black_box(sink);
    ops
}

/// A state and a symbol.
type Pair = (u32, u16);

/// Calls `op` on every pair, `rounds` times; returns how many calls it made.
#[inline(always)]
fn each_pair(pairs: &[Pair], rounds: usize, op: impl Fn(u32, u16) -> u64) -> u64 {
    let mut sink = 0u64;
    for _ in 0..rounds {
        for &(s, x) in pairs {
            sink = sink.wrapping_add(op(s, x));
        }
    }
    std::hint::black_box(sink);
    (rounds * pairs.len()) as u64
}

/// One probe, as a call of its own.
#[inline(never)]
fn probe_once<T: Transducer>(t: &T, next: TransitionTableIndex, x: SymbolNumber) -> bool {
    t.has_transitions(next, Some(x))
}

/// The arcs of one state on one symbol, as a call of its own.
#[inline(never)]
fn arcs_once<T: Transducer>(t: &T, state: TransitionTableIndex, x: SymbolNumber) -> u64 {
    t.transitions(state, x)
        .map(|tr| tr.target().map_or(0, |t| t.0 as u64))
        .sum()
}

/// The `(state, symbol)` pairs, each distinct state of `states` once and
/// symbols 1 to 255, on which a state has arcs, and those on which it has
/// none.
fn pairs<T: Transducer>(t: &T, states: &[u32]) -> (Vec<Pair>, Vec<Pair>) {
    let distinct: BTreeSet<u32> = states.iter().copied().collect();
    let (mut hits, mut misses) = (Vec::new(), Vec::new());
    for &s in &distinct {
        for x in 1..256u16 {
            if t.has_transitions(TransitionTableIndex(s + 1), Some(SymbolNumber(x))) {
                hits.push((s, x));
            } else {
                misses.push((s, x));
            }
        }
    }
    (hits, misses)
}

#[inline(never)]
fn run<T: Transducer>(t: &T, states: &[u32], mode: &str, rounds: usize) -> u64 {
    let (hits, misses) = pairs(t, states);
    let probe = |s: u32, symbols: std::ops::Range<u16>, sink: &mut u64| {
        let next = TransitionTableIndex(s + 1);
        let n = symbols.len() as u64;
        for x in symbols {
            *sink += u64::from(t.has_transitions(next, Some(SymbolNumber(x))));
        }
        n
    };
    let call = |s: u32, symbols: std::ops::Range<u16>, sink: &mut u64| {
        let next = TransitionTableIndex(s + 1);
        let n = symbols.len() as u64;
        for x in symbols {
            *sink += u64::from(probe_once(t, next, SymbolNumber(x)));
        }
        n
    };
    match mode {
        "base" => each(states, rounds, |s, sink| {
            *sink = sink.wrapping_add(std::hint::black_box(s as u64));
            1
        }),
        "probe_lo" => each(states, rounds, |s, sink| probe(s, 1..256, sink)),
        "probe_call" => each(states, rounds, |s, sink| call(s, 1..256, sink)),
        "probe_hi" => each(states, rounds, |s, sink| probe(s, 256..512, sink)),
        "base_hit" => each_pair(&hits, rounds, |s, x| s as u64 ^ x as u64),
        "base_miss" => each_pair(&misses, rounds, |s, x| s as u64 ^ x as u64),
        "probe_hit" | "probe_miss" => each_pair(
            if mode == "probe_hit" { &hits } else { &misses },
            rounds,
            |s, x| u64::from(probe_once(t, TransitionTableIndex(s + 1), SymbolNumber(x))),
        ),
        "arcs" => each_pair(&hits, rounds, |s, x| {
            arcs_once(t, TransitionTableIndex(s), SymbolNumber(x))
        }),
        "probe_one" => each(states, rounds, |s, sink| {
            let x = (s % 190 + 1) as u16;
            probe(s, x..x + 1, sink)
        }),
        "free" => each(states, rounds, |s, sink| {
            for (symbol, tr) in t.free_arcs(TransitionTableIndex(s)) {
                *sink = sink.wrapping_add(symbol.0 as u64 + tr.target().map_or(0, |t| t.0 as u64));
            }
            1
        }),
        "final" => each(states, rounds, |s, sink| {
            let q = TransitionTableIndex(s);
            if t.is_final(q) {
                *sink = sink.wrapping_add(t.final_weight(q).map_or(0, |w| w.0.to_bits() as u64));
            }
            1
        }),
        _ => panic!("unknown mode {mode}"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let words: Vec<String> = std::fs::read_to_string(&args[3])
        .expect("words")
        .lines()
        .map(|l| l.split('\t').next().unwrap_or("").to_string())
        .collect();
    let mode = args[4].as_str();
    let rounds: usize = args[5].parse().expect("rounds");
    let range = args.get(6).map(|r| {
        let (min, max) = r.split_once('-').expect("MIN-MAX");
        (min.parse().expect("MIN"), max.parse().expect("MAX"))
    });
    let (states, ops) = match args[1].as_str() {
        "thfst" => {
            let t = MmapThfstTransducer::from_path(&Fs, Path::new(&args[2])).expect("load");
            let states = filtered(&t, &words, range);
            let ops = run(&t, &states, mode, rounds);
            (states.len(), ops)
        }
        _ => {
            let t = DhfstAcceptor::from_path(&Fs, Path::new(&args[2])).expect("load");
            let states = filtered(&t, &words, range);
            let ops = run(&t, &states, mode, rounds);
            (states.len(), ops)
        }
    };
    println!("{mode}: {states} states");
    println!("{ops}");
}
