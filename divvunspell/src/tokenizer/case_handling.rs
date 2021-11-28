use itertools::Itertools;
use smol_str::SmolStr;

#[inline(always)]
pub fn lower_case(s: &str) -> SmolStr {
    s.chars()
        .map(|c| c.to_lowercase().collect::<String>())
        .collect::<SmolStr>()
}

#[inline(always)]
pub fn upper_case(s: &str) -> SmolStr {
    s.chars()
        .map(|c| c.to_uppercase().collect::<String>())
        .collect::<SmolStr>()
}

#[inline(always)]
pub fn upper_first(s: &str) -> SmolStr {
    let mut c = s.chars();
    match c.next() {
        None => SmolStr::new(""),
        Some(f) => SmolStr::from(f.to_uppercase().collect::<String>() + c.as_str()),
    }
}

#[inline(always)]
pub fn lower_first(s: &str) -> SmolStr {
    let mut c = s.chars();
    match c.next() {
        None => SmolStr::new(""),
        Some(f) => SmolStr::from(f.to_lowercase().collect::<String>() + c.as_str()),
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Case {
    Upper,
    Lower,
    Neither,
}

impl Case {
    #[inline(always)]
    fn new(ch: char) -> Case {
        if ch.is_lowercase() {
            Case::Lower
        } else if ch.is_uppercase() {
            Case::Upper
        } else {
            Case::Neither
        }
    }
}

pub fn is_mixed_case(word: &str) -> bool {
    let mut chars = word.chars();
    let mut last_case = match chars.next() {
        Some(ch) => Case::new(ch),
        None => return false,
    };

    if last_case == Case::Neither {
        return false;
    }

    let mut case_changes = 0;

    for ch in chars {
        let next_case = Case::new(ch);

        match (last_case, next_case) {
            (_, Case::Neither) => return false,
            (_, Case::Upper) => case_changes += 2,
            (Case::Upper, Case::Lower) => case_changes += 1,
            _ => {}
        }

        last_case = next_case;
    }

    case_changes > 1
}

pub fn is_all_caps(word: &str) -> bool {
    upper_case(word) == word
}

pub fn is_first_caps(word: &str) -> bool {
    upper_first(word) == word
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseMutation {
    FirstCaps,
    AllCaps,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseMode {
    FirstResults,
    MergeAll,
}

#[derive(Debug, Clone)]
pub struct CaseHandler {
    pub original_input: SmolStr,
    pub mutation: CaseMutation,
    pub mode: CaseMode,
    pub words: Vec<SmolStr>,
}

fn mixed_case_word_variants(word: &str) -> CaseHandler {
    // The input string should be accepted IFF it is accepted exactly as given,
    // or with the initial letter downcased, or all upper.
    //
    // Crucially, it should not be accepted if it is only accepted when all lowercased.

    let mut words = vec![];
    if is_first_caps(word) {
        words.push(lower_first(word));
    } else {
        let upper = upper_first(word);
        // Edge case of "sOMETHING"
        if !is_all_caps(&upper) {
            words.push(upper);
        }
    }

    CaseHandler {
        original_input: word.into(),
        mutation: if is_first_caps(word) {
            CaseMutation::FirstCaps
        } else {
            CaseMutation::None
        },
        mode: CaseMode::FirstResults,
        words,
    }
}

pub fn word_variants(word: &str) -> CaseHandler {
    if is_mixed_case(word) {
        return mixed_case_word_variants(word);
    }

    let word = SmolStr::new(word);
    let mut base: Vec<SmolStr> = vec![];

    base.append(
        &mut std::iter::once(&word)
            .chain(base.iter())
            .filter(|x| is_all_caps(x))
            .map(|x| upper_first(&lower_case(x)))
            .collect(),
    );

    base.append(
        &mut std::iter::once(&word)
            .chain(base.iter())
            .map(|x| lower_case(x))
            .collect(),
    );

    let words = base.into_iter().unique().collect();

    let (mutation, mode) = if is_all_caps(&word) {
        (CaseMutation::AllCaps, CaseMode::MergeAll)
    } else if is_first_caps(&word) {
        (CaseMutation::FirstCaps, CaseMode::MergeAll)
    } else {
        (CaseMutation::None, CaseMode::MergeAll)
    };

    CaseHandler {
        original_input: word.into(),
        mode,
        mutation,
        words,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
        let a = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
            .chars()
            .map(|c| SmolStr::from(c.to_string()))
            .collect::<Vec<SmolStr>>();
        // println!("{:?}", word_variants(&a, "FOO"));
        // println!("{:?}", word_variants(&a, "Giella"));
        // println!("{:?}", word_variants(&a, "abc"));
        // println!("{:?}", word_variants(&a, "$GIELLA$"));
    }

    #[test]
    fn variants() {
        assert_eq!(word_variants("IDENTITETE").mutation, CaseMutation::AllCaps);
        assert_eq!(
            word_variants("Identitete").mutation,
            CaseMutation::FirstCaps
        );
    }

    #[test]
    fn mixed_case() {
        assert_eq!(is_mixed_case("McDonald"), true);
        assert_eq!(is_mixed_case("Mcdonald"), false);
        assert_eq!(is_mixed_case("McDoNaLd"), true);
        assert_eq!(is_mixed_case("MCDONALD"), false);
        assert_eq!(is_mixed_case("mcDonald"), true);
        assert_eq!(is_mixed_case("mcdonald"), false);

        assert_eq!(is_mixed_case("ab"), false);
        assert_eq!(is_mixed_case("aB"), true);
        assert_eq!(is_mixed_case("Ab"), false);
        assert_eq!(is_mixed_case("AB"), false);

        assert_eq!(is_mixed_case("A"), false);
        assert_eq!(is_mixed_case("a"), false);
        assert_eq!(is_mixed_case("aS:"), false);
        assert_eq!(is_mixed_case(":"), false);

        assert_eq!(is_mixed_case("DavveVássján"), true);
        assert_eq!(is_mixed_case("davveVássján"), true);
        assert_eq!(is_mixed_case("Davvevássján"), false);

        assert_eq!(is_mixed_case("SGPai"), false);
        assert_eq!(is_mixed_case("SgPaI"), true);
        assert_eq!(is_mixed_case("SGPaiSGP"), true);
        assert_eq!(is_mixed_case("sgpAI"), true);
    }
}
