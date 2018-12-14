use std::cmp::Ordering;
use std::cmp::Ordering::Equal;
use crate::types::Weight;

#[derive(Clone, Debug)]
pub struct Suggestion {
    pub value: String,
    pub weight: Weight,
}

impl Suggestion {
    pub fn new(value: String, weight: Weight) -> Suggestion {
        Suggestion {
            value: value,
            weight: weight,
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn weight(&self) -> Weight {
        self.weight
    }
}

impl PartialOrd for Suggestion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Suggestion {
    fn cmp(&self, other: &Self) -> Ordering {
        let x = self
            .weight
            .partial_cmp(&other.weight)
            .unwrap_or(Equal);

        if let Equal = x {
            return self.value.cmp(&other.value)
        }

        x
    }
}

impl PartialEq for Suggestion {
    fn eq(&self, other: &Self) -> bool {
        self.weight == other.weight
    }
}

impl Eq for Suggestion {}
