//! Albums service — data loading, on-demand artwork fetching, and UI projection
//!
//! `AlbumsService` loads albums via the Navidrome API and projects `Album`
//! models into `AlbumUIViewData` for the view layer. Artwork is fetched
//! on-demand from Navidrome (no client-side persistent cache); UI Handle
//! maps in `ArtworkState` provide session-scoped render caching.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Semaphore;
use tracing::trace;

use crate::{
    backend::{auth::AuthGateway, lazy_authed_service::LazyAuthedService},
    services::api::albums::AlbumsApiService,
    types::{album::Album, reactive::ReactiveInt},
    utils::url_redaction::redact_subsonic_url,
};

/// A `200 OK` response whose body is not an image — e.g. Navidrome answers a
/// `getCoverArt` failure with a Subsonic JSON error doc (code 70, since we pass
/// `f=json`). This is DETERMINISTIC: retrying re-fetches the identical body, so
/// `fetch_artwork_by_url_with_retry` fails fast on it (1 request) instead of
/// burning all 3 attempts, while genuinely-transient errors (429 throttle,
/// timeout, empty body) stay retryable.
#[derive(Debug, thiserror::Error)]
#[error("artwork response was not an image (content-type={content_type}): {snippet}")]
struct NonImageResponse {
    content_type: String,
    snippet: String,
}

/// True if `err` is a DETERMINISTIC "no image at this id" failure — a Navidrome
/// code-70 / non-image 200 body (see [`NonImageResponse`]) — as opposed to a
/// TRANSIENT error (HTTP 429 throttle, timeout, empty body) that a retry or a
/// later prefetch revisit could resolve. The UI artwork negative cache keys off
/// this so a transient drop is never negatively cached (which would otherwise
/// permanently blank a thumbnail that actually has art).
pub fn is_missing_artwork(err: &anyhow::Error) -> bool {
    err.downcast_ref::<NonImageResponse>().is_some()
}

/// UI-specific view data for albums
/// UI-projected data
#[derive(Debug, Clone)]
pub struct AlbumUIViewData {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub artist_id: String,
    pub song_count: u32,
    pub artwork_url: String,
    pub year: Option<u32>,
    pub genre: Option<String>,
    pub genres: Option<String>,
    pub duration: Option<f64>,
    pub is_starred: bool,
    pub play_count: Option<u32>,
    pub created_at: Option<String>,
    pub play_date: Option<String>,
    pub rating: Option<u32>,
    pub compilation: Option<bool>,
    pub size: Option<u64>,
    pub updated_at: Option<String>,
    pub mbz_album_id: Option<String>,
    pub release_type: Option<String>,
    pub comment: Option<String>,
    pub tags: Vec<(String, String)>,
    pub participants: Vec<(String, String)>,
    /// Raw release date string from Navidrome (ISO 8601, e.g. "2023-11-05")
    pub release_date: Option<String>,
    /// Raw original date string (e.g. "1973-03-24" for a remaster's original release)
    pub original_date: Option<String>,
    /// Original release year (Feishin uses max_original_year)
    pub original_year: Option<u32>,
    /// Pre-lowercased search index — built once at construction so the filter
    /// loop avoids per-keystroke `to_lowercase()` allocations. See
    /// `crate::utils::search::Searchable`.
    pub searchable_lower: String,
}

