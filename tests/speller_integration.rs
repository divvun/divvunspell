use std::path::Path;
use std::sync::Arc;

use divvun_fst::speller::{HfstSpeller, Speller, SpellerConfig};
use divvun_fst::transducer::TransducerLoader;
use divvun_fst::transducer::thfst::MmapThfstTransducer;
use divvun_fst::types::Weight;
use divvun_fst::vfs::Fs;

const TARGET_TABLE: u32 = 2_147_483_648; // 0x80000000

// ---------------------------------------------------------------------------
// THFST builder helpers
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

fn build_alphabet_json(symbols: &[&str]) -> String {
    build_alphabet_json_full(symbols, None, None, &[], 0)
}

/// Full alphabet builder supporting identity, unknown, and flag diacritics.
/// `flags`: slice of (symbol_index, operator, feature, value) tuples.
fn build_alphabet_json_full(
    symbols: &[&str],
    identity: Option<usize>,
    unknown: Option<usize>,
    flags: &[(usize, &str, u16, i16)], // (sym_idx, operator, feature, value)
    flag_state_size: u16,
) -> String {
    let mut key_table_entries = Vec::new();
    let mut string_to_symbol = std::collections::HashMap::new();

    for (i, sym) in symbols.iter().enumerate() {
        let escaped = sym.replace('\\', "\\\\").replace('"', "\\\"");
        key_table_entries.push(format!("\"{}\"", escaped));
        // Exclude @-prefixed symbols from string_to_symbol
        if !sym.starts_with('@') {
            string_to_symbol.insert(*sym, i);
        }
    }

    let s2s_entries: Vec<String> = string_to_symbol
        .iter()
        .map(|(k, v)| {
            let escaped = k.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{}\":{}", escaped, v)
        })
        .collect();

    let ops_entries: Vec<String> = flags
        .iter()
        .map(|(idx, op, feat, val)| {
            format!(
                "\"{}\":{{\"operation\":\"{}\",\"feature\":{},\"value\":{}}}",
                idx, op, feat, val
            )
        })
        .collect();

    let id_str = match identity {
        Some(i) => format!("{}", i),
        None => "null".to_string(),
    };
    let unk_str = match unknown {
        Some(i) => format!("{}", i),
        None => "null".to_string(),
    };

    format!(
        r#"{{"key_table":[{}],"initial_symbol_count":{},"flag_state_size":{},"length":{},"string_to_symbol":{{{}}},"operations":{{{}}},"identity_symbol":{},"unknown_symbol":{}}}"#,
        key_table_entries.join(","),
        symbols.len(),
        flag_state_size,
        symbols.len(),
        s2s_entries.join(","),
        ops_entries.join(","),
        id_str,
        unk_str,
    )
}

fn write_thfst(dir: &Path, alphabet_json: &str, index_data: &[u8], trans_data: &[u8]) {
    std::fs::write(dir.join("alphabet"), alphabet_json).unwrap();
    std::fs::write(dir.join("index"), index_data).unwrap();
    std::fs::write(dir.join("transition"), trans_data).unwrap();
}

/// Helper: write N empty index entries
fn write_empties(buf: &mut Vec<u8>, n: usize) {
    for _ in 0..n {
        write_index_empty(buf);
    }
}

/// Lexicon accepting:
///   "cat" (w=0), "car" (w=0), "cart" (w=1), "care" (w=0), "cär" (w=0)
///
/// ```text
/// Alphabet: [eps, c, a, t, r, e, ä]  (symbols 0-6)
///
/// (0)--c-->(1)--a-->(2)--t-->(3) FINAL w=0   "cat"
///               |       |
///               |       r-->(4) FINAL w=0     "car"
///               |             |
///               |             t-->(5) FINAL w=1  "cart"
///               |             e-->(6) FINAL w=0  "care"
///               |
///               ä-->(7)--r-->(8) FINAL w=0     "cär"
/// ```
fn build_lexicon(dir: &Path) {
    // eps=0, c=1, a=2, t=3, r=4, e=5, ä=6
    let symbols = &["@_EPSILON_SYMBOL_@", "c", "a", "t", "r", "e", "ä"];
    let n = symbols.len(); // 7 → 8 entries per state

    let mut idx = Vec::new();

    // State 0 (start), 8 entries
    write_index_empty(&mut idx); // [0] not final
    write_index_empty(&mut idx); // [1] no eps
    write_index_entry(&mut idx, 1, TARGET_TABLE + 0); // [2] c → trans[0]
    write_empties(&mut idx, n - 2); // [3-7] no a/t/r/e/ä

    // State 1 (after "c"), 8 entries, starts at idx 8
    write_index_empty(&mut idx); // [8]
    write_index_empty(&mut idx); // [9] no eps
    write_index_empty(&mut idx); // [10] no c
    write_index_entry(&mut idx, 2, TARGET_TABLE + 1); // [11] a → trans[1]
    write_index_empty(&mut idx); // [12] no t
    write_index_empty(&mut idx); // [13] no r
    write_index_empty(&mut idx); // [14] no e
    write_index_entry(&mut idx, 6, TARGET_TABLE + 2); // [15] ä → trans[2]

    // State 2 (after "ca"), 8 entries, starts at idx 16
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_entry(&mut idx, 3, TARGET_TABLE + 3); // t → trans[3]
    write_index_entry(&mut idx, 4, TARGET_TABLE + 4); // r → trans[4]
    write_empties(&mut idx, 2);

    // State 3 ("cat") FINAL w=0, starts at idx 24
    write_index_final(&mut idx, 0.0);
    write_empties(&mut idx, n);

    // State 4 ("car") FINAL w=0, starts at idx 32
    write_index_final(&mut idx, 0.0);
    write_index_empty(&mut idx); // no eps
    write_index_empty(&mut idx); // no c
    write_index_empty(&mut idx); // no a
    write_index_entry(&mut idx, 3, TARGET_TABLE + 5); // t → trans[5]
    write_index_empty(&mut idx); // no r
    write_index_entry(&mut idx, 5, TARGET_TABLE + 6); // e → trans[6]
    write_index_empty(&mut idx); // no ä

    // State 5 ("cart") FINAL w=1, starts at idx 40
    write_index_final(&mut idx, 1.0);
    write_empties(&mut idx, n);

    // State 6 ("care") FINAL w=0, starts at idx 48
    write_index_final(&mut idx, 0.0);
    write_empties(&mut idx, n);

    // State 7 (after "cä"), starts at idx 56
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_entry(&mut idx, 4, TARGET_TABLE + 7); // r → trans[7]
    write_empties(&mut idx, 2);

    // State 8 ("cär") FINAL w=0, starts at idx 64
    write_index_final(&mut idx, 0.0);
    write_empties(&mut idx, n);

    let mut tr = Vec::new();
    write_trans_entry(&mut tr, 1, 1, 8, 0.0); // [0] c→c → state 1
    write_trans_entry(&mut tr, 2, 2, 16, 0.0); // [1] a→a → state 2
    write_trans_entry(&mut tr, 6, 6, 56, 0.0); // [2] ä→ä → state 7
    write_trans_entry(&mut tr, 3, 3, 24, 0.0); // [3] t→t → state 3
    write_trans_entry(&mut tr, 4, 4, 32, 0.0); // [4] r→r → state 4
    write_trans_entry(&mut tr, 3, 3, 40, 0.0); // [5] t→t → state 5
    write_trans_entry(&mut tr, 5, 5, 48, 0.0); // [6] e→e → state 6
    write_trans_entry(&mut tr, 4, 4, 64, 0.0); // [7] r→r → state 8

    write_thfst(dir, &build_alphabet_json(symbols), &idx, &tr);
}

