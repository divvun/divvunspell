/*! Spell-checking and correction with Finite-State Automata.

Implements spell-checking and correction using weighted finite-state
automata. The automata can be compiled using [`HFST`],
this library is originally based on C++ code in [`HFST
ospell`]

[`HFST`]: (https://hfst.github.io)
[`HFST ospell`]: (https://github.com/hfst/hfst-ospell)

# Usage examples

```
use divvunspell::archive::ZipSpellerArchive

let path = Path();
let speller = ZipSpellerArchive::open(path)
let cfg = SpellerConfig::default();
let words = vec!("words", "schmords");
todo!
```

Further examples of how to use divvunspell library can be found in the
[`divvunspell-bin`] in the same repository.

[`divvunspell-bin`]: (https://github.com/divvun/divvunspell)

*/

#![warn(missing_docs)]
pub mod archive;
#[cfg(feature = "internal_ffi")]
pub mod ffi;

pub mod paths;
pub mod predictor;
pub mod speller;
pub mod tokenizer;
pub mod transducer;
pub mod vfs;

pub(crate) mod constants;
pub(crate) mod types;
