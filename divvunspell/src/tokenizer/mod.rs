use unic_ucd_common::alphanumeric::is_alphanumeric;
use word::{WordBoundIndices, Words};

pub(crate) mod case_handling;
pub mod word;
mod word_break;

pub struct WordIndices<'a> {
    iter: WordBoundIndices<'a>,
}

impl<'a> Iterator for WordIndices<'a> {
    type Item = (usize, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(item) = self.iter.next() {
            if item.1.chars().any(is_alphanumeric) {
                return Some(item);
            }
        }

        None
    }
}

pub trait Tokenize {
    fn word_bound_indices(&self) -> WordBoundIndices<'_>;
    fn word_indices(&self) -> WordIndices<'_>;
    fn word_bound_indices_with_alphabet(&self, alphabet: Vec<char>) -> WordBoundIndices;
    fn words_with_alphabet(&self, alphabet: Vec<char>) -> Words;
}

impl Tokenize for str {
    fn word_bound_indices(&self) -> WordBoundIndices<'_> {
        WordBoundIndices::new(self)
    }

    fn word_indices(&self) -> WordIndices<'_> {
        WordIndices {
            iter: WordBoundIndices::new(self),
        }
    }

    fn word_bound_indices_with_alphabet(&self, alphabet: Vec<char>) -> WordBoundIndices {
        WordBoundIndices::new_with_alphabet(self, alphabet)
    }

    fn words_with_alphabet(&self, alphabet: Vec<char>) -> Words {
        Words::new_with_alphabet(self, |s| s.chars().any(|ch| ch.is_alphanumeric()), alphabet)
    }
}

pub struct IndexedWord {
    pub index: usize,
    pub word: String,
}

#[derive(Debug, Clone)]
pub struct WordContext {
    pub current: (usize, String),
    pub first_before: Option<(usize, String)>,
    pub second_before: Option<(usize, String)>,
    pub first_after: Option<(usize, String)>,
    pub second_after: Option<(usize, String)>,
}

#[cfg(feature = "internal_ffi")]
impl crate::ffi::fbs::IntoFlatbuffer for WordContext {
    fn into_flatbuffer<'a>(self) -> Vec<u8> {
        use crate::ffi::fbs::tokenizer::*;

        macro_rules! add_indexed_word {
            ($fbb:expr, $data:expr) => {{
                use $crate::ffi::fbs::tokenizer::*;

                if let Some((index, word)) = $data {
                    let s = $fbb.create_string(&word);
                    Some(IndexedWord::create(
                        &mut $fbb,
                        &IndexedWordArgs {
                            index: index as u64,
                            value: Some(s),
                        },
                    ))
                } else {
                    None
                }
            }};
        }

        let mut builder = flatbuffers::FlatBufferBuilder::new_with_capacity(1024);
        let current = add_indexed_word!(builder, Some(self.current));
        let first_before = add_indexed_word!(builder, self.first_before);
        let second_before = add_indexed_word!(builder, self.second_before);
        let first_after = add_indexed_word!(builder, self.first_after);
        let second_after = add_indexed_word!(builder, self.second_after);
        let word_context = WordContext::create(
            &mut builder,
            &WordContextArgs {
                current,
                first_before,
                second_before,
                first_after,
                second_after,
            },
        );
        builder.finish(word_context, None);
        builder.finished_data().to_vec()
    }
}

pub fn cursor_context(first_half: &str, second_half: &str) -> WordContext {
    // Find the point in the first half where the first "word" happens
    let mut first_half_iter = first_half.word_bound_indices().rev();
    let mut second_half_iter = second_half.word_bound_indices();

    let current = {
        let first_half_last_item = match first_half_iter.next() {
            Some(v) if v.1.chars().any(is_alphanumeric) => v,
            _ => (0, ""),
        };

        let second_half_first_item = match second_half_iter.next() {
            Some(v) if v.1.chars().any(is_alphanumeric) => v,
            _ => (0, ""),
        };

        let first_word = format!("{}{}", first_half_last_item.1, second_half_first_item.1);
        let first_index = if first_half_last_item.1 == "" {
            first_half.len() + second_half_first_item.0
        } else {
            first_half_last_item.0
        };

        (first_index, first_word)
    };

    let mut first_half_iter = first_half_iter
        .filter(|x| x.1.chars().any(is_alphanumeric))
        .map(|x| (x.0, x.1.to_string()));
    let mut second_half_iter = second_half_iter
        .filter(|x| x.1.chars().any(is_alphanumeric))
        .map(|x| (x.0, x.1.to_string()));

    WordContext {
        current,
        first_before: first_half_iter.next(),
        second_before: first_half_iter.next(),
        first_after: second_half_iter.next(),
        second_after: second_half_iter.next(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rev() {
        let msg = "this is life";
        assert_eq!(
            msg.word_bound_indices()
                .rev()
                .collect::<Vec<(usize, &str)>>(),
            vec![(8, "life"), (7, " "), (5, "is"), (4, " "), (0, "this")]
        );
    }

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
            msg.word_bound_indices_with_alphabet("abcdefghijklmnopqr-stuvwxyz".chars().collect())
                .collect::<Vec<_>>(),
            vec![
                (0, "this"),
                (4, " "),
                (5, "is"),
                (7, " "),
                (8, "an"),
                (10, " "),
                (11, "ordinary-sentence"),
                (28, "!"),
                (29, " "),
                (30, "\""),
                (31, "This"),
                (35, " "),
                (36, "was"),
                (39, " "),
                (40, "quoted"),
                (46, ","),
                (47, "\""),
                (48, ","),
                (49, " "),
                (50, "an"),
                (52, " "),
                (53, "emoji"),
                (58, ":"),
                (59, " "),
                (60, "("),
                (61, "😄"),
                (65, ")"),
                (66, ","),
                (67, " "),
                (68, "and"),
                (71, "\t"),
                (72, " "),
                (73, "a")
            ]
        );
    }
}
