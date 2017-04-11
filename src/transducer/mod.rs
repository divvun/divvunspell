pub mod alphabet;
pub mod header;
pub mod index_table;
pub mod transition_table;
pub mod tree_node;

use std::collections::{BinaryHeap, BTreeMap};

use types::{TransitionTableIndex, Weight};
use constants::{TRANS_INDEX_SIZE, TRANS_SIZE, TARGET_TABLE};
use self::header::TransducerHeader;
use self::alphabet::TransducerAlphabet;
use self::index_table::IndexTable;
use self::transition_table::TransitionTable;
use self::tree_node::TreeNode;

#[derive(Debug)]
pub struct Transducer<'a> {
    buf: &'a [u8],
    header: TransducerHeader,
    alphabet: TransducerAlphabet,
    index_table: IndexTable<'a>,
    transition_table: TransitionTable<'a>
}

impl<'a> Transducer<'a> {
    fn new(buf: &[u8]) -> Transducer {
        let header = TransducerHeader::new(&buf);
        let alphabet = TransducerAlphabet::new(&buf[header.alphabet_offset..buf.len()], header.symbols);

        let index_table_offset = header.alphabet_offset + alphabet.length;
        let index_table_end = index_table_offset + TRANS_INDEX_SIZE * header.trans_index_table;
        let index_table = IndexTable::new(&buf[index_table_offset..index_table_end], header.trans_index_table as u32);

        let trans_table_end = index_table_end + TRANS_SIZE * header.trans_target_table;
        let trans_table = TransitionTable::new(&buf[index_table_end..trans_table_end], header.trans_target_table as u32);

        Transducer {
            buf: buf,
            header: header,
            alphabet: alphabet,
            index_table: index_table,
            transition_table: trans_table
        }
    }

    fn is_final(&self, i: TransitionTableIndex) -> bool {
        if i >= TARGET_TABLE {
            self.transition_table.is_final(i - TARGET_TABLE)
        } else {
            self.index_table.is_final(i)
        }
    }

    fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= TARGET_TABLE {
            self.transition_table.weight(i - TARGET_TABLE)
        } else {
            panic!("final_weight on an index seems fishy as fuck")
            //self.index_table.
        }
    }

    /*
    fn has_epsilons_or_flags(&self, i: TransitionTableIndex) -> bool {
        if i >= TARGET_TABLE {
            let sym = self.transition_table.input_symbol(i - TARGET_TABLE);

            sym == 0 || self.alphabet.is_flag(sym)
        } else {
            self.index_table.input_symbol(i) == 0
        }
    }

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

    /*
    fn next(&self, i: TransitionTableIndex, symbol: SymbolNumber) -> Option<TransitionTableIndex> {
        if i >= TARGET_TABLE {
            Some(i - TARGET_TABLE + 1)
        } else {
            self.index_table.target(i + 1 + symbol as u32) - TARGET_TABLE
        }
    }
    */
}