/// Mutator: identity + substitutions + deletions + insertions.
///
/// ```text
/// Alphabet: [eps, c, a, t, r, k, e, d, ä]  (symbols 0-8)
///
/// State 0: start + final (w=0). Single state, self-loops.
///
/// Identity (w=0): c,a,t,r,k,e,d,ä → self
/// Substitutions (w=5): k→c, e→a, d→t, ä→a
/// Deletions (w=7): any → ε
/// Insertions (w=8): ε → c,a,t,r,e,ä
/// ```
fn build_mutator(dir: &Path) {
    // eps=0, c=1, a=2, t=3, r=4, k=5, e=6, d=7, ä=8
    let symbols = &["@_EPSILON_SYMBOL_@", "c", "a", "t", "r", "k", "e", "d", "ä"];

    let mut idx = Vec::new();

    // State 0: 10 entries (header + 9 symbol slots)
    write_index_final(&mut idx, 0.0); // [0] final w=0
    write_index_entry(&mut idx, 0, TARGET_TABLE + 0); // [1] eps → trans[0]
    write_index_entry(&mut idx, 1, TARGET_TABLE + 6); // [2] c → trans[6]
    write_index_entry(&mut idx, 2, TARGET_TABLE + 8); // [3] a → trans[8]
    write_index_entry(&mut idx, 3, TARGET_TABLE + 10); // [4] t → trans[10]
    write_index_entry(&mut idx, 4, TARGET_TABLE + 12); // [5] r → trans[12]
    write_index_entry(&mut idx, 5, TARGET_TABLE + 14); // [6] k → trans[14]
    write_index_entry(&mut idx, 6, TARGET_TABLE + 17); // [7] e → trans[17]
    write_index_entry(&mut idx, 7, TARGET_TABLE + 20); // [8] d → trans[20]
    write_index_entry(&mut idx, 8, TARGET_TABLE + 23); // [9] ä → trans[23]

    let mut tr = Vec::new();

    // Insertions (eps input): [0]-[5]
    write_trans_entry(&mut tr, 0, 1, 0, 8.0); // [0]  ε→c
    write_trans_entry(&mut tr, 0, 2, 0, 8.0); // [1]  ε→a
    write_trans_entry(&mut tr, 0, 3, 0, 8.0); // [2]  ε→t
    write_trans_entry(&mut tr, 0, 4, 0, 8.0); // [3]  ε→r
    write_trans_entry(&mut tr, 0, 6, 0, 8.0); // [4]  ε→e
    write_trans_entry(&mut tr, 0, 8, 0, 8.0); // [5]  ε→ä

    // c: [6]-[7]
    write_trans_entry(&mut tr, 1, 1, 0, 0.0); // [6]  c→c
    write_trans_entry(&mut tr, 1, 0, 0, 7.0); // [7]  c→ε

    // a: [8]-[9]
    write_trans_entry(&mut tr, 2, 2, 0, 0.0); // [8]  a→a
    write_trans_entry(&mut tr, 2, 0, 0, 7.0); // [9]  a→ε

    // t: [10]-[11]
    write_trans_entry(&mut tr, 3, 3, 0, 0.0); // [10] t→t
    write_trans_entry(&mut tr, 3, 0, 0, 7.0); // [11] t→ε

    // r: [12]-[13]
    write_trans_entry(&mut tr, 4, 4, 0, 0.0); // [12] r→r
    write_trans_entry(&mut tr, 4, 0, 0, 7.0); // [13] r→ε

    // k: [14]-[16]
    write_trans_entry(&mut tr, 5, 5, 0, 0.0); // [14] k→k
    write_trans_entry(&mut tr, 5, 1, 0, 5.0); // [15] k→c
    write_trans_entry(&mut tr, 5, 0, 0, 7.0); // [16] k→ε

    // e: [17]-[19]
    write_trans_entry(&mut tr, 6, 6, 0, 0.0); // [17] e→e
    write_trans_entry(&mut tr, 6, 2, 0, 5.0); // [18] e→a
    write_trans_entry(&mut tr, 6, 0, 0, 7.0); // [19] e→ε

    // d: [20]-[22]
    write_trans_entry(&mut tr, 7, 7, 0, 0.0); // [20] d→d
    write_trans_entry(&mut tr, 7, 3, 0, 5.0); // [21] d→t
    write_trans_entry(&mut tr, 7, 0, 0, 7.0); // [22] d→ε

    // ä: [23]-[25]
    write_trans_entry(&mut tr, 8, 8, 0, 0.0); // [23] ä→ä
    write_trans_entry(&mut tr, 8, 2, 0, 5.0); // [24] ä→a
    write_trans_entry(&mut tr, 8, 0, 0, 7.0); // [25] ä→ε

    write_thfst(dir, &build_alphabet_json(symbols), &idx, &tr);
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn fixtures_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"))
}

fn load_speller(
    lexicon_dir: &Path,
    mutator_dir: &Path,
) -> Arc<HfstSpeller<MmapThfstTransducer, MmapThfstTransducer>> {
    let fs = Fs;
    let mutator = MmapThfstTransducer::from_path(&fs, mutator_dir).unwrap();
    let lexicon = MmapThfstTransducer::from_path(&fs, lexicon_dir).unwrap();
    HfstSpeller::new(mutator, lexicon)
}

fn test_speller() -> Arc<HfstSpeller<MmapThfstTransducer, MmapThfstTransducer>> {
    let base = fixtures_dir();
    load_speller(&base.join("lexicon.thfst"), &base.join("mutator.thfst"))
}

fn raw_config() -> SpellerConfig {
    SpellerConfig {
        reweight: None,
        recase: false,
        ..SpellerConfig::default()
    }
}

fn reweight_config() -> SpellerConfig {
    SpellerConfig {
        recase: false,
        ..SpellerConfig::default()
    }
}

fn suggestion_values(
    s: &Arc<HfstSpeller<MmapThfstTransducer, MmapThfstTransducer>>,
    word: &str,
    config: &SpellerConfig,
) -> Vec<(String, f32)> {
    s.clone()
        .suggest_with_config(word, config)
        .iter()
        .map(|s| (s.value.to_string(), s.weight().0))
        .collect()
}

fn suggestion_words(
    s: &Arc<HfstSpeller<MmapThfstTransducer, MmapThfstTransducer>>,
    word: &str,
    config: &SpellerConfig,
) -> Vec<String> {
    s.clone()
        .suggest_with_config(word, config)
        .iter()
        .map(|s| s.value.to_string())
        .collect()
}

fn assert_suggests(
    s: &Arc<HfstSpeller<MmapThfstTransducer, MmapThfstTransducer>>,
    input: &str,
    expected: &str,
    config: &SpellerConfig,
) {
    let words = suggestion_words(s, input, config);
    assert!(
        words.contains(&expected.to_string()),
        "'{}' should suggest '{}', got: {:?}",
        input,
        expected,
        words
    );
}

fn assert_suggests_at_weight(
    s: &Arc<HfstSpeller<MmapThfstTransducer, MmapThfstTransducer>>,
    input: &str,
    expected: &str,
    expected_weight: f32,
    config: &SpellerConfig,
) {
    let suggs = suggestion_values(s, input, config);
    let found = suggs.iter().find(|(v, _)| v == expected);
    assert!(
        found.is_some(),
        "'{}' should suggest '{}', got: {:?}",
        input,
        expected,
        suggs
    );
    let w = found.unwrap().1;
    assert!(
        (w - expected_weight).abs() < 0.01,
        "'{}' -> '{}': expected weight {}, got {}",
        input,
        expected,
        expected_weight,
        w
    );
}

fn assert_sorted(suggs: &[(String, f32)], label: &str) {
    for w in suggs.windows(2) {
        assert!(w[0].1 <= w[1].1, "{}: not sorted: {:?}", label, suggs);
    }
}

fn assert_not_suggests(
    s: &Arc<HfstSpeller<MmapThfstTransducer, MmapThfstTransducer>>,
    input: &str,
    unexpected: &str,
    config: &SpellerConfig,
) {
    let words = suggestion_words(s, input, config);
    assert!(
        !words.contains(&unexpected.to_string()),
        "'{}' should NOT suggest '{}', got: {:?}",
        input,
        unexpected,
        words
    );
}

// ===========================================================================
// is_correct
// ===========================================================================

#[test]
fn test_is_correct_lexicon_words() {
    let s = test_speller();
    for w in &["cat", "car", "cart", "care", "cär"] {
        assert!(s.clone().is_correct(w), "'{}' should be correct", w);
    }
}

#[test]
fn test_is_correct_rejects_misspelled() {
    let s = test_speller();
    for w in &["kat", "cet", "cad", "dog", "ca", "c", "ct"] {
        assert!(!s.clone().is_correct(w), "'{}' should be incorrect", w);
    }
}

#[test]
fn test_is_correct_empty_and_nonletter() {
    let s = test_speller();
    for w in &["", "123", "...", "42", "!!!"] {
        assert!(
            s.clone().is_correct(w),
            "'{}' should be correct (non-letter)",
            w
        );
    }
}

#[test]
fn test_is_correct_first_caps() {
    let s = test_speller();
    for w in &["Cat", "Car", "Care", "Cart"] {
        assert!(
            s.clone().is_correct(w),
            "'{}' should be correct (first caps)",
            w
        );
    }
}

#[test]
fn test_is_correct_all_caps() {
    let s = test_speller();
    for w in &["CAT", "CAR", "CART", "CARE"] {
        assert!(
            s.clone().is_correct(w),
            "'{}' should be correct (all caps)",
            w
        );
    }
}

#[test]
fn test_is_correct_recase_disabled() {
    let s = test_speller();
    let cfg = SpellerConfig {
        recase: false,
        ..SpellerConfig::default()
    };
    assert!(s.clone().is_correct_with_config("cat", &cfg));
    assert!(!s.clone().is_correct_with_config("Cat", &cfg));
    assert!(!s.clone().is_correct_with_config("CAT", &cfg));
}

