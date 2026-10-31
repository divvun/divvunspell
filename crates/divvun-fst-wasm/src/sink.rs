//! What the module tells the wrapper unasked: the message of a panic, and the
//! library's tracing events.

use std::fmt;

use tracing_core::field::{Field, Visit};
use tracing_core::subscriber::Interest;
use tracing_core::{Event, Level, LevelFilter, Metadata, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use wasm_bindgen::prelude::*;

use crate::error::GlueError;

#[wasm_bindgen(module = "/src/sink.js")]
extern "C" {
    #[wasm_bindgen(js_name = installSink)]
    fn install_sink(sink: JsValue, level: u8);
    #[wasm_bindgen(js_name = setSinkLevel)]
    fn set_sink_level(level: u8);
    #[wasm_bindgen(js_name = sinkLevel)]
    fn sink_level() -> u8;
    #[wasm_bindgen(js_name = sinkLog)]
    fn sink_log(level: u8, target: &str, message: &str);
    #[wasm_bindgen(js_name = sinkPanic)]
    fn sink_panic(message: &str);
}

/// Starts the module instance: hands it the wrapper's sink, sends every panic
/// message there before the panic aborts into a trap, and sends tracing events
/// at `level` and above there. The wrapper calls it once, from `init()`.
#[wasm_bindgen]
pub fn install(sink: JsValue, level: u8) -> Result<(), GlueError> {
    install_sink(sink, level);
    std::panic::set_hook(Box::new(|info| sink_panic(&info.to_string())));
    let subscriber = tracing_subscriber::registry().with(SinkLayer);
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|error| GlueError::new("internal", error.to_string()))
}

/// Sends tracing events at `level` and above from here on. Tracing caches
/// which callsites are enabled, so the cache is rebuilt against the new level.
#[wasm_bindgen(js_name = setLogLevel)]
pub fn set_log_level(level: u8) {
    set_sink_level(level);
    tracing_core::callsite::rebuild_interest_cache();
}

/// The levels in the wrapper's numbering: level `n` is `LEVELS[n - 1]`, and 0
/// turns logging off.
const LEVELS: [Level; 5] = [
    Level::ERROR,
    Level::WARN,
    Level::INFO,
    Level::DEBUG,
    Level::TRACE,
];

fn level_number(level: Level) -> u8 {
    LEVELS
        .iter()
        .position(|known| *known == level)
        .map_or(5, |index| index as u8 + 1)
}

fn level_filter(number: u8) -> LevelFilter {
    match number.checked_sub(1) {
        None => LevelFilter::OFF,
        Some(index) => LevelFilter::from_level(LEVELS[usize::from(index).min(4)]),
    }
}

/// Sends tracing events to the wrapper's logger. It keeps no state: the level
/// lives beside the sink in `sink.js`, and a callsite below it gets
/// `Interest::never`, so a disabled event costs one cached check and never
/// reaches JavaScript. Spans are never enabled, so the registry under it never
/// records one.
struct SinkLayer;

impl SinkLayer {
    fn filter() -> LevelFilter {
        level_filter(sink_level())
    }

    fn wants(metadata: &Metadata<'_>) -> bool {
        metadata.is_event() && *metadata.level() <= Self::filter()
    }
}

impl<S: Subscriber> Layer<S> for SinkLayer {
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        if Self::wants(metadata) {
            Interest::always()
        } else {
            Interest::never()
        }
    }

    fn enabled(&self, metadata: &Metadata<'_>, _context: Context<'_, S>) -> bool {
        Self::wants(metadata)
    }

    fn max_level_hint(&self) -> Option<LevelFilter> {
        Some(Self::filter())
    }

    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let metadata = event.metadata();
        sink_log(
            level_number(*metadata.level()),
            metadata.target(),
            &render(event),
        );
    }
}

/// An event's message followed by its other fields as `name=value`.
fn render(event: &Event<'_>) -> String {
    let mut text = Text::default();
    event.record(&mut text);
    if text.message.is_empty() {
        text.fields.trim_start().to_owned()
    } else {
        text.message + &text.fields
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn levels_round_trip_through_their_numbers() {
        assert_eq!(level_filter(0), LevelFilter::OFF);
        for level in LEVELS {
            assert_eq!(
                level_filter(level_number(level)),
                LevelFilter::from_level(level)
            );
        }
        assert_eq!(level_number(Level::ERROR), 1);
        assert_eq!(level_number(Level::TRACE), 5);
        assert_eq!(level_filter(9), LevelFilter::TRACE);
    }

    #[test]
    fn a_filter_passes_its_level_and_more_severe() {
        let warn = level_filter(2);
        assert!(Level::ERROR <= warn);
        assert!(Level::WARN <= warn);
        assert!(!(Level::INFO <= warn));
    }

    /// Renders every event it sees, as the wrapper's layer does.
    struct Capture(Arc<Mutex<Vec<String>>>);

    impl<S: Subscriber> Layer<S> for Capture {
        fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
            let mut seen = self.0.lock().expect("no test panics while holding it");
            seen.push(render(event));
        }
    }

    #[test]
    fn events_render_as_message_then_fields() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(Capture(Arc::clone(&seen)));
        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(count = 3, name = "sma", "found {} symbols", 7);
            tracing::warn!(count = 3);
            tracing::warn!("plain");
        });
        let seen = seen.lock().expect("the subscriber is gone");
        assert_eq!(
            *seen,
            ["found 7 symbols count=3 name=\"sma\"", "count=3", "plain"]
        );
    }
}
