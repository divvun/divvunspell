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
    let mut result = String::with_capacity(s.len());
    let mut done = false;
    for c in s.chars() {
        if !done && c.is_alphabetic() {
            result.extend(c.to_uppercase());
            done = true;
        } else {
            result.push(c);
        }
    }
    SmolStr::from(result)
}

#[inline(always)]
pub fn lower_first(s: &str) -> SmolStr {
    let mut result = String::with_capacity(s.len());
    let mut done = false;
    for c in s.chars() {
        if !done && c.is_alphabetic() {
            result.extend(c.to_lowercase());
            done = true;
        } else {
            result.push(c);
        }
    }
    SmolStr::from(result)
}

#[derive(Debug, Clone, Copy)]
enum WordCase {
    AllUpper,
    AllLower,
    Mixed,
    FirstUpper,
    None,
}

impl From<&str> for WordCase {
    #[inline(always)]
    fn from(value: &str) -> Self {
        let mut chars = value.chars().filter(|c| c.is_alphabetic());

        let Some(first_char) = chars.next() else {
            return WordCase::None;
        };

        let upper_first_char = first_char.is_uppercase();

        let mut has_upper = false;
        let mut has_lower = !upper_first_char;

        for c in chars {
            if c.is_uppercase() {
                has_upper = true;
            } else if c.is_lowercase() {
                has_lower = true;
            }
        }

        match (upper_first_char, has_upper, has_lower) {
            (true, true, false) => WordCase::AllUpper,
            (false, false, true) => WordCase::AllLower,
            (_, true, true) => WordCase::Mixed,
            (true, false, true) => WordCase::FirstUpper,
            _ => WordCase::None,
        }
    }
}

pub fn is_mixed_case(word: &str) -> bool {
    matches!(WordCase::from(word), WordCase::Mixed)
}

pub fn is_all_caps(word: &str) -> bool {
    matches!(WordCase::from(word), WordCase::AllUpper)
}

pub fn is_first_caps(word: &str) -> bool {
    matches!(WordCase::from(word), WordCase::FirstUpper)
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
        let _a = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
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
    fn digit_prefixed_case() {
        // Leading digits should not trick is_first_caps
        assert_eq!(is_first_caps("1heavvanit"), false);
        assert_eq!(is_first_caps("1riikkačaohkkima"), false);
        assert_eq!(is_first_caps("1Heavvanit"), true);
        assert_eq!(is_first_caps("123"), false);

        // Leading digits should not trick is_all_caps
        assert_eq!(is_all_caps("123"), false);
        assert_eq!(is_all_caps("1HELLO"), true);
        assert_eq!(is_all_caps("1hello"), false);

        // word_variants should produce CaseMutation::None for digit-prefixed lowercase
        assert_eq!(word_variants("1heavvanit").mutation, CaseMutation::None);
        assert_eq!(
            word_variants("1Heavvanit").mutation,
            CaseMutation::FirstCaps
        );
        assert_eq!(word_variants("1HEAVVANIT").mutation, CaseMutation::AllCaps);
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
        assert_eq!(is_mixed_case("aS:"), true);
        assert_eq!(is_mixed_case(":"), false);

        assert_eq!(is_mixed_case("DavveVássján"), true);
        assert_eq!(is_mixed_case("davveVássján"), true);
        assert_eq!(is_mixed_case("Davvevássján"), false);

        assert_eq!(is_mixed_case("SGPai"), true);
        assert_eq!(is_mixed_case("SgPaI"), true);
        assert_eq!(is_mixed_case("SGPaiSGP"), true);
        assert_eq!(is_mixed_case("sgpAI"), true);
        assert_eq!(is_mixed_case("SGPAI"), false);
        assert_eq!(is_mixed_case("Sgpai"), false);
    }
}
