//! Suggestion for a spelling correction.
use crate::types::Weight;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::cmp::Ordering;
use std::cmp::Ordering::Equal;

#[derive(Clone, Debug, Serialize, Deserialize)]
/// Suggestion for a spelling correction
pub struct Suggestion {
    /// the suggested word-form
    pub value: SmolStr,
    /// total penalty weight of the word-form
    pub weight: Weight,
    /// whether the word is completed or partial
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<bool>,
    /// detailed weight information (only filled when verbose mode is enabled)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight_details: Option<WeightDetails>,
    /// The lexicon's own share of `weight`: what the language model charged for
    /// the word-form itself, before anything the error model charged for
    /// reaching it. Lower means a likelier word.
    ///
    /// Internal, and carried in *every* mode rather than only the verbose one,
    /// because the ordering below breaks exact ties on `weight` with it — a tie
    /// that the search can decide on evidence it already has, instead of on
    /// whichever candidate happened to be produced first. `weight_details` will
    /// not do: it is only meaningful under `verbose`, and there it holds a
    /// figure measured differently (the best lexicon-only analysis of the
    /// output form, per #73), so ordering on it would make a debugging flag
    /// change the answer.
    ///
    /// Never serialized: the wire format is the public one, and a suggestion
    /// read back from it carries no lexicon share, which leaves the tie-break
    /// inert rather than wrong.
    #[serde(skip, default = "no_lexicon_weight")]
    pub(crate) lexicon_weight: Weight,
    /// Whether this suggestion puts a space back into the input — a word-split
    /// correction, produced by `HfstSpeller::word_split_suggestions` rather
    /// than by the error-model search.
    ///
    /// Splitting a compound is itself a bad-behaviour pattern: the language
    /// writes compounds, and offering to break one apart is rarely what the
    /// writer meant even when the arithmetic makes it look as good as an
    /// ordinary correction. So a split never wins a tie — see
    /// [`Suggestion::cmp`].
    ///
    /// Carried the same way as `lexicon_weight`: internal, present in every
    /// mode, and never serialized, so the wire format is unchanged and a
    /// suggestion read back from it is simply not a split.
    #[serde(skip)]
    pub(crate) is_split: bool,
}

/// Serde default for the skipped [`Suggestion::lexicon_weight`].
fn no_lexicon_weight() -> Weight {
    Weight::ZERO
}

#[derive(Clone, Debug, Serialize, Deserialize)]
/// Detailed weight information for a suggestion
pub struct WeightDetails {
    /// weight from the lexicon (acceptor)
    pub lexicon_weight: Weight,
    /// weight from the error model (mutator)
    pub mutator_weight: Weight,
    /// reweighting penalty at start of word
    pub reweight_start: f32,
    /// reweighting penalty in middle of word  
    pub reweight_mid: f32,
    /// reweighting penalty at end of word
    pub reweight_end: f32,
}

impl Suggestion {
    /// creates a spelling correction suggestion
    pub fn new(value: SmolStr, weight: Weight, completed: Option<bool>) -> Suggestion {
        Suggestion {
            value,
            weight,
            completed,
            weight_details: None,
            lexicon_weight: Weight::ZERO,
            is_split: false,
        }
    }

    /// creates a spelling correction suggestion with detailed weight information
    pub fn new_with_details(
        value: SmolStr,
        weight: Weight,
        completed: Option<bool>,
        details: WeightDetails,
    ) -> Suggestion {
        Suggestion {
            value,
            weight,
            completed,
            weight_details: Some(details),
            lexicon_weight: Weight::ZERO,
            is_split: false,
        }
    }

    /// Record what the lexicon charged for this word-form, for the tie-break in
    /// [`Suggestion::cmp`].
    pub(crate) fn with_lexicon_weight(mut self, lexicon_weight: Weight) -> Suggestion {
        self.lexicon_weight = lexicon_weight;
        self
    }

    /// Mark this suggestion as a word split, which loses every tie it enters.
    pub(crate) fn with_word_split(mut self) -> Suggestion {
        self.is_split = true;
        self
    }

    /// gets the suggested word-form
    pub fn value(&self) -> &str {
        &self.value
    }

