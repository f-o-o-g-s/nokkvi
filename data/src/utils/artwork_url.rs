//! Centralized artwork URL generation and fetching for Navidrome Subsonic API
//!
//! This module provides a single source of truth for building cover art URLs
//! and fetching artwork via POST (credentials in body, not URL).

/// Known Subsonic cover art ID prefixes
const KNOWN_PREFIXES: [&str; 5] = ["al-", "ar-", "mf-", "pl-", "sh-"];

/// Default size for high-resolution artwork (matches QML client)
pub const HIGH_RES_SIZE: u32 = 1000;

/// Default size for thumbnails
pub const THUMBNAIL_SIZE: u32 = 80;

/// Safely construct a consistent string cache key for a given artwork ID and size
/// Maps the requested size to the filename string. Omitted size is 'original'.
pub fn build_cache_key(art_id: &str, size: Option<u32>) -> String {
    let normalized_id = if KNOWN_PREFIXES
        .iter()
        .any(|prefix| art_id.starts_with(prefix))
    {
        art_id.to_string()
    } else {
        format!("al-{art_id}")
    };

    match size {
        Some(s) => format!("{normalized_id}_{s}"),
        None => format!("{normalized_id}_original"),
    }
}

/// Parse album ID and size from a getCoverArt URL to build a stable cache key.
///
/// This ensures that URLs with and without the `size` parameter map to the same
/// cache key format as `build_cache_key`.
pub fn parse_cache_key_from_url(artwork_url: &str) -> String {
    let album_id = artwork_url
        .split("id=")
        .nth(1)
        .and_then(|s| s.split('&').next())
        .unwrap_or("unknown");
    let requested_size: Option<u32> = artwork_url
        .split("size=")
        .nth(1)
        .and_then(|s| s.split('&').next())
        .and_then(|s| s.parse().ok());
    build_cache_key(album_id, requested_size)
}

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

/// Build the POST endpoint URL and form body for a getCoverArt request.
///
/// Returns `(url, form_body)` where:
/// - `url` is the endpoint without credentials: `{server_url}/rest/getCoverArt`
/// - `form_body` is `application/x-www-form-urlencoded` with all params including credentials
///
/// Returns `None` if art_id is empty or credentials are missing.
pub fn build_cover_art_post_params(
    art_id: &str,
    server_url: &str,
    subsonic_credential: &str,
    size: Option<u32>,
    updated_at: Option<&str>,
) -> Option<(String, String)> {
    if art_id.is_empty() || subsonic_credential.is_empty() {
        return None;
    }

    // HTTP URLs are external, can't POST to them
    if art_id.starts_with("http") {
        return None;
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

    let actual_size = size.unwrap_or(HIGH_RES_SIZE);
    let cache_buster = updated_at.unwrap_or("");
    let url = format!("{server_url}/rest/getCoverArt");
    let body = format!(
        "id={final_id}&{subsonic_credential}&size={actual_size}&square=true&f=json&v=1.8.0&c=nokkvi&_u={cache_buster}"
    );

    Some((url, body))
}

/// Fetch cover art via POST request (credentials in form body, not URL).
///
/// Falls back to GET for external HTTP URLs that can't be POSTed to.
/// Returns raw image bytes on success.
pub async fn fetch_cover_art(
    client: &reqwest::Client,
    art_id: &str,
    server_url: &str,
    subsonic_credential: &str,
    size: Option<u32>,
) -> Option<Vec<u8>> {
    if art_id.is_empty() || subsonic_credential.is_empty() {
        return None;
    }

    // External HTTP URLs: GET directly (can't POST to third-party servers)
    if art_id.starts_with("http") {
        let response = client.get(art_id).send().await.ok()?;
        if response.status().is_success() {
            return response.bytes().await.ok().map(|b| b.to_vec());
        }
        return None;
    }

    let (url, body) =
        build_cover_art_post_params(art_id, server_url, subsonic_credential, size, None)?;

    let response = client
        .post(&url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .ok()?;

    if response.status().is_success() {
        response.bytes().await.ok().map(|b| b.to_vec())
    } else {
        None
    }
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
    fn post_params_id_matches_get_url_id() {
        let get_url = build_cover_art_url("al-abc", "http://srv", "u=x", Some(80));
        let (_, post_body) =
            build_cover_art_post_params("al-abc", "http://srv", "u=x", Some(80), None)
                .expect("should produce params");
        // Both must contain identical id= values
        assert!(get_url.contains("id=al-abc"));
        assert!(post_body.contains("id=al-abc"));
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

    #[test]
    fn test_build_cache_key() {
        assert_eq!(build_cache_key("al-123", Some(1000)), "al-123_1000");
        assert_eq!(build_cache_key("ar-xyz", Some(1500)), "ar-xyz_1500");
        assert_eq!(build_cache_key("mf-abc", None), "mf-abc_original");

        // Also handle auto-prefixing if art_id misses known prefixes
        assert_eq!(build_cache_key("123", Some(80)), "al-123_80");
        assert_eq!(build_cache_key("xyz", None), "al-xyz_original");
    }

    #[test]
    fn test_parse_cache_key_from_url() {
        let url_with_size = "http://srv/rest/getCoverArt?id=al-123&u=x&p=y&size=80";
        assert_eq!(parse_cache_key_from_url(url_with_size), "al-123_80");

        let url_original = "http://srv/rest/getCoverArt?id=al-abc&u=x&p=y";
        assert_eq!(parse_cache_key_from_url(url_original), "al-abc_original");

        // Regresson test for #large-artwork-bug: ensure no size doesn't fallback to _0
        let url_no_size = "http://srv/rest/getCoverArt?id=123";
        assert_eq!(parse_cache_key_from_url(url_no_size), "al-123_original");
    }
}
