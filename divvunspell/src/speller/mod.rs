//! Speller model for spell-checking and corrections.
use std::f32;
use std::sync::Arc;

use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use unic_emoji_char::is_emoji;
use unic_segment::Graphemes;
use unic_ucd_category::GeneralCategory;

use self::worker::SpellerWorker;
use crate::speller::suggestion::Suggestion;
use crate::tokenizer::case_handling::CaseHandler;
use crate::transducer::Transducer;
use crate::types::{SymbolNumber, Weight};

pub mod suggestion;
mod worker;

/// configurable extra penalties for edit distance
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ReweightingConfig {
    #[serde(default = "default_start_penalty")]
    start_penalty: f32,
    #[serde(default = "default_end_penalty")]
    end_penalty: f32,
    #[serde(default = "default_mid_penalty")]
    mid_penalty: f32,
}

impl Default for ReweightingConfig {
    fn default() -> Self {
        Self::default_const()
    }
}

impl ReweightingConfig {
    pub const fn default_const() -> Self {
        Self {
            start_penalty: 10.0,
            end_penalty: 10.0,
            mid_penalty: 5.0,
        }
    }
}

const fn default_start_penalty() -> f32 {
    10.0
}

const fn default_end_penalty() -> f32 {
    10.0
}

const fn default_mid_penalty() -> f32 {
    5.0
}

/// finetuning configuration of the spelling correction algorithms
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SpellerConfig {
    /// upper limit for suggestions given
    #[serde(default = "default_n_best")]
    pub n_best: Option<usize>,
    /// upper limit for weight of any suggestion
    #[serde(default = "default_max_weight")]
    pub max_weight: Option<Weight>,
    /// weight distance between best suggestion and worst
    #[serde(default = "default_beam")]
    pub beam: Option<Weight>,
    /// extra penalties for different edit distance type errors
    #[serde(default = "default_reweight")]
    pub reweight: Option<ReweightingConfig>,
    /// some parallel stuff?
    #[serde(default = "default_node_pool_size")]
    pub node_pool_size: usize,
    /// used when suggesting unfinished word parts
    pub continuation_marker: Option<String>,
    /// whether we try to recase mispelt word before other suggestions
    #[serde(default = "default_recase")]
    pub recase: bool,
}

impl SpellerConfig {
    /// create a default configuration with following values:
    /// * n_best = 10
    /// * max_weight = 10000
    /// * beam = None
    /// * reweight = default (c.f. ReweightingConfig::default())
    /// * node_pool_size = 128
    /// * recase = true
    pub const fn default() -> SpellerConfig {
        SpellerConfig {
            n_best: default_n_best(),
            max_weight: default_max_weight(),
            beam: default_beam(),
            reweight: default_reweight(),
            node_pool_size: default_node_pool_size(),
            continuation_marker: None,
            recase: default_recase(),
        }
    }
}

const fn default_n_best() -> Option<usize> {
    Some(10)
}

const fn default_max_weight() -> Option<Weight> {
    Some(10000.0)
}

const fn default_beam() -> Option<Weight> {
    None
}

const fn default_reweight() -> Option<ReweightingConfig> {
    Some(ReweightingConfig::default_const())
}

const fn default_node_pool_size() -> usize {
    128
}

const fn default_recase() -> bool {
    true
}
/// can determine if string is a correct word or suggest corrections.
/// Also with SpellerConfig.
pub trait Speller {
    /// check if the word is correctly spelled
    fn is_correct(self: Arc<Self>, word: &str) -> bool;
    /// check if word is correctly spelled with config recasing etc.
    fn is_correct_with_config(self: Arc<Self>, word: &str, config: &SpellerConfig) -> bool;
    /// suggest corrections to word
    fn suggest(self: Arc<Self>, word: &str) -> Vec<Suggestion>;
    /// suggest corrections with recasing and reweighting from config
    fn suggest_with_config(self: Arc<Self>, word: &str, config: &SpellerConfig) -> Vec<Suggestion>;
}

/// can provide in-depth analyses along with suggestions
pub trait Analyzer: Speller {
    /// analyse the input word form
    fn analyze_input(self: Arc<Self>, word: &str) -> Vec<Suggestion>;
    /// analyse input word form with recasing and stuff from configs
    fn analyze_input_with_config(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
    ) -> Vec<Suggestion>;
    /// analyse the suggested word forms
    fn analyze_output(self: Arc<Self>, word: &str) -> Vec<Suggestion>;
    /// analyse the suggested word forms with recasing and stuff from configs
    fn analyze_output_with_config(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
    ) -> Vec<Suggestion>;
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

        let words = if config.recase {
            let variants = word_variants(word);
            variants.words
        } else {
            vec![]
        };
        log::debug!(
            "is_correct_with_config: ‘{}’ ~ {:?}?; config: {:?}",
            word,
            words,
            config
        );
        for word in std::iter::once(word.into()).chain(words.into_iter()) {
            let worker = SpellerWorker::new(self.clone(),
                self.to_input_vec(&word), config.clone(), false);

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

        if let Some(reweight) = config.reweight.as_ref() {
            let case_handler = word_variants(word);

            self.suggest_case(case_handler, config, reweight)
        } else {
            self.suggest_single(word, config)
        }
    }
}

