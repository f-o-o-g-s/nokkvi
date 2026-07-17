//! AppService — top-level orchestrator for backend services
//!
//! Composes domain services (Albums, Artists, Songs, Queue, Settings, Auth) with
//! `PlaybackController`. Provides intent-based methods (play album, add to queue)
//! that encapsulate multi-step workflows.

use std::{collections::HashSet, sync::Arc};

use anyhow::Result;
use parking_lot::RwLock;
use tokio::sync::{Mutex, mpsc::UnboundedReceiver};
use tracing::{debug, info, warn};

use crate::{
    audio::engine::CustomAudioEngine,
    backend::{
        albums::AlbumsService,
        artists::ArtistsService,
        auth::AuthGateway,
        playback_controller::PlaybackController,
        queue::QueueService,
        queue_orchestrator::{QueueVerb, StartPosition},
        settings::SettingsService,
        songs::SongsService,
    },
    services::{
        radio_scrobble::{
            RadioSubmitOutcome, ScrobbleTargets, ScrobbleTrack,
            lastfm::{LastfmClient, LastfmSession},
            listenbrainz::{DEFAULT_API_URL, ListenBrainzClient, TokenValidation},
        },
        state_storage::ACTIVE_LIBRARY_IDS_KEY,
        storage_keys::{
            LASTFM_API_KEY, LASTFM_API_SECRET, LASTFM_SESSION_KEY, LASTFM_USERNAME,
            LISTENBRAINZ_TOKEN,
        },
        task_manager::TaskManager,
    },
    types::{
        library::Library, one_shot_shuffle::OneShotShuffle, queue_sort_mode::QueueSortMode,
        song_source::SongSource,
    },
};

