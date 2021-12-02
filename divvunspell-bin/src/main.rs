use std::io::{self, Read};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use gumdrop::Options;
use serde::Serialize;

use divvunspell::archive::{
    boxf::ThfstBoxSpellerArchive, error::SpellerArchiveError, BoxSpellerArchive, SpellerArchive,
    ZipSpellerArchive,
};
use divvunspell::speller::suggestion::{Suggestion, AISuggestion};
use divvunspell::speller::{Speller, SpellerConfig};
use divvunspell::tokenizer::Tokenize;
use divvunspell::ml_speller;
use rust_bert::pipelines::text_generation::{TextGenerationModel};


trait AIOutputWriter {
    fn write_correction(&mut self, word: &str, is_correct: bool);
    fn write_ai_suggestions(&mut self, word: &str, suggestions: &[AISuggestion]);
    fn finish(&mut self);
}

trait OutputWriter {
    fn write_correction(&mut self, word: &str, is_correct: bool);
    fn write_suggestions(&mut self, word: &str, suggestions: &[Suggestion]);
    fn finish(&mut self);
}

struct StdoutWriter;
struct AIStdoutWriter;

impl AIOutputWriter for AIStdoutWriter {
    fn write_correction(&mut self, word: &str, is_correct: bool) {
        println!(
            "Input: {}\t\t[{}]",
            &word,
            if is_correct { "CORRECT" } else { "INCORRECT" }
        );
    }

    fn write_ai_suggestions(&mut self, _word: &str, suggestions: &[AISuggestion]) {
        for sugg in suggestions {
            println!("Completed: {}\t\t", sugg.value);
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
struct AISuggestionRequest {
    word: String,
    is_correct: bool,
    suggestions: Vec<AISuggestion>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AIJsonWriter {
    results: Vec<AISuggestionRequest>,
}
struct JsonWriter {
    results: Vec<SuggestionRequest>,
}

impl JsonWriter {
    pub fn new() -> JsonWriter {
        JsonWriter { results: vec![] }
    }
}

impl AIJsonWriter {
    pub fn new() -> AIJsonWriter {
        AIJsonWriter { results: vec![] }
    }
}
impl AIOutputWriter for AIJsonWriter {
    fn write_correction(&mut self, word: &str, is_correct: bool) {
        self.results.push(AISuggestionRequest {
            word: word.to_owned(),
            is_correct,
            suggestions: vec![],
        });
    }

    // fn write_suggestions(&mut self, _word: &str, suggestions: &[Suggestion]) {
    //     let i = self.results.len() - 1;
    //     self.results[i].suggestions = suggestions.to_vec();
    // }

    fn write_ai_suggestions(&mut self, word: &str, suggestions: &[AISuggestion]) {
        let i = self.results.len() - 1;
        self.results[i].suggestions = suggestions.to_vec();
    }   
    

    fn finish(&mut self) {
        println!("{}", serde_json::to_string_pretty(self).unwrap());
    }
}

fn run_ai(
    model: TextGenerationModel,
    words: Vec<String>,
    writer: &mut dyn AIOutputWriter, 
    speller: Arc<dyn Speller + Send>,
    suggest_cfg: &SpellerConfig,
) {
    for word in words {
        let suggestions = ml_speller::gpt2::generate_suggestions(&model, &word);
        // println!("{:?}", suggestions);
        writer.write_ai_suggestions(&word, &suggestions);
        let is_correct = speller.clone().is_correct_with_config(&word, &suggest_cfg);
        writer.write_correction(&word, is_correct);

        for s in suggestions{
            
            for w in s.value.split_whitespace() {
                let is_correct = speller.clone().is_correct_with_config(&w, &suggest_cfg);

            writer.write_correction(&w, is_correct);
        }}
    }

}

// fn run(
//     speller: Arc<dyn Speller + Send>,
//     words: Vec<String>,
//     writer: &mut dyn OutputWriter,
//     is_suggesting: bool,
//     is_always_suggesting: bool,
//     suggest_cfg: &SpellerConfig,
// ) {
//     for word in words {
//         let is_correct = speller.clone().is_correct_with_config(&word, &suggest_cfg);
//         writer.write_correction(&word, is_correct);

//         if is_suggesting && (is_always_suggesting || !is_correct) {
//             let suggestions = speller.clone().suggest_with_config(&word, &suggest_cfg);
//             writer.write_suggestions(&word, &suggestions);
//         }
//     }
// }

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
    Suggest(SuggestAIArgs),

    #[options(help = "print input in word-separated tokenized form")]
    Tokenize(TokenizeArgs),
}

#[derive(Debug, Options)]
struct SuggestAIArgs {
    #[options(help = "print help message")]
    help: bool,
    #[options(free, help = "words to be processed")]
    inputs: Vec<String>,
    #[options(no_short, long = "json", help = "output in JSON format")]
    use_json: bool,
    #[options(help = "BHFST or ZHFST archive to be used", required)]
    archive: PathBuf,

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
                ),
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
            ),
        ))
    }
}


fn suggest(args: SuggestAIArgs) -> anyhow::Result<()> {
    let mut suggest_cfg = SpellerConfig::default();

    // if args.disable_case_handling {
    //     suggest_cfg.case_handling = None;
    // }

    // if let Some(v) = args.nbest {
    //     if v == 0 {
    //         suggest_cfg.n_best = None;
    //     } else {
    //         suggest_cfg.n_best = Some(v);
    //     }
    // }

    // if let Some(v) = args.weight.filter(|x| x >= &0.0) {
    //     if v == 0.0 {
    //         suggest_cfg.max_weight = None;
    //     } else {
    //         suggest_cfg.max_weight = Some(v);
    //     }
    // }

    let mut writer: Box<dyn AIOutputWriter> = if args.use_json {
        Box::new(AIJsonWriter::new())
    } else {
        Box::new(AIStdoutWriter)
    };

    let words = if args.inputs.is_empty() {
        eprintln!("Reading from stdin...");
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .expect("reading stdin");
        buffer
            .trim()
            .split("\n")
            .map(|x| x.trim().to_string())
            .collect()
    } else {
        args.inputs.into_iter().map(|x| x.to_string()).collect()
    };

    let archive = load_archive(&args.archive).unwrap();
    let model = ml_speller::gpt2::load_mlmodel().unwrap();
    let speller = archive.speller();
   
    run_ai(
        model,
        vec![words],
        &mut *writer,
        speller,
        &suggest_cfg,
        
        
    );

    writer.finish();

    Ok(())
}

fn main() -> anyhow::Result<()> {
    pretty_env_logger::init();

    let args = Args::parse_args_default_or_exit();

    match args.command {
        None => Ok(()),
        Some(Command::Suggest(args)) => suggest(args),
        Some(Command::Tokenize(args)) => tokenize(args),
    }
}
