use std::io::{self, Read};
use std::sync::Arc;

use clap::{App, AppSettings, Arg, ArgGroup};
use serde::Serialize;

use divvunspell::archive::{BoxSpellerArchive, ZipSpellerArchive};
use divvunspell::speller::suggestion::Suggestion;
use divvunspell::speller::{Speller, SpellerConfig};
use divvunspell::tokenizer::Tokenize;
use divvunspell::transducer::{thfst::ThfstTransducer, Transducer};

trait OutputWriter {
    fn write_correction(&mut self, word: &str, is_correct: bool);
    fn write_suggestions(&mut self, word: &str, suggestions: &[Suggestion]);
    fn finish(&mut self);
}

struct StdoutWriter;

impl OutputWriter for StdoutWriter {
    fn write_correction(&mut self, word: &str, is_correct: bool) {
        println!(
            "Input: {}\t\t[{}]",
            &word,
            if is_correct { "CORRECT" } else { "INCORRECT" }
        );
    }

    fn write_suggestions(&mut self, _word: &str, suggestions: &[Suggestion]) {
        for sugg in suggestions {
            println!("{}\t\t{}", sugg.value, sugg.weight);
        }
        println!();
    }

    fn finish(&mut self) {}
}

#[derive(Serialize)]
struct SuggestionRequest {
    word: String,
    is_correct: bool,
    suggestions: Vec<Suggestion>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonWriter {
    results: Vec<SuggestionRequest>,
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
            suggestions: vec![],
        });
    }

    fn write_suggestions(&mut self, _word: &str, suggestions: &[Suggestion]) {
        let i = self.results.len() - 1;
        self.results[i].suggestions = suggestions.to_vec();
    }

    fn finish(&mut self) {
        println!("{}", serde_json::to_string_pretty(self).unwrap());
    }
}

fn run<T: Transducer, U: Transducer>(
    speller: Arc<Speller<T, U>>,
    words: Vec<String>,
    writer: &mut dyn OutputWriter,
    is_suggesting: bool,
    is_always_suggesting: bool,
    suggest_cfg: &SpellerConfig,
) {
    for word in words {
        let is_correct = speller.clone().is_correct(&word);
        writer.write_correction(&word, is_correct);

        if is_suggesting && (is_always_suggesting || !is_correct) {
            let suggestions = speller.clone().suggest_with_config(&word, &suggest_cfg);
            writer.write_suggestions(&word, &suggestions);
        }
    }
}

fn main() {
    let matches = App::new("divvunspell")
        .setting(AppSettings::ArgRequiredElseHelp)
        .version(env!("CARGO_PKG_VERSION"))
        .about("Testing frontend for the DivvunSpell library")
        .arg(
            Arg::with_name("zhfst")
                .short("z")
                .long("zhfst")
                .value_name("ZHFST")
                .help("Use the given ZHFST file")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("bhfst")
                .short("b")
                .long("bhfst")
                .value_name("BHFST")
                .help("Use the given BHFST file")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("acceptor")
                .long("acceptor")
                .value_name("acceptor")
                .requires("errmodel")
                .help("Use the given acceptor file")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("errmodel")
                .long("errmodel")
                .value_name("errmodel")
                .requires("acceptor")
                .help("Use the given errmodel file")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("suggest")
                .short("s")
                .long("suggest")
                .help("Show suggestions for given word(s)"),
        )
        .arg(
            Arg::with_name("always-suggest")
                .short("S")
                .long("always-suggest")
                .help("Always show suggestions even if word is correct (implies -s)"),
        )
        .arg(
            Arg::with_name("weight")
                .short("w")
                .long("weight")
                .requires("suggest")
                .takes_value(true)
                .help("Maximum weight limit for suggestions"),
        )
        .arg(
            Arg::with_name("nbest")
                .short("n")
                .long("nbest")
                .requires("suggest")
                .takes_value(true)
                .help("Maximum number of results for suggestions"),
        )
        .arg(
            Arg::with_name("json")
                .long("json")
                .help("Output results in JSON"),
        )
        .arg(
            Arg::with_name("WORDS")
                .multiple(true)
                .help("The words to be processed"),
        )
        .group(
            ArgGroup::with_name("archive")
                .args(&["zhfst", "bhfst", "acceptor"])
                .required(true),
        )
        .get_matches();

    let is_always_suggesting = matches.is_present("always-suggest");
    let is_suggesting = matches.is_present("suggest") || is_always_suggesting;
    let is_json = matches.is_present("json");

    let n_best = matches
        .value_of("nbest")
        .and_then(|v| v.parse::<usize>().ok());
    let max_weight = matches
        .value_of("weight")
        .and_then(|v| v.parse::<f32>().ok());

    let words: Vec<String> = match matches.values_of("WORDS") {
        Some(v) => v.map(|x| x.to_string()).collect(),
        None => {
            eprintln!("Reading from stdin...");
            let mut buffer = String::new();
            io::stdin()
                .read_to_string(&mut buffer)
                .expect("reading stdin");
            buffer.words().map(|x| x.to_string()).collect()
        }
    };

    let mut writer: Box<dyn OutputWriter> = if is_json {
        Box::new(JsonWriter::new())
    } else {
        Box::new(StdoutWriter)
    };

    let suggest_cfg = SpellerConfig {
        max_weight,
        n_best,
        beam: None,
        pool_max: 128,
        pool_start: 128,
        seen_node_sample_rate: 20,
        with_caps: true,
    };

    if let Some(zhfst_file) = matches.value_of("zhfst") {
        let archive = match ZipSpellerArchive::new(zhfst_file) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{:?}", e);
                std::process::exit(1);
            }
        };

        let speller = archive.speller();
        run(
            speller,
            words,
            &mut *writer,
            is_suggesting,
            is_always_suggesting,
            &suggest_cfg,
        );
    } else if let Some(bhfst_file) = matches.value_of("bhfst") {
        let archive: BoxSpellerArchive<ThfstTransducer, ThfstTransducer> =
            match BoxSpellerArchive::new(bhfst_file) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{:?}", e);
                    std::process::exit(1);
                }
            };

        let speller = archive.speller();
        run(
            speller,
            words,
            &mut *writer,
            is_suggesting,
            is_always_suggesting,
            &suggest_cfg,
        );
    } else {
        match (matches.value_of("acceptor"), matches.value_of("errmodel")) {
            (Some(acceptor_file), Some(errmodel_file)) => {
                let fs = divvunspell::util::Fs;
                let acceptor = ThfstTransducer::from_path(&fs, acceptor_file).unwrap();
                let errmodel = ThfstTransducer::from_path(&fs, errmodel_file).unwrap();
                let speller = Speller::new(errmodel, acceptor);

                run(
                    speller,
                    words,
                    &mut *writer,
                    is_suggesting,
                    is_always_suggesting,
                    &suggest_cfg,
                );
            }
            _ => {
                eprintln!("No acceptor or errmodel");
                std::process::exit(1);
            }
        }
    }

    writer.finish();
}
