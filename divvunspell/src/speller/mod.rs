use std::f32;
use std::sync::Arc;

use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use unic_ucd_category::GeneralCategory;

use self::worker::SpellerWorker;
use crate::speller::suggestion::Suggestion;
use crate::tokenizer::case_handling::CaseHandler;
use crate::transducer::Transducer;
use crate::types::{SymbolNumber, Weight};

pub mod suggestion;
mod worker;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CaseHandlingConfig {
    start_penalty: f32,
    end_penalty: f32,
    mid_penalty: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpellerConfig {
    pub n_best: Option<usize>,
    pub max_weight: Option<Weight>,
    pub beam: Option<Weight>,
    pub case_handling: Option<CaseHandlingConfig>,
    pub node_pool_size: usize,
}

impl SpellerConfig {
    pub const fn default() -> SpellerConfig {
        SpellerConfig {
            n_best: Some(10),
            max_weight: Some(10000.0),
            beam: None,
            case_handling: Some(CaseHandlingConfig::default()),
            node_pool_size: 128,
        }
    }
}

impl CaseHandlingConfig {
    pub const fn default() -> CaseHandlingConfig {
        CaseHandlingConfig {
            start_penalty: 10.0,
            end_penalty: 10.0,
            mid_penalty: 5.0,
        }
    }
}

pub trait Speller {
    fn is_correct(self: Arc<Self>, word: &str) -> bool;
    fn is_correct_with_config(self: Arc<Self>, word: &str, config: &SpellerConfig) -> bool;
    fn suggest(self: Arc<Self>, word: &str) -> Vec<Suggestion>;
    fn suggest_with_config(self: Arc<Self>, word: &str, config: &SpellerConfig) -> Vec<Suggestion>;
}

impl<F, T, U> Speller for HfstSpeller<F, T, U>
where
    F: crate::vfs::File + Send,
    T: Transducer<F> + Send,
    U: Transducer<F> + Send,
{
    #[allow(clippy::wrong_self_convention)]
    fn is_correct_with_config(self: Arc<Self>, word: &str, config: &SpellerConfig) -> bool {
        use crate::tokenizer::case_handling::*;

        if word.len() == 0 {
            return true;
        }

        // Check if there are zero letters in the word according to
        // Unicode letter category
        if word.chars().all(|c| !GeneralCategory::of(c).is_letter()) {
            return true;
        }

        let words = if config.case_handling.is_some() {
            let variants = word_variants(word);
            variants.words
        } else {
            vec![]
        };

        for word in std::iter::once(word.into()).chain(words.into_iter()) {
            let worker = SpellerWorker::new(self.clone(), self.to_input_vec(&word), config.clone());

            if worker.is_correct() {
                return true;
            }
        }

        false
    }

    #[inline]
    fn is_correct(self: Arc<Self>, word: &str) -> bool {
        self.is_correct_with_config(word, &SpellerConfig::default())
    }

    #[inline]
    fn suggest(self: Arc<Self>, word: &str) -> Vec<Suggestion> {
        self.suggest_with_config(word, &SpellerConfig::default())
    }

    fn suggest_with_config(self: Arc<Self>, word: &str, config: &SpellerConfig) -> Vec<Suggestion> {
        use crate::tokenizer::case_handling::*;

        if word.len() == 0 {
            return vec![];
        }

        if let Some(case_handling) = config.case_handling.as_ref() {
            let case_handler = word_variants(word);

            self.suggest_case(case_handler, config, case_handling)
        } else {
            self.suggest_single(word, config)
        }
    }
}

#[derive(Debug)]
pub struct HfstSpeller<F, T, U>
where
    F: crate::vfs::File,
    T: Transducer<F>,
    U: Transducer<F>,
{
    mutator: T,
    lexicon: U,
    alphabet_translator: Vec<SymbolNumber>,
    _file: std::marker::PhantomData<F>,
}

impl<F, T, U> HfstSpeller<F, T, U>
where
    F: crate::vfs::File,
    T: Transducer<F>,
    U: Transducer<F>,
{
    pub fn new(mutator: T, mut lexicon: U) -> Arc<HfstSpeller<F, T, U>> {
        let alphabet_translator = lexicon.mut_alphabet().create_translator_from(&mutator);

        Arc::new(HfstSpeller {
            mutator,
            lexicon,
            alphabet_translator,
            _file: std::marker::PhantomData::<F>,
        })
    }

    pub fn mutator(&self) -> &T {
        &self.mutator
    }

    pub fn lexicon(&self) -> &U {
        &self.lexicon
    }

    fn alphabet_translator(&self) -> &Vec<SymbolNumber> {
        &self.alphabet_translator
    }

    fn to_input_vec(&self, word: &str) -> Vec<SymbolNumber> {
        let alphabet = self.mutator().alphabet();
        let key_table = alphabet.key_table();

        word.chars()
            .map(|ch| {
                let s = ch.to_string();
                key_table
                    .iter()
                    .position(|x| x == &s)
                    .map(|x| x as u16)
                    .unwrap_or_else(|| alphabet.unknown().unwrap_or(0u16))
            })
            .collect()
    }

    fn suggest_single(self: Arc<Self>, word: &str, config: &SpellerConfig) -> Vec<Suggestion> {
        let worker = SpellerWorker::new(self.clone(), self.to_input_vec(word), config.clone());

        worker.suggest()
    }

    fn suggest_case(
        self: Arc<Self>,
        case: CaseHandler,
        config: &SpellerConfig,
        case_handling: &CaseHandlingConfig,
    ) -> Vec<Suggestion> {
        use crate::tokenizer::case_handling::*;

        let CaseHandler {
            original_input,
            mutation,
            mode,
            words,
        } = case;
        let mut best: HashMap<SmolStr, f32> = HashMap::new();

        for word in std::iter::once(&original_input).chain(words.iter()) {
            let worker = SpellerWorker::new(self.clone(), self.to_input_vec(&word), config.clone());
            let mut suggestions = worker.suggest();

            match mutation {
                CaseMutation::FirstCaps => {
                    suggestions.iter_mut().for_each(|x| {
                        x.value = upper_first(x.value());
                    });
                }
                CaseMutation::AllCaps => {
                    suggestions.iter_mut().for_each(|x| {
                        x.value = upper_case(x.value());
                    });
                }
                _ => {}
            }

            match mode {
                CaseMode::MergeAll => {
                    for sugg in suggestions.into_iter() {
                        let penalty_start =
                            if !sugg.value().starts_with(word.chars().next().unwrap()) {
                                case_handling.start_penalty
                            } else {
                                0.0
                            };
                        let penalty_end =
                            if !sugg.value().ends_with(word.chars().rev().next().unwrap()) {
                                case_handling.end_penalty
                            } else {
                                0.0
                            };

                        let distance =
                            strsim::damerau_levenshtein(&words[0].as_str(), &word.as_str())
                                + strsim::damerau_levenshtein(&word.as_str(), sugg.value());
                        let penalty_middle = case_handling.mid_penalty * distance as f32;
                        let additional_weight = penalty_start + penalty_end + penalty_middle;

                        best.entry(sugg.value.clone())
                            .and_modify(|entry| {
                                let weight = sugg.weight + additional_weight;
                                if entry as &_ > &weight {
                                    *entry = weight
                                }
                            })
                            .or_insert(sugg.weight + additional_weight);
                    }
                }
                CaseMode::FirstResults => {
                    if !suggestions.is_empty() {
                        return suggestions;
                    }
                }
            }
        }

        if best.is_empty() {
            return vec![];
        }

        let mut out = best
            .into_iter()
            .map(|(k, v)| Suggestion {
                value: k,
                weight: v,
            })
            .collect::<Vec<_>>();
        out.sort();
        if let Some(n_best) = config.n_best {
            out.truncate(n_best);
        }
        out
    }
}

#[cfg(feature = "internal_ffi")]
pub(crate) mod ffi {
    use super::*;
    use cffi::{FromForeign, ToForeign};
    use std::convert::Infallible;
    use std::ffi::c_void;

