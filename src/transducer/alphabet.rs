use types::{SymbolNumber, FlagDiacriticOperator, FlagDiacriticOperation};
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct TransducerAlphabet {
    key_table: Vec<String>,
    pub flag_state_size: SymbolNumber,
    pub length: usize,
    string_to_symbol: BTreeMap<String, SymbolNumber>,
    operations: BTreeMap<SymbolNumber, FlagDiacriticOperation>
}

impl TransducerAlphabet {
    pub fn new(buf: &[u8], symbols: SymbolNumber) -> TransducerAlphabet {
        //println!("{:?}", symbols);

        // Buf should be beginning of alphabet.
        let mut key_table: Vec<String> = vec![];

        let mut offset = 0;

        for i in 1..symbols {
            let mut end = 0;

            while buf[offset + end] != 0 {
                end += 1;
            }

            let key = String::from_utf8_lossy(&buf[offset..offset+end]).into_owned();

            if key.starts_with("@") && key.ends_with("@") {
                if key.chars().nth(2).unwrap() == '.' {
                    TransducerAlphabet::parse_flag_diacritic(&key);
                }
            } else {
                key_table.push(key);
            }

            offset += end + 1;
        }

        TransducerAlphabet {
            key_table: key_table,
            length: offset,
            flag_state_size: 0,
            string_to_symbol: BTreeMap::new(),
            operations: BTreeMap::new()
        }
    }

    pub fn key_table(&self) -> &Vec<String> {
        &self.key_table
    }

    pub fn state_size(&self) -> SymbolNumber {
        self.flag_state_size
    }

    pub fn string_to_symbol(&self) -> &BTreeMap<String, SymbolNumber> {
        &self.string_to_symbol
    }

    pub fn parse_flag_diacritic(key: &str) {
        let flag = key.chars().nth(1).unwrap();

        FlagDiacriticOperator::from_str(flag);
    }

    pub fn is_flag(&self, symbol: SymbolNumber) -> bool {
        self.operations.contains_key(&symbol)
    }
}