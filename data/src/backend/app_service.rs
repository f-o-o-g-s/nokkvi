//! AppService — top-level orchestrator for backend services
//!
//! Composes domain services (Albums, Artists, Songs, Queue, Settings, Auth) with
//! `PlaybackController`. Provides intent-based methods (play album, add to queue)
//! that encapsulate multi-step workflows.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::{Mutex, mpsc::UnboundedReceiver};
use tracing::debug;

use crate::{
    audio::engine::CustomAudioEngine,
    backend::{
        albums::AlbumsService, artists::ArtistsService, auth::AuthGateway,
        playback_controller::PlaybackController, queue::QueueService, settings::SettingsService,
        songs::SongsService,
    },
    services::task_manager::TaskManager,
};

/// AppService — Application-level orchestration and state management.
///
/// Coordinates between domain services and the playback controller.
/// Direct playback operations are delegated to [`PlaybackController`].
#[derive(Clone)]
pub struct AppService {
    auth_gateway: AuthGateway,
    queue_service: QueueService,
    settings_service: SettingsService,
    albums_service: AlbumsService,
    artists_service: ArtistsService,
    songs_service: SongsService,
    playback: PlaybackController,
    task_manager: Arc<TaskManager>,
    storage: crate::services::state_storage::StateStorage,
    /// Receiver for repeat-one loop song IDs from the audio engine.
    /// Wrapped in `Arc<Mutex<>>` so AppService can be cloned.
    /// The UI takes this once via `take_loop_receiver()` to build a subscription.
    loop_rx: Arc<Mutex<Option<UnboundedReceiver<String>>>>,
    /// Receiver for queue-changed events from the completion callback.
    /// Fires after each track auto-advance (post-consume, post-refresh).
    /// The UI takes this once via `take_queue_changed_receiver()` to build a subscription.
    queue_changed_rx: Arc<Mutex<Option<UnboundedReceiver<()>>>>,
}

impl std::fmt::Debug for AppService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppService").finish()
    }
}

impl AppService {
    pub async fn new() -> Result<Self> {
        // `get_app_db_path` runs the legacy → XDG-state-dir migration on
        // first call (gated by an internal OnceLock in `paths`), so the
        // open below always lands on the correct location.
        let db_path = crate::utils::paths::get_app_db_path()?;
        let storage = crate::services::state_storage::StateStorage::new(db_path)?;
        Self::new_with_storage(storage).await
    }

    /// Create an AppService re-using an existing StateStorage handle.
    ///
    /// This avoids the redb exclusive-lock conflict when re-logging in
    /// after logout, since background tasks may still hold Arc clones
    /// of the old AppService (and its inner StateStorage).
    pub async fn new_with_storage(
        storage: crate::services::state_storage::StateStorage,
    ) -> Result<Self> {
        let auth_gateway = AuthGateway::new()?;
        let queue_service = QueueService::new(auth_gateway.clone(), storage.clone())?;
        let settings_service = SettingsService::new(storage.clone())?;

        // Initialize the queue service with persisted data
        queue_service.initialize().await?;

        let task_manager = Arc::new(TaskManager::new());
        let albums_service = AlbumsService::new().with_auth(auth_gateway.clone());
        let artists_service = ArtistsService::new().with_auth(auth_gateway.clone());
        let songs_service = SongsService::new().with_auth(auth_gateway.clone());

        // Create the playback controller (owns audio engine + queue navigator)
        let (playback, loop_rx, queue_changed_rx) = PlaybackController::new(
            queue_service.clone(),
            settings_service.clone(),
            task_manager.clone(),
        )
        .await?;

        Ok(Self {
            auth_gateway,
            queue_service,
            settings_service,
            albums_service,
            artists_service,
            songs_service,
            playback,
            task_manager,
            storage,
            loop_rx: Arc::new(Mutex::new(Some(loop_rx))),
            queue_changed_rx: Arc::new(Mutex::new(Some(queue_changed_rx))),
        })
    }