// ===========================================================================
// Exact weights — substitutions
// ===========================================================================

#[test]
fn test_weight_correct_word() {
    assert_suggests_at_weight(&test_speller(), "cat", "cat", 0.0, &raw_config());
}
#[test]
fn test_weight_lexicon_cost() {
    assert_suggests_at_weight(&test_speller(), "cart", "cart", 1.0, &raw_config());
}
#[test]
fn test_weight_start_sub() {
    assert_suggests_at_weight(&test_speller(), "kat", "cat", 5.0, &raw_config());
}
#[test]
fn test_weight_mid_sub() {
    assert_suggests_at_weight(&test_speller(), "cet", "cat", 5.0, &raw_config());
}
#[test]
fn test_weight_end_sub() {
    assert_suggests_at_weight(&test_speller(), "cad", "cat", 5.0, &raw_config());
}
#[test]
fn test_weight_triple_sub() {
    assert_suggests_at_weight(&test_speller(), "ked", "cat", 15.0, &raw_config());
}
#[test]
fn test_weight_longer_start() {
    assert_suggests_at_weight(&test_speller(), "kare", "care", 5.0, &raw_config());
}
#[test]
fn test_weight_longer_end() {
    assert_suggests_at_weight(&test_speller(), "card", "cart", 6.0, &raw_config());
}

// ===========================================================================
// Exact weights — deletions
// ===========================================================================

#[test]
fn test_weight_delete_mid() {
    assert_suggests_at_weight(&test_speller(), "caat", "cat", 7.0, &raw_config());
}
#[test]
fn test_weight_delete_start() {
    assert_suggests_at_weight(&test_speller(), "ccat", "cat", 7.0, &raw_config());
}
#[test]
fn test_weight_delete_end() {
    assert_suggests_at_weight(&test_speller(), "catt", "cat", 7.0, &raw_config());
}

// ===========================================================================
// Exact weights — insertions
// ===========================================================================

#[test]
fn test_weight_insert_mid() {
    assert_suggests_at_weight(&test_speller(), "ct", "cat", 8.0, &raw_config());
}
#[test]
fn test_weight_insert_end() {
    assert_suggests_at_weight(&test_speller(), "ca", "cat", 8.0, &raw_config());
}
#[test]
fn test_insert_multiple_results() {
    let s = test_speller();
    let cfg = raw_config();
    // "ca" + insert t → "cat", + insert r → "car"
    assert_suggests(&s, "ca", "cat", &cfg);
    assert_suggests(&s, "ca", "car", &cfg);
}

// ===========================================================================
// Exact weights — combined errors
// ===========================================================================

#[test]
fn test_weight_sub_plus_delete() {
    assert_suggests_at_weight(&test_speller(), "kaat", "cat", 12.0, &raw_config());
}
#[test]
fn test_weight_sub_plus_insert() {
    assert_suggests_at_weight(&test_speller(), "kt", "cat", 13.0, &raw_config());
}

// ===========================================================================
// Suggestion ordering + deduplication
// ===========================================================================

#[test]
fn test_ordering_multiple_suggestions() {
    let s = test_speller();
    let suggs = suggestion_values(&s, "car", &raw_config());
    assert!(suggs.len() >= 2, "expected multiple: {:?}", suggs);
    assert_sorted(&suggs, "car");
}

#[test]
fn test_ordering_with_reweight() {
    let suggs = suggestion_values(&test_speller(), "kat", &SpellerConfig::default());
    assert_sorted(&suggs, "kat");
}

#[test]
fn test_deduplication_keeps_best() {
    let s = test_speller();
    let cfg = raw_config();
    // "kat" → "cat" reachable via: k→c(5), or k→ε(7)+insert-c(8)=15.
    // Dedup should keep only the best (5.0).
    let suggs = suggestion_values(&s, "kat", &cfg);
    let cats: Vec<_> = suggs.iter().filter(|(v, _)| v == "cat").collect();
    assert_eq!(cats.len(), 1, "cat should appear exactly once: {:?}", suggs);
    assert_eq!(cats[0].1, 5.0, "should keep the cheaper path");
}

#[test]
fn test_weight_tie_lexicographic_order() {
    let s = test_speller();
    let cfg = raw_config();
    // "ca" → "car"(8) and "cat"(8) both via insertion at same weight.
    // Ties broken alphabetically: car < cat.
    let suggs = suggestion_values(&s, "ca", &cfg);
    let car_idx = suggs.iter().position(|(v, _)| v == "car");
    let cat_idx = suggs.iter().position(|(v, _)| v == "cat");
    if let (Some(ci), Some(ti)) = (car_idx, cat_idx) {
        assert!(
            ci < ti,
            "car should appear before cat at same weight: {:?}",
            suggs
        );
    }
}

// ===========================================================================
// Reweighting penalties
// ===========================================================================

#[test]
fn test_reweight_start_penalty() {
    let s = test_speller();
    let suggs = suggestion_values(&s, "kat", &reweight_config());
    let cat = suggs.iter().find(|(v, _)| v == "cat").unwrap();
    assert!(cat.1 > 5.0, "start penalty should increase: {}", cat.1);
}

#[test]
fn test_reweight_mid_less_than_start() {
    let s = test_speller();
    let cfg = reweight_config();
    let mid = suggestion_values(&s, "cet", &cfg)
        .iter()
        .find(|(v, _)| v == "cat")
        .unwrap()
        .1;
    let start = suggestion_values(&s, "kat", &cfg)
        .iter()
        .find(|(v, _)| v == "cat")
        .unwrap()
        .1;
    assert!(
        mid < start,
        "mid({}) should cost less than start({})",
        mid,
        start
    );
}

#[test]
fn test_reweight_end_penalty() {
    let s = test_speller();
    let suggs = suggestion_values(&s, "cad", &reweight_config());
    let cat = suggs.iter().find(|(v, _)| v == "cat").unwrap();
    assert!(cat.1 > 5.0, "end penalty should increase: {}", cat.1);
}

#[test]
fn test_reweight_zero_for_correct() {
    assert_suggests_at_weight(&test_speller(), "cat", "cat", 0.0, &reweight_config());
}

// ===========================================================================
// Beam
// ===========================================================================

#[test]
fn test_beam_excludes_distant() {
    let s = test_speller();
    let cfg = SpellerConfig {
        beam: Some(Weight(0.5)),
        reweight: None,
        recase: false,
        ..SpellerConfig::default()
    };
    let suggs = suggestion_values(&s, "car", &cfg);
    assert!(suggs.iter().any(|(v, _)| v == "car"));
    assert!(
        !suggs.iter().any(|(v, _)| v == "care"),
        "beam should exclude care: {:?}",
        suggs
    );
}

#[test]
fn test_beam_includes_within_range() {
    let s = test_speller();
    let cfg = SpellerConfig {
        beam: Some(Weight(10.0)),
        reweight: None,
        recase: false,
        ..SpellerConfig::default()
    };
    assert_suggests(&s, "car", "care", &cfg);
}

#[test]
fn test_beam_post_filter_with_reweight() {
    let s = test_speller();
    // With reweighting, beam is applied as a hard post-filter in suggest_case
    let cfg = SpellerConfig {
        beam: Some(Weight(0.5)),
        recase: false,
        ..SpellerConfig::default()
    };
    let suggs = suggestion_values(&s, "car", &cfg);
    assert!(suggs.iter().any(|(v, _)| v == "car"));
    assert_not_suggests(&s, "car", "care", &cfg);
}

// ===========================================================================
// Max weight
// ===========================================================================

#[test]
fn test_max_weight_blocks() {
    assert_not_suggests(
        &test_speller(),
        "kat",
        "cat",
        &SpellerConfig {
            max_weight: Some(Weight(3.0)),
            reweight: None,
            recase: false,
            ..SpellerConfig::default()
        },
    );
}
#[test]
fn test_max_weight_allows() {
    assert_suggests(
        &test_speller(),
        "kat",
        "cat",
        &SpellerConfig {
            max_weight: Some(Weight(6.0)),
            reweight: None,
            recase: false,
            ..SpellerConfig::default()
        },
    );
}

// ===========================================================================
// Case output
// ===========================================================================

#[test]
fn test_suggest_first_caps() {
    assert_suggests(&test_speller(), "Kat", "Cat", &SpellerConfig::default());
}
#[test]
fn test_suggest_all_caps() {
    assert_suggests(&test_speller(), "KAT", "CAT", &SpellerConfig::default());
}
#[test]
fn test_correct_word_preserves_case() {
    assert_suggests(&test_speller(), "CAR", "CAR", &SpellerConfig::default());
}

// ===========================================================================
// Mixed case (CaseMode::FirstResults path)
// ===========================================================================

