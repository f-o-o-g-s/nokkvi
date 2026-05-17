//! AppService — top-level orchestrator for backend services
//!
//! Composes domain services (Albums, Artists, Songs, Queue, Settings, Auth) with
//! `PlaybackController`. Provides intent-based methods (play album, add to queue)
//! that encapsulate multi-step workflows.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::{Mutex, mpsc::UnboundedReceiver};
use tracing::{debug, warn};

use crate::{
    audio::engine::CustomAudioEngine,
    backend::{
        albums::AlbumsService, artists::ArtistsService, auth::AuthGateway,
        playback_controller::PlaybackController, queue::QueueService, settings::SettingsService,
        songs::SongsService,
    },
    services::task_manager::TaskManager,
    types::queue_sort_mode::QueueSortMode,
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
        match self.loop_rx.try_lock() {
            Ok(mut guard) => {
                if guard.is_none() {
                    warn!("[APP SERVICE] take_loop_receiver called after receiver already taken");
                }
                guard.take()
            }
            Err(_) => None,
        }
    }

    /// Take the queue-changed receiver (once, synchronously).
    ///
    /// Returns the `UnboundedReceiver<()>` that fires after each track
    /// auto-advance (post-consume, post-`refresh_from_queue`).
    /// The UI layer calls this once at login time to build an iced subscription.
    ///
    /// Returns `None` if the receiver was already taken or the lock is contended.
    pub fn take_queue_changed_receiver(&self) -> Option<UnboundedReceiver<()>> {
        match self.queue_changed_rx.try_lock() {
            Ok(mut guard) => {
                if guard.is_none() {
                    warn!(
                        "[APP SERVICE] take_queue_changed_receiver called after receiver already taken"
                    );
                }
                guard.take()
            }
            Err(_) => None,
        }
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
        let songs = self.library_orchestrator().resolve_album(album_id).await?;
        self.queue_orchestrator().play(songs, 0).await
    }

    /// Play an album starting from a specific track index.
    ///
    /// Loads all songs in the album, sets queue, and starts playback from `track_idx`.
    pub async fn play_album_from_track(&self, album_id: &str, track_idx: usize) -> Result<()> {
        let songs = self.library_orchestrator().resolve_album(album_id).await?;
        let start = track_idx.min(songs.len().saturating_sub(1));
        self.queue_orchestrator().play(songs, start).await
    }

    /// Play all songs by an artist.
    ///
    /// Loads all songs by this artist, sets queue, and starts playback.
    pub async fn play_artist(&self, artist_id: &str) -> Result<()> {
        let songs = self
            .library_orchestrator()
            .resolve_artist(artist_id)
            .await?;
        self.queue_orchestrator().play(songs, 0).await
    }

    /// Play all songs in a genre.
    ///
    /// Loads all songs in this genre, sets queue, and starts playback.
    pub async fn play_genre(&self, genre_name: &str) -> Result<()> {
        let songs = self
            .library_orchestrator()
            .resolve_genre(genre_name)
            .await?;
        self.queue_orchestrator().play(songs, 0).await
    }

    /// Roulette variant of [`Self::play_genre`]: load all genre songs, then
    /// start playback from a random index. Used by the slot-machine
    /// roulette pick on the Genres view.
    pub async fn play_genre_random(&self, genre_name: &str) -> Result<()> {
        let songs = self
            .library_orchestrator()
            .resolve_genre(genre_name)
            .await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found in genre"));
        }
        let start = rand::random_range(0..songs.len());
        self.queue_orchestrator().play(songs, start).await
    }

    /// Roulette variant of [`Self::play_artist`]: load all artist songs,
    /// then start playback from a random index.
    pub async fn play_artist_random(&self, artist_id: &str) -> Result<()> {
        let songs = self
            .library_orchestrator()
            .resolve_artist(artist_id)
            .await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found for artist"));
        }
        let start = rand::random_range(0..songs.len());
        self.queue_orchestrator().play(songs, start).await
    }

    /// Play all songs in a playlist.
    ///
    /// Loads all songs in this playlist, sets queue, and starts playback.
    pub async fn play_playlist(&self, playlist_id: &str) -> Result<()> {
        let songs = self
            .library_orchestrator()
            .resolve_playlist(playlist_id)
            .await?;
        self.queue_orchestrator().play(songs, 0).await
    }

    /// Play a playlist starting from a specific track index.
    ///
    /// Loads all songs in the playlist, sets queue, and starts playback from `track_idx`.
    pub async fn play_playlist_from_track(
        &self,
        playlist_id: &str,
        track_idx: usize,
    ) -> Result<()> {
        let songs = self
            .library_orchestrator()
            .resolve_playlist(playlist_id)
            .await?;
        let start = track_idx.min(songs.len().saturating_sub(1));
        self.queue_orchestrator().play(songs, start).await
    }

    /// Load a playlist's songs into the queue WITHOUT starting playback.
    ///
    /// Used for playlist editing mode where we want to populate the queue
    /// for reordering/editing without auto-playing. An empty playlist is
    /// valid here (e.g. the create-new-playlist flow) — the queue is cleared
    /// so the user can populate it from the browsing panel.
    pub async fn load_playlist_into_queue(&self, playlist_id: &str) -> Result<()> {
        let songs = self
            .library_orchestrator()
            .resolve_playlist(playlist_id)
            .await?;
        let cursor = if songs.is_empty() { None } else { Some(0) };
        let effect = self.queue_service.set_queue(songs, cursor).await?;
        effect.apply_to(&self.audio_engine()).await;
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
        self.queue_orchestrator().play(songs, start_index).await
    }

    // =========================================================================
    // Add-to-Queue Orchestration Methods
    //
    // These methods add songs to the existing queue WITHOUT clearing it or
    // starting playback. They mirror the "play X" methods above.
    // =========================================================================

    /// Add an album's songs to the queue (without starting playback).
    pub async fn add_album_to_queue(&self, album_id: &str) -> Result<()> {
        let songs = self.library_orchestrator().resolve_album(album_id).await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found in album"));
        }
        self.queue_orchestrator().enqueue(songs).await?;
        debug!("➕ Added album {} to queue", album_id);
        Ok(())
    }

    /// Add an artist's songs to the queue (without starting playback).
    pub async fn add_artist_to_queue(&self, artist_id: &str) -> Result<()> {
        let songs = self
            .library_orchestrator()
            .resolve_artist(artist_id)
            .await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found for artist"));
        }
        self.queue_orchestrator().enqueue(songs).await?;
        debug!("➕ Added artist {} to queue", artist_id);
        Ok(())
    }

    /// Add a single song to the queue (without starting playback).
    pub async fn add_song_to_queue(&self, song: crate::types::song::Song) -> Result<()> {
        self.queue_orchestrator().enqueue(vec![song]).await?;
        debug!("➕ Added song to queue");
        Ok(())
    }

    /// Add a single song to the queue and immediately start playing it.
    ///
    /// Used by `EnterBehavior::AppendAndPlay` — preserves existing queue.
    pub async fn add_song_and_play(&self, song: crate::types::song::Song) -> Result<()> {
        let song_id = song.id.clone();
        self.queue_orchestrator()
            .enqueue_and_play(vec![song])
            .await?;
        debug!("➕▶ Added song to queue and started playing: {}", song_id);
        Ok(())
    }

    /// Add a single song to the queue by ID (loads from album first).
    pub async fn add_song_to_queue_by_id(&self, song_id: &str, album_id: &str) -> Result<()> {
        let songs = self.albums_service.load_album_songs(album_id).await?;
        if let Some(song) = songs.into_iter().find(|s| s.id == song_id) {
            let effect = self.queue_service.add_songs(vec![song]).await?;
            effect.apply_to(&self.audio_engine()).await;
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
        let songs = self
            .library_orchestrator()
            .resolve_genre(genre_name)
            .await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found in genre"));
        }
        self.queue_orchestrator().enqueue(songs).await?;
        debug!("➕ Added genre '{}' to queue", genre_name);
        Ok(())
    }

    /// Add all songs in a playlist to the queue (without starting playback).
    pub async fn add_playlist_to_queue(&self, playlist_id: &str) -> Result<()> {
        let songs = self
            .library_orchestrator()
            .resolve_playlist(playlist_id)
            .await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found in playlist"));
        }
        self.queue_orchestrator().enqueue(songs).await?;
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
        let songs = self.library_orchestrator().resolve_album(album_id).await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found in album"));
        }
        self.queue_orchestrator().enqueue_and_play(songs).await?;
        debug!("➕▶ Added album {} to queue and started playing", album_id);
        Ok(())
    }

    /// Append an artist's songs to the queue and start playing the first one.
    pub async fn add_artist_and_play(&self, artist_id: &str) -> Result<()> {
        let songs = self
            .library_orchestrator()
            .resolve_artist(artist_id)
            .await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found for artist"));
        }
        self.queue_orchestrator().enqueue_and_play(songs).await?;
        debug!(
            "➕▶ Added artist {} to queue and started playing",
            artist_id
        );
        Ok(())
    }

    /// Append a genre's songs to the queue and start playing the first one.
    pub async fn add_genre_and_play(&self, genre_name: &str) -> Result<()> {
        let songs = self
            .library_orchestrator()
            .resolve_genre(genre_name)
            .await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found in genre"));
        }
        self.queue_orchestrator().enqueue_and_play(songs).await?;
        debug!(
            "➕▶ Added genre '{}' to queue and started playing",
            genre_name
        );
        Ok(())
    }

    /// Append a playlist's songs to the queue and start playing the first one.
    pub async fn add_playlist_and_play(&self, playlist_id: &str) -> Result<()> {
        let songs = self
            .library_orchestrator()
            .resolve_playlist(playlist_id)
            .await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found in playlist"));
        }
        self.queue_orchestrator().enqueue_and_play(songs).await?;
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
        let songs = self.library_orchestrator().resolve_album(album_id).await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found in album"));
        }
        self.queue_orchestrator().insert_at(songs, position).await?;
        debug!(
            "📌 Inserted album {} at queue position {}",
            album_id, position
        );
        Ok(())
    }

    /// Insert an artist's songs at a specific position in the queue.
    pub async fn insert_artist_at_position(&self, artist_id: &str, position: usize) -> Result<()> {
        let songs = self
            .library_orchestrator()
            .resolve_artist(artist_id)
            .await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found for artist"));
        }
        self.queue_orchestrator().insert_at(songs, position).await?;
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
        self.queue_orchestrator()
            .insert_at(vec![song], position)
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
            let effect = self
                .queue_service
                .insert_songs_at(position, vec![song])
                .await?;
            effect.apply_to(&self.audio_engine()).await;
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
        let songs = self
            .library_orchestrator()
            .resolve_genre(genre_name)
            .await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs found in genre"));
        }
        self.queue_orchestrator().insert_at(songs, position).await?;
        debug!(
            "📌 Inserted genre '{}' at queue position {}",
            genre_name, position
        );
        Ok(())
    }
}

