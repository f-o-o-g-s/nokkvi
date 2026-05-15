//! Centralized artwork URL generation for Navidrome Subsonic API.
//!
//! Single source of truth for `getCoverArt` URLs. Fetches go through
//! `AlbumsService::fetch_album_artwork` against the bare reqwest client; there
//! is no client-side cache, so URL form is the only thing that matters here.

use crate::types::song::Song;

/// Known Subsonic cover art ID prefixes
const KNOWN_PREFIXES: [&str; 5] = ["al-", "ar-", "mf-", "pl-", "sh-"];

/// Default size for high-resolution artwork (matches QML client)
pub const HIGH_RES_SIZE: u32 = 1000;

/// Default size for thumbnails
pub const THUMBNAIL_SIZE: u32 = 80;

/// Build a Subsonic getCoverArt URL
///
/// # Arguments
/// * `art_id` - The artwork identifier (album ID, cover_art field, etc.)
/// * `server_url` - Base Navidrome server URL
/// * `subsonic_credential` - Pre-formatted credential string (e.g., "u=user&t=token&s=salt")
/// * `size` - Optional size in pixels (square). If None, uses HIGH_RES_SIZE (1000)
///
/// # Returns
/// Complete URL string, or empty string if credentials are missing
///
/// # Examples
/// ```ignore
/// let url = build_cover_art_url("al-abc123", "http://server", "u=user&t=tok&s=salt", Some(80), None);
/// ```
pub fn build_cover_art_url(
    art_id: &str,
    server_url: &str,
    subsonic_credential: &str,
    size: Option<u32>,
) -> String {
    build_cover_art_url_with_timestamp(art_id, server_url, subsonic_credential, size, None)
}

/// Build cover art URL with optional updated_at timestamp for cache invalidation
/// When artwork is updated on the server, the timestamp changes and triggers re-download
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

    if !subsonic_credential.is_empty() {
        // Include updated_at in URL for cache invalidation
        // This becomes part of the URL hash, so changed artwork = new cache file
        let cache_buster = updated_at.unwrap_or("");
        format!(
            "{server_url}/rest/getCoverArt?id={final_id}&{subsonic_credential}{size_param}&square=true&f=json&v=1.8.0&c=nokkvi&_u={cache_buster}"
        )
    } else {
        String::new()
    }
}

/// Build the thumbnail artwork URL for a song.
///
/// **Invariant**: uses the song's `album_id`, NOT `song.cover_art`. The
/// Subsonic API returns `cover_art` as `mf-{mediafile_id}` for playlist
/// songs, but background prefetch caches thumbnails under
/// `al-{album_id}_80`. Routing through `cover_art` would create a cache
/// key mismatch — every playlist song would miss the disk cache,
/// triggering a network fetch and leaving ~90% of thumbnails blank.
///
/// Always uses [`THUMBNAIL_SIZE`].
pub fn build_song_artwork_url(song: &Song, server_url: &str, subsonic_credential: &str) -> String {
    let album_id = song.album_id.as_deref().unwrap_or_default();
    build_cover_art_url(
        album_id,
        server_url,
        subsonic_credential,
        Some(THUMBNAIL_SIZE),
    )
}

/// Build a Subsonic stream URL for audio playback
///
/// # Arguments
/// * `song_id` - The song ID
/// * `server_url` - Base Navidrome server URL
/// * `subsonic_credential` - Pre-formatted credential string
///
/// # Returns
/// Complete stream URL string, or empty string if credentials are missing
pub fn build_stream_url(song_id: &str, server_url: &str, subsonic_credential: &str) -> String {
    if song_id.is_empty() || subsonic_credential.is_empty() {
        return String::new();
    }

    let cache_bust = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());

    format!(
        "{server_url}/rest/stream?id={song_id}&{subsonic_credential}&f=json&v=1.8.0&c=nokkvi&_={cache_bust}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
            build_stream_url("", "http://srv", "u=x"),
            "",
            "empty song_id"
        );
        assert_eq!(
            build_stream_url("id", "http://srv", ""),
            "",
            "empty credential"
        );
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
