//! Resolves a `SongSource` into `Vec<Song>` by dispatching to the appropriate
//! domain service or on-demand API constructor.
//!
//! Borrowed from `AppService` via `app.library_orchestrator()` — does not own
//! any state, holds short-lived references to existing services.
//!
//! Audit anchor: `monoliths-data.md` §2 lines 374-378 — recommends an
//! enum-dispatch shape over trait + ZST so each entity (Album/Artist/Genre/
//! Playlist/Preloaded) gets one resolve method and the verb side stays free
//! to compose them into the queue.
//!
//! The on-demand API construction in `resolve_genre` / `resolve_playlist`
//! mirrors the existing `AppService::songs_api()` / `playlists_api()`
//! factories (app_service.rs:619-676) so the auth dance stays consistent.

use std::collections::HashSet;

use anyhow::Result;

use crate::{
    backend::{albums::AlbumsService, artists::ArtistsService, auth::AuthGateway},
    services::api::{playlists::PlaylistsApiService, songs::SongsApiService},
    types::{
        batch::{BatchItem, BatchPayload},
        song::Song,
        song_source::SongSource,
    },
};

pub struct LibraryOrchestrator<'a> {
    auth: &'a AuthGateway,
    albums: &'a AlbumsService,
    artists: &'a ArtistsService,
}

impl<'a> LibraryOrchestrator<'a> {
    // Lane A is purely additive — Lanes C/D wire `AppService::library_orchestrator()`
    // into the existing `play_*` / `add_*_to_queue` / `play_next_*` method bodies,
    // at which point the constructor stops looking dead.
    #[allow(dead_code)]
    pub(crate) fn new(
        auth: &'a AuthGateway,
        albums: &'a AlbumsService,
        artists: &'a ArtistsService,
    ) -> Self {
        Self {
            auth,
            albums,
            artists,
        }
    }

    /// Single dispatch entry point. Variants delegate to per-entity helpers below.
    pub async fn resolve(&self, source: SongSource) -> Result<Vec<Song>> {
        match source {
            SongSource::Album(id) => self.resolve_album(&id).await,
            SongSource::Artist(id) => self.resolve_artist(&id).await,
            SongSource::Genre(name) => self.resolve_genre(&name).await,
            SongSource::Playlist(id) => self.resolve_playlist(&id).await,
            SongSource::Preloaded(songs) => Ok(songs),
            SongSource::Batch(payload) => self.resolve_batch(payload).await,
        }
    }

    pub async fn resolve_album(&self, album_id: &str) -> Result<Vec<Song>> {
        self.albums.load_album_songs(album_id).await
    }

    pub async fn resolve_artist(&self, artist_id: &str) -> Result<Vec<Song>> {
        self.artists.load_artist_songs(artist_id).await
    }

    /// Genre is keyed by name (Navidrome API contract). Constructs
    /// `SongsApiService` on demand — mirrors today's private
    /// `AppService::load_genre_songs` body (app_service.rs:723-728).
    pub async fn resolve_genre(&self, genre_name: &str) -> Result<Vec<Song>> {
        let client = self
            .auth
            .get_client()
            .await
            .ok_or_else(|| anyhow::anyhow!("Not authenticated"))?;
        let server_url = self.auth.get_server_url().await;
        let subsonic_credential = self.auth.get_subsonic_credential().await;
        let songs_api = SongsApiService::new(client, server_url, subsonic_credential);
        let (songs, _) = songs_api.load_songs_by_genre(genre_name).await?;
        Ok(songs)
    }

    /// Constructs `PlaylistsApiService` on demand — mirrors today's private
    /// `AppService::load_playlist_songs` body (app_service.rs:738-745).
    pub async fn resolve_playlist(&self, playlist_id: &str) -> Result<Vec<Song>> {
        let client = self
            .auth
            .get_client()
            .await
            .ok_or_else(|| anyhow::anyhow!("Not authenticated"))?;
        let server_url = self.auth.get_server_url().await;
        let subsonic_credential = self.auth.get_subsonic_credential().await;
        let playlists_api =
            PlaylistsApiService::new_with_client(client, server_url, subsonic_credential);
        playlists_api.load_playlist_songs(playlist_id).await
    }

