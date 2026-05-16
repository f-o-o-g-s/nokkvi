//! Test helper utilities
//!
//! Factory functions for constructing test data without boilerplate.
//! Only compiled under `#[cfg(test)]`.

use nokkvi_data::{
    backend::{
        albums::AlbumUIViewData, artists::ArtistUIViewData, genres::GenreUIViewData,
        queue::QueueSongUIViewData, songs::SongUIViewData,
    },
    utils::search::build_searchable_lower,
};

use crate::Nokkvi;

/// Create a default `Nokkvi` for testing.
/// No network calls are made; `app_service` is `None`.
pub(crate) fn test_app() -> Nokkvi {
    Nokkvi::default()
}

/// Create a `QueueSongUIViewData` with the given fields, defaulting the rest.
pub(crate) fn make_queue_song(
    id: &str,
    title: &str,
    artist: &str,
    album: &str,
) -> QueueSongUIViewData {
    QueueSongUIViewData {
        id: id.to_string(),
        track_number: 1,
        title: title.to_string(),
        artist: artist.to_string(),
        artist_id: format!("artist_{id}"),
        album: album.to_string(),
        album_id: format!("album_{id}"),
        artwork_url: String::new(),
        duration: "3:00".to_string(),
        duration_seconds: 180,
        genre: "Rock".to_string(),
        starred: false,
        rating: None,
        play_count: None,
        searchable_lower: build_searchable_lower(&[title, artist, album, "Rock"]),
    }
}

/// Extended queue song with explicit duration and genre.
pub(crate) fn make_queue_song_full(
    id: &str,
    title: &str,
    artist: &str,
    album: &str,
    track_number: i32,
    duration_seconds: u32,
    genre: &str,
) -> QueueSongUIViewData {
    QueueSongUIViewData {
        id: id.to_string(),
        track_number,
        title: title.to_string(),
        artist: artist.to_string(),
        artist_id: format!("artist_{id}"),
        album: album.to_string(),
        album_id: format!("album_{id}"),
        artwork_url: String::new(),
        duration: format!("{}:{:02}", duration_seconds / 60, duration_seconds % 60),
        duration_seconds,
        genre: genre.to_string(),
        starred: false,
        rating: None,
        play_count: None,
        searchable_lower: build_searchable_lower(&[title, artist, album, genre]),
    }
}

/// Create a `SongUIViewData` with the given fields, defaulting the rest.
pub(crate) fn make_song(id: &str, title: &str, artist: &str) -> SongUIViewData {
    SongUIViewData {
        id: id.to_string(),
        title: title.to_string(),
        artist: artist.to_string(),
        artist_id: None,
        album: "Test Album".to_string(),
        album_id: Some(format!("album_{id}")),
        duration: 180,
        is_starred: false,
        track: None,
        year: None,
        genre: None,
        bpm: None,
        rating: None,
        channels: None,
        comment: None,
        play_count: None,
        created_at: None,
        play_date: None,
        album_artist: None,
        bitrate: None,
        size: 0,
        disc: None,
        suffix: None,
        sample_rate: None,
        compilation: None,
        bit_depth: None,
        updated_at: None,
        replay_gain: None,
        tags: None,
        path: format!("/music/{id}.flac"),
        participants: Vec::new(),
        searchable_lower: build_searchable_lower(&[title, artist, "Test Album"]),
    }
}

/// Create an `AlbumUIViewData` with the given fields, defaulting the rest.
pub(crate) fn make_album(id: &str, name: &str, artist: &str) -> AlbumUIViewData {
    AlbumUIViewData {
        id: id.to_string(),
        name: name.to_string(),
        artist: artist.to_string(),
        artist_id: format!("artist_{id}"),
        song_count: 10,
        artwork_url: String::new(),
        year: None,
        genre: None,
        genres: None,
        duration: None,
        is_starred: false,
        play_count: None,
        created_at: None,
        play_date: None,
        rating: None,
        compilation: None,
        size: None,
        updated_at: None,
        mbz_album_id: None,
        release_type: None,
        comment: None,
        tags: Vec::new(),
        participants: Vec::new(),
        release_date: None,
        original_date: None,
        original_year: None,
        searchable_lower: build_searchable_lower(&[name, artist]),
    }
}

/// Create an `ArtistUIViewData` with the given fields, defaulting the rest.
pub(crate) fn make_artist(id: &str, name: &str) -> ArtistUIViewData {
    ArtistUIViewData {
        id: id.to_string(),
        name: name.to_string(),
        album_count: 5,
        song_count: 50,
        is_starred: false,
        image_url: None,
        artwork_url: None,
        rating: None,
        play_count: None,
        play_date: None,
        size: None,
        mbz_artist_id: None,
        biography: None,
        external_url: None,
        searchable_lower: build_searchable_lower(&[name]),
    }
}