    /// gets the penalty weight of the suggestion
    pub fn weight(&self) -> Weight {
        self.weight
    }

    /// returns whether this suggestion is a full word or partial
    pub fn completed(&self) -> Option<bool> {
        self.completed
    }

    /// gets the detailed weight information if available
    pub fn weight_details(&self) -> Option<&WeightDetails> {
        self.weight_details.as_ref()
    }
}

impl PartialOrd for Suggestion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Suggestion {
    /// Cheapest total first; then whole words before splits; then the lexicon.
    ///
    /// Two corrections can cost the same to reach and still not be equally good
    /// answers. Two things separate them, in this order.
    ///
    /// A word split — putting a space back into what was typed as one word —
    /// is a suggestion of a different kind, not merely a dearer one. The
    /// languages this serves compound freely, so a split can be arithmetically
    /// as cheap as an ordinary correction while being the wrong advice:
    /// telling a writer to break a compound apart is itself a bad-behaviour
    /// pattern. At equal total weight the whole word wins, always. The split is
    /// still offered, just never ahead of a correction that cost the same.
    ///
    /// Below that, the lexicon's own share of the weight: one candidate may be
    /// a word the language model rates as far more plausible. That figure is
    /// computed during the search and was, until now, thrown away before the
    /// final ordering, leaving equal-weight suggestions in whatever order the
    /// search produced them. Consulting it costs nothing and is
    /// language-independent — it is the lexicon's own opinion, whichever
    /// lexicon is loaded.
    ///
    /// `Weight` orders by `f32::total_cmp`, so this is total and NaN-safe: a
    /// NaN weight sorts to one end rather than making the comparison
    /// inconsistent.
    fn cmp(&self, other: &Self) -> Ordering {
        match self.weight.cmp(&other.weight) {
            // `false < true`, so a non-split sorts ahead of a split.
            Equal => match self.is_split.cmp(&other.is_split) {
                Equal => match self.lexicon_weight.cmp(&other.lexicon_weight) {
                    Equal => self.value.cmp(&other.value),
                    lexicon => lexicon,
                },
                split => split,
            },
            weight => weight,
        }
    }
}

impl PartialEq for Suggestion {
    fn eq(&self, other: &Self) -> bool {
        // Includes the lexicon share and the split flag so `Eq` stays
        // consistent with `Ord`, which only reports `Equal` when those agree
        // too. Nothing outside this crate can set either, so suggestions built
        // through the public constructors compare exactly as they always did.
        self.value == other.value
            && self.weight == other.weight
            && self.is_split == other.is_split
            && self.lexicon_weight == other.lexicon_weight
    }
}

impl Eq for Suggestion {}

#[cfg(test)]
mod tests {
    use super::*;

    fn sugg(value: &str, weight: f32, lexicon_weight: f32) -> Suggestion {
        Suggestion::new(SmolStr::new(value), Weight(weight), None)
            .with_lexicon_weight(Weight(lexicon_weight))
    }

    fn ordered(mut suggestions: Vec<Suggestion>) -> Vec<String> {
        suggestions.sort();
        suggestions
            .into_iter()
            .map(|s| s.value.to_string())
            .collect()
    }

    // The point of the whole exercise: two corrections that cost exactly the
    // same are separated by the lexicon, and the likelier word wins — even
    // when, as here, that reverses the alphabetical order the tie used to fall
    // back on.
    #[test]
    fn exact_tie_prefers_the_likelier_word() {
        let order = ordered(vec![sugg("aardvark", 8.0, 40.0), sugg("zebra", 8.0, 1.0)]);
        assert_eq!(order, ["zebra", "aardvark"]);
    }

    // The tie-break is strictly a tie-break: a cheaper total wins outright, no
    // matter how implausible the word behind it.
    #[test]
    fn cheaper_total_wins_over_a_likelier_word() {
        let order = ordered(vec![sugg("likely", 9.0, 1.0), sugg("cheap", 8.0, 40.0)]);
        assert_eq!(order, ["cheap", "likely"]);
    }

    // Equal totals *and* equal lexicon weights fall through to the previous
    // behaviour, so nothing that was already decided changes.
    #[test]
    fn equal_lexicon_weights_keep_the_previous_order() {
        let order = ordered(vec![sugg("cat", 8.0, 3.0), sugg("car", 8.0, 3.0)]);
        assert_eq!(order, ["car", "cat"]);
    }

