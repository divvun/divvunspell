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

## License

This project is licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
