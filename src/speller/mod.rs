//! Speller model for spell-checking and corrections.
//!
//! The spell-checker uses a two-transducer architecture:
//!
//! - **Lexicon**: The acceptor transducer containing valid words in the language
//! - **Mutator** (Error Model): A transducer that models common spelling errors and their corrections
//!
//! During spell-checking, input is processed through both transducers in parallel to find
//! valid corrections with minimal edit distance.
use std::f32;
use std::sync::Arc;

use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use unic_emoji_char::is_emoji;
use unic_segment::Graphemes;
use unic_ucd_category::GeneralCategory;

use self::worker::SpellerWorker;
use crate::speller::suggestion::Suggestion;
use crate::tokenizer::case_handling::{CaseHandler, CaseMutation, upper_case, upper_first};
use crate::transducer::Transducer;
use crate::types::{SymbolNumber, Weight};

pub mod error;
pub mod suggestion;

mod subset;
mod worker;

/// Calculate Damerau-Levenshtein distance between pre-split grapheme slices.
///
/// Uses a flat reusable buffer to avoid per-call heap allocation.
/// The buffer is resized as needed and reused across calls.
fn grapheme_damerau_levenshtein(s1: &[&str], s2: &[&str], buf: &mut Vec<usize>) -> usize {
    let len1 = s1.len();
    let len2 = s2.len();

    if len1 == 0 {
        return len2;
    }
    if len2 == 0 {
        return len1;
    }

    let cols = len2 + 1;
    let needed = (len1 + 1) * cols;
    buf.clear();
    buf.resize(needed, 0);

    for i in 0..=len1 {
        buf[i * cols] = i;
    }
    for j in 0..=len2 {
        buf[j] = j;
    }

    for i in 1..=len1 {
        for j in 1..=len2 {
            let cost = if s1[i - 1] == s2[j - 1] { 0 } else { 1 };

            buf[i * cols + j] = std::cmp::min(
                std::cmp::min(
                    buf[(i - 1) * cols + j] + 1, // deletion
                    buf[i * cols + (j - 1)] + 1, // insertion
                ),
                buf[(i - 1) * cols + (j - 1)] + cost, // substitution
            );

            // Transposition
            if i > 1 && j > 1 && s1[i - 1] == s2[j - 2] && s1[i - 2] == s2[j - 1] {
                buf[i * cols + j] =
                    std::cmp::min(buf[i * cols + j], buf[(i - 2) * cols + (j - 2)] + cost);
            }
        }
    }

    buf[len1 * cols + len2]
}

/// Position-bucketed reweight penalties and the resulting additional weight.
#[derive(Clone, Copy, Debug)]
struct ReweightPenalties {
    start: f32,
    /// `-1.0` signals "no middle section" (rendered as "-" in verbose output);
    /// otherwise non-negative.
    mid: f32,
    end: f32,
    additional_weight: Weight,
}

