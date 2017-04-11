use std::cmp::Ordering;

use types::Weight;

#[derive(Clone)]
pub struct Suggestion {
    value: String,
    weight: Weight
}

impl PartialEq for Suggestion {
    fn eq(&self, other: &Self) -> bool {
        self.weight == other.weight
    }
}

impl Eq for Suggestion {}

impl PartialOrd for Suggestion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Suggestion {
    fn cmp(&self, other: &Self) -> Ordering {
        other.value.cmp(&self.value)
    }
}