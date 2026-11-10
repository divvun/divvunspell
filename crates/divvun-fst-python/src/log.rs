//! The library's tracing events, kept while a call goes on without the GIL
//! and handed to Python's `logging` once the call has it back.
//!
//! divvun-fst logs under its module paths, `divvun_fst::speller::worker` and
//! the like, so every event lands on a logger under `divvun_fst`, which is
//! also the package's name.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use pyo3::prelude::*;
use tracing::field::{Field, Visit};
use tracing::level_filters::LevelFilter;
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Dispatch, Level, Metadata, Subscriber};

/// The logger every event's logger sits under.
const ROOT_LOGGER: &str = "divvun_fst";

struct Event {
    level: Level,
    target: &'static str,
    message: String,
    fields: String,
}

impl Event {
    /// The message followed by the event's other fields as `name=value`.
    fn rendered(&self) -> String {
        if self.message.is_empty() {
            self.fields.trim_start().to_owned()
        } else {
            format!("{}{}", self.message, self.fields)
        }
    }
}

/// The events a call kept, all at or above the level it started with.
pub struct Kept {
    events: Vec<Event>,
}

/// The least level the `divvun_fst` logger handles at the start of a call, as
/// `logging.disable` and the logger's effective level decide. Nothing below it
/// is kept.
pub fn threshold(py: Python<'_>) -> PyResult<LevelFilter> {
    let logger = py
        .import("logging")?
        .call_method1("getLogger", (ROOT_LOGGER,))?;
    for level in [
        Level::TRACE,
        Level::DEBUG,
        Level::INFO,
        Level::WARN,
        Level::ERROR,
    ] {
        if logger
            .call_method1("isEnabledFor", (python_level(level),))?
            .is_truthy()?
        {
            return Ok(LevelFilter::from_level(level));
        }
    }
    Ok(LevelFilter::OFF)
}

/// `logging`'s number for a level. Python has no trace level, so trace is 5,
/// below `logging.DEBUG`.
fn python_level(level: Level) -> u8 {
    match level {
        Level::ERROR => 40,
        Level::WARN => 30,
        Level::INFO => 20,
        Level::DEBUG => 10,
        Level::TRACE => 5,
    }
}

/// Runs `f`, keeping every event at `threshold` or above. With logging off it
/// runs `f` alone, with no subscriber to make.
pub fn collecting<T>(threshold: LevelFilter, f: impl FnOnce() -> T) -> (T, Kept) {
    if threshold == LevelFilter::OFF {
        return (f(), Kept { events: Vec::new() });
    }
    let events = Arc::new(Mutex::new(Vec::new()));
    let dispatch = Dispatch::new(Collector {
        keep: threshold,
        events: Arc::clone(&events),
    });
    let value = tracing::dispatcher::with_default(&dispatch, f);
    drop(dispatch);
    let events = std::mem::take(&mut *events.lock().unwrap_or_else(PoisonError::into_inner));
    (value, Kept { events })
}

impl Kept {
    /// Logs each kept event, in order, on the logger named after its target
    /// with `::` read as `.`.
    pub fn hand_over(&self, py: Python<'_>) -> PyResult<()> {
        if self.events.is_empty() {
            return Ok(());
        }
        let get_logger = py.import("logging")?.getattr("getLogger")?;
        for event in &self.events {
            get_logger
                .call1((event.target.replace("::", "."),))?
                .call_method1("log", (python_level(event.level), event.rendered()))?;
        }
        Ok(())
    }
}

/// The subscriber for one call, set as its thread's default for the call
/// alone. Spans are never enabled, so none is ever made.
///
/// Tracing caches one interest per callsite for every live subscriber
/// together, and rebuilds it whenever a subscriber is made, so a call made with
/// a lower threshold than another thread's still sees its events. `event`
/// checks again what it receives, because the cache is shared.
struct Collector {
    keep: LevelFilter,
    events: Arc<Mutex<Vec<Event>>>,
}

impl Collector {
    fn keeps(&self, metadata: &Metadata<'_>) -> bool {
        metadata.is_event() && *metadata.level() <= self.keep
    }
}

impl Subscriber for Collector {
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        if self.keeps(metadata) {
            Interest::always()
        } else {
            Interest::never()
        }
    }

    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.keeps(metadata)
    }

    fn max_level_hint(&self) -> Option<LevelFilter> {
        Some(self.keep)
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let metadata = event.metadata();
        if !self.keeps(metadata) {
            return;
        }
        let mut text = Text::default();
        event.record(&mut text);
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Event {
                level: *metadata.level(),
                target: metadata.target(),
                message: text.message,
                fields: text.fields,
            });
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}

#[derive(Default)]
struct Text {
    message: String,
    fields: String,
}

impl Visit for Text {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            self.fields
                .push_str(&format!(" {}={value:?}", field.name()));
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message.push_str(&format!("{value:?}"));
        } else {
            self.fields
                .push_str(&format!(" {}={value:?}", field.name()));
        }
    }
}

/// Runs `f` without the GIL, then hands the events it logged to `logging`.
pub fn detached<T: Send>(py: Python<'_>, f: impl FnOnce() -> T + Send) -> PyResult<T> {
    let threshold = threshold(py)?;
    let (value, kept) = py.detach(|| collecting(threshold, f));
    kept.hand_over(py)?;
    Ok(value)
}
