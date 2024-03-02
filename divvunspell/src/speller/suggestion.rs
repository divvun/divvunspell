use crate::types::Weight;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::cmp::Ordering;
use std::cmp::Ordering::Equal;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Suggestion {
    pub value: SmolStr,
    pub weight: Weight,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<bool>,
}

impl Suggestion {
    pub fn new(value: SmolStr, weight: Weight, completed: Option<bool>) -> Suggestion {
        Suggestion {
            value,
            weight,
            completed,
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn weight(&self) -> Weight {
        self.weight
    }

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