macro_rules! native_api_factory {
    ( $( ($method:ident, $ty:path) ),+ $(,)? ) => {
        $(
            /// Construct an authenticated Native-API service. Use `.await?` in `shell_task` closures.
            pub async fn $method(&self) -> anyhow::Result<$ty> {
                let client = self
                    .auth_gateway
                    .get_client()
                    .await
                    .ok_or_else(|| anyhow::anyhow!("Not authenticated"))?;
                Ok(<$ty>::new(client))
            }
        )+
    };
}

macro_rules! subsonic_api_factory {
    ( $( ($method:ident, $ty:path) ),+ $(,)? ) => {
        $(
            /// Construct an authenticated Subsonic-API service. Use `.await?` in `shell_task` closures.
            pub async fn $method(&self) -> anyhow::Result<$ty> {
                let client = self
                    .auth_gateway
                    .get_client()
                    .await
                    .ok_or_else(|| anyhow::anyhow!("Not authenticated"))?;
                let (server_url, subsonic_credential) = self.auth_gateway.server_config().await;
                Ok(<$ty>::new(client, server_url, subsonic_credential))
            }
        )+
    };
}

// === API factory methods ===
impl AppService {
    native_api_factory!((songs_api, crate::services::api::songs::SongsApiService),);

    subsonic_api_factory!(
        (genres_api, crate::services::api::genres::GenresApiService),
        (
            playlists_api,
            crate::services::api::playlists::PlaylistsApiService
        ),
        (radios_api, crate::services::api::radios::RadiosApiService),
        (
            similar_api,
            crate::services::api::similar::SimilarApiService
        ),
    );
}

