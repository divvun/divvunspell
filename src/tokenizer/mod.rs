//! Tokenizer splits strings into words and punctuations.
use std::borrow::Cow;
use unic_ucd_common::alphanumeric::is_alphanumeric;
use word::{WordBoundIndices, Words};

pub(crate) mod case_handling;
pub mod word;
mod word_break;

/// Iterator over word indices in a string, filtering out non-alphanumeric tokens.
///
/// Returns tuples of (byte_offset, word_str) for each word containing at least
/// one alphanumeric character.
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

/// Trait for tokenizing strings into words.
///
/// Provides methods to split text into words with various options for
/// alphabet customization and boundary detection.
pub trait Tokenize {
    /// Get an iterator over word boundaries with byte indices.
    fn word_bound_indices(&self) -> WordBoundIndices<'_>;

    /// Get an iterator over words with byte indices (alphanumeric words only).
    fn word_indices(&self) -> WordIndices<'_>;

    /// Get word boundaries using a custom alphabet.
    fn word_bound_indices_with_alphabet(&self, alphabet: Vec<char>) -> WordBoundIndices<'_>;

    /// Get words using a custom alphabet.
    fn words_with_alphabet(&self, alphabet: Vec<char>) -> Words<'_>;
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

    fn word_bound_indices_with_alphabet(&self, alphabet: Vec<char>) -> WordBoundIndices<'_> {
        WordBoundIndices::new_with_alphabet(self, alphabet)
    }

    fn words_with_alphabet(&self, alphabet: Vec<char>) -> Words<'_> {
        Words::new_with_alphabet(self, |s| s.chars().any(|ch| ch.is_alphanumeric()), alphabet)
    }
}

/// A word with its byte offset in the original string.
pub struct IndexedWord {
    /// Byte offset of the word in the original string
    pub index: usize,
    /// The word text
    pub word: String,
}

/// Context information for a word, including surrounding words.
///
/// Useful for context-sensitive spell-checking and analysis.
#[derive(Debug, Clone)]
pub struct WordContext<'a> {
    /// The current word (byte_offset, text)
    /// Uses Cow to handle words that span the cursor (owned) vs words on one side (borrowed)
    pub current: (usize, Cow<'a, str>),
    /// The word immediately before, if any
    pub first_before: Option<(usize, &'a str)>,
    /// The second word before, if any
    pub second_before: Option<(usize, &'a str)>,
    /// The word immediately after, if any
    pub first_after: Option<(usize, &'a str)>,
    /// The second word after, if any
    pub second_after: Option<(usize, &'a str)>,
}

/// Extract word context around a cursor position.
///
/// Given text split at a cursor position (first_half, second_half),
/// returns the word at the cursor and up to 2 words before/after.
///
/// The returned string slices reference the input strings, so they must
/// remain valid for the lifetime of the returned `WordContext`.
///
/// When the cursor splits a word, the current word will be owned (Cow::Owned),
/// otherwise it will be borrowed (Cow::Borrowed).
///
/// # Example
/// ```ignore
/// let context = cursor_context("hello wo", "rld goodbye");
/// // context.current would be ("hello ".len(), Cow::Owned("world"))
/// ```
pub fn cursor_context<'a>(first_half: &'a str, second_half: &'a str) -> WordContext<'a> {
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

        if first_half_last_item.1.is_empty() {
            let index = first_half.len() + second_half_first_item.0;
            (index, Cow::Borrowed(second_half_first_item.1))
        } else if second_half_first_item.1.is_empty() {
            (first_half_last_item.0, Cow::Borrowed(first_half_last_item.1))
        } else {
            let first_word = format!("{}{}", first_half_last_item.1, second_half_first_item.1);
            (first_half_last_item.0, Cow::Owned(first_word))
        }
    };

    let mut first_half_iter = first_half_iter
        .filter(|x| x.1.chars().any(is_alphanumeric));
    let mut second_half_iter = second_half_iter
        .filter(|x| x.1.chars().any(is_alphanumeric));

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
