use word::{WordBoundIndices, Words};

pub mod caps;
pub mod word;
mod word_break;

pub trait Tokenize {
    fn word_bound_indices(&self) -> WordBoundIndices;
    fn words(&self) -> Words;

    fn word_bound_indices_with_alphabet(&self, alphabet: Vec<char>) -> WordBoundIndices;
    fn words_with_alphabet(&self, alphabet: Vec<char>) -> Words;
}

impl Tokenize for str {
    fn word_bound_indices(&self) -> WordBoundIndices {
        WordBoundIndices::new(self)
    }

    fn words(&self) -> Words {
        Words::new(self, |s| s.chars().any(|ch| ch.is_alphanumeric()))
    }

    fn word_bound_indices_with_alphabet(&self, alphabet: Vec<char>) -> WordBoundIndices {
        WordBoundIndices::new_with_alphabet(self, alphabet)
    }

    fn words_with_alphabet(&self, alphabet: Vec<char>) -> Words {
        Words::new_with_alphabet(self, |s| s.chars().any(|ch| ch.is_alphanumeric()), alphabet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let msg = "this is an ordinary sentence! \"This was quoted,\", an emoji: (😄), and\t a tab was there and a new line.\n Some extreme unicode; bismala: (﷽), in long form: بِسْمِ اللهِ الرَّحْمٰنِ الرَّحِيْمِ.";
        msg.word_bound_indices().for_each(|t| println!("{:?}", t));
        println!("{}", &msg);
    }

    #[test]
    fn word_bounds_alphabet() {
        let msg = "this is an ordinary-sentence! \"This was quoted,\", an emoji: (😄), and\t a";

        assert_eq!(
            msg.word_bound_indices_with_alphabet("abcdefghijklmnopqr-stuvwxyz".chars().collect()).collect::<Vec<(usize, &str)>>(),
            vec![(0, "this"), (4, " "), (5, "is"), (7, " "), (8, "an"), (10, " "),
                (11, "ordinary-sentence"), (28, "!"), (29, " "),
                (30, "\""), (31, "This"), (35, " "), (36, "was"), (39, " "), (40, "quoted"),
                (46, ","), (47, "\""), (48, ","), (49, " "), (50, "an"), (52, " "),
                (53, "emoji"), (58, ":"), (59, " "), (60, "("), (61, "😄"), (65, ")"),
                (66, ","), (67, " "), (68, "and"), (71, "\t"), (72, " "), (73, "a")]
        );
    }
}
