//! UTF-8-safe JSON parse helper with response preview for diagnostics.
//!
//! Centralizes the per-module `serde_json::from_str` + `with_context` pattern.
//! The previous byte-slice preview (`&body[..body.len().min(200)]`) panicked
//! on multibyte boundaries; this helper uses `chars().take(N)`.

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;

/// Maximum characters included in a parse-error preview.
pub(crate) const PARSE_PREVIEW_LIMIT: usize = 500;

/// Parse a JSON body into `T`, attaching a UTF-8-safe preview of the first
/// `PARSE_PREVIEW_LIMIT` characters on failure for diagnostic logging.
///
/// `label` is a short noun phrase describing the response (e.g. "albums JSON",
/// "radio stations JSON"); it appears in the error message as
/// `"Failed to parse <label>: <preview>"`.
pub(crate) fn parse_json_with_preview<T>(body: &str, label: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_str::<T>(body)
        .with_context(|| format!("Failed to parse {label}: {}", preview(body)))
}

/// Build a UTF-8-safe preview of the first `PARSE_PREVIEW_LIMIT` characters
/// of a response body. Use when the parse itself isn't direct (e.g. tries
/// multiple shapes) but you still want a consistent preview.
pub(crate) fn preview(body: &str) -> String {
    body.chars().take(PARSE_PREVIEW_LIMIT).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_handles_multibyte_utf8_at_boundary() {
        // 200 'a's followed by a 4-byte emoji. Byte-slicing at 200 would
        // crash; chars().take(N) cannot.
        let body = format!("{}{}", "a".repeat(200), "🎶");
        // Should not panic.
        let p = preview(&body);
        assert!(!p.is_empty());
        assert!(p.starts_with("aaa"));
    }

    #[test]
    fn preview_caps_at_parse_preview_limit() {
        let body = "x".repeat(PARSE_PREVIEW_LIMIT + 50);
        let p = preview(&body);
        assert_eq!(p.chars().count(), PARSE_PREVIEW_LIMIT);
    }

    #[test]
    fn parse_json_with_preview_succeeds_on_valid_json() {
        let v: serde_json::Value = parse_json_with_preview(r#"{"k": 1}"#, "test JSON").unwrap();
        assert_eq!(v["k"], 1);
    }

    #[test]
    fn parse_json_with_preview_includes_label_and_preview_on_failure() {
        let err = parse_json_with_preview::<serde_json::Value>("not json", "demo").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("demo"), "label missing: {msg}");
        assert!(msg.contains("not json"), "preview missing: {msg}");
    }

    #[test]
    fn parse_json_with_preview_does_not_panic_on_multibyte_body() {
        // 199 'a's + 4-byte emoji so the boundary lands inside the emoji
        // for a hypothetical byte-slice of length 200. Helper must not panic.
        let body = format!("{}{}", "a".repeat(199), "🎶");
        let _ = parse_json_with_preview::<serde_json::Value>(&body, "demo");
    }
}
