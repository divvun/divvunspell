use types::{SymbolNumber, FlagDiacriticOperator};

#[derive(Debug)]
pub struct TransducerAlphabet {
    pub key_table: Vec<String>,
    pub flag_state_size: SymbolNumber,
    pub length: usize,
    //operations: BTreeMap
}

impl TransducerAlphabet {
    pub fn new(buf: &[u8], symbols: SymbolNumber) -> TransducerAlphabet {
        println!("{:?}", symbols);

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
            //operations: BTreeMap::new()
        }
    }

    fn parse_flag_diacritic(key: &str) {
        let flag = key.chars().nth(1).unwrap();

        FlagDiacriticOperator::from_str(flag);
    }

    /*
    fn is_flag(&self, symbol: SymbolNumber) -> bool {
        self.operations.
    }
    */
}