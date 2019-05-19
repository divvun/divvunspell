use unic_segment::{WordBoundIndices, Words};

pub mod caps;

pub trait Tokenize {
    fn word_bound_indices(&self) -> WordBoundIndices;
    fn words(&self) -> Words;
}

impl Tokenize for str {
    fn word_bound_indices(&self) -> WordBoundIndices {
        WordBoundIndices::new(self)
    }

    fn words(&self) -> Words {
        Words::new(self, |s| s.chars().any(|ch| ch.is_alphanumeric()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let msg = "this is an ordinary sentence! \"This was quoted,\", an emoji: (😄), and\t a tab was there and a new line.\n Some extreme unicode; bismala: (﷽), in long form: بِسْمِ اللهِ الرَّحْمٰنِ الرَّحِيْمِ.";
        msg.word_bound_indices().for_each(|t| { println!("{:?}", t) });
        println!("{}", &msg);
    }
}