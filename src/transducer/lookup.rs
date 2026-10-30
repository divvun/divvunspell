//! Lookup: every output a transducer gives a word, with its weight.
//!
//! This is what an analyser or a generator is asked, and what
//! [`Speller::analyze_input`](crate::speller::Speller::analyze_input) asks a
//! speller's lexicon, without a speller around it. The word is read as
//! `hfst-optimized-lookup` reads it, by longest match: at each position the
//! longest input symbol whose text the rest of the word starts with, so that
//! a generator's tags such as `+N` are read as the symbols they are. Where no
//! symbol matches, the next grapheme cluster is read as the transducer's
//! `@_UNKNOWN_SYMBOL_@`, and without one the word has no path. Flag
//! diacritics are never read from the word.
//!
//! The walk follows every path: epsilon arcs, flag diacritic arcs, evaluated
//! as the speller evaluates them, and arcs on the next input symbol. A path's
//! weight is the sum of its arc weights and the final weight of the state it
//! ends in. A path that comes back to a state it has been in, at the same
//! input position with the same flag values, is not followed round again: an
//! epsilon cycle is walked once and left, and every output a path without
//! such a loop writes is found.

use hashbrown::HashSet;
use smol_str::SmolStr;
use unic_segment::Graphemes;

use super::Transducer;
use super::symbol_transition::SymbolTransition;
use super::tree_node::{NodeTables, TreeNode};
use crate::types::{InputIndex, SymbolNumber, Weight};

/// One output of a [`lookup`], and the weight of the paths that write it.
#[derive(Clone, Debug, PartialEq)]
pub struct LookupResult {
    /// The output: the text of [`symbols`](Self::symbols), in order.
    pub output: SmolStr,
    /// The output symbols the path writes, by their text. Epsilons and flag
    /// diacritics are not written; an `@_IDENTITY_SYMBOL_@` output writes the
    /// input symbol it read.
    pub symbols: Vec<SmolStr>,
    /// The sum of the path's arc weights and its final weight.
    pub weight: Weight,
}

/// Every output `transducer` gives `word`, cheapest first, then by output.
///
/// No two results have the same output and weight; one output may come at
/// several weights. See the [module documentation](self) for how the word is
/// read and walked. A [`Lookup`] does the same and keeps its tables from one
/// word to the next.
pub fn lookup<T: Transducer>(transducer: &T, word: &str) -> Vec<LookupResult> {
    Lookup::new().lookup(transducer, word)
}

/// The tables a lookup works in, kept from one lookup to the next so that
/// they keep their capacity. One serves any transducer, one word at a time.
#[derive(Default)]
pub struct Lookup {
    tables: Option<NodeTables>,
    input: Vec<SymbolNumber>,
    /// The nodes of the path being walked, root first.
    path: Vec<Frame>,
    /// The `(state, input position, flag state)` of every node of `path`,
    /// which no two of them share.
    on_path: HashSet<(u32, u32, u32)>,
    /// Nodes still to walk, each with how many nodes of `path` lead to it.
    pending: Vec<Pending>,
    /// The output symbols of the path being walked.
    output: Vec<SymbolNumber>,
    found: HashSet<(Vec<SymbolNumber>, u32)>,
}

struct Frame {
    node: TreeNode,
    /// How much of the output the path to this node has written.
    output_len: usize,
}

struct Pending {
    /// How many nodes of the path lead to this one.
    depth: usize,
    node: TreeNode,
    /// The symbol the arc to this node writes, epsilon for none.
    output: SymbolNumber,
}

impl Lookup {
    /// Empty tables.
    pub fn new() -> Lookup {
        Lookup::default()
    }

