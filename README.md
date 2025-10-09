# divvunspell

[![CI](https://builds.giellalt.org/api/badge/divvunspell)](https://builds.giellalt.org/pipelines/divvunspell)
[![Crates.io](https://img.shields.io/crates/v/divvunspell.svg)](https://crates.io/crates/divvunspell)
[![Documentation](https://docs.rs/divvunspell/badge.svg)](https://docs.rs/divvunspell)

A fast, feature-rich spell checking library and toolset for HFST-based spell checkers. Written in Rust, divvunspell is a modern reimplementation and extension of [hfst-ospell](https://github.com/hfst/hfst-ospell) with additional features like parallel processing, comprehensive tokenization, case handling, and morphological analysis.

## Features

- **High Performance**: Memory-mapped transducers and parallel suggestion generation
- **ZHFST/BHFST Support**: Load standard HFST spell checker archives
- **Smart Tokenization**: Unicode-aware word boundary detection with customizable alphabets
- **Case Handling**: Intelligent case preservation and suggestion recasing
- **Morphological Analysis**: Extract and filter suggestions based on morphological tags
- **Cross-Platform**: Works on macOS, Linux, Windows, iOS and Android

## Quick Start

### As a Command-Line Tool

```sh
# Install the CLI
cargo install divvunspell-cli

# Check spelling and get suggestions
divvunspell suggest --archive speller.zhfst --json "sámi"
```

### As a Rust Library

Add to your `Cargo.toml`:

```toml
[dependencies]
divvunspell = "1.0.0-beta.5"
```

Basic usage:

```rust
use divvunspell::archive::{SpellerArchive, ZipSpellerArchive};
use divvunspell::speller::{Speller, SpellerConfig, OutputMode};

// Load a spell checker archive
let archive = ZipSpellerArchive::open("language.zhfst")?;
let speller = archive.speller();

// Check if a word is correct
if !speller.clone().is_correct("wordd") {
    // Get spelling suggestions
    let config = SpellerConfig::default();
    let suggestions = speller.clone().suggest("wordd");

    for suggestion in suggestions {
        println!("{} (weight: {})", suggestion.value, suggestion.weight);
    }
}

// Morphological analysis
let analyses = speller.analyze_input("running");
for analysis in analyses {
    println!("{}", analysis.value); // e.g., "run+V+PresPartc"
}
```

## Command-Line Tools

### divvunspell

The main spell checking tool with support for suggestions, analysis, and tokenization.

```sh
# Get suggestions for a word
divvunspell suggest --archive language.zhfst "wordd"

# Always show suggestions even for correct words
divvunspell suggest --archive language.zhfst --always-suggest "word"

# Limit number and weight of suggestions
divvunspell suggest --archive language.zhfst --nbest 5 --weight 20.0 "wordd"

# JSON output
divvunspell suggest --archive language.zhfst --json "wordd"

# Tokenize text
divvunspell tokenize --archive language.zhfst "This is some text."

# Analyze word forms morphologically
divvunspell analyze-input --archive language.zhfst "running"
divvunspell analyze-output --archive language.zhfst "runing"
```

**Options:**
- `-a, --archive <FILE>` - BHFST or ZHFST archive to use
- `-S, --always-suggest` - Show suggestions even if word is correct
- `-w, --weight <WEIGHT>` - Maximum weight limit for suggestions
- `-n, --nbest <N>` - Maximum number of suggestions to return
- `--no-reweighting` - Disable suggestion reweighting (closer to hfst-ospell behavior)
- `--no-recase` - Disable case-aware suggestion handling
- `--json` - Output results as JSON

**Debugging:**

Set `RUST_LOG=trace` to enable detailed logging:

```sh
RUST_LOG=trace divvunspell suggest --archive language.zhfst "wordd"
```

### thfst-tools

Convert HFST and ZHFST files to optimized THFST and BHFST formats.

**THFST** (Tromsø-Helsinki FST): A byte-aligned HFST format optimized for fast loading and memory mapping, required for ARM processors.

**BHFST** (Box HFST): THFST files packaged in a [box](https://github.com/bbqsrc/box) container with JSON metadata for efficient processing.

```sh
# Convert HFST to THFST
thfst-tools hfst-to-thfst acceptor.hfst acceptor.thfst

# Convert ZHFST to BHFST (recommended for distribution)
thfst-tools zhfst-to-bhfst language.zhfst language.bhfst

# Convert THFST pair to BHFST
thfst-tools thfsts-to-bhfst --errmodel errmodel.thfst --lexicon lexicon.thfst output.bhfst

# View BHFST metadata
thfst-tools bhfst-info language.bhfst
```

### accuracy

Test spell checker accuracy against known typo/correction pairs.

```sh
# Install
cd crates/accuracy
cargo install --path .

# Run accuracy test
accuracy typos.tsv language.zhfst

# Save detailed JSON report
accuracy -o report.json typos.tsv language.zhfst

# Limit test size and save TSV summary
accuracy -w 1000 -t results.tsv typos.tsv language.zhfst

# Use custom config
accuracy -c config.json typos.tsv language.zhfst
```

**Input format** (`typos.tsv`): Tab-separated values with typo in first column, expected correction in second:

```
wordd    word
recieve  receive
teh      the
```

**Accuracy viewer** (prototype web UI):

```sh
accuracy -o support/accuracy-viewer/public/report.json typos.txt language.zhfst
cd support/accuracy-viewer
npm i && npm run dev
# Open http://localhost:5000
```

## Building from Source

### Install Rust

```sh
curl https://sh.rustup.rs -sSf | sh
source $HOME/.cargo/env
rustup default stable
```

### Build Everything

```sh
# Build all crates
cargo build --release

# Install specific tools
cargo install --path ./cli          # divvunspell CLI
cargo install --path ./crates/thfst-tools
cargo install --path ./crates/accuracy
```

### Run Tests

```sh
cargo test
```

## Documentation

- **API Documentation**: [docs.rs/divvunspell](https://docs.rs/divvunspell)
- **GitHub Pages**: [divvun.github.io/divvunspell](https://divvun.github.io/divvunspell/)

## License

The **divvunspell library** is dual-licensed under:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

You may choose either license for library use.

The **command-line tools** (`divvunspell`, `thfst-tools`, `accuracy`) are licensed under **GPL-3.0** ([LICENSE-GPL](LICENSE-GPL)).
