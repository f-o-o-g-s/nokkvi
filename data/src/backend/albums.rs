//! Albums service — data loading, on-demand artwork fetching, and UI projection
//!
//! `AlbumsService` loads albums via the Navidrome API and projects `Album`
//! models into `AlbumUIViewData` for the view layer. Artwork is fetched
//! on-demand from Navidrome (no client-side persistent cache); UI Handle
//! maps in `ArtworkState` provide session-scoped render caching.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::OnceCell;
use tracing::debug;

use crate::{
    backend::auth::AuthGateway,
    services::api::albums::AlbumsApiService,
    types::{album::Album, reactive::ReactiveInt},
};

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
}

impl AlbumUIViewData {
    /// Convert an `Album` model into UI view data, building the artwork URL.
    pub fn from_album(album: &Album, server_url: &str, subsonic_credential: &str) -> Self {
        let art_id = album.cover_art.as_deref().unwrap_or(&album.id);
        let artwork_url = crate::utils::artwork_url::build_cover_art_url(
            art_id,
            server_url,
            subsonic_credential,
            Some(80),
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

        Self {
            id: album.id.clone(),
            name: album.name.clone(),
            artist: album.display_artist().to_string(),
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
    fn searchable_fields(&self) -> Vec<&str> {
        vec![&self.name, &self.artist]
    }
}

#[derive(Clone)]
pub struct AlbumsService {
    // API service (lazily initialized on first use after login)
    albums_service: Arc<OnceCell<AlbumsApiService>>,

    // Reactive properties
    pub total_count: ReactiveInt,

    /// Bare HTTP client for `getCoverArt`. No on-disk cache — every fetch goes
    /// straight to Navidrome (which has its own `ImageCacheSize` cache). Session-
    /// scoped Handle reuse is provided by the UI's `album_art` / `large_artwork`
    /// maps in `ArtworkState`.
    artwork_client: Arc<reqwest::Client>,

    // Dependencies
    auth_gateway: Arc<OnceCell<AuthGateway>>,
}

impl Default for AlbumsService {
    fn default() -> Self {
        Self::new()
    }
}

impl AlbumsService {
    pub fn new() -> Self {
        Self {
            albums_service: Arc::new(OnceCell::new()),
            total_count: ReactiveInt::new(0),
            artwork_client: Arc::new(reqwest::Client::new()),
            auth_gateway: Arc::new(OnceCell::new()),
        }
    }

    /// Fetch album artwork from Navidrome, given a fully-built URL. No client
    /// cache — every call goes to the server. Returns the raw image bytes.
    pub async fn fetch_artwork_by_url(&self, url: &str) -> Result<Vec<u8>> {
        if url.is_empty() {
            return Err(anyhow::anyhow!("empty artwork url"));
        }

        let response = self
            .artwork_client
            .get(url)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("artwork fetch failed: {e}"))?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "artwork fetch returned {}",
                response.status()
            ));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| anyhow::anyhow!("artwork body read failed: {e}"))?;

        Ok(bytes.to_vec())
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

    /// Associate an authentication gateway.
    ///
    /// Stores the `AuthGateway` reference. The inner `AlbumsApiService` is
    /// lazily initialized on first API call via [`get_service()`].
    pub fn with_auth(self, auth: AuthGateway) -> Self {
        let _ = self.auth_gateway.set(auth);
        self
    }

    /// Get the initialized API service, lazily creating it on first call.
    ///
    /// Uses `OnceCell::get_or_try_init` for atomic init-once semantics
    /// and lock-free reads on subsequent calls.
    async fn get_service(&self) -> Result<&AlbumsApiService> {
        self.albums_service
            .get_or_try_init(|| async {
                let auth = self.auth_gateway.get().ok_or_else(|| {
                    anyhow::anyhow!("AlbumsService not initialized. Please authenticate first.")
                })?;
                let client = auth.get_client().await.ok_or_else(|| {
                    anyhow::anyhow!("AlbumsService not initialized. Please authenticate first.")
                })?;
                Ok(AlbumsApiService::new(client, String::new(), String::new()))
            })
            .await
    }

    /// No-op kept for API compatibility with the Settings → Clear Artwork
    /// Cache handler. The on-disk cache is gone; UI Handle maps live in the
    /// frontend and are cleared there.
    pub async fn clear_and_reset_cache(&self) -> usize {
        0
    }

    /// Load albums and return raw Album structs (first page only).
    /// Uses PAGE_SIZE as the default limit for pagination.
    pub async fn load_raw_albums(
        &self,
        sort_mode: Option<&str>,
        sort_order: Option<&str>,
        search_query: Option<&str>,
        filter: Option<&crate::types::filter::LibraryFilter>,
    ) -> Result<Vec<Album>> {
        use crate::types::paged_buffer::PAGE_SIZE;
        self.load_raw_albums_page(sort_mode, sort_order, search_query, filter, 0, PAGE_SIZE)
            .await
    }

    /// Load a specific page of albums with explicit offset/limit.
    pub async fn load_raw_albums_page(
        &self,
        sort_mode: Option<&str>,
        sort_order: Option<&str>,
        search_query: Option<&str>,
        filter: Option<&crate::types::filter::LibraryFilter>,
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
                debug!(
                    " AlbumsService.load_raw_albums_page: offset={}, limit={}, got={}, total={}",
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
        if let Some(auth) = self.auth_gateway.get() {
            let server_url = auth.get_server_url().await;
            let subsonic_credential = auth.get_subsonic_credential().await;
            (server_url, subsonic_credential)
        } else {
            (String::new(), String::new())
        }
    }

    /// Load all songs for an album
    /// Returns Vec<Song> for adding to queue
    pub async fn load_album_songs(&self, album_id: &str) -> Result<Vec<crate::types::song::Song>> {
        let auth = self
            .auth_gateway
            .get()
            .ok_or_else(|| anyhow::anyhow!("Not authenticated"))?;

        let client = auth
            .get_client()
            .await
            .ok_or_else(|| anyhow::anyhow!("No API client"))?;

        let server_url = auth.get_server_url().await;
        let subsonic_credential = auth.get_subsonic_credential().await;

        let songs_service = crate::services::api::songs::SongsApiService::new(
            client,
            server_url,
            subsonic_credential,
        );

        songs_service.load_album_songs(album_id).await
    }
}
