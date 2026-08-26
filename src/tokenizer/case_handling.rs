use itertools::Itertools;
use smol_str::SmolStr;

#[inline(always)]
pub fn lower_case(s: &str) -> SmolStr {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        for lc in c.to_lowercase() {
            result.push(lc);
        }
    }
    SmolStr::from(result)
}

#[inline(always)]
pub fn upper_case(s: &str) -> SmolStr {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        for uc in c.to_uppercase() {
            result.push(uc);
        }
    }
    SmolStr::from(result)
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

/// Whether a character is bicameral, i.e. carries case at all. Digits,
/// punctuation and caseless letters are neutral: they are evidence of neither
/// upper nor lower case.
#[inline(always)]
fn is_cased(c: char) -> bool {
    c.is_uppercase() || c.is_lowercase()
}

/// Acronym compounds and inflections deliberately change case after their
/// separator (`ILO-s`, `NSR:a`). Counting that lower-case suffix as a typo
/// would turn the whole suggestion into upper case (`ILO:AS`).
fn is_upper_acronym_with_lower_suffix(value: &str) -> bool {
    let Some(separator) = value.rfind(['-', ':']) else {
        return false;
    };
    let (prefix, suffix) = value.split_at(separator);
    let suffix = &suffix[1..];

    let mut prefix_count = 0;
    for c in prefix.chars().filter(|c| is_cased(*c)) {
        prefix_count += 1;
        if !c.is_uppercase() {
            return false;
        }
    }

    let mut suffix_count = 0;
    for c in suffix.chars().filter(|c| is_cased(*c)) {
        suffix_count += 1;
        if !c.is_lowercase() {
            return false;
        }
    }

    prefix_count >= 2 && suffix_count >= 1
}

#[derive(Debug, Clone, Copy)]
enum WordCase {
    AllUpper,
    /// Every cased character but one is upper case, in a word long enough for
    /// the odd one out to read as a slip: `RÁðI` for `RÁĐI`, where the user
    /// reached for a foreign letter instead of the capital they meant.
    MostlyUpper,
    AllLower,
    Mixed,
    FirstUpper,
    None,
}

impl From<&str> for WordCase {
    #[inline(always)]
    fn from(value: &str) -> Self {
        if is_upper_acronym_with_lower_suffix(value) {
            return WordCase::Mixed;
        }

        let mut chars = value.chars().filter(|c| is_cased(*c));

        let Some(first_char) = chars.next() else {
            return WordCase::None;
        };

        let upper_first_char = first_char.is_uppercase();

        let mut upper = usize::from(upper_first_char);
        let mut lower = usize::from(!upper_first_char);

        for c in chars {
            if c.is_uppercase() {
                upper += 1;
            } else {
                lower += 1;
            }
        }

        if lower == 0 {
            // A lone capital states no pattern: "C" is neither first-caps nor
            // all-caps.
            return if upper >= 2 {
                WordCase::AllUpper
            } else {
                WordCase::None
            };
        }

        if upper == 0 {
            return WordCase::AllLower;
        }

        if !upper_first_char {
            return WordCase::Mixed;
        }

        if upper == 1 {
            return WordCase::FirstUpper;
        }

        // One stray lower-case character among at least four cased ones is a
        // slip in an all-caps word, not deliberate mixed case. The word has to
        // open with a capital for that reading to hold ("cAT" is still mixed),
        // and three-character words are excluded: "AaS" is a stylised
        // abbreviation, not "AAS" mistyped.
        if lower == 1 && upper + lower >= 4 {
            return WordCase::MostlyUpper;
        }

        WordCase::Mixed
    }
}

/// Whether the first bicameral character of the word is upper case, however
/// irregular the rest of the word is.
pub(crate) fn starts_upper_case(word: &str) -> bool {
    word.chars()
        .find(|c| is_cased(*c))
        .is_some_and(char::is_uppercase)
}

pub fn is_mixed_case(word: &str) -> bool {
    matches!(WordCase::from(word), WordCase::Mixed)
}

