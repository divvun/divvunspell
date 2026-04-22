//! Wrapper for `serde_json::Error` that renders a source-code snippet at the
//! failure location.
//!
//! Plain `serde_json::Error` stringifies as `"<msg> at line N column M"`, which
//! tells the user nothing about the bytes at that position. When the file was
//! produced by a tool, the user often needs to see the actual malformed bytes
//! to diagnose whether the file is the wrong format, truncated, or built by a
//! buggy producer. This module produces error messages like:
//!
//! ```text
//! key must be a string at line 1 column 2
//!
//!    1 | {0: "se"
//!      |  ^
//! ```

use std::fmt;

/// A parse error from `serde_json` augmented with a snippet of the source
/// bytes at the failure location.
///
/// This is a leaf error: it does not expose the original `serde_json::Error`
/// as a `.source()`, because doing so would cause the underlying message to
/// be printed twice by error-chain renderers. If you need the typed
/// `serde_json::Error` programmatically, use [`JsonParseError::new`]'s input
/// before constructing this wrapper.
#[derive(Debug)]
pub struct JsonParseError {
    message: String,
    snippet: String,
}

impl JsonParseError {
    /// Wrap a `serde_json::Error` with a snippet extracted from `source_bytes`.
    pub fn new(err: serde_json::Error, source_bytes: &[u8]) -> Self {
        let message = err.to_string();
        let snippet = render_snippet(source_bytes, err.line(), err.column());
        Self { message, snippet }
    }
}

impl fmt::Display for JsonParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        if !self.snippet.is_empty() {
            write!(f, "\n\n{}", self.snippet)?;
        }
        Ok(())
    }
}

impl std::error::Error for JsonParseError {}

/// Maximum characters shown on either side of the caret when clipping long
/// lines. Keeps output readable even when the offending line is thousands of
/// bytes long (e.g. a minified JSON blob).
const CONTEXT_CHARS: usize = 80;

fn render_snippet(bytes: &[u8], line: usize, column: usize) -> String {
    if line == 0 {
        return String::new();
    }

    let text = String::from_utf8_lossy(bytes);
    let Some(target) = text.split('\n').nth(line.saturating_sub(1)) else {
        return String::new();
    };

    // Clip very long lines around the caret.
    let target_chars: Vec<char> = target.chars().collect();
    let caret_idx = column.saturating_sub(1).min(target_chars.len());
    let (start, end) = clip_window(caret_idx, target_chars.len(), CONTEXT_CHARS);
    let clipped: String = target_chars[start..end].iter().collect();
    let left_ellipsis = if start > 0 { "…" } else { "" };
    let right_ellipsis = if end < target_chars.len() { "…" } else { "" };

    let display_line = format!("{left_ellipsis}{clipped}{right_ellipsis}");
    let caret_offset = left_ellipsis.chars().count() + (caret_idx - start);

    let n = line.to_string();
    let gutter_pad = " ".repeat(n.len());
    let caret_pad = " ".repeat(caret_offset);

    format!("  {n} | {display_line}\n  {gutter_pad} | {caret_pad}^")
}

fn clip_window(caret: usize, total: usize, radius: usize) -> (usize, usize) {
    let start = caret.saturating_sub(radius);
    let end = (caret + radius).min(total);
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse<T: for<'a> serde::Deserialize<'a>>(bytes: &[u8]) -> Result<T, JsonParseError> {
        serde_json::from_slice(bytes).map_err(|e| JsonParseError::new(e, bytes))
    }

    #[test]
    fn snippet_points_at_failure() {
        let bytes = br#"{0: "hi"}"#;
        let err = parse::<serde_json::Value>(bytes).unwrap_err();
        let rendered = err.to_string();
        assert!(rendered.contains("line 1 column 2"), "got: {rendered}");
        assert!(rendered.contains("| {0:"), "got: {rendered}");
        // Caret lines up under position 2 (the `0`).
        assert!(rendered.contains("\n    |  ^"), "got: {rendered}");
    }

    #[test]
    fn long_lines_get_clipped() {
        let mut raw = String::from("{");
        for i in 0..500 {
            raw.push_str(&format!("\"field_{i}\": {i},"));
        }
        raw.push('}');
        let bytes = raw.as_bytes();
        let err: JsonParseError = serde_json::from_slice::<serde_json::Value>(bytes)
            .map_err(|e| JsonParseError::new(e, bytes))
            .unwrap_err();
        let rendered = err.to_string();
        assert!(
            rendered.contains('…'),
            "expected clip marker, got: {rendered}"
        );
    }

    #[test]
    fn no_snippet_for_unknown_location() {
        // An empty input produces line 1 col 1 from serde_json. We still render
        // *something* — just make sure we don't panic.
        let _ = parse::<serde_json::Value>(b"").unwrap_err().to_string();
    }
}
