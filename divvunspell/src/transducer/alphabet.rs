use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

use crate::transducer::Transducer;
use crate::types::{OperationsMap, SymbolNumber};

#[derive(Debug, Serialize, Deserialize)]
pub struct TransducerAlphabet {
    pub(crate) key_table: Vec<SmolStr>,
    pub(crate) initial_symbol_count: SymbolNumber,
    pub(crate) flag_state_size: SymbolNumber,
    pub(crate) length: usize,
    pub(crate) string_to_symbol: HashMap<SmolStr, SymbolNumber>,
    pub(crate) operations: OperationsMap,
    pub(crate) identity_symbol: Option<SymbolNumber>,
    pub(crate) unknown_symbol: Option<SymbolNumber>,
}

impl TransducerAlphabet {
    pub fn key_table(&self) -> &Vec<SmolStr> {
        &self.key_table
    }

    pub fn state_size(&self) -> SymbolNumber {
        self.flag_state_size
    }

    pub fn operations(&self) -> &OperationsMap {
        &self.operations
    }

    pub fn string_to_symbol(&self) -> &HashMap<SmolStr, SymbolNumber> {
        &self.string_to_symbol
    }

    pub fn is_flag(&self, symbol: SymbolNumber) -> bool {
        self.operations.contains_key(&symbol)
    }

    pub fn add_symbol(&mut self, string: &str) {
        self.string_to_symbol
            .insert(string.into(), self.key_table.len() as u16);
        self.key_table.push(string.into());
    }

    pub fn identity(&self) -> Option<SymbolNumber> {
        self.identity_symbol
    }

    pub fn unknown(&self) -> Option<SymbolNumber> {
        self.unknown_symbol
    }

    pub fn initial_symbol_count(&self) -> SymbolNumber {
        self.initial_symbol_count
    }

    pub fn len(&self) -> usize {
        self.length
    }

    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub fn create_translator_from<T: Transducer>(&mut self, mutator: &T) -> Vec<SymbolNumber> {
        let from = mutator.alphabet();
        let from_keys = from.key_table();

        let mut translator = Vec::with_capacity(64);
        translator.push(0);

        for from_sym in from_keys.iter().skip(1) {
            if let Some(&sym) = self.string_to_symbol.get(from_sym) {
                translator.push(sym);
            } else {
                let lexicon_key = self.key_table.len() as SymbolNumber;
                translator.push(lexicon_key);
                self.add_symbol(from_sym);
            }
        }

        translator
    }
}
