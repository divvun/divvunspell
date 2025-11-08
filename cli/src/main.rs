use std::io::{self, Read};
use std::process;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use clap::{Parser, Subcommand};
use divvun_fst::speller::HfstSpeller;
use divvun_fst::transducer::TransducerLoader;
use divvun_fst::transducer::hfst::HfstTransducer;
use divvun_fst::types::Weight;
use divvun_fst::vfs::Fs;
use serde::Serialize;

use divvun_fst::{
    archive::{
        SpellerArchive, boxf::BoxSpellerArchive, boxf::ThfstBoxSpellerArchive,
        error::SpellerArchiveError, zip::ZipSpellerArchive,
    },
    speller::{Speller, SpellerConfig, suggestion::Suggestion},
    tokenizer::Tokenize,
};

trait OutputWriter {
    fn write_correction(&mut self, word: &str, is_correct: bool);
    fn write_suggestions(&mut self, word: &str, suggestions: &[Suggestion]);
    fn write_input_analyses(&mut self, word: &str, analyses: &[Suggestion]);
    fn write_output_analyses(&mut self, word: &str, analyses: &[Suggestion]);
    fn finish(&mut self);
}

struct StdoutWriter {
    has_continuation_marker: Option<String>,
}

impl OutputWriter for StdoutWriter {
    fn write_correction(&mut self, word: &str, is_correct: bool) {
        println!(
            "Input: {}\t\t[{}]",
            &word,
            if is_correct { "CORRECT" } else { "INCORRECT" }
        );
    }

    fn write_suggestions(&mut self, _word: &str, suggestions: &[Suggestion]) {
        if let Some(s) = &self.has_continuation_marker {
            for sugg in suggestions {
                print!("{}", sugg.value);
                if sugg.completed == Some(true) {
                    print!("{s}");
                }
                println!("\t\t{}", sugg.weight);
            }
        } else {
            for sugg in suggestions {
                println!("{}\t\t{}", sugg.value, sugg.weight);
            }
        }
        println!();
    }

    fn write_input_analyses(&mut self, _word: &str, suggestions: &[Suggestion]) {
        println!("Input analyses: ");
        for sugg in suggestions {
            println!("{}\t\t{}", sugg.value, sugg.weight);
        }
        println!();
    }

    fn write_output_analyses(&mut self, _word: &str, suggestions: &[Suggestion]) {
        println!("Output analyses: ");
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
struct AnalysisRequest {
    word: String,
    suggestions: Vec<Suggestion>,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonWriter {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    suggest: Vec<SuggestionRequest>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    input_analysis: Vec<AnalysisRequest>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    output_analysis: Vec<AnalysisRequest>,
}

impl JsonWriter {
    pub fn new() -> JsonWriter {
        Self::default()
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

    fn write_input_analyses(&mut self, word: &str, suggestions: &[Suggestion]) {
        self.input_analysis.push(AnalysisRequest {
            word: word.to_string(),
            suggestions: suggestions.to_vec(),
        })
    }

    fn write_output_analyses(&mut self, word: &str, suggestions: &[Suggestion]) {
        self.output_analysis.push(AnalysisRequest {
            word: word.to_string(),
            suggestions: suggestions.to_vec(),
        })
    }

    fn finish(&mut self) {
        println!("{}", serde_json::to_string_pretty(self).unwrap());
    }
}

fn run(
    speller: Arc<dyn Speller + Send>,
    words: Vec<String>,
    writer: &mut dyn OutputWriter,
    is_analyzing: bool,
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

        if is_analyzing {
            let input_analyses = speller
                .clone()
                .analyze_input_with_config(&word, &suggest_cfg);
            writer.write_input_analyses(&word, &input_analyses);

            let output_analyses = speller
                .clone()
                .analyze_output_with_config(&word, &suggest_cfg);
            writer.write_output_analyses(&word, &output_analyses);

            let final_suggs = speller
                .clone()
                .analyze_suggest_with_config(&word, &suggest_cfg);
            writer.write_suggestions(&word, &final_suggs);
        }
    }
}
#[derive(Debug, Parser)]
#[command(
    name = "divvunspell",
    about = "Spell checking tool for ZHFST/BHFST spellers"
)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Get suggestions for provided input
    Suggest(SuggestArgs),

    /// Print input in word-separated tokenized form
    Tokenize(TokenizeArgs),
}

#[derive(Debug, Parser)]
struct SuggestArgs {
    /// BHFST or ZHFST archive to be used
    #[arg(short = 'a', long = "archive")]
    archive_path: Option<PathBuf>,

