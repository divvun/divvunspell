extern crate hfstospell;

use std::time::{Duration, Instant};

use hfstospell::archive::SpellerArchive;
use hfstospell::speller::{Speller, SpellerConfig};
use hfstospell::speller::suggestion::Suggestion;

fn time_suggest(speller: &Speller, word: &str) {
    let cfg = SpellerConfig {
        max_weight: Some(10000.0),
        n_best: None,
        beam: None
    };
    
    let now = Instant::now();
    let res = speller.suggest_with_config(word, &cfg);
    let then = now.elapsed();

    println!("{}: {}s{}ms - results: {:?}", word, then.as_secs(), then.subsec_nanos() / 1000000, res.len());
}

fn main() {
    let zhfst = SpellerArchive::new("./se-store.zhfst");
    let speller = zhfst.speller();
    // let res = speller.suggest("nuvviDspeller");

    // let human_rights = ["buot", "olbmot", "leat", "riegádan", "friddjan", "ja", 
    //     "olmmošárvvu", "ja", "olmmošvuoigatvuođaid", "dáfus", "Sii", "leat", 
    //     "jierbmalaš", "olbmot", "geain", "lea", "oamedovdu", "ja", "sii", "gálggaše", 
    //     "leat", "dego", "vieljačagat"];

    // let correct: Vec<bool> = human_rights.iter().map(|w| speller.is_correct(w)).collect();
    
    // let cfg = SpellerConfig {
    //     max_weight: None, // Some(10000.0),
    //     n_best: None,
    //     beam: None
    // };

    // let res: Vec<Vec<Suggestion>> = human_rights.iter().map(|w| speller.suggest_with_config(w, &cfg)).collect();

    // println!("{:?}", correct);
    // println!("{:?}", res);

    // let res = speller.suggest_with_config("gáibiđivččii", &cfg);
    // println!("{:?}", res);

    let words = ["vuovdinfállovuogiŧ", "eanavuoigatvuohtadutkamušas", "nannesivččii", "gárvanivččii", "gáibiđivččii"];

    for word in words.iter() {
        time_suggest(&speller, &word);
    }
}