impl AlbumUIViewData {
    /// Convert an `Album` model into UI view data, building the artwork URL.
    pub fn from_album(album: &Album, server_url: &str, subsonic_credential: &str) -> Self {
        let art_id = album.cover_art.as_deref().unwrap_or(&album.id);
        // Carry the album's `updated_at` as a cache-buster so that when the
        // server-side cover changes, the grid thumbnail URL changes too — the
        // version-aware prefetch dedup then treats it as a genuine miss and
        // re-fetches (N17). Without the timestamp the passive mini path never
        // re-fetched a changed cover for the rest of the session.
        let artwork_url = crate::utils::artwork_url::build_cover_art_url_with_timestamp(
            art_id,
            server_url,
            subsonic_credential,
            Some(crate::utils::artwork_url::THUMBNAIL_SIZE),
            album.updated_at.as_deref(),
        );
        // Build genres display string: "Black Metal • Heavy Metal • Rock"
        let genres = album.genres.as_ref().map(|g| {
            g.iter()
                .map(|genre| genre.name.as_str())
                .collect::<Vec<_>>()
                .join(" \u{2022} ")
        });

        // Flatten tags HashMap into sorted (key, value) pairs for the Tags section
        let tags = Self::flatten_album_tags(album.tags.as_ref());

        // Flatten participants into sorted (role, names) pairs
        let participants = crate::backend::flatten_participants(album.participants.as_ref());

        let name = album.name.clone();
        let artist = album.display_artist().to_string();
        let searchable_lower = crate::utils::search::build_searchable_lower(&[&name, &artist]);

        Self {
            id: album.id.clone(),
            name,
            artist,
            artist_id: album
                .album_artist_id
                .clone()
                .or_else(|| album.artist_id.clone())
                .unwrap_or_default(),
            artwork_url,
            song_count: album.song_count.unwrap_or(0),
            year: album.year.or(album.max_year),
            genre: album.genre.clone(),
            genres,
            duration: album.duration,
            is_starred: album.is_starred(),
            play_count: album.play_count,
            created_at: album.created_at.clone(),
            play_date: album.play_date.clone(),
            rating: album.rating,
            compilation: album.compilation,
            size: album.size,
            updated_at: album.updated_at.clone(),
            mbz_album_id: album.mbz_album_id.clone(),
            release_type: album.mbz_album_type.clone(),
            comment: album.comment.clone(),
            tags,
            participants,
            release_date: album.release_date.clone(),
            original_date: album.original_date.clone(),
            original_year: album.max_original_year,
            searchable_lower,
        }
    }

