//! Centralized artwork URL generation for Navidrome Subsonic API.
//!
//! Single source of truth for `getCoverArt` URLs. Fetches go through
//! `AlbumsService::fetch_album_artwork` against the bare reqwest client; there
//! is no client-side cache, so URL form is the only thing that matters here.

use crate::types::{song::Song, stream_quality::StreamParams};

/// Navidrome cover-art ID kind prefixes — the set `ParseArtworkID` accepts
/// (`model/artwork_id.go` `artworkKindMap`: mf, ar, al, pl, dc, ra). An ID that
/// already carries one of these is a fully-formed artwork ID and must pass
/// through untouched; anything else is treated as a bare album ID and gets an
/// `al-` prefix. `dc-` (per-disc cover, added in navidrome#5182) and `ra-`
/// (radio) were missing, so disc-numbered tracks' `coverArt` (`dc-<albumID>:<disc>`)
/// got mangled into `al-dc-…` and 404'd as "Artwork not found" (code 70); `sh-`
/// was never an artwork kind and is dropped.
const KNOWN_PREFIXES: [&str; 6] = ["al-", "ar-", "mf-", "pl-", "dc-", "ra-"];

/// Default size for high-resolution artwork (matches QML client)
pub const HIGH_RES_SIZE: u32 = 1000;

/// Default size for thumbnails
pub const THUMBNAIL_SIZE: u32 = 80;

/// Test-only convenience wrapper over [`build_cover_art_url_with_timestamp`]
/// that omits the cache-busting `updated_at` timestamp. Production code calls
/// [`build_cover_art_url_with_timestamp`] directly.
#[cfg(test)]
fn build_cover_art_url(
    art_id: &str,
    server_url: &str,
    subsonic_credential: &str,
    size: Option<u32>,
) -> String {
    build_cover_art_url_with_timestamp(art_id, server_url, subsonic_credential, size, None)
}

/// Build a `getCoverArt` URL carrying the cover's version, which is either
/// Navidrome's image hash (0.64+, see `types::image_info::artwork_version`)
/// or an `updated_at` timestamp:
/// - a valid 16-hex hash rides the id as `<id>_<hash>` (the server's own
///   form; it ignores a stale one and serves the current image) and the
///   `_u=` parameter is dropped. Disc art (`dc-`) takes no hash, and an id
///   that already carries a `_` suffix is never suffixed twice;
/// - anything else rides `_u=`, exactly as before 0.64. The server never
///   reads `_u=`; it only makes a changed version a different URL.
pub fn build_cover_art_url_with_timestamp(
    art_id: &str,
    server_url: &str,
    subsonic_credential: &str,
    size: Option<u32>,
    updated_at: Option<&str>,
) -> String {
    // Handle empty or already-complete URLs
    if art_id.is_empty() {
        return String::new();
    }

    if art_id.starts_with("http") {
        return art_id.to_string();
    }

    // Normalize ID: add "al-" prefix if no known prefix present
    let final_id = if KNOWN_PREFIXES
        .iter()
        .any(|prefix| art_id.starts_with(prefix))
    {
        art_id.to_string()
    } else {
        format!("al-{art_id}")
    };

    // Build size parameter string conditionally
    let size_param = size.map(|s| format!("&size={s}")).unwrap_or_default();

    if subsonic_credential.is_empty() {
        return String::new();
    }
    let image_hash = updated_at.filter(|v| {
        crate::types::image_info::is_valid_image_hash(v)
            && !final_id.contains('_')
            && !final_id.starts_with("dc-")
    });
    let standard = crate::services::api::subsonic::STANDARD_PARAMS;
    match image_hash {
        Some(hash) => format!(
            "{server_url}/rest/getCoverArt?id={final_id}_{hash}&{subsonic_credential}{size_param}&square=true&{standard}"
        ),
        None => {
            let cache_buster = updated_at.unwrap_or("");
            format!(
                "{server_url}/rest/getCoverArt?id={final_id}&{subsonic_credential}{size_param}&square=true&{standard}&_u={cache_buster}"
            )
        }
    }
}