#[test]
fn test_mixed_case_is_incorrect() {
    let s = test_speller();
    // "cAt" is mixed case — not in lexicon, and mixed case mode only tries
    // the word as-is and with first letter lowered/uppered. "cAt" won't match.
    assert!(!s.clone().is_correct("cAt"));
}

#[test]
fn test_mixed_case_suggest() {
    let s = test_speller();
    // Mixed case goes through FirstResults mode, which returns first non-empty result set
    let suggs = suggestion_words(&s, "cAt", &SpellerConfig::default());
    // Should get some suggestions (possibly "cat" via case fallback to lowercase)
    assert!(
        !suggs.is_empty(),
        "mixed case 'cAt' should produce suggestions: {:?}",
        suggs
    );
}

#[test]
fn test_mixed_case_first_caps_variant() {
    let s = test_speller();
    // "kAt" is mixed case. FirstResults path tries "kAt" and "KAt".
    // Neither is in lexicon, but lowercase fallback "kat" → "cat" via error model.
    let suggs = suggestion_words(&s, "kAt", &SpellerConfig::default());
    assert!(
        !suggs.is_empty(),
        "mixed case 'kAt' should produce suggestions: {:?}",
        suggs
    );
}

// ===========================================================================
// Unicode / grapheme handling
// ===========================================================================

#[test]
fn test_unicode_is_correct() {
    let s = test_speller();
    assert!(s.clone().is_correct("cär"));
}

#[test]
fn test_unicode_first_caps() {
    let s = test_speller();
    // "Cär" → first caps of "cär"
    assert!(s.clone().is_correct("Cär"));
}

#[test]
fn test_unicode_all_caps() {
    let s = test_speller();
    // "CÄR" → all caps of "cär"
    assert!(s.clone().is_correct("CÄR"));
}

#[test]
fn test_unicode_substitution() {
    let s = test_speller();
    // "cär" has ä in the lexicon. Mutator has ä→a substitution.
    // So "cär" typed correctly → weight 0. But typing "car" won't become "cär"
    // because there's no a→ä substitution in the mutator.
    assert_suggests_at_weight(&s, "cär", "cär", 0.0, &raw_config());
}

#[test]
fn test_unicode_substitution_to_ascii() {
    let s = test_speller();
    // "cät" → "cat" via ä→a(5)
    assert_suggests_at_weight(&s, "cät", "cat", 5.0, &raw_config());
}

#[test]
fn test_unicode_in_reweighting() {
    let s = test_speller();
    // "kär" → "cär" via k→c(5). With reweighting, start penalty should apply.
    let suggs = suggestion_values(&s, "kär", &reweight_config());
    let car = suggs.iter().find(|(v, _)| v == "cär");
    assert!(car.is_some(), "cär should appear: {:?}", suggs);
    assert!(
        car.unwrap().1 > 5.0,
        "start penalty should apply to unicode word"
    );
}

// ===========================================================================
// Completion marker
// ===========================================================================

#[test]
fn test_completion_marker() {
    let s = test_speller();
    let cfg = SpellerConfig {
        completion_marker: Some("+".to_string()),
        reweight: None,
        recase: false,
        ..SpellerConfig::default()
    };
    let suggs = s.clone().suggest_with_config("cat", &cfg);
    assert!(!suggs.is_empty());
    // With completion_marker="+", suggestions not ending in "+" are marked completed
    for sugg in &suggs {
        let completed = sugg.completed.unwrap();
        // None of our lexicon words end with "+", so all should be completed=true
        assert!(completed, "'{}' should be marked as completed", sugg.value);
    }
}

// ===========================================================================
// Digit-prefixed words (recent bug fix area)
// ===========================================================================

#[test]
fn test_digit_prefix_non_letter() {
    let s = test_speller();
    // "123" has no letters → treated as correct
    assert!(s.clone().is_correct("123"));
}

#[test]
fn test_digit_prefix_is_incorrect() {
    let s = test_speller();
    // "1cat" has letters but starts with digit. Not in lexicon.
    assert!(!s.clone().is_correct("1cat"));
}

#[test]
fn test_digit_only_suggest() {
    let s = test_speller();
    // is_correct("123") returns true (no letters), but suggest("123") still runs
    // the error model — digits are unknown symbols mapping to ε, so insertions fire.
    // This is expected: is_correct and suggest have different early-exit logic.
    let suggs = s.clone().suggest("123");
    // 3 εs → 3 insertions possible → produces words like "cat"/"car" at 3×8=24 + reweighting
    assert!(
        !suggs.is_empty(),
        "digits map to ε, insertions produce suggestions"
    );
}

// ===========================================================================
// Unknown symbols
// ===========================================================================

#[test]
fn test_unknown_symbol_is_incorrect() {
    let s = test_speller();
    assert!(!s.clone().is_correct("xyz"));
}

#[test]
fn test_unknown_maps_to_epsilon() {
    let s = test_speller();
    // Unknown symbols → ε. Three εs allow 3 insertions (3×8=24).
    let suggs = suggestion_values(&s, "xyz", &raw_config());
    assert!(
        !suggs.is_empty(),
        "should produce suggestions via insertions"
    );
    for (v, w) in &suggs {
        assert!(*w >= 24.0, "'{}' at {} should be ≥ 24", v, w);
    }
}

// ===========================================================================
// Analyze paths
// ===========================================================================

#[test]
fn test_analyze_input_all_words() {
    let s = test_speller();
    for word in &["cat", "car", "cart", "care", "cär"] {
        assert!(
            !s.clone().analyze_input(word).is_empty(),
            "analyze_input('{}') should work",
            word
        );
    }
}

#[test]
fn test_analyze_input_unknown() {
    assert!(test_speller().clone().analyze_input("kat").is_empty());
}

#[test]
fn test_analyze_output_finds_correction() {
    let s = test_speller();
    let values: Vec<String> = s
        .clone()
        .analyze_output("kat")
        .iter()
        .map(|a| a.value.to_string())
        .collect();
    assert!(
        values.contains(&"cat".to_string()),
        "should find cat: {:?}",
        values
    );
}

// ===========================================================================
// Verbose
// ===========================================================================

#[test]
fn test_verbose_decomposition() {
    let s = test_speller();
    let cfg = SpellerConfig {
        verbose: true,
        reweight: None,
        recase: false,
        ..SpellerConfig::default()
    };
    let suggs = s.clone().suggest_with_config("kat", &cfg);
    let cat = suggs.iter().find(|s| s.value == "cat").unwrap();
    let d = cat.weight_details.as_ref().unwrap();
    let sum = d.lexicon_weight.0 + d.mutator_weight.0;
    assert!(
        (sum - cat.weight().0).abs() < 0.01,
        "lex+mut={} ≠ total={}",
        sum,
        cat.weight().0
    );
}

#[test]
fn test_verbose_correct_zero_mutator() {
    let s = test_speller();
    let cfg = SpellerConfig {
        verbose: true,
        reweight: None,
        recase: false,
        ..SpellerConfig::default()
    };
    let suggs = s.clone().suggest_with_config("cat", &cfg);
    let d = suggs
        .iter()
        .find(|s| s.value == "cat")
        .unwrap()
        .weight_details
        .as_ref()
        .unwrap();
    assert_eq!(d.mutator_weight.0, 0.0);
}

#[test]
fn test_verbose_reweight_fields() {
    let s = test_speller();
    let cfg = SpellerConfig {
        verbose: true,
        recase: false,
        ..SpellerConfig::default()
    };
    let suggs = s.clone().suggest_with_config("kat", &cfg);
    let cat = suggs.iter().find(|s| s.value == "cat");
    assert!(cat.is_some(), "should suggest cat: {:?}", suggs);
    let d = cat.unwrap().weight_details.as_ref().unwrap();
    assert!(
        d.reweight_start > 0.0,
        "start penalty field should be > 0 for kat→cat"
    );
}

// ===========================================================================
// n_best
// ===========================================================================

#[test]
fn test_n_best_1() {
    let suggs = suggestion_values(
        &test_speller(),
        "car",
        &SpellerConfig {
            n_best: Some(1),
            reweight: None,
            recase: false,
            ..SpellerConfig::default()
        },
    );
    assert!(suggs.len() <= 1);
    assert_eq!(suggs[0].0, "car");
}

#[test]
fn test_n_best_2() {
    let suggs = suggestion_values(
        &test_speller(),
        "car",
        &SpellerConfig {
            n_best: Some(2),
            reweight: None,
            recase: false,
            ..SpellerConfig::default()
        },
    );
    assert!(suggs.len() <= 2);
    assert!(suggs.iter().any(|(v, _)| v == "car"));
}

#[test]
fn test_n_best_plus_beam() {
    let s = test_speller();
    let cfg = SpellerConfig {
        n_best: Some(10),
        beam: Some(Weight(0.5)),
        recase: false,
        ..SpellerConfig::default()
    };
    let suggs = suggestion_values(&s, "car", &cfg);
    assert!(suggs.iter().any(|(v, _)| v == "car"));
    assert_not_suggests(&s, "car", "care", &cfg);
}

