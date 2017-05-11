pub mod suggestion;

use std::cell::RefCell;
use std::collections::{BinaryHeap, BTreeMap};
use std::cmp::{Ordering};
use std::cmp::Ordering::Equal;

use transducer::Transducer;
use transducer::tree_node::TreeNode;
use speller::suggestion::Suggestion;
use types::{SymbolNumber, Weight, FlagDiacriticOperation};

type OperationMap = BTreeMap<SymbolNumber, FlagDiacriticOperation>;

#[derive(Debug)]
pub struct Speller<'a> {
    mutator: Transducer<'a>,
    lexicon: Transducer<'a>,
    input: Vec<SymbolNumber>,
    nodes: RefCell<Vec<TreeNode>>,
    operations: RefCell<OperationMap>,
    alphabet_translator: Vec<SymbolNumber>
}

fn gen_alphabet_translator(mutator: &Transducer, lexicon: &Transducer) -> Vec<SymbolNumber> {
    let from = mutator.alphabet();
    let to = lexicon.alphabet();
    let from_keys = from.key_table();
    let to_symbols = to.string_to_symbol();

    let mut translator = vec![0];

    for i in 1..from_keys.len() {
        let sym = from_keys.get(i).unwrap();

        match to_symbols.get(sym) {
            Some(v) => translator.push(v.clone()),
            None => {
                //let lexicon_key = lexicon.key_table().len();
                //lexicon.encoder().read_input_symbol(sym, lexicon_key);
                //lexicon.alphabet().add_symbol(sym);
                //translator.push(lexicon_key);
            }
        }
    }

    translator
}

impl<'a> Speller<'a> {
    pub fn new(mutator: Transducer<'a>, lexicon: Transducer<'a>) -> Speller<'a> {
        // TODO: review why this i16 -> u16 is happening
        let size = (&lexicon).alphabet().state_size() as i16;
        let alphabet_translator = gen_alphabet_translator(&mutator, &lexicon);
        
        Speller {
            mutator: mutator,
            lexicon: lexicon,
            input: vec![],
            nodes: RefCell::new(vec![]),
            operations: RefCell::new(BTreeMap::new()),
            alphabet_translator: alphabet_translator
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

    // TODO: move this to the Lexicon itself, this is stupid to be here.
    fn lexicon_epsilons(lexicon: &Transducer, operations: &OperationMap, nodes: &mut Vec<TreeNode>, next_node: &TreeNode) {
        if !lexicon.has_epsilons_or_flags(next_node.lexicon_state + 1) {
            return
        }

        let mut next = lexicon.next(next_node.lexicon_state, 0);
        let mut i_s = lexicon.take_epsilons_and_flags(next);
        
        while let Some(_) = i_s.symbol {
            // TODO: re-add weight limit checks
            match lexicon.transition_table().input_symbol(next) {
                None => {
                    // TODO: unwrap_or reqview
                    let x = next_node.update_lexicon(i_s.symbol, i_s.index, i_s.weight.unwrap_or(0.0));
                    
                    nodes.push(x);
                },
                Some(sym) => {
                    if let Some(op) = operations.get(&sym) {
                        let (is_success, applied_node) = next_node.apply_operation(op);

                        if is_success {
                            nodes.push(applied_node.update_lexicon(None, i_s.index, i_s.weight.unwrap_or(0.0)));
                        }
                    }
                }
            };

            next += 1;
            i_s = lexicon.take_epsilons_and_flags(next);
        }
    }

    fn mutator_epsilons(mutator: &Transducer, lexicon: &Transducer, alphabet_translator: &Vec<SymbolNumber>, nodes: &mut Vec<TreeNode>, next_node: &TreeNode) {
        if !mutator.has_transitions(next_node.mutator_state + 1, Some(0)) {
            return
        }

        let mut next_m = mutator.next(next_node.mutator_state, 0);
        let mut mutator_i_s = mutator.take_epsilons(next_m);

        while let Some(sym) = mutator_i_s.symbol {
            if sym == 0 {
                // TODO weight limit
                nodes.push(next_node.update_mutator(mutator_i_s.index, mutator_i_s.weight.unwrap_or(0.0)));
                next_m += 1;
                mutator_i_s = mutator.take_epsilons(next_m);
                continue;
            }

            let sym = alphabet_translator[mutator_i_s.symbol.unwrap_or(0) as usize];
            
            if !lexicon.has_transitions(next_node.lexicon_state + 1, Some(sym)) {
                //if sym >= lexicon.alphabet().orig_symbol_count() {

                //}

                next_m += 1;
                mutator_i_s = mutator.take_epsilons(next_m);
                continue;
            }

            Speller::queue_lexicon_arcs(
                sym, mutator_i_s.index, mutator_i_s.weight.unwrap_or(0.0), 0);
            
            next_m += 1;
            mutator_i_s = mutator.take_epsilons(next_m);
        }
    }

    pub fn queue_lexicon_arcs(input: SymbolNumber, mutator_state: u32, mutator_weight: Weight, input_increment: i16) {
        /*
        TransitionTableIndex next = lexicon->next(next_node.lexicon_state,
                                              input_sym);
        STransition i_s = lexicon->take_non_epsilons(next, input_sym);
        while (i_s.symbol != NO_SYMBOL)
        {
            if (i_s.symbol == lexicon->get_identity())
            {
                i_s.symbol = input[next_node.input_state];
            }
            if (mode == Correct || is_under_weight_limit(next_node.weight + i_s.weight + mutator_weight))
            {
                node_queue.push_back(next_node.update(
                                        (mode == Correct) ? input_sym : i_s.symbol,
                                        next_node.input_state + input_increment,
                                        mutator_state,
                                        i_s.index,
                                        i_s.weight + mutator_weight));
            }
            ++next;
            i_s = lexicon->take_non_epsilons(next, input_sym);
        }
        */
    }

    // Known as Speller::correct in C++    
    pub fn suggest(&self, input: &str) -> Vec<String> {
        // vec![input.to_string(), "extra".to_string()]
        let correction_queue = BinaryHeap::<StringWeightPair>::new();
        let start_node = TreeNode::empty(vec![self.state_size() as i16, 0]);

        let mut nodes = self.nodes.borrow_mut();
        nodes.clear();
        nodes.push(start_node);

        let corrections = BTreeMap::<String, Weight>::new();
        
        while nodes.len() > 0 {
            let next_node = nodes.pop().unwrap();
            
            Speller::lexicon_epsilons(self.lexicon(), &self.operations.borrow(), &mut nodes, &next_node);
            Speller::mutator_epsilons(self.mutator(), self.lexicon(), &self.alphabet_translator, &mut nodes, &next_node);
        }
        

        return vec![];
    }
}

struct StringWeightPair(String, Weight);

impl Eq for StringWeightPair {}

impl Ord for StringWeightPair {
    fn cmp(&self, other: &StringWeightPair) -> Ordering {
        self.1.partial_cmp(&other.1).unwrap_or(Equal)
    }
}

impl PartialOrd for StringWeightPair {
    fn partial_cmp(&self, other: &StringWeightPair) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for StringWeightPair {
    fn eq(&self, other: &StringWeightPair) -> bool {
        self.1 == other.1
    }
}


impl<'a> Drop for Speller<'a> {
    fn drop(&mut self) {
        // println!("Dropped: {:?}", self);
    }
}