impl<F, T, U> Analyzer for HfstSpeller<F, T, U>
where
    F: crate::vfs::File + Send,
    T: Transducer<F> + Send,
    U: Transducer<F> + Send,
{
    #[allow(clippy::wrong_self_convention)]
    fn analyze_input_with_config(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
    ) -> Vec<Suggestion> {
        if word.len() == 0 {
            return vec![];
        }

        let worker = SpellerWorker::new(self.clone(),
            self.to_input_vec(&word), config.clone(), false);

        log::trace!("Beginning analyze with config in mod");
        worker.analyze()
    }

    #[inline]
    fn analyze_input(self: Arc<Self>, word: &str) -> Vec<Suggestion> {
        self.analyze_input_with_config(word, &SpellerConfig::default())
    }

    #[inline]
    fn analyze_output(self: Arc<Self>, word: &str) -> Vec<Suggestion> {
        self.analyze_output_with_config(word, &SpellerConfig::default())
    }

    fn analyze_output_with_config(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
    ) -> Vec<Suggestion> {
        if word.len() == 0 {
            return vec![];
        }
        log::trace!("Beginning analyze suggest with config in mod");
        let worker = SpellerWorker::new(self.clone(),
            self.to_input_vec(word), config.clone(), false);

        worker.suggest()
    }
}

/// a speller consisting of two HFST automata
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
    /// create new speller from two automata
    pub fn new(mutator: T, mut lexicon: U) -> Arc<HfstSpeller<F, T, U>> {
        let alphabet_translator = lexicon.mut_alphabet().create_translator_from(&mutator);

        Arc::new(HfstSpeller {
            mutator,
            lexicon,
            alphabet_translator,
            _file: std::marker::PhantomData::<F>,
        })
    }

    /// get the error model automaton
    pub fn mutator(&self) -> &T {
        &self.mutator
    }

    /// get the language model automaton
    pub fn lexicon(&self) -> &U {
        &self.lexicon
    }

    fn alphabet_translator(&self) -> &Vec<SymbolNumber> {
        &self.alphabet_translator
    }

    fn to_input_vec(&self, word: &str) -> Vec<SymbolNumber> {
        let alphabet = self.mutator().alphabet();
        let key_table = alphabet.key_table();

        log::trace!("to_input_vec: {}", word);
        Graphemes::new(word)
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
        let worker = SpellerWorker::new(self.clone(), self.to_input_vec(word),
            config.clone(), true);

        log::trace!("suggesting single {}", word);
        worker.suggest()
    }

    fn suggest_case(
        self: Arc<Self>,
        case: CaseHandler,
        config: &SpellerConfig,
        reweight: &ReweightingConfig,
    ) -> Vec<Suggestion> {
        use crate::tokenizer::case_handling::*;

        log::trace!("suggesting cases...");
        let CaseHandler {
            original_input,
            mutation,
            mode,
            words,
        } = case;
        let mut best: HashMap<SmolStr, f32> = HashMap::new();

        for word in std::iter::once(&original_input).chain(words.iter()) {
            log::trace!("suggesting for word {}", word);
            let worker = SpellerWorker::new(self.clone(),
                self.to_input_vec(&word), config.clone(), true);
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
                    log::trace!("Case merge all");
                    for sugg in suggestions.into_iter() {
                        log::trace!("for {}", sugg.value);
                        let penalty_start =
                            if !sugg.value().starts_with(word.chars().next().unwrap()) {
                                reweight.start_penalty - reweight.mid_penalty
                            } else {
                                0.0
                            };
                        let penalty_end =
                            if !sugg.value().ends_with(word.chars().rev().next().unwrap()) {
                                reweight.end_penalty - reweight.mid_penalty
                            } else {
                                0.0
                            };

                        let distance =
                            strsim::damerau_levenshtein(&words[0].as_str(), &word.as_str())
                                + strsim::damerau_levenshtein(&word.as_str(), sugg.value());
                        let penalty_middle = reweight.mid_penalty * distance as f32;
                        let additional_weight = if sugg.value.chars().all(|c| is_emoji(c)) {
                            0.0
                        } else {
                            penalty_start + penalty_end + penalty_middle
                        };
                        log::trace!(
                            "Penalty: +{} = {} + {} * {} + {}",
                            additional_weight,
                            penalty_start,
                            distance,
                            reweight.mid_penalty,
                            penalty_end
                        );

                        best.entry(sugg.value.clone())
                            .and_modify(|entry| {
                                let weight = sugg.weight + additional_weight;
                                log::trace!(
                                    "=> Reweighting: {} {} = {} + {}",
                                    sugg.value,
                                    weight,
                                    sugg.weight,
                                    additional_weight
                                );
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
        let mut out: Vec<Suggestion>;
        if let Some(s) = &config.continuation_marker {
            out = best
                .into_iter()
                .map(|(k, v)| Suggestion {
                    value: k.clone(),
                    weight: v,
                    completed: Some(!k.ends_with(s)),
                })
                .collect::<Vec<_>>();
        } else {
            out = best
                .into_iter()
                .map(|(k, v)| Suggestion {
                    value: k,
                    weight: v,
                    completed: None,
                })
                .collect::<Vec<_>>();
        }
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
    pub struct FfiReweightingConfig {
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
        pub reweight: FfiReweightingConfig,
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
            let reweight = config
                .reweight
                .map(|c| FfiReweightingConfig {
                    start_penalty: c.start_penalty,
                    end_penalty: c.end_penalty,
                    mid_penalty: c.mid_penalty,
                })
                .unwrap_or_else(|| FfiReweightingConfig::default());

            let out = FfiSpellerConfig {
                n_best: config.n_best.unwrap_or(0),
                max_weight: config.max_weight.unwrap_or(0.0),
                beam: config.beam.unwrap_or(0.0),
                reweight,
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

            let reweight = if config.reweight == FfiReweightingConfig::default() {
                None
            } else {
                let c = config.reweight;
                Some(ReweightingConfig {
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
                reweight,
                node_pool_size: config.node_pool_size,
                continuation_marker: None,
                recase: true,
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
