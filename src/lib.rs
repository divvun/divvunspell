/*! Spell-checking and correction with Finite-State Automata.

Implements spell-checking and correction using weighted finite-state
automata. The automata can be compiled using [`HFST`],
this library is originally based on C++ code in [`HFST
ospell`]

[`HFST`]: (https://hfst.github.io)
[`HFST ospell`]: (https://github.com/hfst/hfst-ospell)

Further examples of how to use divvunspell library can be found in the
[`cli`] in the same repository.

[`cli`]: (https://github.com/divvun/divvunspell)
*/

// #![warn(missing_docs)]

#![deny(unsafe_op_in_unsafe_fn)]

pub mod archive;

pub mod paths;
pub mod speller;
pub mod tokenizer;
pub mod transducer;

/// Virtual filesystem abstraction (internal use only)
///
/// **Warning:** This module is only for internal tooling use and should not be used in normal applications.
/// It may be removed or significantly changed in a future version without a major version bump.
/// Use the higher-level [`archive`] module APIs instead.
#[doc(hidden)]
pub mod vfs;

pub(crate) mod constants;
/// Core types for transducers and spell-checking.
///
/// This module contains type aliases and enums used throughout the transducer API.
pub mod types;
