pub mod suggestion;

use std::cell::RefCell;

use transducer::Transducer;
use transducer::tree_node::TreeNode;
use speller::suggestion::Suggestion;
use types::{SymbolNumber};

#[derive(Debug)]
pub struct Speller<'a> {
    mutator: Transducer<'a>,
    lexicon: Transducer<'a>,
    input: Vec<SymbolNumber>,
    nodes: RefCell<Vec<TreeNode>>,
    next_node: TreeNode
}

impl<'a> Speller<'a> {
    pub fn new(mutator: Transducer<'a>, lexicon: Transducer<'a>) -> Speller<'a> {
        // TODO: review why this i16 -> u16 is happening
        let size = (&lexicon).alphabet().state_size() as i16;
        
        Speller {
            mutator: mutator,
            lexicon: lexicon,
            input: vec![],
            nodes: RefCell::new(vec![]),
            next_node: TreeNode::empty(vec![size, 0])
        }
    }

    pub fn mutator(&self) -> &Transducer {
        &self.mutator
    }

    pub fn lexicon(&self) -> &Transducer {
        &self.lexicon
    }

    fn state_size(&self) -> SymbolNumber {
        self.lexicon.alphabet().flag_state_size
    }

    fn lexicon_epsilons(self) {
        let lexicon = self.lexicon();

        if lexicon.has_epsilons_or_flags(self.next_node.lexicon_state + 1) {
            return
        }

        let mut next = lexicon.next(self.next_node.lexicon_state, 0);
        let mut i_s = lexicon.take_epsilons_and_flags(next);
        
        while let Some(_) = i_s.symbol {
            // TODO: re-add weight limit checks
            match lexicon.transition_table().input_symbol(next) {
                Some(value) => {
                    // TODO: unwrap_or reqview
                    let x = self.next_node.update_lexicon(i_s.symbol, i_s.index, i_s.weight.unwrap_or(0.0));
                    
                    self.nodes.borrow_mut().push(x);
                },
                None => {
                    // let old_flags = self.next_node.flag_state
                }
            };

            next += 1;
            i_s = lexicon.take_epsilons_and_flags(next);
        }
    }

    pub fn correct(&self, line: &str) -> Vec<String> {
        vec![]
    }
    
    pub fn suggest(&self, input: &str) -> Vec<String> {
        vec![input.to_string(), "extra".to_string()]
    }
}

impl<'a> Drop for Speller<'a> {
    fn drop(&mut self) {
        // println!("Dropped: {:?}", self);
    }
}