// === Queue orchestrator accessor ===
impl AppService {
    /// Borrow-handle for queue-mutation verbs. Holds no state.
    pub(crate) fn queue_orchestrator(&self) -> crate::backend::QueueOrchestrator<'_> {
        crate::backend::QueueOrchestrator::new(&self.queue_service, &self.playback)
    }
}

impl AppService {
    /// Load all internet radio stations via the Radios API.
    pub async fn load_radio_stations(
        &self,
    ) -> Result<Vec<crate::types::radio_station::RadioStation>> {
        let radios_service = self.radios_api().await?;
        radios_service.load_radio_stations().await
    }

    // =========================================================================
    // Play-Next Orchestration Methods
    //
    // These methods add songs to the queue and then move them to right after
    // the currently playing track, so they play next.
    // =========================================================================

    /// Play next: add album songs right after currently playing track.
    pub async fn play_next_album(&self, album_id: &str) -> Result<()> {
        let songs = self.library_orchestrator().resolve_album(album_id).await?;
        self.queue_orchestrator().play_next(songs).await
    }

    /// Play next: add a single song (by ID) right after currently playing track.
    pub async fn play_next_song_by_id(&self, song_id: &str, album_id: &str) -> Result<()> {
        let songs = self.albums_service.load_album_songs(album_id).await?;
        if let Some(song) = songs.into_iter().find(|s| s.id == song_id) {
            self.queue_orchestrator().play_next(vec![song]).await
        } else {
            Err(anyhow::anyhow!(
                "Song {song_id} not found in album {album_id}"
            ))
        }
    }