/// Compute reweight penalties for a single suggestion against the case-folded
/// input. Mirrors the alignment/duplicate-grapheme logic previously inlined in
/// `suggest_case`'s `MergeAll` branch — extracted so `FirstResults` can use it
/// too (fixes #65 where mixed-case inputs silently skipped the reweight step).
fn compute_reweight_penalties(
    input_lower: &[&str],
    input_first: Option<&str>,
    sugg_value: &str,
    mutator_weight: Option<Weight>,
    reweight: Option<&ReweightingConfig>,
    dl_buf: &mut Vec<usize>,
) -> ReweightPenalties {
    // No penalties configured: still return a well-formed result, because case
    // handling runs either way.
    let Some(reweight) = reweight else {
        return ReweightPenalties {
            start: 0.0,
            mid: 0.0,
            end: 0.0,
            additional_weight: Weight(0.0),
        };
    };

    let sugg_lower_str = sugg_value.to_lowercase();
    let sugg_lower: Vec<&str> = Graphemes::new(&sugg_lower_str).collect();

    let is_short = input_lower.len() <= 2 && sugg_lower.len() <= 2;

    let (start_dist, mid_dist, end_dist): (usize, i32, usize) = if input_lower.is_empty()
        && sugg_lower.is_empty()
    {
        (0, 0, 0)
    } else if is_short {
        let start_d = if !input_lower.is_empty() && !sugg_lower.is_empty() {
            if input_lower[0] != sugg_lower[0] {
                1
            } else {
                0
            }
        } else {
            input_lower.len().max(sugg_lower.len()).min(1)
        };

        let end_d = if input_lower.len() > 1 && sugg_lower.len() > 1 {
            if input_lower[input_lower.len() - 1] != sugg_lower[sugg_lower.len() - 1] {
                1
            } else {
                0
            }
        } else {
            0
        };

        (start_d, -1, end_d)
    } else {
        const OFFSETS: [(usize, usize); 4] = [(0, 0), (0, 1), (1, 0), (1, 1)];

        let mut best_score = 0;
        let mut best_alignment = (0, 0, 0, 0, 0, 0, 0, 0, 0);

        for (start_in_off, start_su_off) in &OFFSETS {
            if *start_in_off >= input_lower.len() || *start_su_off >= sugg_lower.len() {
                continue;
            }

            for (end_in_off, end_su_off) in &OFFSETS {
                if *end_in_off >= input_lower.len() || *end_su_off >= sugg_lower.len() {
                    continue;
                }

                let inp = &input_lower[*start_in_off..];
                let sug = &sugg_lower[*start_su_off..];

                let prefix_len = inp
                    .iter()
                    .zip(sug.iter())
                    .take_while(|(a, b)| a == b)
                    .count();

                let inp_len = input_lower.len() - start_in_off - end_in_off;
                let sug_len = sugg_lower.len() - start_su_off - end_su_off;

                if inp_len == 0 || sug_len == 0 {
                    continue;
                }

                let inp_for_suffix = &input_lower[*start_in_off..input_lower.len() - end_in_off];
                let sug_for_suffix = &sugg_lower[*start_su_off..sugg_lower.len() - end_su_off];

                let suffix_len = inp_for_suffix
                    .iter()
                    .rev()
                    .zip(sug_for_suffix.iter().rev())
                    .take_while(|(a, b)| a == b)
                    .count();

                let score = prefix_len + suffix_len;

                if score > best_score {
                    let start_d = if *start_in_off == 0 && *start_su_off == 0 {
                        0
                    } else {
                        grapheme_damerau_levenshtein(
                            &input_lower[0..*start_in_off],
                            &sugg_lower[0..*start_su_off],
                            dl_buf,
                        )
                    };

                    let end_d = if *end_in_off == 0 && *end_su_off == 0 {
                        0
                    } else {
                        grapheme_damerau_levenshtein(
                            &input_lower[input_lower.len().saturating_sub(*end_in_off)..],
                            &sugg_lower[sugg_lower.len().saturating_sub(*end_su_off)..],
                            dl_buf,
                        )
                    };

                    best_score = score;
                    best_alignment = (
                        *start_in_off,
                        *start_su_off,
                        *end_in_off,
                        *end_su_off,
                        prefix_len,
                        suffix_len,
                        start_d,
                        end_d,
                        score,
                    );
                }
            }
        }

        let (
            start_in_off,
            start_su_off,
            end_in_off,
            end_su_off,
            prefix_len,
            suffix_len,
            start_d,
            end_d,
            _,
        ) = best_alignment;

        let min_total_len = (input_lower.len() - start_in_off - end_in_off)
            .min(sugg_lower.len() - start_su_off - end_su_off);

        let actual_suffix = if prefix_len + suffix_len > min_total_len {
            min_total_len.saturating_sub(prefix_len)
        } else {
            suffix_len
        };

        let inp_start_pos = start_in_off + prefix_len;
        let sug_start_pos = start_su_off + prefix_len;
        let inp_end_pos = input_lower.len() - end_in_off - actual_suffix;
        let sug_end_pos = sugg_lower.len() - end_su_off - actual_suffix;

        let inp_remaining = inp_end_pos.saturating_sub(inp_start_pos);
        let sug_remaining = sug_end_pos.saturating_sub(sug_start_pos);

        let (mid_d, adjusted_end_d) = if inp_remaining == 0 && sug_remaining == 0 {
            (0, end_d)
        } else if (inp_remaining <= 1 && sug_remaining <= 1) && actual_suffix == 0 {
            let end_change = inp_remaining.max(sug_remaining) > 0;
            (0, if end_change { 1 } else { end_d })
        } else if inp_start_pos < inp_end_pos || sug_start_pos < sug_end_pos {
            let d = grapheme_damerau_levenshtein(
                &input_lower[inp_start_pos.min(inp_end_pos)..inp_end_pos.max(inp_start_pos)],
                &sugg_lower[sug_start_pos.min(sug_end_pos)..sug_end_pos.max(sug_start_pos)],
                dl_buf,
            ) as i32;
            (d, end_d)
        } else {
            (0, end_d)
        };

        if mid_d > 0 {
            if prefix_len == 0 && actual_suffix > 0 {
                let start_changes = 1;
                let remaining_mid = (mid_d as usize).saturating_sub(start_changes);
                (
                    start_d + start_changes,
                    remaining_mid as i32,
                    adjusted_end_d,
                )
            } else if actual_suffix == 0 && prefix_len > 0 {
                let end_changes = 1;
                let remaining_mid = (mid_d as usize).saturating_sub(end_changes);
                (start_d, remaining_mid as i32, adjusted_end_d + end_changes)
            } else {
                (start_d, mid_d, adjusted_end_d)
            }
        } else {
            (start_d, mid_d, adjusted_end_d)
        }
    };

    // Special case: when input or suggestion has duplicate graphemes at start/end that match
    let (start_dist, mid_dist, end_dist) =
        if !is_short && !input_lower.is_empty() && !sugg_lower.is_empty() {
            let adjusted_start = if start_dist > 0 && input_lower[0] == sugg_lower[0] {
                let sugg_has_dup = sugg_lower.len() > 1 && sugg_lower[0] == sugg_lower[1];
                let input_has_dup = input_lower.len() > 1 && input_lower[0] == input_lower[1];
                if sugg_has_dup || input_has_dup {
                    0
                } else {
                    start_dist
                }
            } else {
                start_dist
            };

            let adjusted_end = if end_dist > 0
                && !input_lower.is_empty()
                && !sugg_lower.is_empty()
                && input_lower[input_lower.len() - 1] == sugg_lower[sugg_lower.len() - 1]
            {
                let sugg_has_dup = sugg_lower.len() > 1
                    && sugg_lower[sugg_lower.len() - 1] == sugg_lower[sugg_lower.len() - 2];
                let input_has_dup = input_lower.len() > 1
                    && input_lower[input_lower.len() - 1] == input_lower[input_lower.len() - 2];
                if sugg_has_dup || input_has_dup {
                    0
                } else {
                    end_dist
                }
            } else {
                end_dist
            };

            let added_to_mid = (start_dist - adjusted_start) + (end_dist - adjusted_end);
            let adjusted_mid = if mid_dist < 0 {
                added_to_mid as i32
            } else {
                mid_dist + added_to_mid as i32
            };

            (adjusted_start, adjusted_mid, adjusted_end)
        } else {
            (start_dist, mid_dist, end_dist)
        };

    // A correction whose only difference at the first letter is its case (e.g.
    // typed lowercase, suggested with an upper-case initial) folds away to a
    // zero start distance above. Treat it like a start-position edit so it
    // carries the start penalty (#65) — but only when the case-folded
    // comparison found no real start edit, to avoid double counting.
    let first_letter_case_change = match (input_first, Graphemes::new(sugg_value).next()) {
        (Some(a), Some(b)) => a != b && a.to_lowercase() == b.to_lowercase(),
        _ => false,
    };
    let start_dist = if first_letter_case_change && start_dist == 0 {
        1
    } else {
        start_dist
    };

    let penalty_start = if start_dist > 0 {
        reweight.start_penalty
    } else {
        0.0
    };
    let penalty_mid = if mid_dist < 0 {
        -1.0
    } else {
        reweight.mid_penalty * mid_dist as f32
    };
    let penalty_end = if end_dist > 0 {
        reweight.end_penalty
    } else {
        0.0
    };

    let raw = if sugg_value.chars().all(is_emoji) {
        0.0
    } else {
        penalty_start + penalty_end + penalty_mid.max(0.0)
    };

    // These penalties describe where a *typo* fell, and the distances above are
    // measured between two strings. When the error model holds an authored
    // whole-word correction — the `words.default.txt` format,
    // `misspelling:correct<TAB>weight` — those distances describe nothing: the
    // model charged (near) nothing for a substitution the strings say is twenty
    // edits, because it is one lexical entry and not twenty typos. Penalising
    // it by the distance buries an entry its author declared certain.
    //
    // So: if the model charged less per apparent edit than the cheapest
    // positional adjustment costs, the apparent edits are not edits, and there
    // is no position to adjust for. Ordinary typos, which any error model
    // charges more for than this, are untouched.
    let apparent_edits = start_dist + mid_dist.max(0) as usize + end_dist;
    let is_authored = match mutator_weight {
        Some(Weight(charged)) if apparent_edits > 0 => {
            charged / (apparent_edits as f32) < reweight.mid_penalty
        }
        _ => false,
    };

    if is_authored {
        return ReweightPenalties {
            start: 0.0,
            mid: 0.0,
            end: 0.0,
            additional_weight: Weight(0.0),
        };
    }

    ReweightPenalties {
        start: penalty_start,
        mid: penalty_mid,
        end: penalty_end,
        additional_weight: Weight(raw),
    }
}

