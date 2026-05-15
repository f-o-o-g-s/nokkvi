//! Artists service — data loading and UI projection
//!
//! `ArtistsService` loads artists and their albums/songs via the Navidrome API,
//! projecting `Artist` models into `ArtistUIViewData` for the view layer.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::OnceCell;
use tracing::trace;

use crate::{
    backend::auth::AuthGateway,
    services::api::artists::ArtistsApiService,
    types::{album::Album, artist::Artist, reactive::ReactiveInt},
};

/// UI-specific view data for artists
/// UI-projected data
#[derive(Debug, Clone)]
pub struct ArtistUIViewData {
    pub id: String,
    pub name: String,
    pub album_count: u32,
    pub song_count: u32,
    pub is_starred: bool,
    pub image_url: Option<String>, // Large image URL (for detail view)
    pub artwork_url: Option<String>, // Mini artwork URL (for slot list slots)
    pub rating: Option<u32>,
    pub play_count: Option<u32>,
    pub play_date: Option<String>,
    pub size: Option<u64>,
    pub mbz_artist_id: Option<String>,
    pub biography: Option<String>,
    pub external_url: Option<String>,
    /// Pre-lowercased search index — see `crate::utils::search::Searchable`.
    pub searchable_lower: String,
}

impl crate::backend::Starable for ArtistUIViewData {
    fn entity_id(&self) -> &str {
        &self.id
    }
    fn set_starred(&mut self, starred: bool) {
        self.is_starred = starred;
    }
    fn display_label(&self) -> String {
        self.name.clone()
    }
}

impl crate::backend::Ratable for ArtistUIViewData {
    fn entity_id(&self) -> &str {
        &self.id
    }
    fn set_rating(&mut self, rating: Option<u32>) {
        self.rating = rating;
    }
    fn display_label(&self) -> String {
        self.name.clone()
    }
}

impl From<Artist> for ArtistUIViewData {
    fn from(a: Artist) -> Self {
        let image_url = a
            .large_image_url
            .clone()
            .or_else(|| a.medium_image_url.clone());
        let artwork_url = a
            .small_image_url
            .clone()
            .or_else(|| a.medium_image_url.clone());
        let album_count = a.get_album_count();
        let song_count = a.get_song_count();
        let is_starred = a.is_starred();
        let searchable_lower = crate::utils::search::build_searchable_lower(&[&a.name]);
        Self {
            id: a.id,
            name: a.name,
            album_count,
            song_count,
            is_starred,
            image_url,
            artwork_url,
            rating: a.rating,
            play_count: a.play_count,
            play_date: a.play_date,
            size: a.size,
            mbz_artist_id: a.mbz_artist_id,
            biography: a.biography,
            external_url: a.external_url,
            searchable_lower,
        }
    }
}

impl crate::utils::search::Searchable for ArtistUIViewData {
    fn matches_query(&self, query_lower: &str) -> bool {
        self.searchable_lower.contains(query_lower)
    }
}

#[derive(Clone)]
pub struct ArtistsService {
    // API service (lazily initialized on first use after login)
    artists_service: Arc<OnceCell<ArtistsApiService>>,

    // Reactive properties
    pub total_count: ReactiveInt,

    // Dependencies
    auth_gateway: Arc<OnceCell<AuthGateway>>,
}

impl Default for ArtistsService {
    fn default() -> Self {
        Self::new()
    }
}

impl ArtistsService {
    pub fn new() -> Self {
        Self {
            artists_service: Arc::new(OnceCell::new()),
            total_count: ReactiveInt::new(0),
            auth_gateway: Arc::new(OnceCell::new()),
        }
    }

    /// Associate an authentication gateway.
    pub fn with_auth(self, auth: AuthGateway) -> Self {
        let _ = self.auth_gateway.set(auth);
        self
    }

