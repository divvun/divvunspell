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

## Headers

C and C++ header files are located in `include/`:
- `include/divvunspell.h` - C API declarations
- `include/divvunspell.hpp` - C++ wrapper class

## Usage

Link against the appropriate library and include the headers in your C/C++ project. The API provides functions for:

- Opening spell checker archives (ZHFST/BHFST files)
- Checking word spelling
- Getting spelling suggestions
- Tokenization and word context extraction

## Implementation Notes

This FFI layer uses the [cffi](https://github.com/cffi-rs/cffi) crate for safe FFI marshaling and flatbuffers for complex data structures. Headers are manually maintained to ensure stable ABI.

## License

Licensed under MIT OR Apache-2.0, same as the parent divvunspell project.