/// Everything the search worker needs to price a candidate the way
/// `suggest_case` will price it afterwards: same case mutation, same penalty
/// function. The worker keys its n-best cutoff on these post-reweight weights;
/// keyed on raw path weights, the cutoff prunes candidates that reweighting
/// would have promoted into the n best (a candidate with final rank 12 could
/// be absent from an n-best=100 run because ~100 raw-cheaper candidates each
/// absorbed +10..+25 in penalties only after the search had already cut it).
#[derive(Clone)]
pub(crate) struct ReweightContext {
    input_lower: Vec<SmolStr>,
    input_first: Option<SmolStr>,
    mutation: CaseMutation,
    reweight: Option<ReweightingConfig>,
}

impl ReweightContext {
    fn new(
        original_input: &str,
        mutation: CaseMutation,
        reweight: Option<&ReweightingConfig>,
    ) -> Self {
        let lower = original_input.to_lowercase();
        ReweightContext {
            input_lower: Graphemes::new(&lower).map(SmolStr::from).collect(),
            input_first: Graphemes::new(original_input).next().map(SmolStr::from),
            mutation,
            reweight: reweight.cloned(),
        }
    }

    /// The reweight surcharge `suggest_case` will add to this candidate.
    pub(crate) fn additional_weight_for(
        &self,
        value: &str,
        mutator_weight: Weight,
        dl_buf: &mut Vec<usize>,
    ) -> Weight {
        let mutated;
        let value = match self.mutation {
            CaseMutation::FirstCaps => {
                mutated = upper_first(value);
                mutated.as_str()
            }
            CaseMutation::AllCaps => {
                mutated = upper_case(value);
                mutated.as_str()
            }
            CaseMutation::None => value,
        };
        let input_lower: Vec<&str> = self.input_lower.iter().map(|s| s.as_str()).collect();
        compute_reweight_penalties(
            &input_lower,
            self.input_first.as_deref(),
            value,
            Some(mutator_weight),
            self.reweight.as_ref(),
            dl_buf,
        )
        .additional_weight
    }
}

/// Apply case mutation and reweight penalties to each suggestion in-place.
///
/// Used by the `CaseMode::FirstResults` path, which returns suggestions
/// directly rather than folding them into a dedup map. Before this helper
/// existed the path skipped reweight entirely, producing zeroed reweight
/// values and unpenalised totals for hyphen/colon-containing inputs (#65).
fn apply_first_results_reweight(
    suggestions: &mut [Suggestion],
    mutation: crate::tokenizer::case_handling::CaseMutation,
    input_lower: &[&str],
    input_first: Option<&str>,
    reweight: Option<&ReweightingConfig>,
    dl_buf: &mut Vec<usize>,
) {
    use crate::tokenizer::case_handling::{CaseMutation, upper_case, upper_first};

    for sugg in suggestions.iter_mut() {
        match mutation {
            CaseMutation::FirstCaps => sugg.value = upper_first(sugg.value()),
            CaseMutation::AllCaps => sugg.value = upper_case(sugg.value()),
            CaseMutation::None => {}
        }

        let penalties = compute_reweight_penalties(
            input_lower,
            input_first,
            sugg.value(),
            sugg.weight_details.as_ref().map(|d| d.mutator_weight),
            reweight,
            dl_buf,
        );
        sugg.weight = sugg.weight + penalties.additional_weight;

        if let Some(ref mut details) = sugg.weight_details {
            details.reweight_start = penalties.start;
            details.reweight_mid = penalties.mid;
            details.reweight_end = penalties.end;
        }
    }
}

/// Re-apply `max_weight` and `beam` to reweighted suggestions.
///
/// Both limits are enforced during the search too, but on pre-reweight weights:
/// the in-search beam tracks a running `best_weight` that can be far above the
/// final best, and neither limit has seen the reweight penalties yet. Without
/// this pass a penalty can push a returned suggestion past `max_weight`, which
/// is the one number a caller can rely on to mean "no worse than this".
///
/// Matches FFI behaviour: beam is only honoured when strictly greater than
/// `Weight::ZERO`.
///
/// Suggestions that are a case-only variant of the input (their lower-cased
/// value equals `input_lower`) are never dropped: the case reweight penalty can
/// push the correct recapitalisation past a limit, and dropping it would lose
/// the right answer (#65).
fn apply_weight_limits(out: &mut Vec<Suggestion>, config: &SpellerConfig, input_lower: &str) {
    let beam_threshold = config
        .beam
        .filter(|beam| *beam > Weight::ZERO)
        .zip(out.first().map(Suggestion::weight))
        .map(|(beam, best)| best + beam);

    if beam_threshold.is_none() && config.max_weight.is_none() {
        return;
    }

    out.retain(|s| {
        let within = beam_threshold.is_none_or(|threshold| s.weight() <= threshold)
            && config.max_weight.is_none_or(|max| s.weight() <= max);

        within || s.value().to_lowercase() == input_lower
    });
}

/// Temporary struct to store weight details during suggestion generation
#[derive(Clone, Debug)]
struct SuggestionData {
    lexicon_weight: Weight,
    mutator_weight: Weight,
    reweight_start: f32,
    reweight_mid: f32,
    reweight_end: f32,
}

/// Controls whether morphological tags are preserved in FST output.
///
/// When traversing an FST, epsilon transitions can either preserve their symbols
/// (keeping morphological tags like "+V", "+Noun", etc.) or convert them to true
/// epsilons (stripping the tags from the output).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum OutputMode {
    /// Strip morphological tags from output.
    ///
    /// Used for spelling correction where you want clean word forms without tags.
    /// Example: "run" instead of "run+V+PresPartc"
    WithoutTags,

    /// Keep morphological tags in output.
    ///
    /// Used for morphological analysis where you want to see the linguistic structure.
    /// Example: "run+V+PresPartc" instead of "run"
    WithTags,
}

/// configurable extra penalties for edit distance
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ReweightingConfig {
    #[serde(default = "default_start_penalty")]
    pub start_penalty: f32,
    #[serde(default = "default_end_penalty")]
    pub end_penalty: f32,
    #[serde(default = "default_mid_penalty")]
    pub mid_penalty: f32,
}

impl Default for ReweightingConfig {
    fn default() -> Self {
        Self::default_const()
    }
}

impl ReweightingConfig {
    pub const fn default_const() -> Self {
        Self {
            start_penalty: 10.0,
            end_penalty: 10.0,
            mid_penalty: 5.0,
        }
    }
}

const fn default_start_penalty() -> f32 {
    10.0
}

const fn default_end_penalty() -> f32 {
    10.0
}

const fn default_mid_penalty() -> f32 {
    5.0
}

