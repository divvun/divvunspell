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
    #[inline(always)]
    pub fn string_from_symbols(&self, syms: &[SymbolNumber]) -> SmolStr {
        syms.iter().map(|s| &*self.key_table[*s as usize]).collect()
    }

    #[inline(always)]
    pub fn key_table(&self) -> &Vec<SmolStr> {
        &self.key_table
    }

    #[inline(always)]
    pub fn state_size(&self) -> SymbolNumber {
        self.flag_state_size
    }

    #[inline(always)]
    pub fn operations(&self) -> &OperationsMap {
        &self.operations
    }

    #[inline(always)]
    pub fn string_to_symbol(&self) -> &HashMap<SmolStr, SymbolNumber> {
        &self.string_to_symbol
    }

    #[inline(always)]
    pub fn is_flag(&self, symbol: SymbolNumber) -> bool {
        self.operations.contains_key(&symbol)
    }

    #[inline(always)]
    pub fn add_symbol(&mut self, string: &str) {
        self.string_to_symbol
            .insert(string.into(), self.key_table.len() as u16);
        self.key_table.push(string.into());
    }

    #[inline(always)]
    pub fn identity(&self) -> Option<SymbolNumber> {
        self.identity_symbol
    }

    #[inline(always)]
    pub fn unknown(&self) -> Option<SymbolNumber> {
        self.unknown_symbol
    }

    #[inline(always)]
    pub fn initial_symbol_count(&self) -> SymbolNumber {
        self.initial_symbol_count
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.length
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    #[inline(always)]
    pub fn create_translator_from<F, T>(&mut self, mutator: &T) -> Vec<SymbolNumber>
    where
        F: crate::vfs::File,
        T: Transducer<F>,
    {
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
