use crate::types::{OperationsMap, SymbolNumber};
use hashbrown::HashMap;
use smol_str::SmolStr;

use super::super::{Alphabet, Transducer};

#[derive(Serialize, Deserialize)]
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

impl Alphabet for TransducerAlphabet {
    fn key_table(&self) -> &Vec<SmolStr> {
        &self.key_table
    }

    fn state_size(&self) -> SymbolNumber {
        self.flag_state_size
    }

    fn operations(&self) -> &OperationsMap {
        &self.operations
    }

    fn string_to_symbol(&self) -> &HashMap<SmolStr, SymbolNumber> {
        &self.string_to_symbol
    }

    fn is_flag(&self, symbol: SymbolNumber) -> bool {
        self.operations.contains_key(&symbol)
    }

    fn add_symbol(&mut self, string: &str) {
        self.string_to_symbol
            .insert(string.into(), self.key_table.len() as u16);
        self.key_table.push(string.into());
    }

    fn identity(&self) -> Option<SymbolNumber> {
        self.identity_symbol
    }

    fn unknown(&self) -> Option<SymbolNumber> {
        self.unknown_symbol
    }

    fn initial_symbol_count(&self) -> SymbolNumber {
        self.initial_symbol_count
    }

    fn len(&self) -> usize {
        self.length
    }

    fn is_empty(&self) -> bool {
        self.length == 0
    }
    fn create_translator_from<T: Transducer<Alphabet = Self>>(
        &mut self,
        mutator: &T,
    ) -> Vec<SymbolNumber> {
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
