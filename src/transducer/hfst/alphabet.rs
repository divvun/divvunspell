use std::borrow::Cow;
use std::path::Path;

use hashbrown::HashMap;
use smol_str::SmolStr;

use crate::transducer::TransducerError;
use crate::transducer::alphabet::TransducerAlphabet;
use crate::types::{
    FlagDiacriticOperation, FlagDiacriticOperator, OperationsMap, SymbolNumber, ValueNumber,
};

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
            flag_state_size: SymbolNumber::ZERO,
            length: 0,
            string_to_symbol: HashMap::new(),
            operations: HashMap::new(),
            feature_bucket: HashMap::new(),
            value_bucket: HashMap::new(),
            val_n: ValueNumber::ZERO,
            feat_n: SymbolNumber::ZERO,
            identity_symbol: None,
            unknown_symbol: None,
        }
    }
}

impl TransducerAlphabetParser {
    pub fn new() -> TransducerAlphabetParser {
        Self::default()
    }

    fn handle_special_symbol(
        &mut self,
        i: SymbolNumber,
        key: &str,
        path: &Path,
    ) -> Result<(), TransducerError> {
        use std::str::FromStr;
        let mut chunks = key.split('.');

        let head = chunks
            .next()
            .ok_or_else(|| TransducerError::AlphabetMalformed {
                path: path.to_path_buf(),
                detail: Cow::Owned(format!("empty alphabet key at symbol {}", i.0)),
            })?;
        let op_chars = head
            .get(1..)
            .ok_or_else(|| TransducerError::AlphabetMalformed {
                path: path.to_path_buf(),
                detail: Cow::Owned(format!(
                    "alphabet key '{key}' is missing its leading '@' sigil"
                )),
            })?;
        let fdo = FlagDiacriticOperator::from_str(op_chars).map_err(|_| {
            TransducerError::AlphabetMalformed {
                path: path.to_path_buf(),
                detail: Cow::Owned(format!("unknown flag diacritic operator in key '{key}'")),
            }
        })?;
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
            self.feat_n = self.feat_n.incr();
        }

        if !self.value_bucket.contains_key(&value) {
            self.value_bucket.insert(value.clone(), self.val_n);
            self.val_n = self.val_n.incr();
        }

        let op = FlagDiacriticOperation {
            operation: fdo,
            feature: self.feature_bucket[&feature],
            value: self.value_bucket[&value],
        };

        self.operations.insert(i, op);
        self.key_table.push(key.into());
        Ok(())
    }

    fn parse_inner(
        &mut self,
        buf: &[u8],
        symbols: SymbolNumber,
        path: &Path,
    ) -> Result<(), TransducerError> {
        let mut offset = 0usize;

        for i in 0..symbols.0 {
            let i = SymbolNumber(i);
            let mut end = 0usize;

            // Find the null terminator, bounded by the buffer.
            loop {
                let probe = offset
                    .checked_add(end)
                    .filter(|p| *p < buf.len())
                    .ok_or_else(|| TransducerError::AlphabetMalformed {
                        path: path.to_path_buf(),
                        detail: Cow::Owned(format!(
                            "alphabet key {} runs past end of buffer at offset {offset}",
                            i.0
                        )),
                    })?;
                if buf[probe] == 0 {
                    break;
                }
                end += 1;
            }

            let key: SmolStr = String::from_utf8_lossy(&buf[offset..offset + end]).into();

            if key.len() > 1 && key.starts_with('@') && key.ends_with('@') {
                // Flag diacritics have the form @<op>.FEATURE.VALUE@ — at least 3 chars plus @@.
                let is_flag = key.len() >= 5 && key.as_bytes().get(2) == Some(&b'.');

                if is_flag {
                    self.handle_special_symbol(i, &key, path)?;
                } else if key == "@_EPSILON_SYMBOL_@" {
                    self.value_bucket.insert("".into(), self.val_n);
                    self.key_table.push("".into());
                    self.val_n = self.val_n.incr();
                } else if key == "@_IDENTITY_SYMBOL_@" {
                    self.identity_symbol = Some(i);
                    self.key_table.push(key);
                } else if key == "@_UNKNOWN_SYMBOL_@" {
                    self.unknown_symbol = Some(i);
                    self.key_table.push(key);
                } else {
                    // An unrecognised @...@ key. Not fatal — the transducer
                    // works without it — but flag it so the user knows something
                    // unexpected is in the alphabet.
                    tracing::warn!("unhandled alphabet key '{}' in '{}'", key, path.display());
                    self.key_table.push(SmolStr::from(""));
                }
            } else {
                self.key_table.push(key.clone());
                self.string_to_symbol.insert(key.clone(), i);
            }

            offset += end + 1;
        }

        self.flag_state_size =
            SymbolNumber(self.feature_bucket.len().try_into().map_err(|_| {
                TransducerError::AlphabetMalformed {
                    path: path.to_path_buf(),
                    detail: Cow::Borrowed("too many flag features to fit in SymbolNumber"),
                }
            })?);

        // Count remaining null padding bytes.
        while offset < buf.len() && buf[offset] == b'\0' {
            offset += 1;
        }

        self.length = offset;
        Ok(())
    }

    pub fn parse(
        buf: &[u8],
        symbols: SymbolNumber,
        path: &Path,
    ) -> Result<TransducerAlphabet, TransducerError> {
        let mut p = TransducerAlphabetParser::new();
        p.parse_inner(buf, symbols, path)?;

        Ok(TransducerAlphabet {
            key_table: p.key_table,
            initial_symbol_count: symbols,
            length: p.length,
            flag_state_size: p.flag_state_size,
            string_to_symbol: p.string_to_symbol,
            operations: p.operations,
            identity_symbol: p.identity_symbol,
            unknown_symbol: p.unknown_symbol,
        })
    }
}