    /// Every output `transducer` gives `word`, as [`lookup`] has them.
    pub fn lookup<T: Transducer>(&mut self, transducer: &T, word: &str) -> Vec<LookupResult> {
        let alphabet = transducer.alphabet();
        if !self.read(transducer, word) {
            return Vec::new();
        }

        let width = alphabet.state_size().0 as usize;
        let tables = match self.tables.take() {
            Some(mut tables) => {
                tables.reset(width);
                tables
            }
            None => NodeTables::new(width),
        };
        self.path.clear();
        self.on_path.clear();
        self.output.clear();
        self.found.clear();
        self.pending.clear();
        self.pending.push(Pending {
            depth: 0,
            node: TreeNode::empty(),
            output: SymbolNumber::ZERO,
        });

        let identity = alphabet.identity();
        let operations = alphabet.operations();
        let written = |symbol: SymbolNumber| {
            if alphabet.is_flag(symbol) {
                SymbolNumber::ZERO
            } else {
                symbol
            }
        };

        while let Some(Pending {
            depth,
            node,
            output,
        }) = self.pending.pop()
        {
            while self.path.len() > depth {
                if let Some(frame) = self.path.pop() {
                    self.on_path.remove(&place(&frame.node));
                }
            }
            self.output
                .truncate(self.path.last().map_or(0, |frame| frame.output_len));
            if !self.on_path.insert(place(&node)) {
                continue;
            }
            if output != SymbolNumber::ZERO {
                self.output.push(output);
            }
            self.path.push(Frame {
                node,
                output_len: self.output.len(),
            });
            let depth = self.path.len();
            let state = node.lexicon_state;
            let at = node.input_state.0 as usize;

            if at == self.input.len()
                && transducer.is_final(state)
                && let Some(final_weight) = transducer.final_weight(state)
            {
                // -0.0 is 0.0, so that the two are one result.
                let weight = match node.weight + final_weight {
                    weight if weight.0 == 0.0 => Weight::ZERO,
                    weight => weight,
                };
                self.found.insert((self.output.clone(), weight.0.to_bits()));
            }

            let pending = &mut self.pending;
            transducer.for_each_arc(state, SymbolNumber::ZERO, |symbol, target, weight| {
                pending.push(Pending {
                    depth,
                    node: TreeNode {
                        lexicon_state: target,
                        weight: node.weight + weight,
                        ..node
                    },
                    output: written(symbol),
                });
            });

            for (symbol, transition) in transducer.free_arcs(state) {
                if symbol == SymbolNumber::ZERO || !well_formed(&transition) {
                    continue;
                }
                let Some(operation) = operations.get(&symbol) else {
                    continue;
                };
                if let Some(next) = node.apply_operation(&tables, operation, &transition) {
                    pending.push(Pending {
                        depth,
                        node: next,
                        output: SymbolNumber::ZERO,
                    });
                }
            }

            if let Some(&input) = self.input.get(at) {
                let next_input = InputIndex(node.input_state.0 + 1);
                transducer.for_each_arc(state, input, |symbol, target, weight| {
                    let symbol = if Some(symbol) == identity {
                        input
                    } else {
                        symbol
                    };
                    pending.push(Pending {
                        depth,
                        node: TreeNode {
                            lexicon_state: target,
                            input_state: next_input,
                            weight: node.weight + weight,
                            ..node
                        },
                        output: written(symbol),
                    });
                });
            }
        }
        self.tables = Some(tables);

        let keys = alphabet.key_table();
        let mut results: Vec<LookupResult> = self
            .found
            .drain()
            .map(|(symbols, weight)| {
                let symbols: Vec<SmolStr> = symbols
                    .iter()
                    .map(|symbol| keys.get(symbol.0 as usize).cloned().unwrap_or_default())
                    .collect();
                LookupResult {
                    output: symbols.iter().map(|symbol| symbol.as_str()).collect(),
                    symbols,
                    weight: Weight(f32::from_bits(weight)),
                }
            })
            .collect();
        results.sort_by(|a, b| {
            a.weight
                .0
                .total_cmp(&b.weight.0)
                .then_with(|| a.output.cmp(&b.output))
                .then_with(|| a.symbols.cmp(&b.symbols))
        });
        results.dedup_by(|later, earlier| {
            later.output == earlier.output && later.weight.0.total_cmp(&earlier.weight.0).is_eq()
        });
        results
    }

    /// Read `word` into `self.input` by longest match; false when the word
    /// reaches text that no input symbol matches and the transducer has no
    /// unknown symbol to read it as.
    ///
    /// The speller reads its input differently, one grapheme cluster per
    /// symbol (`to_input_vec_lexicon`), and is meant to: lookup reads as
    /// `hfst-optimized-lookup` does so that a generator's multi-character
    /// tags are read, and its results are that tool's.
    ///
    /// Like that tool, the reading is settled before the walk: where the
    /// longest match leads nowhere and a shorter symbol would have, the word
    /// has no path rather than being read again another way. A symbol a
    /// speller added to its lexicon's alphabet, from its error model's, is not
    /// one of the lexicon's own and is never matched.
    fn read<T: Transducer>(&mut self, transducer: &T, word: &str) -> bool {
        let alphabet = transducer.alphabet();
        self.input.clear();
        let mut rest = word;
        while !rest.is_empty() {
            let (symbol, len) = match alphabet.longest_input_symbol(rest) {
                Some(found) => found,
                None => {
                    let Some(unknown) = alphabet.unknown() else {
                        return false;
                    };
                    let cluster = Graphemes::new(rest).next().unwrap_or(rest);
                    (unknown, cluster.len())
                }
            };
            self.input.push(symbol);
            rest = &rest[len..];
        }
        true
    }
}

