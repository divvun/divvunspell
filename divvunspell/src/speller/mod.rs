pub mod suggestion;
pub mod worker;

use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::f32;
use std::sync::Arc;

use self::worker::SpellerWorker;
use crate::speller::suggestion::Suggestion;
use crate::transducer::Transducer;
use crate::types::{SymbolNumber, Weight};
use crate::tokenizer::case_handling::CaseHandler;


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
            mid_penalty: 5.0
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
                key_table.iter().position(|x| x == &s)
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

    fn suggest_case(self: Arc<Self>, case: CaseHandler, config: &SpellerConfig, case_handling: &CaseHandlingConfig) -> Vec<Suggestion> {
        use crate::tokenizer::case_handling::{CaseMutation, CaseMode};
        use crate::tokenizer::case_handling::*;

        let CaseHandler { mutation, mode, words } = case;
        let mut best: HashMap<SmolStr, f32> = HashMap::new();

        for word in words.into_iter() {
            let worker = SpellerWorker::new(self.clone(), self.to_input_vec(&word), config.clone());
            let mut suggestions = worker.suggest();

            match mutation {
                CaseMutation::FirstCaps => {
                    suggestions
                        .iter_mut()
                        .for_each(|x| {
                            x.value = upper_first(x.value());
                        });
                }
                CaseMutation::AllCaps => {
                    suggestions
                        .iter_mut()
                        .for_each(|x| {
                            x.value = upper_case(x.value());
                        });
                }
                _ => {}
            }

            match mode {
                CaseMode::MergeAll => {
                    for sugg in suggestions.into_iter() {
                        let penalty_start = if !sugg.value().starts_with(word.chars().next().unwrap()) {
                            case_handling.start_penalty
                        } else {
                            0.0
                        };
                        let penalty_end = if !sugg.value().ends_with(word.chars().rev().next().unwrap()) {
                            case_handling.end_penalty
                        } else {
                            0.0
                        };

                        let distance = strsim::damerau_levenshtein(&word.as_str(), sugg.value());
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