    /// Flatten album tags HashMap into sorted (label, value) pairs for display.
    /// Filters out keys already shown as dedicated fields.
    fn flatten_album_tags(
        tags: Option<&std::collections::HashMap<String, Vec<String>>>,
    ) -> Vec<(String, String)> {
        let Some(map) = tags else {
            return Vec::new();
        };

        // Keys already displayed as dedicated fields — skip them
        // Also skip keys that Feishin extracts into dedicated fields
        const SKIP_KEYS: &[&str] = &[
            "genre",
            "artist",
            "albumartist",
            "album",
            "date",
            "comment",
            "recordlabel",
            "releasetype",
            "albumversion",
        ];

        let mut pairs: Vec<(String, String)> = map
            .iter()
            .filter(|(k, _)| !SKIP_KEYS.contains(&k.to_lowercase().as_str()))
            .map(|(k, v)| {
                // Title-case the key for display
                let label = k
                    .split('_')
                    .flat_map(|word| word.split(' '))
                    .filter(|w| !w.is_empty())
                    .map(|word| {
                        let mut chars = word.chars();
                        match chars.next() {
                            Some(c) => {
                                let upper: String = c.to_uppercase().collect();
                                format!("{upper}{}", chars.collect::<String>())
                            }
                            None => String::new(),
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                let value = v.join(" \u{2022} ");
                (label, value)
            })
            .collect();

        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        pairs
    }
}

impl crate::backend::Starable for AlbumUIViewData {
    fn entity_id(&self) -> &str {
        &self.id
    }
    fn set_starred(&mut self, starred: bool) {
        self.is_starred = starred;
    }
    fn display_label(&self) -> String {
        format!("{} - {}", self.name, self.artist)
    }
}

impl crate::backend::Ratable for AlbumUIViewData {
    fn entity_id(&self) -> &str {
        &self.id
    }
    fn set_rating(&mut self, rating: Option<u32>) {
        self.rating = rating;
    }
    fn display_label(&self) -> String {
        format!("{} - {}", self.name, self.artist)
    }
}

impl crate::utils::search::Searchable for AlbumUIViewData {
    fn matches_query(&self, query_lower: &str) -> bool {
        self.searchable_lower.contains(query_lower)
    }
}

/// Cap on simultaneously in-flight `getCoverArt` requests issued by this
/// process. Sized well below Navidrome's default throttle (`max(2, NumCPU/2)`
/// in-flight + 100 backlog) so a worst-case settle from a 25-slot viewport
/// (~250 fetches in a single tick) drains as a queue on our side instead of
/// flooding Navidrome's backlog and tripping HTTP 429s. Leaves ~85+ backlog
/// slots free for other clients/instances. The retry layer above this is
/// kept as belt-and-braces for genuine transient failures.
const ARTWORK_CONCURRENCY_LIMIT: usize = 16;

#[derive(Clone)]
pub struct AlbumsService {
    /// Lazily-initialized API service paired with its shared `AuthGateway`.
    /// Built on first `get_service()` call via `AlbumsApiService::new`.
    inner: LazyAuthedService<AlbumsApiService>,

    // Reactive properties
    pub total_count: ReactiveInt,

    /// Bare HTTP client for `getCoverArt`. No on-disk cache — every fetch goes
    /// straight to Navidrome (which has its own `ImageCacheSize` cache). Session-
    /// scoped Handle reuse is provided by the UI's `album_art` / `large_artwork`
    /// maps in `ArtworkState`.
    artwork_client: Arc<reqwest::Client>,

    /// Per-process gate that bounds concurrent in-flight artwork fetches —
    /// see [`ARTWORK_CONCURRENCY_LIMIT`].
    artwork_semaphore: Arc<Semaphore>,
}

impl Default for AlbumsService {
    fn default() -> Self {
        Self::new()
    }
}

impl AlbumsService {
    pub fn new() -> Self {
        Self {
            inner: LazyAuthedService::new(AlbumsApiService::new),
            total_count: ReactiveInt::new(0),
            artwork_client: Arc::new(
                reqwest::Client::builder()
                    .user_agent(crate::USER_AGENT)
                    .build()
                    .expect("Failed to build artwork HTTP client"),
            ),
            artwork_semaphore: Arc::new(Semaphore::new(ARTWORK_CONCURRENCY_LIMIT)),
        }
    }

    /// Fetch album artwork from Navidrome, given a fully-built URL. No client
    /// cache — every call goes to the server. Returns the raw image bytes.
    ///
    /// Acquires a permit from `artwork_semaphore` for the lifetime of the
    /// request so a viewport-wide settle (~25 slots × up to 10 fetches each)
    /// queues on our side rather than flooding Navidrome's `getCoverArt`
    /// backlog cap.
    ///
    /// Treats a zero-byte success body as an error. Navidrome's
    /// `getCoverArt` throttle middleware can return `200 OK` with an empty
    /// body when a backlog cap is exceeded (or when the server rejects a
    /// burst of concurrent requests); without this check we'd happily
    /// cache an empty `image::Handle` and never retry, leaving a few
    /// permanently-blank thumbnails after large expansions.
    pub async fn fetch_artwork_by_url(&self, url: &str) -> Result<Vec<u8>> {
        if url.is_empty() {
            return Err(anyhow::anyhow!("empty artwork url"));
        }

        let _permit = self
            .artwork_semaphore
            .acquire()
            .await
            .map_err(|e| anyhow::anyhow!("artwork semaphore closed: {e}"))?;

        let response = self
            .artwork_client
            .get(url)
            .send()
            .await
            // `reqwest::Error`'s `Display`/`Debug` embed the full request URL —
            // including the `s=`/`t=` Subsonic credential — and that text is baked
            // permanently into the `anyhow` message string here, so it must be
            // stripped at construction (no downstream redaction can recover it).
            .map_err(|e| anyhow::anyhow!("artwork fetch failed: {}", e.without_url()))?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "artwork fetch returned {}",
                response.status()
            ));
        }

        // Capture the content type before `bytes()` consumes the response.
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_ascii_lowercase);

        let bytes = response
            .bytes()
            .await
            // Unlike the send() path above, a body-read (`Decode`-kind) error carries
            // no URL in reqwest, so this `without_url()` is a no-op kept for symmetry /
            // defense-in-depth; the load-bearing strip is the send() error above.
            .map_err(|e| anyhow::anyhow!("artwork body read failed: {}", e.without_url()))?;

        if bytes.is_empty() {
            return Err(anyhow::anyhow!("artwork fetch returned 0 bytes"));
        }

