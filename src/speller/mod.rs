pub mod suggestion;

use transducer::Transducer;
use speller::suggestion::Suggestion;

#[derive(Debug)]
pub struct Speller<'a> {
    mutator: &'a Transducer<'a>,
    lexicon: &'a Transducer<'a>
}

impl<'a> Speller<'a> {
    fn new(mutator: &'a Transducer<'a>, lexicon: &'a Transducer<'a>) -> Speller<'a> {
        Speller {
            mutator: mutator,
            lexicon: lexicon
        }
    }

    fn correct(&self, line: &str) -> Vec<String> {
        vec![]
    }
    
    pub fn suggest(&self, input: &str) -> Vec<String> {
        vec![input.to_string(), "extra".to_string()]
    }
}

impl<'a> Drop for Speller<'a> {
    fn drop(&mut self) {
        println!("Dropped: {:?}", self);
    }
}
