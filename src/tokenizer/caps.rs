use hashbrown::HashSet;
use std::iter::FromIterator;
use unic_segment::GraphemeIndices;

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

fn without_punctuation(alphabet: &Vec<String>) -> Vec<&String> {
    alphabet.iter().filter(|x| {
        !PUNCTUATION.contains(&x.as_str())
    }).collect()
}

struct TrimThingy<'a> {
    start: &'a str,
    end: &'a str,
    both: &'a str,
}

fn trim<'word>(word: &'word str, alphabet: &[&String]) -> TrimThingy<'word> {
    let start = GraphemeIndices::new(word)
        .take_while(|(_, c)| alphabet.iter().any(|x: &&String| c == x))
        .next()
        .map(|(i, _)| i)
        .unwrap_or(0);

    let end = GraphemeIndices::new(&word[start..])
        .take_while(|(_, c)| !alphabet.iter().any(|x: &&String| c == x))
        .next()
        .map(|(i, _)| start + i)
        .unwrap_or(word.len());

    TrimThingy {
        start: &word[start..],
        end: &word[..end],
        both: &word[start..end],
    }
}

pub fn word_variants(alphabet: &Vec<String>, word: &str) -> Vec<String> {
    let alphabet = without_punctuation(alphabet);
    let trim = trim(word, &alphabet);

    let mut base = vec![
        word.to_string(),
        trim.start.to_string(),
        trim.end.to_string(),
        trim.both.to_string(),
    ];

    base.append(&mut base.iter().map(|x| lower_case(x)).collect());
    base.append(&mut base.iter().map(|x| upper_first(x)).collect());

    let mut ret = vec![];

    for b in base.into_iter() {
        if !ret.contains(&b) {
            ret.push(b);
        }
    }

    ret
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