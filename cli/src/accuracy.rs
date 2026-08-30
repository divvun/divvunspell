use divvun_fst::types::Weight;
use jiff::Zoned;
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
    astar_lookahead: false,
    search_dedup: true,
    mutator_subsets: true,
    search_budget: None,
    word_split_weight: None,
    boundary_edit_weight: None,
    verbose: false,
};

fn load_words(
    path: &str,
    max_words: Option<usize>,
) -> anyhow::Result<Vec<(String, Option<String>)>> {
    let mut rdr = csv::ReaderBuilder::new()
        .quoting(false)
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
    set_summary: SetSummary,
    results: Vec<AccuracyResult<'a>>,
    start_timestamp: Time,
    total_time: Time,
}

/// Accuracy scored over correction *sets* rather than one row per pair.
///
/// A test list keyed by (misspelling, correction) pairs charges a misspelling
/// with two valid corrections twice, and only one of them can hold rank 1 --
/// so one of the two rows is lost however good the speller is. Grouping by
/// misspelling asks the question the pair-wise metric cannot: did the speller
/// put *the whole correction set* where it belongs?
///
/// Each field reduces to its `Summary` counterpart when every misspelling has a
/// single correction, so the two agree on lists without ambiguity.
#[derive(Serialize, Default, Debug, Clone)]
struct SetSummary {
    /// Distinct misspellings scored -- the denominator here, as against
    /// `Summary::total_words`, which counts pairs.
    total_inputs: u32,
    /// Misspellings whose corrections are exactly the top `k` suggestions, for
    /// `k` corrections. Equivalent to top-1 when `k` is 1.
    exact_set: u32,
    /// Misspellings with every correction inside the first `max(5, k)`.
    top_five_set: u32,
    /// Misspellings with every correction suggested at all.
    any_set: u32,
    /// Of `total_inputs`, how many carry more than one correction.
    ambiguous_inputs: u32,
    /// Pair-wise top-1 rows that no speller can win, because the misspellings
    /// they belong to have more corrections than there are rank-1 slots.
    unwinnable_pairs: u32,
}

impl std::fmt::Display for SetSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        let percent =
            |v: u32| -> String { format!("{:.2}%", v as f32 / self.total_inputs as f32 * 100f32) };

        write!(
            f,
            "[set #1] {} [set ^5] {} [set any] {} [ambiguous] {} of {}",
            percent(self.exact_set),
            percent(self.top_five_set),
            percent(self.any_set),
            self.ambiguous_inputs,
            self.total_inputs
        )
    }
}

