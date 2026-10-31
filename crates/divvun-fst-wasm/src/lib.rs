//! The wasm-bindgen glue behind `@divvun/fst`.
//!
//! These exports are not a public API. The TypeScript wrapper under `js/fst/`
//! is, and every call reaches the glue through it. The names here are chosen
//! for that wrapper, not for end users. Structured results cross as JSON text,
//! which the wrapper parses into the plain objects its declarations describe,
//! so their shape is built and tested here, natively.

use wasm_bindgen::prelude::wasm_bindgen;

mod config;
mod error;
mod sink;
mod speller;
#[cfg(feature = "test-hooks")]
mod test_hooks;
mod transducer;

pub use sink::{install, set_log_level};
pub use speller::{Speller, read_speller};
#[cfg(feature = "test-hooks")]
pub use test_hooks::{TestObject, test_default_config, test_log, test_panic, test_resolve_config};
pub use transducer::{Transducer, read_transducer};

/// The divvun-fst version this module was built from.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_the_workspace_version() {
        assert_eq!(super::version(), env!("CARGO_PKG_VERSION"));
    }
}
