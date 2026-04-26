//! Test helper utilities
//!
//! Factory functions for constructing test data without boilerplate.
//! Only compiled under `#[cfg(test)]`.

use nokkvi_data::backend::{
    albums::AlbumUIViewData, artists::ArtistUIViewData, queue::QueueSongUIViewData,
    songs::SongUIViewData,
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
    }
}

/// Create a `RadioStation` with the given fields, defaulting the rest.
#[allow(dead_code)]
pub(crate) fn make_radio_station(
    id: &str,
    name: &str,
    stream_url: &str,
) -> nokkvi_data::types::radio_station::RadioStation {
    nokkvi_data::types::radio_station::RadioStation {
        id: id.to_string(),
        name: name.to_string(),
        stream_url: stream_url.to_string(),
        home_page_url: None,
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
