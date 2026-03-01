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

/// Temporary struct to store weight details during suggestion generation
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct SuggestionData {
    weight: Weight,
    base_weight: Weight,
    lexicon_weight: Weight,
    mutator_weight: Weight,
    reweight_start: f32,
    reweight_mid: f32,
    reweight_end: f32,
}

/// Controls whether morphological tags are preserved in FST output.
///
/// When traversing an FST, epsilon transitions can either preserve their symbols
/// (keeping morphological tags like "+V", "+Noun", etc.) or convert them to true
/// epsilons (stripping the tags from the output).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum OutputMode {
    /// Strip morphological tags from output.
    ///
    /// Used for spelling correction where you want clean word forms without tags.
    /// Example: "run" instead of "run+V+PresPartc"
    WithoutTags,

    /// Keep morphological tags in output.
    ///
    /// Used for morphological analysis where you want to see the linguistic structure.
    /// Example: "run+V+PresPartc" instead of "run"
    WithTags,
}

/// configurable extra penalties for edit distance
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ReweightingConfig {
    #[serde(default = "default_start_penalty")]
    pub start_penalty: f32,
    #[serde(default = "default_end_penalty")]
    pub end_penalty: f32,
    #[serde(default = "default_mid_penalty")]
    pub mid_penalty: f32,
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
    /// whether we try to recase mispelt word before other suggestions
    #[serde(default = "default_recase")]
    pub recase: bool,
    /// used when suggesting unfinished word parts
    #[serde(default)]
    pub completion_marker: Option<String>,
    /// whether to output detailed weight information (not serialized)
    #[serde(skip)]
    pub verbose: bool,
}

impl SpellerConfig {
    /// create a default configuration with following values:
    /// * n_best = 10
    /// * max_weight = 10000
    /// * beam = None
    /// * reweight = default (c.f. ReweightingConfig::default())
    /// * node_pool_size = 128
    /// * recase = true
    /// * verbose = false
    pub const fn default() -> SpellerConfig {
        SpellerConfig {
            n_best: default_n_best(),
            max_weight: default_max_weight(),
            beam: default_beam(),
            reweight: default_reweight(),
            node_pool_size: default_node_pool_size(),
            recase: default_recase(),
            completion_marker: None,
            verbose: false,
        }
    }
}

const fn default_n_best() -> Option<usize> {
    Some(10)
}