        // Navidrome answers a `getCoverArt` failure with `200 OK` + a JSON error
        // body (we pass `f=json`), e.g. `{"subsonic-response":{"status":"failed",
        // "error":{"code":70,"message":"Artwork not found"}}}` — not image bytes.
        // Caching those hands every consumer (MPRIS shells, our own UI) an
        // undecodable `.jpg`, so reject anything that isn't a real image.
        if !Self::response_is_image(content_type.as_deref(), &bytes) {
            return Err(NonImageResponse {
                content_type: content_type.as_deref().unwrap_or("<none>").to_string(),
                snippet: String::from_utf8_lossy(&bytes[..bytes.len().min(180)]).into_owned(),
            }
            .into());
        }

        Ok(bytes.to_vec())
    }

    /// Guard against caching Subsonic error documents as artwork. Navidrome
    /// returns `getCoverArt` failures as `200 OK` with a JSON/XML body (we
    /// request `f=json`); those must never reach the image cache. Accept when
    /// the server labels the payload `image/*`, or when the bytes carry a known
    /// image signature — so a real cover with an odd/missing content-type still
    /// passes, while a JSON/XML/text error (neither `image/*` nor image magic)
    /// is rejected.
    fn response_is_image(content_type: Option<&str>, bytes: &[u8]) -> bool {
        let labelled_image = content_type.is_some_and(|ct| ct.starts_with("image/"));
        labelled_image || Self::has_image_magic(bytes)
    }

    /// Magic-byte sniff for the raster formats Navidrome serves as cover art
    /// (JPEG, PNG, GIF, TIFF, plus container-based WebP and AVIF/HEIC). BMP is
    /// omitted deliberately: its 2-byte "BM" signature is weak enough to match
    /// non-image bodies, and a correctly-typed image/bmp response is still
    /// accepted via the content-type branch of response_is_image.
    fn has_image_magic(bytes: &[u8]) -> bool {
        const SIGNATURES: &[&[u8]] = &[
            b"\xFF\xD8\xFF",      // JPEG
            b"\x89PNG\r\n\x1a\n", // PNG
            b"GIF87a",            // GIF
            b"GIF89a",
            b"II*\x00", // TIFF (little-endian)
            b"MM\x00*", // TIFF (big-endian)
        ];
        if SIGNATURES.iter().any(|sig| bytes.starts_with(sig)) {
            return true;
        }
        // Brands that sit past the first bytes: `RIFF????WEBP` (WebP) and
        // `????ftyp…` (AVIF/HEIC, ISO base media file format).
        if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            return true;
        }
        if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
            return true;
        }
        false
    }

    /// Convenience wrapper: build the URL from `art_id`/`size`/`updated_at` and
    /// dispatch to [`fetch_artwork_by_url`]. Used when callers don't already have
    /// the URL constructed.
    pub async fn fetch_album_artwork(
        &self,
        art_id: &str,
        size: Option<u32>,
        updated_at: Option<&str>,
    ) -> Result<Vec<u8>> {
        let (server_url, subsonic_credential) = self.get_server_config().await;
        if server_url.is_empty() || subsonic_credential.is_empty() {
            return Err(anyhow::anyhow!("missing server config"));
        }
        let url = crate::utils::artwork_url::build_cover_art_url_with_timestamp(
            art_id,
            &server_url,
            &subsonic_credential,
            size,
            updated_at,
        );
        self.fetch_artwork_by_url(&url).await
    }

    /// Burst-tolerant variant of [`fetch_artwork_by_url`]: up to 3 attempts
    /// with 100 ms / 200 ms backoff. Single retry implementation shared by
    /// every artwork-fetch caller — `fetch_album_artwork_with_retry`,
    /// `expansion_album_artwork_tasks`, and the collage path all funnel
    /// through here. Required because Navidrome's `getCoverArt` throttle
    /// middleware caps in-flight requests at `max(2, NumCPU/2)` with a
    /// 100-request backlog and rejects overflow with HTTP 429;
    /// `fetch_artwork_by_url` surfaces that as an error, and without retry
    /// the dropped request would leave a permanently-blank thumbnail until
    /// the slot was revisited.
    pub async fn fetch_artwork_by_url_with_retry(&self, url: &str) -> Result<Vec<u8>> {
        const MAX_ATTEMPTS: u32 = 3;
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..MAX_ATTEMPTS {
            if attempt > 0 {
                let backoff = 100u64 << (attempt - 1);
                tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
            }
            match self.fetch_artwork_by_url(url).await {
                Ok(bytes) => return Ok(bytes),
                Err(e) => {
                    // A non-image body is deterministic — every attempt re-fetches
                    // the same error doc. Fail fast (1 request) instead of 3, while
                    // keeping transient errors (429/timeout/empty body) retryable.
                    if e.downcast_ref::<NonImageResponse>().is_some() {
                        return Err(e);
                    }
                    last_err = Some(e);
                }
            }
        }
        let err = last_err.unwrap_or_else(|| anyhow::anyhow!("artwork fetch failed"));
        tracing::warn!(
            "Artwork fetch gave up after {} attempts for {}: {:?}",
            MAX_ATTEMPTS,
            redact_subsonic_url(url),
            err
        );
        Err(err)
    }

    /// Convenience wrapper for callers that have an `art_id` rather than a
    /// pre-built URL — builds the URL once and delegates to
    /// [`fetch_artwork_by_url_with_retry`] (no per-attempt URL rebuild).
    pub async fn fetch_album_artwork_with_retry(
        &self,
        art_id: &str,
        size: Option<u32>,
        updated_at: Option<&str>,
    ) -> Result<Vec<u8>> {
        let (server_url, subsonic_credential) = self.get_server_config().await;
        if server_url.is_empty() || subsonic_credential.is_empty() {
            return Err(anyhow::anyhow!("missing server config"));
        }
        let url = crate::utils::artwork_url::build_cover_art_url_with_timestamp(
            art_id,
            &server_url,
            &subsonic_credential,
            size,
            updated_at,
        );
        self.fetch_artwork_by_url_with_retry(&url).await
    }

    /// Associate an authentication gateway.
    ///
    /// Stores the `AuthGateway` reference. The inner `AlbumsApiService` is
    /// lazily initialized on first API call via [`get_service()`].
    pub fn with_auth(mut self, auth: AuthGateway) -> Self {
        self.inner = self.inner.with_auth(auth);
        self
    }

    /// Get the initialized API service, lazily creating it on first call.
    async fn get_service(&self) -> Result<&AlbumsApiService> {
        self.inner.get().await
    }

    /// Load a specific page of albums with explicit offset/limit, scoped to
    /// the given `library_ids` via the orthogonal Native API filter. An
    /// empty slice omits the param entirely (Navidrome auto-scopes to
    /// libraries the user can access).
    #[allow(clippy::too_many_arguments)]
    pub async fn load_raw_albums_page_with_libraries(
        &self,
        sort_mode: Option<&str>,
        sort_order: Option<&str>,
        search_query: Option<&str>,
        filter: Option<&crate::types::filter::LibraryFilter>,
        library_ids: &[i32],
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Album>> {
        let service = self.get_service().await?;

        let sort_mode = sort_mode.unwrap_or("recentlyAdded");
        let sort_order = sort_order.unwrap_or("DESC");
        let search_opt = search_query.filter(|s| !s.is_empty());

        match service
            .load_albums(
                sort_mode,
                sort_order,
                search_opt,
                filter,
                library_ids,
                Some(offset),
                Some(limit),
            )
            .await
        {
            Ok((mut albums, total_count)) => {
                // Populate display_artist_cached to eliminate repeated .to_string() allocations during scrolling
                for album in &mut albums {
                    album.display_artist_cached = album.display_artist().to_string();
                }

                // Set the total_count reactive property
                self.total_count.set(total_count as i32);
                trace!(
                    " AlbumsService.load_raw_albums_page_with_libraries: offset={}, limit={}, got={}, total={}",
                    offset,
                    limit,
                    albums.len(),
                    total_count
                );
                Ok(albums)
            }
            Err(e) => Err(e),
        }
    }

    /// Get total count (reactive property)
    pub fn get_total_count(&self) -> i32 {
        self.total_count.get()
    }

    /// Get server configuration for artwork URLs
    pub async fn get_server_config(&self) -> (String, String) {
        self.inner.server_config().await
    }

    /// Load all songs for an album
    /// Returns Vec<Song> for adding to queue
    pub async fn load_album_songs(&self, album_id: &str) -> Result<Vec<crate::types::song::Song>> {
        let songs_service = self
            .inner
            .build_authed(crate::services::api::songs::SongsApiService::new)
            .await?;
        songs_service.load_album_songs(album_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn album_from_json(value: serde_json::Value) -> Album {
        serde_json::from_value(value).expect("valid album json")
    }

    /// The UI negative cache keys off `is_missing_artwork`: a deterministic
    /// non-image body (Navidrome code-70) is negative-cacheable, but a transient
    /// error (429 throttle, timeout, empty body) must NOT be — caching the latter
    /// would permanently blank a thumbnail that actually has art.
    #[test]
    fn is_missing_artwork_distinguishes_deterministic_from_transient() {
        let missing: anyhow::Error = NonImageResponse {
            content_type: "application/json".to_string(),
            snippet: "{\"subsonic-response\":{\"status\":\"failed\"}}".to_string(),
        }
        .into();
        assert!(
            is_missing_artwork(&missing),
            "a code-70 non-image body is a deterministic miss"
        );

        for transient in [
            anyhow::anyhow!("artwork fetch returned 429"),
            anyhow::anyhow!("artwork fetch failed: timeout"),
            anyhow::anyhow!("artwork fetch returned 0 bytes"),
        ] {
            assert!(
                !is_missing_artwork(&transient),
                "transient errors must not be treated as a deterministic miss: {transient}"
            );
        }
    }

    /// Regression guard for the leak that surfaced in a user's MPRIS "art fetch
    /// failed" WARN: a transport failure on the REAL artwork path must not carry
    /// the `s=`/`t=` Subsonic credential into the returned error (it is logged at
    /// WARN level and ships in bug-report logs). Unlike the `without_url()` unit
    /// test in `utils::url_redaction`, this exercises the actual call site, so it
    /// fails if a future edit drops the strip at `fetch_artwork_by_url`.
    #[tokio::test]
    async fn fetch_artwork_by_url_error_omits_subsonic_credential() {
        // Bind :0 then drop it, so the port is guaranteed closed (connection
        // refused) without colliding with a real listener under parallel runs.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().expect("local addr").port();
        drop(listener);
        let url = format!(
            "http://127.0.0.1:{port}/rest/getCoverArt\
             ?id=al-1&u=demo&s=SALT_SECRET&t=TOKEN_SECRET&f=json"
        );

        let err = AlbumsService::new()
            .fetch_artwork_by_url(&url)
            .await
            .expect_err("a connection to a closed port must fail");

        let rendered = format!("{err:?}");
        for needle in ["SALT_SECRET", "TOKEN_SECRET"] {
            assert!(
                !rendered.contains(needle),
                "credential leaked from fetch_artwork_by_url error: {rendered}"
            );
        }
    }

    /// The grid thumbnail URL must embed the album's `updated_at` as the
    /// `_u=` cache-buster so a server-side cover change becomes a genuine
    /// prefetch miss (N17). Previously `from_album` built the URL with no
    /// timestamp, so `_u=` was always empty and the passive mini path never
    /// re-fetched a changed cover for the rest of the session.
    #[test]
    fn from_album_thumbnail_includes_updated_at_cache_buster() {
        let album = album_from_json(serde_json::json!({
            "id": "al-1",
            "name": "Test Album",
            "updatedAt": "2026-05-30T00:00:00Z",
        }));

        let view = AlbumUIViewData::from_album(&album, "http://srv", "u=x");

        assert!(
            view.artwork_url.contains("_u=2026-05-30T00:00:00Z"),
            "thumbnail URL must carry the updated_at cache-buster, got: {}",
            view.artwork_url
        );
    }

    /// When `updated_at` is absent the URL still builds (with an empty
    /// `_u=`), matching the historical no-timestamp shape — no regression
    /// for servers that don't expose the field.
    #[test]
    fn from_album_thumbnail_empty_cache_buster_when_no_updated_at() {
        let album = album_from_json(serde_json::json!({
            "id": "al-2",
            "name": "No Timestamp",
        }));

        let view = AlbumUIViewData::from_album(&album, "http://srv", "u=x");

        assert!(view.artwork_url.contains("id=al-2"));
        assert!(view.artwork_url.ends_with("_u="));
    }
}
