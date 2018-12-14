use hashbrown::HashSet;

fn trim_start(alphabet: &Vec<String>, word: &str) -> String {
    word.trim_start_matches(|x: char| !alphabet.contains(&x.to_string())).to_string()
}

fn trim_end(alphabet: &Vec<String>, word: &str) -> String {
    word.trim_end_matches(|x: char| !alphabet.contains(&x.to_string())).to_string()
}

fn trim_both(alphabet: &Vec<String>, word: &str) -> String {
    word.trim_matches(|x: char| !alphabet.contains(&x.to_string())).to_string()
}

pub fn lower_case(s: &str) -> String {
    s.chars().map(|c| c.to_lowercase().collect::<String>()).collect::<String>()
}

pub fn upper_case(s: &str) -> String {
    s.chars().map(|c| c.to_uppercase().collect::<String>()).collect::<String>()
}

pub fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

static PUNCTUATION: &'static [&'static str] = &[
    "!", "\"", "#", "$", "%", "&", "'", "(", ")", "*", "+", 
    ",", "-", ".", "/", ":", ";", "<", "=", ">", "?", 
    "@", "[", "\\", "]", "^", "_", "`", "{", "|", "}", "~"
];

fn without_punctuation(alphabet: &Vec<String>) -> Vec<String> {
    let x = alphabet.iter().filter(|x| {
        !PUNCTUATION.contains(&x.as_str())
    }).map(|x| x.to_owned());
    x.collect::<Vec<_>>()
}

use std::iter::FromIterator;

pub fn word_variants(alphabet: &Vec<String>, word: &str) -> HashSet<String> {
    let alphabet = without_punctuation(alphabet);

    let mut base = vec![
        word.to_string(),
        trim_start(&alphabet, word),
        trim_end(&alphabet, word),
        trim_both(&alphabet, word)
    ];

    base.append(&mut base.iter().map(|x| lower_case(x)).collect());
    base.append(&mut base.iter().map(|x| upper_first(x)).collect());

    HashSet::from_iter(base)
}

pub fn is_all_caps(word: &str) -> bool {
    upper_case(word) == word
}

pub fn is_first_caps(word: &str) -> bool {
    upper_first(word) == word
}

mod tests {
    use super::*;

    #[test]
    fn testsd() {
        let a: Vec<String> = vec!["a".into(), "b".into(), "c".into(), "A".into(), "B".into(), "C".into()];
        println!("{:?}", word_variants(&a, "abc"));
        println!("{:?}", word_variants(&a, "$ABC$"));
    }
}