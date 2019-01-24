use std::io::{self, Read};

use clap::{Arg, App, AppSettings, SubCommand};
use hashbrown::HashMap;

use divvunspell::archive::SpellerArchive;
use divvunspell::speller::{Speller, SpellerConfig};
use divvunspell::speller::suggestion::Suggestion;
use divvunspell::tokenizer::{Tokenize, Token};
use divvunspell::transducer::chunk::ChfstBundle;

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
        .version(env!("CARGO_PKG_VERSION"))
        .author("Brendan Molloy <brendan@bbqsrc.net>")
        .about("Testing frontend for the DivvunSpell library")
        .arg(Arg::with_name("zhfst")
            .short("z")
            .long("zhfst")
            .value_name("ZHFST")
            // .required(true)
            .help("Use the given ZHFST file")
            .takes_value(true))
        .arg(Arg::with_name("chfst")
            .short("c")
            .long("chfst")
            .value_name("CHFST")
            .help("Use the given CHFST bundle")
            .takes_value(true))
        .arg(Arg::with_name("suggest")
            .short("s")
            .long("suggest")
            .help("Show suggestions for given word(s)"))
        .arg(Arg::with_name("always-suggest")
            .short("S")
            .long("always-suggest")
            .help("Always show suggestions even if word is correct (implies -s)"))
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
        .subcommand(SubCommand::with_name("chunk")
            .arg(Arg::with_name("zhfst")
            .short("z")
            .long("zhfst")
            .value_name("ZHFST")
            .required(true)
            .help("Use the given ZHFST file")
            .takes_value(true)))
        .get_matches();

    if let Some(ref matches) = matches.subcommand_matches("chunk") {
        let zhfst_file = matches.value_of("zhfst").unwrap();

        let archive = match divvunspell::archive::SpellerArchive::new(zhfst_file) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{:?}", e);
                std::process::exit(1);
            }
        };

        let speller = archive.speller();
        let mutator = speller.mutator();
        let lexicon = speller.lexicon();

        use std::path::Path;

        let target_dir = Path::new("./out.chfst");
        let chunk_size: usize = 36 * 1024 * 1024;

        eprintln!("Serializing lexicon...");
        lexicon.serialize(chunk_size, &target_dir.join("lexicon")).unwrap();

        eprintln!("Serializing mutator...");
        mutator.serialize(chunk_size, &target_dir.join("mutator")).unwrap();

        return;
    }

    let is_always_suggesting = matches.is_present("always-suggest");
    let is_suggesting = matches.is_present("suggest") || is_always_suggesting;
    let is_json = matches.is_present("json");

    let n_best = matches.value_of("nbest").and_then(|v| v.parse::<usize>().ok());
    let max_weight = matches.value_of("weight").and_then(|v| v.parse::<f32>().ok());

    let words: Vec<String> = match matches.values_of("WORDS") {
        Some(v) => v.map(|x| x.to_string()).collect(),
        None => {
            eprintln!("Reading from stdin...");
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer).expect("reading stdin");
            buffer.tokenize().filter(|x| x.is_word()).map(|x| x.value().to_string()).collect()
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

    if let Some(zhfst_file) = matches.value_of("zhfst") {
        let archive = match divvunspell::archive::SpellerArchive::new(zhfst_file) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{:?}", e);
                std::process::exit(1);
            }
        };

        let speller = archive.speller();

        for word in words {
            let is_correct = speller.clone().is_correct(&word);
            writer.write_correction(&word, is_correct);

            if is_suggesting && (is_always_suggesting || !is_correct) {
                let suggestions = speller.clone().suggest_with_config(&word, &suggest_cfg);
                writer.write_suggestions(&word, &suggestions);
            }
        }
    } else if let Some(chfst_file) = matches.value_of("chfst") {
        let bundle = match ChfstBundle::from_path(std::path::Path::new(chfst_file)) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{:?}", e);
                std::process::exit(1);
            }
        };

        let speller = bundle.speller();
        
        for word in words {
            let is_correct = speller.clone().is_correct(&word);
            writer.write_correction(&word, is_correct);

            if is_suggesting && (is_always_suggesting || !is_correct) {
                let suggestions = speller.clone().suggest_with_config(&word, &suggest_cfg);
                writer.write_suggestions(&word, &suggestions);
            }
        }
    }

    writer.finish();
}
