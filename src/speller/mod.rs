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
struct SuggestionData {
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
        let mut suggestions = worker.suggest();

        // Apply beam filtering: remove suggestions that are more than beam away from best.
        // Only enable beam when it is strictly greater than Weight::ZERO, to match FFI behavior.
        if let Some(beam) = config.beam {
            if beam > Weight::ZERO {
                if let Some(best) = suggestions.first() {
                    let beam_threshold = best.weight() + beam;
                    suggestions.retain(|s| s.weight() <= beam_threshold);
                }
            }
        }

        suggestions
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
                        
                        // Apply case mutation first (for output display),
                        // then calculate penalties using case-insensitive comparison below
                        match mutation {
                            CaseMutation::FirstCaps => {
                                sugg.value = upper_first(sugg.value());
                            }
                            CaseMutation::AllCaps => {
                                sugg.value = upper_case(sugg.value());
                            }
                            _ => {}
                        }

                        // Calculate distances based on case-insensitive alignment
                        // by scanning from both ends and splicing in the middle
                        let input_lower: Vec<char> = original_input.to_lowercase().chars().collect();
                        let sugg_lower: Vec<char> = sugg.value().to_lowercase().chars().collect();
                        
                        // Special case: both input and suggestion are 2 chars or less - no middle section
                        let is_short = input_lower.len() <= 2 && sugg_lower.len() <= 2;
                        
                        let (start_dist, mid_dist, end_dist): (usize, i32, usize) = if input_lower.is_empty() && sugg_lower.is_empty() {
                            (0, 0, 0)
                        } else if is_short {
                            // For very short words, compare first and last only (no middle)
                            let start_d = if !input_lower.is_empty() && !sugg_lower.is_empty() {
                                if input_lower[0] != sugg_lower[0] { 1 } else { 0 }
                            } else {
                                input_lower.len().max(sugg_lower.len()).min(1)
                            };
                            
                            let end_d = if input_lower.len() > 1 && sugg_lower.len() > 1 {
                                if input_lower[input_lower.len()-1] != sugg_lower[sugg_lower.len()-1] { 1 } else { 0 }
                            } else {
                                0
                            };
                            
                            // Use -1 to signal no middle section (will be displayed as "-")
                            (start_d, -1, end_d)
                        } else {
                            // Try all combinations of start and end offsets to find best overall match
                            let max_offset = 1;  // Try skipping up to 1 char at each end
                            let mut start_offsets = vec![];
                            let mut end_offsets = vec![];
                            
                            for i in 0..=max_offset {
                                for j in 0..=max_offset {
                                    start_offsets.push((i, j));
                                    end_offsets.push((i, j));
                                }
                            }
                            
                            let mut best_score = 0;
                            let mut best_alignment = (0, 0, 0, 0, 0, 0, 0, 0, 0); // (start_in, start_su, end_in, end_su, prefix, suffix, start_d, end_d, score)
                            
                            for (start_in_off, start_su_off) in &start_offsets {
                                if *start_in_off >= input_lower.len() || *start_su_off >= sugg_lower.len() {
                                    continue;
                                }
                                
                                for (end_in_off, end_su_off) in &end_offsets {
                                    if *end_in_off >= input_lower.len() || *end_su_off >= sugg_lower.len() {
                                        continue;
                                    }
                                    
                                    let inp = &input_lower[*start_in_off..];
                                    let sug = &sugg_lower[*start_su_off..];
                                    
                                    // Find prefix length
                                    let prefix_len = inp.iter().zip(sug.iter())
                                        .take_while(|(a, b)| a == b)
                                        .count();
                                    
                                    // Find suffix length (from the available lengths after offsets)
                                    let inp_len = input_lower.len() - start_in_off - end_in_off;
                                    let sug_len = sugg_lower.len() - start_su_off - end_su_off;
                                    
                                    if inp_len == 0 || sug_len == 0 {
                                        continue;
                                    }
                                    
                                    let inp_for_suffix = &input_lower[*start_in_off..input_lower.len() - end_in_off];
                                    let sug_for_suffix = &sugg_lower[*start_su_off..sugg_lower.len() - end_su_off];
                                    
                                    let suffix_len = inp_for_suffix.iter().rev()
                                        .zip(sug_for_suffix.iter().rev())
                                        .take_while(|(a, b)| a == b)
                                        .count();
                                    
                                    // Calculate total match score
                                    let score = prefix_len + suffix_len;
                                    
                                    if score > best_score {
                                        // Calculate start distance based on what's skipped
                                        let start_d = if *start_in_off == 0 && *start_su_off == 0 {
                                            0  // No offset, will be handled by prefix matching
                                        } else {
                                            // DL between skipped portions
                                            let inp_start_str: String = input_lower[0..*start_in_off].iter().collect();
                                            let sug_start_str: String = sugg_lower[0..*start_su_off].iter().collect();
                                            strsim::damerau_levenshtein(&inp_start_str, &sug_start_str)
                                        };
                                        
                                        // Calculate end distance based on what's skipped
                                        let end_d = if *end_in_off == 0 && *end_su_off == 0 {
                                            0  // No offset, will be handled by suffix matching
                                        } else {
                                            // DL between skipped portions
                                            let inp_end_str: String = input_lower[input_lower.len().saturating_sub(*end_in_off)..].iter().collect();
                                            let sug_end_str: String = sugg_lower[sugg_lower.len().saturating_sub(*end_su_off)..].iter().collect();
                                            strsim::damerau_levenshtein(&inp_end_str, &sug_end_str)
                                        };
                                        
                                        best_score = score;
                                        best_alignment = (*start_in_off, *start_su_off, *end_in_off, *end_su_off, 
                                                         prefix_len, suffix_len, start_d, end_d, score);
                                    }
                                }
                            }
                            
                            let (start_in_off, start_su_off, end_in_off, end_su_off, prefix_len, suffix_len, start_d, end_d, _) = best_alignment;
                            
                            // Calculate what's between prefix and suffix, avoiding overlap
                            let min_total_len = (input_lower.len() - start_in_off - end_in_off)
                                .min(sugg_lower.len() - start_su_off - end_su_off);
                            
                            let actual_suffix = if prefix_len + suffix_len > min_total_len {
                                min_total_len.saturating_sub(prefix_len)
                            } else {
                                suffix_len
                            };
                            
                            // Calculate what's between prefix and suffix
                            let inp_start_pos = start_in_off + prefix_len;
                            let sug_start_pos = start_su_off + prefix_len;
                            let inp_end_pos = input_lower.len() - end_in_off - actual_suffix;
                            let sug_end_pos = sugg_lower.len() - end_su_off - actual_suffix;
                            
                            // Check if there's a real middle section
                            let inp_remaining = inp_end_pos.saturating_sub(inp_start_pos);
                            let sug_remaining = sug_end_pos.saturating_sub(sug_start_pos);
                            
                            let (mid_d, adjusted_end_d) = if inp_remaining == 0 && sug_remaining == 0 {
                                (0, end_d)
                            } else if (inp_remaining <= 1 && sug_remaining <= 1) && actual_suffix == 0 {
                                let end_change = inp_remaining.max(sug_remaining) > 0;
                                (0, if end_change { 1 } else { end_d })
                            } else if inp_start_pos < inp_end_pos || sug_start_pos < sug_end_pos {
                                let inp_mid: String = input_lower[inp_start_pos.min(inp_end_pos)..inp_end_pos.max(inp_start_pos)].iter().collect();
                                let sug_mid: String = sugg_lower[sug_start_pos.min(sug_end_pos)..sug_end_pos.max(sug_start_pos)].iter().collect();
                                let d = strsim::damerau_levenshtein(&inp_mid, &sug_mid) as i32;
                                (d, end_d)
                            } else {
                                (0, end_d)
                            };
                            
                            (start_d, mid_d, adjusted_end_d)
                        };
                        
