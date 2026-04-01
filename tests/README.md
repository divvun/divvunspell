# Integration Tests: Synthetic THFST Transducers

The `speller_integration.rs` test file builds minimal THFST transducers from
scratch and commits them as fixtures under `tests/fixtures/`. Tests load them
directly and exercise the full speller pipeline.

## What's in the test transducers

### Lexicon (acceptor)

Accepts exactly four words:

| Word   | Weight |
|--------|--------|
| `cat`  | 0.0    |
| `car`  | 0.0    |
| `cart` | 1.0    |
| `care` | 0.0    |

Alphabet symbols: `[eps, "c", "a", "t", "r", "e"]`

State diagram:

```
(0)--c-->(1)--a-->(2)--t-->(3) FINAL w=0.0   "cat"
                      |
                      r-->(4) FINAL w=0.0     "car"
                            |
                            t-->(5) FINAL w=1.0  "cart"
                            e-->(6) FINAL w=0.0  "care"
```

### Mutator (error model)

A single-state transducer that models:

- **Identity**: every symbol passes through unchanged at weight 0.0
  (self-loops on `c`, `a`, `t`, `r`, `k`, `e`, `d`)
- **Substitutions** (all weight 5.0):
  - `k` -> `c` (start-of-word errors: `kat` -> `cat`)
  - `e` -> `a` (mid-word errors: `cet` -> `cat`)
  - `d` -> `t` (end-of-word errors: `cad` -> `cat`)

Alphabet symbols: `[eps, "c", "a", "t", "r", "k", "e", "d"]`

```
       c:c/0  a:a/0  t:t/0  r:r/0  k:k/0  e:e/0  d:d/0
       .---------------self-loops-----------------.
       v                                          |
      (0) FINAL w=0.0 <--------------------------'
       ^              |              |             |
       '-- k:c/5.0 --'-- e:a/5.0 --'-- d:t/5.0 --'
```

### Error positions tested

| Input  | Output | Substitutions      | Mutator w | Error position  |
|--------|--------|--------------------|-----------|-----------------|
| `kat`  | `cat`  | k->c               | 5         | start           |
| `cet`  | `cat`  | e->a               | 5         | mid             |
| `cad`  | `cat`  | d->t               | 5         | end             |
| `kare` | `care` | k->c               | 5         | start (4-char)  |
| `card` | `cart`  | d->t              | 5 (+1 lex)| end (4-char)    |
| `ked`  | `cat`  | k->c, e->a, d->t   | 15        | start+mid+end   |

## THFST binary format (quick reference)

Each THFST transducer is a directory containing three files:

| File         | Format                          | Purpose                        |
|--------------|---------------------------------|--------------------------------|
| `alphabet`   | JSON (`TransducerAlphabet`)     | Symbol table + metadata        |
| `index`      | Binary, 8 bytes/entry (LE)      | Sparse state array             |
| `transition` | Binary, 12 bytes/entry (LE)     | Arc (transition) records       |

**Index table entry** (8 bytes):

```
u16  input_symbol   (0xFFFF = no transition / final marker)
u16  padding
u32  target          (0xFFFFFFFF = empty slot; otherwise TARGET_TABLE + trans_index,
                      or f32 weight bits for final states)
```

State `s` with `N` alphabet symbols occupies `N+1` consecutive index entries:
- `[s+0]`: state header. Final if `input_symbol == 0xFFFF && target != 0xFFFFFFFF`
  (the target field holds the final weight as f32 bits).
- `[s+1+sym]`: slot for symbol number `sym`. If `input_symbol == sym`, there is a
  transition; `target` points into the transition table as `TARGET_TABLE + offset`.

**Transition table entry** (12 bytes):

```
u16  input_symbol    (0xFFFF = no more transitions)
u16  output_symbol
u32  target          (index table position of the destination state)
f32  weight
```

Transitions for the same (state, symbol) pair are stored contiguously.
Iteration stops when `input_symbol` no longer matches.

`TARGET_TABLE = 0x80000000` is the constant that distinguishes index-table
state references (< TARGET_TABLE) from transition-table references
(>= TARGET_TABLE).

