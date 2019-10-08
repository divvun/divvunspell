pub mod suggestion;
pub mod worker;

use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::f32;
use std::sync::Arc;

use self::worker::SpellerWorker;
use crate::speller::suggestion::Suggestion;
use crate::tokenizer::case_handling::CaseHandler;
use crate::transducer::Transducer;
use crate::types::{SymbolNumber, Weight};

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
    pub pool_start: usize,
    pub pool_max: usize,
    pub seen_node_sample_rate: u64,
}

impl SpellerConfig {
    pub const fn default() -> SpellerConfig {
        SpellerConfig {
            n_best: Some(10),
            max_weight: Some(10000.0),
            beam: None,
            case_handling: Some(CaseHandlingConfig::default()),
            pool_start: 128,
            pool_max: 128,
            seen_node_sample_rate: 20,
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

#[derive(Debug)]
pub struct Speller<F, T: Transducer<F>, U: Transducer<F>>
where
    F: crate::vfs::File + crate::vfs::ToMemmap,
{
    mutator: T,
    lexicon: U,
    alphabet_translator: Vec<SymbolNumber>,
    _file: std::marker::PhantomData<F>,
}

impl<F, T, U> Speller<F, T, U>
where
    F: crate::vfs::File + crate::vfs::ToMemmap,
    T: Transducer<F>,
    U: Transducer<F>,
{
    pub fn new(mutator: T, mut lexicon: U) -> Arc<Speller<F, T, U>> {
        let alphabet_translator = lexicon.mut_alphabet().create_translator_from(&mutator);

        Arc::new(Speller {
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

    #[allow(clippy::wrong_self_convention)]
    pub fn is_correct(self: Arc<Self>, word: &str) -> bool {
        use crate::tokenizer::case_handling::*;

        let words = word_variants(word).words;

        for word in words.into_iter() {
            let worker = SpellerWorker::new(
                self.clone(),
                self.to_input_vec(&word),
                SpellerConfig::default(),
            );

            if worker.is_correct() {
                return true;
            }
        }

        false
    }

    pub fn suggest(self: Arc<Self>, word: &str) -> Vec<Suggestion> {
        self.suggest_with_config(word, &SpellerConfig::default())
    }

    pub fn suggest_with_config(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
    ) -> Vec<Suggestion> {
        use crate::tokenizer::case_handling::*;

        if let Some(case_handling) = config.case_handling.as_ref() {
            let case_handler = word_variants(word);

            self.suggest_case(case_handler, config, case_handling)
        } else {
            self.suggest_single(word, config)
        }
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
        use crate::tokenizer::case_handling::{CaseMode, CaseMutation};

        let CaseHandler {
            mutation,
            mode,
            words,
        } = case;
        let mut best: HashMap<SmolStr, f32> = HashMap::new();

        for word in words.iter() {
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

#[cfg(feature = "ffi")]
pub(crate) mod ffi {
    use super::*;
    use std::ffi::c_void;
    use std::convert::Infallible;
    use cursed::{ToForeign, FromForeign};

    pub type SuggestionVecMarshaler = cursed::VecMarshaler<Suggestion>;
    pub type SuggestionVecRefMarshaler = cursed::VecRefMarshaler<Suggestion>;

    #[derive(Clone, Copy)]
    #[repr(C)]
    pub struct FfiCaseHandlingConfig {
        start_penalty: f32,
        end_penalty: f32,
        mid_penalty: f32,
    }

    #[derive(Clone, Copy)]
    #[repr(C)]
    pub struct FfiSpellerConfig {
        pub n_best: Option<usize>,
        pub max_weight: Option<Weight>,
        pub beam: Option<Weight>,
        pub case_handling: Option<FfiCaseHandlingConfig>,
        pub pool_start: usize,
        pub pool_max: usize,
        pub seen_node_sample_rate: u64,
    }

    pub struct SpellerConfigMarshaler;

    impl cursed::InputType for SpellerConfigMarshaler {
        type Foreign = *const c_void;
    }

    impl cursed::ReturnType for SpellerConfigMarshaler {
        type Foreign = *const c_void;

        fn foreign_default() -> Self::Foreign { std::ptr::null() }
    }

    impl ToForeign<SpellerConfig, *const c_void> for SpellerConfigMarshaler {
        type Error = Infallible;

        fn to_foreign(config: SpellerConfig) -> Result<*const c_void, Self::Error> {
            let out = FfiSpellerConfig {
                n_best: config.n_best,
                max_weight: config.max_weight,
                beam: config.beam,
                case_handling: config.case_handling.map(|c| {
                    FfiCaseHandlingConfig {
                        start_penalty: c.start_penalty,
                        end_penalty: c.end_penalty,
                        mid_penalty: c.mid_penalty
                    }
                }),
                pool_start: config.pool_start,
                pool_max: config.pool_max,
                seen_node_sample_rate: config.seen_node_sample_rate,
            };

            Ok(Box::into_raw(Box::new(out)) as *const _)
        }
    }

    impl FromForeign<*const c_void, SpellerConfig> for SpellerConfigMarshaler {
        type Error = Infallible;

        fn from_foreign(ptr: *const c_void) -> Result<SpellerConfig, Self::Error> {
            if ptr.is_null() {
                return Ok(SpellerConfig::default());
            }

            let config: &FfiSpellerConfig = unsafe { &*ptr.cast() };
            
            let out = SpellerConfig {
                n_best: config.n_best,
                max_weight: config.max_weight,
                beam: config.beam,
                case_handling: config.case_handling.map(|c| {
                    CaseHandlingConfig {
                        start_penalty: c.start_penalty,
                        end_penalty: c.end_penalty,
                        mid_penalty: c.mid_penalty
                    }
                }),
                pool_start: config.pool_start,
                pool_max: config.pool_max,
                seen_node_sample_rate: config.seen_node_sample_rate,
            };

            Ok(out)
        }
    }

    use crate::archive::boxf::ffi::ThfstBoxSpeller;
    
    #[cthulhu::invoke]
    pub extern fn divvun_thfst_box_speller_is_correct(
        #[marshal(cursed::ArcMarshaler)] speller: Arc<ThfstBoxSpeller>,
        #[marshal(cursed::StrMarshaler)] word: &str
    ) -> bool {
        speller.is_correct(word)
    }

    #[cthulhu::invoke(return_marshaler = "SuggestionVecMarshaler")]
    pub extern fn divvun_thfst_box_speller_suggest(
        #[marshal(cursed::ArcMarshaler)] speller: Arc<ThfstBoxSpeller>,
        #[marshal(cursed::StrMarshaler)] word: &str
    ) -> Vec<Suggestion> {
        speller.suggest(word)
    }

    #[cthulhu::invoke(return_marshaler = "SuggestionVecMarshaler")]
    pub extern fn divvun_thfst_box_speller_suggest_with_config(
        #[marshal(cursed::ArcMarshaler)] speller: Arc<ThfstBoxSpeller>,
        #[marshal(cursed::StrMarshaler)] word: &str,
        #[marshal(SpellerConfigMarshaler)] config: SpellerConfig
    ) -> Vec<Suggestion> {
        speller.suggest_with_config(word, &config)
    }

    use crate::archive::boxf::ffi::ThfstChunkedBoxSpeller;

    #[cthulhu::invoke]
    pub extern fn divvun_thfst_chunked_box_speller_is_correct(
        #[marshal(cursed::ArcMarshaler)] speller: Arc<ThfstChunkedBoxSpeller>,
        #[marshal(cursed::StrMarshaler)] word: &str
    ) -> bool {
        speller.is_correct(word)
    }

    #[cthulhu::invoke(return_marshaler = "SuggestionVecMarshaler")]
    pub extern fn divvun_thfst_chunked_box_speller_suggest(
        #[marshal(cursed::ArcMarshaler)] speller: Arc<ThfstChunkedBoxSpeller>,
        #[marshal(cursed::StrMarshaler)] word: &str
    ) -> Vec<Suggestion> {
        speller.suggest(word)
    }

    #[cthulhu::invoke(return_marshaler = "SuggestionVecMarshaler")]
    pub extern fn divvun_thfst_chunked_box_speller_suggest_with_config(
        #[marshal(cursed::ArcMarshaler)] speller: Arc<ThfstChunkedBoxSpeller>,
        #[marshal(cursed::StrMarshaler)] word: &str,
        #[marshal(SpellerConfigMarshaler)] config: SpellerConfig
    ) -> Vec<Suggestion> {
        speller.suggest_with_config(word, &config)
    }

    // Suggestions vec

    #[cthulhu::invoke]
    pub extern fn divvun_vec_suggestion_len(
        #[marshal(SuggestionVecRefMarshaler)]
        suggestions: &[Suggestion]
    ) -> usize {
        suggestions.len()
    }

    #[cthulhu::invoke(return_marshaler = "cursed::StrMarshaler")]
    pub extern fn divvun_vec_suggestion_get_value(
        #[marshal(SuggestionVecRefMarshaler)]
        suggestions: &[Suggestion],
        index: usize
    ) -> &str {
        suggestions[index].value()
    }

    // #[no_mangle]
    // pub extern "C" fn divvun_vec_suggestion_get_value(
    //     suggestions: <SuggestionVecRefMarshaler as ::cursed::InputType>::Foreign,//*const ::std::ffi::c_void,
    //     index: usize,
    //     __exception: ::cursed::ErrCallback,
    //     __return: ::cursed::RetCallback<<cursed::StrMarshaler as ::cursed::ReturnType>::Foreign>,
    // ) {
    //     let suggestions: &[Suggestion] =
    //         match SuggestionVecRefMarshaler::from_foreign(suggestions) {
    //             Ok(v) => v,
    //             Err(e) => {
    //                 if let Some(callback) = __exception {
    //                     let s = std::ffi::CString::new("<unknown>".to_string()).unwrap();
    //                     callback(s.as_ptr().cast());
    //                 }
    //                 return;
    //             }
    //         };
    //     #[inline(always)]
    //     fn divvun_vec_suggestion_get_value(suggestions: &[Suggestion], index: usize) -> &str {
    //         suggestions[index].value()
    //     }
    //     let result = divvun_vec_suggestion_get_value(suggestions, index);
    //     if let Some(__return) = __return {
    //         match cursed::StrMarshaler::to_foreign(result) {
    //             Ok(v) => __return(v),
    //             Err(e) => {
    //                 if let Some(callback) = __exception {
    //                     let s = std::ffi::CString::new("<unknown>".to_string()).unwrap();
    //                     callback(s.as_ptr().cast());
    //                 }
    //             }
    //         }
    //     }
    // }
}
