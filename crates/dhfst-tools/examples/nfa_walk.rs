//! Least weight of an input:output pair through an error model, walking it
//! the way the suggestion search does (arc groups, epsilon moves), without a
//! lexicon. For checking a model's relation as the search sees it.
//!
//! usage: cargo run --example nfa_walk -- MODEL.dhfst INPUT OUTPUT

use std::collections::{BinaryHeap, HashMap};

use divvun_fst::transducer::dhfst::DhfstTransducer;
use divvun_fst::transducer::{ArcGroup, Transducer, TransducerLoader};
use divvun_fst::types::{SymbolNumber, TransitionTableIndex};
use divvun_fst::vfs::Fs;

#[derive(PartialEq)]
struct Item(f32, u32, usize, usize);
impl Eq for Item {}
impl PartialOrd for Item {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Item {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.0.total_cmp(&self.0)
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let t = DhfstTransducer::from_path(&Fs, &args[1])?;
    let sym = |s: &str| -> anyhow::Result<u16> {
        t.alphabet()
            .string_to_symbol()
            .get(s)
            .map(|n| n.0)
            .ok_or_else(|| anyhow::anyhow!("no symbol {s}"))
    };
    let input: Vec<u16> = args[2]
        .chars()
        .map(|c| sym(&c.to_string()))
        .collect::<anyhow::Result<_>>()?;
    let output: Vec<u16> = args[3]
        .chars()
        .map(|c| sym(&c.to_string()))
        .collect::<anyhow::Result<_>>()?;
    let mut best: HashMap<(u32, usize, usize), f32> = HashMap::new();
    let mut heap = BinaryHeap::new();
    heap.push(Item(0.0, 0, 0, 0));
    while let Some(Item(w, q, i, j)) = heap.pop() {
        if best.get(&(q, i, j)).is_some_and(|b| *b < w) {
            continue;
        }
        let at = TransitionTableIndex(q);
        if i == input.len() && j == output.len() && t.is_final(at) {
            println!(
                "{} -> {}: {}",
                args[2],
                args[3],
                w + t.final_weight(at).map_or(0.0, |f| f.0)
            );
            return Ok(());
        }
        let mut push = |w2: f32, q2: u32, i2: usize, j2: usize, heap: &mut BinaryHeap<Item>| {
            if best.get(&(q2, i2, j2)).is_none_or(|b| w2 < *b) {
                best.insert((q2, i2, j2), w2);
                heap.push(Item(w2, q2, i2, j2));
            }
        };
        let mut moves: Vec<(u16, u32, f32, usize)> = Vec::new();
        for (x, di) in [(0u16, 0usize)]
            .into_iter()
            .chain(input.get(i).map(|x| (*x, 1usize)))
        {
            t.for_each_arc_group(at, SymbolNumber(x), |g| match g {
                ArcGroup::One {
                    output,
                    target,
                    weight,
                } => moves.push((output.0, target.0, weight.0, di)),
                ArcGroup::Each {
                    outputs,
                    target,
                    weight,
                } => {
                    for o in outputs.iter() {
                        moves.push((o.0, target.0, weight.0, di));
                    }
                }
            });
        }
        for (o, q2, aw, di) in moves {
            if o == 0 {
                push(w + aw, q2, i + di, j, &mut heap);
            } else if output.get(j) == Some(&o) {
                push(w + aw, q2, i + di, j + 1, &mut heap);
            }
        }
    }
    println!("{} -> {}: no path", args[2], args[3]);
    Ok(())
}