/// Create a `GenreUIViewData` with the given fields, defaulting the rest.
pub(crate) fn make_genre(id: &str, name: &str) -> nokkvi_data::backend::genres::GenreUIViewData {
    nokkvi_data::backend::genres::GenreUIViewData {
        id: id.to_string(),
        name: name.to_string(),
        album_count: 3,
        song_count: 30,
        artwork_url: None,
        artwork_album_ids: Vec::new(),
        searchable_lower: build_searchable_lower(&[name]),
    }
}

// ============================================================================
// Bulk fixtures for the album/artist/genre/song tri-mirror tests
// ============================================================================
//
// These pair with `for_each_expandable_entity!` in tests/navigation.rs and
// dedup the inline `(0..N).map(make_X)` / `pending_expand = Some(...)` /
// `library.X.set_from_vec(...)` sites that recur ~38× across navigation.rs
// and ~11× across library_refresh.rs. See `~/nokkvi-audit-results/dry-tests.md`
// §3 for the call-site survey.

/// `n` indexed albums with ids `a0..a{n-1}` and names `Album 0..Album {n-1}`.
pub(crate) fn albums_indexed(n: usize) -> Vec<AlbumUIViewData> {
    (0..n)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect()
}

/// Build a top-pane `PendingExpand::Album` with the given id.
pub(crate) fn pending_album(id: &str) -> crate::state::PendingExpand {
    crate::state::PendingExpand::Album {
        album_id: id.to_string(),
        for_browsing_pane: false,
    }
}

/// Build a top-pane `PendingExpand::Artist` with the given id.
pub(crate) fn pending_artist(id: &str) -> crate::state::PendingExpand {
    crate::state::PendingExpand::Artist {
        artist_id: id.to_string(),
        for_browsing_pane: false,
    }
}

/// Build a top-pane `PendingExpand::Genre` with the given id.
pub(crate) fn pending_genre(id: &str) -> crate::state::PendingExpand {
    crate::state::PendingExpand::Genre {
        genre_id: id.to_string(),
        for_browsing_pane: false,
    }
}

/// Build a top-pane `PendingExpand::Song` with the given id.
pub(crate) fn pending_song(id: &str) -> crate::state::PendingExpand {
    crate::state::PendingExpand::Song {
        song_id: id.to_string(),
        for_browsing_pane: false,
    }
}

/// Arm `pending_expand` for an Album target (top-pane, not browsing).
pub(crate) fn arm_pending_album(app: &mut Nokkvi, id: &str) {
    app.pending_expand = Some(pending_album(id));
}

/// Arm `pending_expand` for an Artist target (top-pane, not browsing).
pub(crate) fn arm_pending_artist(app: &mut Nokkvi, id: &str) {
    app.pending_expand = Some(pending_artist(id));
}

/// Arm `pending_expand` for a Genre target (top-pane, not browsing).
pub(crate) fn arm_pending_genre(app: &mut Nokkvi, id: &str) {
    app.pending_expand = Some(pending_genre(id));
}

/// Arm `pending_expand` for a Song target (top-pane, not browsing).
pub(crate) fn arm_pending_song(app: &mut Nokkvi, id: &str) {
    app.pending_expand = Some(pending_song(id));
}

/// `n` indexed artists with ids `ar0..ar{n-1}` and names `Artist 0..Artist {n-1}`.
pub(crate) fn artists_indexed(n: usize) -> Vec<ArtistUIViewData> {
    (0..n)
        .map(|i| make_artist(&format!("ar{i}"), &format!("Artist {i}")))
        .collect()
}

/// `n` indexed genres with ids `uuid-0..uuid-{n-1}` and names `Genre 0..Genre {n-1}`.
pub(crate) fn genres_indexed(n: usize) -> Vec<GenreUIViewData> {
    (0..n)
        .map(|i| make_genre(&format!("uuid-{i}"), &format!("Genre {i}")))
        .collect()
}

/// Replace the entire albums library buffer (sets `total_count = items.len()`).
pub(crate) fn seed_albums(app: &mut Nokkvi, items: Vec<AlbumUIViewData>) {
    app.library.albums.set_from_vec(items);
}

/// Replace the entire artists library buffer (sets `total_count = items.len()`).
pub(crate) fn seed_artists(app: &mut Nokkvi, items: Vec<ArtistUIViewData>) {
    app.library.artists.set_from_vec(items);
}

