// // use std::collections::HashMap;
// use crate::ml_speller::gpt2;
// use smol_str::SmolStr;

// pub struct MLSuggestion {
//     pub value: SmolStr,
// }

// impl MLSuggestion {
//     pub fn new(value: SmolStr) -> MLSuggestion {
//         MLSuggestion {value}
//     }

//     pub fn value(&self) -> &str {
//         &self.value
//     }

//     pub fn suggest(&self, word:String) -> Vec<MLSuggestion> {
//         log::trace!("Beginning suggest");

//         // let mut corrections = HashMap::new();
//         let mut suggestions: Vec<MLSuggestion> = vec![];

//         let model = gpt2::load_mlmodel().unwrap();

//         let preds = gpt2::generate_suggestions(model, word);
//         for p in preds {
//             suggestions.push(MLSuggestion::new(SmolStr::new(p)));
//         }
//         suggestions
//     }
// }
