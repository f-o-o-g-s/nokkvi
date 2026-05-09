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