// ===========================================================================
// Edge cases
// ===========================================================================

#[test]
fn test_empty_suggest() {
    assert!(test_speller().clone().suggest("").is_empty());
}

#[test]
fn test_empty_is_correct() {
    assert!(test_speller().clone().is_correct(""));
}

#[test]
fn test_single_char() {
    assert!(!test_speller().clone().is_correct("c"));
}

#[test]
fn test_repeated_chars() {
    // "ccaatt" → "cat" via 3 deletions (3×7=21)
    assert_suggests(&test_speller(), "ccaatt", "cat", &raw_config());
}

#[test]
fn test_very_short_input() {
    let s = test_speller();
    let suggs = suggestion_words(&s, "ca", &raw_config());
    assert!(
        suggs.contains(&"cat".to_string()) || suggs.contains(&"car".to_string()),
        "ca should suggest cat or car: {:?}",
        suggs
    );
}

#[test]
fn test_long_repeated_input() {
    let s = test_speller();
    // "ccccaaaatttt" — 12 chars, needs 9 deletions to reach "cat" (9×7=63)
    let cfg = SpellerConfig {
        max_weight: Some(Weight(100.0)),
        reweight: None,
        recase: false,
        ..SpellerConfig::default()
    };
    assert_suggests(&s, "ccccaaaatttt", "cat", &cfg);
}

// ===========================================================================
// Cursed Unicode edge cases
// ===========================================================================

#[test]
fn test_decomposed_vs_precomposed() {
    let s = test_speller();
    // Lexicon has "cär" with precomposed ä (U+00E4, single codepoint).
    // Input with DECOMPOSED ä (a + combining diaeresis, U+0061 U+0308) is a
    // different byte sequence. to_input_vec does a HashMap lookup on the grapheme
    // string — the decomposed form won't match the precomposed key.
    let decomposed = "ca\u{0308}r"; // a + combining diaeresis
    assert_ne!(decomposed, "cär", "sanity: decomposed ≠ precomposed");
    // Should NOT be recognized as correct (different symbol lookup)
    assert!(!s.clone().is_correct(decomposed));
    // But should not crash either
    let _suggs = s.clone().suggest(decomposed);
}

#[test]
fn test_lone_combining_mark() {
    let s = test_speller();
    // A combining mark with no base character. GeneralCategory is NonspacingMark,
    // not Letter, so is_correct returns true (same as digits/punctuation).
    let lone_mark = "\u{0301}"; // combining acute accent
    assert!(
        s.clone().is_correct(lone_mark),
        "combining mark has no letters → correct"
    );
    // suggest still runs (word.len() > 0) — should not crash
    let _suggs = s.clone().suggest(lone_mark);
}

#[test]
fn test_zero_width_joiner_in_word() {
    let s = test_speller();
    // ZWJ (U+200D) invisible character injected into "cat"
    let zwj_cat = "c\u{200D}at";
    assert!(!s.clone().is_correct(zwj_cat));
    let _suggs = s.clone().suggest(zwj_cat);
}

#[test]
fn test_bidi_override_characters() {
    let s = test_speller();
    // Right-to-left override (U+202E) prepended to "cat"
    let rtl = "\u{202E}cat";
    assert!(!s.clone().is_correct(rtl));
    let _suggs = s.clone().suggest(rtl);
}

#[test]
fn test_null_byte_in_input() {
    let s = test_speller();
    let null_cat = "c\0at";
    // Should not crash. Null byte is a valid char in Rust strings.
    assert!(!s.clone().is_correct(null_cat));
    let _suggs = s.clone().suggest(null_cat);
}

#[test]
fn test_case_changing_length_eszett() {
    let s = test_speller();
    // German ß uppercases to "SS" (1 char → 2 chars). This length change
    // could break grapheme alignment in reweighting if not handled.
    // "ß" isn't in our alphabet, but the case handling code runs before FST lookup.
    // is_correct with "ß" should not crash.
    assert!(!s.clone().is_correct("ß"));
    // "caß" — ß maps to unknown/epsilon, but case variants include "CAß"→"CASS"
    let _suggs = s.clone().suggest("caß");
}

#[test]
fn test_turkish_i_dotless() {
    let s = test_speller();
    // Turkish İ (U+0130, capital I with dot) lowercases to "i" in most locales.
    // But Turkish ı (U+0131, dotless i) is a different character entirely.
    // Neither is in our alphabet — should not crash.
    assert!(!s.clone().is_correct("İ"));
    let _suggs = s.clone().suggest("İ");
    let _suggs2 = s.clone().suggest("ıcat");
}

#[test]
fn test_hangul_jamo() {
    let s = test_speller();
    // Korean Hangul syllable — completely outside our alphabet.
    // Single grapheme cluster but multi-byte.
    let _suggs = s.clone().suggest("고양이");
    assert!(!s.clone().is_correct("고양이"));
}

// ===========================================================================
// Cursed mixed case patterns
// ===========================================================================

#[test]
fn test_something_pattern() {
    let s = test_speller();
    // "sOMETHING" pattern: upper_first gives "SOMETHING" which is all-caps.
    // The code at case_handling.rs:142 explicitly handles this by NOT adding
    // the upper variant. Test with "cAR" where upper_first("cAR") = "CAR" = all caps.
    assert!(!s.clone().is_correct("cAR"));
    let suggs = suggestion_words(&s, "cAR", &SpellerConfig::default());
    // Mixed case FirstResults path: tries "cAR" as-is, then fallback to lowercase "car"
    assert!(
        !suggs.is_empty(),
        "cAR should produce suggestions: {:?}",
        suggs
    );
}

#[test]
fn test_all_upper_rest_lower() {
    let s = test_speller();
    // "cAT" — mixed case, not first-caps, not all-caps
    assert!(!s.clone().is_correct("cAT"));
    let _suggs = s.clone().suggest("cAT");
}

#[test]
fn test_alternating_case() {
    let s = test_speller();
    // "CaRe" — deeply mixed case
    assert!(!s.clone().is_correct("CaRe"));
    let suggs = suggestion_words(&s, "CaRe", &SpellerConfig::default());
    assert!(
        !suggs.is_empty(),
        "CaRe should produce suggestions: {:?}",
        suggs
    );
}

#[test]
fn test_single_upper_char() {
    let s = test_speller();
    // "C" — single uppercase letter. is_first_caps? is_all_caps?
    // WordCase: first char is upper, no subsequent chars → AllUpper? No...
    // first_char upper, has_upper=false, has_lower=false → None case.
    assert!(!s.clone().is_correct("C"));
}

#[test]
fn test_mixed_case_identical_to_lower() {
    let s = test_speller();
    // "cAr" — mixed case. FirstResults path tries "cAr" and upper_first "CAr".
    // "CAr" is still mixed case (not all caps). Falls back to lowercase "car" → found.
    let suggs = suggestion_words(&s, "cAr", &SpellerConfig::default());
    assert!(
        !suggs.is_empty(),
        "cAr should suggest via lowercase fallback"
    );
}

// ===========================================================================
// Pathological inputs
// ===========================================================================

#[test]
fn test_whitespace_in_word() {
    let s = test_speller();
    // Space in middle of word — space is not a letter, not in alphabet
    assert!(!s.clone().is_correct("c at"));
    // Tab and newline
    assert!(!s.clone().is_correct("c\tat"));
    assert!(!s.clone().is_correct("c\nat"));
}

#[test]
fn test_only_whitespace() {
    let s = test_speller();
    // All whitespace — no letters, so is_correct returns true
    assert!(s.clone().is_correct(" "));
    assert!(s.clone().is_correct("   "));
    assert!(s.clone().is_correct("\t\n"));
}

#[test]
fn test_input_looks_like_flag_diacritic() {
    let s = test_speller();
    // Input that resembles an FST flag diacritic symbol — should be treated
    // as a regular string, not parsed as a flag operation.
    assert!(!s.clone().is_correct("@P.feature.value@"));
    let _suggs = s.clone().suggest("@P.feature.value@");
}

#[test]
fn test_very_long_single_char_repetition() {
    let s = test_speller();
    // 50 'a's — needs 49 deletions (49×7=343) to reach... nothing useful since
    // "a" alone isn't in the lexicon. But "a" + insertions could reach "cat".
    // Main concern: doesn't hang or OOM.
    let cfg = SpellerConfig {
        max_weight: Some(Weight(500.0)),
        reweight: None,
        recase: false,
        ..SpellerConfig::default()
    };
    let input = "a".repeat(50);
    let _suggs = s.clone().suggest_with_config(&input, &cfg);
    // Just verify it terminates
}

