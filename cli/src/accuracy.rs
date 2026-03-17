use chrono::prelude::*;
use divvun_fst::types::Weight;
use std::{
    io::Write,
    path::Path,
    time::{Instant, SystemTime},
};

use clap::Parser;
use divvun_fst::archive;
use divvun_fst::speller::suggestion::Suggestion;
use divvun_fst::speller::{ReweightingConfig, SpellerConfig};
use indicatif::{ParallelProgressIterator, ProgressBar, ProgressStyle};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use serde::Serialize;
use std::path::PathBuf;
use unic_segment::Graphemes;

/// Calculate Damerau-Levenshtein distance based on grapheme clusters
/// instead of Unicode code points, for proper handling of composed characters
fn grapheme_damerau_levenshtein(s1: &str, s2: &str) -> usize {
    let s1_graphemes: Vec<&str> = Graphemes::new(s1).collect();
    let s2_graphemes: Vec<&str> = Graphemes::new(s2).collect();

    let len1 = s1_graphemes.len();
    let len2 = s2_graphemes.len();

    if len1 == 0 {
        return len2;
    }
    if len2 == 0 {
        return len1;
    }

    let mut matrix = vec![vec![0usize; len2 + 1]; len1 + 1];

    for i in 0..=len1 {
        matrix[i][0] = i;
    }
    for j in 0..=len2 {
        matrix[0][j] = j;
    }

    for i in 1..=len1 {
        for j in 1..=len2 {
            let cost = if s1_graphemes[i - 1] == s2_graphemes[j - 1] {
                0
            } else {
                1
            };

            matrix[i][j] = std::cmp::min(
                std::cmp::min(
                    matrix[i - 1][j] + 1, // deletion
                    matrix[i][j - 1] + 1, // insertion
                ),
                matrix[i - 1][j - 1] + cost, // substitution
            );

            // Transposition
            if i > 1
                && j > 1
                && s1_graphemes[i - 1] == s2_graphemes[j - 2]
                && s1_graphemes[i - 2] == s2_graphemes[j - 1]
            {
                matrix[i][j] = std::cmp::min(matrix[i][j], matrix[i - 2][j - 2] + cost);
            }
        }
    }

    matrix[len1][len2]
}

static CFG: SpellerConfig = SpellerConfig {
    n_best: Some(10),
    max_weight: Some(Weight(10000.0)),
    beam: None,
    reweight: Some(ReweightingConfig::default_const()),
    node_pool_size: 128,
    recase: true,
    completion_marker: None,
    verbose: false,
};

fn load_words(
    path: &str,
    max_words: Option<usize>,
) -> anyhow::Result<Vec<(String, Option<String>)>> {
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
            r.get(0).map(|x| {
                let expected = r
                    .get(1)
                    .map(|y| y.trim())
                    .filter(|y| !y.is_empty())
                    .map(|y| y.to_string());
                (x.to_string(), expected)
            })
        })
        .take(max_words.unwrap_or(usize::MAX))
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
    expected: Option<&'a str>,
    distance: usize,
    suggestions: Vec<Suggestion>,
    position: Option<usize>,
    time: Time,
    false_accept: bool,
}

#[derive(Debug, Serialize)]
struct Report<'a> {
    metadata: Option<&'a divvun_fst::archive::meta::SpellerMetadata>,
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
    false_accept: u32,
    true_positive: u32,
    false_negative: u32,
    true_negative: u32,
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
            "[#1] {} [^5] {} [any] {} [none] {} [wrong] {} [false+] {} [fast] {} [slow] {}",
            percent(self.first_position),
            percent(self.top_five),
            percent(self.any_position),
            percent(self.no_suggestions),
            percent(self.only_wrong),
            percent(self.false_accept),
            self.fastest_lookup,
            self.slowest_lookup
        )
    }
}