## What the tests cover

| Test                                 | Exercises                                             |
|--------------------------------------|-------------------------------------------------------|
| `test_is_correct`                    | Lexicon traversal for all 4 words + negatives          |
| `test_is_correct_empty_and_nonletter`| Empty string, digits, punctuation always "correct"     |
| `test_is_correct_first_caps`         | "Cat"/"Car"/"Care" recognized via case lowering        |
| `test_is_correct_all_caps`           | "CAT"/"CAR"/"CART" recognized via all-caps lowering    |
| `test_exact_weights_correct_word`    | "cat" self-suggests at weight 0.0                      |
| `test_exact_weights_lexicon_weight`  | "cart" has lexicon weight 1.0                          |
| `test_exact_weights_single_substitution` | "kat"->"cat" = 5.0 (k->c)                         |
| `test_exact_weights_mid_substitution`| "cet"->"cat" = 5.0 (e->a)                             |
| `test_exact_weights_end_substitution`| "cad"->"cat" = 5.0 (d->t)                             |
| `test_exact_weights_multi_substitution`| "ked"->"cat" = 15.0 (k->c + e->a + d->t)            |
| `test_suggestion_ordering_*`         | Suggestions sorted by weight                           |
| `test_reweight_start_penalty`        | First grapheme error gets start penalty (10.0)         |
| `test_reweight_mid_penalty`          | Middle error gets mid penalty (5.0), less than start   |
| `test_reweight_end_penalty`          | Last grapheme error gets end penalty (10.0)            |
| `test_reweight_no_penalty_for_correct`| Correct word stays at weight 0.0 after reweighting    |
| `test_beam_filtering`                | beam=0.5 excludes "cart" (w=1) from "cat" suggestions  |
| `test_max_weight_cutoff`             | max_weight=3.0 blocks "cat" from "kat" (w=5)           |
| `test_max_weight_allows_lighter`     | max_weight=6.0 allows it through                       |
| `test_suggest_first_caps_output`     | "Kat" -> "Cat" (case mutation preserved)               |
| `test_suggest_all_caps_output`       | "KAT" -> "CAT" (case mutation preserved)               |
| `test_unknown_symbol_is_incorrect`   | "xyz" (not in alphabet) is incorrect                   |
| `test_unknown_symbol_suggest`        | "xyz" produces no suggestions                          |
| `test_analyze_input_correct_word`    | analyze_input("cat") returns analysis                  |
| `test_analyze_input_unknown_word`    | analyze_input("kat") returns nothing                   |
| `test_analyze_output`                | analyze_output("kat") finds "cat" via error model      |
| `test_verbose_weight_breakdown`      | lexicon_weight + mutator_weight = total weight          |
| `test_verbose_correct_word`          | Correct word has mutator_weight = 0                    |
| `test_n_best_limits_count`           | n_best=1 returns at most 1 suggestion                  |
| `test_n_best_returns_best`           | n_best=1 returns the lowest-weight suggestion           |
| `test_empty_input_suggest`           | suggest("") returns empty                              |
| `test_single_char_correct`           | "c" (incomplete word) is incorrect                     |
| `test_longer_word_start_error`       | "kare"->"care" at weight 5.0                           |
| `test_longer_word_end_error`         | "card"->"cart" at weight 6.0 (5 mutator + 1 lexicon)   |

## Committed fixture files

```
tests/fixtures/
  lexicon.thfst/   (alphabet, index, transition)
  mutator.thfst/   (alphabet, index, transition)
```

Tests load them directly -- no temp dirs, no build step.

To regenerate after changing the builder code:

```sh
cargo test --test speller_integration rebuild_fixtures -- --ignored --nocapture
```

## Manual testing

The fixtures are THFST format. The CLI's `--lexicon-path`/`--mutator-path`
flags expect HFST format, so use `cargo test` to exercise the fixtures:

```sh
# Run all integration tests
cargo test --test speller_integration

# Run a single test with output
cargo test --test speller_integration test_suggest_misspelled -- --nocapture
```