    /// Get the initialized API service, lazily creating it on first call.
    async fn get_service(&self) -> Result<&ArtistsApiService> {
        self.artists_service
            .get_or_try_init(|| async {
                let auth = self.auth_gateway.get().ok_or_else(|| {
                    anyhow::anyhow!("ArtistsService not initialized. Please authenticate first.")
                })?;
                let client = auth.get_client().await.ok_or_else(|| {
                    anyhow::anyhow!("ArtistsService not initialized. Please authenticate first.")
                })?;
                Ok(ArtistsApiService::new(client))
            })
            .await
    }

    /// Load artists and return raw Artist structs (first page only).
    /// Uses PAGE_SIZE as the default limit for pagination.
    pub async fn load_raw_artists(
        &self,
        sort_mode: Option<&str>,
        sort_order: Option<&str>,
        search_query: Option<&str>,
        filter: Option<&crate::types::filter::LibraryFilter>,
        album_artists_only: bool,
    ) -> Result<Vec<Artist>> {
        use crate::types::paged_buffer::PAGE_SIZE;
        self.load_raw_artists_page(
            sort_mode,
            sort_order,
            search_query,
            filter,
            album_artists_only,
            0,
            PAGE_SIZE,
        )
        .await
    }

    /// Load a specific page of artists with explicit offset/limit.
    #[allow(clippy::too_many_arguments)]
    pub async fn load_raw_artists_page(
        &self,
        sort_mode: Option<&str>,
        sort_order: Option<&str>,
        search_query: Option<&str>,
        filter: Option<&crate::types::filter::LibraryFilter>,
        album_artists_only: bool,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Artist>> {
        let service = self.get_service().await?;

        let sort_mode = sort_mode.unwrap_or("random");
        let sort_order = sort_order.unwrap_or("ASC");
        let search_opt = search_query.filter(|s| !s.is_empty());

        match service
            .load_artists(
                sort_mode,
                sort_order,
                search_opt,
                filter,
                album_artists_only,
                Some(offset),
                Some(limit),
            )
            .await
        {
            Ok((artists, total_count)) => {
                self.total_count.set(total_count as i32);
                trace!(
                    " ArtistsService.load_raw_artists_page: offset={}, limit={}, got={}, total={}",
                    offset,
                    limit,
                    artists.len(),
                    total_count
                );
                Ok(artists)
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
            auth.server_config().await
        } else {
            (String::new(), String::new())
        }
    }

    /// Load albums for a specific artist
    pub async fn load_artist_albums(&self, artist_id: &str) -> Result<Vec<Album>> {
        let service = self.get_service().await?;
        service.load_artist_albums(artist_id).await
    }

    /// Load all songs for an artist (by loading all their albums first)
    pub async fn load_artist_songs(
        &self,
        artist_id: &str,
    ) -> Result<Vec<crate::types::song::Song>> {
        // First load the artist's albums
        let albums = self.load_artist_albums(artist_id).await?;

        // Get auth for songs service
        let auth = self
            .auth_gateway
            .get()
            .ok_or_else(|| anyhow::anyhow!("Not authenticated"))?;

        let client = auth
            .get_client()
            .await
            .ok_or_else(|| anyhow::anyhow!("No API client"))?;

        let songs_service = crate::services::api::songs::SongsApiService::new(client);

        // Load songs from all albums
        let mut all_songs = Vec::new();
        for album in albums {
            if let Ok(songs) = songs_service.load_album_songs(&album.id).await {
                all_songs.extend(songs);
            }
        }

        // Sort by album, disc, track
        all_songs.sort_by(|a, b| {
            let album_cmp = a.album.cmp(&b.album);
            if album_cmp != std::cmp::Ordering::Equal {
                return album_cmp;
            }
            let disc_cmp = a.disc.unwrap_or(1).cmp(&b.disc.unwrap_or(1));
            if disc_cmp != std::cmp::Ordering::Equal {
                return disc_cmp;
            }
            a.track.unwrap_or(0).cmp(&b.track.unwrap_or(0))
        });

        Ok(all_songs)
    }
}
