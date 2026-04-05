//! Albums service — data loading, artwork caching, and UI projection
//!
//! `AlbumsService` loads albums via the Navidrome API, manages a two-tier
//! artwork cache (in-memory LRU + on-disk), and projects `Album` models
//! into `AlbumUIViewData` for the view layer.

use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use tokio::sync::{Mutex, OnceCell};
use tracing::debug;

use crate::{
    backend::auth::AuthGateway,
    services::api::albums::AlbumsApiService,
    types::{album::Album, reactive::ReactiveInt},
    utils::cache::DiskCache,
};

/// UI-specific view data for albums
/// UI-projected data
#[derive(Debug, Clone)]
pub struct AlbumUIViewData {
    pub id: String,
    pub name: String,
    pub artist: String,
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

    // Internal state for caching
    artwork_cache: Arc<Mutex<lru::LruCache<String, Vec<u8>>>>,
    disk_cache: Arc<Option<DiskCache>>,
    prefetch_started: Arc<std::sync::atomic::AtomicBool>,

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
            artwork_cache: Arc::new(Mutex::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(50).expect("non-zero cap"),
            ))),
            disk_cache: Arc::new(DiskCache::new("artwork")),
            prefetch_started: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            auth_gateway: Arc::new(OnceCell::new()),
        }
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

    /// Clear the album artwork disk + memory caches and reset the prefetch flag
    /// so `start_artwork_prefetch()` will re-run.
    pub async fn clear_and_reset_cache(&self) -> usize {
        // Clear disk cache
        let removed = if let Some(dc) = self.disk_cache.as_ref() {
            dc.clear()
        } else {
            0
        };
        // Clear in-memory LRU
        self.artwork_cache.lock().await.clear();
        // Reset prefetch flag so it can run again
        self.prefetch_started
            .store(false, std::sync::atomic::Ordering::SeqCst);
        removed
    }

    /// Load albums and return raw Album structs (first page only).
    /// Uses PAGE_SIZE as the default limit for pagination.
    pub async fn load_raw_albums(
        &self,
        sort_mode: Option<&str>,
        sort_order: Option<&str>,
        search_query: Option<&str>,
    ) -> Result<Vec<Album>> {
        use crate::types::paged_buffer::PAGE_SIZE;
        self.load_raw_albums_page(sort_mode, sort_order, search_query, 0, PAGE_SIZE)
            .await
    }

    /// Load a specific page of albums with explicit offset/limit.
    pub async fn load_raw_albums_page(
        &self,
        sort_mode: Option<&str>,
        sort_order: Option<&str>,
        search_query: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Album>> {
        let service = self.get_service().await?;

        let sort_mode = sort_mode.unwrap_or("recentlyAdded");
        let sort_order = sort_order.unwrap_or("DESC");
        let search_opt = search_query.filter(|s| !s.is_empty());

        match service
            .load_albums(sort_mode, sort_order, search_opt, Some(offset), Some(limit))
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

    /// Load album artwork and return as raw bytes (which is Send)
    ///
    /// Optimization: If the requested size isn't cached, try resizing from a larger
    /// cached version (e.g., 1000px) to avoid network calls. This is especially useful
    /// for genres/playlists which need 300px artwork that may not be prefetched.
    pub async fn load_album_artwork_buffer(
        &self,
        artwork_url: &str,
        target_size: Option<u32>,
    ) -> Option<Vec<u8>> {
        if artwork_url.is_empty() {
            return None;
        }

        // Check in-memory cache first
        {
            let mut cache = self.artwork_cache.lock().await;
            if let Some(cached) = cache.get(artwork_url) {
                return Some(cached.clone());
            }
        }

        // Extract album ID and size from URL for stable cache key (URL contains changing auth tokens)
        let album_id = artwork_url
            .split("id=")
            .nth(1)
            .and_then(|s| s.split('&').next())
            .unwrap_or("unknown");
        let requested_size: u32 = artwork_url
            .split("size=")
            .nth(1)
            .and_then(|s| s.split('&').next())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let cache_key = format!("{album_id}_{requested_size}");

        // Check disk cache using stable album ID + size as key
        if let Some(dc) = self.disk_cache.as_ref() {
            // 1. Try exact size match first
            if let Some(cached) = dc.get(&cache_key) {
                // Cache in memory and return
                let mut mem_cache = self.artwork_cache.lock().await;
                mem_cache.put(artwork_url.to_string(), cached.clone());
                return Some(cached);
            }

            // 2. Try resizing from larger cached sizes (1000px is prefetched on startup)
            let target = target_size.unwrap_or(requested_size);
            if target > 0 && target < 1000 {
                // Common prefetched sizes to try (larger first)
                for fallback_size in [1000u32, 500, 300] {
                    if fallback_size <= target {
                        break; // No point resizing from smaller
                    }
                    let fallback_key = format!("{album_id}_{fallback_size}");
                    if let Some(large_bytes) = dc.get(&fallback_key)
                        && let Some(resized) = Self::resize_artwork_bytes(&large_bytes, target)
                    {
                        debug!(
                            " [RESIZE] {}px → {}px for {}",
                            fallback_size, target, album_id
                        );

                        // Cache resized version to disk for future runs (avoids repeated resize CPU cost)
                        let resized_key = format!("{album_id}_{target}");
                        dc.insert(&resized_key, &resized);

                        // Also cache in memory
                        let mut mem_cache = self.artwork_cache.lock().await;
                        mem_cache.put(artwork_url.to_string(), resized.clone());
                        return Some(resized);
                    }
                }
            }
        }

        // 3. No usable cache - fetch from network via POST (credentials in body, not URL)
        // Use get_service() to lazily initialize the API client. Using .get() alone
        // returns None when genres/playlists load before albums, causing all collage
        // tile fetches to silently fail.
        let service = match self.get_service().await {
            Ok(s) => s,
            Err(e) => {
                debug!(
                    " [ARTWORK] Network fallback skipped — {} (album_id={})",
                    e, album_id
                );
                return None;
            }
        };
        let client = service.get_http_client();

        let (server_url, subsonic_credential) = self.get_server_config().await;

        let bytes = crate::utils::artwork_url::fetch_cover_art(
            &client,
            album_id,
            &server_url,
            &subsonic_credential,
            Some(requested_size),
        )
        .await?;

        // Save to disk cache using stable album ID + size as key
        if let Some(dc) = self.disk_cache.as_ref() {
            dc.insert(&cache_key, &bytes);
        }

        // Cache in memory
        {
            let mut cache = self.artwork_cache.lock().await;
            cache.put(artwork_url.to_string(), bytes.clone());
        }

        Some(bytes)
    }

    /// Get the disk cache path for album artwork, ensuring the file exists.
    ///
    /// Mirrors `load_album_artwork_buffer()` logic (disk cache check, resize fallback,
    /// network fetch) but returns the stable file path instead of bytes. This enables
    /// `Handle::from_path()` which produces stable hash-based IDs that Iced's GPU
    /// texture cache can recognize across frames, eliminating first-frame flicker.
    pub async fn get_artwork_cache_path(
        &self,
        artwork_url: &str,
        target_size: Option<u32>,
    ) -> Option<PathBuf> {
        if artwork_url.is_empty() {
            return None;
        }

        let album_id = artwork_url
            .split("id=")
            .nth(1)
            .and_then(|s| s.split('&').next())
            .unwrap_or("unknown");
        let requested_size: u32 = artwork_url
            .split("size=")
            .nth(1)
            .and_then(|s| s.split('&').next())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let cache_key = format!("{album_id}_{requested_size}");

        let dc = self.disk_cache.as_ref().as_ref()?;

        // 1. Exact size match on disk
        if dc.contains(&cache_key) {
            return Some(dc.get_path(&cache_key));
        }

        // 2. Resize fallback from larger cached sizes
        let target = target_size.unwrap_or(requested_size);
        if target > 0 && target < 1000 {
            for fallback_size in [1000u32, 500, 300] {
                if fallback_size <= target {
                    break;
                }
                let fallback_key = format!("{album_id}_{fallback_size}");
                if let Some(large_bytes) = dc.get(&fallback_key)
                    && let Some(resized) = Self::resize_artwork_bytes(&large_bytes, target)
                {
                    debug!(
                        " [RESIZE→PATH] {}px → {}px for {}",
                        fallback_size, target, album_id
                    );
                    let resized_key = format!("{album_id}_{target}");
                    dc.insert(&resized_key, &resized);
                    return Some(dc.get_path(&resized_key));
                }
            }
        }

        // 3. Fetch from network, write to disk, return path
        let client = self.get_service().await.ok()?.get_http_client();
        let (server_url, subsonic_credential) = self.get_server_config().await;

        let bytes = crate::utils::artwork_url::fetch_cover_art(
            &client,
            album_id,
            &server_url,
            &subsonic_credential,
            Some(requested_size),
        )
        .await?;

        dc.insert(&cache_key, &bytes);
        Some(dc.get_path(&cache_key))
    }

    /// Get cache path for a given art_id + size directly (no URL parsing).
    ///
    /// Returns `Some(path)` if the file exists on disk, `None` otherwise.
    /// Used by artist/collage code that already has the art ID.
    pub fn get_cache_path_for_id(&self, art_id: &str, size: u32) -> Option<PathBuf> {
        let dc = self.disk_cache.as_ref().as_ref()?;
        let cache_key = format!("{art_id}_{size}");
        if dc.contains(&cache_key) {
            Some(dc.get_path(&cache_key))
        } else {
            None
        }
    }

    /// Resize image bytes to target dimensions, returning JPEG-encoded bytes.
    /// Returns None if decoding or encoding fails.
    fn resize_artwork_bytes(bytes: &[u8], target_size: u32) -> Option<Vec<u8>> {
        use std::io::Cursor;

        use image::{GenericImageView, ImageFormat};

        // Decode the source image
        let img = image::load_from_memory(bytes).ok()?;

        // Skip if already smaller than target
        let (width, height) = img.dimensions();
        if width <= target_size && height <= target_size {
            return Some(bytes.to_vec());
        }

        // Resize using Lanczos3 for quality (good for downscaling)
        let resized = img.resize(
            target_size,
            target_size,
            image::imageops::FilterType::Lanczos3,
        );

        // Encode as JPEG (good compression, fast)
        let mut output = Cursor::new(Vec::new());
        resized.write_to(&mut output, ImageFormat::Jpeg).ok()?;

        Some(output.into_inner())
    }

    /// Start background prefetch of all album artwork
    /// This downloads thumbnails and large artwork for all albums in the background
    /// Only runs once per session and skips if cache is already mostly complete
    pub async fn start_artwork_prefetch(
        &self,
        progress: Option<crate::types::progress::ProgressHandle>,
        high_res_size: Option<u32>,
    ) {
        use std::sync::atomic::Ordering;

        // Only run once per session
        if self.prefetch_started.swap(true, Ordering::SeqCst) {
            debug!(" [PREFETCH] Already started, skipping");
            return;
        }

        // Check if cache is already mostly complete
        let total_albums = self.total_count.get() as usize;
        if !crate::services::artwork_prefetch::is_cache_incomplete(&self.disk_cache, total_albums) {
            debug!(
                " [PREFETCH] Cache appears complete ({} albums), skipping",
                total_albums
            );
            return;
        }

        // Get all albums for prefetching, paginating if there are many
        let mut albums = Vec::new();
        if let Some(service) = self.albums_service.get() {
            let mut offset = 0;
            let limit = 500;
            loop {
                match service
                    .load_albums("name", "ASC", None, Some(offset), Some(limit))
                    .await
                {
                    Ok((mut batch, total_count)) => {
                        let batch_len = batch.len();
                        albums.append(&mut batch);
                        if batch_len < limit || albums.len() >= total_count as usize {
                            break;
                        }
                        offset += limit;
                    }
                    Err(e) => {
                        debug!(" [PREFETCH] Failed to load albums batch: {}", e);
                        if albums.is_empty() {
                            return;
                        }
                        break; // proceed with what we have
                    }
                }
            }
        } else {
            debug!(" [PREFETCH] Service not initialized");
            return;
        };

        // Get server config
        let (server_url, subsonic_credential) = self.get_server_config().await;

        if server_url.is_empty() || subsonic_credential.is_empty() {
            debug!(" [PREFETCH] Missing server config");
            return;
        }

        debug!(
            " [PREFETCH] Starting background prefetch for {} albums...",
            albums.len()
        );

        // Start the prefetch in background
        let _rx = crate::services::artwork_prefetch::start_prefetch(
            albums,
            server_url,
            subsonic_credential,
            self.disk_cache.clone(),
            progress,
            high_res_size,
        );
    }

    /// Refresh artwork for a single album: evict from disk + memory caches, re-fetch all sizes.
    ///
    /// This allows updating one album's artwork without rebuilding the entire cache.
    /// Re-fetches thumbnail (80px) and the specified high-res size from the server.
    /// Returns the raw bytes for each size so callers can build handles directly,
    /// bypassing any cache re-read race conditions.
    pub async fn refresh_single_album_artwork(
        &self,
        album_id: &str,
        high_res_size: Option<u32>,
    ) -> Result<Vec<(Option<u32>, Vec<u8>)>> {
        use crate::utils::artwork_url;

        let (server_url, subsonic_credential) = self.get_server_config().await;
        if server_url.is_empty() || subsonic_credential.is_empty() {
            return Err(anyhow::anyhow!("Missing server config"));
        }

        // Normalize art_id (add "al-" prefix if needed)
        let art_id = if album_id.starts_with("al-")
            || album_id.starts_with("ar-")
            || album_id.starts_with("mf-")
        {
            album_id.to_string()
        } else {
            format!("al-{album_id}")
        };

        let sizes = vec![Some(artwork_url::THUMBNAIL_SIZE), high_res_size];
        let mut fetched = Vec::new();

        // Evict from disk cache and re-fetch each size
        for size in sizes {
            let cache_key = artwork_url::build_cache_key(&art_id, size);

            // Remove from disk cache
            if let Some(dc) = self.disk_cache.as_ref() {
                let path = dc.get_path(&cache_key);
                let _ = std::fs::remove_file(&path);
            }

            // Remove from in-memory cache (keyed by full artwork URL)
            {
                let mut mem = self.artwork_cache.lock().await;
                // In-memory cache is keyed by full URL, but we need to evict by album_id pattern.
                // Pop all entries matching this album_id (we'll just evict all cached sizes for this ID to be safe)
                let keys_to_remove: Vec<String> = mem
                    .iter()
                    .filter(|(k, _)| k.contains(&format!("id={art_id}")))
                    .map(|(k, _)| k.clone())
                    .collect();
                for key in keys_to_remove {
                    mem.pop(&key);
                }
            }

            // Re-fetch from server
            let bytes = artwork_url::fetch_cover_art(
                &self
                    .albums_service
                    .get()
                    .ok_or_else(|| anyhow::anyhow!("Service not initialized"))?
                    .get_http_client(),
                &art_id,
                &server_url,
                &subsonic_credential,
                size,
            )
            .await;

            if let Some(data) = bytes {
                // Re-insert into disk cache
                if let Some(dc) = self.disk_cache.as_ref() {
                    dc.insert(&cache_key, &data);
                }
                debug!(
                    " [REFRESH] Re-fetched {:?}px artwork for {} ({} bytes)",
                    size,
                    album_id,
                    data.len()
                );
                fetched.push((size, data));
            } else {
                debug!(
                    " [REFRESH] No artwork returned for {} at {:?}px",
                    album_id, size
                );
            }
        }

        Ok(fetched)
    }
}
