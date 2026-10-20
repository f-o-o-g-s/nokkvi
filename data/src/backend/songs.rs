//! Songs ViewModel
//!
//! Manages song data loading and transformation for the Songs view.

use std::collections::HashMap;

use anyhow::Result;
use tracing::trace;

use crate::{
    backend::{auth::AuthGateway, lazy_authed_service::LazyAuthedService},
    services::api::songs::SongsApiService,
    types::{
        library_query::LibraryQuery,
        reactive::ReactiveInt,
        song::{Song, SongCover},
    },
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
    /// The `mf-<id>` key of the art in the song's own file
    /// ([`SongCover::own_cover`]); `None` when it has none.
    pub own_cover: Option<String>,
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

impl SongCover for SongUIViewData {
    fn own_cover(&self) -> Option<&str> {
        self.own_cover.as_deref()
    }

    fn album_cover(&self) -> Option<&str> {
        self.album_id.as_deref()
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
        let own_cover = song.own_cover().map(str::to_string);
        Self {
            id: song.id,
            title: song.title,
            artist: song.artist,
            artist_id: song.artist_id,
            album: song.album,
            album_id: song.album_id,
            own_cover,
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
            // The song's own cover when it has one, else the album id.
            cover_art: ui.own_cover.or(ui.album_id),
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
    /// Lazily-initialized API service paired with its shared `AuthGateway`.
    /// Built on first `get_service()` call via `SongsApiService::new`.
    inner: LazyAuthedService<SongsApiService>,

    // Reactive properties
    pub total_count: ReactiveInt,
}

impl Default for SongsService {
    fn default() -> Self {
        Self::new()
    }
}

impl SongsService {
    pub fn new() -> Self {
        Self {
            inner: LazyAuthedService::new(SongsApiService::new),
            total_count: ReactiveInt::new(0),
        }
    }

    /// Associate an authentication gateway.
    pub fn with_auth(mut self, auth: AuthGateway) -> Self {
        self.inner = self.inner.with_auth(auth);
        self
    }

    /// Get the initialized API service, lazily creating it on first call.
    async fn get_service(&self) -> Result<&SongsApiService> {
        self.inner.get().await
    }

    /// Load a specific page of songs with explicit offset/limit, scoped to
    /// the given `library_ids` via the orthogonal Native API filter. An
    /// empty slice omits the param entirely (Navidrome auto-scopes to
    /// libraries the user can access).
    ///
    /// Returns `Vec<Song>`; the total count is published to
    /// [`Self::total_count`] for `PagedBuffer` consumers.
    pub async fn load_raw_songs_page_with_libraries(
        &self,
        query: &LibraryQuery<'_>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Song>> {
        let service = self.get_service().await?;

        match service.load_songs(query, Some(offset), Some(limit)).await {
            Ok((songs, total_count)) => {
                self.total_count.set(total_count as i32);
                trace!(
                    " SongsService.load_raw_songs_page_with_libraries: offset={}, limit={}, got={}, total={}",
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
        self.inner.server_config().await
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::player_settings::TrackArtwork;

    fn song(cover_art: Option<&str>) -> Song {
        Song {
            cover_art: cover_art.map(str::to_string),
            album_id: Some("a1".to_string()),
            ..Song::test_default("s1", "Song")
        }
    }

    /// The Songs view row keeps the song's own cover, and queueing the row
    /// (back into a `Song`) keeps it too, so the queue shows the same art.
    #[test]
    fn own_cover_survives_the_view_row_round_trip() {
        let row = SongUIViewData::from(song(Some("mf-s1_1a2b")));
        assert_eq!(row.own_cover.as_deref(), Some("mf-s1"));
        assert_eq!(row.cover_key(TrackArtwork::OwnCover), Some("mf-s1"));
        assert_eq!(row.cover_key(TrackArtwork::AlbumCover), Some("a1"));

        let back = Song::from(row);
        assert_eq!(back.own_cover(), Some("mf-s1"));
        assert_eq!(back.album_id.as_deref(), Some("a1"));
    }

    #[test]
    fn a_row_without_its_own_cover_queues_with_the_album_cover() {
        let row = SongUIViewData::from(song(Some("al-a1_ff")));
        assert_eq!(row.own_cover, None);
        assert_eq!(row.cover_key(TrackArtwork::OwnCover), Some("a1"));

        let back = Song::from(row);
        assert_eq!(back.own_cover(), None);
        assert_eq!(back.cover_art.as_deref(), Some("a1"));
    }
}
