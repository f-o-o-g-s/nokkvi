//! Source-of-songs descriptor for library → queue dispatch.
//!
//! Every queue verb (`play`, `enqueue`, `play_next`, `insert_at`) accepts a
//! `SongSource`, which `LibraryOrchestrator::resolve` turns into `Vec<Song>`.
//! Pre-resolved song lists (search results, batch-flattened multi-selections,
//! restored queue state) bypass resolution via the `Preloaded` variant.
//!
//! Audit anchor: `monoliths-data.md` §2 lines 374-378 — recommends enum
//! dispatch over trait + ZST. 5 actions × 1 dispatch = 5 methods. Caller
//! writes `app.play(SongSource::Album(id)).await?`.

use crate::types::{batch::BatchPayload, song::Song};

#[derive(Debug, Clone)]
pub enum SongSource {
    /// Resolve via `albums_service.load_album_songs(album_id)`.
    Album(String),
    /// Resolve via `artists_service.load_artist_songs(artist_id)`.
    Artist(String),
    /// Resolve via on-demand `SongsApiService::load_songs_by_genre(genre_name)`.
    /// Note: genre is keyed by NAME, not ID, per Navidrome API.
    Genre(String),
    /// Resolve via on-demand `PlaylistsApiService::load_playlist_songs(playlist_id)`.
    Playlist(String),
    /// Already-resolved songs — skip the load step entirely.
    Preloaded(Vec<Song>),
    /// Multi-selection or context-menu batch. Resolved via
    /// `LibraryOrchestrator::resolve_batch` — flattens + dedups across
    /// per-item dispatch.
    Batch(BatchPayload),
}

impl SongSource {
    /// Entity-named error message for the "resolved to zero songs" guard in
    /// `AppService::dispatch`. Toast-surfaced — these strings are
    /// user-facing.
    ///
    /// `Preloaded` keeps the engine's historical "No songs to play" text
    /// (matches `play_songs_from_index`); `Batch` matches
    /// `LibraryOrchestrator::resolve_batch`'s own empty error, which fires
    /// first anyway.
    pub fn empty_error_message(&self) -> &'static str {
        match self {
            SongSource::Album(_) => "No songs found in album",
            SongSource::Artist(_) => "No songs found for artist",
            SongSource::Genre(_) => "No songs found in genre",
            SongSource::Playlist(_) => "No songs found in playlist",
            SongSource::Preloaded(_) => "No songs to play",
            SongSource::Batch(_) => "No songs found in batch payload",
        }
    }

    /// Human-readable source label for structured dispatch logs.
    pub fn log_label(&self) -> String {
        match self {
            SongSource::Album(id) => format!("album {id}"),
            SongSource::Artist(id) => format!("artist {id}"),
            SongSource::Genre(name) => format!("genre '{name}'"),
            SongSource::Playlist(id) => format!("playlist {id}"),
            SongSource::Preloaded(songs) => format!("{} preloaded songs", songs.len()),
            SongSource::Batch(_) => "batch".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the per-variant empty-guard messages. The entity-named ones are
    /// toast-surfaced by every `play_*` / `add_*` / `insert_*` /
    /// `play_next_*` wrapper; `Preloaded` / `Batch` must keep matching their
    /// historical downstream guards.
    #[test]
    fn empty_error_message_is_entity_named() {
        assert_eq!(
            SongSource::Album("al".into()).empty_error_message(),
            "No songs found in album"
        );
        assert_eq!(
            SongSource::Artist("ar".into()).empty_error_message(),
            "No songs found for artist"
        );
        assert_eq!(
            SongSource::Genre("Jazz".into()).empty_error_message(),
            "No songs found in genre"
        );
        assert_eq!(
            SongSource::Playlist("pl".into()).empty_error_message(),
            "No songs found in playlist"
        );
        assert_eq!(
            SongSource::Preloaded(Vec::new()).empty_error_message(),
            "No songs to play"
        );
        assert_eq!(
            SongSource::Batch(BatchPayload::new()).empty_error_message(),
            "No songs found in batch payload"
        );
    }

    /// Pins the log-label shapes used by the consolidated dispatch debug log.
    #[test]
    fn log_label_names_the_source() {
        assert_eq!(SongSource::Album("al-1".into()).log_label(), "album al-1");
        assert_eq!(SongSource::Artist("ar-1".into()).log_label(), "artist ar-1");
        assert_eq!(SongSource::Genre("Jazz".into()).log_label(), "genre 'Jazz'");
        assert_eq!(
            SongSource::Playlist("pl-1".into()).log_label(),
            "playlist pl-1"
        );
        assert_eq!(
            SongSource::Preloaded(vec![Song::test_default("s1", "Song 1")]).log_label(),
            "1 preloaded songs"
        );
        assert_eq!(SongSource::Batch(BatchPayload::new()).log_label(), "batch");
    }
}