    /// Mutator to use (if archive not provided)
    #[arg(long)]
    mutator_path: Option<PathBuf>,

    /// Lexicon to use (if archive not provided)
    #[arg(long)]
    lexicon_path: Option<PathBuf>,

    /// Always show suggestions even if word is correct
    #[arg(short = 'S', long = "always-suggest")]
    always_suggest: bool,

    /// Analyze words and suggestions
    #[arg(short = 'A', long)]
    analyze: bool,

    /// Maximum weight limit for suggestions
    #[arg(short = 'w', long)]
    weight: Option<f32>,

    /// Maximum number of results
    #[arg(short = 'n', long)]
    nbest: Option<usize>,

    /// Character for incomplete suggestions
    #[arg(long)]
    continuation_marker: Option<String>,

    /// Disables reweighting algorithm (makes results more like hfst-ospell)
    #[arg(long = "no-reweighting")]
    disable_reweight: bool,

    /// Disables recasing algorithm (makes results more like hfst-ospell)
    #[arg(long = "no-recase")]
    disable_recase: bool,

    /// Uses supplied config file
    #[arg(short = 'c', long)]
    config: Option<PathBuf>,

    /// Output in JSON format
    #[arg(long)]
    json: bool,

    /// Words to be processed
    inputs: Vec<String>,
}

#[derive(Debug, Parser)]
struct TokenizeArgs {
    /// Show words only
    #[arg(short = 'w', long = "words")]
    is_words_only: bool,

    /// Text to be tokenized
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
            ));
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
    // 1. default config
    let mut suggest_cfg = SpellerConfig::default();

    let speller = if let Some(archive_path) = args.archive_path {
        let archive = load_archive(&archive_path)?;
        // 2. config from metadata
        if let Some(metadata) = archive.metadata() {
            if let Some(continuation) = metadata.acceptor().continuation() {
                suggest_cfg.completion_marker = Some(continuation.to_string());
            }
        }
        let speller = archive.speller();
        speller
    } else if let (Some(lexicon_path), Some(mutator_path)) = (args.lexicon_path, args.mutator_path)
    {
        let acceptor = HfstTransducer::from_path(&Fs, lexicon_path)?;
        let errmodel = HfstTransducer::from_path(&Fs, mutator_path)?;
        HfstSpeller::new(errmodel, acceptor) as _
    } else {
        eprintln!("Either a BHFST or ZHFST archive must be provided, or a mutator and lexicon.");
        process::exit(1);
    };
    // 3. config from explicit config file
    if let Some(config_path) = args.config {
        let config_file = std::fs::File::open(config_path)?;
        let config: SpellerConfig = serde_json::from_reader(config_file)?;
        suggest_cfg = config;
    }
    // 4. config from other command line stuff
    if args.disable_reweight {
        suggest_cfg.reweight = None;
    }
    if args.disable_recase {
        suggest_cfg.recase = false;
    }
    suggest_cfg.completion_marker = args.continuation_marker.clone();
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
            suggest_cfg.max_weight = Some(Weight(v));
        }
    }

    let mut writer: Box<dyn OutputWriter> = if args.json {
        Box::new(JsonWriter::new())
    } else {
        Box::new(StdoutWriter {
            has_continuation_marker: args.continuation_marker,
        })
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

    run(
        speller,
        words,
        &mut *writer,
        args.analyze,
        true,
        args.always_suggest,
        &suggest_cfg,
    );

    writer.finish();

    Ok(())
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    match args.command {
        None => Ok(()),
        Some(Command::Suggest(args)) => suggest(args),
        Some(Command::Tokenize(args)) => tokenize(args),
    }
}
