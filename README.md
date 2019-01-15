# divvunspell

An implementation of [hfst-ospell](https://github.com/hfst/hfst-ospell) in Rust, with added features like tokenization, case handling, and being thread safe.

## No rust?
```
curl https://sh.rustup.rs -sSf | sh
source $HOME/.cargo/env
rustup default nightly
cargo build --bin divvunspell --features binaries --release
```

## Building command line frontend

To build the command line frontend for testing spellers:

```
cargo build --bin divvunspell --features binaries --release
```

The result will be in the `target/release/` directory. To install the binary on your $PATH:

```
cargo install --bin divvunspell --features binaries --path .
```

Usage:

```
divvunspell -h
divvunspell 0.2.0
Brendan Molloy <brendan@bbqsrc.net>
Testing frontend for the DivvunSpell library

USAGE:
    divvunspell [FLAGS] [OPTIONS] --zhfst <ZHFST> [WORDS]...

FLAGS:
    -h, --help       Prints help information
        --json       Output results in JSON
    -s, --suggest    Show suggestions for given word(s)
    -V, --version    Prints version information

OPTIONS:
    -n, --nbest <nbest>      Maximum number of results for suggestions
    -w, --weight <weight>    Maximum weight limit for suggestions
    -z, --zhfst <ZHFST>      Use the given ZHFST file

ARGS:
    <WORDS>...    The words to be processed
```

Please note that the `ZHFST` file must be uncompressed. `ZHFST` files built by
the Giella infrastructure in the dir `LANGUAGE/tools/spellcheckers/mobile/hfst/*.zhfst` are uncompressed, and can be used directly with `divvunspell`.

## License

This project is licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
