use std::sync::OnceLock;

use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

use crate::transducer::Transducer;
use crate::types::{FlagDiacriticOperation, OperationsMap, SymbolNumber};

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
    /// `operations`' symbols as a bitset, built by the loaders: the lexicon
    /// walk asks whether a symbol is a flag at every state it leaves.
    #[serde(skip)]
    pub(crate) flag_bits: Vec<u64>,
    /// `operations` by symbol number, built with `flag_bits`.
    #[serde(skip)]
    pub(crate) flag_operations: Vec<Option<FlagDiacriticOperation>>,
    /// How many of the symbols, numbered first, occur on the input side,
    /// where the file says so: an optimized-lookup header does, a THFST
    /// alphabet does not.
    #[serde(skip)]
    pub(crate) input_symbol_count: Option<SymbolNumber>,
    /// The length in bytes of the longest symbol
    /// [`longest_input_symbol`](Self::longest_input_symbol) can match, found
    /// on first use. It counts only symbols numbered below
    /// `initial_symbol_count`, which [`add_symbol`](Self::add_symbol) never
    /// adds, so it holds for the alphabet's lifetime.
    #[serde(skip)]
    pub(crate) longest_input: OnceLock<usize>,
}

impl TransducerAlphabet {
    #[inline(always)]
    pub fn string_from_symbols(&self, syms: &[SymbolNumber]) -> SmolStr {
        syms.iter()
            .map(|s| &*self.key_table[s.0 as usize])
            .collect()
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
        match self.flag_bits.get(symbol.0 as usize / 64) {
            Some(word) => word & (1 << (symbol.0 % 64)) != 0,
            // An alphabet no loader indexed answers from the map.
            None if self.flag_bits.is_empty() => self.operations.contains_key(&symbol),
            None => false,
        }
    }

    /// Build the flag bitset [`is_flag`](Self::is_flag) reads, and the table
    /// [`flag_operation`](Self::flag_operation) reads, from the flag
    /// operations; the loaders call this once the alphabet is complete.
    pub(crate) fn index_flags(&mut self) {
        let len = self
            .operations
            .keys()
            .map(|s| s.0 as usize + 1)
            .max()
            .unwrap_or(0);
        let mut bits = vec![0u64; len.div_ceil(64)];
        let mut operations = vec![None; len];
        for (s, operation) in &self.operations {
            bits[s.0 as usize / 64] |= 1 << (s.0 % 64);
            operations[s.0 as usize] = Some(*operation);
        }
        self.flag_bits = bits;
        self.flag_operations = operations;
    }

    /// The flag diacritic operation of `symbol`, if it is a flag.
    #[inline(always)]
    pub fn flag_operation(&self, symbol: SymbolNumber) -> Option<&FlagDiacriticOperation> {
        match self.flag_operations.get(symbol.0 as usize) {
            Some(operation) => operation.as_ref(),
            // An alphabet no loader indexed answers from the map.
            None if self.flag_bits.is_empty() => self.operations.get(&symbol),
            None => None,
        }
    }

    /// Whether input text may be read as `symbol`: a regular symbol of the
    /// transducer's own, on its input side where the file says which those
    /// are, and not a flag diacritic.
    fn is_input_symbol(&self, symbol: SymbolNumber) -> bool {
        symbol < self.initial_symbol_count
            && self.input_symbol_count.is_none_or(|count| symbol < count)
            && !self.is_flag(symbol)
    }

    /// The longest input symbol whose text `text` starts with, and the length
    /// of that text in bytes.
    pub(crate) fn longest_input_symbol(&self, text: &str) -> Option<(SymbolNumber, usize)> {
        let longest = *self.longest_input.get_or_init(|| {
            self.string_to_symbol
                .iter()
                .filter(|(_, symbol)| self.is_input_symbol(**symbol))
                .map(|(key, _)| key.len())
                .max()
                .unwrap_or(0)
        });
        (1..=longest.min(text.len()))
            .rev()
            .filter(|&len| text.is_char_boundary(len))
            .find_map(|len| {
                self.string_to_symbol
                    .get(&text[..len])
                    .copied()
                    .filter(|symbol| self.is_input_symbol(*symbol))
                    .map(|symbol| (symbol, len))
            })
    }

    #[inline(always)]
    pub fn add_symbol(&mut self, string: &str) {
        self.string_to_symbol.insert(
            string.into(),
            SymbolNumber(self.key_table.len().try_into().expect("too many symbols")),
        );
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
    pub fn create_translator_from<T>(&mut self, mutator: &T) -> Vec<SymbolNumber>
    where
        T: Transducer,
    {
        tracing::trace!("create_translator_from");
        let from = mutator.alphabet();
        let from_keys = from.key_table();

        let mut translator = Vec::with_capacity(64);
        translator.push(SymbolNumber::ZERO);

        for from_sym in from_keys.iter().skip(1) {
            tracing::trace!("key {}", from_sym);
            if let Some(sym) = self.string_to_symbol.get(from_sym) {
                translator.push(*sym);
            } else {
                let lexicon_key = SymbolNumber(self.key_table.len() as u16);
                translator.push(lexicon_key);
                self.add_symbol(from_sym);
            }
        }

        translator
    }
}