    // =========================================================================
    // Service Accessors
    // =========================================================================

    /// Get reference to AuthGateway
    pub fn auth(&self) -> &AuthGateway {
        &self.auth_gateway
    }

    /// Get reference to QueueService
    pub fn queue(&self) -> &QueueService {
        &self.queue_service
    }

    /// Get reference to AlbumsService
    pub fn albums(&self) -> &AlbumsService {
        &self.albums_service
    }

    /// Get reference to ArtistsService
    pub fn artists(&self) -> &ArtistsService {
        &self.artists_service
    }

    /// Get reference to SongsService
    pub fn songs(&self) -> &SongsService {
        &self.songs_service
    }

    /// Get reference to SettingsService for preferences, player settings, and hotkeys
    pub fn settings(&self) -> &SettingsService {
        &self.settings_service
    }

    /// Get reference to PlaybackController
    pub fn playback(&self) -> &PlaybackController {
        &self.playback
    }

    /// Get audio engine (forwarded from PlaybackController)
    pub fn audio_engine(&self) -> Arc<Mutex<CustomAudioEngine>> {
        self.playback.audio_engine()
    }

    /// Get task manager for spawning tracked background tasks
    pub fn task_manager(&self) -> Arc<TaskManager> {
        self.task_manager.clone()
    }

    /// Get reference to the shared StateStorage (redb)
    pub fn storage(&self) -> &crate::services::state_storage::StateStorage {
        &self.storage
    }

    /// Take the repeat-one loop receiver (once, synchronously).
    ///
    /// Returns the `UnboundedReceiver<String>` that fires a looping song ID
    /// each time a track repeats in repeat-one mode. The UI layer calls this
    /// once at login time to build an iced subscription.
    ///
    /// Uses `try_lock()` — safe from synchronous code because the lock is never
    /// contended at login time (freshly constructed).
    ///
    /// Returns `None` if the receiver was already taken or the lock is contended.
    pub fn take_loop_receiver(&self) -> Option<UnboundedReceiver<String>> {
        self.loop_rx.try_lock().ok()?.take()
    }

    /// Take the queue-changed receiver (once, synchronously).
    ///
    /// Returns the `UnboundedReceiver<()>` that fires after each track
    /// auto-advance (post-consume, post-`refresh_from_queue`).
    /// The UI layer calls this once at login time to build an iced subscription.
    ///
    /// Returns `None` if the receiver was already taken or the lock is contended.
    pub fn take_queue_changed_receiver(&self) -> Option<UnboundedReceiver<()>> {
        self.queue_changed_rx.try_lock().ok()?.take()
    }

    /// Take the task status receiver (once, synchronously).
    ///
    /// Returns the `UnboundedReceiver<(TaskHandle, TaskStatus)>` for background task updates.
    /// The UI layer calls this once at login time to build an iced subscription.
    pub fn take_task_status_receiver(
        &self,
    ) -> Option<crate::services::task_manager::TaskStatusReceiver> {
        self.task_manager.take_status_receiver()
    }

    // =========================================================================
    // Playback Delegation
    //
    // These delegate to PlaybackController so call sites using
    // `shell.play()`, `shell.next()`, etc. continue to work unchanged.
    // =========================================================================