/// finetuning configuration of the spelling correction algorithms
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SpellerConfig {
    /// upper limit for suggestions given
    #[serde(default = "default_n_best")]
    pub n_best: Option<usize>,
    /// upper limit for weight of any suggestion
    #[serde(default = "default_max_weight")]
    pub max_weight: Option<Weight>,
    /// weight distance between best suggestion and worst
    #[serde(default = "default_beam")]
    pub beam: Option<Weight>,
    /// extra penalties for different edit distance type errors
    #[serde(default = "default_reweight")]
    pub reweight: Option<ReweightingConfig>,
    /// some parallel stuff?
    #[serde(default = "default_node_pool_size")]
    pub node_pool_size: usize,
    /// whether we try to recase mispelt word before other suggestions
    #[serde(default = "default_recase")]
    pub recase: bool,
    /// used when suggesting unfinished word parts
    #[serde(default)]
    pub completion_marker: Option<String>,
    /// whether the suggestion search orders itself by the A* heuristic
    ///
    /// The heuristic is admissible, so switching it off changes how long the
    /// search runs, not what it finds. It is here as an escape hatch, and so a
    /// test can assert the two orders agree; leave it on.
    #[serde(default = "default_astar_lookahead")]
    pub astar_lookahead: bool,
    /// whether the suggestion search walks states rather than paths
    ///
    /// With this off the search re-walks every path that leads to a given
    /// search state, which for a determinised ("expanded") error model is
    /// roughly one path per state, and for a compact one — a plain union of
    /// components, 19x smaller on disk — is combinatorially many. Two paths
    /// that arrive at the same state having spelled the same output are
    /// interchangeable, so keeping only the cheapest changes what the search
    /// costs, not what it finds. It is here as an escape hatch; leave it on.
    #[serde(default = "default_search_dedup")]
    pub search_dedup: bool,
    /// whether the suggestion search determinises the error model as it goes
    ///
    /// An error model built as a plain union of components offers several
    /// routes carrying one and the same `input:output` label sequence, so the
    /// search stands in several model states at once for one partial
    /// correction and pays the whole product walk once per state. With this on
    /// it walks interned *subsets* of model states instead — the standard
    /// weighted subset construction, built lazily and memoised — which merges
    /// those routes back together. The merged arc weight is the minimum over
    /// the routes it stands for, so this changes what the search costs, not
    /// what it finds. A model that was determinised before it shipped produces
    /// singleton subsets and is unaffected. It is here as an escape hatch;
    /// leave it on.
    #[serde(default = "default_mutator_subsets")]
    pub mutator_subsets: bool,
    /// whether to output detailed weight information (not serialized)
    #[serde(skip)]
    pub verbose: bool,
}

impl SpellerConfig {
    /// create a default configuration with following values:
    /// * n_best = 10
    /// * max_weight = 10000
    /// * beam = None
    /// * reweight = default (c.f. ReweightingConfig::default())
    /// * node_pool_size = 128
    /// * recase = true
    /// * astar_lookahead = false
    /// * verbose = false
    pub const fn default() -> SpellerConfig {
        SpellerConfig {
            n_best: default_n_best(),
            max_weight: default_max_weight(),
            beam: default_beam(),
            reweight: default_reweight(),
            node_pool_size: default_node_pool_size(),
            recase: default_recase(),
            completion_marker: None,
            astar_lookahead: default_astar_lookahead(),
            search_dedup: default_search_dedup(),
            mutator_subsets: default_mutator_subsets(),
            verbose: false,
        }
    }
}

const fn default_n_best() -> Option<usize> {
    Some(10)
}

const fn default_max_weight() -> Option<Weight> {
    Some(Weight(10000.0))
}

const fn default_beam() -> Option<Weight> {
    None
}

const fn default_reweight() -> Option<ReweightingConfig> {
    Some(ReweightingConfig::default_const())
}

const fn default_node_pool_size() -> usize {
    128
}

const fn default_recase() -> bool {
    true
}

// Off by default: the giella spellers are weight-pushed to the initial state,
// which makes distance-to-final identically zero — the precompute (~250 ms and
// a large transient allocation per transducer) buys nothing there, and
// divvunspell runs on memory-constrained mobile devices. Enable for transducers
// that carry weight toward their finals.
const fn default_astar_lookahead() -> bool {
    false
}

// On: it costs one hash of the search state per node and saves re-walking
// every path into that state. Even a determinised error model repeats a third
// to three quarters of its pops without it.
const fn default_search_dedup() -> bool {
    true
}

// On: a determinised error model produces singleton subsets and pays only a
// memo lookup per node, and a compact one — the same relation left as a union
// of components, 19x smaller on disk — stops paying for its own
// non-determinism.
const fn default_mutator_subsets() -> bool {
    true
}
/// FST-based spell checker and morphological analyzer.
///
/// This trait provides methods for spell checking and morphological analysis
/// using finite-state transducers. The same FST traversal logic is used for both
/// operations - the difference is controlled by the `OutputMode`:
///
/// - `OutputMode::WithoutTags` strips morphological tags (for spelling correction)
/// - `OutputMode::WithTags` preserves morphological tags (for morphological analysis)
pub trait Speller {
    /// Check if the word is correctly spelled
    #[must_use]
    fn is_correct(self: Arc<Self>, word: &str) -> bool;

    /// Check if word is correctly spelled with config (handles recasing, etc.)
    #[must_use]
    fn is_correct_with_config(self: Arc<Self>, word: &str, config: &SpellerConfig) -> bool;

    /// Generate suggestions or analyses for a word.
    #[must_use]
    fn suggest(self: Arc<Self>, word: &str) -> Vec<Suggestion>;

    /// Generate suggestions with config options (recasing, reweighting, etc.)
    #[must_use]
    fn suggest_with_config(self: Arc<Self>, word: &str, config: &SpellerConfig) -> Vec<Suggestion>;

    /// Analyze the input word form.
    ///
    /// Performs lexicon-only traversal (no error model) to get morphological analyses
    /// of exactly what was typed. Does not generate spelling corrections.
    #[must_use]
    fn analyze_input(self: Arc<Self>, word: &str) -> Vec<Suggestion>;

    /// Analyze input word form with config options.
    #[must_use]
    fn analyze_input_with_config(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
    ) -> Vec<Suggestion>;

    /// Get lexicon weight for a word form (lexicon-only traversal).
    ///
    /// Returns the weight of the best analysis using only the lexicon FST.
    /// If the word is not in the lexicon, returns Weight(0.0).
    /// Useful for separating lexicon vs mutator contributions to total weight.
    #[must_use]
    fn get_lexicon_weight(self: Arc<Self>, word: &str) -> Weight {
        self.get_lexicon_weight_with_config(word, &SpellerConfig::default())
    }

    /// Get lexicon weight with custom config.
    ///
    /// Default implementation returns Weight(0.0) to preserve API compatibility.
    /// Override this method if you want to provide lexicon weight analysis.
    #[must_use]
    fn get_lexicon_weight_with_config(
        self: Arc<Self>,
        _word: &str,
        _config: &SpellerConfig,
    ) -> Weight {
        Weight(0.0)
    }