/// User-Agent for direct scrobble-service requests. ListenBrainz/MusicBrainz
/// etiquette wants a descriptive agent with a contact URL (distinct from the
/// bare `crate::USER_AGENT` used for Navidrome).
const RADIO_SCROBBLE_USER_AGENT: &str = "nokkvi (+https://github.com/f-o-o-g-s/nokkvi)";

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
    /// User's currently-active library (music folder) ID selection.
    /// Empty == "no filter" (treated as "all"). Shared across clones via
    /// `Arc`, with `parking_lot::RwLock` for cheap read-mostly access on
    /// the hot path (every `load_*` reads on dispatch).
    active_library_ids: Arc<RwLock<HashSet<i32>>>,
    /// Last-seen full list of accessible libraries reported by the
    /// server. Refreshed via [`refresh_libraries`]; the popover renders
    /// from this snapshot. Shared across clones for the same reason as
    /// `active_library_ids`.
    all_libraries: Arc<RwLock<Vec<Library>>>,
    /// Shared HTTP client for direct scrobble-service submissions (ListenBrainz
    /// and Last.fm). Separate from the Navidrome `ApiClient` and the artwork
    /// client — radio scrobbling does not go through Navidrome.
    radio_scrobble_http: Arc<reqwest::Client>,
    /// Session cache for `resolve_lyrics`, keyed by song id. Caches negatives
    /// (a `None` value) too, so a repeated skip past an unmatched track doesn't
    /// re-hit the network. `Arc<Mutex<...>>` so it survives the per-`shell_task`
    /// `AppService` clone (a plain field would be cloned independently).
    lyrics_cache:
        Arc<parking_lot::Mutex<lru::LruCache<String, Option<crate::types::lyrics::LrcDocument>>>>,
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
        // Move any GUI-entered radio creds left in redb into config.toml (the
        // GUI write target) so existing values surface where they're now stored.
        Self::migrate_radio_creds_to_config(&storage);

        let auth_gateway = AuthGateway::new()?;
        let queue_service = QueueService::new(auth_gateway.clone(), storage.clone())?;
        let settings_service = SettingsService::new(storage.clone())?;

        // Initialize the queue service with persisted data
        queue_service.initialize().await?;

        // Restore the persisted multi-library selection. A read failure
        // is non-fatal — the user gets the "no filter" default for this
        // session and a warning lands in the file log. The pruning step
        // (validating each ID against the server's current library list)
        // happens lazily on the first `refresh_libraries` call.
        let active_library_ids: HashSet<i32> = match storage
            .load_binary::<HashSet<i32>>(ACTIVE_LIBRARY_IDS_KEY)
        {
            Ok(Some(set)) => {
                debug!(
                    " [APP SERVICE] restored {} active library IDs from storage",
                    set.len()
                );
                set
            }
            Ok(None) => HashSet::new(),
            Err(e) => {
                warn!(
                    "[APP SERVICE] failed to load active_library_ids ({e}); defaulting to empty selection"
                );
                HashSet::new()
            }
        };

        let task_manager = Arc::new(TaskManager::new());
        let radio_scrobble_http = Arc::new(
            reqwest::Client::builder()
                .user_agent(RADIO_SCROBBLE_USER_AGENT)
                .connect_timeout(std::time::Duration::from_secs(8))
                .read_timeout(std::time::Duration::from_secs(20))
                .build()
                .expect("Failed to build radio-scrobble HTTP client"),
        );
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
            active_library_ids: Arc::new(RwLock::new(active_library_ids)),
            all_libraries: Arc::new(RwLock::new(Vec::new())),
            radio_scrobble_http,
            lyrics_cache: Arc::new(parking_lot::Mutex::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(128).unwrap_or(std::num::NonZeroUsize::MIN),
            ))),
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
    pub async fn previous(&self) -> Result<crate::services::queue::PreviousOutcome> {
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
    /// Drift-immune row-handle play. See
    /// [`crate::backend::playback_controller::PlaybackController::play_entry_from_queue`].
    pub async fn play_entry_from_queue(&self, entry_id: u64) -> Result<()> {
        self.playback.play_entry_from_queue(entry_id).await
    }

    // =========================================================================
    // Intent-Based Orchestration Methods
    //
    // Every public entity-verb wrapper (play_* / add_* / insert_* /
    // play_next_*) is a one-line delegation to `dispatch` below.
    // Handlers should call the wrappers instead of defining workflows inline.
    // =========================================================================

    /// Single source-verb entry point behind every entity-verb wrapper:
    /// resolve `source` into songs, apply the one shared empty guard
    /// (entity-named, toast-surfaced message from
    /// [`SongSource::empty_error_message`]), then hand the songs to the
    /// matching [`crate::backend::QueueOrchestrator`] verb.
    ///
    /// Never holds the engine lock — `NextTrackResetEffect` discharge stays
    /// inside the orchestrator verbs.
    async fn dispatch(&self, source: SongSource, verb: QueueVerb) -> Result<()> {
        self.dispatch_shuffled(source, verb, OneShotShuffle::None)
            .await
    }

    /// [`Self::dispatch`] plus an optional one-shot permutation of the resolved
    /// list.
    ///
    /// The shuffle is applied to the detached `Vec<Song>` AFTER resolution and
    /// BEFORE the queue verb runs; it NEVER writes the persistent shuffle MODE
    /// (`queue.shuffle`), so the player-bar indicator stays off (the firmium-trap
    /// guard). A shuffled list is always played from index 0 — `AnchorFirst` has
    /// pre-pinned the intended track there — so any caller-supplied
    /// `StartPosition::Index` is ignored under shuffle.
    async fn dispatch_shuffled(
        &self,
        source: SongSource,
        verb: QueueVerb,
        shuffle: OneShotShuffle,
    ) -> Result<()> {
        // `resolve` consumes the source — capture the log label and the
        // empty-resolution message first.
        let label = source.log_label();
        let empty_msg = source.empty_error_message();
        let mut songs = self.library_orchestrator().resolve(source).await?;
        if songs.is_empty() {
            return Err(anyhow::anyhow!(empty_msg));
        }
        // One-shot permutation of the detached list (no-op for `None`).
        shuffle.apply(&mut songs);
        let count = songs.len();
        match verb {
            QueueVerb::Play(position) => {
                let start = if shuffle.shuffles() {
                    // The list is already permuted; play from the top.
                    0
                } else {
                    match position {
                        StartPosition::First => 0,
                        // `play_songs_from_index` clamps with `.min(len - 1)`,
                        // identical to the wrappers' former pre-clamps.
                        StartPosition::Index(index) => index,
                        // Safe post-guard: `songs` is non-empty.
                        StartPosition::Random => rand::random_range(0..songs.len()),
                    }
                };
                self.queue_orchestrator().play(songs, start).await?;
            }
            QueueVerb::Enqueue => self.queue_orchestrator().enqueue(songs).await?,
            QueueVerb::EnqueueAndPlay => {
                self.queue_orchestrator().enqueue_and_play(songs).await?;
            }
            QueueVerb::InsertAt(position) => {
                self.queue_orchestrator().insert_at(songs, position).await?;
            }
            QueueVerb::PlayNext => self.queue_orchestrator().play_next(songs).await?,
        }
        debug!(
            "Queue dispatch: {:?} of {} song(s) from {} (shuffle={:?})",
            verb, count, label, shuffle
        );
        Ok(())
    }

    /// Play an album by ID.
    ///
    /// Loads all songs in the album, sets queue, and starts playback.
    pub async fn play_album(&self, album_id: &str, shuffle: OneShotShuffle) -> Result<()> {
        self.dispatch_shuffled(
            SongSource::Album(album_id.to_owned()),
            QueueVerb::Play(StartPosition::First),
            shuffle,
        )
        .await
    }

    /// Play an album starting from a specific track index.
    ///
    /// Loads all songs in the album, sets queue, and starts playback from `track_idx`.
    pub async fn play_album_from_track(
        &self,
        album_id: &str,
        track_idx: usize,
        shuffle: OneShotShuffle,
    ) -> Result<()> {
        if shuffle == OneShotShuffle::AnchorFirst {
            return self
                .dispatch_anchor_first_from_track(SongSource::Album(album_id.to_owned()), track_idx)
                .await;
        }
        self.dispatch_shuffled(
            SongSource::Album(album_id.to_owned()),
            QueueVerb::Play(StartPosition::Index(track_idx)),
            shuffle,
        )
        .await
    }

    /// Resolve `source` to its tracks, rotate the clicked track to index 0, then
    /// shuffle the tail and play from the top (`AnchorFirst`). Shared by the
    /// album/playlist `*_from_track` paths — `dispatch` resolves internally and
    /// cannot relocate the clicked track, so the pin happens here before dispatch.
    async fn dispatch_anchor_first_from_track(
        &self,
        source: SongSource,
        track_idx: usize,
    ) -> Result<()> {
        let mut songs = self.library_orchestrator().resolve(source).await?;
        if !songs.is_empty() {
            let idx = track_idx.min(songs.len() - 1);
            songs.swap(0, idx);
        }
        self.dispatch_shuffled(
            SongSource::Preloaded(songs),
            QueueVerb::Play(StartPosition::First),
            OneShotShuffle::AnchorFirst,
        )
        .await
    }

    /// Play all songs by an artist.
    ///
    /// Loads all songs by this artist, sets queue, and starts playback.
    pub async fn play_artist(&self, artist_id: &str, shuffle: OneShotShuffle) -> Result<()> {
        self.dispatch_shuffled(
            SongSource::Artist(artist_id.to_owned()),
            QueueVerb::Play(StartPosition::First),
            shuffle,
        )
        .await
    }

    /// Play all songs in a genre.
    ///
    /// Loads all songs in this genre, sets queue, and starts playback.
    pub async fn play_genre(&self, genre_name: &str, shuffle: OneShotShuffle) -> Result<()> {
        self.dispatch_shuffled(
            SongSource::Genre(genre_name.to_owned()),
            QueueVerb::Play(StartPosition::First),
            shuffle,
        )
        .await
    }

    /// Roulette variant of [`Self::play_genre`]: load all genre songs, then
    /// start playback from a random index. Used by the slot-machine
    /// roulette pick on the Genres view.
    pub async fn play_genre_random(&self, genre_name: &str) -> Result<()> {
        self.dispatch(
            SongSource::Genre(genre_name.to_owned()),
            QueueVerb::Play(StartPosition::Random),
        )
        .await
    }

    /// Roulette variant of [`Self::play_artist`]: load all artist songs,
    /// then start playback from a random index.
    pub async fn play_artist_random(&self, artist_id: &str) -> Result<()> {
        self.dispatch(
            SongSource::Artist(artist_id.to_owned()),
            QueueVerb::Play(StartPosition::Random),
        )
        .await
    }

    /// Play all songs in a playlist.
    ///
    /// Loads all songs in this playlist, sets queue, and starts playback.
    pub async fn play_playlist(&self, playlist_id: &str, shuffle: OneShotShuffle) -> Result<()> {
        self.dispatch_shuffled(
            SongSource::Playlist(playlist_id.to_owned()),
            QueueVerb::Play(StartPosition::First),
            shuffle,
        )
        .await
    }

    /// Play a playlist starting from a specific track index.
    ///
    /// Loads all songs in the playlist, sets queue, and starts playback from `track_idx`.
    pub async fn play_playlist_from_track(
        &self,
        playlist_id: &str,
        track_idx: usize,
        shuffle: OneShotShuffle,
    ) -> Result<()> {
        if shuffle == OneShotShuffle::AnchorFirst {
            return self
                .dispatch_anchor_first_from_track(
                    SongSource::Playlist(playlist_id.to_owned()),
                    track_idx,
                )
                .await;
        }
        self.dispatch_shuffled(
            SongSource::Playlist(playlist_id.to_owned()),
            QueueVerb::Play(StartPosition::Index(track_idx)),
            shuffle,
        )
        .await
    }

    /// Resolve a playlist's tracks into UI view-data WITHOUT touching the play
    /// queue, the audio engine, or persisted state.
    ///
    /// This is the playlist editor's load path: the editor owns an independent
    /// in-memory buffer, so editing a playlist never disturbs what the user is
    /// hearing.
    pub async fn resolve_playlist_for_editor(
        &self,
        playlist_id: &str,
    ) -> Result<Vec<crate::backend::queue::QueueSongUIViewData>> {
        let songs = self
            .library_orchestrator()
            .resolve_playlist(playlist_id)
            .await?;
        Ok(self.project_songs_for_editor(&songs).await)
    }

    /// Resolve a `BatchPayload` (e.g. a cross-pane drag from the browsing
    /// panel) into editor view-data rows WITHOUT touching the queue, audio
    /// engine, or redb.
    ///
    /// The caller assigns final `entry_id`s when splicing the rows into the
    /// editor buffer (they must not collide with existing buffer ids); the
    /// `entry_id`/`track_number` baked in here are placeholders derived from
    /// the batch's own ordering.
    pub async fn resolve_batch_for_editor(
        &self,
        batch: crate::types::batch::BatchPayload,
    ) -> Result<Vec<crate::backend::queue::QueueSongUIViewData>> {
        let songs = self.library_orchestrator().resolve_batch(batch).await?;
        Ok(self.project_songs_for_editor(&songs).await)
    }

    /// Shared projection tail for the editor resolvers above. Rows use the
    /// shared projection
    /// ([`crate::backend::queue::build_queue_song_ui_view_data`]) so they
    /// match live-queue rows exactly; `entry_id`s/`track_number`s are
    /// positional placeholders (the editor has no `QueueManager` to hand
    /// them out — callers re-assign on splice).
    async fn project_songs_for_editor(
        &self,
        songs: &[crate::types::song::Song],
    ) -> Vec<crate::backend::queue::QueueSongUIViewData> {
        let (server_url, subsonic_credential) = self.auth_gateway.server_config().await;
        songs
            .iter()
            .enumerate()
            .map(|(index, song)| {
                crate::backend::queue::build_queue_song_ui_view_data(
                    song,
                    index,
                    index as u64,
                    &server_url,
                    &subsonic_credential,
                )
            })
            .collect()
    }

    /// Play a pre-loaded list of songs, starting at a specific index.
    ///
    /// Use this when you already have the songs list (e.g., Songs view with current sort).
    pub async fn play_songs(
        &self,
        songs: Vec<crate::types::song::Song>,
        start_index: usize,
        shuffle: OneShotShuffle,
    ) -> Result<()> {
        // Under `Full` the supplied `start_index` is ignored (dispatch plays the
        // permuted list from the top); `None` preserves the click-to-play index.
        self.dispatch_shuffled(
            SongSource::Preloaded(songs),
            QueueVerb::Play(StartPosition::Index(start_index)),
            shuffle,
        )
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
        self.dispatch(SongSource::Album(album_id.to_owned()), QueueVerb::Enqueue)
            .await
    }

    /// Add an artist's songs to the queue (without starting playback).
    pub async fn add_artist_to_queue(&self, artist_id: &str) -> Result<()> {
        self.dispatch(SongSource::Artist(artist_id.to_owned()), QueueVerb::Enqueue)
            .await
    }

    /// Add a single song to the queue (without starting playback).
    pub async fn add_song_to_queue(&self, song: crate::types::song::Song) -> Result<()> {
        self.dispatch(SongSource::Preloaded(vec![song]), QueueVerb::Enqueue)
            .await
    }

    /// Add a single song to the queue and immediately start playing it.
    ///
    /// Used by `EnterBehavior::AppendAndPlay` — preserves existing queue.
    pub async fn add_song_and_play(&self, song: crate::types::song::Song) -> Result<()> {
        self.dispatch(SongSource::Preloaded(vec![song]), QueueVerb::EnqueueAndPlay)
            .await
    }

    /// Add a single song to the queue by ID (loads from album first).
    pub async fn add_song_to_queue_by_id(&self, song_id: &str, album_id: &str) -> Result<()> {
        let songs = self.albums_service.load_album_songs(album_id).await?;
        if let Some(song) = songs.into_iter().find(|s| s.id == song_id) {
            self.queue_orchestrator().enqueue(vec![song]).await?;
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
        self.dispatch(SongSource::Genre(genre_name.to_owned()), QueueVerb::Enqueue)
            .await
    }

    /// Add all songs in a playlist to the queue (without starting playback).
    pub async fn add_playlist_to_queue(&self, playlist_id: &str) -> Result<()> {
        self.dispatch(
            SongSource::Playlist(playlist_id.to_owned()),
            QueueVerb::Enqueue,
        )
        .await
    }

    // =========================================================================
    // Append-and-Play helpers  (EnterBehavior::AppendAndPlay)
    //
    // Each loads songs, appends to queue, then starts playing the first one.
    // =========================================================================

    /// Append an album's songs to the queue and start playing the first one.
    pub async fn add_album_and_play(&self, album_id: &str, shuffle: OneShotShuffle) -> Result<()> {
        self.dispatch_shuffled(
            SongSource::Album(album_id.to_owned()),
            QueueVerb::EnqueueAndPlay,
            shuffle,
        )
        .await
    }

    /// Append an artist's songs to the queue and start playing the first one.
    pub async fn add_artist_and_play(
        &self,
        artist_id: &str,
        shuffle: OneShotShuffle,
    ) -> Result<()> {
        self.dispatch_shuffled(
            SongSource::Artist(artist_id.to_owned()),
            QueueVerb::EnqueueAndPlay,
            shuffle,
        )
        .await
    }

    /// Append a genre's songs to the queue and start playing the first one.
    pub async fn add_genre_and_play(
        &self,
        genre_name: &str,
        shuffle: OneShotShuffle,
    ) -> Result<()> {
        self.dispatch_shuffled(
            SongSource::Genre(genre_name.to_owned()),
            QueueVerb::EnqueueAndPlay,
            shuffle,
        )
        .await
    }

    /// Append a playlist's songs to the queue and start playing the first one.
    pub async fn add_playlist_and_play(
        &self,
        playlist_id: &str,
        shuffle: OneShotShuffle,
    ) -> Result<()> {
        self.dispatch_shuffled(
            SongSource::Playlist(playlist_id.to_owned()),
            QueueVerb::EnqueueAndPlay,
            shuffle,
        )
        .await
    }

    // =========================================================================
    // Insert-at-Position Orchestration Methods
    //
    // These mirror the add-to-queue methods above but insert at a specific
    // queue index (used by cross-pane drag-and-drop).
    // =========================================================================

    /// Insert an album's songs at a specific position in the queue.
    pub async fn insert_album_at_position(&self, album_id: &str, position: usize) -> Result<()> {
        self.dispatch(
            SongSource::Album(album_id.to_owned()),
            QueueVerb::InsertAt(position),
        )
        .await
    }

    /// Insert an artist's songs at a specific position in the queue.
    pub async fn insert_artist_at_position(&self, artist_id: &str, position: usize) -> Result<()> {
        self.dispatch(
            SongSource::Artist(artist_id.to_owned()),
            QueueVerb::InsertAt(position),
        )
        .await
    }

    /// Insert a single song at a specific position in the queue.
    pub async fn insert_song_at_position(
        &self,
        song: crate::types::song::Song,
        position: usize,
    ) -> Result<()> {
        self.dispatch(
            SongSource::Preloaded(vec![song]),
            QueueVerb::InsertAt(position),
        )
        .await
    }

    /// Insert all songs in a genre at a specific position in the queue.
    pub async fn insert_genre_at_position(&self, genre_name: &str, position: usize) -> Result<()> {
        self.dispatch(
            SongSource::Genre(genre_name.to_owned()),
            QueueVerb::InsertAt(position),
        )
        .await
    }
}

/// Outcome summary of a pull-from-server queue restore.
#[derive(Debug, Clone, Copy)]
pub struct PullSummary {
    /// Entries actually restored (0 = nothing saved on the server).
    pub restored: usize,
    /// Clamped physical playhead index the queue was anchored on.
    pub current_index: Option<usize>,
    /// Server-saved offset into the current track, in milliseconds.
    pub position_ms: i64,
}

// === Server queue sync (OpenSubsonic indexBasedQueue) ===
impl AppService {
    /// PUSH — serialize the whole ordered song queue to the server via
    /// `savePlayQueueByIndex` (full replace; one queue per user, each save
    /// overwrites the previous). Returns the pushed row count.
    ///
    /// An empty local queue is refused rather than silently CLEARING the
    /// server's saved queue (an empty save is a clear, server-side).
    pub async fn push_queue(&self) -> Result<usize> {
        // Read the id list + derived playhead in ONE queue-lock snapshot so a
        // gapless auto-advance can't land between two separate reads.
        let (ids, current_index): (Vec<String>, Option<usize>) = {
            let qm_arc = self.queue_service.queue_manager();
            let qm = qm_arc.lock().await;
            (qm.song_ids_snapshot(), qm.current_index())
        };
        if ids.is_empty() {
            anyhow::bail!("Queue is empty — nothing to push");
        }
        // Position in ms from the ENGINE (u64 ms; a stopped engine reads 0).
        // A staged-but-never-played pull keeps its server position in the
        // armed one-shot offset while `position()` reads 0 — prefer it so a
        // pull→push round trip without pressing Play doesn't zero the
        // server's saved position. The tokio mutex hold is trivially short.
        let position_ms = {
            let engine_arc = self.audio_engine();
            let engine = engine_arc.lock().await;
            engine
                .pending_start_ms()
                .unwrap_or_else(|| engine.position()) as i64
        };
        // A None cursor on a non-empty queue is coerced to 0 inside the API
        // layer (Navidrome range-checks currentIndex on non-empty saves).
        let id_refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        self.play_queue_api()
            .await?
            .save_play_queue_by_index(&id_refs, current_index, position_ms)
            .await?;
        info!("🌊 Pushed {} queue rows to the server", ids.len());
        Ok(ids.len())
    }

    /// PULL — fetch the server's saved queue, clamp the playhead against the
    /// actually-returned entries (the server silently drops library-missing
    /// ids), replace the local queue model, and cue the engine ("cue, don't
    /// play" — play/pause state is preserved).
    pub async fn pull_queue(&self) -> Result<PullSummary> {
        use crate::services::api::play_queue::clamp_pulled_index;

        let pq = self
            .play_queue_api()
            .await?
            .get_play_queue_by_index()
            .await?;
        let Some(pq) = pq else {
            return Ok(PullSummary {
                restored: 0,
                current_index: None,
                position_ms: 0,
            });
        };
        if pq.entry.is_empty() {
            return Ok(PullSummary {
                restored: 0,
                current_index: None,
                position_ms: 0,
            });
        }

        let clamped = clamp_pulled_index(pq.current_index, pq.entry.len());
        let restored = pq.entry.len();
        let position_ms = pq.position.max(0);
        let current_song_id = clamped.and_then(|i| pq.entry.get(i)).map(|s| s.id.clone());

        // Capture play/pause BEFORE mutating anything.
        let was_playing = self.playback.engine_is_playing().await;

        // 1. Replace the queue MODEL + reactive playhead; discharge the
        //    gapless-prep reset obligation against the engine.
        let effect = self.queue_service.set_queue(pq.entry, clamped).await?;
        let engine_arc = self.audio_engine();
        effect.apply_to(&engine_arc).await;

        // 2. Cue the engine: set_queue touched neither the engine source nor
        //    the navigator — without this, play() would resume the PRE-pull
        //    song (its non-empty-source early return).
        if let Some(song_id) = current_song_id {
            self.playback
                .cue_pulled_queue(&song_id, position_ms, was_playing)
                .await?;
        }
        info!(
            "🌊 Pulled {restored} queue rows from the server (index {clamped:?}, {position_ms}ms)"
        );
        Ok(PullSummary {
            restored,
            current_index: clamped,
            position_ms,
        })
    }
}

macro_rules! native_api_factory {
    ( $( ($method:ident, $ty:path) ),+ $(,)? ) => {
        $(
            /// Construct an authenticated Native-API service. Use `.await?` in `shell_task` closures.
            pub async fn $method(&self) -> anyhow::Result<$ty> {
                self.auth_gateway.build_native_api(<$ty>::new).await
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
    native_api_factory!(
        (songs_api, crate::services::api::songs::SongsApiService),
        (albums_api, crate::services::api::albums::AlbumsApiService),
        (
            artists_api,
            crate::services::api::artists::ArtistsApiService
        ),
    );

    subsonic_api_factory!(
        (genres_api, crate::services::api::genres::GenresApiService),
        (
            libraries_api,
            crate::services::api::libraries::LibrariesApiService
        ),
        (
            playlists_api,
            crate::services::api::playlists::PlaylistsApiService
        ),
        (radios_api, crate::services::api::radios::RadiosApiService),
        (
            similar_api,
            crate::services::api::similar::SimilarApiService
        ),
        (lyrics_api, crate::services::api::lyrics::LyricsApiService),
        (
            play_queue_api,
            crate::services::api::play_queue::PlayQueueApiService
        ),
    );
}

// === Lyrics resolution ===
impl AppService {
    /// Resolve synced lyrics for a song through the store -> `getLyricsBySongId`
    /// -> LRCLIB chain (the "Feishin or better" precedence), caching the result
    /// including a negative. Iced-free; the UI's `shell_task` drives it with the
    /// current index and the gating opts. Probes are lazy — a store hit never
    /// touches the network.
    pub async fn resolve_lyrics(
        &self,
        song: &crate::types::song::Song,
        index: Option<std::sync::Arc<crate::types::lyrics::LyricsIndex>>,
        opts: crate::services::lyrics_source::ResolveOpts,
    ) -> Option<crate::types::lyrics::LrcDocument> {
        use crate::types::lyrics::{LrcDocument, parse};

        if let Some(cached) = self.lyrics_cache.lock().get(&song.id).cloned() {
            return cached;
        }

        let store = || async {
            let index = index.as_ref()?;
            let album = (!song.album.is_empty()).then_some(song.album.as_str());
            let path = index
                .find(
                    &song.artist,
                    &song.title,
                    album,
                    Some(song.duration.saturating_mul(1000)),
                )?
                .path
                .clone();
            let text = tokio::fs::read_to_string(&path).await.ok()?;
            let doc = parse(&text);
            doc.synced.then_some(doc)
        };
        let api = || async {
            let service = self.lyrics_api().await.ok()?;
            let list = service.get_lyrics_by_song_id(&song.id, true).await.ok()?;
            // Kind-selection lives here (the converter takes one entry): prefer
            // the main synced layer, else the first synced.
            let chosen = list
                .iter()
                .find(|s| s.kind.as_deref() == Some("main") && s.synced)
                .or_else(|| list.iter().find(|s| s.synced))?;
            let doc = LrcDocument::from_structured(chosen);
            doc.synced.then_some(doc)
        };
        let lrclib = || async {
            let (doc, raw) = crate::services::lyrics_source::fetch_lrclib(
                &song.artist,
                &song.title,
                &song.album,
                song.duration,
            )
            .await?;
            crate::services::lyrics_source::cache_to_store(&raw, song).await;
            Some(doc)
        };

        let result = crate::services::lyrics_source::resolve_from(store, api, lrclib, opts).await;
        self.lyrics_cache
            .lock()
            .put(song.id.clone(), result.clone());
        result
    }
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
    // Radio scrobbling (direct to ListenBrainz + Last.fm; bypasses Navidrome)
    // =========================================================================

    /// One-time move of GUI-entered radio creds from redb into `config.toml`,
    /// run at construction. Earlier builds wrote the ListenBrainz token + Last.fm
    /// key/secret to redb; they're now stored in `[radio_scrobble]` in
    /// config.toml. Copies any non-empty redb value forward only when config.toml
    /// doesn't already define that field (never clobbers a hand-edit), then clears
    /// the redb copies (config.toml is authoritative). Best-effort; non-fatal.
    fn migrate_radio_creds_to_config(storage: &crate::services::state_storage::StateStorage) {
        use crate::services::radio_scrobble::source;

        const KEYS: [(&str, &str); 3] = [
            (LISTENBRAINZ_TOKEN, "listenbrainz_token"),
            (LASTFM_API_KEY, "lastfm_api_key"),
            (LASTFM_API_SECRET, "lastfm_api_secret"),
        ];
        let redb_vals: Vec<(&str, String)> = KEYS
            .iter()
            .filter_map(|(redb_key, field)| {
                storage
                    .load::<String>(redb_key)
                    .ok()
                    .flatten()
                    .filter(|v| !v.trim().is_empty())
                    .map(|v| (*field, v))
            })
            .collect();
        if redb_vals.is_empty() {
            return;
        }

        // Only fill fields config.toml doesn't already define.
        let cfg = source::RadioScrobbleToml::load();
        let defined = |field: &str| match field {
            "listenbrainz_token" => cfg.listenbrainz_token.is_some(),
            "lastfm_api_key" => cfg.lastfm_api_key.is_some(),
            "lastfm_api_secret" => cfg.lastfm_api_secret.is_some(),
            _ => true,
        };
        let pending: Vec<(&str, &str)> = redb_vals
            .iter()
            .filter(|(field, _)| !defined(field))
            .map(|(field, val)| (*field, val.as_str()))
            .collect();
        if !pending.is_empty()
            && let Err(e) = source::write_config_fields(&pending)
        {
            warn!("radio-scrobble redb->config migration failed: {e}");
            return;
        }
        // Clear the redb copies — config.toml now owns these.
        for (redb_key, _) in KEYS {
            let _ = storage.save(redb_key, &String::new());
        }
    }

    /// Read a non-empty trimmed string from redb, or `None`.
    fn redb_string(&self, key: &str) -> Option<String> {
        match self.storage.load::<String>(key) {
            Ok(Some(s)) if !s.trim().is_empty() => Some(s),
            _ => None,
        }
    }

    /// Resolve every radio-scrobble credential together from a **single**
    /// config.toml read, each by precedence (env, then `[radio_scrobble]` in
    /// config.toml, then redb — the GUI write target), reporting which layer
    /// supplied each. The one shared resolution used by client construction and
    /// the settings badges — call this once rather than the per-field getters
    /// when more than one credential is needed (each getter re-reads config.toml).
    pub fn radio_credentials(&self) -> crate::services::radio_scrobble::source::RadioCreds {
        use crate::services::radio_scrobble::source;
        let cfg = source::RadioScrobbleToml::load();
        let (listenbrainz_token, listenbrainz_source) = source::resolve_with_source(
            std::env::var(source::ENV_LISTENBRAINZ_TOKEN).ok(),
            cfg.listenbrainz_token.as_deref(),
            self.redb_string(LISTENBRAINZ_TOKEN),
        );
        let (lastfm, lastfm_source) = source::resolve_pair_with_source(
            std::env::var(source::ENV_LASTFM_API_KEY).ok(),
            std::env::var(source::ENV_LASTFM_API_SECRET).ok(),
            cfg.lastfm_api_key.as_deref(),
            cfg.lastfm_api_secret.as_deref(),
            self.redb_string(LASTFM_API_KEY),
            self.redb_string(LASTFM_API_SECRET),
        );
        source::RadioCreds {
            listenbrainz_token,
            listenbrainz_source,
            lastfm,
            lastfm_source,
            lastfm_session: self.redb_string(LASTFM_SESSION_KEY),
            lastfm_username: self.redb_string(LASTFM_USERNAME),
        }
    }

    /// The effective ListenBrainz radio-scrobble token, or `None` when unset.
    /// Convenience over [`Self::radio_credentials`] for single-credential callers
    /// (validate / verify); resolves the same precedence.
    pub fn listenbrainz_token(&self) -> Option<String> {
        self.radio_credentials().listenbrainz_token
    }

    /// Persist (or clear, with an empty string) the ListenBrainz token to
    /// `config.toml`'s `[radio_scrobble]` — the GUI write target, so the value
    /// lands where the user can see and hand-edit it — and clear any legacy redb
    /// copy so a stale value can't shadow a cleared config entry.
    pub fn set_listenbrainz_token(&self, token: &str) -> Result<()> {
        crate::services::radio_scrobble::source::write_config_fields(&[(
            "listenbrainz_token",
            token,
        )])?;
        let _ = self.storage.save(LISTENBRAINZ_TOKEN, &String::new());
        Ok(())
    }

    /// Validate an arbitrary ListenBrainz token against the API (for the
    /// settings "Verify" action — does not persist).
    pub async fn validate_listenbrainz_token(&self, token: String) -> Result<TokenValidation> {
        ListenBrainzClient::new(self.radio_scrobble_http.clone(), DEFAULT_API_URL, token)
            .validate_token()
            .await
    }

    /// Validate a token and map to the owning username (possibly empty) on
    /// success, or a human-readable reason on failure. The single shared mapping
    /// used by both the settings "Verify" and "Set token" paths so they can't
    /// drift in how they report success/failure for the same token.
    pub async fn validate_listenbrainz_token_to_name(
        &self,
        token: String,
    ) -> std::result::Result<String, String> {
        let v = self
            .validate_listenbrainz_token(token)
            .await
            .map_err(|e| e.to_string())?;
        if v.valid {
            Ok(v.user_name.unwrap_or_default())
        } else {
            Err(v
                .message
                .unwrap_or_else(|| "token is not valid".to_string()))
        }
    }

    /// The effective Last.fm app key + secret, or `None` when unset. Convenience
    /// over [`Self::radio_credentials`]; the pair always resolves from one layer
    /// so the key + secret can't mismatch across registered apps.
    pub fn lastfm_credentials(&self) -> Option<(String, String)> {
        self.radio_credentials().lastfm
    }

    /// Persist the Last.fm app key + secret to `config.toml`'s `[radio_scrobble]`
    /// (the GUI write target), clearing any legacy redb copies. The session key +
    /// username stay in redb (browser-auth output).
    pub fn set_lastfm_credentials(&self, api_key: &str, api_secret: &str) -> Result<()> {
        crate::services::radio_scrobble::source::write_config_fields(&[
            ("lastfm_api_key", api_key),
            ("lastfm_api_secret", api_secret),
        ])?;
        let _ = self.storage.save(LASTFM_API_KEY, &String::new());
        let _ = self.storage.save(LASTFM_API_SECRET, &String::new());
        Ok(())
    }

    /// The linked Last.fm username, or `None` when not connected.
    pub fn lastfm_username(&self) -> Option<String> {
        self.redb_string(LASTFM_USERNAME)
    }

    /// Last.fm desktop-auth step 1: fetch a request token (needs key+secret).
    /// Returns `(token, authorize_url)` for the browser step.
    pub async fn lastfm_begin_auth(&self) -> Result<(String, String)> {
        let (key, secret) = self
            .lastfm_credentials()
            .ok_or_else(|| anyhow::anyhow!("Set your Last.fm API key and secret first"))?;
        let client = LastfmClient::new(self.radio_scrobble_http.clone(), key, secret);
        let token = client.get_token().await?;
        let url = client.authorize_url(&token);
        Ok((token, url))
    }

    /// Last.fm desktop-auth step 3: exchange an authorized token for a session,
    /// persist the session key + username, and return the username.
    pub async fn lastfm_complete_auth(&self, token: String) -> Result<String> {
        let (key, secret) = self
            .lastfm_credentials()
            .ok_or_else(|| anyhow::anyhow!("Set your Last.fm API key and secret first"))?;
        debug!("Last.fm complete_auth: exchanging token for a session");
        let LastfmSession { name, key: sk } =
            LastfmClient::new(self.radio_scrobble_http.clone(), key, secret)
                .get_session(&token)
                .await?;
        self.storage.save(LASTFM_SESSION_KEY, &sk)?;
        self.storage.save(LASTFM_USERNAME, &name)?;
        debug!("Last.fm complete_auth: session saved for user '{name}'");
        Ok(name)
    }

    /// Clear the Last.fm session (disconnect); keeps the app key/secret.
    pub fn disconnect_lastfm(&self) -> Result<()> {
        self.storage.save(LASTFM_SESSION_KEY, &String::new())?;
        self.storage.save(LASTFM_USERNAME, &String::new())
    }

    /// Run a submission against each REQUESTED + configured target concurrently,
    /// returning each target's outcome **independently** so a retry can
    /// re-dispatch only the targets that haven't yet succeeded. Resolves
    /// credentials once (single config.toml read) and builds both clients from
    /// that snapshot. A target that's unrequested or unconfigured yields `None`.
    async fn dispatch_radio<'a, LbFut, LfFut>(
        &'a self,
        targets: ScrobbleTargets,
        lb: impl FnOnce(ListenBrainzClient) -> LbFut,
        lf: impl FnOnce(LastfmClient) -> LfFut,
    ) -> RadioSubmitOutcome
    where
        LbFut: std::future::Future<Output = Result<()>> + 'a,
        LfFut: std::future::Future<Output = Result<()>> + 'a,
    {
        let creds = self.radio_credentials();
        let lb_client = (targets.listenbrainz)
            .then(|| creds.listenbrainz_token.clone())
            .flatten()
            .map(|t| ListenBrainzClient::new(self.radio_scrobble_http.clone(), DEFAULT_API_URL, t));
        let lf_client = (targets.lastfm)
            .then(|| match (&creds.lastfm, &creds.lastfm_session) {
                (Some((k, s)), Some(sk)) => Some(
                    LastfmClient::new(self.radio_scrobble_http.clone(), k.clone(), s.clone())
                        .with_session_key(sk.clone()),
                ),
                _ => None,
            })
            .flatten();
        debug!(
            "radio scrobble dispatch: listenbrainz={}, lastfm={}",
            lb_client.is_some(),
            lf_client.is_some()
        );
        // Run both targets concurrently — wall-clock is max(lb, lf), not the
        // sum, so a stalled target can't delay the other.
        let lb_fut = lb_client.map(lb);
        let lf_fut = lf_client.map(lf);
        let (lb_res, lf_res) = match (lb_fut, lf_fut) {
            (Some(a), Some(b)) => {
                let (ra, rb) = tokio::join!(a, b);
                (Some(ra), Some(rb))
            }
            (Some(a), None) => (Some(a.await), None),
            (None, Some(b)) => (None, Some(b.await)),
            (None, None) => (None, None),
        };
        RadioSubmitOutcome {
            listenbrainz: lb_res.map(|r| r.map_err(|e| e.to_string())),
            lastfm: lf_res.map(|r| r.map_err(|e| e.to_string())),
        }
    }

    /// Submit a now-playing update for a radio track to every configured target.
    /// Best-effort (no per-target retry) — collapses to one combined result.
    pub async fn radio_scrobble_now_playing(&self, track: ScrobbleTrack) -> Result<()> {
        let track = &track;
        self.dispatch_radio(
            ScrobbleTargets::ALL,
            |c| async move { c.submit_playing_now(track).await },
            |c| async move { c.update_now_playing(track).await },
        )
        .await
        .into_combined()
        .map_err(|e| anyhow::anyhow!(e))
    }

    /// Submit a completed listen (timestamped at `started_at`) to the requested
    /// targets, returning each target's outcome so the caller can latch / retry
    /// them independently.
    pub async fn radio_scrobble_submit(
        &self,
        track: ScrobbleTrack,
        started_at: i64,
        targets: ScrobbleTargets,
    ) -> RadioSubmitOutcome {
        let track = &track;
        self.dispatch_radio(
            targets,
            |c| async move { c.submit_listen(track, started_at).await },
            |c| async move { c.scrobble(track, started_at).await },
        )
        .await
    }

    // =========================================================================
    // Whole-library search
    // =========================================================================

    /// Search the entire library across all five entity types at once and
    /// return them as separate typed groups.
    ///
    /// Fans out the five per-entity browse loaders concurrently (each already
    /// maps the right search param: albums/artists/genres by `name`, songs by
    /// `title`, playlists by `q`) and merges via
    /// [`LibrarySearchResults::from_partial`], which is partial-tolerant — one
    /// flaky endpoint degrades to an empty group rather than blanking the whole
    /// search; `Err` only when all five fail (network down / session gone).
    ///
    /// `per_type_limit` caps every group: albums/artists/songs are limited
    /// server-side, genres/playlists (whose loaders have no limit param) are
    /// truncated inside `from_partial`. Callers should gate on a minimum query
    /// length before invoking — this method itself makes five network calls
    /// unconditionally.
    pub async fn search_library(
        &self,
        query: &str,
        per_type_limit: usize,
        library_ids: &[i32],
    ) -> Result<crate::types::library_search::LibrarySearchResults> {
        let q = query.trim();

        // Albums / artists / songs go through per-call native API services
        // (constructed by the `*_api()` factories), NOT the shared
        // Albums/Artists/Songs service singletons. Those singletons' raw-page
        // loaders write `total_count` on a process-wide reactive property that
        // the Albums/Artists/Songs browse views read to drive pagination — a
        // search fan-out firing on every keystroke would clobber their totals
        // mid-scroll. A freshly-constructed API service has no such shared
        // state; the search total is unused, so it is dropped like the
        // genres/playlists groups below. Artists: `album_artists_only = false`
        // so a search sees every artist, not just album artists. Each factory's
        // "not authenticated" error folds into the group's Result so one
        // failure degrades that group rather than failing the whole search.
        let albums_fut = async {
            let svc = self.albums_api().await?;
            svc.load_albums(
                "name",
                "ASC",
                Some(q),
                None,
                library_ids,
                Some(0),
                Some(per_type_limit),
            )
            .await
            .map(|(albums, _total)| albums)
        };
        let artists_fut = async {
            let svc = self.artists_api().await?;
            svc.load_artists(
                "name",
                "ASC",
                Some(q),
                None,
                library_ids,
                false,
                Some(0),
                Some(per_type_limit),
            )
            .await
            .map(|(artists, _total)| artists)
        };
        let songs_fut = async {
            let svc = self.songs_api().await?;
            svc.load_songs(
                "title",
                "ASC",
                Some(q),
                None,
                library_ids,
                Some(0),
                Some(per_type_limit),
            )
            .await
            .map(|(songs, _total)| songs)
        };

        // Genres / playlists are Subsonic-factory services; fold the
        // "not authenticated" construction error into the group's Result so a
        // single failure degrades that group instead of failing the whole
        // search (from_partial only errs when ALL five fail). Their loaders
        // return `(Vec, total)` — drop the count.
        let genres_fut = async {
            let svc = self.genres_api().await?;
            svc.load_genres_with_libraries("name", "ASC", Some(q), library_ids)
                .await
                .map(|(genres, _total)| genres)
        };
        let playlists_fut = async {
            let svc = self.playlists_api().await?;
            svc.load_playlists_with_libraries("name", "ASC", Some(q), library_ids)
                .await
                .map(|(playlists, _total)| playlists)
        };

        let (artists, albums, songs, genres, playlists) = futures::join!(
            artists_fut,
            albums_fut,
            songs_fut,
            genres_fut,
            playlists_fut
        );

        crate::types::library_search::LibrarySearchResults::from_partial(
            artists,
            albums,
            songs,
            genres,
            playlists,
            per_type_limit,
        )
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
    pub async fn play_batch(
        &self,
        batch: crate::types::batch::BatchPayload,
        shuffle: OneShotShuffle,
    ) -> Result<()> {
        self.dispatch_shuffled(
            SongSource::Batch(batch),
            QueueVerb::Play(StartPosition::First),
            shuffle,
        )
        .await
    }

    /// Add a batch to the queue (append).
    pub async fn add_batch_to_queue(&self, batch: crate::types::batch::BatchPayload) -> Result<()> {
        self.dispatch(SongSource::Batch(batch), QueueVerb::Enqueue)
            .await
    }

    /// Add a batch right after the currently playing track.
    pub async fn play_next_batch(&self, batch: crate::types::batch::BatchPayload) -> Result<()> {
        self.dispatch(SongSource::Batch(batch), QueueVerb::PlayNext)
            .await
    }

    /// Insert a batch at a specific position in the queue.
    pub async fn insert_batch_at_position(
        &self,
        batch: crate::types::batch::BatchPayload,
        position: usize,
    ) -> Result<()> {
        self.dispatch(SongSource::Batch(batch), QueueVerb::InsertAt(position))
            .await
    }

    // =========================================================================
    // Trawl Mix Methods
    //
    // A trawl crate resolves through `LibraryOrchestrator::resolve_trawl`
    // (per-seed fetch → length/rating filters → sample cap → blend) and then
    // dispatches the pre-blended list as `SongSource::Preloaded` — the blend
    // IS the ordering, so no one-shot shuffle is layered on top.
    // =========================================================================

    /// Resolve the trawl crate and replace the queue with the blended mix.
    pub async fn play_trawl(&self, mix: &crate::types::trawl::TrawlCrate) -> Result<()> {
        let songs = self.library_orchestrator().resolve_trawl(mix).await?;
        self.dispatch_shuffled(
            SongSource::Preloaded(songs),
            QueueVerb::Play(StartPosition::First),
            OneShotShuffle::None,
        )
        .await
    }

    /// Resolve the trawl crate and append the blended mix to the queue.
    /// Returns the number of songs added, for the confirmation toast.
    pub async fn add_trawl_to_queue(&self, mix: &crate::types::trawl::TrawlCrate) -> Result<usize> {
        let songs = self.library_orchestrator().resolve_trawl(mix).await?;
        let count = songs.len();
        self.dispatch(SongSource::Preloaded(songs), QueueVerb::Enqueue)
            .await?;
        Ok(count)
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
        // The engine's *real* transport state — distinct from "the navigator
        // names a current song" (which is true on a freshly-reopened, stopped
        // queue). Snapshotted here, before the mutation, as an independent
        // one-shot engine lock so removing the current row of a stopped/paused
        // app re-cues the engine without starting playback.
        let engine_playing = self.playback.engine_is_playing().await;

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
                        .and_then(|idx| qm.song_id_at(idx).map(str::to_owned))
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
                engine_playing,
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

    /// Multi-selection drag reorder, addressed by per-row `entry_id`.
    /// Resolves entry_ids → current queue positions under the queue lock,
    /// then reorders atomically. See
    /// [`crate::services::queue::QueueManager::move_batch_by_entry_ids`]
    /// for the per-row identity preservation contract; discharges the
    /// resulting `NextTrackResetEffect` against the engine.
    pub async fn move_queue_batch_by_entry_ids(
        &self,
        entry_ids: Vec<u64>,
        target: crate::types::queue::MoveBatchTarget,
    ) -> Result<()> {
        let effect = self
            .queue_service
            .move_batch_by_entry_ids(&entry_ids, target)
            .await?;
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
                    let song_id = qm.song_id_at(idx)?.to_owned();
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
            engine.set_source(String::new(), None).await;
        }
        let effect = self.queue_service.set_queue(Vec::new(), None).await?;
        effect.apply_to(&engine_arc).await;
        Ok(())
    }
}

// =============================================================================
// === Multi-library filter ===
// =============================================================================
impl AppService {
    /// Snapshot the currently-active library (music folder) IDs.
    ///
    /// An empty set is the explicit "all libraries" default — every
    /// `load_*` wrapper should omit the `library_id` filter param in
    /// that case (Navidrome auto-scopes to user-accessible libraries).
    pub fn active_library_ids(&self) -> HashSet<i32> {
        self.active_library_ids.read().clone()
    }

    /// Snapshot the currently-active library IDs as a sorted `Vec<i32>`.
    /// Convenience for callers that want to pass the IDs to a
    /// `library_ids: &[i32]` argument — sorting yields a stable wire
    /// order across calls so cache keys can use the slice directly.
    pub fn active_library_ids_vec(&self) -> Vec<i32> {
        let mut v: Vec<i32> = self.active_library_ids.read().iter().copied().collect();
        v.sort_unstable();
        v
    }

    /// Snapshot the last-seen full list of libraries reported by the
    /// server. Empty until [`refresh_libraries`] succeeds at least
    /// once.
    pub fn all_libraries(&self) -> Vec<Library> {
        self.all_libraries.read().clone()
    }

    /// Number of libraries currently known to the client (the size of
    /// the popover's list). Cheap, avoids cloning the `Vec`.
    pub fn library_count(&self) -> usize {
        self.all_libraries.read().len()
    }

    /// Toggle the membership of a library ID in the active selection,
    /// persisting the new set to redb. Returns the *next* membership
    /// state (`true` == library is now active, `false` == removed).
    ///
    /// Persistence is a best-effort sync write — a failure is logged
    /// at `warn!` and the in-memory state still reflects the toggle so
    /// the UI doesn't appear to swallow the click. The next successful
    /// toggle will re-attempt persistence with the latest snapshot.
    pub fn toggle_library(&self, id: i32) -> bool {
        let (snapshot, now_active) = {
            let mut guard = self.active_library_ids.write();
            let now_active = if guard.contains(&id) {
                guard.remove(&id);
                false
            } else {
                guard.insert(id);
                true
            };
            (guard.clone(), now_active)
        };

        if let Err(e) = self.storage.save_binary(ACTIVE_LIBRARY_IDS_KEY, &snapshot) {
            warn!("[APP SERVICE] failed to persist active_library_ids after toggling {id}: {e}");
        }

        now_active
    }

    /// Replace the active library selection wholesale and persist it.
    /// Used by the "select all" / "clear all" affordances and by the
    /// pruning path inside [`refresh_libraries`]. Mirrors
    /// [`toggle_library`]'s persistence policy (best-effort).
    pub fn set_active_library_ids(&self, ids: HashSet<i32>) {
        {
            let mut guard = self.active_library_ids.write();
            *guard = ids.clone();
        }
        if let Err(e) = self.storage.save_binary(ACTIVE_LIBRARY_IDS_KEY, &ids) {
            warn!("[APP SERVICE] failed to persist active_library_ids set wholesale: {e}");
        }
    }

    /// Refresh the cached library list from the server and prune the
    /// active selection of any IDs no longer present.
    ///
    /// Returns the new `Vec<Library>` so callers can update view state
    /// without re-reading the lock. The pruning step persists the
    /// pruned `active_library_ids` only if at least one ID was actually
    /// removed (avoids redundant redb writes).
    pub async fn refresh_libraries(&self) -> Result<Vec<Library>> {
        let service = self.libraries_api().await?;
        let libraries = service.load().await?;
        self.apply_library_refresh(libraries.clone());
        Ok(libraries)
    }

    /// Apply a refreshed library list to the in-memory caches, pruning
    /// `active_library_ids` of any IDs no longer present and persisting
    /// the pruned set when the delta is non-empty. Factored out of
    /// [`refresh_libraries`] so the pruning logic can be exercised by
    /// unit tests that don't have a live server.
    pub fn apply_library_refresh(&self, libraries: Vec<Library>) {
        // Compute the pruning delta under the lock — keep this hot
        // path tight so we don't hold the write lock across the redb
        // commit below.
        let snapshot_for_persist = {
            let valid_ids: HashSet<i32> = libraries.iter().map(|l| l.id).collect();
            let mut guard = self.active_library_ids.write();
            let before = guard.len();
            guard.retain(|id| valid_ids.contains(id));
            let after = guard.len();
            if after < before {
                Some(guard.clone())
            } else {
                None
            }
        };

        if let Some(pruned) = snapshot_for_persist {
            debug!(
                " [APP SERVICE] pruned active_library_ids to {} entries after server refresh",
                pruned.len()
            );
            if let Err(e) = self.storage.save_binary(ACTIVE_LIBRARY_IDS_KEY, &pruned) {
                warn!("[APP SERVICE] failed to persist pruned active_library_ids: {e}");
            }
        }

        *self.all_libraries.write() = libraries;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::state_storage::StateStorage;

    /// Build a minimal `AppService` for tests. Uses an isolated redb
    /// path under `std::env::temp_dir()` so independent tests don't
    /// collide on the shared file lock. `CustomAudioEngine::new()`
    /// grabs `tokio::runtime::Handle::current()` inside
    /// `AudioRenderer::new()`, which is why every test below is
    /// annotated `#[tokio::test]`.
    async fn test_app() -> (AppService, std::path::PathBuf) {
        // Use a process-and-nanosecond-suffixed db file. `std::env::temp_dir`
        // is shared across the test runner's threads, and two
        // concurrent tests opening the same redb would race the
        // exclusive lock.
        let suffix = format!(
            "test_app_libraries_{}_{}.redb",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        );
        let db_path = std::env::temp_dir().join(suffix);
        let _ = std::fs::remove_file(&db_path);
        let storage = StateStorage::new(db_path.clone()).expect("redb open");
        let app = AppService::new_with_storage(storage)
            .await
            .expect("app service");
        (app, db_path)
    }

    /// Fresh storage means no persisted selection — the active set
    /// starts empty and is interpreted as "no filter / show all".
    #[tokio::test]
    async fn fresh_app_service_has_empty_active_library_ids() {
        let (app, db_path) = test_app().await;
        assert!(
            app.active_library_ids().is_empty(),
            "fresh app must report no active library IDs (== all)"
        );
        assert!(
            app.all_libraries().is_empty(),
            "fresh app must report no cached library list yet"
        );
        drop(app);
        let _ = std::fs::remove_file(&db_path);
    }

    /// `toggle_library(id)` flips membership and reports the new state.
    /// First call adds, second call removes — the round-trip is what
    /// the popover checkbox row binds to.
    #[tokio::test]
    async fn toggle_library_adds_then_removes_membership() {
        let (app, db_path) = test_app().await;

        let after_add = app.toggle_library(7);
        assert!(after_add, "first toggle must report `now_active = true`");
        assert!(app.active_library_ids().contains(&7));

        let after_remove = app.toggle_library(7);
        assert!(
            !after_remove,
            "second toggle must report `now_active = false`"
        );
        assert!(!app.active_library_ids().contains(&7));

        drop(app);
        let _ = std::fs::remove_file(&db_path);
    }

    /// Toggled selection must survive an AppService rebuild — that's
    /// the persistence contract behind the "restart nokkvi, keep the
    /// same libraries selected" UX requirement.
    #[tokio::test]
    async fn toggled_selection_persists_across_rebuild() {
        let suffix = format!(
            "test_app_libraries_persist_{}_{}.redb",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        );
        let db_path = std::env::temp_dir().join(suffix);
        let _ = std::fs::remove_file(&db_path);

        // First boot: toggle two libraries on, then drop everything.
        {
            let storage = StateStorage::new(db_path.clone()).expect("first storage");
            let app = AppService::new_with_storage(storage)
                .await
                .expect("first app");
            assert!(app.toggle_library(1));
            assert!(app.toggle_library(2));
            assert_eq!(app.active_library_ids().len(), 2);
            // Drop forces redb to flush its WAL — the next `new` must
            // see the persisted set.
            drop(app);
        }

        // Second boot: a fresh AppService over the same redb file must
        // restore exactly the two IDs we toggled.
        {
            let storage = StateStorage::new(db_path.clone()).expect("second storage");
            let app = AppService::new_with_storage(storage)
                .await
                .expect("second app");
            let restored = app.active_library_ids();
            assert!(restored.contains(&1));
            assert!(restored.contains(&2));
            assert_eq!(restored.len(), 2);
            drop(app);
        }

        let _ = std::fs::remove_file(&db_path);
    }

    /// `apply_library_refresh` must prune IDs that are no longer
    /// present in the refreshed server list, and the pruned set must
    /// be persisted so the next launch doesn't see the ghost IDs.
    #[tokio::test]
    async fn apply_library_refresh_prunes_missing_ids_and_persists() {
        let suffix = format!(
            "test_app_libraries_prune_{}_{}.redb",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        );
        let db_path = std::env::temp_dir().join(suffix);
        let _ = std::fs::remove_file(&db_path);

        // Boot 1: pre-seed the active set with IDs 1, 2, 3.
        {
            let storage = StateStorage::new(db_path.clone()).expect("boot1 storage");
            let app = AppService::new_with_storage(storage)
                .await
                .expect("boot1 app");
            app.set_active_library_ids([1, 2, 3].into_iter().collect());

            // Refresh with a server list that only contains IDs 1, 2 —
            // ID 3 (the "deleted" library) must be pruned.
            app.apply_library_refresh(vec![
                Library {
                    id: 1,
                    name: "Music".to_string(),
                    song_count: None,
                },
                Library {
                    id: 2,
                    name: "Audiobooks".to_string(),
                    song_count: None,
                },
            ]);

            let active = app.active_library_ids();
            assert!(active.contains(&1));
            assert!(active.contains(&2));
            assert!(
                !active.contains(&3),
                "ID 3 (no longer in server list) must be pruned"
            );
            assert_eq!(active.len(), 2);
            assert_eq!(app.all_libraries().len(), 2);
            drop(app);
        }

        // Boot 2: the pruned set must be persisted.
        {
            let storage = StateStorage::new(db_path.clone()).expect("boot2 storage");
            let app = AppService::new_with_storage(storage)
                .await
                .expect("boot2 app");
            let restored = app.active_library_ids();
            assert!(restored.contains(&1));
            assert!(restored.contains(&2));
            assert!(
                !restored.contains(&3),
                "pruned ID must not resurrect from disk"
            );
            assert_eq!(restored.len(), 2);
            drop(app);
        }

        let _ = std::fs::remove_file(&db_path);
    }

    /// When every active ID is still present in the refreshed list,
    /// `apply_library_refresh` must NOT touch the active set — the
    /// "no-op refresh skips the redb write" optimization.
    #[tokio::test]
    async fn apply_library_refresh_is_no_op_when_no_pruning_needed() {
        let (app, db_path) = test_app().await;
        app.set_active_library_ids([1, 2].into_iter().collect());

        app.apply_library_refresh(vec![
            Library {
                id: 1,
                name: "Music".to_string(),
                song_count: None,
            },
            Library {
                id: 2,
                name: "Audiobooks".to_string(),
                song_count: None,
            },
            Library {
                id: 3,
                name: "Podcasts".to_string(),
                song_count: None,
            },
        ]);

        let active = app.active_library_ids();
        assert!(active.contains(&1));
        assert!(active.contains(&2));
        assert!(
            !active.contains(&3),
            "ID 3 was NOT in the active set before; refresh must not auto-select it"
        );
        assert_eq!(active.len(), 2);
        assert_eq!(app.all_libraries().len(), 3);

        drop(app);
        let _ = std::fs::remove_file(&db_path);
    }

    /// `active_library_ids_vec` must return a stable sorted slice so
    /// callers can use it as a cache key without re-sorting per call.
    #[tokio::test]
    async fn active_library_ids_vec_is_sorted() {
        let (app, db_path) = test_app().await;
        // Insert in non-monotonic order to make sure sorting is the
        // accessor's responsibility, not the underlying HashSet's.
        app.toggle_library(5);
        app.toggle_library(1);
        app.toggle_library(3);

        let v = app.active_library_ids_vec();
        assert_eq!(v, vec![1, 3, 5]);

        drop(app);
        let _ = std::fs::remove_file(&db_path);
    }

    /// `library_count` reflects the cached `all_libraries` length —
    /// the popover hides at N <= 1 (Phase 7 trigger logic) so this is
    /// the source of truth for the Phase-7 gate too.
    #[tokio::test]
    async fn library_count_reflects_apply_refresh_payload() {
        let (app, db_path) = test_app().await;
        assert_eq!(app.library_count(), 0);

        app.apply_library_refresh(vec![Library {
            id: 1,
            name: "Music".to_string(),
            song_count: None,
        }]);
        assert_eq!(app.library_count(), 1);

        app.apply_library_refresh(vec![
            Library {
                id: 1,
                name: "Music".to_string(),
                song_count: None,
            },
            Library {
                id: 2,
                name: "Audiobooks".to_string(),
                song_count: None,
            },
        ]);
        assert_eq!(app.library_count(), 2);

        drop(app);
        let _ = std::fs::remove_file(&db_path);
    }

    /// `set_active_library_ids` replaces the selection wholesale and
    /// persists the new set immediately. Two consecutive calls must
    /// honor the second call's contents — no leak from the first.
    #[tokio::test]
    async fn set_active_library_ids_replaces_wholesale_and_persists() {
        let suffix = format!(
            "test_app_libraries_set_{}_{}.redb",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        );
        let db_path = std::env::temp_dir().join(suffix);
        let _ = std::fs::remove_file(&db_path);

        // Boot 1: assign {1, 2, 3}, then wholesale-replace with {4, 5}.
        {
            let storage = StateStorage::new(db_path.clone()).expect("boot1 storage");
            let app = AppService::new_with_storage(storage)
                .await
                .expect("boot1 app");
            app.set_active_library_ids([1, 2, 3].into_iter().collect());
            app.set_active_library_ids([4, 5].into_iter().collect());

            let active = app.active_library_ids();
            assert_eq!(active.len(), 2);
            assert!(active.contains(&4));
            assert!(active.contains(&5));
            assert!(!active.contains(&1));
            drop(app);
        }

        // Boot 2: confirms the wholesale-replace was the persisted shape.
        {
            let storage = StateStorage::new(db_path.clone()).expect("boot2 storage");
            let app = AppService::new_with_storage(storage)
                .await
                .expect("boot2 app");
            let restored = app.active_library_ids();
            assert_eq!(restored.len(), 2);
            assert!(restored.contains(&4));
            assert!(restored.contains(&5));
            drop(app);
        }

        let _ = std::fs::remove_file(&db_path);
    }

    // =========================================================================
    // dispatch (source-verb entry point) tests. Non-empty Play /
    // EnqueueAndPlay inputs are deliberately avoided — those verbs reach the
    // engine-load path (mirrors queue_orchestrator's compile-only smoke
    // convention for playback-touching verbs).
    // =========================================================================

    fn dispatch_test_songs(ids: &[&str]) -> Vec<crate::types::song::Song> {
        ids.iter()
            .map(|id| crate::types::song::Song::test_default(id, &format!("Song {id}")))
            .collect()
    }

    fn dispatch_queue_ids(app: &AppService) -> Vec<String> {
        app.queue_service
            .get_songs()
            .iter()
            .map(|s| s.id.clone())
            .collect()
    }

    /// An empty `Preloaded` source must hit the single shared empty guard
    /// with the Preloaded message — for every non-playback-reaching verb.
    #[tokio::test]
    async fn dispatch_empty_preloaded_errors_no_songs_to_play() {
        let (app, db_path) = test_app().await;

        for verb in [
            QueueVerb::Enqueue,
            QueueVerb::InsertAt(1),
            QueueVerb::PlayNext,
        ] {
            let err = app
                .dispatch(SongSource::Preloaded(Vec::new()), verb)
                .await
                .expect_err("empty preloaded dispatch must error");
            assert!(
                err.to_string().contains("No songs to play"),
                "expected 'No songs to play' for {verb:?}, got: {err}"
            );
        }

        drop(app);
        let _ = std::fs::remove_file(&db_path);
    }

    /// `dispatch(Preloaded, Enqueue)` appends to the live queue.
    #[tokio::test]
    async fn dispatch_preloaded_enqueue_appends() {
        let (app, db_path) = test_app().await;

        app.dispatch(
            SongSource::Preloaded(dispatch_test_songs(&["a", "b"])),
            QueueVerb::Enqueue,
        )
        .await
        .expect("enqueue dispatch");

        assert_eq!(dispatch_queue_ids(&app), vec!["a", "b"]);

        drop(app);
        let _ = std::fs::remove_file(&db_path);
    }

    /// `dispatch(Preloaded, InsertAt(1))` splices at the requested position.
    #[tokio::test]
    async fn dispatch_preloaded_insert_at_splices_in_order() {
        let (app, db_path) = test_app().await;

        app.dispatch(
            SongSource::Preloaded(dispatch_test_songs(&["a", "b"])),
            QueueVerb::Enqueue,
        )
        .await
        .expect("seed enqueue");
        app.dispatch(
            SongSource::Preloaded(dispatch_test_songs(&["x", "y"])),
            QueueVerb::InsertAt(1),
        )
        .await
        .expect("insert dispatch");

        assert_eq!(dispatch_queue_ids(&app), vec!["a", "x", "y", "b"]);

        drop(app);
        let _ = std::fs::remove_file(&db_path);
    }
}
