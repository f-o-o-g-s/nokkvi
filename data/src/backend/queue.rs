//! Queue service — playback queue state and UI projection
//!
//! `QueueService` wraps `QueueManager` with reactive properties and transforms
//! raw `Song` data into `QueueSongUIViewData` for the view layer.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;

use crate::{
    backend::auth::AuthGateway,
    services::queue::QueueManager,
    types::{
        reactive::{ReactiveInt, ReactiveVecProperty},
        song::Song,
    },
    utils::artwork_url,
};

/// UI-specific view data for queue songs
/// UI-projected data
#[derive(Debug, Clone)]
pub struct QueueSongUIViewData {
    pub id: String,
    pub track_number: i32,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_id: String,
    pub artwork_url: String,
    pub duration: String,
    pub duration_seconds: u32, // For sorting
    pub genre: String,         // For sorting/display
    pub starred: bool,
    pub rating: Option<u32>,
}

impl crate::backend::Starable for QueueSongUIViewData {
    fn entity_id(&self) -> &str {
        &self.id
    }
    fn set_starred(&mut self, starred: bool) {
        self.starred = starred;
    }
    fn display_label(&self) -> String {
        format!("{} - {}", self.title, self.artist)
    }
}

impl crate::backend::Ratable for QueueSongUIViewData {
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

impl crate::utils::search::Searchable for QueueSongUIViewData {
    fn searchable_fields(&self) -> Vec<&str> {
        // Match QML implementation: search across title, artist, album, and genre
        vec![&self.title, &self.artist, &self.album, &self.genre]
    }
}

#[derive(Clone)]
pub struct QueueService {
    // Queue manager for playback queue state
    queue_manager: Arc<Mutex<QueueManager>>,

    // Reactive properties for Views to bind to
    pub songs: ReactiveVecProperty<QueueSongUIViewData>,
    pub current_index: ReactiveInt,
    pub total_count: ReactiveInt,

    // Dependencies
    auth_gateway: Arc<Mutex<Option<AuthGateway>>>,
}

impl QueueService {
    pub fn new(
        auth_gateway: AuthGateway,
        storage: crate::services::state_storage::StateStorage,
    ) -> Result<Self> {
        let queue_manager = QueueManager::new(storage)?;
        Ok(Self {
            queue_manager: Arc::new(Mutex::new(queue_manager)),
            songs: ReactiveVecProperty::new(),
            current_index: ReactiveInt::new(0),
            total_count: ReactiveInt::new(0),
            auth_gateway: Arc::new(Mutex::new(Some(auth_gateway))),
        })
    }

    /// Initialize the viewmodel by loading persisted queue data into reactive properties
    pub async fn initialize(&self) -> Result<()> {
        // Get persisted queue data
        let queue_manager = self.queue_manager.lock().await;

        // Get server config for UI transformation
        let (server_url, subsonic_credential) = self.get_server_config().await;

        // Transform songs for UI directly from pool (no intermediate clone)
        let ui_data =
            Self::transform_songs_from_pool(&queue_manager, &server_url, &subsonic_credential);
        let song_count = ui_data.len();
        self.songs.set(ui_data);

        // Set current index
        let current_index = queue_manager.get_queue().current_index.unwrap_or(0);
        self.current_index.set(current_index as i32);

        // Set total count
        self.total_count.set(song_count as i32);

        Ok(())
    }

    /// Get songs UI data (reactive property)
    pub fn get_songs(&self) -> Vec<QueueSongUIViewData> {
        self.songs.get()
    }

    /// Add songs to the queue
    pub async fn add_songs(&self, songs: Vec<Song>) -> Result<()> {
        {
            let mut queue_manager = self.queue_manager.lock().await;
            queue_manager.add_songs(songs)?;
        }

        // Update reactive properties (ViewModel responsibility)
        let (server_url, subsonic_credential) = self.get_server_config().await;
        let queue_manager = self.queue_manager.lock().await;
        let ui_data =
            Self::transform_songs_from_pool(&queue_manager, &server_url, &subsonic_credential);
        self.songs.set(ui_data);
        Ok(())
    }

    /// Set the queue (replace all songs)
    pub async fn set_queue(&self, songs: Vec<Song>, current_index: Option<usize>) -> Result<()> {
        {
            let mut queue_manager = self.queue_manager.lock().await;
            queue_manager.set_queue(songs, current_index)?;
        }

        // Update reactive properties (ViewModel responsibility)
        let (server_url, subsonic_credential) = self.get_server_config().await;
        let queue_manager = self.queue_manager.lock().await;
        let ui_data =
            Self::transform_songs_from_pool(&queue_manager, &server_url, &subsonic_credential);
        let song_count = ui_data.len();
        self.songs.set(ui_data);
        self.current_index.set(current_index.unwrap_or(0) as i32);
        self.total_count.set(song_count as i32);

        Ok(())
    }