    /// Analyze the suggested word forms.
    ///
    /// Generates spelling corrections using the error model, then returns them with
    /// morphological tags preserved (equivalent to `suggest(word, OutputMode::WithTags)`).
    #[must_use]
    fn analyze_output(self: Arc<Self>, word: &str) -> Vec<Suggestion>;

    /// Analyze suggested word forms with config options.
    #[must_use]
    fn analyze_output_with_config(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
    ) -> Vec<Suggestion>;

    /// Create suggestion list and use their analyses for filtering.
    ///
    /// Gets spelling corrections, analyzes each one, and filters based on
    /// morphological analysis results.
    #[must_use]
    fn analyze_suggest(self: Arc<Self>, word: &str) -> Vec<Suggestion>;

    /// Create suggestion list and use analyses for filtering with config.
    #[must_use]
    fn analyze_suggest_with_config(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
    ) -> Vec<Suggestion>;

    /// Forward generation: produce every inflected surface form whose
    /// **output-tape prefix** equals `lemma`.
    ///
    /// The inverse direction of [`analyze_input`](Self::analyze_input):
    /// where `analyze_input("dieđaheami")` returns the analysis
    /// `"dieđahit+V+Action+Acc+Sg"`, `generate("dieđahit")` returns
    /// `[("dieđahit","+V+Inf"), ("dieđaheami","+V+Action+Acc+Sg"), …]`.
    ///
    /// Default implementation returns an empty Vec to preserve API
    /// compatibility for custom `Speller` impls. `HfstSpeller`
    /// overrides with the real walker.
    #[must_use]
    fn generate(self: Arc<Self>, _lemma: &str) -> Vec<crate::generator::GenerationResult> {
        Vec::new()
    }

    /// Forward generation with config options.
    #[must_use]
    fn generate_with_config(
        self: Arc<Self>,
        _lemma: &str,
        _config: &crate::generator::GeneratorConfig,
    ) -> Vec<crate::generator::GenerationResult> {
        Vec::new()
    }
}

impl<T, U> Speller for HfstSpeller<T, U>
where
    T: Transducer + Send,
    U: Transducer + Send,
{
    #[allow(clippy::wrong_self_convention)]
    fn is_correct_with_config(self: Arc<Self>, word: &str, config: &SpellerConfig) -> bool {
        use crate::tokenizer::case_handling::*;

        if word.len() == 0 {
            return true;
        }

        // Check if there are zero letters in the word according to
        // Unicode letter category
        if word.chars().all(|c| !GeneralCategory::of(c).is_letter()) {
            return true;
        }

        let words = if config.recase {
            let variants = word_variants(word);
            variants.words
        } else {
            vec![]
        };
        tracing::debug!(
            "is_correct_with_config: ‘{}’ ~ {:?}?; config: {:?}",
            word,
            words,
            config
        );
        for word in std::iter::once(word.into()).chain(words.into_iter()) {
            let worker = SpellerWorker::new_lexicon_input(
                self.clone(),
                self.to_input_vec_lexicon(&word),
                config,
                OutputMode::WithoutTags,
            );

            if worker.is_correct() {
                return true;
            }
        }

        false
    }

    #[inline]
    fn is_correct(self: Arc<Self>, word: &str) -> bool {
        self.is_correct_with_config(word, &SpellerConfig::default())
    }

    #[inline]
    fn suggest(self: Arc<Self>, word: &str) -> Vec<Suggestion> {
        self.suggest_with_config(word, &SpellerConfig::default())
    }

    fn suggest_with_config(self: Arc<Self>, word: &str, config: &SpellerConfig) -> Vec<Suggestion> {
        self._suggest_with_config(word, config, OutputMode::WithoutTags)
    }

    fn analyze_input_with_config(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
    ) -> Vec<Suggestion> {
        if word.is_empty() {
            return vec![];
        }

        let worker = SpellerWorker::new_lexicon_input(
            self.clone(),
            self.to_input_vec_lexicon(word),
            config,
            OutputMode::WithTags,
        );

        tracing::trace!("Beginning analyze_input with config");
        worker.analyze()
    }

    #[inline]
    fn analyze_input(self: Arc<Self>, word: &str) -> Vec<Suggestion> {
        self.analyze_input_with_config(word, &SpellerConfig::default())
    }

    fn get_lexicon_weight_with_config(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
    ) -> Weight {
        if word.is_empty() {
            return Weight(0.0);
        }

        // Analyze output form using lexicon-only traversal (without error model)
        // This gives us the weight from the lexicon/acceptor alone
        let non_verbose_config = SpellerConfig {
            verbose: false,
            ..config.clone()
        };
        let worker = SpellerWorker::new_lexicon_input(
            self.clone(),
            self.to_input_vec_lexicon(word),
            &non_verbose_config,
            OutputMode::WithoutTags,
        );

        let analyses = worker.analyze();
        analyses.first().map(|s| s.weight()).unwrap_or(Weight(0.0))
    }

    fn analyze_output_with_config(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
    ) -> Vec<Suggestion> {
        self._suggest_with_config(word, config, OutputMode::WithTags)
    }

    #[inline]
    fn analyze_output(self: Arc<Self>, word: &str) -> Vec<Suggestion> {
        self.analyze_output_with_config(word, &SpellerConfig::default())
    }

    fn analyze_suggest_with_config(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
    ) -> Vec<Suggestion> {
        let mut suggs = self.clone().suggest_with_config(word, config);
        suggs.retain(|sugg| {
            tracing::trace!("suggestion {}", sugg.value);
            let analyses = self
                .clone()
                .analyze_input_with_config(sugg.value.as_str(), config);
            let mut all_filtered = true;
            for analysis in analyses {
                tracing::trace!("-> {}", analysis.value);
                if !analysis.value.contains("+Spell/NoSugg") {
                    all_filtered = false;
                } else {
                    tracing::trace!("filtering=?");
                }
            }
            !all_filtered
        });
        suggs
    }

    #[inline]
    fn analyze_suggest(self: Arc<Self>, word: &str) -> Vec<Suggestion> {
        self.analyze_suggest_with_config(word, &SpellerConfig::default())
    }

    fn generate_with_config(
        self: Arc<Self>,
        lemma: &str,
        config: &crate::generator::GeneratorConfig,
    ) -> Vec<crate::generator::GenerationResult> {
        if lemma.is_empty() {
            return Vec::new();
        }
        crate::generator::generate_from_lexicon(self.lexicon(), lemma, config)
    }

    #[inline]
    fn generate(self: Arc<Self>, lemma: &str) -> Vec<crate::generator::GenerationResult> {
        self.generate_with_config(lemma, &crate::generator::GeneratorConfig::default())
    }
}

