use std::cmp::Ordering;
use std::cmp::Ordering::Equal;
use types::Weight;

#[derive(Clone, Debug)]
pub struct Suggestion {
    value: String,
    weight: Weight
}

impl Suggestion {
    pub fn new(value: String, weight: Weight) -> Suggestion {
        Suggestion {
            value: value,
            weight: weight
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
        other.weight.partial_cmp(&self.weight).unwrap_or(Equal).reverse()
    }
}

impl PartialEq for Suggestion {
    fn eq(&self, other: &Self) -> bool {
        self.weight == other.weight
    }
}

impl Eq for Suggestion {}
