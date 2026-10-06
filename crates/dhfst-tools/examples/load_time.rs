//! Time opening a speller archive and its first lookups.
//!
//! usage: load_time ARCHIVE WORD ROUNDS
//!
//! Each round opens the archive (which loads and, for a DHFST acceptor,
//! validates it), then checks WORD once and asks for its suggestions twice:
//! the second search is the first that uses the lexicon's least arc weight,
//! which a THFST lexicon finds by reading its whole transition table.

use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (archive, word) = (&args[1], &args[2]);
    let rounds: usize = args[3].parse().expect("rounds");
    let mut open = Vec::new();
    let mut first = Vec::new();
    let mut second = Vec::new();
    for _ in 0..rounds {
        let t = Instant::now();
        let a = divvun_fst::archive::open(archive).expect("archive opens");
        let speller = a.speller();
        open.push(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        std::hint::black_box(speller.clone().is_correct(word));
        std::hint::black_box(speller.clone().suggest(word));
        first.push(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        std::hint::black_box(speller.clone().suggest(word));
        second.push(t.elapsed().as_secs_f64() * 1e3);
    }
    let median = |v: &mut Vec<f64>| {
        v.sort_by(|a, b| a.total_cmp(b));
        v[v.len() / 2]
    };
    println!(
        "open {:.2} ms, first check+suggest {:.2} ms, second suggest {:.2} ms (medians of {rounds}; first open {:.2} ms)",
        median(&mut open.clone()),
        median(&mut first),
        median(&mut second),
        open[0]
    );
}
