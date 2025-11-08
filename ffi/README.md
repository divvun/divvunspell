# divvunspell-ffi

C/C++ FFI bindings for the divvunspell spell checking library.

## Overview

This crate provides Foreign Function Interface (FFI) bindings to use divvunspell from C and C++ code. It exposes the core spell checking functionality through a C-compatible API.

## Building

To build the FFI library:

```bash
# Build as both static and dynamic library
cargo build --release -p divvunspell-ffi

# Output will be in target/release/:
# - libdivvunspell.a (static library)
# - libdivvunspell.dylib/.so/.dll (dynamic library)
```

## Examples

See the `examples/` directory for working C examples:

```bash
cd ffi/examples
make
./tokenize "Your text here"
```

The `tokenize` example demonstrates the working tokenization API.

## Headers

C header files are located in `include/`:
- `include/divvun_fst.h` - CFFI-based API

## Current Status

The FFI layer is currently in transition:

- **Working**: Manual FFI functions for tokenization (`divvun_word_indices`, etc.)
- **In Progress**: CFFI-marshaled functions for speller operations need C header signature adjustments

The manual FFI functions (those using `#[unsafe(no_mangle)]`) work correctly. The CFFI-marshaled functions (using `#[cffi::marshal]`) require specific marshaler types on the C side that match the Rust marshaler expectations.

## Implementation Notes

This FFI layer uses:
- Manual `extern "C"` functions for simple types (strings, pointers)
- The [cffi](https://github.com/cffi-rs/cffi) crate for complex type marshaling (Arc, PathBuf, Result types)
- Flatbuffers for complex data structures like WordContext

Headers are manually maintained to ensure stable ABI.

## License

Licensed under MIT OR Apache-2.0, same as the parent divvunspell project.