    /// Play next: add artist songs right after currently playing track.
    pub async fn play_next_artist(&self, artist_id: &str) -> Result<()> {
        let songs = self
            .library_orchestrator()
            .resolve_artist(artist_id)
            .await?;
        self.queue_orchestrator().play_next(songs).await
    }

    /// Play next: add genre songs right after currently playing track.
    pub async fn play_next_genre(&self, genre_name: &str) -> Result<()> {
        let songs = self
            .library_orchestrator()
            .resolve_genre(genre_name)
            .await?;
        self.queue_orchestrator().play_next(songs).await
    }

    /// Play next: add playlist songs right after currently playing track.
    pub async fn play_next_playlist(&self, playlist_id: &str) -> Result<()> {
        let songs = self
            .library_orchestrator()
            .resolve_playlist(playlist_id)
            .await?;
        self.queue_orchestrator().play_next(songs).await
    }

    /// Play next: add pre-loaded songs right after currently playing track.
    pub async fn play_next_preloaded(&self, songs: Vec<crate::types::song::Song>) -> Result<()> {
        self.queue_orchestrator().play_next(songs).await
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
        self.library_orchestrator().resolve_batch(batch).await
    }

    /// Play a batch. Replaces the current queue and starts playing.
    pub async fn play_batch(&self, batch: crate::types::batch::BatchPayload) -> Result<()> {
        let songs = self.library_orchestrator().resolve_batch(batch).await?;
        self.queue_orchestrator().play(songs, 0).await
    }