                        // Special case: when input or suggestion has duplicate chars at start/end that match
                        // Examples: 
                        // - Insertion: "Anar" → "Aanaar" - both start with 'a', insertion of second 'a' should be middle
                        // - Deletion: "Aarâhšoddâdem" → "Arâšoddâdem" - both start with 'A', deletion of second 'a' should be middle
                        let (start_dist, mid_dist, end_dist) = if !is_short && input_lower.len() > 0 && sugg_lower.len() > 0 {
                            let adjusted_start = if start_dist > 0 && input_lower[0] == sugg_lower[0] {
                                // First char matches - check if either has duplicate at position 1
                                let sugg_has_dup = sugg_lower.len() > 1 && sugg_lower[0] == sugg_lower[1];
                                let input_has_dup = input_lower.len() > 1 && input_lower[0] == input_lower[1];
                                if sugg_has_dup || input_has_dup {
                                    // Move the start change to middle
                                    0
                                } else {
                                    start_dist
                                }
                            } else {
                                start_dist
                            };
                            
                            let adjusted_end = if end_dist > 0 && 
                                                input_lower.len() > 0 && 
                                                sugg_lower.len() > 0 &&
                                                input_lower[input_lower.len()-1] == sugg_lower[sugg_lower.len()-1] {
                                // Last char matches - check if either has duplicate at position len-2
                                let sugg_has_dup = sugg_lower.len() > 1 && sugg_lower[sugg_lower.len()-1] == sugg_lower[sugg_lower.len()-2];
                                let input_has_dup = input_lower.len() > 1 && input_lower[input_lower.len()-1] == input_lower[input_lower.len()-2];
                                if sugg_has_dup || input_has_dup {
                                    // Move the end change to middle
                                    0
                                } else {
                                    end_dist
                                }
                            } else {
                                end_dist
                            };
                            
                            // Add any moved changes to mid_dist
                            let added_to_mid = (start_dist - adjusted_start) + (end_dist - adjusted_end);
                            let adjusted_mid = if mid_dist < 0 {
                                // Was no middle section, now there is
                                added_to_mid as i32
                            } else {
                                mid_dist + added_to_mid as i32
                            };
                            
                            (adjusted_start, adjusted_mid, adjusted_end)
                        } else {
                            (start_dist, mid_dist, end_dist)
                        };
                        
                        let penalty_start = if start_dist > 0 { reweight.start_penalty } else { 0.0 };
                        let penalty_middle = if mid_dist < 0 { 
                            -1.0  // Signal for no middle section (will be displayed as "-")
                        } else { 
                            reweight.mid_penalty * mid_dist as f32 
                        };
                        let penalty_end = if end_dist > 0 { reweight.end_penalty } else { 0.0 };
                        let additional_weight =
                            Weight(if sugg.value.chars().all(|c| is_emoji(c)) {
                                0.0
                            } else {
                                penalty_start + penalty_end + penalty_middle.max(0.0)  // Don't add -1 to weight
                            });
                        
                        tracing::trace!(
                            "Penalty: +{} = {} + {} * {} + {}",
                            additional_weight,
                            penalty_start,
                            mid_dist,
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

        // Apply beam filtering: remove suggestions that are more than beam away from best.
        // Only enable beam when it is strictly greater than Weight::ZERO, to match FFI behavior.
        if let Some(beam) = config.beam {
            if beam > Weight::ZERO {
                if let Some(best) = out.first() {
                    let beam_threshold = best.weight() + beam;
                    out.retain(|s| s.weight() <= beam_threshold);
                }
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
                verbose: false,
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