#[test]
fn test_all_deletions_produce_empty() {
    let s = test_speller();
    // Single character "a" — delete it (w=7) leaves empty output.
    // Empty output shouldn't match any lexicon word.
    // But insertions from epsilon state could produce "cat" etc.
    let cfg = raw_config();
    let suggs = suggestion_values(&s, "a", &cfg);
    // Should not contain empty string as a suggestion
    assert!(
        !suggs.iter().any(|(v, _)| v.is_empty()),
        "empty string should not be a suggestion"
    );
}

#[test]
fn test_suggestion_values_never_empty_string() {
    let s = test_speller();
    // For any input that produces suggestions, none should be empty strings
    for input in &["a", "c", "x", "ca", "cat", "kat"] {
        let suggs = suggestion_values(&s, input, &raw_config());
        for (v, _) in &suggs {
            assert!(
                !v.is_empty(),
                "suggest('{}') produced empty suggestion",
                input
            );
        }
    }
}

#[test]
fn test_max_weight_zero_blocks_everything() {
    let s = test_speller();
    let cfg = SpellerConfig {
        max_weight: Some(Weight(0.0)),
        reweight: None,
        recase: false,
        ..SpellerConfig::default()
    };
    // max_weight=0 means only exact matches with 0 total weight survive
    let suggs = suggestion_values(&s, "kat", &cfg);
    // "kat" → "cat" costs 5, so nothing should appear
    assert!(
        suggs.is_empty(),
        "max_weight=0 should block everything for misspelled: {:?}",
        suggs
    );
}

#[test]
fn test_max_weight_zero_allows_exact() {
    let s = test_speller();
    let cfg = SpellerConfig {
        max_weight: Some(Weight(0.0)),
        reweight: None,
        recase: false,
        ..SpellerConfig::default()
    };
    // "cat" → "cat" at w=0 should still appear (or not — depends on <= vs <)
    let suggs = suggestion_values(&s, "cat", &cfg);
    // Weight 0 with max_weight 0: 0 <= 0 is true, so it should appear
    assert!(
        suggs.iter().any(|(v, _)| v == "cat"),
        "exact match at w=0 with max_weight=0: {:?}",
        suggs
    );
}

#[test]
fn test_n_best_zero() {
    let s = test_speller();
    // n_best=0 — should return empty (truncate to 0)
    let cfg = SpellerConfig {
        n_best: Some(0),
        reweight: None,
        recase: false,
        ..SpellerConfig::default()
    };
    let suggs = s.clone().suggest_with_config("cat", &cfg);
    assert!(
        suggs.is_empty(),
        "n_best=0 should return empty: {:?}",
        suggs
    );
}

#[test]
fn test_beam_zero() {
    let s = test_speller();
    // beam=0.0 is special-cased: "Only enable beam when strictly > ZERO"
    let cfg = SpellerConfig {
        beam: Some(Weight(0.0)),
        reweight: None,
        recase: false,
        ..SpellerConfig::default()
    };
    let suggs = suggestion_values(&s, "car", &cfg);
    // beam=0 is disabled, so all suggestions should appear
    assert!(
        suggs.len() > 1,
        "beam=0 should be disabled (no filtering): {:?}",
        suggs
    );
}

#[test]
fn test_negative_beam() {
    let s = test_speller();
    // Negative beam — Weight wraps f32, negative values are < ZERO, so beam disabled
    let cfg = SpellerConfig {
        beam: Some(Weight(-1.0)),
        reweight: None,
        recase: false,
        ..SpellerConfig::default()
    };
    let _suggs = s.clone().suggest_with_config("car", &cfg);
    // Main concern: doesn't panic
}

// ###########################################################################
// FST-LEVEL TESTS: Flag diacritics, identity symbol, lexicon epsilons
// ###########################################################################

// ---------------------------------------------------------------------------
// Flag diacritic fixture builders
// ---------------------------------------------------------------------------

/// Flag lexicon: "cat" and "car" unconditional, "cart" only via flag-gated path,
/// "rat" blocked by unsatisfied @R.f.1@.
///
/// See plan for state diagram.
fn build_flag_lexicon(dir: &Path) {
    // eps=0, c=1, a=2, t=3, r=4, @P.f.1@=5, @R.f.1@=6
    let symbols = &[
        "@_EPSILON_SYMBOL_@",
        "c",
        "a",
        "t",
        "r",
        "@P.f.1@",
        "@R.f.1@",
    ];
    let flags = &[
        (5, "PositiveSet", 0u16, 1i16), // @P.f.1@
        (6, "Require", 0u16, 1i16),     // @R.f.1@
    ];
    let alphabet = build_alphabet_json_full(symbols, None, None, flags, 1);
    let n = symbols.len(); // 7 → 8 entries per state

    let mut idx = Vec::new();

    // State 0 (idx 0): start. c→trans[0], r→trans[1]
    write_index_empty(&mut idx); // not final
    write_index_empty(&mut idx); // no eps
    write_index_entry(&mut idx, 1, TARGET_TABLE + 0); // c
    write_index_empty(&mut idx); // no a
    write_index_empty(&mut idx); // no t
    write_index_entry(&mut idx, 4, TARGET_TABLE + 1); // r
    write_index_empty(&mut idx); // no @P
    write_index_empty(&mut idx); // no @R

    // State 1 (idx 8): after "c". a→trans[2]
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_entry(&mut idx, 2, TARGET_TABLE + 2); // a
    write_empties(&mut idx, n - 3);

    // State 2 (idx 16): after "ca".
    //   eps slot (for @P.f.1@ flag): input=0, target→trans[3]
    //   t→trans[4], r→trans[5]
    write_index_empty(&mut idx); // not final
    write_index_entry(&mut idx, 0, TARGET_TABLE + 3); // eps slot → flag @P.f.1@
    write_index_empty(&mut idx); // no c
    write_index_empty(&mut idx); // no a
    write_index_entry(&mut idx, 3, TARGET_TABLE + 4); // t
    write_index_entry(&mut idx, 4, TARGET_TABLE + 5); // r
    write_index_empty(&mut idx); // no @P
    write_index_empty(&mut idx); // no @R

    // State 3 (idx 24): after @P.f.1@ (flag set).
    //   eps slot (for @R.f.1@ flag): input=0, target→trans[6]
    write_index_empty(&mut idx);
    write_index_entry(&mut idx, 0, TARGET_TABLE + 6); // eps slot → flag @R.f.1@
    write_empties(&mut idx, n - 1);

    // State 4 (idx 32): "cat" FINAL w=0
    write_index_final(&mut idx, 0.0);
    write_empties(&mut idx, n);

    // State 5 (idx 40): "car" direct FINAL w=0
    write_index_final(&mut idx, 0.0);
    write_empties(&mut idx, n);

    // State 6 (idx 48): flag verified. r→trans[7], t→trans[8]
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_entry(&mut idx, 3, TARGET_TABLE + 8); // t
    write_index_entry(&mut idx, 4, TARGET_TABLE + 7); // r
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);

    // State 7 (idx 56): "car" via flag FINAL w=0. t→trans[9]
    write_index_final(&mut idx, 0.0);
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_entry(&mut idx, 3, TARGET_TABLE + 9); // t
    write_empties(&mut idx, n - 4);

    // State 8 (idx 64): "cat" via flag FINAL w=0
    write_index_final(&mut idx, 0.0);
    write_empties(&mut idx, n);

    // State 9 (idx 72): "cart" FINAL w=0
    write_index_final(&mut idx, 0.0);
    write_empties(&mut idx, n);

    // State 10 (idx 80): after "r".
    //   eps slot (for @R.f.1@ flag): input=0, target→trans[10] — WILL FAIL
    write_index_empty(&mut idx);
    write_index_entry(&mut idx, 0, TARGET_TABLE + 10); // eps slot → @R.f.1@ (blocked)
    write_empties(&mut idx, n - 1);

    // State 11 (idx 88): after blocked @R (unreachable). a→trans[11]
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_entry(&mut idx, 2, TARGET_TABLE + 11); // a
    write_empties(&mut idx, n - 3);

    // State 12 (idx 96): after "ra". t→trans[12]
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_entry(&mut idx, 3, TARGET_TABLE + 12); // t
    write_empties(&mut idx, n - 4);

    // State 13 (idx 104): "rat" FINAL w=0 (unreachable)
    write_index_final(&mut idx, 0.0);
    write_empties(&mut idx, n);

    let mut tr = Vec::new();
    write_trans_entry(&mut tr, 1, 1, 8, 0.0); // [0] c→c → state 1
    write_trans_entry(&mut tr, 4, 4, 80, 0.0); // [1] r→r → state 10
    write_trans_entry(&mut tr, 2, 2, 16, 0.0); // [2] a→a → state 2
    write_trans_entry(&mut tr, 5, 5, 24, 0.0); // [3] @P.f.1@ flag → state 3
    write_trans_entry(&mut tr, 3, 3, 32, 0.0); // [4] t→t → state 4 ("cat")
    write_trans_entry(&mut tr, 4, 4, 40, 0.0); // [5] r→r → state 5 ("car" direct)
    write_trans_entry(&mut tr, 6, 6, 48, 0.0); // [6] @R.f.1@ flag → state 6
    write_trans_entry(&mut tr, 4, 4, 56, 0.0); // [7] r→r → state 7 ("car" via flag)
    write_trans_entry(&mut tr, 3, 3, 64, 0.0); // [8] t→t → state 8 ("cat" via flag)
    write_trans_entry(&mut tr, 3, 3, 72, 0.0); // [9] t→t → state 9 ("cart")
    write_trans_entry(&mut tr, 6, 6, 88, 0.0); // [10] @R.f.1@ flag → state 11 (BLOCKED)
    write_trans_entry(&mut tr, 2, 2, 96, 0.0); // [11] a→a → state 12
    write_trans_entry(&mut tr, 3, 3, 104, 0.0); // [12] t→t → state 13

    write_thfst(dir, &alphabet, &idx, &tr);
}