    /// Add a batch to the queue (append).
    pub async fn add_batch_to_queue(&self, batch: crate::types::batch::BatchPayload) -> Result<()> {
        let songs = self.library_orchestrator().resolve_batch(batch).await?;
        self.queue_orchestrator().enqueue(songs).await?;
        debug!("➕ Added batch to queue");
        Ok(())
    }

    /// Add a batch right after the currently playing track.
    pub async fn play_next_batch(&self, batch: crate::types::batch::BatchPayload) -> Result<()> {
        let songs = self.library_orchestrator().resolve_batch(batch).await?;
        self.queue_orchestrator().play_next(songs).await
    }

    /// Insert a batch at a specific position in the queue.
    pub async fn insert_batch_at_position(
        &self,
        batch: crate::types::batch::BatchPayload,
        position: usize,
    ) -> Result<()> {
        let songs = self.library_orchestrator().resolve_batch(batch).await?;
        self.queue_orchestrator().insert_at(songs, position).await?;
        debug!("📌 Inserted batch at queue position {}", position);
        Ok(())
    }

    /// Remove queue rows by per-row `entry_id` and keep the audio engine in
    /// sync. Targets specific rows rather than every row matching a song_id,
    /// so right-click "Remove from queue" on one of several duplicate rows
    /// leaves the other duplicates playing.
    ///
    /// The bare [`QueueService::remove_entries_by_ids`] only mutates queue
    /// state. If the currently-playing row is among those removed, the
    /// queue's `current_index` is clamped to the next valid slot — but the
    /// engine keeps decoding the removed song's URL, so the UI would
    /// advertise a different "now playing" track than the engine is
    /// producing.
    ///
    /// This method closes that gap: snapshot what the navigator was playing,
    /// resolve the removed entry_ids back to song_ids before mutating (so
    /// the aftermath plan can ask "was the playing song among the removed?"),
    /// mutate the queue, then ask
    /// [`crate::services::playback::decide_removal_aftermath`] whether the
    /// engine needs to swap sources or stop, and execute that plan via
    /// [`PlaybackController::apply_removal_aftermath`]. The reactive UI
    /// projection happens atomically inside `remove_entries_by_ids`; the
    /// aftermath step does engine/navigator work only and never mutates the
    /// queue, so no trailing refresh is needed.
    pub async fn remove_queue_entries(&self, entry_ids: &[u64]) -> Result<()> {
        if entry_ids.is_empty() {
            return Ok(());
        }

        let was_playing_id = self.playback.current_song_id().await;

        // Resolve each entry_id → its song_id *before* the removal. The
        // post-removal queue no longer holds those entries, and
        // `decide_removal_aftermath` needs the song_ids to ask "was the
        // currently-playing song among the removed?".
        let removed_song_ids: Vec<String> = {
            let qm_arc = self.queue_service.queue_manager();
            let qm = qm_arc.lock().await;
            entry_ids
                .iter()
                .filter_map(|&eid| {
                    qm.index_of_entry(eid)
                        .and_then(|idx| qm.get_queue().song_ids.get(idx).cloned())
                })
                .collect()
        };

        let effect = self.queue_service.remove_entries_by_ids(entry_ids).await?;

        let plan = {
            let qm_arc = self.queue_service.queue_manager();
            let qm = qm_arc.lock().await;
            crate::services::playback::decide_removal_aftermath(
                &qm,
                was_playing_id.as_deref(),
                &removed_song_ids,
            )
        };

        self.playback.apply_removal_aftermath(plan).await?;
        // `apply_removal_aftermath` invalidates engine prep on the
        // `LoadNewCurrent` path (via `load_track_with_rg`), but the
        // `NoCurrentChange` branch doesn't touch the engine — the
        // removed row could still be the song the engine had buffered
        // as the next gapless track. Always discharge the
        // `NextTrackResetEffect` so that case can't leave a stale
        // prepared decoder pointing at a vanished queue row.
        effect.apply_to(&self.audio_engine()).await;

        Ok(())
    }