    /// Helper to get server URL and credential
    pub async fn get_server_config(&self) -> (String, String) {
        let auth_guard = self.auth_gateway.lock().await;
        if let Some(auth) = auth_guard.as_ref() {
            (
                auth.get_server_url().await,
                auth.get_subsonic_credential().await,
            )
        } else {
            (String::new(), String::new())
        }
    }

    /// Transform songs for UI directly from pool references (no intermediate clone).
    ///
    /// Iterates the queue's ordered IDs, looks up each song from the pool,
    /// and builds `QueueSongUIViewData` from the `&Song` reference.
    fn transform_songs_from_pool(
        qm: &QueueManager,
        server_url: &str,
        subsonic_credential: &str,
    ) -> Vec<QueueSongUIViewData> {
        qm.get_queue()
            .song_ids
            .iter()
            .enumerate()
            .filter_map(|(index, id)| {
                let song = qm.get_song(id)?;
                let track_number = (index + 1) as i32;
                let album_id = song.album_id.clone().unwrap_or_default();
                let cover_art_id = song.cover_art.as_deref().unwrap_or(&album_id);
                let url = artwork_url::build_cover_art_url(
                    cover_art_id,
                    server_url,
                    subsonic_credential,
                    Some(artwork_url::THUMBNAIL_SIZE),
                );
                let duration_str = crate::utils::formatters::format_duration(song.duration);
                let genre = song.genre.clone().unwrap_or_default();

                Some(QueueSongUIViewData {
                    id: song.id.clone(),
                    track_number,
                    title: song.title.clone(),
                    artist: song.artist.clone(),
                    album: song.album.clone(),
                    album_id,
                    artwork_url: url,
                    duration: duration_str,
                    duration_seconds: song.duration,
                    genre,
                    starred: song.starred,
                    rating: song.rating,
                })
            })
            .collect()
    }

    /// Get reference to QueueManager (for PlaybackController)
    pub fn queue_manager(&self) -> Arc<Mutex<QueueManager>> {
        self.queue_manager.clone()
    }

    /// Move a queue item from one position to another (drag-and-drop reorder)
    pub async fn move_item(&self, from: usize, to: usize) -> Result<()> {
        let mut qm = self.queue_manager.lock().await;
        qm.move_item(from, to)?;
        Ok(())
    }

    /// Remove a song from the queue by index
    pub async fn remove_song(&self, index: usize) -> Result<()> {
        let mut qm = self.queue_manager.lock().await;
        qm.remove_song(index)?;
        Ok(())
    }

    /// Move a song to the top of the queue
    pub async fn move_to_top(&self, index: usize) -> Result<()> {
        let mut qm = self.queue_manager.lock().await;
        qm.move_to_top(index)?;
        Ok(())
    }

    /// Move a song to the bottom of the queue
    pub async fn move_to_bottom(&self, index: usize) -> Result<()> {
        let mut qm = self.queue_manager.lock().await;
        qm.move_to_bottom(index)?;
        Ok(())
    }

    /// Insert songs at a specific position in the queue (cross-pane drag drop)
    pub async fn insert_songs_at(&self, index: usize, songs: Vec<Song>) -> Result<()> {
        {
            let mut queue_manager = self.queue_manager.lock().await;
            queue_manager.insert_songs_at(index, songs)?;
        }

        // Update reactive properties (ViewModel responsibility)
        let (server_url, subsonic_credential) = self.get_server_config().await;
        let queue_manager = self.queue_manager.lock().await;
        let ui_data =
            Self::transform_songs_from_pool(&queue_manager, &server_url, &subsonic_credential);
        self.songs.set(ui_data);
        Ok(())
    }

    /// Get the current playing index
    pub async fn current_index(&self) -> Option<usize> {
        let qm = self.queue_manager.lock().await;
        qm.get_queue().current_index
    }

    /// Refresh reactive properties from current queue state
    /// This is needed when the queue is modified externally (e.g., consume mode)
    pub async fn refresh_from_queue(&self) -> Result<()> {
        let queue_manager = self.queue_manager.lock().await;
        let queue = queue_manager.get_queue();

        // Update reactive properties from pool (no intermediate clone)
        let (server_url, subsonic_credential) = self.get_server_config().await;
        let ui_data =
            Self::transform_songs_from_pool(&queue_manager, &server_url, &subsonic_credential);
        let song_count = ui_data.len();
        self.songs.set(ui_data);

        let current_index = queue.current_index.unwrap_or(0) as i32;
        self.current_index.set(current_index);
        self.total_count.set(song_count as i32);

        tracing::trace!(
            "🔄 QueueService refreshed: {} songs, current_index: {}",
            song_count,
            current_index
        );

        Ok(())
    }
}
