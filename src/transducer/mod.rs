pub mod alphabet;
pub mod header;
pub mod index_table;
pub mod symbol_transition;
pub mod transition_table;
pub mod tree_node;

use std::collections::{BinaryHeap, BTreeMap};

use types::{TransitionTableIndex, Weight, SymbolNumber};
use constants::{TRANS_INDEX_SIZE, TRANS_SIZE, TARGET_TABLE};
use self::header::TransducerHeader;
use self::alphabet::TransducerAlphabet;
use self::index_table::IndexTable;
use self::transition_table::TransitionTable;
use self::symbol_transition::SymbolTransition;

#[derive(Debug)]
pub struct Transducer<'a> {
    buf: &'a [u8],
    header: TransducerHeader,
    alphabet: TransducerAlphabet,
    index_table: IndexTable<'a>,
    transition_table: TransitionTable<'a>
}

impl<'a> Transducer<'a> {
    pub fn from_bytes(buf: &[u8]) -> Transducer {
        let header = TransducerHeader::new(&buf);
        let alphabet_offset = header.alphabet_offset();
        let alphabet = TransducerAlphabet::new(&buf[alphabet_offset..buf.len()], header.symbol_count());

        let index_table_offset = alphabet_offset + alphabet.length();
        
        let index_table_end = index_table_offset + TRANS_INDEX_SIZE * header.index_table_size();
        let index_table = IndexTable::new(&buf[index_table_offset..index_table_end], header.index_table_size() as u32);

        let trans_table_end = index_table_end + TRANS_SIZE * header.target_table_size();
        let trans_table = TransitionTable::new(&buf[index_table_end..trans_table_end], header.target_table_size() as u32);

        Transducer {
            buf: buf,
            header: header,
            alphabet: alphabet,
            index_table: index_table,
            transition_table: trans_table
        }
    }

    pub fn index_table(&self) -> &'a IndexTable {
        &self.index_table
    }

    pub fn transition_table(&self) -> &'a TransitionTable {
        &self.transition_table
    }

    pub fn is_final(&self, i: TransitionTableIndex) -> bool {
        if i >= TARGET_TABLE {
            self.transition_table.is_final(i - TARGET_TABLE)
        } else {
            self.index_table.is_final(i)
        }
    }

    pub fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= TARGET_TABLE {
            self.transition_table.weight(i - TARGET_TABLE)
        } else {
            panic!("final_weight on an index seems fishy as fuck")
            //self.index_table.
        }
    }

    pub fn has_transitions(&self, i: TransitionTableIndex, s: Option<SymbolNumber>) -> bool {
        if s.is_none() {
            return false
        }

        let sym = s.unwrap();

        /*
        if i >= TARGET_TABLE {
            if let res = self.transition_table().input_symbol(i - TARGET_TABLE)
        } else {
            self.index_table(i + sym) == sym
        }
        */
        return true
    }

    pub fn has_epsilons_or_flags(&self, i: TransitionTableIndex) -> bool {
        if i >= TARGET_TABLE {
            let sym = self.transition_table.input_symbol(i - TARGET_TABLE);

            match sym {
                Some(sym) => self.alphabet.is_flag(sym),
                None => true
            }
        } else {
            self.index_table.input_symbol(i).is_none()
        }
    }

    pub fn take_epsilons(&self, i: TransitionTableIndex) -> SymbolTransition {
        // TODO IMPLEMENT
        unimplemented!()
        //self.take_epsilons_and_flags(i)
    }

    pub fn take_epsilons_and_flags(&self, i: TransitionTableIndex) -> SymbolTransition {
        let sym = self.transition_table.input_symbol(i);

        if sym.is_some() && !self.alphabet().is_flag(sym.unwrap()) {
            return SymbolTransition::empty()
        }

        SymbolTransition {
            index: self.transition_table.target(i).unwrap_or(0),
            symbol: self.transition_table.output_symbol(i),
            weight: self.transition_table.weight(i)
        }
    }
    
    fn take_non_epsilons(&self, i: TransitionTableIndex, symbol: SymbolNumber) -> SymbolTransition {
        if let Some(value) = self.transition_table.input_symbol(i) {
            if value != symbol {
                return SymbolTransition::empty()
            }
        }
        
        SymbolTransition {
            index: self.transition_table.target(i).unwrap_or(0),
            symbol: self.transition_table.output_symbol(i),
            weight: self.transition_table.weight(i)
        }
    }

/*
    fn lookup(&self, line: &str) {
        let mut analyses = BinaryHeap::new();
        let mut outputs = BTreeMap::new();

        let start_node = TreeNode::empty(vec![0; self.alphabet.flag_state_size as usize]);
        let mut node_queue: Vec<TreeNode> = vec![start_node];

        /*
        while node_queue.len > 0 {
            let next_node = node_queue.pop().unwrap();


        }*/


        while let Some(next_node) = node_queue.pop() {
            // Final states
            let node: TreeNode = next_node;

            if node.input_state == line.len && self.is_final(node.lexicon_state) {
                let weight = node.weight + self.final_weight(node.lexicon_state);
                // TODO
            }

            // Epsilon loop


            // Input consumption loop
        }


    }
*/
    pub fn next(&self, i: TransitionTableIndex, symbol: SymbolNumber) -> TransitionTableIndex {
        if i >= TARGET_TABLE {
            i - TARGET_TABLE + 1
        } else if let Some(v) = self.index_table.target(i + 1 + symbol as u32) {
            v - TARGET_TABLE
        } else {
            panic!("No next transition table index.")
        }
    }

    pub fn header(&self) -> &TransducerHeader {
        &self.header
    }

    pub fn alphabet(&self) -> &TransducerAlphabet {
        &self.alphabet
    }

    pub fn mut_alphabet(&mut self) -> &mut TransducerAlphabet {
        &mut self.alphabet
    }
}
