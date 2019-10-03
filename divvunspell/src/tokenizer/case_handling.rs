use smol_str::SmolStr;

fn trim_start(alphabet: &[SmolStr], word: &str) -> SmolStr {
    word.trim_start_matches(|x: char| !alphabet.contains(&SmolStr::from(x.to_string())))
        .into()
}

fn trim_end(alphabet: &[SmolStr], word: &str) -> SmolStr {
    word.trim_end_matches(|x: char| !alphabet.contains(&SmolStr::from(x.to_string())))
        .into()
}

fn trim_both(alphabet: &[SmolStr], word: &str) -> SmolStr {
    word.trim_matches(|x: char| !alphabet.contains(&SmolStr::from(x.to_string())))
        .into()
}

pub fn lower_case(s: &str) -> SmolStr {
    s.chars()
        .map(|c| c.to_lowercase().collect::<String>())
        .collect::<SmolStr>()
}

pub fn upper_case(s: &str) -> SmolStr {
    s.chars()
        .map(|c| c.to_uppercase().collect::<String>())
        .collect::<SmolStr>()
}

pub fn upper_first(s: &str) -> SmolStr {
    let mut c = s.chars();
    match c.next() {
        None => SmolStr::new(""),
        Some(f) => SmolStr::from(f.to_uppercase().collect::<String>() + c.as_str()),
    }
}

static PUNCTUATION: &[&str] = &[
    "!", "\"", "#", "$", "%", "&", "'", "(", ")", "*", "+", ",", "-", ".", "/", ":", ";", "<", "=",
    ">", "?", "@", "[", "\\", "]", "^", "_", "`", "{", "|", "}", "~",
];

fn without_punctuation(alphabet: &[SmolStr]) -> Vec<SmolStr> {
    let x = alphabet
        .iter()
        .filter(|x| !PUNCTUATION.contains(&x.as_str()))
        .map(|x| x.to_owned());
    x.collect::<Vec<_>>()
}

pub fn word_variants(alphabet: &[SmolStr], word: &str) -> Vec<SmolStr> {
    let alphabet = without_punctuation(alphabet);

    let mut base = vec![
        word.into(),
        trim_start(&alphabet, word),
        trim_end(&alphabet, word),
        trim_both(&alphabet, word),
    ];

    base.append(
        &mut base
            .iter()
            .filter(|x| is_all_caps(x))
            .map(|x| upper_first(&lower_case(x)))
            .collect(),
    );
    base.append(&mut base.iter().map(|x| lower_case(x)).collect());

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn testsd() {
        let a = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
            .chars()
            .map(|c| SmolStr::from(c.to_string()))
            .collect::<Vec<SmolStr>>();
        println!("{:?}", word_variants(&a, "FOO"));
        println!("{:?}", word_variants(&a, "Giella"));
        println!("{:?}", word_variants(&a, "abc"));
        println!("{:?}", word_variants(&a, "$GIELLA$"));
    }
}