/// The symbols an `@_UNKNOWN_@` on the mutator's output tape can stand for.
///
/// The marker denotes "some symbol outside the mutator's alphabet", so the
/// candidates are exactly the lexicon's symbols that the mutator's alphabet
/// does not name — the same set HFST's harmonisation expands an unknown
/// against when it composes two transducers, computed here once because the
/// speller composes them on the fly instead.
///
/// `alphabet_translator` maps every mutator symbol to its lexicon counterpart,
/// so its image is precisely the part of the lexicon alphabet the mutator can
/// write literally. Epsilon, flag diacritics and the lexicon's own wildcard
/// markers are not symbols a correction can contain and are excluded with it.
fn build_unknown_output_domain<U>(
    lexicon: &U,
    alphabet_translator: &[SymbolNumber],
) -> Vec<SymbolNumber>
where
    U: Transducer,
{
    let alphabet = lexicon.alphabet();
    let symbol_count = alphabet.key_table().len();

    let mut named_by_mutator = vec![false; symbol_count];
    for sym in alphabet_translator {
        if let Some(slot) = named_by_mutator.get_mut(sym.0 as usize) {
            *slot = true;
        }
    }

    (1..symbol_count)
        .map(|i| SymbolNumber(i as u16))
        .filter(|sym| !named_by_mutator[sym.0 as usize])
        .filter(|sym| !alphabet.is_flag(*sym))
        .filter(|sym| Some(*sym) != alphabet.identity() && Some(*sym) != alphabet.unknown())
        .collect()
}

/// A determinisation warmed up past this many subsets is dropped rather than
/// handed back to the pool.
///
/// The reachable determinisation of a real error model settles in the low
/// thousands, so only a transducer whose subsets keep diverging gets here — and
/// there the memo is not earning the memory it holds.
const SUBSET_POOL_LIMIT: usize = 1 << 16;

#[derive(Debug)]
pub struct HfstSpeller<T, U>
where
    T: Transducer,
    U: Transducer,
{
    mutator: T,
    lexicon: U,
    alphabet_translator: Vec<SymbolNumber>,
    unknown_output_domain: Vec<SymbolNumber>,
    /// Error-model determinisations warmed up by earlier searches.
    ///
    /// Nothing one holds depends on the word that built it, so determinising is
    /// work done once for the speller rather than once per word — and the
    /// reachable determinisation is a few thousand subsets against however many
    /// words a run checks. A search takes one out and puts it back, which keeps
    /// every lookup inside the search lock-free: the lock is touched twice per
    /// word, not once per node. Passing them round a pool rather than sharing
    /// one is what buys that, at the price of warming up once per thread.
    subset_pool: parking_lot::Mutex<Vec<subset::MutatorSubsets>>,
}

