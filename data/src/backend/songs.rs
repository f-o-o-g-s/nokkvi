//! Songs ViewModel
//!
//! Manages song data loading and transformation for the Songs view.

use std::{collections::HashMap, sync::Arc};

use anyhow::Result;
use tokio::sync::OnceCell;
use tracing::debug;

use crate::{
    backend::auth::AuthGateway,
    services::api::songs::SongsApiService,
    types::{reactive::ReactiveInt, song::Song},
};

/// UI-specific view data for songs
/// UI-projected data
#[derive(Debug, Clone)]
pub struct SongUIViewData {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: Option<String>,
    pub album: String,
    pub album_id: Option<String>,
    pub duration: u32,
    pub is_starred: bool,
    pub track: Option<u32>,
    pub year: Option<u32>,
    pub genre: Option<String>,
    pub bpm: Option<u32>,
    pub rating: Option<u32>,
    pub channels: Option<u32>,
    pub comment: Option<String>,
    pub play_count: Option<u32>,
    pub created_at: Option<String>,
    pub play_date: Option<String>,
    pub album_artist: Option<String>,
    pub bitrate: Option<u32>,
    pub size: u64,
    pub disc: Option<u32>,
    pub suffix: Option<String>,
    pub sample_rate: Option<u32>,
    /// File path - needed to extract format suffix (e.g., "flac", "mp3")
    pub path: String,
    pub compilation: Option<bool>,
    pub bit_depth: Option<u32>,
    pub updated_at: Option<String>,
    pub replay_gain: Option<crate::types::song::ReplayGain>,
    pub tags: Option<HashMap<String, Vec<String>>>,
    pub participants: Vec<(String, String)>,
    /// Pre-lowercased search index — see `crate::utils::search::Searchable`.
    pub searchable_lower: String,
}

impl crate::backend::Starable for SongUIViewData {
    fn entity_id(&self) -> &str {
        &self.id
    }
    fn set_starred(&mut self, starred: bool) {
        self.is_starred = starred;
    }
    fn display_label(&self) -> String {
        format!("{} - {}", self.title, self.artist)
    }
}

impl crate::backend::Ratable for SongUIViewData {
    fn entity_id(&self) -> &str {
        &self.id
    }
    fn set_rating(&mut self, rating: Option<u32>) {
        self.rating = rating;
    }
    fn display_label(&self) -> String {
        format!("{} - {}", self.title, self.artist)
    }
}

impl crate::backend::PlayCountable for SongUIViewData {
    fn entity_id(&self) -> &str {
        &self.id
    }
    fn play_count(&self) -> Option<u32> {
        self.play_count
    }
    fn set_play_count(&mut self, count: Option<u32>) {
        self.play_count = count;
    }
    fn display_label(&self) -> String {
        format!("{} - {}", self.title, self.artist)
    }
}

impl crate::utils::search::Searchable for SongUIViewData {
    fn matches_query(&self, query_lower: &str) -> bool {
        self.searchable_lower.contains(query_lower)
    }
}

impl From<Song> for SongUIViewData {
    fn from(song: Song) -> Self {
        let participants = crate::backend::flatten_participants(song.participants.as_ref());
        let searchable_lower =
            crate::utils::search::build_searchable_lower(&[&song.title, &song.artist, &song.album]);
        Self {
            id: song.id,
            title: song.title,
            artist: song.artist,
            artist_id: song.artist_id,
            album: song.album,
            album_id: song.album_id,
            duration: song.duration,
            is_starred: song.starred,
            track: song.track,
            year: song.year,
            genre: song.genre,
            bpm: song.bpm,
            rating: song.rating,
            channels: song.channels,
            comment: song.comment,
            play_count: song.play_count,
            created_at: song.created_at,
            play_date: song.play_date,
            album_artist: song.album_artist,
            bitrate: song.bitrate,
            size: song.size,
            disc: song.disc,
            suffix: song.suffix,
            sample_rate: song.sample_rate,
            path: song.path,
            compilation: song.compilation,
            bit_depth: song.bit_depth,
            updated_at: song.updated_at,
            replay_gain: song.replay_gain,
            tags: song.tags,
            participants,
            searchable_lower,
        }
    }
}

impl From<SongUIViewData> for Song {
    fn from(ui: SongUIViewData) -> Self {
        Self {
            id: ui.id,
            title: ui.title,
            artist: ui.artist,
            artist_id: ui.artist_id,
            album: ui.album,
            album_id: ui.album_id.clone(),
            cover_art: ui.album_id, // Use album_id as cover_art fallback
            duration: ui.duration,
            track: ui.track,
            disc: ui.disc,
            year: ui.year,
            genre: ui.genre,
            path: ui.path,
            size: ui.size,
            bitrate: ui.bitrate,
            starred: ui.is_starred,
            play_count: ui.play_count,
            bpm: ui.bpm,
            channels: ui.channels,
            comment: ui.comment,
            rating: ui.rating,
            album_artist: ui.album_artist,
            suffix: ui.suffix,
            sample_rate: ui.sample_rate,
            created_at: ui.created_at,
            play_date: ui.play_date,
            compilation: ui.compilation,
            bit_depth: ui.bit_depth,
            updated_at: ui.updated_at,
            replay_gain: ui.replay_gain,
            tags: ui.tags,
            participants: None,      // Flattened form can't round-trip
            original_position: None, // Set by QueueManager::set_queue/add_songs
        }
    }
}