    /// Move a queue item from one position to another (drag-and-drop reorder
    /// or Shift+↑ / Shift+↓ hotkey). Mutates the queue, refreshes the
    /// reactive projection, and invalidates the audio engine's prepared
    /// next-track decoder — without that final step, a shuffle+crossfade
    /// reorder leaves the engine streaming the originally-prepared song
    /// while the UI highlights whichever row the queue's re-shuffled
    /// `order[]` now picks as next.
    pub async fn move_queue_item(&self, from: usize, to: usize) -> Result<()> {
        let effect = self.queue_service.move_item(from, to).await?;
        effect.apply_to(&self.audio_engine()).await;
        Ok(())
    }

    /// Multi-selection drag reorder: remove the rows named by
    /// `raw_indices_desc` (which must already be in descending order) and
    /// re-insert their songs at `raw_target`, with the insertion point
    /// adjusted by however many of the removed rows fell before it.
    /// Discharges the resulting `NextTrackResetEffect` against the engine.
    pub async fn move_queue_batch(
        &self,
        raw_indices_desc: Vec<usize>,
        raw_target: usize,
    ) -> Result<()> {
        let effect = {
            let qm_arc = self.queue_service.queue_manager();
            let mut qm = qm_arc.lock().await;
            let mut extracted = Vec::new();
            for &qi in &raw_indices_desc {
                if let Some(id) = qm.get_queue().song_ids.get(qi).cloned()
                    && let Some(song) = qm.get_song(&id)
                {
                    extracted.push(song.clone());
                }
            }
            for &qi in &raw_indices_desc {
                let _ = qm.remove_song(qi);
            }
            extracted.reverse();
            let removed_before = raw_indices_desc
                .iter()
                .filter(|&&qi| qi < raw_target)
                .count();
            let adj = raw_target.saturating_sub(removed_before);
            let pos = adj.min(qm.get_queue().song_ids.len());
            qm.insert_songs_at(pos, extracted)?
        };
        self.queue_service.refresh_from_queue().await?;
        effect.apply_to(&self.audio_engine()).await;
        Ok(())
    }

    /// "Play Next" batch from the queue context menu: remove the targeted
    /// rows by `entry_id`, then re-insert their songs right after the
    /// currently-playing position. Discharges the resulting effect against
    /// the engine.
    pub async fn play_next_in_queue(&self, entry_ids: Vec<u64>) -> Result<()> {
        let effect = {
            let qm_arc = self.queue_service.queue_manager();
            let mut qm = qm_arc.lock().await;
            // Resolve each entry_id → its current song_id → pool clone
            // *before* the removal so pool entries that would otherwise
            // be dropped along with the last queue row are still
            // reachable.
            let extracted: Vec<_> = entry_ids
                .iter()
                .filter_map(|&eid| {
                    let idx = qm.index_of_entry(eid)?;
                    let song_id = qm.get_queue().song_ids.get(idx).cloned()?;
                    qm.get_song(&song_id).cloned()
                })
                .collect();
            let _ = qm.remove_entries_by_ids(&entry_ids);
            qm.insert_after_current(extracted)?
        };
        self.queue_service.refresh_from_queue().await?;
        effect.apply_to(&self.audio_engine()).await;
        Ok(())
    }

