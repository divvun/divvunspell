/*! Accuracy testing for Finite-State Spell-Checkers

A tool to help testing quality of finite-state spell-checkers. Shows precision
and recall and F scores.

# Usage examples

It's a command-line tool:
```console
$ cargo run -- typos.txt se.zhfst
```
will produce statistics of spelling corrections.

It is possible to fine-tune the options using a configuration file in json
format. The format of json file follows from the [`SpellerConfig`] definition in
the main library:
```console
$ cargo run -- --config config.json typos.txt se.zhfst
```
For automated testing in CI there is a --threshold parametre:
```console
$ cargo run -- --threshold 0.9 typos.txt se.zhfst
```
*/

use chrono::prelude::*;
use divvunspell::types::Weight;
use std::error::Error;
use std::{
    io::Write,
    path::Path,
    time::{Instant, SystemTime},
};

use clap::Parser;
use distance::damerau_levenshtein;
use divvunspell::archive;
use divvunspell::speller::suggestion::Suggestion;
use divvunspell::speller::{ReweightingConfig, SpellerConfig};
use indicatif::{ParallelProgressIterator, ProgressBar, ProgressStyle};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use serde::Serialize;
use std::path::PathBuf;

static CFG: SpellerConfig = SpellerConfig {
    n_best: Some(10),
    max_weight: Some(Weight(10000.0)),
    beam: None,
    reweight: Some(ReweightingConfig::default_const()),
    node_pool_size: 128,
    recase: true,
    completion_marker: None,
};

fn load_words(
    path: &str,
    max_words: Option<usize>,
) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let mut rdr = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .delimiter(b'\t')
        .has_headers(false)
        .flexible(true)
        .from_path(path)?;

    Ok(rdr
        .records()
        .filter_map(Result::ok)
        .filter_map(|r| {
            r.get(0)
                .and_then(|x| r.get(1).map(|y| (x.to_string(), y.to_string())))
        })
        .take(max_words.unwrap_or(std::usize::MAX))
        .collect())
}

#[derive(Debug, Default, Serialize, PartialOrd, Ord, PartialEq, Eq, Clone, Copy)]
struct Time {
    secs: u64,
    subsec_nanos: u32,
}

impl std::fmt::Display for Time {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        let ms = self.secs * 1000 + (self.subsec_nanos as u64 / 1_000_000);
        write!(f, "{}ms", ms)
    }
}

#[derive(Debug, Serialize)]
struct AccuracyResult<'a> {
    input: &'a str,
    expected: &'a str,
    distance: usize,
    suggestions: Vec<Suggestion>,
    position: Option<usize>,
    time: Time,
}

#[derive(Debug, Serialize)]
struct Report<'a> {
    metadata: Option<&'a divvunspell::archive::meta::SpellerMetadata>,
    config: &'a SpellerConfig,
    summary: Summary,
    results: Vec<AccuracyResult<'a>>,
    start_timestamp: Time,
    total_time: Time,
}

#[derive(Serialize, Default, Debug, Clone)]
struct Summary {
    total_words: u32,
    first_position: u32,
    top_five: u32,
    any_position: u32,
    no_suggestions: u32,
    only_wrong: u32,
    slowest_lookup: Time,
    fastest_lookup: Time,
    average_time: Time,
    average_time_95pc: Time,
    average_position_of_correct: f32,
    average_suggestions_for_correct: f32,
}

impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        let percent =
            |v: u32| -> String { format!("{:.2}%", v as f32 / self.total_words as f32 * 100f32) };

        write!(
            f,
            "[#1] {} [^5] {} [any] {} [none] {} [wrong] {} [fast] {} [slow] {}",
            percent(self.first_position),
            percent(self.top_five),
            percent(self.any_position),
            percent(self.no_suggestions),
            percent(self.only_wrong),
            self.fastest_lookup,
            self.slowest_lookup
        )
    }
}

impl Summary {
    fn new<'a>(results: &[AccuracyResult<'a>]) -> Summary {
        let mut summary = Summary::default();

        results.iter().for_each(|result| {
            summary.total_words += 1;

            if let Some(position) = result.position {
                summary.any_position += 1;

                if position == 0 {
                    summary.first_position += 1;
                }

                if position < 5 {
                    summary.top_five += 1;
                }
            } else if result.suggestions.is_empty() {
                summary.no_suggestions += 1;
            } else {
                summary.only_wrong += 1;
            }
        });

        summary.slowest_lookup = results
            .iter()
            .max_by(|x, y| x.time.cmp(&y.time))
            .unwrap()
            .time;
        summary.fastest_lookup = results
            .iter()
            .min_by(|x, y| x.time.cmp(&y.time))
            .unwrap()
            .time;

        // Calculate average position and average suggestions for correct results only
        let correct_results: Vec<_> = results
            .iter()
            .filter(|r| r.position.is_some())
            .collect();
        
        if !correct_results.is_empty() {
            let total_position: usize = correct_results
                .iter()
                .map(|r| r.position.unwrap())
                .sum();
            summary.average_position_of_correct = 
                total_position as f32 / correct_results.len() as f32;
            
            let total_suggestions: usize = correct_results
                .iter()
                .map(|r| r.suggestions.len())
                .sum();
            summary.average_suggestions_for_correct = 
                total_suggestions as f32 / correct_results.len() as f32;
        }

        summary
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "divvunspell-accuracy",
    version,
    about = "Accuracy testing for DivvunSpell."
)]
struct Args {
    /// Provide JSON config file to override test defaults
    #[arg(short = 'c', long)]
    config: Option<PathBuf>,

