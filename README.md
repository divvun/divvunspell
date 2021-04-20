# divvunspell

An implementation of [hfst-ospell](https://github.com/hfst/hfst-ospell) in Rust, with added features like tokenization, case handling, and parallelisation.

[![Actions Status](https://github.com/divvun/divvunspell/workflows/Continuous%20Integration/badge.svg)](https://github.com/divvun/divvunspell/actions)


## Building and installing commandline tools

```sh
# For the `divvunspell` binary:
cargo install divvunspell-bin

# For `thfst-tools` binary:
cargo install thfst-tools

# To build the development version from this source, cd into the relevant directory and:
cargo install --path .
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

```
divvunspell 0.5.0
Testing frontend for the Divvunspell library

USAGE:
    divvunspell [FLAGS] [OPTIONS] <--zhfst <ZHFST>|--bhfst <BHFST>|--acceptor <acceptor>> [WORDS]...

FLAGS:
    -S, --always-suggest    Always show suggestions even if word is correct (implies -s)
    -h, --help              Prints help information
        --json              Output results in JSON
    -s, --suggest           Show suggestions for given word(s)
    -V, --version           Prints version information

OPTIONS:
        --acceptor <acceptor>    Use the given acceptor file
    -b, --bhfst <BHFST>          Use the given BHFST file
        --errmodel <errmodel>    Use the given errmodel file
    -n, --nbest <nbest>          Maximum number of results for suggestions
    -w, --weight <weight>        Maximum weight limit for suggestions
    -z, --zhfst <ZHFST>          Use the given ZHFST file

ARGS:
    <WORDS>...    The words to be processed
```

### accuracy
Usage:

```
divvunspell-accuracy 1.0.0-alpha.5
Accuracy testing for Divvunspell.

USAGE:
    accuracy [OPTIONS] [ARGS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -c <config>             Provide JSON config file to override test defaults
    -o <JSON-OUTPUT>        The file path for the JSON report output
    -w <max-words>          Truncate typos list to max number of words specified

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