impl<T, U> HfstSpeller<T, U>
where
    T: Transducer,
    U: Transducer,
{
    /// create new speller from two automata
    pub fn new(mutator: T, mut lexicon: U) -> Arc<HfstSpeller<T, U>> {
        let alphabet_translator = lexicon.alphabet_mut().create_translator_from(&mutator);
        let unknown_output_domain = build_unknown_output_domain(&lexicon, &alphabet_translator);

        Arc::new(HfstSpeller {
            mutator,
            lexicon,
            alphabet_translator,
            unknown_output_domain,
            subset_pool: parking_lot::Mutex::new(Vec::new()),
        })
    }

    /// Borrow a determinisation of the error model, warmed up by an earlier
    /// search where one is going spare.
    ///
    /// `None` when the model is one the construction cannot handle, which the
    /// caller answers by walking it as an NFA.
    pub(crate) fn take_subsets(&self, track_distance: bool) -> Option<subset::MutatorSubsets> {
        let taken = {
            let mut pool = self.subset_pool.lock();
            pool.iter()
                .position(|s| s.tracks_distance() == track_distance)
                .map(|at| pool.swap_remove(at))
        };

        match taken {
            Some(subsets) => Some(subsets),
            None => subset::MutatorSubsets::new(&self.mutator, track_distance),
        }
    }

    /// Hand a determinisation back for the next search to reuse.
    pub(crate) fn give_subsets(&self, subsets: subset::MutatorSubsets) {
        if subsets.len() <= SUBSET_POOL_LIMIT {
            self.subset_pool.lock().push(subsets);
        }
    }

    fn _suggest_with_config(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
        mode: OutputMode,
    ) -> Vec<Suggestion> {
        use crate::tokenizer::case_handling::*;

        if word.len() == 0 {
            return vec![];
        }

        // Case handling is not conditional on reweighting: without it, an
        // all-caps input used to produce no suggestions at all.
        self.suggest_case(word_variants(word), config, config.reweight.as_ref(), mode)
    }

    /// get the error model automaton
    pub fn mutator(&self) -> &T {
        &self.mutator
    }

    /// get the language model automaton
    pub fn lexicon(&self) -> &U {
        &self.lexicon
    }

    fn alphabet_translator(&self) -> &Vec<SymbolNumber> {
        &self.alphabet_translator
    }

    /// The symbols an `@_UNKNOWN_@` on the mutator's output tape stands for.
    fn unknown_output_domain(&self) -> &[SymbolNumber] {
        &self.unknown_output_domain
    }

    fn to_input_vec(&self, word: &str) -> Vec<SymbolNumber> {
        let alphabet = self.mutator().alphabet();
        let string_to_symbol = alphabet.string_to_symbol();

        tracing::trace!("to_input_vec: {}", word);
        Graphemes::new(word)
            .map(|ch| {
                string_to_symbol
                    .get(ch)
                    .copied()
                    .unwrap_or_else(|| alphabet.unknown().unwrap_or(SymbolNumber::ZERO))
            })
            .collect()
    }

    /// Convert input word to a symbol vector keyed by the **lexicon** alphabet.
    ///
    /// Used for lexicon-only operations (`is_correct`, `analyze_input`,
    /// `get_lexicon_weight`). Using the mutator alphabet here would collapse any
    /// character that is in the lexicon but not in the error model to UNKNOWN,
    /// causing valid words to be rejected when the two alphabets diverge.
    fn to_input_vec_lexicon(&self, word: &str) -> Vec<SymbolNumber> {
        let alphabet = self.lexicon().alphabet();
        let string_to_symbol = alphabet.string_to_symbol();

        tracing::trace!("to_input_vec_lexicon: {}", word);
        Graphemes::new(word)
            .map(|ch| {
                string_to_symbol
                    .get(ch)
                    .copied()
                    .unwrap_or_else(|| alphabet.unknown().unwrap_or(SymbolNumber::ZERO))
            })
            .collect()
    }

    fn suggest_case(
        self: Arc<Self>,
        case: CaseHandler,
        config: &SpellerConfig,
        reweight: Option<&ReweightingConfig>,
        output_mode: OutputMode,
    ) -> Vec<Suggestion> {
        use crate::tokenizer::case_handling::*;

        tracing::trace!("suggesting cases...");
        let CaseHandler {
            original_input,
            mutation,
            mode,
            words,
        } = case;
        // Total weight and the lexicon's share of it, keyed by output form. The
        // two travel together so that when a later case variant improves a
        // form's weight, the lexicon share follows that winning path instead of
        // being left behind at whichever path got there first — the share is
        // the tie-break key in `Suggestion::cmp`, and a stale one would break
        // ties on evidence from a path that lost.
        let mut best: HashMap<SmolStr, (Weight, Weight)> = HashMap::new();
        let mut suggestion_data: Option<HashMap<SmolStr, SuggestionData>> = if config.verbose {
            Some(HashMap::new())
        } else {
            None
        };

        let input_lower_str = original_input.to_lowercase();
        let input_lower: Vec<&str> = Graphemes::new(&input_lower_str).collect();
        let input_first: Option<&str> = Graphemes::new(original_input.as_str()).next();
        let mut dl_buf: Vec<usize> = Vec::new();
        let reweight_ctx = ReweightContext::new(&original_input, mutation, reweight);

        // `word_variants` echoes the input itself for lower-case words; searching
        // an identical variant twice can only reproduce the same suggestions.
        for word in
            std::iter::once(&original_input).chain(words.iter().filter(|w| **w != original_input))
        {
            tracing::trace!("suggesting for word {}", word);
            let worker = SpellerWorker::new_mutator_input(
                self.clone(),
                self.to_input_vec(&word),
                self.to_input_vec_lexicon(&word),
                config,
                output_mode,
            )
            .with_reweight_ctx(reweight_ctx.clone());
            let suggestions = worker.suggest();

            match mode {
                CaseMode::MergeAll => {
                    tracing::trace!("Case merge all");
                    for mut sugg in suggestions.into_iter() {
                        tracing::trace!("for {}", sugg.value);

                        // Apply case mutation first (for output display),
                        // then calculate penalties using case-insensitive comparison below
                        match mutation {
                            CaseMutation::FirstCaps => {
                                sugg.value = upper_first(sugg.value());
                            }
                            CaseMutation::AllCaps => {
                                sugg.value = upper_case(sugg.value());
                            }
                            _ => {}
                        }

                        let ReweightPenalties {
                            start: penalty_start,
                            mid: penalty_middle,
                            end: penalty_end,
                            additional_weight,
                        } = compute_reweight_penalties(
                            &input_lower,
                            input_first,
                            sugg.value(),
                            sugg.weight_details.as_ref().map(|d| d.mutator_weight),
                            reweight,
                            &mut dl_buf,
                        );

                        tracing::trace!(
                            "Penalty: +{} = {} + {} + {}",
                            additional_weight,
                            penalty_start,
                            penalty_middle,
                            penalty_end
                        );

                        let weight = sugg.weight + additional_weight;
                        let lexicon_weight = sugg.lexicon_weight;

                        best.entry(sugg.value.clone())
                            .and_modify(|entry| {
                                tracing::trace!(
                                    "=> Reweighting: {} {} = {} + {}",
                                    sugg.value,
                                    weight,
                                    sugg.weight,
                                    additional_weight
                                );
                                if entry.0 > weight {
                                    *entry = (weight, lexicon_weight);
                                    // Update suggestion data (only when verbose)
                                    if let Some(ref mut data) = suggestion_data {
                                        let (lex_w, mut_w) =
                                            if let Some(ref details) = sugg.weight_details {
                                                (details.lexicon_weight, details.mutator_weight)
                                            } else {
                                                (Weight(0.0), Weight(0.0))
                                            };
                                        data.insert(
                                            sugg.value.clone(),
                                            SuggestionData {
                                                lexicon_weight: lex_w,
                                                mutator_weight: mut_w,
                                                reweight_start: penalty_start,
                                                reweight_mid: penalty_middle,
                                                reweight_end: penalty_end,
                                            },
                                        );
                                    }
                                }
                            })
                            .or_insert_with(|| {
                                // Store suggestion data (only when verbose)
                                if let Some(ref mut data) = suggestion_data {
                                    let (lex_w, mut_w) =
                                        if let Some(ref details) = sugg.weight_details {
                                            (details.lexicon_weight, details.mutator_weight)
                                        } else {
                                            (Weight(0.0), Weight(0.0))
                                        };
                                    data.insert(
                                        sugg.value.clone(),
                                        SuggestionData {
                                            lexicon_weight: lex_w,
                                            mutator_weight: mut_w,
                                            reweight_start: penalty_start,
                                            reweight_mid: penalty_middle,
                                            reweight_end: penalty_end,
                                        },
                                    );
                                }
                                (weight, lexicon_weight)
                            });
                    }
                }
                CaseMode::FirstResults => {
                    if !suggestions.is_empty() {
                        let mut suggestions = suggestions;
                        apply_first_results_reweight(
                            &mut suggestions,
                            mutation,
                            &input_lower,
                            input_first,
                            reweight,
                            &mut dl_buf,
                        );
                        suggestions.sort();
                        if let Some(n_best) = config.n_best {
                            suggestions.truncate(n_best);
                        }
                        apply_weight_limits(&mut suggestions, config, &input_lower_str);
                        return suggestions;
                    }
                }
            }
        }

        // Fallback for mixed case: if FirstResults found nothing, try lowercase
        if mode == CaseMode::FirstResults {
            let lower = lower_case(&original_input);
            if lower.as_str() != original_input.as_str() {
                let worker = SpellerWorker::new_mutator_input(
                    self.clone(),
                    self.to_input_vec(&lower),
                    self.to_input_vec_lexicon(&lower),
                    config,
                    output_mode,
                )
                .with_reweight_ctx(reweight_ctx.clone());
                let mut suggestions = worker.suggest();
                if !suggestions.is_empty() {
                    apply_first_results_reweight(
                        &mut suggestions,
                        mutation,
                        &input_lower,
                        input_first,
                        reweight,
                        &mut dl_buf,
                    );
                    suggestions.sort();
                    if let Some(n_best) = config.n_best {
                        suggestions.truncate(n_best);
                    }
                    apply_weight_limits(&mut suggestions, config, &input_lower_str);
                    return suggestions;
                }
            }
        }

        if best.is_empty() {
            return vec![];
        }
        let mut out: Vec<Suggestion>;
        if config.verbose {
            // Verbose mode: include weight details
            if let Some(s) = &config.completion_marker {
                out = best
                    .into_iter()
                    .map(|(k, (weight, lexicon_weight))| {
                        let data = suggestion_data.as_ref().and_then(|map| map.get(&k));
                        Suggestion {
                            value: k.clone(),
                            weight,
                            completed: Some(!k.ends_with(s)),
                            weight_details: data.map(|d| suggestion::WeightDetails {
                                lexicon_weight: d.lexicon_weight,
                                mutator_weight: d.mutator_weight,
                                reweight_start: d.reweight_start,
                                reweight_mid: d.reweight_mid,
                                reweight_end: d.reweight_end,
                            }),
                            lexicon_weight,
                        }
                    })
                    .collect::<Vec<_>>();
            } else {
                out = best
                    .into_iter()
                    .map(|(k, (weight, lexicon_weight))| {
                        let data = suggestion_data.as_ref().and_then(|map| map.get(&k));
                        Suggestion {
                            value: k,
                            weight,
                            completed: None,
                            weight_details: data.map(|d| suggestion::WeightDetails {
                                lexicon_weight: d.lexicon_weight,
                                mutator_weight: d.mutator_weight,
                                reweight_start: d.reweight_start,
                                reweight_mid: d.reweight_mid,
                                reweight_end: d.reweight_end,
                            }),
                            lexicon_weight,
                        }
                    })
                    .collect::<Vec<_>>();
            }
        } else {
            // Normal mode: no weight details
            if let Some(s) = &config.completion_marker {
                out = best
                    .into_iter()
                    .map(|(k, (weight, lexicon_weight))| Suggestion {
                        value: k.clone(),
                        weight,
                        completed: Some(!k.ends_with(s)),
                        weight_details: None,
                        lexicon_weight,
                    })
                    .collect::<Vec<_>>();
            } else {
                out = best
                    .into_iter()
                    .map(|(k, (weight, lexicon_weight))| Suggestion {
                        value: k,
                        weight,
                        completed: None,
                        weight_details: None,
                        lexicon_weight,
                    })
                    .collect::<Vec<_>>();
            }
        }
        out.sort();
        if let Some(n_best) = config.n_best {
            out.truncate(n_best);
        }
        apply_weight_limits(&mut out, config, &input_lower_str);

        out
    }
}

