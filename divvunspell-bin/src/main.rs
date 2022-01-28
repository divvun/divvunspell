use std::io::{self, Read};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use gumdrop::Options;
use serde::Serialize;

#[cfg(feature = "gpt2")]
use divvunspell::archive::{
    boxf::BoxGpt2PredictorArchive, error::PredictorArchiveError, PredictorArchive,
};

use divvunspell::{
    archive::{
        boxf::ThfstBoxSpellerArchive, error::SpellerArchiveError, BoxSpellerArchive,
        SpellerArchive, ZipSpellerArchive,
    },
    speller::{suggestion::Suggestion, Speller, SpellerConfig},
    tokenizer::Tokenize,
};

trait OutputWriter {
    fn write_correction(&mut self, word: &str, is_correct: bool);
    fn write_suggestions(&mut self, word: &str, suggestions: &[Suggestion]);
    fn write_predictions(&mut self, predictions: &[String]);
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

    fn write_predictions(&mut self, predictions: &[String]) {
        println!("Predictions: ");
        println!("{}", predictions.join(" "));
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
    suggest: Vec<SuggestionRequest>,
    predict: Option<Vec<String>>,
}

impl JsonWriter {
    pub fn new() -> JsonWriter {
        JsonWriter {
            suggest: vec![],
            predict: None,
        }
    }
}

impl OutputWriter for JsonWriter {
    fn write_correction(&mut self, word: &str, is_correct: bool) {
        self.suggest.push(SuggestionRequest {
            word: word.to_owned(),
            is_correct,
            suggestions: vec![],
        });
    }

    fn write_suggestions(&mut self, _word: &str, suggestions: &[Suggestion]) {
        let i = self.suggest.len() - 1;
        self.suggest[i].suggestions = suggestions.to_vec();
    }

    fn write_predictions(&mut self, predictions: &[String]) {
        self.predict = Some(predictions.to_vec());
    }

    fn finish(&mut self) {
        println!("{}", serde_json::to_string_pretty(self).unwrap());
    }
}

fn run(
    speller: Arc<dyn Speller + Send>,
    words: Vec<String>,
    writer: &mut dyn OutputWriter,
    is_suggesting: bool,
    is_always_suggesting: bool,
    suggest_cfg: &SpellerConfig,
) {
    for word in words {
        let is_correct = speller.clone().is_correct_with_config(&word, &suggest_cfg);
        writer.write_correction(&word, is_correct);

        if is_suggesting && (is_always_suggesting || !is_correct) {
            let suggestions = speller.clone().suggest_with_config(&word, &suggest_cfg);
            writer.write_suggestions(&word, &suggestions);
        }
    }
}
#[derive(Debug, Options)]
struct Args {
    #[options(help = "print help message")]
    help: bool,

    #[options(command)]
    command: Option<Command>,
}

#[derive(Debug, Options)]
enum Command {
    #[options(help = "get suggestions for provided input")]
    Suggest(SuggestArgs),

    #[options(help = "print input in word-separated tokenized form")]
    Tokenize(TokenizeArgs),

    #[options(help = "predict next words using GPT2 model")]
    Predict(PredictArgs),
}

#[derive(Debug, Options)]
struct SuggestArgs {
    #[options(help = "print help message")]
    help: bool,

    #[options(help = "BHFST or ZHFST archive to be used", required)]
    archive: PathBuf,

    #[options(short = "S", help = "always show suggestions even if word is correct")]
    always_suggest: bool,

    #[options(help = "maximum weight limit for suggestions")]
    weight: Option<f32>,

    #[options(help = "maximum number of results")]
    nbest: Option<usize>,

    #[options(
        no_short,
        long = "no-case-handling",
        help = "disables case-handling algorithm (makes results more like hfst-ospell)"
    )]
    disable_case_handling: bool,

    #[options(no_short, long = "json", help = "output in JSON format")]
    use_json: bool,

    #[options(free, help = "words to be processed")]
    inputs: Vec<String>,
}

#[derive(Debug, Options)]
struct TokenizeArgs {
    #[options(help = "print help message")]
    help: bool,

    #[options(short = "w", long = "words", help = "show words only")]
    is_words_only: bool,

    #[options(free, help = "text to be tokenized")]
    inputs: Vec<String>,
}

#[derive(Debug, Options)]
struct PredictArgs {
    #[options(help = "print help message")]
    help: bool,

    #[options(help = "BHFST archive to be used", required)]
    archive: PathBuf,

    #[options(
        short = "n",
        long = "name",
        help = "Predictor name to use (default: gpt2_predictor)"
    )]
    predictor_name: Option<String>,

    #[options(help = "whether suggestions should not be validated against a speller")]
    disable_spelling_validation: bool,

    #[options(no_short, long = "json", help = "output in JSON format")]
    use_json: bool,

    #[options(free, help = "text to be tokenized")]
    inputs: Vec<String>,
}

fn tokenize(args: TokenizeArgs) -> anyhow::Result<()> {
    let inputs: String = if args.inputs.is_empty() {
        eprintln!("Reading from stdin...");
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .expect("reading stdin");
        buffer
    } else {
        args.inputs.into_iter().collect::<Vec<_>>().join(" ")
    };

    if args.is_words_only {
        for (index, token) in inputs.word_indices() {
            println!("{:>4}: \"{}\"", index, token);
        }
    } else {
        for (index, token) in inputs.word_bound_indices() {
            println!("{:>4}: \"{}\"", index, token);
        }
    }

    Ok(())
}

