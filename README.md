# divvunspell

An implementation of [hfst-ospell](https://github.com/hfst/hfst-ospell) in Rust, with added features like tokenization, case handling, and parallelisation.

[![Actions Status](https://github.com/divvun/divvunspell/workflows/Continuous%20Integration/badge.svg)](https://github.com/divvun/divvunspell/actions)

## No rust?
```
curl https://sh.rustup.rs -sSf | sh
source $HOME/.cargo/env
rustup default nightly
cargo build --bin divvunspell --release
```

## Building command line frontend

To build the command line frontend for testing spellers:

```
cargo build --bin divvunspell --release
```

The result will be in the `target/release/` directory. To install the binary on your $PATH:

```
cargo install --bin divvunspell --path .
```

Usage:

```
divvunspell 0.5.0
Testing frontend for the DivvunSpell library

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

## License

The crate `divvunspell` is licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

The `divvunspell-tools` binaries are licensed under the GPL version 3 license.