    pub type SuggestionVecMarshaler = cffi::VecMarshaler<Suggestion>;
    pub type SuggestionVecRefMarshaler = cffi::VecRefMarshaler<Suggestion>;

    #[derive(Clone, Copy, Default, PartialEq)]
    #[repr(C)]
    pub struct FfiCaseHandlingConfig {
        start_penalty: f32,
        end_penalty: f32,
        mid_penalty: f32,
    }

    #[derive(Clone, Copy)]
    #[repr(C)]
    pub struct FfiSpellerConfig {
        pub n_best: usize,
        pub max_weight: Weight,
        pub beam: Weight,
        pub case_handling: FfiCaseHandlingConfig,
        pub node_pool_size: usize,
    }

    pub struct SpellerConfigMarshaler;

    impl cffi::InputType for SpellerConfigMarshaler {
        type Foreign = *const c_void;
        type ForeignTraitObject = ();
    }

    impl cffi::ReturnType for SpellerConfigMarshaler {
        type Foreign = *const c_void;
        type ForeignTraitObject = ();

        fn foreign_default() -> Self::Foreign {
            std::ptr::null()
        }
    }

    impl ToForeign<SpellerConfig, *const c_void> for SpellerConfigMarshaler {
        type Error = Infallible;

        fn to_foreign(config: SpellerConfig) -> Result<*const c_void, Self::Error> {
            let case_handling = config
                .case_handling
                .map(|c| FfiCaseHandlingConfig {
                    start_penalty: c.start_penalty,
                    end_penalty: c.end_penalty,
                    mid_penalty: c.mid_penalty,
                })
                .unwrap_or_else(|| FfiCaseHandlingConfig::default());

            let out = FfiSpellerConfig {
                n_best: config.n_best.unwrap_or(0),
                max_weight: config.max_weight.unwrap_or(0.0),
                beam: config.beam.unwrap_or(0.0),
                case_handling,
                node_pool_size: config.node_pool_size,
            };

            Ok(Box::into_raw(Box::new(out)) as *const _)
        }
    }

