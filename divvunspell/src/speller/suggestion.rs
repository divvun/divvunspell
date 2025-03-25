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
}

impl Suggestion {
    /// creates a spelling correction suggestion
    pub fn new(value: SmolStr, weight: Weight, completed: Option<bool>) -> Suggestion {
        Suggestion {
            value,
            weight,
            completed,
        }
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
}

impl PartialOrd for Suggestion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Suggestion {
    fn cmp(&self, other: &Self) -> Ordering {
        let x = self.weight.partial_cmp(&other.weight).unwrap_or(Equal);

        if let Equal = x {
            return self.value.cmp(&other.value);
        }

        x
    }
}

impl PartialEq for Suggestion {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value && self.weight == other.weight
    }
}

impl Eq for Suggestion {}
