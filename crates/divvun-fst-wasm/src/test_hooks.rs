//! Exports for the JavaScript test suite: a small object that owns module
//! memory, a panic, log events at every level, the config a call would run
//! with, and the built-in one. Only the `test-hooks` feature compiles them,
//! and only `js/build.mjs --test` turns it on, writing outside the package
//! directory, so the published package never carries them.

use wasm_bindgen::prelude::*;

use crate::config;
use crate::error::GlueError;

const TARGET: &str = "divvun_fst_wasm::test_hooks";

/// A small owned object for testing handle lifetime. Dropping it logs a
/// debug event, so a test can see when its memory is released.
#[wasm_bindgen]
pub struct TestObject {
    label: String,
}

#[wasm_bindgen]
impl TestObject {
    #[wasm_bindgen(constructor)]
    pub fn new(label: String) -> TestObject {
        TestObject { label }
    }

    pub fn label(&self) -> String {
        self.label.clone()
    }

    /// Traps while wasm-bindgen holds a borrow of the object, a borrow the
    /// trap never gives back, so freeing the object afterwards would fail.
    #[wasm_bindgen(js_name = panicWhileBorrowed)]
    pub fn panic_while_borrowed(&self, message: &str) {
        panic!("{}: {message}", self.label)
    }
}

impl Drop for TestObject {
    fn drop(&mut self) {
        tracing::debug!(target: TARGET, "dropped {}", self.label);
    }
}

#[wasm_bindgen(js_name = testPanic)]
pub fn test_panic(message: &str) {
    panic!("{message}")
}

/// Logs `message` at `level`, in the wrapper's numbering (1 is error, 5 is trace).
#[wasm_bindgen(js_name = testLog)]
pub fn test_log(level: u8, message: &str) {
    match level {
        0..=1 => tracing::error!(target: TARGET, "{message}"),
        2 => tracing::warn!(target: TARGET, "{message}"),
        3 => tracing::info!(target: TARGET, "{message}"),
        4 => tracing::debug!(target: TARGET, "{message}"),
        5.. => tracing::trace!(target: TARGET, "{message}"),
    }
}

/// The config a speller call passed `json` runs with, as the JSON text the
/// wrapper reads back: what proves a field the wrapper takes reaches
/// divvun-fst rather than being dropped on the way.
#[wasm_bindgen(js_name = testResolveConfig)]
pub fn test_resolve_config(json: &str) -> Result<String, GlueError> {
    config::parse(json).map(|config| config::to_json(&config))
}

/// divvun-fst's built-in config, as the JSON text the wrapper reads back,
/// for holding beside the defaults the wrapper declares.
#[wasm_bindgen(js_name = testDefaultConfig)]
pub fn test_default_config() -> String {
    config::default_json()
}