/// Build the album thumbnail artwork URL for a song (a queue row's
/// `artwork_url`).
///
/// **Invariant**: uses the song's `album_id`, NOT `song.cover_art`. A row
/// keyed by its album fetches through this URL and the playlist strip quad
/// reuses it under the album id, so a track's own `mf-` cover here would be
/// cached as the album's. Under Track Artwork's Own Cover a song's own art is
/// fetched by its cover key instead (`SongCover::cover_key`).
///
/// Always uses [`THUMBNAIL_SIZE`].
///
/// Carries the song's `updated_at` as the `_u=` cache-buster so a server-side
/// cover change busts the URL — the version-aware prefetch dedup then re-fetches
/// it (N17) instead of serving the stale thumbnail for the rest of the session.
pub fn build_song_artwork_url(song: &Song, server_url: &str, subsonic_credential: &str) -> String {
    let album_id = song.album_id.as_deref().unwrap_or_default();
    build_cover_art_url_with_timestamp(
        album_id,
        server_url,
        subsonic_credential,
        Some(THUMBNAIL_SIZE),
        song.updated_at.as_deref(),
    )
}

/// Build a Subsonic stream URL for audio playback
///
/// # Arguments
/// * `song_id` - The song ID
/// * `server_url` - Base Navidrome server URL
/// * `subsonic_credential` - Pre-formatted credential string
/// * `transcode` - The transcode to ask for (`format` + `maxBitRate`), or
///   `None` for the file as it is
///
/// # Returns
/// Complete stream URL string, or empty string if credentials are missing
pub fn build_stream_url(
    song_id: &str,
    server_url: &str,
    subsonic_credential: &str,
    transcode: Option<StreamParams>,
) -> String {
    if song_id.is_empty() || subsonic_credential.is_empty() {
        return String::new();
    }
    // Right after the id, which stays the first param
    // (`StreamSession::song_id_of` reads it from there). The format and the
    // cap always travel together, and "raw" is never sent.
    let transcode = transcode.map_or_else(String::new, |params| {
        format!(
            "&format={}&maxBitRate={}",
            params.format.wire(),
            params.max_bitrate_kbps
        )
    });

    let cache_bust = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());

    let standard = crate::services::api::subsonic::STANDARD_PARAMS;
    format!(
        "{server_url}/rest/stream?id={song_id}{transcode}&{subsonic_credential}&{standard}&_={cache_bust}"
    )
}

/// Whether `url` asks the server for a transcode: a `/rest/stream` request
/// carrying a `format` param, which only [`build_stream_url`] puts there (for
/// the Streaming Quality setting). The decoder opens such a URL with one GET;
/// the waveform scan skips the song it plays.
pub fn requests_transcode(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|url| {
        url.path().ends_with("/rest/stream") && url.query_pairs().any(|(key, _)| key == "format")
    })
}