    impl FromForeign<*const c_void, SpellerConfig> for SpellerConfigMarshaler {
        type Error = Infallible;

        unsafe fn from_foreign(ptr: *const c_void) -> Result<SpellerConfig, Self::Error> {
            if ptr.is_null() {
                return Ok(SpellerConfig::default());
            }

            let config: &FfiSpellerConfig = &*ptr.cast();

            let case_handling = if config.case_handling == FfiCaseHandlingConfig::default() {
                None
            } else {
                let c = config.case_handling;
                Some(CaseHandlingConfig {
                    start_penalty: c.start_penalty,
                    end_penalty: c.end_penalty,
                    mid_penalty: c.mid_penalty,
                })
            };

            let out = SpellerConfig {
                n_best: if config.n_best > 0 {
                    Some(config.n_best)
                } else {
                    None
                },
                max_weight: if config.max_weight > 0.0 {
                    Some(config.max_weight)
                } else {
                    None
                },
                beam: if config.beam > 0.0 {
                    Some(config.beam)
                } else {
                    None
                },
                case_handling,
                node_pool_size: config.node_pool_size,
            };

            Ok(out)
        }
    }

    #[cffi::marshal]
    pub extern "C" fn divvun_speller_is_correct(
        #[marshal(cffi::ArcRefMarshaler::<dyn Speller + Sync + Send>)] speller: Arc<
            dyn Speller + Sync + Send,
        >,
        #[marshal(cffi::StrMarshaler)] word: &str,
    ) -> bool {
        speller.is_correct(word)
    }

    #[cffi::marshal(return_marshaler = "SuggestionVecMarshaler")]
    pub extern "C" fn divvun_speller_suggest(
        #[marshal(cffi::ArcRefMarshaler::<dyn Speller + Sync + Send>)] speller: Arc<
            dyn Speller + Sync + Send,
        >,
        #[marshal(cffi::StrMarshaler)] word: &str,
    ) -> Vec<Suggestion> {
        speller.suggest(word)
    }

    #[cffi::marshal(return_marshaler = "SuggestionVecMarshaler")]
    pub extern "C" fn divvun_speller_suggest_with_config(
        #[marshal(cffi::ArcRefMarshaler::<dyn Speller + Sync + Send>)] speller: Arc<
            dyn Speller + Sync + Send,
        >,
        #[marshal(cffi::StrMarshaler)] word: &str,
        #[marshal(SpellerConfigMarshaler)] config: SpellerConfig,
    ) -> Vec<Suggestion> {
        speller.suggest_with_config(word, &config)
    }

    // Suggestions vec

    #[cffi::marshal]
    pub extern "C" fn divvun_vec_suggestion_len(
        #[marshal(SuggestionVecRefMarshaler)] suggestions: &[Suggestion],
    ) -> usize {
        suggestions.len()
    }

    #[cffi::marshal(return_marshaler = "cffi::StringMarshaler")]
    pub extern "C" fn divvun_vec_suggestion_get_value(
        #[marshal(SuggestionVecRefMarshaler)] suggestions: &[Suggestion],
        index: usize,
    ) -> String {
        suggestions[index].value().to_string()
    }
}
