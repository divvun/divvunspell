// // use unic_ucd_category;

// use unic_ucd_category::{GeneralCategory};
use unic_segment::{WordBoundIndices, Words};

pub mod caps;

// #[derive(Debug)]
// pub enum Token<'a> {
//     Word(&'a str, usize, usize),
//     Punctuation(&'a str, usize, usize),
//     Whitespace(&'a str, usize, usize),
//     Other(&'a str, usize, usize)
// }

// impl<'a> Token<'a> {
//     pub fn value(&self) -> &str {
//         match self {
//             Token::Word(x, _, _) => x,
//             Token::Punctuation(x, _, _) => x,
//             Token::Whitespace(x, _, _) => x,
//             Token::Other(x, _, _) => x
//         }
//     }

//     pub fn start(&self) -> usize {
//         match *self {
//             Token::Word(_, x, _) => x,
//             Token::Punctuation(_, x, _) => x,
//             Token::Whitespace(_, x, _) => x,
//             Token::Other(_, x, _) => x
//         }
//     }

//     pub fn end(&self) -> usize {
//         match *self {
//             Token::Word(_, _, x) => x,
//             Token::Punctuation(_, _, x) => x,
//             Token::Whitespace(_, _, x) => x,
//             Token::Other(_, _, x) => x
//         }
//     }

//     pub fn is_word(&self) -> bool {
//         match *self {
//             Token::Word(_, _, _) => true,
//             _ => false
//         }
//     }
// }

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

// pub struct Tokenizer<'a> {
//     pub(crate) text: &'a str,
//     context: TokenContext,
//     indices: ::std::str::CharIndices<'a>,
//     first_word_ch: usize,
//     c: usize
// }

// impl<'a> Tokenizer<'a> {
//     fn new(text: &'a str) -> Tokenizer<'a> {
//         Tokenizer {
//             text,
//             context: TokenContext::Start,
//             indices: text.char_indices(),
//             first_word_ch: 0,
//             c: 0
//         }
//     }
// }

// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
// enum TokenContext {
//     Start,
//     Word,
//     Punctuation,
//     Whitespace,
//     Other
// }

// impl TokenContext {
//     fn from(word_break: WordBreak, category: GeneralCategory) -> TokenContext {
//         match (word_break, category) {
//             (WordBreak::ALetter, _) | (WordBreak::Extend, _) => TokenContext::Word,
//             (_, category) if category.is_punctuation() => TokenContext::Punctuation,
//             (WordBreak::CR, _) | (WordBreak::LF, _) | (WordBreak::Newline, _) => TokenContext::Whitespace,
//             (_, category) if category.is_separator() => TokenContext::Whitespace,
//             _ => TokenContext::Other
//         }
//     }

//     fn token<'a>(&self, text: &'a str, start: usize, end: usize) -> Token<'a> {
//         match self {
//             TokenContext::Start => unreachable!(),
//             TokenContext::Word => Token::Word(text, start, end),
//             TokenContext::Punctuation => Token::Punctuation(text, start, end),
//             TokenContext::Whitespace => Token::Whitespace(text, start, end),
//             TokenContext::Other => Token::Other(text, start, end)
//         }
//     }
// }

// impl<'a> Iterator for Tokenizer<'a> {
//     type Item = Token<'a>;

//     fn next(&mut self) -> Option<Self::Item> {
//         while let Some((n, ch)) = self.indices.next() {
//             let context = TokenContext::from(WordBreak::of(ch), GeneralCategory::of(ch));
//             if self.context == TokenContext::Start {
//                 self.context = context
//             }
//             let prev_context = self.context;

//             if context != prev_context {
//                 self.context = context;

//                 let start = self.first_word_ch;
//                 let end = start + self.c;
//                 let value = &self.text[start..end];

//                 self.first_word_ch = n;
//                 self.c = 0;

//                 self.c += ch.len_utf8();

//                 return Some(prev_context.token(value, start, end));
//             } else {
//                 self.c += ch.len_utf8();
//             }
//         };

//         if self.c > 0 {
//             let start = self.first_word_ch;
//             let end = start + self.c;
//             let value = &self.text[start..end];

//             self.c = 0;

//             return Some(self.context.token(value, start, end));
//         }

//         None
//     }
// }

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