    // A suggestion built through the public constructors carries no lexicon
    // share, which leaves the tie-break inert rather than ordering everything
    // as if it were maximally likely.
    #[test]
    fn suggestions_without_a_lexicon_share_are_unaffected() {
        let order = ordered(vec![
            Suggestion::new(SmolStr::new("cat"), Weight(8.0), None),
            Suggestion::new(SmolStr::new("car"), Weight(8.0), None),
        ]);
        assert_eq!(order, ["car", "cat"]);
    }

    fn split(value: &str, weight: f32, lexicon_weight: f32) -> Suggestion {
        sugg(value, weight, lexicon_weight).with_word_split()
    }

    // The directive: at equal total weight the compound wins, whatever the
    // lexicon thinks of it and whatever the alphabet would have said. Both
    // orders of construction, so this cannot pass by luck of the input order.
    #[test]
    fn a_split_never_outranks_an_equal_weight_whole_word() {
        assert_eq!(
            ordered(vec![
                split("com pound", 20.0, 1.0),
                sugg("compound", 20.0, 40.0)
            ]),
            ["compound", "com pound"]
        );
        assert_eq!(
            ordered(vec![
                sugg("compound", 20.0, 40.0),
                split("com pound", 20.0, 1.0)
            ]),
            ["compound", "com pound"]
        );
        // Even against a whole word the alphabet would have put last.
        assert_eq!(
            ordered(vec![split("a b", 20.0, 0.0), sugg("zebra", 20.0, 0.0)]),
            ["zebra", "a b"]
        );
    }

    // Strictly a tie-break: weight still decides when the weights differ, in
    // both directions. A cheaper split is still the first answer.
    #[test]
    fn weight_still_decides_against_a_split() {
        assert_eq!(
            ordered(vec![
                sugg("compound", 25.0, 0.0),
                split("com pound", 20.0, 0.0)
            ]),
            ["com pound", "compound"]
        );
        assert_eq!(
            ordered(vec![
                sugg("compound", 20.0, 0.0),
                split("com pound", 25.0, 0.0)
            ]),
            ["compound", "com pound"]
        );
    }

    // Two splits at the same weight fall through to the ordering that was
    // already there, rather than to whichever the search produced first.
    #[test]
    fn splits_among_themselves_keep_the_lexicon_tie_break() {
        assert_eq!(
            ordered(vec![
                split("aard vark", 8.0, 40.0),
                split("ze bra", 8.0, 1.0)
            ]),
            ["ze bra", "aard vark"]
        );
    }

    // A spaced value is not by itself a split: the lexicon can spell a phrase.
    // Only the flag demotes, so a phrase the search found keeps competing on
    // the lexicon share as before.
    #[test]
    fn a_space_alone_does_not_demote() {
        assert_eq!(
            ordered(vec![
                sugg("dear phrase", 8.0, 1.0),
                sugg("cheap", 8.0, 40.0)
            ]),
            ["dear phrase", "cheap"]
        );
    }

    // The wire format is the public one and does not change: a split is
    // serialized exactly as any other suggestion, and read back as a
    // non-split, which leaves the demotion inert rather than wrong.
    #[test]
    fn the_split_flag_stays_off_the_wire() {
        let json =
            serde_json::to_string(&split("com pound", 20.0, 1.0)).expect("a suggestion serializes");
        assert_eq!(json, r#"{"value":"com pound","weight":20.0}"#);

        let read_back: Suggestion = serde_json::from_str(&json).expect("and reads back");
        assert!(!read_back.is_split);
    }

    // `sort` needs a total order; a NaN weight must not make the comparison
    // inconsistent. `Weight` compares by `f32::total_cmp`, so NaN lands at one
    // end and everything else keeps its order.
    #[test]
    fn nan_weight_sorts_last_without_disturbing_the_rest() {
        let order = ordered(vec![
            sugg("nan", f32::NAN, 0.0),
            sugg("dear", 9.0, 0.0),
            sugg("cheap", 8.0, 0.0),
        ]);
        assert_eq!(order, ["cheap", "dear", "nan"]);
    }
}