pub fn is_all_caps(word: &str) -> bool {
    matches!(
        WordCase::from(word),
        WordCase::AllUpper | WordCase::MostlyUpper
    )
}

/// Whether every cased character is lower case, with at least one cased
/// character present. Caseless punctuation and digits are neutral.
pub(crate) fn is_all_lower(word: &str) -> bool {
    matches!(WordCase::from(word), WordCase::AllLower)
}

/// All caps with no allowance for a stray lower-case character.
fn is_strictly_all_caps(word: &str) -> bool {
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
        // Edge case of "sOMETHING": the upper variant would read as all caps,
        // which is the one reading this path must not accept. The test stays
        // strict — a variant that merely reads as mostly upper is still a
        // distinct word worth searching.
        if !is_strictly_all_caps(&upper) {
            words.push(upper);
        }
    }

    CaseHandler {
        original_input: word.into(),
        // Irregular casing still opens with a capital when the user meant it
        // to: "ŦMuitalusat" is a slip in front of "Muitalusat", so corrections
        // reached through the lower-case variants are capitalised rather than
        // handed back bare.
        mutation: if starts_upper_case(word) {
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
    fn all_caps_with_one_stray_lower_case() {
        // "RÁðI" is "RÁĐI" typed with a foreign letter that the user's
        // keyboard only offers in lower case: still an all-caps word.
        assert_eq!(is_all_caps("RÁðI"), true);
        assert_eq!(is_mixed_case("RÁðI"), false);

        let variants = word_variants("RÁðI");
        assert_eq!(variants.mutation, CaseMutation::AllCaps);
        assert_eq!(variants.mode, CaseMode::MergeAll);
        assert!(
            variants.words.iter().any(|w| w == "ráði"),
            "lower-case variant should be searched: {:?}",
            variants.words
        );

        // The allowance is one stray character in a word long enough for it to
        // read as a slip, and only when the word opens with a capital.
        assert_eq!(is_all_caps("Ab"), false);
        assert_eq!(is_all_caps("SGPai"), false);
        assert_eq!(is_mixed_case("SGPai"), true);
        assert_eq!(is_all_caps("cAT"), false);
        assert_eq!(is_mixed_case("cAT"), true);
    }

    #[test]
    fn acronym_suffix_is_deliberate_mixed_case() {
        for word in ["ILO-s", "NSR:a", "ABC-def"] {
            assert!(is_mixed_case(word), "{word}");
            let variants = word_variants(word);
            assert_eq!(variants.mutation, CaseMutation::FirstCaps, "{word}");
            assert_eq!(variants.mode, CaseMode::FirstResults, "{word}");
        }

        assert!(is_all_caps("RÁðI"));
        assert!(is_all_caps("ABC-DEF"));
    }

    #[test]
    fn caseless_characters_are_neutral() {
        // Only bicameral characters carry case; digits, punctuation and
        // caseless letters state nothing about the word's case.
        assert_eq!(is_all_caps("ABC-DEF"), true);
        assert_eq!(is_all_caps("日ABC"), true);
        assert_eq!(is_first_caps("日Abc"), true);
        assert!(is_all_lower("日abc-123"));
        assert!(!is_all_lower("日Abc-123"));
        assert!(!is_all_lower("日-123"));
        assert_eq!(is_all_caps("123"), false);
        assert_eq!(is_first_caps("123"), false);
    }

    #[test]
    fn irregular_case_keeps_the_opening_capital() {
        // "ŦMuitalusat" is "Muitalusat" with a stray capital in front: the
        // correction has to come back capitalised, not bare.
        let variants = word_variants("ŦMuitalusat");
        assert_eq!(variants.mode, CaseMode::FirstResults);
        assert_eq!(variants.mutation, CaseMutation::FirstCaps);

        assert_eq!(word_variants("EOvddidat").mutation, CaseMutation::FirstCaps);

        // An input that opens in lower case keeps its own casing.
        assert_eq!(word_variants("cAt").mutation, CaseMutation::None);
        assert_eq!(word_variants("iPhone").mutation, CaseMutation::None);
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