#[cfg(test)]
mod reweight_tests {
    use super::*;

    fn graphemes(s: &str) -> Vec<&str> {
        Graphemes::new(s).collect()
    }

    // #65: a correction that only differs from the input by the case of the
    // first letter must carry the start penalty, not 0/0/0.
    #[test]
    fn first_letter_case_change_gets_start_penalty() {
        let reweight = ReweightingConfig::default_const();
        let input_lower = graphemes("girona");
        let mut dl = Vec::new();

        let p = compute_reweight_penalties(
            &input_lower,
            Some("g"),
            "Girona",
            None,
            Some(&reweight),
            &mut dl,
        );
        assert_eq!(p.start, reweight.start_penalty);
        assert_eq!(p.mid, 0.0);
        assert_eq!(p.end, 0.0);
        assert_eq!(p.additional_weight, Weight(reweight.start_penalty));
    }

    // An error model that charged next to nothing for what the strings call a
    // long substitution is describing one authored correction, not many typos:
    // there is no edit position to adjust for. This is what keeps
    // `words.default.txt` entries — and the `nuvviDspeller` version strings
    // built in the same format — at the weight their author gave them.
    #[test]
    fn authored_correction_is_not_reweighted() {
        let reweight = ReweightingConfig::default_const();
        let input_lower = graphemes("nuvvidspeller");
        let mut dl = Vec::new();

        let p = compute_reweight_penalties(
            &input_lower,
            Some("n"),
            "Divvun speller for Northern Sami",
            Some(Weight(1.0)),
            Some(&reweight),
            &mut dl,
        );

        assert_eq!(p.additional_weight, Weight(0.0));
        assert_eq!((p.start, p.mid, p.end), (0.0, 0.0, 0.0));
    }

    // An ordinary typo, which any error model charges a real edit for, keeps
    // its positional penalty.
    #[test]
    fn ordinary_edit_is_still_reweighted() {
        let reweight = ReweightingConfig::default_const();
        let input_lower = graphemes("kat");
        let mut dl = Vec::new();

        let p = compute_reweight_penalties(
            &input_lower,
            Some("k"),
            "cat",
            Some(Weight(10.0)),
            Some(&reweight),
            &mut dl,
        );

        assert_eq!(p.start, reweight.start_penalty);
        assert_eq!(p.additional_weight, Weight(reweight.start_penalty));
    }

    // Without a configured `ReweightingConfig` there are no penalties — but the
    // caller still gets a well-formed result, because case handling runs either
    // way.
    #[test]
    fn no_reweight_config_means_no_penalties() {
        let input_lower = graphemes("kat");
        let mut dl = Vec::new();

        let p = compute_reweight_penalties(&input_lower, Some("k"), "cat", None, None, &mut dl);

        assert_eq!(p.additional_weight, Weight(0.0));
    }

    // No case difference at the first letter => no extra penalty.
    #[test]
    fn identical_first_letter_case_no_penalty() {
        let reweight = ReweightingConfig::default_const();
        let input_lower = graphemes("girona");
        let mut dl = Vec::new();

        let p = compute_reweight_penalties(
            &input_lower,
            Some("G"),
            "Girona",
            None,
            Some(&reweight),
            &mut dl,
        );
        assert_eq!(p.start, 0.0);
        assert_eq!(p.mid, 0.0);
        assert_eq!(p.end, 0.0);
        assert_eq!(p.additional_weight, Weight(0.0));
    }

    // A real first-letter substitution (different letters) is unaffected by the
    // case-only branch and still gets the start penalty exactly once.
    #[test]
    fn real_first_letter_edit_not_double_counted() {
        let reweight = ReweightingConfig::default_const();
        let input_lower = graphemes("kat");
        let mut dl = Vec::new();

        let p = compute_reweight_penalties(
            &input_lower,
            Some("k"),
            "cat",
            None,
            Some(&reweight),
            &mut dl,
        );
        assert_eq!(p.start, reweight.start_penalty);
        assert_eq!(p.end, 0.0);
        assert_eq!(p.additional_weight, Weight(reweight.start_penalty));
    }

    // #65: the case reweight penalty can push the correct recapitalisation past
    // the beam; case-only variants of the input must survive the beam filter.
    #[test]
    fn beam_filter_keeps_case_only_variant() {
        let mut out = vec![
            Suggestion::new(SmolStr::new("girnoa"), Weight(5.0), None), // best, non-variant
            Suggestion::new(SmolStr::new("Girona"), Weight(25.0), None), // case-only variant
            Suggestion::new(SmolStr::new("garona"), Weight(25.0), None), // non-variant
        ];
        out.sort();

        let config = SpellerConfig {
            beam: Some(Weight(0.5)),
            max_weight: None,
            ..SpellerConfig::default()
        };
        apply_weight_limits(&mut out, &config, "girona");

        assert!(
            out.iter().any(|s| s.value() == "Girona"),
            "case-only variant should survive the beam: {:?}",
            out.iter().map(|s| s.value()).collect::<Vec<_>>()
        );
        assert!(
            !out.iter().any(|s| s.value() == "garona"),
            "non-variant beyond the beam should be dropped"
        );
        assert!(out.iter().any(|s| s.value() == "girnoa"));
    }
}