impl Summary {
    fn new(results: &[AccuracyResult<'_>]) -> Summary {
        let mut summary = Summary::default();

        results.iter().for_each(|result| {
            summary.total_words += 1;

            match result.expected {
                None => {
                    if result.false_accept {
                        summary.false_accept += 1;
                    } else {
                        summary.true_negative += 1;
                    }
                }
                Some(_) => {
                    if result.false_accept {
                        summary.false_negative += 1;
                    } else {
                        summary.true_positive += 1;
                    }
                }
            }

            if result.expected.is_some() && !result.false_accept {
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

        let total_nanos: u128 = results
            .iter()
            .map(|r| (r.time.secs as u128 * 1_000_000_000) + r.time.subsec_nanos as u128)
            .sum();
        let avg_nanos = total_nanos / results.len() as u128;
        summary.average_time = Time {
            secs: (avg_nanos / 1_000_000_000) as u64,
            subsec_nanos: (avg_nanos % 1_000_000_000) as u32,
        };

        let mut sorted_times: Vec<_> = results.iter().map(|r| r.time).collect();
        sorted_times.sort();
        let percentile_95_count = (results.len() as f32 * 0.95).ceil() as usize;
        let total_nanos_95pc: u128 = sorted_times
            .iter()
            .take(percentile_95_count)
            .map(|t| (t.secs as u128 * 1_000_000_000) + t.subsec_nanos as u128)
            .sum();
        let avg_nanos_95pc = total_nanos_95pc / percentile_95_count as u128;
        summary.average_time_95pc = Time {
            secs: (avg_nanos_95pc / 1_000_000_000) as u64,
            subsec_nanos: (avg_nanos_95pc % 1_000_000_000) as u32,
        };

        let correct_results: Vec<_> = results.iter().filter(|r| r.position.is_some()).collect();

        if !correct_results.is_empty() {
            let total_position: usize = correct_results
                .iter()
                .map(|r| r.position.unwrap() + 1)
                .sum();
            summary.average_position_of_correct =
                total_position as f32 / correct_results.len() as f32;

            let total_suggestions: usize =
                correct_results.iter().map(|r| r.suggestions.len()).sum();
            summary.average_suggestions_for_correct =
                total_suggestions as f32 / correct_results.len() as f32;
        }

        summary
    }
}

#[derive(Debug, Parser)]
pub struct AccuracyArgs {
    /// Provide JSON config file to override test defaults
    #[arg(short = 'c', long)]
    config: Option<PathBuf>,

    /// The 'input -> expected' list in tab-delimited value file (TSV)
    words: Option<String>,

    /// Use the given ZHFST/BHFST file
    archive: Option<String>,

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

    /// Enable verbose mode to include weight details in output
    #[arg(short = 'v', long)]
    verbose: bool,
}

pub fn run(args: AccuracyArgs) -> anyhow::Result<()> {
    let mut cfg: SpellerConfig = match args.config {
        Some(path) => {
            let file = std::fs::File::open(path)?;
            serde_json::from_reader(file)?
        }
        None => CFG.clone(),
    };

    cfg.verbose = args.verbose;

    let archive = match args.archive {
        Some(path) => archive::open(Path::new(&path))?,
        None => {
            anyhow::bail!("No archive path provided; aborting.");
        }
    };

    let words = match args.words {
        Some(path) => load_words(&path, args.max_words)?,
        None => {
            anyhow::bail!("No word list path provided; aborting.");
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

            let is_accepted = archive.speller().is_correct_with_config(&input, &cfg);

            let (suggestions, position, false_accept) = match expected.as_ref() {
                None => {
                    if is_accepted {
                        (Vec::new(), None, false)
                    } else {
                        let suggestions = archive.speller().suggest_with_config(&input, &cfg);
                        (suggestions, None, true)
                    }
                }
                Some(exp) => {
                    if is_accepted {
                        (Vec::new(), None, true)
                    } else {
                        let suggestions = archive.speller().suggest_with_config(&input, &cfg);
                        let position = suggestions.iter().position(|x| &x.value == exp);
                        (suggestions, position, false)
                    }
                }
            };

            let now = now.elapsed();

            let time = Time {
                secs: now.as_secs(),
                subsec_nanos: now.subsec_nanos(),
            };

            let distance = match expected.as_ref() {
                Some(exp) => grapheme_damerau_levenshtein(input, exp),
                None => 0,
            };

            AccuracyResult {
                input,
                expected: expected.as_deref(),
                distance,
                time,
                suggestions,
                position,
                false_accept,
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
                anyhow::bail!("accuracy @5 lower threshold")
            }
        }
        None => Ok(()),
    }
}