impl SetSummary {
    fn new(results: &[AccuracyResult<'_>]) -> SetSummary {
        // Preserve first-seen order so the report is stable across runs.
        let mut order: Vec<&str> = Vec::new();
        let mut by_input: std::collections::HashMap<&str, Vec<&AccuracyResult<'_>>> =
            std::collections::HashMap::new();

        for result in results {
            // Rows with no correction are false-accept probes, and a row the
            // speller accepted outright never produced a suggestion list to
            // score. Neither says anything about ranking.
            if result.expected.is_none() || result.false_accept {
                continue;
            }
            by_input.entry(result.input).or_insert_with(|| {
                order.push(result.input);
                Vec::new()
            });
            if let Some(group) = by_input.get_mut(result.input) {
                group.push(result);
            }
        }

        let mut summary = SetSummary::default();

        for input in order {
            let Some(group) = by_input.get(input) else {
                continue;
            };

            // The same (input, correction) pair can be listed more than once;
            // it is one correction either way.
            let mut corrections: Vec<&str> = group.iter().filter_map(|r| r.expected).collect();
            corrections.sort_unstable();
            corrections.dedup();
            let k = corrections.len();

            summary.total_inputs += 1;
            if k > 1 {
                summary.ambiguous_inputs += 1;
                summary.unwinnable_pairs += (k - 1) as u32;
            }

            // Every row in a group shares an input, so they share a suggestion
            // list; take the longest in case a row was cut short.
            let Some(suggestions) = group.iter().map(|r| &r.suggestions).max_by_key(|s| s.len())
            else {
                continue;
            };

            let positions: Vec<Option<usize>> = corrections
                .iter()
                .map(|c| suggestions.iter().position(|s| &s.value == c))
                .collect();

            if positions.iter().any(|p| p.is_none()) {
                continue;
            }

            summary.any_set += 1;

            let deepest = positions.iter().flatten().max().copied().unwrap_or(0);
            if deepest < std::cmp::max(5, k) {
                summary.top_five_set += 1;
            }
            if deepest < k {
                summary.exact_set += 1;
            }
        }

        summary
    }
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
            .template("{pos}/{len} [{percent}%] {wide_bar} {elapsed_precise}")
            .unwrap(),
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

    let set_summary = SetSummary::new(&results);
    if set_summary.ambiguous_inputs > 0 {
        println!("{}", set_summary);
    }

    if let Some(path) = args.json_output {
        let output = std::fs::File::create(path)?;
        let report = Report {
            metadata: archive.metadata(),
            config: &cfg,
            summary: summary.clone(),
            set_summary,
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
        output.write_all(Zoned::now().to_string().as_bytes())?;
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

#[cfg(test)]
mod set_summary_tests {
    use super::*;

    fn result<'a>(input: &'a str, expected: &'a str, suggestions: &[&str]) -> AccuracyResult<'a> {
        let suggestions: Vec<Suggestion> = suggestions
            .iter()
            .enumerate()
            .map(|(i, v)| Suggestion::new((*v).into(), Weight(i as f32), None))
            .collect();
        let position = suggestions.iter().position(|s| s.value == expected);

        AccuracyResult {
            input,
            expected: Some(expected),
            distance: 1,
            suggestions,
            position,
            time: Time::default(),
            false_accept: false,
        }
    }

    #[test]
    fn reduces_to_pairwise_scoring_when_no_input_is_ambiguous() {
        let results = [
            result("aa", "ab", &["ab", "ac"]),
            result("ba", "bz", &["bb", "bz"]),
        ];
        let summary = SetSummary::new(&results);

        assert_eq!(summary.total_inputs, 2);
        assert_eq!(summary.ambiguous_inputs, 0);
        assert_eq!(summary.unwinnable_pairs, 0);
        // One gold leads, the other is second: exactly the pair-wise verdict.
        assert_eq!(summary.exact_set, 1);
        assert_eq!(summary.top_five_set, 2);
        assert_eq!(summary.any_set, 2);
    }

    #[test]
    fn both_corrections_leading_counts_as_an_exact_set() {
        // The case the pair-wise metric cannot express: two valid corrections,
        // both offered first. One of its two rows is a guaranteed top-1 loss.
        let results = [
            result("x", "xa", &["xa", "xb", "xc"]),
            result("x", "xb", &["xa", "xb", "xc"]),
        ];
        let summary = SetSummary::new(&results);

        assert_eq!(summary.total_inputs, 1);
        assert_eq!(summary.ambiguous_inputs, 1);
        assert_eq!(summary.unwinnable_pairs, 1);
        assert_eq!(summary.exact_set, 1);
        assert_eq!(summary.top_five_set, 1);
        assert_eq!(summary.any_set, 1);
    }

    #[test]
    fn a_correction_outside_the_leading_k_is_not_an_exact_set() {
        let results = [
            result("x", "xa", &["xa", "xz", "xb"]),
            result("x", "xb", &["xa", "xz", "xb"]),
        ];
        let summary = SetSummary::new(&results);

        assert_eq!(summary.exact_set, 0);
        assert_eq!(summary.top_five_set, 1);
        assert_eq!(summary.any_set, 1);
    }

    #[test]
    fn a_missing_correction_disqualifies_the_whole_set() {
        let results = [
            result("x", "xa", &["xa", "xb"]),
            result("x", "xq", &["xa", "xb"]),
        ];
        let summary = SetSummary::new(&results);

        assert_eq!(summary.any_set, 0);
        assert_eq!(summary.top_five_set, 0);
        assert_eq!(summary.exact_set, 0);
    }

    #[test]
    fn the_top_five_window_widens_for_more_than_five_corrections() {
        let suggestions = ["c0", "c1", "c2", "c3", "c4", "c5"];
        let results: Vec<AccuracyResult<'_>> = suggestions
            .iter()
            .map(|c| result("x", c, &suggestions))
            .collect();
        let summary = SetSummary::new(&results);

        // Six corrections cannot fit in five slots; the window is max(5, k).
        assert_eq!(summary.total_inputs, 1);
        assert_eq!(summary.unwinnable_pairs, 5);
        assert_eq!(summary.top_five_set, 1);
        assert_eq!(summary.exact_set, 1);
    }

    #[test]
    fn duplicate_rows_are_one_correction() {
        let results = [
            result("x", "xa", &["xa", "xb"]),
            result("x", "xa", &["xa", "xb"]),
        ];
        let summary = SetSummary::new(&results);

        assert_eq!(summary.total_inputs, 1);
        assert_eq!(summary.ambiguous_inputs, 0);
        assert_eq!(summary.unwinnable_pairs, 0);
        assert_eq!(summary.exact_set, 1);
    }

    #[test]
    fn false_accepts_and_probes_are_not_scored() {
        let mut accepted = result("x", "xa", &[]);
        accepted.false_accept = true;
        let mut probe = result("y", "ya", &["ya"]);
        probe.expected = None;

        let summary = SetSummary::new(&[accepted, probe]);
        assert_eq!(summary.total_inputs, 0);
    }
}
