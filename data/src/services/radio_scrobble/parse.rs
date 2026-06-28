//! Parsing + cleanup of ICY `StreamTitle` metadata into a [`ScrobbleTrack`].
//!
//! Kept dependency-free (no `regex`): the heavy cleanup pass (stripping
//! `(Remaster)` / `(feat. …)` tags, smart-quote normalization, etc.) is a
//! later slice that will reuse these primitives.

use super::ScrobbleTrack;

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

/// Parse a raw ICY `StreamTitle` value (e.g. `"Artist - Title"`) into a
/// [`ScrobbleTrack`].
///
/// Splits on the FIRST `" - "` — matching nokkvi's existing radio-metadata
/// split in `src/update/playback.rs` (`splitn(2, " - ")`), so
/// `"Artist - Title - Remix"` yields `artist = "Artist"`,
/// `title = "Title - Remix"`. Returns `None` when the value has no `" - "`
/// separator or either side is blank after cleanup (title-only streams are
/// skipped — see [`ScrobbleTrack::from_icy`]).
///
/// This is the fallback path for stations that emit a single combined title;
/// when nokkvi already has the artist and title split apart, build the track
/// via [`ScrobbleTrack::from_icy`] directly instead.
pub fn parse_stream_title(raw: &str) -> Option<ScrobbleTrack> {
    let cleaned = clean(raw);
    let (artist, title) = cleaned.split_once(" - ")?;
    ScrobbleTrack::from_icy(Some(artist), Some(title), None, None)
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

    #[test]
    fn parse_splits_artist_and_title() {
        let t = parse_stream_title("Daft Punk - Around the World").unwrap();
        assert_eq!(t.artist, "Daft Punk");
        assert_eq!(t.title, "Around the World");
        assert_eq!(t.album, None);
        assert_eq!(t.station_name, None);
    }

    #[test]
    fn parse_splits_on_first_separator_only() {
        // Matches splitn(2, " - "): remainder stays in the title.
        let t = parse_stream_title("Artist - Title - Remix").unwrap();
        assert_eq!(t.artist, "Artist");
        assert_eq!(t.title, "Title - Remix");
    }

    #[test]
    fn parse_cleans_padding_and_whitespace_around_split() {
        let t = parse_stream_title("  Daft Punk  -  Around the World \0\0").unwrap();
        assert_eq!(t.artist, "Daft Punk");
        assert_eq!(t.title, "Around the World");
    }

    #[test]
    fn parse_rejects_title_only_and_blank() {
        assert!(parse_stream_title("Just A Title").is_none());
        assert!(parse_stream_title("").is_none());
        assert!(parse_stream_title("\0\0").is_none());
    }

    #[test]
    fn parse_rejects_blank_side_of_separator() {
        assert!(parse_stream_title(" - Title").is_none());
        assert!(parse_stream_title("Artist - ").is_none());
    }
}
