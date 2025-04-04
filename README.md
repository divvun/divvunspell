# divvunspell

An implementation of [hfst-ospell](https://github.com/hfst/hfst-ospell) in Rust, with added features like tokenization, case handling, and parallelisation.

[![CI](https://github.com/divvun/divvunspell/actions/workflows/ci.yml/badge.svg)](https://github.com/divvun/divvunspell/actions/workflows/ci.yml)

## Building and installing commandline tools

```sh
# For the `divvunspell` binary:
cargo install divvunspell-bin

# For `thfst-tools` binary (most people can skip this one):
cargo install thfst-tools

# To build the development version from this source, cd into the relevant directory and:
cargo install --path .
```

### Building with `gpt2` support on macOS aarch64

(Skip this if you are not experimenting with gpt2 support. So skip. Now.)

Clone this repo then:

```bash
brew install libtorch
LIBTORCH=/opt/homebrew/opt/libtorch cargo build --features gpt2 --bin divvunspell
```

### No Rust?

```sh
curl https://sh.rustup.rs -sSf | sh
source $HOME/.cargo/env
rustup default stable
cargo build --release
```

### divvunspell
Usage:

```sh
Usage: divvunspell SUBCOMMAND [OPTIONS]

Optional arguments:
  -h, --help  print help message

Available subcommands:
  suggest   get suggestions for provided input
  tokenize  print input in word-separated tokenized form
  predict   predict next words using GPT2 model

$ divvunspell suggest -h
Usage: divvunspell suggest [OPTIONS]

Positional arguments:
  inputs                 words to be processed

Optional arguments:
  -h, --help             print help message
  -a, --archive ARCHIVE  BHFST or ZHFST archive to be used
  -S, --always-suggest   always show suggestions even if word is correct
  -w, --weight WEIGHT    maximum weight limit for suggestions
  -n, --nbest NBEST      maximum number of results
  --no-reweighting       disables reweighting algorithm (makes results more like hfst-ospell)
  --no-recase            disables recasing algorithm (makes results more like hfst-ospell)
  --json                 output in JSON format
```

If you want to debug divvunspell behaviour, simply enable rust's logging
features by setting `RUST_LOG=trace` on your commandline's environment
variables.

### accuracy

Building:
```sh
cd accuracy/
cargo install --path .
```

The resulting binary `accuracy` is placed in `$HOME/.cargo/bin/`, make sure it is on the path.

Usage:

```
divvunspell-accuracy 1.0.0-beta.1
Accuracy testing for DivvunSpell.

USAGE:
    accuracy [OPTIONS] [ARGS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -c <config>             Provide JSON config file to override test defaults
    -o <JSON-OUTPUT>        The file path for the JSON report output
    -w <max-words>          Truncate typos list to max number of words specified
    -t <TSV-OUTPUT>         The file path for the TSV line append

ARGS:
    <WORDS>    The 'input -> expected' list in tab-delimited value file (TSV)
    <ZHFST>    Use the given ZHFST file
```

### thfst-tools

Convert hfst and zhfst files to thfst and bhfst formats.

- **thfst**: byte-aligned hfst for fast and efficient loading and memory mapping, required to run `divvunspell` on ARM processors
- **bhfst**: thfst files wrapped in a [box](https://github.com/bbqsrc/box) container; in the case of zhfst files converted to bhfst, the metadata file (`index.xml` in the zhfst archive) is converted to a json file for faster and leaner processing by the `divvunspell` library.

Usage:

```
thfst-tools 1.0.0-alpha.5
Tromsø-Helsinki Finite State Transducer toolkit.

USAGE:
    thfst-tools <SUBCOMMAND>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

SUBCOMMANDS:
    bhfst-info         Print metadata for BHFST
    help               Prints this message or the help of the given subcommand(s)
    hfst-to-thfst      Convert an HFST file to THFST
    thfsts-to-bhfst    Convert a THFST acceptor/errmodel pair to BHFST
    zhfst-to-bhfst     Convert a ZHFST file to BHFST
```

## Speller testing

There's a prototype-level testing tool in `support/accuracy-viewer`. Use it like:

```
accuracy -o support/accuracy-viewer/public/report.json typos.txt sma.zhfst
cd support/accuracy-viewer
npm i && npm run dev
```

View in `http://localhost:5000`.

`typos.txt` is a TSV file with typos in the first column and expected correction in the second.
More info by `accuracy --help`.

## License

The crate `divvunspell` is licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

The `divvunspell`, `thfst-tools` and `accuracy` binaries are licensed under the GPL version 3 license.

## More docs?

We have [GitHub pages site](https://divvun.github.io/divvunspell/) for
divvunspell with some more tech docs and stuff (WIP).