#[derive(Clone)]
pub struct SongsService {
    // Service layer (lazily initialized on first use after login)
    songs_service: Arc<OnceCell<SongsApiService>>,

    // Reactive properties
    pub total_count: ReactiveInt,

    // Dependencies
    auth_gateway: Arc<OnceCell<AuthGateway>>,
}

impl Default for SongsService {
    fn default() -> Self {
        Self::new()
    }
}

impl SongsService {
    pub fn new() -> Self {
        Self {
            songs_service: Arc::new(OnceCell::new()),
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
    async fn get_service(&self) -> Result<&SongsApiService> {
        self.songs_service
            .get_or_try_init(|| async {
                let auth = self.auth_gateway.get().ok_or_else(|| {
                    anyhow::anyhow!("SongsService not initialized. Please authenticate first.")
                })?;
                let client = auth.get_client().await.ok_or_else(|| {
                    anyhow::anyhow!("SongsService not initialized. Please authenticate first.")
                })?;
                Ok(SongsApiService::new(client))
            })
            .await
    }

    /// Load songs and return raw Song structs (first page only).
    /// Uses PAGE_SIZE as the default limit for pagination.
    pub async fn load_raw_songs(
        &self,
        sort_mode: Option<&str>,
        sort_order: Option<&str>,
        search_query: Option<&str>,
        filter: Option<&crate::types::filter::LibraryFilter>,
    ) -> Result<Vec<Song>> {
        use crate::types::paged_buffer::PAGE_SIZE;
        self.load_raw_songs_page(sort_mode, sort_order, search_query, filter, 0, PAGE_SIZE)
            .await
    }

    /// Load a specific page of songs with explicit offset/limit.
    /// Returns (songs, total_count) for use with PagedBuffer.
    pub async fn load_raw_songs_page(
        &self,
        sort_mode: Option<&str>,
        sort_order: Option<&str>,
        search_query: Option<&str>,
        filter: Option<&crate::types::filter::LibraryFilter>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Song>> {
        let service = self.get_service().await?;

        let sort_mode = sort_mode.unwrap_or("recentlyAdded");
        let sort_order = sort_order.unwrap_or("DESC");
        let search_opt = search_query.filter(|s| !s.is_empty());

        match service
            .load_songs(
                sort_mode,
                sort_order,
                search_opt,
                filter,
                Some(offset),
                Some(limit),
            )
            .await
        {
            Ok((songs, total_count)) => {
                self.total_count.set(total_count as i32);
                debug!(
                    " SongsService.load_raw_songs_page: offset={}, limit={}, got={}, total={}",
                    offset,
                    limit,
                    songs.len(),
                    total_count
                );
                Ok(songs)
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

    /// Get extra column data based on current sort mode. Rating, MostPlayed,
    /// and Duration are no longer rendered here — Stars/Plays are dedicated
    /// columns and Duration has its own toggleable column.
    pub fn get_extra_column_data(song: &SongUIViewData, sort_mode: &str) -> String {
        match sort_mode {
            "recentlyAdded" => song
                .created_at
                .as_ref()
                .and_then(|d| d.split('T').next())
                .map(|s| s.to_string())
                .unwrap_or_default(),
            "recentlyPlayed" => song.play_date.as_ref().map_or_else(
                || "never".to_string(),
                |d| d.split('T').next().unwrap_or(d).to_string(),
            ),
            "year" => song.year.map(|y| y.to_string()).unwrap_or_default(),
            "bpm" => song.bpm.map(|b| format!("{b} BPM")).unwrap_or_default(),
            "channels" => song
                .channels
                .map(|c| {
                    if c == 1 {
                        "Mono".to_string()
                    } else if c == 2 {
                        "Stereo".to_string()
                    } else {
                        format!("{c} ch")
                    }
                })
                .unwrap_or_default(),
            // Genre is rendered in the album column slot via the dedicated
            // `songs_show_genre` toggle, not in the dynamic slot.
            "comment" => song.comment.clone().unwrap_or_default(),
            "albumArtist" => song.album_artist.clone().unwrap_or_default(),
            // No extra column for these views: Rating/MostPlayed/Duration
            // have dedicated columns; the rest sort by data already visible.
            _ => String::new(),
        }
    }
}
