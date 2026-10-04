//! Cleanup of raw ICY metadata fields before they become a
//! [`ScrobbleTrack`](super::ScrobbleTrack).
//!
//! Kept dependency-free (no `regex`): the heavy cleanup pass (stripping
//! `(Remaster)` / `(feat. …)` tags, smart-quote normalization, etc.) is a
//! later slice that will reuse these primitives. The `"Artist - Title"`
//! split itself happens in the UI's radio-metadata handler, which hands
//! the two halves to [`ScrobbleTrack::from_icy`](super::ScrobbleTrack::from_icy).

/// Clean a raw ICY metadata field: drop the NUL padding ICY frames carry
/// (treating any NUL — leading, trailing, or interior — as whitespace), trim
/// surrounding whitespace, and collapse internal runs of whitespace to a single
/// space. Idempotent.
pub fn clean(s: &str) -> String {
    s.replace('\0', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_strips_nul_trim_and_collapses_whitespace() {
        assert_eq!(clean("Daft Punk\0\0"), "Daft Punk");
        assert_eq!(clean("  Around   the  World  "), "Around the World");
        assert_eq!(clean("\0  A\tB \0"), "A B");
        assert_eq!(clean(""), "");
        assert_eq!(clean("   \0  "), "");
    }
}
