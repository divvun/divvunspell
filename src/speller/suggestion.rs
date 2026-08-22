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
        }
    }

    /// Record what the lexicon charged for this word-form, for the tie-break in
    /// [`Suggestion::cmp`].
    pub(crate) fn with_lexicon_weight(mut self, lexicon_weight: Weight) -> Suggestion {
        self.lexicon_weight = lexicon_weight;
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
    /// Cheapest total first; exact ties decided by the lexicon.
    ///
    /// Two corrections can cost the same to reach and still not be equally good
    /// answers: one of them may be a word the language model rates as far more
    /// plausible. That figure is computed during the search and was, until now,
    /// thrown away before the final ordering, leaving equal-weight suggestions
    /// in whatever order the search produced them. Consulting it costs nothing
    /// and is language-independent — it is the lexicon's own opinion, whichever
    /// lexicon is loaded.
    ///
    /// `Weight` orders by `f32::total_cmp`, so this is total and NaN-safe: a
    /// NaN weight sorts to one end rather than making the comparison
    /// inconsistent.
    fn cmp(&self, other: &Self) -> Ordering {
        match self.weight.cmp(&other.weight) {
            Equal => match self.lexicon_weight.cmp(&other.lexicon_weight) {
                Equal => self.value.cmp(&other.value),
                lexicon => lexicon,
            },
            weight => weight,
        }
    }
}

impl PartialEq for Suggestion {
    fn eq(&self, other: &Self) -> bool {
        // Includes the lexicon share so `Eq` stays consistent with `Ord`, which
        // only reports `Equal` when that agrees too. Nothing outside this crate
        // can set it, so suggestions built through the public constructors
        // compare exactly as they always did.
        self.value == other.value
            && self.weight == other.weight
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
