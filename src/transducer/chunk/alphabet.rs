use super::TransducerAlphabet;
use crate::types::{FlagDiacriticOperation, FlagDiacriticOperator, SymbolNumber, ValueNumber};
use hashbrown::HashMap;
use smol_str::SmolStr;

type OperationsMap = HashMap<SymbolNumber, FlagDiacriticOperation>;

pub struct TransducerAlphabetParser {
    key_table: Vec<SmolStr>,
    flag_state_size: SymbolNumber,
    string_to_symbol: HashMap<SmolStr, SymbolNumber>,
    operations: OperationsMap,
    feature_bucket: HashMap<SmolStr, SymbolNumber>,
    value_bucket: HashMap<SmolStr, ValueNumber>,
    val_n: ValueNumber,
    feat_n: SymbolNumber,
    identity_symbol: Option<SymbolNumber>,
    unknown_symbol: Option<SymbolNumber>,
}

impl TransducerAlphabetParser {
    fn new() -> TransducerAlphabetParser {
        TransducerAlphabetParser {
            key_table: Vec::with_capacity(64),
            flag_state_size: 0,
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

    fn handle_special_symbol(&mut self, i: SymbolNumber, key: &str) {
        let mut chunks = key.split('.');
        //debug!("chunks: {:?}", chunks);

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
        self.key_table.push("".into());
    }

    fn parse_inner(&mut self, buf: &[String]) {
        for (i, key) in buf.iter().enumerate() {
            let i = i as u16;

            if key.starts_with('@') && key.ends_with('@') {
                if key.chars().nth(2).unwrap() == '.' {
                    self.handle_special_symbol(i, &key);
                } else if key == "@_EPSILON_SYMBOL_@" {
                    self.value_bucket.insert("".into(), self.val_n);
                    self.key_table.push("".into());
                    self.val_n += 1;
                } else if key == "@_IDENTITY_SYMBOL_@" {
                    self.identity_symbol = Some(i);
                    self.key_table.push(key.into());
                } else if key == "@_UNKNOWN_SYMBOL_@" {
                    self.unknown_symbol = Some(i);
                    self.key_table.push(key.into());
                } else {
                    // No idea, skip.
                    self.key_table.push(SmolStr::from(""));
                }
            } else {
                self.key_table.push(key.into());
                self.string_to_symbol.insert(key.into(), i);
            }
        }

        self.flag_state_size = self.feature_bucket.len() as SymbolNumber;
    }

    pub fn parse(buf: &[String]) -> TransducerAlphabet {
        if buf.len() >= std::u16::MAX as usize {
            panic!("Alphabet larger than u16");
        }

        let mut p = TransducerAlphabetParser::new();
        p.parse_inner(buf);

        TransducerAlphabet {
            key_table: p.key_table,
            initial_symbol_count: buf.len() as u16,
            flag_state_size: p.flag_state_size,
            length: std::usize::MAX,
            string_to_symbol: p.string_to_symbol,
            operations: p.operations,
            identity_symbol: p.identity_symbol,
            unknown_symbol: p.unknown_symbol,
        }
    }
}