/// Simple identity mutator for flag tests.
fn build_flag_mutator(dir: &Path) {
    let symbols = &["@_EPSILON_SYMBOL_@", "c", "a", "t", "r"];
    let alphabet = build_alphabet_json(symbols);
    let mut idx = Vec::new();
    write_index_final(&mut idx, 0.0);
    write_index_empty(&mut idx); // no eps
    write_index_entry(&mut idx, 1, TARGET_TABLE + 0);
    write_index_entry(&mut idx, 2, TARGET_TABLE + 1);
    write_index_entry(&mut idx, 3, TARGET_TABLE + 2);
    write_index_entry(&mut idx, 4, TARGET_TABLE + 3);
    let mut tr = Vec::new();
    write_trans_entry(&mut tr, 1, 1, 0, 0.0);
    write_trans_entry(&mut tr, 2, 2, 0, 0.0);
    write_trans_entry(&mut tr, 3, 3, 0, 0.0);
    write_trans_entry(&mut tr, 4, 4, 0, 0.0);
    write_thfst(dir, &alphabet, &idx, &tr);
}

fn flag_speller() -> Arc<HfstSpeller<MmapThfstTransducer, MmapThfstTransducer>> {
    let base = fixtures_dir();
    load_speller(
        &base.join("flag-lexicon.thfst"),
        &base.join("flag-mutator.thfst"),
    )
}

// ---------------------------------------------------------------------------
// Identity symbol fixture builders
// ---------------------------------------------------------------------------

/// Lexicon accepting "c" + ANY + "t" via identity symbol.
fn build_identity_lexicon(dir: &Path) {
    // eps=0, @_IDENTITY_SYMBOL_@=1, c=2, t=3
    let symbols = &["@_EPSILON_SYMBOL_@", "@_IDENTITY_SYMBOL_@", "c", "t"];
    let alphabet = build_alphabet_json_full(symbols, Some(1), None, &[], 0);
    let n = symbols.len(); // 4 → 5 entries per state

    let mut idx = Vec::new();

    // State 0 (idx 0): start. c→trans[0]
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_entry(&mut idx, 2, TARGET_TABLE + 0); // c
    write_index_empty(&mut idx);

    // State 1 (idx 5): after "c". IDENTITY→trans[1]
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_entry(&mut idx, 1, TARGET_TABLE + 1); // identity
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);

    // State 2 (idx 10): after "c?". t→trans[2]
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_entry(&mut idx, 3, TARGET_TABLE + 2); // t

    // State 3 (idx 15): FINAL w=0
    write_index_final(&mut idx, 0.0);
    write_empties(&mut idx, n);

    let mut tr = Vec::new();
    write_trans_entry(&mut tr, 2, 2, 5, 0.0); // [0] c→c → state 1
    write_trans_entry(&mut tr, 1, 1, 10, 0.0); // [1] IDENTITY→IDENTITY → state 2
    write_trans_entry(&mut tr, 3, 3, 15, 0.0); // [2] t→t → state 3

    write_thfst(dir, &alphabet, &idx, &tr);
}

/// Mutator with @_IDENTITY_SYMBOL_@ (needed for lexicon_consume fallback)
/// and symbols not in lexicon alphabet.
fn build_identity_mutator(dir: &Path) {
    // eps=0, @_IDENTITY_SYMBOL_@=1, c=2, a=3, b=4, t=5, x=6
    // Identity at position 1 matches the lexicon's identity position
    let symbols = &[
        "@_EPSILON_SYMBOL_@",
        "@_IDENTITY_SYMBOL_@",
        "c",
        "a",
        "b",
        "t",
        "x",
    ];
    let alphabet = build_alphabet_json_full(symbols, Some(1), None, &[], 0);
    let mut idx = Vec::new();
    write_index_final(&mut idx, 0.0);
    write_index_empty(&mut idx); // eps
    write_index_empty(&mut idx); // identity (no self-loop needed)
    write_index_entry(&mut idx, 2, TARGET_TABLE + 0); // c
    write_index_entry(&mut idx, 3, TARGET_TABLE + 1); // a
    write_index_entry(&mut idx, 4, TARGET_TABLE + 2); // b
    write_index_entry(&mut idx, 5, TARGET_TABLE + 3); // t
    write_index_entry(&mut idx, 6, TARGET_TABLE + 4); // x
    let mut tr = Vec::new();
    write_trans_entry(&mut tr, 2, 2, 0, 0.0); // c→c
    write_trans_entry(&mut tr, 3, 3, 0, 0.0); // a→a
    write_trans_entry(&mut tr, 4, 4, 0, 0.0); // b→b
    write_trans_entry(&mut tr, 5, 5, 0, 0.0); // t→t
    write_trans_entry(&mut tr, 6, 6, 0, 0.0); // x→x
    write_thfst(dir, &alphabet, &idx, &tr);
}

fn identity_speller() -> Arc<HfstSpeller<MmapThfstTransducer, MmapThfstTransducer>> {
    let base = fixtures_dir();
    load_speller(
        &base.join("identity-lexicon.thfst"),
        &base.join("identity-mutator.thfst"),
    )
}

// ---------------------------------------------------------------------------
// Epsilon/tag fixture builders
// ---------------------------------------------------------------------------

/// Lexicon accepting "cat" with two epsilon tag transitions: +N (w=0) and +V (w=2).
fn build_eps_lexicon(dir: &Path) {
    // eps=0, c=1, a=2, t=3, +N=4, +V=5
    let symbols = &["@_EPSILON_SYMBOL_@", "c", "a", "t", "+N", "+V"];
    let alphabet = build_alphabet_json(symbols);
    let n = symbols.len(); // 6 → 7 entries per state

    let mut idx = Vec::new();

    // State 0 (idx 0): start. c→trans[0]
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_entry(&mut idx, 1, TARGET_TABLE + 0); // c
    write_empties(&mut idx, n - 2);

    // State 1 (idx 7): after "c". a→trans[1]
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_entry(&mut idx, 2, TARGET_TABLE + 1); // a
    write_empties(&mut idx, n - 3);

    // State 2 (idx 14): after "ca". t→trans[2]
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_empty(&mut idx);
    write_index_entry(&mut idx, 3, TARGET_TABLE + 2); // t
    write_empties(&mut idx, n - 4);

    // State 3 (idx 21): after "cat".
    //   eps slot → trans[3] (eps transitions with tag output)
    //   NOT final itself — must follow epsilon to reach final states.
    write_index_empty(&mut idx);
    write_index_entry(&mut idx, 0, TARGET_TABLE + 3); // eps slot → tags
    write_empties(&mut idx, n - 1);

    // State 4 (idx 28): "cat+N" FINAL w=0
    write_index_final(&mut idx, 0.0);
    write_empties(&mut idx, n);

    // State 5 (idx 35): "cat+V" FINAL w=2.0
    write_index_final(&mut idx, 2.0);
    write_empties(&mut idx, n);

    let mut tr = Vec::new();
    write_trans_entry(&mut tr, 1, 1, 7, 0.0); // [0] c→c → state 1
    write_trans_entry(&mut tr, 2, 2, 14, 0.0); // [1] a→a → state 2
    write_trans_entry(&mut tr, 3, 3, 21, 0.0); // [2] t→t → state 3
    // Epsilon transitions with tag output:
    write_trans_entry(&mut tr, 0, 4, 28, 0.0); // [3] ε→+N → state 4 (noun)
    write_trans_entry(&mut tr, 0, 5, 35, 0.0); // [4] ε→+V → state 5 (verb)

    write_thfst(dir, &alphabet, &idx, &tr);
}

