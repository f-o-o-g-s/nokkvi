//! Artwork dispatch.
//!
//! Routes `Message::Artwork(...)` variants to their per-handler
//! implementations. Handler bodies live in `albums.rs` (general album
//! artwork), `collage.rs` (genre/playlist collage artwork), and `songs.rs`
//! (song mini artwork); this file only does the dispatch + the two API
//! fetchers used to resolve album IDs for collage targets.

use iced::Task;

use crate::{
    Nokkvi,
    app_message::{ArtworkMessage, CollageTarget, Message},
};

/// Fetch album IDs for a genre from the API.
/// Used as the `fetch_album_ids_fn` closure for genre collage artwork loading.
async fn load_genre_album_ids(
    client: nokkvi_data::services::api::client::ApiClient,
    server_url: String,
    subsonic_credential: String,
    entity_id: String,
) -> Vec<String> {
    let service = nokkvi_data::services::api::genres::GenresApiService::new(
        client,
        server_url,
        subsonic_credential,
    );
    service
        .load_genre_albums(&entity_id)
        .await
        .unwrap_or_default()
}

/// Fetch album IDs for a playlist from the API.
/// Used as the `fetch_album_ids_fn` closure for playlist collage artwork loading.
async fn load_playlist_album_ids(
    client: nokkvi_data::services::api::client::ApiClient,
    server_url: String,
    subsonic_credential: String,
    entity_id: String,
) -> Vec<String> {
    let service = nokkvi_data::services::api::playlists::PlaylistsApiService::new(
        client,
        server_url,
        subsonic_credential,
    );
    service
        .load_playlist_albums(&entity_id)
        .await
        .unwrap_or_default()
}

impl Nokkvi {
    /// Dispatch an `ArtworkMessage` to its handler.
    pub(super) fn dispatch_artwork(&mut self, msg: ArtworkMessage) -> Task<Message> {
        match msg {
            // Shared album artwork
            ArtworkMessage::Loaded(id, handle) => self.handle_artwork_loaded(id, handle),
            ArtworkMessage::LoadLarge(album_id) => self.handle_load_large_artwork(album_id),
            ArtworkMessage::LargeLoaded(id, handle) => self.handle_large_artwork_loaded(id, handle),
            ArtworkMessage::LargeArtistLoaded(id, handle, color) => {
                if let Some(h) = handle {
                    self.artwork.large_artwork.put(id.clone(), h);
                    self.artwork.refresh_large_artwork_snapshot();
                }
                if let Some(c) = color {
                    self.artwork.album_dominant_colors.put(id, c);
                    self.artwork.refresh_dominant_colors_snapshot();
                }
                self.artwork.loading_large_artwork = None;
                Task::none()
            }
            ArtworkMessage::DominantColorCalculated(id, color) => {
                self.artwork.album_dominant_colors.put(id, color);
                self.artwork.refresh_dominant_colors_snapshot();
                Task::none()
            }
            ArtworkMessage::RefreshAlbumArtwork(album_id) => {
                self.handle_refresh_album_artwork(album_id)
            }
            ArtworkMessage::RefreshAlbumArtworkSilent(album_id) => {
                self.handle_refresh_album_artwork_silent(album_id)
            }
            ArtworkMessage::RefreshComplete(album_id, thumb, large, silent) => {
                self.handle_refresh_complete(album_id, thumb, large, silent)
            }
            // Collage artwork pipeline (genre / playlist)
            ArtworkMessage::LoadCollage(target, id, server_url, cred, album_ids) => match target {
                CollageTarget::Genre => self.handle_load_collage_artwork(
                    target,
                    id,
                    server_url,
                    cred,
                    album_ids,
                    load_genre_album_ids,
                ),
                CollageTarget::Playlist => self.handle_load_collage_artwork(
                    target,
                    id,
                    server_url,
                    cred,
                    album_ids,
                    load_playlist_album_ids,
                ),
            },
            ArtworkMessage::LoadCollageMini(target, id, server_url, cred, album_ids) => {
                match target {
                    CollageTarget::Genre => self.handle_load_collage_mini_artwork(
                        target,
                        id,
                        server_url,
                        cred,
                        album_ids,
                        load_genre_album_ids,
                    ),
                    CollageTarget::Playlist => self.handle_load_collage_mini_artwork(
                        target,
                        id,
                        server_url,
                        cred,
                        album_ids,
                        load_playlist_album_ids,
                    ),
                }
            }
            ArtworkMessage::StartCollagePrefetch(target) => {
                // Collect items needing album IDs from the appropriate library
                let items_needing_ids: Vec<(String, String)> = match target {
                    CollageTarget::Genre => self
                        .library
                        .genres
                        .iter()
                        .filter(|g| g.artwork_album_ids.is_empty())
                        .map(|g| (g.id.clone(), g.name.clone()))
                        .collect(),
                    CollageTarget::Playlist => self
                        .library
                        .playlists
                        .iter()
                        .filter(|p| p.artwork_album_ids.is_empty())
                        .map(|p| (p.id.clone(), p.name.clone()))
                        .collect(),
                };
                match target {
                    CollageTarget::Genre => self.handle_start_collage_prefetch(
                        target,
                        items_needing_ids,
                        load_genre_album_ids,
                    ),
                    CollageTarget::Playlist => self.handle_start_collage_prefetch(
                        target,
                        items_needing_ids,
                        load_playlist_album_ids,
                    ),
                }
            }
            ArtworkMessage::CollageAlbumIdsLoaded(target, results) => {
                self.handle_collage_album_ids_loaded(target, results)
            }
            ArtworkMessage::CollageMiniLoaded(target, id, handle_opt) => {
                self.handle_collage_mini_loaded(target, id, handle_opt)
            }
            ArtworkMessage::CollageLoaded(target, id, handle_opt, collage_handles, album_ids) => {
                self.handle_collage_artwork_loaded(
                    target,
                    id,
                    handle_opt,
                    collage_handles,
                    album_ids,
                )
            }
            ArtworkMessage::CollageBatchReady(target, ids, server_url, cred) => {
                Task::batch(ids.into_iter().map(|id| {
                    Task::done(Message::Artwork(ArtworkMessage::LoadCollage(
                        target,
                        id,
                        server_url.clone(),
                        cred.clone(),
                        Vec::new(),
                    )))
                }))
            }
            // Song artwork
            ArtworkMessage::SongMiniLoaded(album_id, handle) => {
                self.handle_song_artwork_loaded(album_id, handle)
            }
            // Artwork-pane drag. The per-view chrome dispatcher already routes
            // the per-view `ArtworkColumnDrag` / `ArtworkColumnVerticalDrag`
            // variants synchronously; these arms exist so that any caller can
            // construct `Message::Artwork(ArtworkMessage::ColumnDrag(ev))`
            // directly and land in the same handler.
            ArtworkMessage::ColumnDrag(ev) => self.handle_artwork_column_drag(ev),
            ArtworkMessage::VerticalDrag(ev) => self.handle_artwork_vertical_drag(ev),
        }
    }
}