    /// Sort the queue physically by `mode` + `ascending`, persist the new
    /// sort prefs, refresh the reactive projection, and discharge the
    /// resulting `NextTrackResetEffect`. `Random` falls through to
    /// `shuffle_queue` (which doesn't persist).
    pub async fn sort_queue(&self, mode: QueueSortMode, ascending: bool) -> Result<()> {
        let effect = {
            let qm_arc = self.queue_service.queue_manager();
            let mut qm = qm_arc.lock().await;
            qm.sort_queue(mode, ascending)?
        };
        self.queue_service.refresh_from_queue().await?;
        effect.apply_to(&self.audio_engine()).await;
        self.settings_service.set_queue_prefs(mode, ascending).await
    }

    /// Re-shuffle the queue (the `Random` sort-mode dispatch). Same as
    /// [`Self::sort_queue`] but skips the prefs persistence — picking
    /// `Random` is not a deterministic preference worth saving.
    pub async fn shuffle_queue_randomly(&self) -> Result<()> {
        let effect = {
            let qm_arc = self.queue_service.queue_manager();
            let mut qm = qm_arc.lock().await;
            qm.sort_queue(QueueSortMode::Random, true)?
        };
        self.queue_service.refresh_from_queue().await?;
        effect.apply_to(&self.audio_engine()).await;
        Ok(())
    }

    /// Clear the queue: stop the engine, drop its source, clear the queue
    /// model, and refresh the projection. Discharges the
    /// `NextTrackResetEffect` produced by `set_queue` so a previously
    /// prepared gapless decoder doesn't outlive the cleared queue.
    pub async fn clear_queue(&self) -> Result<()> {
        let engine_arc = self.audio_engine();
        {
            let mut engine = engine_arc.lock().await;
            let _ = engine.stop().await;
            engine.set_source(String::new()).await;
        }
        let effect = self.queue_service.set_queue(Vec::new(), None).await?;
        effect.apply_to(&engine_arc).await;
        Ok(())
    }
}

// =============================================================================
// === Shutdown ===
// =============================================================================
impl AppService {
    /// Signal everything to stop before the process exits.
    ///
    /// Fans out to:
    /// 1. `CustomAudioEngine::request_shutdown` — supersedes the decode-loop
    ///    generation counter, joins the render thread, and stops the renderer.
    ///    The engine mutex is held only for the duration of this synchronous
    ///    call; no network I/O occurs here.
    /// 2. `TaskManager::shutdown_all` — fires the shared `CancellationToken` and
    ///    awaits tracked `JoinHandle`s up to a 500 ms internal budget, aborting
    ///    stragglers. The 500 ms sits inside the caller's 750 ms outer budget.
    ///
    /// The caller is responsible for wrapping this in a `tokio::time::timeout`
    /// (recommended ≤ 750 ms) so a slow engine mutex acquisition or a stuck
    /// blocking worker cannot defer window close beyond user patience.
    ///
    /// Idempotent: generation supersede is monotonic; `CancellationToken::cancel`
    /// is a no-op when already cancelled; `shutdown_all` on a drained `JoinSet`
    /// returns 0 without panicking.
    pub async fn request_shutdown(&self) {
        debug!(" [APP SERVICE] request_shutdown: locking audio engine");
        let engine_arc = self.audio_engine();
        let mut engine = engine_arc.lock().await;
        engine.request_shutdown();
        drop(engine);

        debug!(" [APP SERVICE] request_shutdown: awaiting task manager drain");
        let clean = self
            .task_manager
            .shutdown_all(std::time::Duration::from_millis(500))
            .await;
        debug!(" [APP SERVICE] request_shutdown: {clean} tasks finished cleanly");
    }
}

// =============================================================================
// === Library orchestrator accessor ===
// =============================================================================
impl AppService {
    /// Borrow-handle for entity-to-songs resolution. Holds no state; the
    /// returned orchestrator borrows the underlying services for the
    /// duration of the call chain.
    pub(crate) fn library_orchestrator(&self) -> crate::backend::LibraryOrchestrator<'_> {
        crate::backend::LibraryOrchestrator::new(
            &self.auth_gateway,
            &self.albums_service,
            &self.artists_service,
        )
    }
}
