//! divvunspell's tokenizer, its byte offsets turned into the code point
//! offsets Python indexes a `str` by.

use divvun_fst::tokenizer::{self, Tokenize as _};
use pyo3::prelude::*;

use crate::record::record_repr;

/// Code point offsets for byte offsets into `text`, asked for in order, so
/// that the text is counted once however many offsets there are.
struct CodePoints<'a> {
    text: &'a str,
    byte: usize,
    code_points: usize,
}

impl<'a> CodePoints<'a> {
    fn new(text: &'a str) -> Self {
        CodePoints {
            text,
            byte: 0,
            code_points: 0,
        }
    }

    /// The code point offset of `byte`, which is no earlier than the byte
    /// asked for before and lies on a character boundary.
    fn at(&mut self, byte: usize) -> usize {
        self.code_points += self.text[self.byte..byte].chars().count();
        self.byte = byte;
        self.code_points
    }

    fn all(text: &'a str, words: impl Iterator<Item = (usize, &'a str)>) -> Vec<(usize, String)> {
        let mut offsets = CodePoints::new(text);
        words
            .map(|(byte, word)| (offsets.at(byte), word.to_owned()))
            .collect()
    }
}

/// The words of `text`, each with the code point offset it starts at, so
/// that `text[start:start + len(word)]` is the word. A word is a run between
/// Unicode word boundaries with a letter or a digit in it; spaces and
/// punctuation are left out.
#[pyfunction]
pub fn word_indices(text: &str) -> Vec<(usize, String)> {
    CodePoints::all(text, text.word_indices())
}

/// Every run of `text` between Unicode word boundaries, spaces and
/// punctuation included, each with the code point offset it starts at. The
/// runs make up the whole text.
#[pyfunction]
pub fn word_bound_indices(text: &str) -> Vec<(usize, String)> {
    CodePoints::all(text, text.word_bound_indices())
}

/// The word at a cursor and the words around it, from the text before the
/// cursor and the text after it. Each offset is a code point offset into
/// `before + after`.
#[pyclass(frozen, get_all, eq, module = "divvun_fst")]
#[derive(Clone, PartialEq)]
pub struct WordContext {
    /// The word the cursor is in or beside, whole even where the cursor
    /// splits it, and where it starts. Where the cursor touches no word it
    /// is the empty string, at the cursor.
    current: (usize, String),
    /// The word before the current one, if any.
    first_before: Option<(usize, String)>,
    /// The word before that, if any.
    second_before: Option<(usize, String)>,
    /// The word after the current one, if any.
    first_after: Option<(usize, String)>,
    /// The word after that, if any.
    second_after: Option<(usize, String)>,
}

#[pymethods]
impl WordContext {
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        record_repr!(
            py,
            "WordContext",
            self,
            [
                current,
                first_before,
                second_before,
                first_after,
                second_after
            ]
        )
    }
}

/// The word at the cursor between `before` and `after`, and up to two words
/// on either side of it.
#[pyfunction]
pub fn cursor_context(before: &str, after: &str) -> WordContext {
    let found = tokenizer::cursor_context(before, after);
    // divvunspell gives the words before the cursor, and a current word that
    // starts there, offsets into `before`; it gives the words after it
    // offsets into `after`, and a current word that starts after the cursor
    // an offset into the two together. Each is made an offset into the two
    // together here.
    let code_points = |byte: usize| match byte.checked_sub(before.len()) {
        None => before[..byte].chars().count(),
        Some(rest) => before.chars().count() + after[..rest].chars().count(),
    };
    let word = |byte: usize, word: &str| (code_points(byte), word.to_owned());
    let after_word = |found: Option<(usize, &str)>| found.map(|(at, w)| word(before.len() + at, w));
    WordContext {
        current: word(found.current.0, &found.current.1),
        first_before: found.first_before.map(|(at, w)| word(at, w)),
        second_before: found.second_before.map(|(at, w)| word(at, w)),
        first_after: after_word(found.first_after),
        second_after: after_word(found.second_after),
    }
}