const fn default_max_weight() -> Option<Weight> {
    Some(Weight(10000.0))
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
/// FST-based spell checker and morphological analyzer.
///
/// This trait provides methods for spell checking and morphological analysis
/// using finite-state transducers. The same FST traversal logic is used for both
/// operations - the difference is controlled by the `OutputMode`:
///
/// - `OutputMode::WithoutTags` strips morphological tags (for spelling correction)
/// - `OutputMode::WithTags` preserves morphological tags (for morphological analysis)
pub trait Speller {
    /// Check if the word is correctly spelled
    #[must_use]
    fn is_correct(self: Arc<Self>, word: &str) -> bool;

    /// Check if word is correctly spelled with config (handles recasing, etc.)
    #[must_use]
    fn is_correct_with_config(self: Arc<Self>, word: &str, config: &SpellerConfig) -> bool;

    /// Generate suggestions or analyses for a word.
    #[must_use]
    fn suggest(self: Arc<Self>, word: &str) -> Vec<Suggestion>;

    /// Generate suggestions with config options (recasing, reweighting, etc.)
    #[must_use]
    fn suggest_with_config(self: Arc<Self>, word: &str, config: &SpellerConfig) -> Vec<Suggestion>;

    /// Analyze the input word form.
    ///
    /// Performs lexicon-only traversal (no error model) to get morphological analyses
    /// of exactly what was typed. Does not generate spelling corrections.
    #[must_use]
    fn analyze_input(self: Arc<Self>, word: &str) -> Vec<Suggestion>;

    /// Analyze input word form with config options.
    #[must_use]
    fn analyze_input_with_config(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
    ) -> Vec<Suggestion>;

    /// Get lexicon weight for a word form (lexicon-only traversal).
    ///
    /// Returns the weight of the best analysis using only the lexicon FST.
    /// If the word is not in the lexicon, returns Weight(0.0).
    /// Useful for separating lexicon vs mutator contributions to total weight.
    #[must_use]
    fn get_lexicon_weight(self: Arc<Self>, word: &str) -> Weight {
        self.get_lexicon_weight_with_config(word, &SpellerConfig::default())
    }

    /// Get lexicon weight with custom config.
    #[must_use]
    fn get_lexicon_weight_with_config(self: Arc<Self>, word: &str, config: &SpellerConfig) -> Weight;

    /// Analyze the suggested word forms.
    ///
    /// Generates spelling corrections using the error model, then returns them with
    /// morphological tags preserved (equivalent to `suggest(word, OutputMode::WithTags)`).
    #[must_use]
    fn analyze_output(self: Arc<Self>, word: &str) -> Vec<Suggestion>;

    /// Analyze suggested word forms with config options.
    #[must_use]
    fn analyze_output_with_config(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
    ) -> Vec<Suggestion>;

    /// Create suggestion list and use their analyses for filtering.
    ///
    /// Gets spelling corrections, analyzes each one, and filters based on
    /// morphological analysis results.
    #[must_use]
    fn analyze_suggest(self: Arc<Self>, word: &str) -> Vec<Suggestion>;

    /// Create suggestion list and use analyses for filtering with config.
    #[must_use]
    fn analyze_suggest_with_config(
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
        tracing::debug!(
            "is_correct_with_config: ‘{}’ ~ {:?}?; config: {:?}",
            word,
            words,
            config
        );
        for word in std::iter::once(word.into()).chain(words.into_iter()) {
            let worker = SpellerWorker::new(
                self.clone(),
                self.to_input_vec(&word),
                config.clone(),
                OutputMode::WithoutTags,
            );

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
        self._suggest_with_config(word, config, OutputMode::WithoutTags)
    }

    fn analyze_input_with_config(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
    ) -> Vec<Suggestion> {
        if word.is_empty() {
            return vec![];
        }

        let worker = SpellerWorker::new(
            self.clone(),
            self.to_input_vec(word),
            config.clone(),
            OutputMode::WithTags,
        );

        tracing::trace!("Beginning analyze_input with config");
        worker.analyze()
    }

    #[inline]
    fn analyze_input(self: Arc<Self>, word: &str) -> Vec<Suggestion> {
        self.analyze_input_with_config(word, &SpellerConfig::default())
    }

    fn get_lexicon_weight(self: Arc<Self>, word: &str) -> Weight {
        self.get_lexicon_weight_with_config(word, &SpellerConfig::default())
    }

    fn get_lexicon_weight_with_config(self: Arc<Self>, word: &str, config: &SpellerConfig) -> Weight {
        if word.is_empty() {
            return Weight(0.0);
        }

        // Analyze output form using lexicon-only traversal (without error model)
        // This gives us the weight from the lexicon/acceptor alone
        let worker = SpellerWorker::new(
            self.clone(),
            self.to_input_vec(word),
            SpellerConfig { verbose: false, ..config.clone() },
            OutputMode::WithoutTags,
        );

        let analyses = worker.analyze();
        analyses.first().map(|s| s.weight()).unwrap_or(Weight(0.0))
    }

    fn analyze_output_with_config(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
    ) -> Vec<Suggestion> {
        self._suggest_with_config(word, config, OutputMode::WithTags)
    }

    #[inline]
    fn analyze_output(self: Arc<Self>, word: &str) -> Vec<Suggestion> {
        self.analyze_output_with_config(word, &SpellerConfig::default())
    }

    fn analyze_suggest_with_config(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
    ) -> Vec<Suggestion> {
        let mut suggs = self.clone().suggest_with_config(word, config);
        suggs.retain(|sugg| {
            tracing::trace!("suggestion {}", sugg.value);
            let analyses = self
                .clone()
                .analyze_input_with_config(sugg.value.as_str(), config);
            let mut all_filtered = true;
            for analysis in analyses {
                tracing::trace!("-> {}", analysis.value);
                if !analysis.value.contains("+Spell/NoSugg") {
                    all_filtered = false;
                } else {
                    tracing::trace!("filtering=?");
                }
            }
            !all_filtered
        });
        suggs
    }

    #[inline]
    fn analyze_suggest(self: Arc<Self>, word: &str) -> Vec<Suggestion> {
        self.analyze_suggest_with_config(word, &SpellerConfig::default())
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

    fn _suggest_with_config(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
        mode: OutputMode,
    ) -> Vec<Suggestion> {
        use crate::tokenizer::case_handling::*;

        if word.len() == 0 {
            return vec![];
        }

        if let Some(reweight) = config.reweight.as_ref() {
            let case_handler = word_variants(word);

            self.suggest_case(case_handler, config, reweight, mode)
        } else {
            self.suggest_single(word, config, mode)
        }
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

        tracing::trace!("to_input_vec: {}", word);
        Graphemes::new(word)
            .map(|ch| {
                let s = ch.to_string();
                key_table
                    .iter()
                    .position(|x| x == &s)
                    .map(|x| SymbolNumber(x as u16))
                    .unwrap_or_else(|| alphabet.unknown().unwrap_or(SymbolNumber::ZERO))
            })
            .collect()
    }

    fn suggest_single(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
        mode: OutputMode,
    ) -> Vec<Suggestion> {
        let worker =
            SpellerWorker::new(self.clone(), self.to_input_vec(word), config.clone(), mode);

        tracing::trace!("suggesting single {}", word);
        worker.suggest()
    }

    fn suggest_case(
        self: Arc<Self>,
        case: CaseHandler,
        config: &SpellerConfig,
        reweight: &ReweightingConfig,
        output_mode: OutputMode,
    ) -> Vec<Suggestion> {
        use crate::tokenizer::case_handling::*;

        tracing::trace!("suggesting cases...");
        let CaseHandler {
            original_input,
            mutation,
            mode,
            words,
        } = case;
        let mut best: HashMap<SmolStr, Weight> = HashMap::new();
        let mut suggestion_data: HashMap<SmolStr, SuggestionData> = HashMap::new();

        for word in std::iter::once(&original_input).chain(words.iter()) {
            tracing::trace!("suggesting for word {}", word);
            let worker = SpellerWorker::new(
                self.clone(),
                self.to_input_vec(&word),
                config.clone(),
                output_mode,
            );
            let suggestions = worker.suggest();
            
            match mode {
                CaseMode::MergeAll => {
                    tracing::trace!("Case merge all");
                    for mut sugg in suggestions.into_iter() {
                        tracing::trace!("for {}", sugg.value);
                        // Calculate penalties BEFORE applying case mutation
                        let penalty_start =
                            if !sugg.value().starts_with(word.chars().next().unwrap()) {
                                reweight.start_penalty
                            } else {
                                0.0
                            };
                        let penalty_end =
                            if !sugg.value().ends_with(word.chars().rev().next().unwrap()) {
                                reweight.end_penalty
                            } else {
                                0.0
                            };
                        
                        // Apply case mutation AFTER calculating penalties
                        match mutation {
                            CaseMutation::FirstCaps => {
                                sugg.value = upper_first(sugg.value());
                            }
                            CaseMutation::AllCaps => {
                                sugg.value = upper_case(sugg.value());
                            }
                            _ => {}
                        }

                        // Calculate mid-distance: exclude first/last chars if they differ
                        let skip_first = original_input.chars().next() != sugg.value().chars().next();
                        let skip_last = original_input.chars().rev().next() != sugg.value().chars().rev().next();
                        
                        let mid_distance = if original_input.chars().count() > 1 && sugg.value().chars().count() > 1 {
                            let input_chars: Vec<char> = original_input.chars().collect();
                            let sugg_chars: Vec<char> = sugg.value().chars().collect();
                            
                            let input_start = if skip_first { 1 } else { 0 };
                            let input_end = input_chars.len() - if skip_last { 1 } else { 0 };
                            let sugg_start = if skip_first { 1 } else { 0 };
                            let sugg_end = sugg_chars.len() - if skip_last { 1 } else { 0 };
                            
                            if input_start < input_end && sugg_start < sugg_end {
                                let input_mid: String = input_chars[input_start..input_end].iter().collect();
                                let sugg_mid: String = sugg_chars[sugg_start..sugg_end].iter().collect();
                                strsim::damerau_levenshtein(&input_mid, &sugg_mid)
                            } else {
                                0
                            }
                        } else {
                            0
                        };
                        
                        let penalty_middle = reweight.mid_penalty * mid_distance as f32;
                        let additional_weight =
                            Weight(if sugg.value.chars().all(|c| is_emoji(c)) {
                                0.0
                            } else {
                                penalty_start + penalty_end + penalty_middle
                            });
                        
                        tracing::trace!(
                            "Penalty: +{} = {} + {} * {} + {}",
                            additional_weight,
                            penalty_start,
                            mid_distance,
                            reweight.mid_penalty,
                            penalty_end
                        );

                        best.entry(sugg.value.clone())
                            .and_modify(|entry| {
                                let weight = sugg.weight + additional_weight;
                                tracing::trace!(
                                    "=> Reweighting: {} {} = {} + {}",
                                    sugg.value,
                                    weight,
                                    sugg.weight,
                                    additional_weight
                                );
                                if entry as &_ > &weight {
                                    *entry = weight;
                                    // Update suggestion data
                                    let (lex_w, mut_w) = if let Some(ref details) = sugg.weight_details {
                                        (details.lexicon_weight, details.mutator_weight)
                                    } else {
                                        (Weight(0.0), Weight(0.0))
                                    };
                                    suggestion_data.insert(sugg.value.clone(), SuggestionData {
                                        weight,
                                        base_weight: sugg.weight,
                                        lexicon_weight: lex_w,
                                        mutator_weight: mut_w,
                                        reweight_start: penalty_start,
                                        reweight_mid: penalty_middle,
                                        reweight_end: penalty_end,
                                    });
                                }
                            })
                            .or_insert_with(|| {
                                let weight = sugg.weight + additional_weight;
                                let (lex_w, mut_w) = if let Some(ref details) = sugg.weight_details {
                                    (details.lexicon_weight, details.mutator_weight)
                                } else {
                                    (Weight(0.0), Weight(0.0))
                                };
                                suggestion_data.insert(sugg.value.clone(), SuggestionData {
                                    weight,
                                    base_weight: sugg.weight,
                                    lexicon_weight: lex_w,
                                    mutator_weight: mut_w,
                                    reweight_start: penalty_start,
                                    reweight_mid: penalty_middle,
                                    reweight_end: penalty_end,
                                });
                                weight
                            });
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
        if config.verbose {
            // Verbose mode: include weight details
            if let Some(s) = &config.completion_marker {
                out = best
                    .into_iter()
                    .map(|(k, v)| {
                        let data = suggestion_data.get(&k);
                        Suggestion {
                            value: k.clone(),
                            weight: v,
                            completed: Some(!k.ends_with(s)),
                            weight_details: data.map(|d| suggestion::WeightDetails {
                                lexicon_weight: d.lexicon_weight,
                                mutator_weight: d.mutator_weight,
                                reweight_start: d.reweight_start,
                                reweight_mid: d.reweight_mid,
                                reweight_end: d.reweight_end,
                            }),
                        }
                    })
                    .collect::<Vec<_>>();
            } else {
                out = best
                    .into_iter()
                    .map(|(k, v)| {
                        let data = suggestion_data.get(&k);
                        Suggestion {
                            value: k,
                            weight: v,
                            completed: None,
                            weight_details: data.map(|d| suggestion::WeightDetails {
                                lexicon_weight: d.lexicon_weight,
                                mutator_weight: d.mutator_weight,
                                reweight_start: d.reweight_start,
                                reweight_mid: d.reweight_mid,
                                reweight_end: d.reweight_end,
                            }),
                        }
                    })
                    .collect::<Vec<_>>();
            }
        } else {
            // Normal mode: no weight details
            if let Some(s) = &config.completion_marker {
                out = best
                    .into_iter()
                    .map(|(k, v)| Suggestion {
                        value: k.clone(),
                        weight: v,
                        completed: Some(!k.ends_with(s)),
                        weight_details: None,
                    })
                    .collect::<Vec<_>>();
            } else {
                out = best
                    .into_iter()
                    .map(|(k, v)| Suggestion {
                        value: k,
                        weight: v,
                        completed: None,
                        weight_details: None,
                    })
                    .collect::<Vec<_>>();
            }
        }
        out.sort();
        if let Some(n_best) = config.n_best {
            out.truncate(n_best);
        }
        
        // Apply beam filtering: remove suggestions that are more than beam away from best
        if let Some(beam) = config.beam {
            if let Some(best) = out.first() {
                let beam_threshold = best.weight() + beam;
                out.retain(|s| s.weight() <= beam_threshold);
            }
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
                max_weight: config.max_weight.unwrap_or(Weight::ZERO),
                beam: config.beam.unwrap_or(Weight::ZERO),
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

            let config: &FfiSpellerConfig = unsafe { &*ptr.cast() };

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
                max_weight: if config.max_weight > Weight::ZERO {
                    Some(config.max_weight)
                } else {
                    None
                },
                beam: if config.beam > Weight::ZERO {
                    Some(config.beam)
                } else {
                    None
                },
                reweight,
                node_pool_size: config.node_pool_size,
                recase: true,
                completion_marker: None,
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