    pub async fn play_pause(&self) -> Result<()> {
        self.playback.play_pause().await
    }
    pub async fn play(&self) -> Result<()> {
        self.playback.play().await
    }
    pub async fn pause(&self) -> Result<()> {
        self.playback.pause().await
    }
    pub async fn stop(&self) -> Result<()> {
        self.playback.stop().await
    }
    pub async fn next(&self) -> Result<bool> {
        self.playback.next().await
    }
    pub async fn previous(&self) -> Result<()> {
        self.playback.previous().await
    }
    pub async fn seek(&self, position_seconds: f64) -> Result<()> {
        self.playback.seek(position_seconds).await
    }
    pub async fn set_volume(&self, volume: f32) -> Result<()> {
        self.playback.set_volume(volume).await
    }
    pub async fn toggle_random(&self) -> Result<bool> {
        self.playback.toggle_random().await
    }
    pub async fn cycle_repeat(&self) -> Result<(bool, bool)> {
        self.playback.cycle_repeat().await
    }
    pub async fn toggle_consume(&self) -> Result<bool> {
        self.playback.toggle_consume().await
    }
    pub async fn get_modes(&self) -> (bool, bool, bool, bool) {
        self.playback.get_modes().await
    }
    pub async fn prepare_next_for_gapless(&self) -> bool {
        self.playback.prepare_next_for_gapless().await
    }
    pub async fn play_song_from_queue(&self, song_id: &str, queue_index: usize) -> Result<()> {
        self.playback
            .play_song_from_queue(song_id, queue_index)
            .await
    }

    // =========================================================================
    // Intent-Based Orchestration Methods
    //
    // These methods encapsulate multi-step "play X" workflows.
    // Handlers should call these instead of defining workflows inline.
    // =========================================================================

    /// Play an album by ID.
    ///
    /// Loads all songs in the album, sets queue, and starts playback.
    pub async fn play_album(&self, album_id: &str) -> Result<()> {
        let songs = self.albums_service.load_album_songs(album_id).await?;
        self.playback.play_songs_from_index(songs, 0).await
    }

    /// Play an album starting from a specific track index.
    ///
    /// Loads all songs in the album, sets queue, and starts playback from `track_idx`.
    pub async fn play_album_from_track(&self, album_id: &str, track_idx: usize) -> Result<()> {
        let songs = self.albums_service.load_album_songs(album_id).await?;
        self.playback.play_songs_from_index(songs, track_idx).await
    }

    /// Play all songs by an artist.
    ///
    /// Loads all songs by this artist, sets queue, and starts playback.
    pub async fn play_artist(&self, artist_id: &str) -> Result<()> {
        let songs = self.artists_service.load_artist_songs(artist_id).await?;
        self.playback.play_songs_from_index(songs, 0).await
    }

    /// Play all songs in a genre.
    ///
    /// Loads all songs in this genre, sets queue, and starts playback.
    pub async fn play_genre(&self, genre_name: &str) -> Result<()> {
        let songs = self.load_genre_songs(genre_name).await?;
        self.playback.play_songs_from_index(songs, 0).await
    }

    /// Play all songs in a playlist.
    ///
    /// Loads all songs in this playlist, sets queue, and starts playback.
    pub async fn play_playlist(&self, playlist_id: &str) -> Result<()> {
        let songs = self.load_playlist_songs(playlist_id).await?;
        self.playback.play_songs_from_index(songs, 0).await
    }

    /// Play a playlist starting from a specific track index.
    ///
    /// Loads all songs in the playlist, sets queue, and starts playback from `track_idx`.
    pub async fn play_playlist_from_track(
        &self,
        playlist_id: &str,
        track_idx: usize,
    ) -> Result<()> {
        let songs = self.load_playlist_songs(playlist_id).await?;
        self.playback.play_songs_from_index(songs, track_idx).await
    }

