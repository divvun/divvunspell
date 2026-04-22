/*! Regression testing for Finite-State Spell-Checkers

A tool to help testing updates in finite-state spell-checkers. Simply shows the
differences between two spell-checker language models. Can be used in automated
scripts to cap amount of changes between two versions.

# Usage examples

It's a command-line tool:
```console
$ cargo run -- --old old.zhfst --new new.zhfst --words typos.txt --threshold 0.9
```
will expect there to be less than 10 % regressions between `old.zhfst` and
`new.zhfst`.
*/

use std::path::PathBuf;

use anyhow::{Context as _, bail};
use clap::Parser;
use divvun_fst::archive;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short, long, value_name = "OLDFILE")]
    old: PathBuf,
    #[arg(short, long, value_name = "NEWFILE")]
    new: PathBuf,
    #[arg(short, long, value_name = "WORDFILE")]
    words: PathBuf,
    #[arg(short, long, value_name = "THOLD")]
    threshold: f32,
}

fn load_words(path: &PathBuf) -> anyhow::Result<Vec<(String, String)>> {
    let mut rdr = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .delimiter(b'\t')
        .has_headers(false)
        .flexible(true)
        .from_path(path)
        .with_context(|| format!("failed to open word list '{}'", path.display()))?;
    Ok(rdr
        .records()
        .filter_map(Result::ok)
        .filter_map(|r| {
            r.get(0)
                .and_then(|x| r.get(1).map(|y| (x.to_string(), y.to_string())))
        })
        .collect())
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let oldarch = archive::open(&cli.old)
        .with_context(|| format!("failed to load old archive '{}'", cli.old.display()))?;
    let newarch = archive::open(&cli.new)
        .with_context(|| format!("failed to load new archive '{}'", cli.new.display()))?;
    let words = load_words(&cli.words)?;
    let mut regressions = 0;
    for word in &words {
        let oldsuggs = oldarch.speller().suggest(&word.0);
        let newsuggs = newarch.speller().suggest(&word.0);
        let oldpos = oldsuggs.iter().position(|x| x.value == word.1);
        let newpos = newsuggs.iter().position(|x| x.value == word.1);
        if oldpos != newpos {
            match (oldpos, newpos) {
                (None, Some(y)) => {
                    println!(
                        "Regression: {} -> {} was uncorrected now {}",
                        word.0, word.1, y
                    );
                }
                (Some(x), None) => {
                    println!(
                        "Regression: {} -> {} was {} now uncorrectable!",
                        word.0, word.1, x
                    );
                }
                (Some(x), Some(y)) => {
                    println!("REGRESSION: {} -> {} was {} now {}", word.0, word.1, x, y);
                }
                (None, None) => {
                    unreachable!("oldpos != newpos but both are None");
                }
            }
            regressions += 1;
        } else {
            print!(".");
        }
    }
    if words.is_empty() {
        bail!("no words loaded from '{}'", cli.words.display());
    }
    let regressionrate = regressions as f32 / words.len() as f32;
    if cli.threshold > regressionrate {
        Ok(())
    } else {
        bail!(
            "regressions exceed threshold: {regressionrate} >= {}",
            cli.threshold
        )
    }
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Error: {err:?}");
            std::process::ExitCode::FAILURE
        }
    }
}