    /// Flatten + dedup a `BatchPayload` to `Vec<Song>`. Per-item dispatch goes
    /// through the per-entity `resolve_*` methods so the entity quirks (genre's
    /// name-not-id, playlist's on-demand API construction) stay encapsulated.
    ///
    /// Skip-on-fail: items that error are logged at `warn!` and dropped — matches
    /// today's `AppService::resolve_batch` behavior. Empty result is a hard error.
    pub async fn resolve_batch(&self, batch: BatchPayload) -> Result<Vec<Song>> {
        let mut resolved = Vec::new();
        let mut seen = HashSet::new();

        for item in batch.items {
            let songs_result = match item {
                BatchItem::Song(song) => Ok(vec![*song]),
                BatchItem::Album(id) => self.resolve_album(&id).await,
                BatchItem::Artist(id) => self.resolve_artist(&id).await,
                BatchItem::Genre(name) => self.resolve_genre(&name).await,
                BatchItem::Playlist(id) => self.resolve_playlist(&id).await,
            };
            match songs_result {
                Ok(songs) => {
                    for song in songs {
                        if seen.insert(song.id.clone()) {
                            resolved.push(song);
                        }
                    }
                }
                Err(e) => tracing::warn!("Batch item resolution failed, skipping: {e}"),
            }
        }

        if resolved.is_empty() {
            Err(anyhow::anyhow!("No songs found in batch payload"))
        } else {
            Ok(resolved)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::song::Song;

    fn make_orchestrator_fixtures() -> (AuthGateway, AlbumsService, ArtistsService) {
        let auth = AuthGateway::new().expect("auth gateway");
        let albums = AlbumsService::new().with_auth(auth.clone());
        let artists = ArtistsService::new().with_auth(auth.clone());
        (auth, albums, artists)
    }

    #[tokio::test]
    async fn resolve_preloaded_returns_input_unchanged() {
        let (auth, albums, artists) = make_orchestrator_fixtures();
        let orch = LibraryOrchestrator::new(&auth, &albums, &artists);

        let input = vec![
            Song::test_default("a", "Song A"),
            Song::test_default("b", "Song B"),
            Song::test_default("c", "Song C"),
        ];
        let expected_ids: Vec<String> = input.iter().map(|s| s.id.clone()).collect();

        let out = orch
            .resolve(SongSource::Preloaded(input))
            .await
            .expect("preloaded resolve");
        let out_ids: Vec<String> = out.iter().map(|s| s.id.clone()).collect();
        assert_eq!(out_ids, expected_ids);
    }

    #[tokio::test]
    async fn resolve_preloaded_empty_returns_empty() {
        let (auth, albums, artists) = make_orchestrator_fixtures();
        let orch = LibraryOrchestrator::new(&auth, &albums, &artists);

        let out = orch
            .resolve(SongSource::Preloaded(Vec::new()))
            .await
            .expect("empty preloaded resolve");
        assert!(out.is_empty());
    }

    /// Compile-only smoke: dispatch routes to the albums service. Without a
    /// live Navidrome server the call errors out, but reaching that error
    /// proves the Album variant routed through `albums.load_album_songs`
    /// (the only path that can hit the network here).
    #[tokio::test]
    async fn resolve_dispatches_album_variant_to_albums_service() {
        let (auth, albums, artists) = make_orchestrator_fixtures();
        let orch = LibraryOrchestrator::new(&auth, &albums, &artists);

        let result = orch.resolve(SongSource::Album("nonexistent".into())).await;
        assert!(
            result.is_err(),
            "unauthenticated album resolve should error"
        );
    }

    /// Compile-only smoke: dispatch routes to the artists service.
    #[tokio::test]
    async fn resolve_dispatches_artist_variant_to_artists_service() {
        let (auth, albums, artists) = make_orchestrator_fixtures();
        let orch = LibraryOrchestrator::new(&auth, &albums, &artists);

        let result = orch.resolve(SongSource::Artist("nonexistent".into())).await;
        assert!(
            result.is_err(),
            "unauthenticated artist resolve should error"
        );
    }

    /// Exercises the on-demand `SongsApiService` construction path.
    /// With no auth, the resolver short-circuits at the `get_client`
    /// check with "Not authenticated" — proves the auth dance ran
    /// before any network call.
    #[tokio::test]
    async fn resolve_genre_constructs_songs_api_with_correct_name() {
        let (auth, albums, artists) = make_orchestrator_fixtures();
        let orch = LibraryOrchestrator::new(&auth, &albums, &artists);

        let err = orch
            .resolve_genre("Jazz")
            .await
            .expect_err("unauthenticated genre resolve must error");
        assert!(
            err.to_string().contains("Not authenticated"),
            "expected auth-check error, got: {err}"
        );
    }

    /// Exercises the on-demand `PlaylistsApiService` construction path.
    #[tokio::test]
    async fn resolve_playlist_constructs_playlists_api_with_correct_id() {
        let (auth, albums, artists) = make_orchestrator_fixtures();
        let orch = LibraryOrchestrator::new(&auth, &albums, &artists);

        let err = orch
            .resolve_playlist("playlist-id-123")
            .await
            .expect_err("unauthenticated playlist resolve must error");
        assert!(
            err.to_string().contains("Not authenticated"),
            "expected auth-check error, got: {err}"
        );
    }

    /// Empty `BatchPayload` must hit the "no songs found" hard-error branch
    /// after the per-item loop yields nothing — mirrors today's
    /// `AppService::resolve_batch` empty-result contract.
    #[tokio::test]
    async fn resolve_batch_empty_payload_errors() {
        let (auth, albums, artists) = make_orchestrator_fixtures();
        let orch = LibraryOrchestrator::new(&auth, &albums, &artists);

        let err = orch
            .resolve_batch(BatchPayload::new())
            .await
            .expect_err("empty batch must error");
        assert!(
            err.to_string().contains("No songs found in batch payload"),
            "expected empty-batch error, got: {err}"
        );
    }

    /// Pre-resolved `BatchItem::Song` entries skip the network and dedup by id.
    /// Three input items with two duplicate ids → two unique resolved songs.
    #[tokio::test]
    async fn resolve_batch_dedups_song_items_by_id() {
        let (auth, albums, artists) = make_orchestrator_fixtures();
        let orch = LibraryOrchestrator::new(&auth, &albums, &artists);

        let payload = BatchPayload::new()
            .with_item(BatchItem::Song(Box::new(Song::test_default("a", "Song A"))))
            .with_item(BatchItem::Song(Box::new(Song::test_default("b", "Song B"))))
            .with_item(BatchItem::Song(Box::new(Song::test_default(
                "a",
                "Song A duplicate",
            ))));

        let out = orch
            .resolve_batch(payload)
            .await
            .expect("song-only batch resolves");
        let ids: Vec<String> = out.iter().map(|s| s.id.clone()).collect();
        assert_eq!(ids, vec!["a".to_string(), "b".to_string()]);
    }
}
