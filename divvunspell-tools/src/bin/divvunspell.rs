use std::io::{self, Read};
use std::sync::Arc;

use clap::{App, AppSettings, Arg, ArgGroup};
use serde::Serialize;

use divvunspell::archive::{boxf::ThfstBoxSpellerArchive, BoxSpellerArchive, ZipSpellerArchive};
use divvunspell::speller::suggestion::Suggestion;
use divvunspell::speller::{Speller, SpellerConfig};
use divvunspell::transducer::{thfst::MemmapThfstTransducer, Transducer};
use divvunspell::vfs;

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

fn run<F: vfs::File, T: Transducer<F>, U: Transducer<F>>(
    speller: Arc<Speller<F, T, U>>,
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
    pretty_env_logger::init();

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
            Arg::with_name("no-case-handling")
                .long("no-case-handling")
                .help("Disables case handling"),
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
    let no_case_handling = matches.is_present("no-case-handling");

    let mut suggest_cfg = SpellerConfig::default();

    if no_case_handling {
        suggest_cfg.case_handling = None;
    }

    if let Some(v) = matches.value_of("nbest") {
        if let Ok(v) = v.parse::<usize>() {
            if v == 0 {
                suggest_cfg.n_best = None;
            } else {
                suggest_cfg.n_best = Some(v);
            }
        }
    }

    if let Some(v) = matches.value_of("weight") {
        if let Some(v) = v.parse::<f32>().ok().filter(|x| x >= &0.0) {
            if v == 0.0 {
                suggest_cfg.max_weight = None;
            } else {
                suggest_cfg.max_weight = Some(v);
            }
        }
    }

    let mut writer: Box<dyn OutputWriter> = if is_json {
        Box::new(JsonWriter::new())
    } else {
        Box::new(StdoutWriter)
    };

    let words: Vec<String> = match matches.values_of("WORDS") {
        Some(v) => v.map(|x| x.to_string()).collect(),
        None => {
            eprintln!("Reading from stdin...");
            let mut buffer = String::new();
            io::stdin()
                .read_to_string(&mut buffer)
                .expect("reading stdin");
            buffer.split(" ").map(|x| x.trim().to_string()).collect()
        }
    };

    if let Some(zhfst_file) = matches.value_of("zhfst") {
        let archive = match ZipSpellerArchive::open(zhfst_file) {
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
        let archive: ThfstBoxSpellerArchive = match BoxSpellerArchive::open(bhfst_file) {
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
                let fs = divvunspell::vfs::Fs;
                let acceptor = MemmapThfstTransducer::from_path(&fs, acceptor_file).unwrap();
                let errmodel = MemmapThfstTransducer::from_path(&fs, errmodel_file).unwrap();
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
