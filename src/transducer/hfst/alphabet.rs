use hashbrown::HashMap;
use smol_str::SmolStr;

use crate::types::{
    FlagDiacriticOperation, FlagDiacriticOperator, OperationsMap, SymbolNumber, ValueNumber,
};

use crate::transducer::alphabet::TransducerAlphabet;

pub struct TransducerAlphabetParser {
    key_table: Vec<SmolStr>,
    flag_state_size: SymbolNumber,
    length: usize,
    string_to_symbol: HashMap<SmolStr, SymbolNumber>,
    operations: OperationsMap,
    feature_bucket: HashMap<SmolStr, SymbolNumber>,
    value_bucket: HashMap<SmolStr, ValueNumber>,
    val_n: ValueNumber,
    feat_n: SymbolNumber,
    identity_symbol: Option<SymbolNumber>,
    unknown_symbol: Option<SymbolNumber>,
}

impl std::default::Default for TransducerAlphabetParser {
    fn default() -> Self {
        TransducerAlphabetParser {
            key_table: Vec::with_capacity(64),
            flag_state_size: 0,
            length: 0,
            string_to_symbol: HashMap::new(),
            operations: HashMap::new(),
            feature_bucket: HashMap::new(),
            value_bucket: HashMap::new(),
            val_n: 0i16,
            feat_n: 0u16,
            identity_symbol: None,
            unknown_symbol: None,
        }
    }
}

impl TransducerAlphabetParser {
    pub fn new() -> TransducerAlphabetParser {
        Self::default()
    }

    fn handle_special_symbol(&mut self, i: SymbolNumber, key: &str) {
        use std::str::FromStr;
        let mut chunks = key.split('.');

        let fdo = FlagDiacriticOperator::from_str(&chunks.next().unwrap()[1..]).unwrap();
        let feature: SmolStr = chunks
            .next()
            .unwrap_or("")
            .chars()
            .filter(|x| x != &'@')
            .collect();
        let value: SmolStr = chunks
            .next()
            .unwrap_or("")
            .chars()
            .filter(|x| x != &'@')
            .collect();

        if !self.feature_bucket.contains_key(&feature) {
            self.feature_bucket.insert(feature.clone(), self.feat_n);
            self.feat_n += 1;
        }

        if !self.value_bucket.contains_key(&value) {
            self.value_bucket.insert(value.clone(), self.val_n);
            self.val_n += 1;
        }

        let op = FlagDiacriticOperation {
            operation: fdo,
            feature: self.feature_bucket[&feature],
            value: self.value_bucket[&value],
        };

        self.operations.insert(i, op);
        self.key_table.push(key.into());
    }

    fn parse_inner(&mut self, buf: &[u8], symbols: SymbolNumber) {
        let mut offset = 0usize;

        for i in 0..symbols {
            let mut end = 0usize;

            while buf[offset + end] != 0 {
                end += 1;
            }

            let key: SmolStr = String::from_utf8_lossy(&buf[offset..offset + end]).into();

            if key.len() > 1 && key.starts_with('@') && key.ends_with('@') {
                if key.chars().nth(2).unwrap() == '.' {
                    self.handle_special_symbol(i, &key);
                } else if key == "@_EPSILON_SYMBOL_@" {
                    self.value_bucket.insert("".into(), self.val_n);
                    self.key_table.push("".into());
                    self.val_n += 1;
                } else if key == "@_IDENTITY_SYMBOL_@" {
                    self.identity_symbol = Some(i);
                    self.key_table.push(key);
                } else if key == "@_UNKNOWN_SYMBOL_@" {
                    self.unknown_symbol = Some(i);
                    self.key_table.push(key);
                } else {
                    // No idea, skip.
                    eprintln!("Unhandled alphabet key: {}", &key);
                    self.key_table.push(SmolStr::from(""));
                }
            } else {
                self.key_table.push(key.clone());
                self.string_to_symbol.insert(key.clone(), i);
            }

            offset += end + 1;
        }

        self.flag_state_size = self.feature_bucket.len() as SymbolNumber;

        // Count remaining null padding bytes
        while buf[offset] == b'\0' {
            offset += 1;
        }

        self.length = offset;
    }

    pub fn parse(buf: &[u8], symbols: SymbolNumber) -> TransducerAlphabet {
        let mut p = TransducerAlphabetParser::new();
        p.parse_inner(buf, symbols);

        TransducerAlphabet {
            key_table: p.key_table,
            initial_symbol_count: symbols,
            length: p.length,
            flag_state_size: p.flag_state_size,
            string_to_symbol: p.string_to_symbol,
            operations: p.operations,
            identity_symbol: p.identity_symbol,
            unknown_symbol: p.unknown_symbol,
        }
    }
}
