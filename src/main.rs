use std::io::{self, Read};

use clap::{Arg, App, AppSettings, SubCommand};
use hashbrown::HashMap;

use divvunspell::archive::SpellerArchive;
use divvunspell::speller::{Speller, SpellerConfig};
use divvunspell::speller::suggestion::Suggestion;
use divvunspell::tokenizer::{Tokenize, Token};

use serde_derive::Serialize;

trait OutputWriter {
    fn write_correction(&mut self, word: &str, is_correct: bool);
    fn write_suggestions(&mut self, word: &str, suggestions: &[Suggestion]);
    fn finish(&mut self);
}

struct StdoutWriter;

impl OutputWriter for StdoutWriter {
    fn write_correction(&mut self, word: &str, is_correct: bool) { 
        println!("Input: {}\t\t[{}]", &word, if is_correct { "CORRECT" } else { "INCORRECT" });
    }

    fn write_suggestions(&mut self, word: &str, suggestions: &[Suggestion]) {
        for sugg in suggestions {
            println!("{}\t\t{}", sugg.value, sugg.weight);
        }
        println!("");
    }

    fn finish(&mut self) {}
}

#[derive(Serialize)]
struct SuggestionRequest {
    word: String,
    is_correct: bool,
    suggestions: Vec<Suggestion>
}

#[derive(Serialize)]
#[serde(rename_all="camelCase")]
struct JsonWriter {
    results: Vec<SuggestionRequest>
}

impl JsonWriter {
    pub fn new() -> JsonWriter {
        JsonWriter { results: vec![] }
    }
}

impl OutputWriter for JsonWriter {
    fn write_correction(&mut self, word: &str, is_correct: bool) { 
        self.results.push(SuggestionRequest {
            word: word.to_owned(),
            is_correct,
            suggestions: vec![]
        });
    }

    fn write_suggestions(&mut self, word: &str, suggestions: &[Suggestion]) {
        let i = self.results.len() - 1;
        self.results[i].suggestions = suggestions.to_vec();
    }

    fn finish(&mut self) {
        println!("{}", serde_json::to_string_pretty(self).unwrap());
    }
}

fn main() {
    let matches = App::new("divvunspell")
        .setting(AppSettings::ArgRequiredElseHelp)
        // .setting(AppSettings::DeriveDisplayOrder)
        .version(env!("CARGO_PKG_VERSION"))
        .author("Brendan Molloy <brendan@bbqsrc.net>")
        .about("Testing frontend for the DivvunSpell library")
        .arg(Arg::with_name("zhfst")
            .short("z")
            .long("zhfst")
            .value_name("ZHFST")
            .required(true)
            .help("Use the given ZHFST file")
            .takes_value(true))
        .arg(Arg::with_name("suggest")
            .short("s")
            .long("suggest")
            .help("Show suggestions for given word(s)"))
        .arg(Arg::with_name("weight")
            .short("w")
            .long("weight")
            .requires("suggest")
            .takes_value(true)
            .help("Maximum weight limit for suggestions"))
        .arg(Arg::with_name("nbest")
            .short("n")
            .long("nbest")
            .requires("suggest")
            .takes_value(true)
            .help("Maximum number of results for suggestions"))
        .arg(Arg::with_name("json")
            .long("json")
            .help("Output results in JSON"))
        .arg(Arg::with_name("WORDS")
            .multiple(true)
            .help("The words to be processed"))
        .get_matches();

    let is_suggesting = matches.is_present("suggest");
    let is_json = matches.is_present("json");
    
    let n_best = matches.value_of("nbest").and_then(|v| v.parse::<usize>().ok());
    let max_weight = matches.value_of("weight").and_then(|v| v.parse::<f32>().ok());

    let zhfst_file = matches.value_of("zhfst").unwrap();
    let words: Vec<String> = match matches.values_of("WORDS") {
        Some(v) => v.map(|x| x.to_string()).collect(),
        None => {
            eprintln!("Reading from stdin...");
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer).expect("reading stdin");
            buffer.tokenize().filter(|x| x.is_word()).map(|x| x.value().to_string()).collect()
        }
    };

    let archive = match divvunspell::archive::SpellerArchive::new(zhfst_file) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{:?}", e);
            std::process::exit(1);
        }
    };

    let mut writer: Box<OutputWriter> = if is_json {
        Box::new(JsonWriter::new())
    } else {
        Box::new(StdoutWriter)
    };

    let suggest_cfg = SpellerConfig {
        max_weight,
        n_best,
        beam: None,
        with_caps: true
    };

    for word in words {
        let result = archive.speller().is_correct(&word);
        writer.write_correction(&word, result);

        if is_suggesting {
            let suggestions = archive.speller().suggest_with_config(&word, &suggest_cfg);
            writer.write_suggestions(&word, &suggestions);
        }
    }

    writer.finish();
}