/// `url` asking the server to start its transcode `offset_secs` whole seconds
/// into the song (`timeOffset`), replacing any offset it already carries; 0
/// gives the URL without one. Navidrome reads the param as a whole number (a
/// fraction or a negative silently means 0), so the type allows nothing else.
/// The rest of the URL is kept as it is, `id` first included.
pub fn with_time_offset(url: &str, offset_secs: u64) -> String {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    let offset = (offset_secs > 0).then(|| format!("timeOffset={offset_secs}"));
    let params: Vec<&str> = query
        .split('&')
        .filter(|param| !param.is_empty() && param.split('=').next() != Some("timeOffset"))
        .chain(offset.as_deref())
        .collect();
    if params.is_empty() {
        path.to_owned()
    } else {
        format!("{path}?{}", params.join("&"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "0123456789abcdef";

    /// A valid image hash (Navidrome 0.64+) rides the id and replaces `_u=`.
    #[test]
    fn image_hash_rides_the_id_and_drops_the_cache_buster() {
        assert_eq!(
            build_cover_art_url_with_timestamp("a1", "http://srv", "u=x", Some(80), Some(HASH)),
            "http://srv/rest/getCoverArt?id=al-a1_0123456789abcdef&u=x&size=80&square=true&f=json&v=1.8.0&c=nokkvi"
        );
        assert_eq!(
            build_cover_art_url_with_timestamp("ar-9", "http://srv", "u=x", None, Some(HASH)),
            "http://srv/rest/getCoverArt?id=ar-9_0123456789abcdef&u=x&square=true&f=json&v=1.8.0&c=nokkvi"
        );
    }

    /// An `updated_at` (or no version) keeps today's URL byte for byte.
    #[test]
    fn timestamp_version_keeps_todays_url() {
        assert_eq!(
            build_cover_art_url_with_timestamp(
                "a1",
                "http://srv",
                "u=x",
                Some(80),
                Some("2026-05-30T00:00:00Z")
            ),
            "http://srv/rest/getCoverArt?id=al-a1&u=x&size=80&square=true&f=json&v=1.8.0&c=nokkvi&_u=2026-05-30T00:00:00Z"
        );
        assert_eq!(
            build_cover_art_url_with_timestamp("a1", "http://srv", "u=x", Some(80), None),
            "http://srv/rest/getCoverArt?id=al-a1&u=x&size=80&square=true&f=json&v=1.8.0&c=nokkvi&_u="
        );
    }

    /// Never double-suffix an id that already carries `_…` (a Subsonic
    /// `coverArt` token), never hash disc art, never touch an http URL, and
    /// never take a malformed hash.
    #[test]
    fn image_hash_is_appended_only_where_it_belongs() {
        let url = build_cover_art_url_with_timestamp(
            "ra-1_18f0c3aa",
            "http://srv",
            "u=x",
            Some(80),
            Some(HASH),
        );
        assert!(url.contains("id=ra-1_18f0c3aa&"), "{url}");
        let url = build_cover_art_url_with_timestamp(
            "dc-a1:2",
            "http://srv",
            "u=x",
            Some(80),
            Some(HASH),
        );
        assert!(url.contains("id=dc-a1:2&"), "{url}");
        let http = "https://img.example/a.jpg";
        assert_eq!(
            build_cover_art_url_with_timestamp(http, "http://srv", "u=x", Some(80), Some(HASH)),
            http
        );
        let url = build_cover_art_url_with_timestamp(
            "a1",
            "http://srv",
            "u=x",
            Some(80),
            Some("0123456789ABCDEF"),
        );
        assert!(
            url.contains("id=al-a1&") && url.ends_with("&_u=0123456789ABCDEF"),
            "{url}"
        );
    }

    #[test]
    fn test_empty_id_returns_empty() {
        assert_eq!(build_cover_art_url("", "http://srv", "cred", None), "");
    }

    #[test]
    fn test_http_url_passthrough() {
        let url = "http://example.com/art.jpg";
        assert_eq!(build_cover_art_url(url, "http://srv", "cred", None), url);
    }

    #[test]
    fn test_adds_al_prefix() {
        let url = build_cover_art_url("123", "http://srv", "u=x", Some(80));
        assert!(url.contains("id=al-123"));
    }

    #[test]
    fn test_preserves_existing_prefix() {
        let url = build_cover_art_url("ar-456", "http://srv", "u=x", Some(80));
        assert!(url.contains("id=ar-456"));
    }

    #[test]
    fn test_original_size() {
        let url = build_cover_art_url("123", "http://srv", "u=x", None);
        assert!(
            !url.contains("size="),
            "Original size should omit the size parameter"
        );
    }

    #[test]
    fn test_empty_credential_returns_empty() {
        assert_eq!(build_cover_art_url("123", "http://srv", "", None), "");
    }

    // ── Cache Key Invariants ──

    #[test]
    fn thumbnail_and_highres_share_same_id() {
        let thumb = build_cover_art_url("al-123", "http://srv", "u=x", Some(80));
        let hires = build_cover_art_url("al-123", "http://srv", "u=x", Some(HIGH_RES_SIZE));
        // Both must reference the same artwork ID
        assert!(thumb.contains("id=al-123"));
        assert!(hires.contains("id=al-123"));
        // But different sizes
        assert!(thumb.contains("size=80"));
        assert!(hires.contains(&format!("size={HIGH_RES_SIZE}")));
    }

    #[test]
    fn mf_prefix_not_double_prefixed() {
        let url = build_cover_art_url("mf-456", "http://srv", "u=x", Some(80));
        assert!(
            url.contains("id=mf-456"),
            "mf- prefix must be preserved, not double-prefixed to al-mf-456"
        );
        assert!(!url.contains("id=al-mf-456"));
    }

    #[test]
    fn disc_and_radio_prefixes_not_double_prefixed() {
        // Regression: Navidrome returns disc-numbered tracks' coverArt as
        // `dc-<albumID>:<disc>` (per-disc art, navidrome#5182) and radio art as
        // `ra-<id>`. Both are fully-formed artwork IDs — prepending `al-` yields
        // `al-dc-…`/`al-ra-…`, which Navidrome rejects with "Artwork not found"
        // (code 70). They must pass through untouched.
        let disc = build_cover_art_url(
            "dc-6nO394rZB3s7LQax01FoVn:1_0",
            "http://srv",
            "u=x",
            Some(80),
        );
        assert!(
            disc.contains("id=dc-6nO394rZB3s7LQax01FoVn:1_0"),
            "dc- disc id must be preserved: {disc}"
        );
        assert!(
            !disc.contains("id=al-dc-"),
            "dc- must not be al-prefixed: {disc}"
        );

        let radio = build_cover_art_url("ra-station9", "http://srv", "u=x", None);
        assert!(
            radio.contains("id=ra-station9"),
            "ra- id must be preserved: {radio}"
        );
        assert!(
            !radio.contains("id=al-ra-"),
            "ra- must not be al-prefixed: {radio}"
        );
    }

    #[test]
    fn timestamp_cache_buster_included() {
        let url = build_cover_art_url_with_timestamp(
            "al-123",
            "http://srv",
            "u=x",
            Some(80),
            Some("2026-01-01"),
        );
        assert!(
            url.contains("_u=2026-01-01"),
            "timestamp cache buster must be in URL"
        );
    }

    #[test]
    fn stream_url_empty_on_missing_inputs() {
        assert_eq!(
            build_stream_url("", "http://srv", "u=x", None),
            "",
            "empty song_id"
        );
        assert_eq!(
            build_stream_url("id", "http://srv", "", None),
            "",
            "empty credential"
        );
    }

    use crate::types::stream_quality::{StreamParams, TranscodeFormat};

    /// The query keys of `url`, in order.
    fn query_keys(url: &str) -> Vec<String> {
        url::Url::parse(url)
            .expect("a URL")
            .query_pairs()
            .map(|(key, _)| key.into_owned())
            .collect()
    }

    fn query_value(url: &str, key: &str) -> Option<String> {
        url::Url::parse(url)
            .expect("a URL")
            .query_pairs()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.into_owned())
    }

    #[test]
    fn only_a_stream_request_with_a_format_param_asks_for_a_transcode() {
        for (url, asks) in [
            (
                "http://srv/rest/stream?id=s1&format=opus&maxBitRate=128&u=a&t=b&s=c&f=json",
                true,
            ),
            (
                "http://srv/rest/stream?id=s1&format=mp3&maxBitRate=96",
                true,
            ),
            ("https://srv/navidrome/rest/stream?id=s1&format=opus", true),
            ("http://srv/rest/stream?id=s1&u=a&t=b&s=c&f=json&_=1", false),
            ("http://srv/rest/stream?id=s1&f=json", false),
            ("http://radio.example/live.mp3?format=mp3", false),
            ("/home/me/song.flac", false),
            ("", false),
        ] {
            assert_eq!(requests_transcode(url), asks, "{url}");
        }
    }

    /// Only a `format` KEY counts: the same text inside another param's
    /// encoded value (a title, a path) is not a request for a transcode.
    #[test]
    fn a_format_hidden_inside_an_encoded_value_is_no_transcode_request() {
        for url in [
            "http://srv/rest/stream?id=s1&title=a%26format%3Dopus&u=a",
            "http://srv/rest/stream?id=s1%26format%3Dmp3&u=a",
            "http://srv/rest/stream?id=s1&xformat=opus",
            "http://srv/rest/stream?id=s1&u=a#format=opus",
        ] {
            assert!(!requests_transcode(url), "{url}");
        }
        assert!(requests_transcode(
            "http://srv/rest/stream?id=s1&title=a%26b&format=opus&maxBitRate=128"
        ));
    }

    #[test]
    fn a_stream_url_without_a_transcode_asks_for_the_file_as_before() {
        let url = build_stream_url("s1", "http://srv", "u=x&t=tok&s=salt", None);
        assert!(
            url.starts_with("http://srv/rest/stream?id=s1&u=x&t=tok&s=salt&f=json"),
            "{url}"
        );
        let keys = query_keys(&url);
        assert!(!keys.iter().any(|k| k == "format"), "{url}");
        assert!(!keys.iter().any(|k| k == "maxBitRate"), "{url}");
    }

    #[test]
    fn a_transcode_rides_after_the_id_with_its_format_and_cap_together() {
        for (format, wire) in [
            (TranscodeFormat::Opus, "opus"),
            (TranscodeFormat::Mp3, "mp3"),
        ] {
            for kbps in [64, 128, 320] {
                let url = build_stream_url(
                    "s1",
                    "http://srv",
                    "u=x&t=tok&s=salt",
                    Some(StreamParams {
                        format,
                        max_bitrate_kbps: kbps,
                    }),
                );
                let keys = query_keys(&url);
                assert_eq!(keys.first().map(String::as_str), Some("id"), "{url}");
                assert_eq!(
                    crate::services::playback::StreamSession::song_id_of(&url),
                    Some("s1"),
                    "{url}"
                );
                assert_eq!(query_value(&url, "format").as_deref(), Some(wire), "{url}");
                assert_eq!(
                    query_value(&url, "maxBitRate"),
                    Some(kbps.to_string()),
                    "{url}"
                );
                assert_eq!(keys.iter().filter(|k| *k == "format").count(), 1, "{url}");
                assert!(!url.contains("format=raw"), "{url}");
                assert!(!url.contains("estimateContentLength"), "{url}");
            }
        }
    }

    /// `timeOffset` is always a whole, non-negative number of seconds (the
    /// server reads anything else as 0), it replaces an offset the URL
    /// already carries instead of adding a second one, and 0 asks for the
    /// stream from the start, with no param at all. The id stays first.
    #[test]
    fn a_time_offset_is_always_a_whole_non_negative_number() {
        let base = build_stream_url(
            "s1",
            "http://srv",
            "u=x&t=tok&s=salt",
            Some(StreamParams {
                format: TranscodeFormat::Opus,
                max_bitrate_kbps: 128,
            }),
        );

        let at_seven = with_time_offset(&base, 7);
        assert_eq!(
            query_value(&at_seven, "timeOffset").as_deref(),
            Some("7"),
            "{at_seven}"
        );
        assert_eq!(
            query_keys(&at_seven).first().map(String::as_str),
            Some("id")
        );
        assert_eq!(query_value(&at_seven, "format").as_deref(), Some("opus"));
        assert!(requests_transcode(&at_seven));

        let moved = with_time_offset(&at_seven, 240);
        let keys = query_keys(&moved);
        assert_eq!(
            keys.iter().filter(|k| *k == "timeOffset").count(),
            1,
            "{moved}"
        );
        assert_eq!(query_value(&moved, "timeOffset").as_deref(), Some("240"));

        assert_eq!(
            with_time_offset(&moved, 0),
            base,
            "0 is the stream from the start"
        );
        assert_eq!(with_time_offset(&base, 0), base);
        for offset in [1_u64, 59, 3_599, 86_399] {
            let value = query_value(&with_time_offset(&base, offset), "timeOffset")
                .expect("an offset param");
            assert!(
                value.chars().all(|c| c.is_ascii_digit()),
                "a whole, unsigned number: {value}"
            );
        }
    }

    // ── Per-Song Artwork URL Invariants ──

    #[test]
    fn song_artwork_url_uses_album_id_when_present() {
        let mut song = crate::types::song::Song::test_default("track-1", "Song 1");
        song.album_id = Some("abc".to_string());
        // Make cover_art a mediafile-prefixed value to prove it is NOT used.
        song.cover_art = Some("mf-track-1".to_string());

        let actual = build_song_artwork_url(&song, "http://srv", "u=x");
        let expected = build_cover_art_url("abc", "http://srv", "u=x", Some(THUMBNAIL_SIZE));
        assert_eq!(actual, expected);
        assert!(
            actual.contains("id=al-abc"),
            "must route through album_id with al- prefix"
        );
        assert!(
            !actual.contains("mf-"),
            "must NOT use song.cover_art's mf- mediafile id"
        );
    }

    #[test]
    fn song_artwork_url_empty_string_fallback_when_album_id_missing() {
        let mut song = crate::types::song::Song::test_default("track-2", "Song 2");
        song.album_id = None;
        // cover_art set but should still be ignored.
        song.cover_art = Some("mf-track-2".to_string());

        let actual = build_song_artwork_url(&song, "http://srv", "u=x");
        let expected = build_cover_art_url("", "http://srv", "u=x", Some(THUMBNAIL_SIZE));
        assert_eq!(
            actual, expected,
            "None album_id must match the empty-string fallback used today"
        );
        assert_eq!(
            actual, "",
            "empty art_id short-circuits build_cover_art_url to empty"
        );
    }

    #[test]
    fn song_artwork_url_size_is_thumbnail() {
        let mut song = crate::types::song::Song::test_default("track-3", "Song 3");
        song.album_id = Some("xyz".to_string());

        let url = build_song_artwork_url(&song, "http://srv", "u=x");
        assert!(
            url.contains(&format!("size={THUMBNAIL_SIZE}")),
            "per-song URL must request THUMBNAIL_SIZE (80px)"
        );
    }
}