fn load_archive(path: &Path) -> Result<Box<dyn SpellerArchive>, SpellerArchiveError> {
    let ext = match path.extension() {
        Some(v) => v,
        None => {
            return Err(SpellerArchiveError::Io(
                path.to_string_lossy().to_string(),
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Unsupported archive (missing .zhfst or .bhfst)",
                )
                .into(),
            ))
        }
    };

    if ext == "bhfst" {
        let archive: ThfstBoxSpellerArchive = match BoxSpellerArchive::open(path) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{:?}", e);
                std::process::exit(1);
            }
        };
        Ok(Box::new(archive))
    } else if ext == "zhfst" {
        let archive = match ZipSpellerArchive::open(path) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{:?}", e);
                std::process::exit(1);
            }
        };
        Ok(Box::new(archive))
    } else {
        Err(SpellerArchiveError::Io(
            path.to_string_lossy().to_string(),
            std::io::Error::new(
                std::io::ErrorKind::Other,
                "Unsupported archive (missing .zhfst or .bhfst)",
            )
            .into(),
        ))
    }
}

fn suggest(args: SuggestArgs) -> anyhow::Result<()> {
    let mut suggest_cfg = SpellerConfig::default();

    if args.disable_case_handling {
        suggest_cfg.case_handling = None;
    }

    if let Some(v) = args.nbest {
        if v == 0 {
            suggest_cfg.n_best = None;
        } else {
            suggest_cfg.n_best = Some(v);
        }
    }

    if let Some(v) = args.weight.filter(|x| x >= &0.0) {
        if v == 0.0 {
            suggest_cfg.max_weight = None;
        } else {
            suggest_cfg.max_weight = Some(v);
        }
    }

    let mut writer: Box<dyn OutputWriter> = if args.use_json {
        Box::new(JsonWriter::new())
    } else {
        Box::new(StdoutWriter)
    };

    let words = if args.inputs.is_empty() {
        eprintln!("Reading from stdin...");
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .expect("reading stdin");
        buffer
            .trim()
            .split('\n')
            .map(|x| x.trim().to_string())
            .collect()
    } else {
        args.inputs.into_iter().collect()
    };

    let archive = load_archive(&args.archive)?;
    let speller = archive.speller();
    run(
        speller,
        words,
        &mut *writer,
        true,
        args.always_suggest,
        &suggest_cfg,
    );

    writer.finish();

    Ok(())
}

#[cfg(feature = "gpt2")]
fn load_predictor_archive(
    path: &Path,
    name: Option<&str>,
) -> Result<Box<dyn PredictorArchive>, PredictorArchiveError> {
    let archive = BoxGpt2PredictorArchive::open(path, name)?;
    let archive = Box::new(archive);
    Ok(archive)
}

#[cfg(feature = "gpt2")]
fn predict(args: PredictArgs) -> anyhow::Result<()> {
    let raw_input = if args.inputs.is_empty() {
        eprintln!("Reading from stdin...");
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .expect("reading stdin");
        buffer
    } else {
        args.inputs.join(" ")
    };

    let predictor_name = args.predictor_name.as_deref();
    let archive = load_predictor_archive(&args.archive, predictor_name)?;
    let predictor = archive.predictor();

    let mut writer: Box<dyn OutputWriter> = if args.use_json {
        Box::new(JsonWriter::new())
    } else {
        Box::new(StdoutWriter)
    };

    let suggest_cfg = SpellerConfig::default();

    let predictions = predictor.predict(&raw_input);
    writer.write_predictions(&predictions);

    let has_speller = archive.metadata().map(|x| x.speller).unwrap_or(false);
    if !args.disable_spelling_validation {
        if !has_speller {
            eprintln!("Error: requested spell checking but no speller present in archive!");
        } else {
            let speller_archive = load_archive(&args.archive)?;
            let speller = speller_archive.speller();

            for word in predictions {
                let cleaned_str = word.as_str().word_indices();
                for w in cleaned_str {
                    let is_correct = speller.clone().is_correct_with_config(&w.1, &suggest_cfg);
                    writer.write_correction(w.1, is_correct);
                }
            }
        }
    };

    Ok(())
}

#[cfg(not(feature = "gpt2"))]
fn predict(_args: PredictArgs) -> anyhow::Result<()> {
    eprintln!("ERROR: DivvunSpell was built without GPT2 support.");
    eprintln!("If you built this using cargo, re-run the build with the following:");
    eprintln!("");
    eprintln!("  cargo build --features gpt2");
    eprintln!("");

    std::process::exit(1);
}

fn main() -> anyhow::Result<()> {
    pretty_env_logger::init();

    let args = Args::parse_args_default_or_exit();

    match args.command {
        None => Ok(()),
        Some(Command::Suggest(args)) => suggest(args),
        Some(Command::Tokenize(args)) => tokenize(args),
        Some(Command::Predict(args)) => predict(args),
    }
}