/// Where a node is: its transducer state, its input position and its flag
/// state. A path that comes back to a place it has been is in a cycle that
/// reads nothing.
fn place(node: &TreeNode) -> (u32, u32, u32) {
    (node.lexicon_state.0, node.input_state.0, node.flags)
}

/// Whether a transition leads somewhere at some weight, as every arc a
/// transducer hands out does.
fn well_formed(transition: &SymbolTransition) -> bool {
    transition.target().is_some() && transition.weight().is_some()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::transducer::TransducerLoader;
    use crate::transducer::hfst::HfstTransducer;
    use crate::transducer::hfst::test_support::{OlState, ol_bytes_with};
    use crate::transducer::thfst::BytesThfstTransducer;
    use crate::vfs::bytes::Filesystem;

    const SYMBOLS: [&str; 18] = [
        "@_EPSILON_SYMBOL_@",
        "@_UNKNOWN_SYMBOL_@",
        "@P.CASE.NOM@",
        "@P.CASE.ACC@",
        "@R.CASE.NOM@",
        "@D.CASE.NOM@",
        "c",
        "a",
        "t",
        "s",
        "+N",
        "+Sg",
        "+Pl",
        "e\u{301}",
        "X",
        "+Nom",
        "+NotNom",
        "x",
    ];
    const EPS: u16 = 0;
    const UNKNOWN: u16 = 1;
    const P_NOM: u16 = 2;
    const P_ACC: u16 = 3;
    const R_NOM: u16 = 4;
    const D_NOM: u16 = 5;
    const C: u16 = 6;
    const A: u16 = 7;
    const T: u16 = 8;
    const S: u16 = 9;
    const N: u16 = 10;
    const SG: u16 = 11;
    const PL: u16 = 12;
    const E_ACUTE: u16 = 13;
    const BIG_X: u16 = 14;
    const NOM: u16 = 15;
    const NOT_NOM: u16 = 16;
    const X: u16 = 17;

    /// An analyser with weights, epsilons, multi-character tags, flag
    /// diacritics, an unknown symbol, a symbol of two characters that is one
    /// grapheme cluster, epsilon cycles and paths that write the same output.
    fn states() -> Vec<OlState> {
        vec![
            // 0: the start.
            (
                None,
                vec![
                    (C, C, 1, 0.25),
                    (P_NOM, P_NOM, 7, 0.0),
                    (P_ACC, P_ACC, 7, 0.0),
                    (X, X, 13, 0.0),
                    (E_ACUTE, E_ACUTE, 15, 0.5),
                    (T, T, 16, 0.0),
                    (S, S, 18, 0.0),
                ],
            ),
            // 1-6: cat, cat+N+Sg, cat+N+Pl.
            (None, vec![(A, A, 2, 0.0)]),
            (None, vec![(T, T, 3, 0.0)]),
            (Some(2.0), vec![(EPS, N, 4, 0.5)]),
            (None, vec![(EPS, SG, 5, 0.0), (S, PL, 6, 1.0)]),
            (Some(1.5), vec![]),
            (Some(0.0), vec![]),
            // 7-12: a+Nom when CASE is NOM, a+NotNom when it is not.
            (None, vec![(A, A, 8, 0.0)]),
            (None, vec![(R_NOM, R_NOM, 9, 0.0), (D_NOM, D_NOM, 11, 0.0)]),
            (None, vec![(EPS, NOM, 10, 0.0)]),
            (Some(0.0), vec![]),
            (None, vec![(EPS, NOT_NOM, 12, 0.0)]),
            (Some(0.0), vec![]),
            // 13-14: x then any symbol outside the alphabet, written X.
            (None, vec![(UNKNOWN, BIG_X, 14, 1.0)]),
            (Some(0.0), vec![]),
            // 15: e with a combining acute.
            (Some(0.0), vec![]),
            // 16-17: t, then a loop writing x and a cycle of two epsilons.
            (Some(0.5), vec![(EPS, X, 16, 1.0), (EPS, EPS, 17, 0.0)]),
            (Some(0.25), vec![(EPS, EPS, 16, 0.0)]),
            // 18-21: s, by two paths of the same weight.
            (None, vec![(EPS, EPS, 19, 0.5), (EPS, EPS, 20, 0.5)]),
            (None, vec![(EPS, EPS, 21, 0.0)]),
            (None, vec![(EPS, EPS, 21, 0.0)]),
            (Some(0.0), vec![]),
        ]
    }

    fn analyser(weighted: bool) -> HfstTransducer {
        HfstTransducer::from_bytes(&ol_bytes_with(&SYMBOLS, &states(), weighted), "test.hfstol")
            .expect("the fixture loads")
    }

    fn found<T: Transducer>(transducer: &T, word: &str) -> Vec<(String, f32)> {
        lookup(transducer, word)
            .into_iter()
            .map(|r| (r.output.to_string(), r.weight.0))
            .collect()
    }

    fn pairs(expected: &[(&str, f32)]) -> Vec<(String, f32)> {
        expected.iter().map(|(o, w)| (o.to_string(), *w)).collect()
    }

    #[test]
    fn weights_add_up_along_the_path_and_with_the_final_weight() {
        let t = analyser(true);
        assert_eq!(
            found(&t, "cat"),
            pairs(&[("cat", 2.25), ("cat+N+Sg", 2.25)])
        );
        assert_eq!(found(&t, "cats"), pairs(&[("cat+N+Pl", 1.75)]));
        assert_eq!(found(&t, "ca"), pairs(&[]));
    }

    #[test]
    fn results_carry_their_output_symbols() {
        let results = lookup(&analyser(true), "cats");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbols, ["c", "a", "t", "+N", "+Pl"]);
        assert_eq!(results[0].output, "cat+N+Pl");
    }

    #[test]
    fn flag_diacritics_gate_paths_and_write_nothing() {
        assert_eq!(
            found(&analyser(true), "a"),
            pairs(&[("a+Nom", 0.0), ("a+NotNom", 0.0)])
        );
    }

    #[test]
    fn a_grapheme_outside_the_alphabet_reads_as_the_unknown_symbol() {
        let t = analyser(true);
        assert_eq!(found(&t, "xq"), pairs(&[("xX", 1.0)]));
        assert_eq!(found(&t, "xé"), pairs(&[("xX", 1.0)]));
        let without_unknown = HfstTransducer::from_bytes(
            &ol_bytes_with(
                &["@_EPSILON_SYMBOL_@", "a"],
                &[(None, vec![(1, 1, 1, 0.0)]), (Some(0.0), vec![])],
                true,
            ),
            "t.hfstol",
        )
        .expect("loads");
        assert_eq!(found(&without_unknown, "a"), pairs(&[("a", 0.0)]));
        assert_eq!(found(&without_unknown, "q"), pairs(&[]));
    }

    #[test]
    fn a_symbol_of_several_characters_is_read_whole() {
        let t = analyser(true);
        assert_eq!(found(&t, "e\u{301}"), pairs(&[("e\u{301}", 0.5)]));
        assert_eq!(
            found(&t, "\u{e9}"),
            pairs(&[]),
            "the precomposed é is another symbol"
        );
    }

    /// Symbols `a`, `b` and `ab` on the input side, `+N` and `+Sg` for a
    /// generator, and `+X`, which only an output carries.
    fn tokens() -> HfstTransducer {
        let symbols = ["@_EPSILON_SYMBOL_@", "a", "b", "ab", "c", "+N", "+Sg", "+X"];
        let mut bytes = ol_bytes_with(
            &symbols,
            &[
                (None, vec![(1, 1, 1, 0.0), (3, 7, 3, 0.0), (5, 5, 5, 0.0)]),
                (None, vec![(2, 2, 2, 0.0)]),
                (Some(0.0), vec![]),
                (None, vec![(4, 4, 4, 0.0)]),
                (Some(0.0), vec![]),
                (None, vec![(6, 6, 6, 0.0), (1, 1, 6, 0.0)]),
                (Some(0.0), vec![]),
            ],
            true,
        );
        // Seven symbols are on the input side; `+X` is not.
        bytes[34..36].copy_from_slice(&7u16.to_le_bytes());
        HfstTransducer::from_bytes(&bytes, "tokens.hfstol").expect("loads")
    }

    #[test]
    fn a_generators_tags_are_read_by_longest_match() {
        let t = tokens();
        assert_eq!(found(&t, "+N+Sg"), pairs(&[("+N+Sg", 0.0)]));
        assert_eq!(found(&t, "+Na"), pairs(&[("+Na", 0.0)]));
        assert_eq!(found(&t, "abc"), pairs(&[("+Xc", 0.0)]));
    }

    #[test]
    fn the_longest_match_is_kept_where_it_leads_nowhere() {
        // `ab` is read as the one symbol `ab`, as hfst-optimized-lookup reads
        // it, though `a` then `b` would have been a path.
        assert_eq!(found(&tokens(), "ab"), pairs(&[]));
    }

    #[test]
    fn a_symbol_only_outputs_carry_is_never_read() {
        assert_eq!(found(&tokens(), "+X"), pairs(&[]));
        assert!(tokens().alphabet().longest_input_symbol("+X").is_none());
        assert_eq!(
            tokens().alphabet().longest_input_symbol("+Sgx"),
            Some((SymbolNumber(6), 3))
        );
    }

    #[test]
    fn a_flag_diacritic_is_never_read_from_the_word() {
        assert_eq!(found(&analyser(true), "@P.CASE.NOM@a"), pairs(&[]));
    }

    #[test]
    fn an_epsilon_cycle_is_walked_once() {
        assert_eq!(
            found(&analyser(true), "t"),
            pairs(&[("t", 0.25), ("t", 0.5)])
        );
    }

    #[test]
    fn two_paths_writing_one_output_at_one_weight_give_one_result() {
        assert_eq!(found(&analyser(true), "s"), pairs(&[("s", 0.5)]));
    }

    #[test]
    fn the_empty_word_is_read_by_paths_that_read_nothing() {
        assert_eq!(found(&analyser(true), ""), pairs(&[]));
        let accepts_nothing = HfstTransducer::from_bytes(
            &ol_bytes_with(
                &["@_EPSILON_SYMBOL_@", "+Empty"],
                &[(None, vec![(0, 1, 1, 0.5)]), (Some(0.25), vec![])],
                true,
            ),
            "t.hfstol",
        )
        .expect("loads");
        assert_eq!(found(&accepts_nothing, ""), pairs(&[("+Empty", 0.75)]));
    }

    #[test]
    fn an_identity_output_writes_the_input_symbol() {
        let symbols = ["@_EPSILON_SYMBOL_@", "@_IDENTITY_SYMBOL_@", "a"];
        let t = HfstTransducer::from_bytes(
            &ol_bytes_with(
                &symbols,
                &[
                    (None, vec![(2, 1, 1, 0.0), (1, 1, 1, 0.0)]),
                    (Some(0.0), vec![]),
                ],
                true,
            ),
            "t.hfstol",
        )
        .expect("loads");
        assert_eq!(found(&t, "a"), pairs(&[("a", 0.0)]));
        // Text is never read as the identity symbol, so a character outside
        // the alphabet finds no path through it.
        assert_eq!(found(&t, "q"), pairs(&[]));
    }

    #[test]
    fn an_unweighted_transducer_gives_the_same_outputs_at_weight_zero() {
        let weighted = analyser(true);
        let unweighted = analyser(false);
        assert!(!unweighted.is_weighted());
        for word in ["cat", "cats", "a", "xq", "e\u{301}", "t", "s", "", "dog"] {
            let mut want: Vec<String> =
                found(&weighted, word).into_iter().map(|(o, _)| o).collect();
            want.dedup();
            let got = found(&unweighted, word);
            assert!(got.iter().all(|(_, w)| *w == 0.0), "{word}: {got:?}");
            let got: Vec<String> = got.into_iter().map(|(o, _)| o).collect();
            let mut want_sorted = want.clone();
            want_sorted.sort();
            assert_eq!(got, want_sorted, "{word}");
        }
    }

    #[test]
    fn a_lookup_keeps_nothing_from_one_word_to_the_next() {
        let t = analyser(true);
        let other = analyser(false);
        let mut kept = Lookup::new();
        for word in ["cat", "a", "t", "cats", "xq", "s", "cat"] {
            assert_eq!(kept.lookup(&t, word), lookup(&t, word), "{word}");
            assert_eq!(kept.lookup(&other, word), lookup(&other, word), "{word}");
        }
    }

    fn fixture(name: &str) -> BytesThfstTransducer {
        let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures")).join(name);
        let mut fs = Filesystem::new();
        for part in ["alphabet", "index", "transition"] {
            fs.insert(
                Path::new(name).join(part),
                std::fs::read(dir.join(part)).expect("fixture"),
            );
        }
        BytesThfstTransducer::from_path(&fs, name).expect("the fixture loads")
    }

    #[test]
    fn thfst_lexicons_are_looked_up_like_optimized_lookup() {
        let lexicon = fixture("lexicon.thfst");
        assert_eq!(found(&lexicon, "cart"), pairs(&[("cart", 1.0)]));
        assert_eq!(found(&lexicon, "cat"), pairs(&[("cat", 0.0)]));
        assert_eq!(found(&lexicon, "kat"), pairs(&[]));

        let flags = fixture("flag-lexicon.thfst");
        assert!(!lookup(&flags, "cart").is_empty(), "reached through a flag");
        assert!(lookup(&flags, "rat").is_empty(), "blocked by a flag");
    }

    #[test]
    fn lookup_agrees_with_the_spellers_analysis() {
        use crate::speller::{HfstSpeller, Speller};

        for (lexicon, mutator, words) in [
            (
                "eps-lexicon.thfst",
                "eps-mutator.thfst",
                &["cat", "ca", "cot"][..],
            ),
            (
                "flag-lexicon.thfst",
                "flag-mutator.thfst",
                &["cat", "car", "cart", "rat"][..],
            ),
            (
                "lexicon.thfst",
                "mutator.thfst",
                &["cat", "car", "cart", "care", "kat"][..],
            ),
        ] {
            let speller = HfstSpeller::new(fixture(mutator), fixture(lexicon));
            let alone = fixture(lexicon);
            for word in words {
                let mut analyses: Vec<(String, f32)> = speller
                    .clone()
                    .analyze_input(word)
                    .into_iter()
                    .map(|s| (s.value.to_string(), s.weight.0))
                    .collect();
                analyses.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
                assert_eq!(found(&alone, word), analyses, "{lexicon}: {word}");
                assert_eq!(
                    found(speller.lexicon(), word),
                    analyses,
                    "{lexicon}, its alphabet extended by the speller: {word}"
                );
            }
        }
    }

    fn names<T: Transducer>(t: &T) -> Vec<String> {
        let mut names: Vec<String> = t
            .alphabet()
            .key_table()
            .iter()
            .map(|k| k.to_string())
            .collect();
        names[0] = "@_EPSILON_SYMBOL_@".to_string();
        names
    }

    #[test]
    fn dhfst_files_are_looked_up_like_the_transducers_they_were_written_from() {
        use crate::transducer::dhfst::DhfstTransducer;
        use crate::transducer::dhfst::acceptor::DhfstAcceptor;
        use crate::transducer::dhfst::acceptor_writer::{AcceptorOptions, write_acceptor};
        use crate::transducer::dhfst::writer::{SourceModel, WriteOptions, write};

        // An acceptor writes what it reads, so the lexicon with tags on its
        // output side has no DHFST form.
        for name in ["lexicon.thfst", "flag-lexicon.thfst"] {
            let thfst = fixture(name);
            let options = AcceptorOptions {
                threads: 1,
                ..AcceptorOptions::default()
            };
            let written = write_acceptor(&thfst, &names(&thfst), &options).expect("writes");
            let dhfst = DhfstAcceptor::from_bytes(&written.bytes, name).expect("loads");
            for word in ["cat", "car", "cart", "care", "rat", "ca", ""] {
                assert_eq!(found(&dhfst, word), found(&thfst, word), "{name}: {word}");
            }
        }

        let thfst = fixture("mutator.thfst");
        let model = SourceModel::from_transducer(&thfst, names(&thfst)).expect("reads");
        let options = WriteOptions {
            threads: 1,
            ..WriteOptions::default()
        };
        let written = write(&model, &options).expect("writes");
        let dhfst = DhfstTransducer::from_bytes(&written.bytes, "mutator.dhfst").expect("loads");
        for word in ["kat", "cet", "cad", "tack", "e", ""] {
            assert_eq!(found(&dhfst, word), found(&thfst, word), "mutator: {word}");
            assert!(!found(&thfst, word).is_empty() || word.is_empty(), "{word}");
        }
    }
}