    /// Load a playlist's songs into the queue WITHOUT starting playback.
    ///
    /// Used for playlist editing mode where we want to populate the queue
    /// for reordering/editing without auto-playing.
    pub async fn load_playlist_into_queue(&self, playlist_id: &str) -> Result<()> {
        let songs = self.load_playlist_songs(playlist_id).await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found in playlist"));
        }
        self.queue_service.set_queue(songs, Some(0)).await?;
        debug!(
            "📋 Loaded playlist {} into queue (no playback)",
            playlist_id
        );
        Ok(())
    }

    /// Play a pre-loaded list of songs, starting at a specific index.
    ///
    /// Use this when you already have the songs list (e.g., Songs view with current sort).
    pub async fn play_songs(
        &self,
        songs: Vec<crate::types::song::Song>,
        start_index: usize,
    ) -> Result<()> {
        self.playback
            .play_songs_from_index(songs, start_index)
            .await
    }

    // =========================================================================
    // Add-to-Queue Orchestration Methods
    //
    // These methods add songs to the existing queue WITHOUT clearing it or
    // starting playback. They mirror the "play X" methods above.
    // =========================================================================

    /// Add an album's songs to the queue (without starting playback).
    pub async fn add_album_to_queue(&self, album_id: &str) -> Result<()> {
        let songs = self.albums_service.load_album_songs(album_id).await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found in album"));
        }
        self.queue_service.add_songs(songs).await?;
        debug!("➕ Added album {} to queue", album_id);
        Ok(())
    }

    /// Add an artist's songs to the queue (without starting playback).
    pub async fn add_artist_to_queue(&self, artist_id: &str) -> Result<()> {
        let songs = self.artists_service.load_artist_songs(artist_id).await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found for artist"));
        }
        self.queue_service.add_songs(songs).await?;
        debug!("➕ Added artist {} to queue", artist_id);
        Ok(())
    }

    /// Add a single song to the queue (without starting playback).
    pub async fn add_song_to_queue(&self, song: crate::types::song::Song) -> Result<()> {
        self.queue_service.add_songs(vec![song]).await?;
        debug!("➕ Added song to queue");
        Ok(())
    }

    /// Add a single song to the queue and immediately start playing it.
    ///
    /// Used by `EnterBehavior::AppendAndPlay` — preserves existing queue.
    pub async fn add_song_and_play(&self, song: crate::types::song::Song) -> Result<()> {
        let song_id = song.id.clone();
        let queue_index = self.queue_service.get_songs().len();
        self.queue_service.add_songs(vec![song]).await?;
        self.playback
            .play_song_from_queue(&song_id, queue_index)
            .await?;
        debug!("➕▶ Added song to queue and started playing: {}", song_id);
        Ok(())
    }

    /// Add a single song to the queue by ID (loads from album first).
    pub async fn add_song_to_queue_by_id(&self, song_id: &str, album_id: &str) -> Result<()> {
        let songs = self.albums_service.load_album_songs(album_id).await?;
        if let Some(song) = songs.into_iter().find(|s| s.id == song_id) {
            self.queue_service.add_songs(vec![song]).await?;
            debug!("➕ Added song {} to queue", song_id);
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Song {song_id} not found in album {album_id}"
            ))
        }
    }

    /// Add all songs in a genre to the queue (without starting playback).
    pub async fn add_genre_to_queue(&self, genre_name: &str) -> Result<()> {
        let songs = self.load_genre_songs(genre_name).await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found in genre"));
        }
        self.queue_service.add_songs(songs).await?;
        debug!("➕ Added genre '{}' to queue", genre_name);
        Ok(())
    }

    /// Add all songs in a playlist to the queue (without starting playback).
    pub async fn add_playlist_to_queue(&self, playlist_id: &str) -> Result<()> {
        let songs = self.load_playlist_songs(playlist_id).await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found in playlist"));
        }
        self.queue_service.add_songs(songs).await?;
        debug!("➕ Added playlist {} to queue", playlist_id);
        Ok(())
    }

    // =========================================================================
    // Append-and-Play helpers  (EnterBehavior::AppendAndPlay)
    //
    // Each loads songs, appends to queue, then starts playing the first one.
    // =========================================================================

    /// Append an album's songs to the queue and start playing the first one.
    pub async fn add_album_and_play(&self, album_id: &str) -> Result<()> {
        let songs = self.albums_service.load_album_songs(album_id).await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found in album"));
        }
        let first_id = songs[0].id.clone();
        let queue_index = self.queue_service.get_songs().len();
        self.queue_service.add_songs(songs).await?;
        self.playback
            .play_song_from_queue(&first_id, queue_index)
            .await?;
        debug!("➕▶ Added album {} to queue and started playing", album_id);
        Ok(())
    }

    /// Append an artist's songs to the queue and start playing the first one.
    pub async fn add_artist_and_play(&self, artist_id: &str) -> Result<()> {
        let songs = self.artists_service.load_artist_songs(artist_id).await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found for artist"));
        }
        let first_id = songs[0].id.clone();
        let queue_index = self.queue_service.get_songs().len();
        self.queue_service.add_songs(songs).await?;
        self.playback
            .play_song_from_queue(&first_id, queue_index)
            .await?;
        debug!(
            "➕▶ Added artist {} to queue and started playing",
            artist_id
        );
        Ok(())
    }

    /// Append a genre's songs to the queue and start playing the first one.
    pub async fn add_genre_and_play(&self, genre_name: &str) -> Result<()> {
        let songs = self.load_genre_songs(genre_name).await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found in genre"));
        }
        let first_id = songs[0].id.clone();
        let queue_index = self.queue_service.get_songs().len();
        self.queue_service.add_songs(songs).await?;
        self.playback
            .play_song_from_queue(&first_id, queue_index)
            .await?;
        debug!(
            "➕▶ Added genre '{}' to queue and started playing",
            genre_name
        );
        Ok(())
    }

    /// Append a playlist's songs to the queue and start playing the first one.
    pub async fn add_playlist_and_play(&self, playlist_id: &str) -> Result<()> {
        let songs = self.load_playlist_songs(playlist_id).await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found in playlist"));
        }
        let first_id = songs[0].id.clone();
        let queue_index = self.queue_service.get_songs().len();
        self.queue_service.add_songs(songs).await?;
        self.playback
            .play_song_from_queue(&first_id, queue_index)
            .await?;
        debug!(
            "➕▶ Added playlist {} to queue and started playing",
            playlist_id
        );
        Ok(())
    }

    // =========================================================================
    // Insert-at-Position Orchestration Methods
    //
    // These mirror the add-to-queue methods above but insert at a specific
    // queue index (used by cross-pane drag-and-drop).
    // =========================================================================

    /// Insert an album's songs at a specific position in the queue.
    pub async fn insert_album_at_position(&self, album_id: &str, position: usize) -> Result<()> {
        let songs = self.albums_service.load_album_songs(album_id).await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found in album"));
        }
        self.queue_service.insert_songs_at(position, songs).await?;
        debug!(
            "📌 Inserted album {} at queue position {}",
            album_id, position
        );
        Ok(())
    }

    /// Insert an artist's songs at a specific position in the queue.
    pub async fn insert_artist_at_position(&self, artist_id: &str, position: usize) -> Result<()> {
        let songs = self.artists_service.load_artist_songs(artist_id).await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found for artist"));
        }
        self.queue_service.insert_songs_at(position, songs).await?;
        debug!(
            "📌 Inserted artist {} at queue position {}",
            artist_id, position
        );
        Ok(())
    }

    /// Insert a single song at a specific position in the queue.
    pub async fn insert_song_at_position(
        &self,
        song: crate::types::song::Song,
        position: usize,
    ) -> Result<()> {
        self.queue_service
            .insert_songs_at(position, vec![song])
            .await?;
        debug!("📌 Inserted song at queue position {}", position);
        Ok(())
    }

    /// Insert a single song (by ID, loaded from album) at a specific position in the queue.
    pub async fn insert_song_by_id_at_position(
        &self,
        song_id: &str,
        album_id: &str,
        position: usize,
    ) -> Result<()> {
        let songs = self.albums_service.load_album_songs(album_id).await?;
        if let Some(song) = songs.into_iter().find(|s| s.id == song_id) {
            self.queue_service
                .insert_songs_at(position, vec![song])
                .await?;
            debug!(
                "📌 Inserted song {} at queue position {}",
                song_id, position
            );
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Song {song_id} not found in album {album_id}"
            ))
        }
    }

    /// Insert all songs in a genre at a specific position in the queue.
    pub async fn insert_genre_at_position(&self, genre_name: &str, position: usize) -> Result<()> {
        let songs = self.load_genre_songs(genre_name).await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found in genre"));
        }
        self.queue_service.insert_songs_at(position, songs).await?;
        debug!(
            "📌 Inserted genre '{}' at queue position {}",
            genre_name, position
        );
        Ok(())
    }

    // =========================================================================
    // Private Song-Loading Helpers
    //
    // These encapsulate the auth + API-service construction for entity types
    // that don't have a dedicated backend service (genres, playlists).
    // =========================================================================

    /// Construct an authenticated `SongsApiService`.
    pub async fn songs_api(&self) -> Result<crate::services::api::songs::SongsApiService> {
        let client = self
            .auth_gateway
            .get_client()
            .await
            .ok_or_else(|| anyhow::anyhow!("Not authenticated"))?;
        let server_url = self.auth_gateway.get_server_url().await;
        let subsonic_credential = self.auth_gateway.get_subsonic_credential().await;
        Ok(crate::services::api::songs::SongsApiService::new(
            client,
            server_url,
            subsonic_credential,
        ))
    }

    /// Construct an authenticated `GenresApiService`.
    ///
    /// Callers inside `shell_task` closures can use `shell.genres_api().await?`
    /// instead of repeating the 4-line auth+construct block.
    pub async fn genres_api(&self) -> Result<crate::services::api::genres::GenresApiService> {
        let client = self
            .auth_gateway
            .get_client()
            .await
            .ok_or_else(|| anyhow::anyhow!("Not authenticated"))?;
        let server_url = self.auth_gateway.get_server_url().await;
        let subsonic_credential = self.auth_gateway.get_subsonic_credential().await;
        Ok(
            crate::services::api::genres::GenresApiService::new_with_client(
                client,
                server_url,
                subsonic_credential,
            ),
        )
    }

    /// Construct an authenticated `PlaylistsApiService`.
    ///
    /// Callers inside `shell_task` closures can use `shell.playlists_api().await?`
    /// instead of repeating the 4-line auth+construct block.
    pub async fn playlists_api(
        &self,
    ) -> Result<crate::services::api::playlists::PlaylistsApiService> {
        let client = self
            .auth_gateway
            .get_client()
            .await
            .ok_or_else(|| anyhow::anyhow!("Not authenticated"))?;
        let server_url = self.auth_gateway.get_server_url().await;
        let subsonic_credential = self.auth_gateway.get_subsonic_credential().await;
        Ok(
            crate::services::api::playlists::PlaylistsApiService::new_with_client(
                client,
                server_url,
                subsonic_credential,
            ),
        )
    }

    /// Construct an authenticated `RadiosApiService`.
    ///
    /// Callers inside `shell_task` closures can use `shell.radios_api().await?`
    /// to fetch internet radio stations from the Subsonic API.
    ///
    /// NOTE from Claude: Built ahead of Gemini's Phase 4 work. This follows
    /// the exact same auth+construct pattern as genres_api/playlists_api.
    pub async fn radios_api(&self) -> Result<crate::services::api::radios::RadiosApiService> {
        let client = self
            .auth_gateway
            .get_client()
            .await
            .ok_or_else(|| anyhow::anyhow!("Not authenticated"))?;
        let server_url = self.auth_gateway.get_server_url().await;
        let subsonic_credential = self.auth_gateway.get_subsonic_credential().await;
        Ok(
            crate::services::api::radios::RadiosApiService::new_with_client(
                client,
                server_url,
                subsonic_credential,
            ),
        )
    }

    /// Construct an authenticated `SimilarApiService`.
    ///
    /// Callers inside `shell_task` closures can use `shell.similar_api().await?`
    /// to fetch similar songs or top songs.
    pub async fn similar_api(&self) -> Result<crate::services::api::similar::SimilarApiService> {
        let client = self
            .auth_gateway
            .get_client()
            .await
            .ok_or_else(|| anyhow::anyhow!("Not authenticated"))?;
        let server_url = self.auth_gateway.get_server_url().await;
        let subsonic_credential = self.auth_gateway.get_subsonic_credential().await;
        Ok(
            crate::services::api::similar::SimilarApiService::new_with_client(
                client,
                server_url,
                subsonic_credential,
            ),
        )
    }

    /// Load all songs for a genre via the Genres API.
    async fn load_genre_songs(&self, genre_name: &str) -> Result<Vec<crate::types::song::Song>> {
        let songs_service = self.songs_api().await?;
        let (songs, _) = songs_service.load_songs_by_genre(genre_name).await?;
        Ok(songs)
    }

    /// Load all internet radio stations via the Radios API.
    pub async fn load_radio_stations(
        &self,
    ) -> Result<Vec<crate::types::radio_station::RadioStation>> {
        let radios_service = self.radios_api().await?;
        radios_service.load_radio_stations().await
    }

    /// Load all songs for a playlist via the Playlists API.
    async fn load_playlist_songs(
        &self,
        playlist_id: &str,
    ) -> Result<Vec<crate::types::song::Song>> {
        let playlists_service = self.playlists_api().await?;
        playlists_service.load_playlist_songs(playlist_id).await
    }

    // =========================================================================
    // Play-Next Orchestration Methods
    //
    // These methods add songs to the queue and then move them to right after
    // the currently playing track, so they play next.
    // =========================================================================

    /// Core helper: insert songs right after the currently playing track.
    ///
    /// Uses `insert_songs_at` for a single O(n) bulk splice instead of the
    /// previous add-then-move pattern which was O(n²) (n lock acquisitions × n
    /// Vec shifts).
    async fn play_next_songs(&self, songs: Vec<crate::types::song::Song>) -> Result<()> {
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs to add"));
        }
        let count = songs.len();

        // Target position: right after the currently playing index
        let current_idx = self.queue_service.current_index().await;
        let target = current_idx.map_or(0, |i| i + 1);

        // Single bulk insert at the target position
        self.queue_service.insert_songs_at(target, songs).await?;
        self.queue_service.refresh_from_queue().await?;
        debug!(
            "⏭ Inserted {} songs as play-next at position {}",
            count, target
        );
        Ok(())
    }

    /// Play next: add album songs right after currently playing track.
    pub async fn play_next_album(&self, album_id: &str) -> Result<()> {
        let songs = self.albums_service.load_album_songs(album_id).await?;
        self.play_next_songs(songs).await
    }

    /// Play next: add a single song (by ID) right after currently playing track.
    pub async fn play_next_song_by_id(&self, song_id: &str, album_id: &str) -> Result<()> {
        let songs = self.albums_service.load_album_songs(album_id).await?;
        if let Some(song) = songs.into_iter().find(|s| s.id == song_id) {
            self.play_next_songs(vec![song]).await
        } else {
            Err(anyhow::anyhow!(
                "Song {song_id} not found in album {album_id}"
            ))
        }
    }

    /// Play next: add artist songs right after currently playing track.
    pub async fn play_next_artist(&self, artist_id: &str) -> Result<()> {
        let songs = self.artists_service.load_artist_songs(artist_id).await?;
        self.play_next_songs(songs).await
    }

    /// Play next: add genre songs right after currently playing track.
    pub async fn play_next_genre(&self, genre_name: &str) -> Result<()> {
        let songs = self.load_genre_songs(genre_name).await?;
        self.play_next_songs(songs).await
    }

    /// Play next: add playlist songs right after currently playing track.
    pub async fn play_next_playlist(&self, playlist_id: &str) -> Result<()> {
        let songs = self.load_playlist_songs(playlist_id).await?;
        self.play_next_songs(songs).await
    }

    /// Play next: add pre-loaded songs right after currently playing track.
    pub async fn play_next_preloaded(&self, songs: Vec<crate::types::song::Song>) -> Result<()> {
        self.play_next_songs(songs).await
    }

    // =========================================================================
    // Batch Orchestration Methods
    //
    // These methods receive a `BatchPayload` and resolve it continuously, flattening
    // the ordered batch items into a de-duplicated `Vec<Song>`.
    // =========================================================================

    /// Resolve a `BatchPayload` into a flat, ordered, de-duplicated Vec of Songs.
    pub async fn resolve_batch(
        &self,
        batch: crate::types::batch::BatchPayload,
    ) -> Result<Vec<crate::types::song::Song>> {
        use std::collections::HashSet;

        use crate::types::batch::BatchItem;

        let mut resolved_songs = Vec::new();
        let mut seen_ids = HashSet::new();

        for item in batch.items {
            let songs_result = match item {
                BatchItem::Song(song) => Ok(vec![*song]),
                BatchItem::Album(album_id) => self.albums_service.load_album_songs(&album_id).await,
                BatchItem::Artist(artist_id) => {
                    self.artists_service.load_artist_songs(&artist_id).await
                }
                BatchItem::Genre(genre) => self.load_genre_songs(&genre).await,
                BatchItem::Playlist(playlist_id) => self.load_playlist_songs(&playlist_id).await,
            };

            match songs_result {
                Ok(songs) => {
                    for song in songs {
                        if seen_ids.insert(song.id.clone()) {
                            resolved_songs.push(song);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Batch item resolution failed, skipping: {e}");
                }
            }
        }

        if resolved_songs.is_empty() {
            Err(anyhow::anyhow!("No songs found in batch payload"))
        } else {
            Ok(resolved_songs)
        }
    }

    /// Play a batch. Replaces the current queue and starts playing.
    pub async fn play_batch(&self, batch: crate::types::batch::BatchPayload) -> Result<()> {
        let songs = self.resolve_batch(batch).await?;
        self.playback_songs(songs, 0).await
    }

    // helper wrapper to call internal logic bypassing self-clone if possible:
    async fn playback_songs(
        &self,
        songs: Vec<crate::types::song::Song>,
        start_index: usize,
    ) -> Result<()> {
        self.playback
            .play_songs_from_index(songs, start_index)
            .await
    }

    /// Add a batch to the queue (append).
    pub async fn add_batch_to_queue(&self, batch: crate::types::batch::BatchPayload) -> Result<()> {
        let songs = self.resolve_batch(batch).await?;
        self.queue_service.add_songs(songs).await?;
        debug!("➕ Added batch to queue");
        Ok(())
    }

    /// Add a batch right after the currently playing track.
    pub async fn play_next_batch(&self, batch: crate::types::batch::BatchPayload) -> Result<()> {
        let songs = self.resolve_batch(batch).await?;
        self.play_next_songs(songs).await
    }

    /// Insert a batch at a specific position in the queue.
    pub async fn insert_batch_at_position(
        &self,
        batch: crate::types::batch::BatchPayload,
        position: usize,
    ) -> Result<()> {
        let songs = self.resolve_batch(batch).await?;
        self.queue_service.insert_songs_at(position, songs).await?;
        debug!("📌 Inserted batch at queue position {}", position);
        Ok(())
    }

    /// Remove a batch of indices from the queue. Indices must be provided as a Vec.
    /// They will be sorted in descending order to prevent index shifting during removal.
    pub async fn remove_batch_from_queue(&self, mut indices: Vec<usize>) -> Result<()> {
        // Sort in descending order
        indices.sort_unstable_by(|a, b| b.cmp(a));

        let mut count = 0;
        for idx in indices {
            // Log individually but we're removing sequentially
            if self.queue_service.remove_song(idx).await.is_ok() {
                count += 1;
            }
        }

        if count > 0 {
            debug!("➖ Removed {} songs from queue via batch", count);
        }
        Ok(())
    }
}