    /// The 'input -> expected' list in tab-delimited value file (TSV)
    words: Option<String>,

    /// Use the given ZHFST file
    zhfst: Option<String>,

    /// The file path for the JSON report output
    #[arg(short = 'o', long = "json-output")]
    json_output: Option<String>,

    /// The file path for the TSV line append
    #[arg(short = 't', long = "tsv-output")]
    tsv_output: Option<String>,

    /// Truncate typos list to max number of words specified
    #[arg(short = 'w', long = "max-words")]
    max_words: Option<usize>,

    /// Minimum precision @ 5 for automated testing
    #[arg(short = 'T', long)]
    threshold: Option<f32>,
}

fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let cfg: SpellerConfig = match args.config {
        Some(path) => {
            let file = std::fs::File::open(path)?;
            serde_json::from_reader(file)?
        }
        None => CFG.clone(),
    };

    let archive = match args.zhfst {
        Some(path) => archive::open(Path::new(&path))?,
        None => {
            eprintln!("No ZHFST found for given path; aborting.");
            std::process::exit(1);
        }
    };

    let words = match args.words {
        Some(path) => load_words(&path, args.max_words)?,
        None => {
            eprintln!("No word list for given path; aborting.");
            std::process::exit(1);
        }
    };

    let pb = ProgressBar::new(words.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{pos}/{len} [{percent}%] {wide_bar} {elapsed_precise}"),
    );

    let start_time = Instant::now();
    let results = words
        .par_iter()
        .progress_with(pb)
        .map(|(input, expected)| {
            let now = Instant::now();
            let suggestions = archive.speller().suggest_with_config(&input, &cfg);
            let now = now.elapsed();

            let time = Time {
                secs: now.as_secs(),
                subsec_nanos: now.subsec_nanos(),
            };

            let position = suggestions.iter().position(|x| x.value == expected);

            let distance = damerau_levenshtein(input, expected);
            AccuracyResult {
                input,
                expected,
                distance,
                time,
                suggestions,
                position,
            }
        })
        .collect::<Vec<_>>();

    let now = start_time.elapsed();
    let total_time = Time {
        secs: now.as_secs(),
        subsec_nanos: now.subsec_nanos(),
    };
    let now_date = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let start_timestamp = Time {
        secs: now_date.as_secs(),
        subsec_nanos: now_date.subsec_nanos(),
    };

    let summary = Summary::new(&results);
    println!("{}", summary);

    if let Some(path) = args.json_output {
        let output = std::fs::File::create(path)?;
        let report = Report {
            metadata: archive.metadata(),
            config: &cfg,
            summary: summary.clone(),
            results,
            start_timestamp,
            total_time,
        };
        println!("Writing JSON report…");
        serde_json::to_writer_pretty(output, &report)?;
    } else if let Some(path) = args.tsv_output {
        let mut output = match std::fs::OpenOptions::new().append(true).open(&path) {
            Ok(f) => Ok(f),
            Err(_) => std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path),
        }?;
        let md = output.metadata()?;
        if md.len() == 0 {
            // new file, write headers:
            output
                .write_all(b"id\tdate\ttag/branch\ttop1\ttop5\tworse\tno suggs\twrong suggs\n")?;
        }
        let git_id = std::process::Command::new("git")
            .arg("rev-parse")
            .arg("--short")
            .arg("HEAD")
            .output()?;
        output.write_all(String::from_utf8(git_id.stdout).unwrap().trim().as_bytes())?;
        output.write_all(b"\t")?;
        output.write_all(Local::now().to_rfc3339().as_bytes())?;
        output.write_all(b"\t")?;
        let git_descr = std::process::Command::new("git").arg("describe").output()?;
        output.write_all(
            String::from_utf8(git_descr.stdout)
                .unwrap()
                .trim()
                .as_bytes(),
        )?;
        output.write_all(b"\t")?;
        output.write_all(summary.first_position.to_string().as_bytes())?;
        output.write_all(b"\t")?;
        output.write_all(summary.top_five.to_string().as_bytes())?;
        output.write_all(b"\t")?;
        output.write_all(summary.any_position.to_string().as_bytes())?;
        output.write_all(b"\t")?;
        output.write_all(summary.no_suggestions.to_string().as_bytes())?;
        output.write_all(b"\t")?;
        output.write_all(summary.only_wrong.to_string().as_bytes())?;
        output.write_all(b"\n")?;
    };

    println!("Done!");
    match args.threshold {
        Some(threshold) => {
            if threshold < (summary.top_five as f32 / summary.total_words as f32 * 100.0) {
                Ok(())
            } else {
                Err("accuracy @5 lower threshold")?
            }
        }
        None => Ok(()),
    }
}
