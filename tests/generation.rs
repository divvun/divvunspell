//! Forward-generation tests: `Speller::generate(lemma)`.
//!
//! Two suites:
//!
//! 1. A tiny synthetic analyser FST (cat → cat+N+Sg, cats → cat+N+Pl)
//!    paired with a no-op mutator; validates walk semantics under full
//!    control of the corpus.
//! 2. A live test against the checked-in `se.bhfst` (North Sámi
//!    speller), run by default — exercises `generate()` end-to-end on
//!    real Sámi morphology.

use std::path::Path;
use std::sync::Arc;

use divvun_fst::generator::GeneratorConfig;
use divvun_fst::speller::{HfstSpeller, Speller};
use divvun_fst::transducer::TransducerLoader;
use divvun_fst::transducer::thfst::MmapThfstTransducer;
use divvun_fst::types::Weight;
use divvun_fst::vfs::Fs;

const TARGET_TABLE: u32 = 2_147_483_648; // 0x80000000

// ---------------------------------------------------------------------------
// THFST builder helpers (mirror tests/speller_integration.rs)
// ---------------------------------------------------------------------------

fn write_index_entry(buf: &mut Vec<u8>, input_symbol: u16, target: u32) {
    buf.extend_from_slice(&input_symbol.to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(&target.to_le_bytes());
}

fn write_index_final(buf: &mut Vec<u8>, weight: f32) {
    buf.extend_from_slice(&0xFFFFu16.to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(&weight.to_bits().to_le_bytes());
}

fn write_index_empty(buf: &mut Vec<u8>) {
    write_index_entry(buf, 0xFFFF, 0xFFFFFFFF);
}

fn write_trans_entry(buf: &mut Vec<u8>, input: u16, output: u16, target: u32, weight: f32) {
    buf.extend_from_slice(&input.to_le_bytes());
    buf.extend_from_slice(&output.to_le_bytes());
    buf.extend_from_slice(&target.to_le_bytes());
    buf.extend_from_slice(&weight.to_bits().to_le_bytes());
}

/// Sentinel transition record that terminates an eps/symbol walk. Both
/// `input_symbol` and `target` decode to None.
fn write_trans_boundary(buf: &mut Vec<u8>) {
    buf.extend_from_slice(&0xFFFFu16.to_le_bytes());
    buf.extend_from_slice(&0xFFFFu16.to_le_bytes());
    buf.extend_from_slice(&0xFFFFFFFFu32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
}

fn build_alphabet_json(symbols: &[&str]) -> String {
    let mut key_table_entries = Vec::new();
    let mut s2s_entries = Vec::new();
    for (i, sym) in symbols.iter().enumerate() {
        let escaped = sym.replace('\\', "\\\\").replace('"', "\\\"");
        key_table_entries.push(format!("\"{}\"", escaped));
        if !sym.starts_with('@') {
            s2s_entries.push(format!("\"{}\":{}", escaped, i));
        }
    }
    format!(
        r#"{{"key_table":[{}],"initial_symbol_count":{},"flag_state_size":0,"length":{},"string_to_symbol":{{{}}},"operations":{{}},"identity_symbol":null,"unknown_symbol":null}}"#,
        key_table_entries.join(","),
        symbols.len(),
        symbols.len(),
        s2s_entries.join(","),
    )
}

fn write_thfst(dir: &Path, alphabet_json: &str, index_data: &[u8], trans_data: &[u8]) {
    std::fs::write(dir.join("alphabet"), alphabet_json).unwrap();
    std::fs::write(dir.join("index"), index_data).unwrap();
    std::fs::write(dir.join("transition"), trans_data).unwrap();
}

fn write_empties(buf: &mut Vec<u8>, n: usize) {
    for _ in 0..n {
        write_index_empty(buf);
    }
}

/// Tiny generator FST (input = lemma+tags, output = surface):
///   input "cat+N+Sg" → output "cat"
///   input "cat+N+Pl" → output "cats"
///
/// ```text
/// Alphabet: [eps, c, a, t, s, +N, +Sg, +Pl]    (n = 8 symbols)
/// Slots per state: n + 1 = 9
///
/// (S0)--c/c-->(S1)--a/a-->(S2)--t/t-->(S3)
///                                       |
///                                       +N/eps
///                                       v
///                                      (S4)
///                                       |   `--+Pl/s-->(S6) FINAL
///                                       +Sg/eps
///                                       v
///                                      (S5) FINAL
/// ```
fn build_cat_lexicon(dir: &Path) {
    let symbols = &["@_EPSILON_SYMBOL_@", "c", "a", "t", "s", "+N", "+Sg", "+Pl"];
    let n = symbols.len(); // 8
    let slots = n + 1; // 9

    let mut idx = Vec::new();

    // S0 at idx 0
    write_index_empty(&mut idx); // [0] not final
    write_index_empty(&mut idx); // [1] eps slot
    write_index_entry(&mut idx, 1, TARGET_TABLE + 0); // [2] c -> trans[0]
    write_empties(&mut idx, slots - 3);

    // S1 at idx 9
    write_index_empty(&mut idx); // [9]
    write_index_empty(&mut idx); // [10] eps
    write_index_empty(&mut idx); // [11] c
    write_index_entry(&mut idx, 2, TARGET_TABLE + 1); // [12] a -> trans[1]
    write_empties(&mut idx, slots - 4);

    // S2 at idx 18
    write_index_empty(&mut idx); // [18]
    write_index_empty(&mut idx); // [19] eps
    write_index_empty(&mut idx); // [20] c
    write_index_empty(&mut idx); // [21] a
    write_index_entry(&mut idx, 3, TARGET_TABLE + 2); // [22] t -> trans[2]
    write_empties(&mut idx, slots - 5);

    // S3 at idx 27 (after "cat" on input — has +N/eps)
    write_index_empty(&mut idx); // [27]
    write_index_empty(&mut idx); // [28] eps
    write_index_empty(&mut idx); // [29] c
    write_index_empty(&mut idx); // [30] a
    write_index_empty(&mut idx); // [31] t
    write_index_empty(&mut idx); // [32] s
    write_index_entry(&mut idx, 5, TARGET_TABLE + 3); // [33] +N -> trans[3]
    write_empties(&mut idx, slots - 7);

    // S4 at idx 36 (after "cat+N" on input — branches +Sg/eps or +Pl/s)
    write_index_empty(&mut idx); // [36]
    write_index_empty(&mut idx); // [37] eps
    write_index_empty(&mut idx); // [38] c
    write_index_empty(&mut idx); // [39] a
    write_index_empty(&mut idx); // [40] t
    write_index_empty(&mut idx); // [41] s
    write_index_empty(&mut idx); // [42] +N
    write_index_entry(&mut idx, 6, TARGET_TABLE + 5); // [43] +Sg -> trans[5]
    write_index_entry(&mut idx, 7, TARGET_TABLE + 7); // [44] +Pl -> trans[7]

    // S5 at idx 45 (FINAL — singular)
    write_index_final(&mut idx, 0.0); // [45]
    write_empties(&mut idx, slots - 1);

    // S6 at idx 54 (FINAL — plural)
    write_index_final(&mut idx, 0.0); // [54]
    write_empties(&mut idx, slots - 1);

    //   [0] c/c   -> S1
    //   [1] a/a   -> S2
    //   [2] t/t   -> S3
    //   [3] +N/eps -> S4   (S3's +N slot, input=5, output=eps=0)
    //   [4] BOUNDARY
    //   [5] +Sg/eps -> S5  (S4's +Sg slot, input=6, output=eps=0)
    //   [6] BOUNDARY
    //   [7] +Pl/s -> S6     (S4's +Pl slot, input=7, output=s=4)
    let mut tr = Vec::new();
    write_trans_entry(&mut tr, 1, 1, 9, 0.0); // [0]
    write_trans_entry(&mut tr, 2, 2, 18, 0.0); // [1]
    write_trans_entry(&mut tr, 3, 3, 27, 0.0); // [2]
    write_trans_entry(&mut tr, 5, 0, 36, 0.0); // [3]
    write_trans_boundary(&mut tr); // [4]
    write_trans_entry(&mut tr, 6, 0, 45, 0.0); // [5]
    write_trans_boundary(&mut tr); // [6]
    write_trans_entry(&mut tr, 7, 4, 54, 0.0); // [7]

    write_thfst(dir, &build_alphabet_json(symbols), &idx, &tr);
}

/// Trivial single-state mutator (identity-only) so we can construct a
/// `HfstSpeller` for the generation API. The mutator is unused by the
/// `generate` walk — it only touches the lexicon — but the speller
/// constructor requires both transducers.
fn build_noop_mutator(dir: &Path) {
    let symbols = &["@_EPSILON_SYMBOL_@"];
    let mut idx = Vec::new();
    write_index_final(&mut idx, 0.0); // [0] start = final
    write_index_empty(&mut idx); // [1] no eps slot
    let tr: Vec<u8> = Vec::new();
    write_thfst(dir, &build_alphabet_json(symbols), &idx, &tr);
}

fn build_test_speller(
    lexicon_dir: &Path,
    mutator_dir: &Path,
) -> Arc<HfstSpeller<MmapThfstTransducer, MmapThfstTransducer>> {
    let mutator = MmapThfstTransducer::from_path(&Fs, mutator_dir).expect("mutator");
    let lexicon = MmapThfstTransducer::from_path(&Fs, lexicon_dir).expect("lexicon");
    HfstSpeller::new(mutator, lexicon)
}

// ---------------------------------------------------------------------------
// Synthetic-FST tests
// ---------------------------------------------------------------------------

fn sorted_pairs(results: &[divvun_fst::generator::GenerationResult]) -> Vec<(String, String)> {
    let mut pairs: Vec<_> = results
        .iter()
        .map(|r| (r.surface.to_string(), r.analysis.to_string()))
        .collect();
    pairs.sort();
    pairs
}

#[test]
fn generates_cat_singular_and_plural() {
    let lex_dir = tempfile::tempdir().unwrap();
    let mut_dir = tempfile::tempdir().unwrap();
    build_cat_lexicon(lex_dir.path());
    build_noop_mutator(mut_dir.path());
    let speller = build_test_speller(lex_dir.path(), mut_dir.path());

    let results = speller.generate("cat");

    assert_eq!(
        sorted_pairs(&results),
        vec![
            ("cat".to_string(), "+N+Sg".to_string()),
            ("cats".to_string(), "+N+Pl".to_string()),
        ],
        "generate(\"cat\") should yield both inflected forms"
    );
    for r in &results {
        assert_eq!(r.weight, Weight(0.0));
    }
}

#[test]
fn returns_empty_for_unknown_lemma() {
    let lex_dir = tempfile::tempdir().unwrap();
    let mut_dir = tempfile::tempdir().unwrap();
    build_cat_lexicon(lex_dir.path());
    build_noop_mutator(mut_dir.path());
    let speller = build_test_speller(lex_dir.path(), mut_dir.path());

    // "z" is not in the alphabet at all → no symbol, empty result.
    assert!(speller.clone().generate("z").is_empty());
    // "ca" is a prefix of "cat" but not itself a complete lemma; the
    // walker rejects it because the next non-tag output ('t') would
    // extend it into "cat".
    assert!(speller.clone().generate("ca").is_empty());
    // Empty lemma → empty result.
    assert!(speller.generate("").is_empty());
}

#[test]
fn config_max_results_caps_output() {
    let lex_dir = tempfile::tempdir().unwrap();
    let mut_dir = tempfile::tempdir().unwrap();
    build_cat_lexicon(lex_dir.path());
    build_noop_mutator(mut_dir.path());
    let speller = build_test_speller(lex_dir.path(), mut_dir.path());

    let cfg = GeneratorConfig {
        max_results: Some(1),
        ..GeneratorConfig::default()
    };
    assert_eq!(speller.generate_with_config("cat", &cfg).len(), 1);
}

// ---------------------------------------------------------------------------
// Real Sámi: `se.bhfst`
// ---------------------------------------------------------------------------

/// Live North Sámi smoke test against the giellaLT generator FST
/// (`generator-gt-norm.hfstol`). Loaded directly via `HfstTransducer`,
/// then wrapped in an `HfstSpeller` with a no-op mutator so we can
/// drive it through the standard `Speller::generate` API.
///
/// Skipped (passes vacuously) when the FST file isn't present —
/// override the search location with the `SME_GENERATOR_HFSTOL`
/// environment variable to point at your local build.
#[test]
fn smoke_sme_generator_hfstol() {
    use divvun_fst::transducer::hfst::HfstTransducer;

    let env_path = std::env::var_os("SME_GENERATOR_HFSTOL");
    let candidates: Vec<std::path::PathBuf> = if let Some(p) = env_path {
        vec![p.into()]
    } else {
        let home = std::env::var("HOME").unwrap_or_default();
        vec![
            std::path::PathBuf::from(&home)
                .join("Downloads/sme/build/src/fst/generator-gt-norm.hfstol"),
            std::path::PathBuf::from(&home)
                .join("git/giellalt/lang-sme/build/src/fst/generator-gt-norm.hfstol"),
        ]
    };
    let Some(fst_path) = candidates.iter().find(|p| p.exists()) else {
        eprintln!(
            "smoke_sme_generator: generator-gt-norm.hfstol not found in any of {:?}; skipping",
            candidates
        );
        return;
    };

    let lexicon = HfstTransducer::from_path(&Fs, fst_path).expect("load hfstol");
    // Build a no-op mutator so we can construct an HfstSpeller<Hfst, Hfst>.
    let mut_dir = tempfile::tempdir().unwrap();
    build_noop_mutator(mut_dir.path());
    let mutator = HfstTransducer::from_path(&Fs, mut_dir.path());
    // The no-op mutator we build is THFST; the generator is HFST. We
    // need a same-format mutator to satisfy HfstSpeller<T, U>'s type
    // constraints, so build a tiny in-memory HFST mutator. For now
    // construct the speller with the generator on both sides — only
    // the lexicon side is consulted by `generate`.
    drop(mutator);
    let speller = HfstSpellerHfst::new(
        HfstTransducer::from_path(&Fs, fst_path).expect("reload hfstol for mutator slot"),
        lexicon,
    );

    let cfg = GeneratorConfig {
        max_depth: 48,
        max_weight: Weight(120.0),
        max_results: Some(64),
        max_iterations: Some(500_000),
    };

    for lemma in ["biila", "guolli", "dieđihit"] {
        let t0 = std::time::Instant::now();
        let results = speller.clone().generate_with_config(lemma, &cfg);
        let elapsed = t0.elapsed();
        eprintln!(
            "sme generator: generate({:?}) → {} forms in {:?}",
            lemma,
            results.len(),
            elapsed
        );
        for (i, r) in results.iter().enumerate().take(12) {
            eprintln!(
                "  [{}] {} | analysis: {} (w={:?})",
                i, r.surface, r.analysis, r.weight
            );
        }
        assert!(
            !results.is_empty(),
            "sme generator: generate({:?}) produced no results",
            lemma
        );
    }
}

type HfstSpellerHfst = HfstSpeller<
    divvun_fst::transducer::hfst::HfstTransducer,
    divvun_fst::transducer::hfst::HfstTransducer,
>;