/// Replace the entire genres library buffer (sets `total_count = items.len()`).
pub(crate) fn seed_genres(app: &mut Nokkvi, items: Vec<GenreUIViewData>) {
    app.library.genres.set_from_vec(items);
}

/// Replace the entire songs library buffer (sets `total_count = items.len()`).
pub(crate) fn seed_songs(app: &mut Nokkvi, items: Vec<SongUIViewData>) {
    app.library.songs.set_from_vec(items);
}

/// `n` indexed songs with ids `s0..s{n-1}` and titles `Song 0..Song {n-1}`.
pub(crate) fn songs_indexed(n: usize) -> Vec<SongUIViewData> {
    (0..n)
        .map(|i| make_song(&format!("s{i}"), &format!("Song {i}"), "Artist"))
        .collect()
}

// ============================================================================
// Expansion setup helpers
// ============================================================================
//
// Collapse the 3-line inline expansion setup (expanded_id + parent_offset +
// children) that appeared across navigation.rs, queue.rs, and star_rating.rs.
// Each helper mirrors the struct's own field order; parent_offset is always 0
// for test setups (the render path doesn't run in tests).

/// Set up an Albums expansion with the given id and child tracks.
pub(crate) fn expand_albums_with(app: &mut Nokkvi, id: &str, children: Vec<SongUIViewData>) {
    app.albums_page.expansion.expanded_id = Some(id.into());
    app.albums_page.expansion.parent_offset = 0;
    app.albums_page.expansion.children = children;
}

/// Set up an Artists expansion with the given id and child albums.
pub(crate) fn expand_artists_with(app: &mut Nokkvi, id: &str, children: Vec<AlbumUIViewData>) {
    app.artists_page.expansion.expanded_id = Some(id.into());
    app.artists_page.expansion.parent_offset = 0;
    app.artists_page.expansion.children = children;
}

/// Set up a Genres expansion with the given id and child albums.
pub(crate) fn expand_genres_with(app: &mut Nokkvi, id: &str, children: Vec<AlbumUIViewData>) {
    app.genres_page.expansion.expanded_id = Some(id.into());
    app.genres_page.expansion.parent_offset = 0;
    app.genres_page.expansion.children = children;
}

/// Set up a Playlists expansion with the given id and child tracks.
pub(crate) fn expand_playlists_with(app: &mut Nokkvi, id: &str, children: Vec<SongUIViewData>) {
    app.playlists_page.expansion.expanded_id = Some(id.into());
    app.playlists_page.expansion.parent_offset = 0;
    app.playlists_page.expansion.children = children;
}

// ============================================================================
// Settings view data helper
// ============================================================================

/// Build a minimal SettingsViewData for testing.
/// Only the structure matters — values are defaults/dummies.
pub(crate) fn make_settings_view_data() -> crate::views::SettingsViewData {
    crate::views::SettingsViewData {
        general: nokkvi_data::types::settings_data::GeneralSettingsData::default(),
        interface: nokkvi_data::types::settings_data::InterfaceSettingsData::default(),
        playback: nokkvi_data::types::settings_data::PlaybackSettingsData::default(),
        visualizer_config: crate::visualizer_config::VisualizerConfig::default(),
        theme_file: nokkvi_data::types::theme_file::ThemeFile::default(),
        active_theme_stem: String::new(),
        window_height: 800.0,
        hotkey_config: nokkvi_data::types::hotkey_config::HotkeyConfig::default(),
        is_light_mode: false,
        rounded_mode: false,
        opacity_gradient: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_creates_default() {
        let app = test_app();
        assert!(!app.playback.playing);
        assert!(app.app_service.is_none());
        assert!(app.library.queue_songs.is_empty());
    }

    #[test]
    fn make_queue_song_sets_fields() {
        let song = make_queue_song("s1", "Title", "Artist", "Album");
        assert_eq!(song.id, "s1");
        assert_eq!(song.title, "Title");
        assert!(!song.starred);
    }

    #[test]
    fn make_song_sets_fields() {
        let song = make_song("s2", "My Song", "My Artist");
        assert_eq!(song.id, "s2");
        assert!(!song.is_starred);
    }

    #[test]
    fn make_album_sets_fields() {
        let album = make_album("a1", "My Album", "My Artist");
        assert_eq!(album.id, "a1");
        assert!(!album.is_starred);
    }

    #[test]
    fn make_artist_sets_fields() {
        let artist = make_artist("ar1", "My Artist");
        assert_eq!(artist.id, "ar1");
        assert!(!artist.is_starred);
    }
}