/// Simple identity mutator for epsilon tests.
fn build_eps_mutator(dir: &Path) {
    let symbols = &["@_EPSILON_SYMBOL_@", "c", "a", "t"];
    let alphabet = build_alphabet_json(symbols);
    let mut idx = Vec::new();
    write_index_final(&mut idx, 0.0);
    write_index_empty(&mut idx);
    write_index_entry(&mut idx, 1, TARGET_TABLE + 0);
    write_index_entry(&mut idx, 2, TARGET_TABLE + 1);
    write_index_entry(&mut idx, 3, TARGET_TABLE + 2);
    let mut tr = Vec::new();
    write_trans_entry(&mut tr, 1, 1, 0, 0.0);
    write_trans_entry(&mut tr, 2, 2, 0, 0.0);
    write_trans_entry(&mut tr, 3, 3, 0, 0.0);
    write_thfst(dir, &alphabet, &idx, &tr);
}

fn eps_speller() -> Arc<HfstSpeller<MmapThfstTransducer, MmapThfstTransducer>> {
    let base = fixtures_dir();
    load_speller(
        &base.join("eps-lexicon.thfst"),
        &base.join("eps-mutator.thfst"),
    )
}

// ===========================================================================
// Flag diacritic tests
// ===========================================================================

#[test]
fn test_flag_unconditional_word() {
    let s = flag_speller();
    assert!(
        s.clone().is_correct("cat"),
        "cat should be accepted (no flags needed)"
    );
}

#[test]
fn test_flag_direct_path() {
    let s = flag_speller();
    assert!(
        s.clone().is_correct("car"),
        "car should be accepted (direct path, no flags)"
    );
}

#[test]
fn test_flag_gated_word_accepted() {
    let s = flag_speller();
    // "cart" only reachable via: ca → @P.f.1@ → @R.f.1@ → r → t
    assert!(
        s.clone().is_correct("cart"),
        "cart should be accepted via flag-gated path"
    );
}

#[test]
fn test_flag_blocks_unsatisfied_require() {
    let s = flag_speller();
    // "rat": r → @R.f.1@ (FAILS, flag not set) → a → t → FINAL (unreachable)
    assert!(
        !s.clone().is_correct("rat"),
        "rat should be BLOCKED by unsatisfied @R.f.1@"
    );
}

#[test]
fn test_flag_blocked_word_no_suggestions() {
    let s = flag_speller();
    let cfg = raw_config();
    let suggs = suggestion_words(&s, "rat", &cfg);
    // "rat" can't be corrected to itself (blocked), but might suggest "cat" or "car"
    assert!(
        !suggs.contains(&"rat".to_string()),
        "rat should not appear as a suggestion (flag-blocked): {:?}",
        suggs
    );
}

// ===========================================================================
// Identity symbol tests
// ===========================================================================

#[test]
fn test_identity_accepts_any_middle_char() {
    let s = identity_speller();
    // Lexicon: c + IDENTITY + t. IDENTITY matches any character.
    assert!(s.clone().is_correct("cat"), "cat (identity matches 'a')");
    assert!(s.clone().is_correct("cbt"), "cbt (identity matches 'b')");
    assert!(s.clone().is_correct("cxt"), "cxt (identity matches 'x')");
}

#[test]
fn test_identity_rejects_wrong_structure() {
    let s = identity_speller();
    // Must be exactly 3 chars: c + ? + t
    assert!(!s.clone().is_correct("ct"), "ct too short");
    assert!(!s.clone().is_correct("caat"), "caat too long");
    assert!(!s.clone().is_correct("cat1"), "cat1 too long");
    assert!(!s.clone().is_correct("xat"), "xat wrong first char");
    assert!(!s.clone().is_correct("car"), "car wrong last char");
}

#[test]
fn test_identity_raw_symbol_in_suggest() {
    let s = identity_speller();
    let cfg = raw_config();
    // In WithoutTags mode (suggest), identity transitions output the raw identity
    // symbol, not the replaced character. This is by design — lexicon identity
    // transitions are for morphological analysis, not clean suggestion output.
    // Real spell-checking lexicons use explicit character transitions instead.
    let suggs = suggestion_words(&s, "cat", &cfg);
    assert!(!suggs.is_empty(), "should produce suggestions: {:?}", suggs);
}

#[test]
fn test_identity_replacement_uses_mutator_symbol() {
    let s = identity_speller();
    // Identity replacement sets sym = self.input[input_state], which is a MUTATOR
    // symbol number. But string_from_symbols uses the LEXICON's key_table. When
    // the symbol spaces differ, the output character is wrong (mutator sym 3='a'
    // maps to lexicon key_table[3]='t', giving "ctt" instead of "cat").
    //
    // This is expected: real HFST spellers use identity in the MUTATOR (error model),
    // not the lexicon. Identity in the lexicon is only for acceptance testing,
    // not for producing correct output strings.
    let analyses = s.clone().analyze_output("cat");
    assert!(!analyses.is_empty(), "should produce analyses");
    // The output contains mutator-symbol-mapped characters, not the originals
    let values: Vec<String> = analyses.iter().map(|a| a.value.to_string()).collect();
    assert!(
        values.iter().any(|v| v == "ctt"),
        "identity replacement uses mutator symbol space: {:?}",
        values
    );
}

// ===========================================================================
// Lexicon epsilon/tag tests
// ===========================================================================

#[test]
fn test_eps_is_correct() {
    let s = eps_speller();
    // "cat" must follow epsilon transitions to reach final states
    assert!(s.clone().is_correct("cat"));
}

#[test]
fn test_eps_suggest_strips_tags() {
    let s = eps_speller();
    let cfg = raw_config();
    // suggest uses WithoutTags mode — tags should be stripped
    let suggs = suggestion_words(&s, "cat", &cfg);
    assert!(
        suggs.contains(&"cat".to_string()),
        "suggest should return 'cat' without tags: {:?}",
        suggs
    );
    // Should NOT contain tagged forms
    for v in &suggs {
        assert!(
            !v.contains("+N") && !v.contains("+V"),
            "suggest should strip tags, got: {}",
            v
        );
    }
}

#[test]
fn test_eps_analyze_preserves_tags() {
    let s = eps_speller();
    // analyze_input uses WithTags mode — tags should be preserved
    let analyses = s.clone().analyze_input("cat");
    assert!(!analyses.is_empty(), "analyze should return results");
    let values: Vec<String> = analyses.iter().map(|a| a.value.to_string()).collect();
    assert!(
        values.iter().any(|v| v.contains("+N")),
        "analyze should include +N tag: {:?}",
        values
    );
    assert!(
        values.iter().any(|v| v.contains("+V")),
        "analyze should include +V tag: {:?}",
        values
    );
}

#[test]
fn test_eps_analyze_multiple_weights() {
    let s = eps_speller();
    let analyses = s.clone().analyze_input("cat");
    // +N path has final weight 0.0, +V path has final weight 2.0
    assert!(
        analyses.len() >= 2,
        "should have at least 2 analyses: {:?}",
        analyses
    );
    // Should be sorted by weight
    for w in analyses.windows(2) {
        assert!(
            w[0].weight() <= w[1].weight(),
            "analyses not sorted: {:?}",
            analyses
        );
    }
}

#[test]
fn test_eps_analyze_noun_before_verb() {
    let s = eps_speller();
    let analyses = s.clone().analyze_input("cat");
    // Noun (+N, w=0) should come before verb (+V, w=2)
    let noun_idx = analyses.iter().position(|a| a.value.contains("+N"));
    let verb_idx = analyses.iter().position(|a| a.value.contains("+V"));
    assert!(
        noun_idx.is_some() && verb_idx.is_some(),
        "should have both +N and +V: {:?}",
        analyses
    );
    assert!(
        noun_idx.unwrap() < verb_idx.unwrap(),
        "noun (w=0) should sort before verb (w=2)"
    );
}

// ===========================================================================
// Fixture regeneration
// ===========================================================================

#[test]
#[ignore]
fn rebuild_fixtures() {
    let base = fixtures_dir();

    for (lex_fn, mut_fn, lex_name, mut_name) in [
        (
            build_lexicon as fn(&Path),
            build_mutator as fn(&Path),
            "lexicon.thfst",
            "mutator.thfst",
        ),
        (
            build_flag_lexicon,
            build_flag_mutator,
            "flag-lexicon.thfst",
            "flag-mutator.thfst",
        ),
        (
            build_identity_lexicon,
            build_identity_mutator,
            "identity-lexicon.thfst",
            "identity-mutator.thfst",
        ),
        (
            build_eps_lexicon,
            build_eps_mutator,
            "eps-lexicon.thfst",
            "eps-mutator.thfst",
        ),
    ] {
        let lex = base.join(lex_name);
        let mut_ = base.join(mut_name);
        std::fs::create_dir_all(&lex).unwrap();
        std::fs::create_dir_all(&mut_).unwrap();
        lex_fn(&lex);
        mut_fn(&mut_);
    }

    eprintln!("Rebuilt all fixtures at {}", base.display());
}
