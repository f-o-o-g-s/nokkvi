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
    pub artist_id: String,
    pub album: String,
    pub album_id: String,
    pub artwork_url: String,
    pub duration: String,
    pub duration_seconds: u32, // For sorting
    pub genre: String,         // For sorting/display
    pub starred: bool,
    pub rating: Option<u32>,
    pub play_count: Option<u32>, // For Most Played sort
    /// Pre-lowercased search index — see `crate::utils::search::Searchable`.
    pub searchable_lower: String,
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

impl crate::backend::PlayCountable for QueueSongUIViewData {
    fn entity_id(&self) -> &str {
        &self.id
    }
    fn play_count(&self) -> Option<u32> {
        self.play_count
    }
    fn set_play_count(&mut self, count: Option<u32>) {
        self.play_count = count;
    }
    fn display_label(&self) -> String {
        format!("{} - {}", self.title, self.artist)
    }
}

impl crate::utils::search::Searchable for QueueSongUIViewData {
    fn matches_query(&self, query_lower: &str) -> bool {
        self.searchable_lower.contains(query_lower)
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
        let queue_manager = self.queue_manager.lock().await;
        self.refresh_from_locked_manager(&queue_manager).await
    }

    /// Get songs UI data (reactive property)
    pub fn get_songs(&self) -> Vec<QueueSongUIViewData> {
        self.songs.get()
    }

    /// Add songs to the queue
    pub async fn add_songs(&self, songs: Vec<Song>) -> Result<()> {
        let mut queue_manager = self.queue_manager.lock().await;
        queue_manager.add_songs(songs)?;
        self.refresh_from_locked_manager(&queue_manager).await
    }

    /// Set the queue (replace all songs)
    pub async fn set_queue(&self, songs: Vec<Song>, current_index: Option<usize>) -> Result<()> {
        let mut queue_manager = self.queue_manager.lock().await;
        queue_manager.set_queue(songs, current_index)?;
        self.refresh_from_locked_manager(&queue_manager).await
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
                // Always use album_id for the artwork URL, NOT song.cover_art.
                // The Subsonic API returns cover_art as "mf-{mediafile_id}" for
                // playlist songs, but the background prefetch caches thumbnails
                // under "al-{album_id}_80". Using cover_art here creates a cache
                // key mismatch, causing every playlist song to miss the disk cache
                // and trigger a network fetch — overwhelming the connection and
                // leaving ~90% of thumbnails blank.
                let url = artwork_url::build_cover_art_url(
                    &album_id,
                    server_url,
                    subsonic_credential,
                    Some(artwork_url::THUMBNAIL_SIZE),
                );
                let duration_str = crate::utils::formatters::format_duration(song.duration);
                let genre = song.genre.clone().unwrap_or_default();
                let searchable_lower = crate::utils::search::build_searchable_lower(&[
                    &song.title,
                    &song.artist,
                    &song.album,
                    &genre,
                ]);

                Some(QueueSongUIViewData {
                    id: song.id.clone(),
                    track_number,
                    title: song.title.clone(),
                    artist: song.artist.clone(),
                    artist_id: song.artist_id.clone().unwrap_or_default(),
                    album: song.album.clone(),
                    album_id,
                    artwork_url: url,
                    duration: duration_str,
                    duration_seconds: song.duration,
                    genre,
                    starred: song.starred,
                    rating: song.rating,
                    play_count: song.play_count,
                    searchable_lower,
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
        self.refresh_from_locked_manager(&qm).await
    }

    /// Remove a song from the queue by index
    pub async fn remove_song(&self, index: usize) -> Result<()> {
        let mut qm = self.queue_manager.lock().await;
        qm.remove_song(index)?;
        self.refresh_from_locked_manager(&qm).await
    }

    /// Remove a batch of songs from the queue by ID. Index-immune — pass the
    /// song identifiers rather than positions so optimistic UI mutations,
    /// client-side sorts, or concurrent backend changes can't desync targets.
    pub async fn remove_songs_by_ids(&self, ids: &[String]) -> Result<()> {
        let mut qm = self.queue_manager.lock().await;
        qm.remove_songs_by_ids(ids)?;
        self.refresh_from_locked_manager(&qm).await
    }

    /// Insert songs at a specific position in the queue (cross-pane drag drop)
    pub async fn insert_songs_at(&self, index: usize, songs: Vec<Song>) -> Result<()> {
        let mut queue_manager = self.queue_manager.lock().await;
        queue_manager.insert_songs_at(index, songs)?;
        self.refresh_from_locked_manager(&queue_manager).await
    }

    /// Get the current playing index
    pub async fn current_index(&self) -> Option<usize> {
        let qm = self.queue_manager.lock().await;
        qm.get_queue().current_index
    }

    /// Refresh reactive properties from current queue state.
    ///
    /// Used when the queue is mutated through `QueueManager` directly (e.g.,
    /// consume mode advancing the playhead) without going through one of the
    /// `QueueService` mutators. Acquires the queue lock and delegates to the
    /// canonical projection step.
    pub async fn refresh_from_queue(&self) -> Result<()> {
        let queue_manager = self.queue_manager.lock().await;
        self.refresh_from_locked_manager(&queue_manager).await
    }

    /// Canonical projection step: read queue state and set all three
    /// reactives (`songs`, `current_index`, `total_count`) atomically given
    /// an already-held queue lock.
    ///
    /// All six mutators (`set_queue`, `add_songs`, `insert_songs_at`,
    /// `move_item`, `remove_song`, `remove_songs_by_ids`) — plus
    /// `refresh_from_queue` for direct projection — acquire the queue lock
    /// once, mutate (or skip mutation), then call this method to project,
    /// so a concurrent task cannot mutate the queue between the mutation
    /// and the projection. Closes the 1-tick projection race the previous
    /// unlock-then-relock pattern allowed.
    ///
    /// Lock order: caller already holds `queue_manager`; this method then
    /// acquires `auth_gateway` via `get_server_config()`. The same order is
    /// established codebase-wide — do not invert.
    async fn refresh_from_locked_manager(&self, qm: &QueueManager) -> Result<()> {
        let (server_url, subsonic_credential) = self.get_server_config().await;
        let ui_data = Self::transform_songs_from_pool(qm, &server_url, &subsonic_credential);
        let song_count = ui_data.len();
        self.songs.set(ui_data);

        let current_index = qm.get_queue().current_index.unwrap_or(0) as i32;
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

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::{
        backend::auth::AuthGateway, services::state_storage::StateStorage, types::song::Song,
    };

    /// Construct a `QueueService` backed by tempfile storage and an empty
    /// `AuthGateway`. Mirrors the fixture in `queue_orchestrator.rs::tests`.
    fn make_service() -> (QueueService, TempDir) {
        let temp = TempDir::new().expect("temp dir");
        let storage = StateStorage::new(temp.path().join("queue.redb")).expect("storage");
        let auth = AuthGateway::new().expect("auth gateway");
        let service = QueueService::new(auth, storage).expect("queue service");
        (service, temp)
    }

    fn make_songs(ids: &[&str]) -> Vec<Song> {
        ids.iter()
            .map(|id| Song::test_default(id, &format!("Song {id}")))
            .collect()
    }

    /// `add_songs` must update `total_count` to match the new queue length,
    /// not leave it stale until the next full refresh.
    #[tokio::test(flavor = "current_thread")]
    async fn add_songs_updates_total_count() {
        let (service, _temp) = make_service();
        service
            .set_queue(make_songs(&["a", "b"]), Some(0))
            .await
            .expect("set_queue");
        assert_eq!(service.total_count.get(), 2);

        service
            .add_songs(make_songs(&["c", "d", "e"]))
            .await
            .expect("add_songs");

        assert_eq!(
            service.total_count.get(),
            5,
            "add_songs must update total_count to match the new queue length"
        );
        assert_eq!(service.songs.get().len(), 5);
    }

    /// `add_songs` must not disturb `current_index` — appended songs land
    /// after the playhead, so the playing position is unchanged.
    #[tokio::test(flavor = "current_thread")]
    async fn add_songs_preserves_current_index() {
        let (service, _temp) = make_service();
        service
            .set_queue(make_songs(&["a", "b", "c", "d"]), Some(2))
            .await
            .expect("set_queue");
        assert_eq!(service.current_index.get(), 2);

        service
            .add_songs(make_songs(&["x", "y"]))
            .await
            .expect("add_songs");

        assert_eq!(
            service.current_index.get(),
            2,
            "add_songs must not shift current_index"
        );
    }

    /// `insert_songs_at` must update `total_count` to match the new queue length.
    #[tokio::test(flavor = "current_thread")]
    async fn insert_songs_at_updates_total_count() {
        let (service, _temp) = make_service();
        service
            .set_queue(make_songs(&["a", "b", "c"]), Some(0))
            .await
            .expect("set_queue");
        assert_eq!(service.total_count.get(), 3);

        service
            .insert_songs_at(1, make_songs(&["x", "y"]))
            .await
            .expect("insert_songs_at");

        assert_eq!(
            service.total_count.get(),
            5,
            "insert_songs_at must update total_count to match the new queue length"
        );
        assert_eq!(service.songs.get().len(), 5);
    }

    /// `insert_songs_at` before the playhead must shift `current_index` forward
    /// to keep tracking the same playing song — mirroring
    /// `QueueManager::insert_songs_at`'s contract.
    #[tokio::test(flavor = "current_thread")]
    async fn insert_songs_at_before_current_shifts_index() {
        let (service, _temp) = make_service();
        service
            .set_queue(make_songs(&["a", "b", "c", "d"]), Some(3))
            .await
            .expect("set_queue");
        assert_eq!(service.current_index.get(), 3);

        service
            .insert_songs_at(1, make_songs(&["x", "y"]))
            .await
            .expect("insert_songs_at");

        assert_eq!(
            service.current_index.get(),
            5,
            "inserting before the playhead must shift current_index forward by the insert count"
        );
    }

    /// `insert_songs_at` after the playhead must leave `current_index` alone.
    #[tokio::test(flavor = "current_thread")]
    async fn insert_songs_at_after_current_preserves_index() {
        let (service, _temp) = make_service();
        service
            .set_queue(make_songs(&["a", "b", "c"]), Some(1))
            .await
            .expect("set_queue");
        assert_eq!(service.current_index.get(), 1);

        service
            .insert_songs_at(3, make_songs(&["x"]))
            .await
            .expect("insert_songs_at");

        assert_eq!(
            service.current_index.get(),
            1,
            "inserting after the playhead must not shift current_index"
        );
    }

    /// `set_queue` continues to set all three reactives atomically (regression
    /// guard for the projection-after-mutation refactor).
    #[tokio::test(flavor = "current_thread")]
    async fn set_queue_sets_all_three_reactives() {
        let (service, _temp) = make_service();
        service
            .set_queue(make_songs(&["a", "b", "c"]), Some(1))
            .await
            .expect("set_queue");

        assert_eq!(service.songs.get().len(), 3);
        assert_eq!(service.current_index.get(), 1);
        assert_eq!(service.total_count.get(), 3);
    }

    /// `refresh_from_queue` continues to set all three reactives atomically
    /// (regression guard — `refresh_from_queue` becomes a thin wrapper around
    /// the canonical projection step).
    #[tokio::test(flavor = "current_thread")]
    async fn refresh_from_queue_sets_all_three_reactives() {
        let (service, _temp) = make_service();
        service
            .set_queue(make_songs(&["a", "b"]), Some(0))
            .await
            .expect("set_queue");

        // Sanity: refresh after a mutation leaves the projection consistent.
        service.refresh_from_queue().await.expect("refresh");

        assert_eq!(service.songs.get().len(), 2);
        assert_eq!(service.current_index.get(), 0);
        assert_eq!(service.total_count.get(), 2);
    }

    /// `remove_song` must project all three reactives atomically — closing
    /// the gap left after Tier 0 #0.4 fixed only the append/insert/set
    /// mutators. Removing before the playhead shifts `current_index` back to
    /// keep tracking the playing song, per `QueueManager::remove_song`'s
    /// contract.
    #[tokio::test(flavor = "current_thread")]
    async fn remove_song_projects_reactives_after_mutation() {
        let (service, _temp) = make_service();
        service
            .set_queue(make_songs(&["a", "b", "c", "d"]), Some(2))
            .await
            .expect("set_queue");
        assert_eq!(service.total_count.get(), 4);
        assert_eq!(service.current_index.get(), 2);

        service.remove_song(0).await.expect("remove_song");

        assert_eq!(
            service.total_count.get(),
            3,
            "remove_song must update total_count to reflect the shorter queue"
        );
        assert_eq!(
            service.current_index.get(),
            1,
            "removing before the playhead must shift current_index back by one"
        );
        assert_eq!(service.songs.get().len(), 3);
    }

    /// `remove_songs_by_ids` must project all three reactives atomically.
    /// Removing two ids before the playhead shifts `current_index` back by
    /// the count actually removed.
    #[tokio::test(flavor = "current_thread")]
    async fn remove_songs_by_ids_projects_reactives_after_mutation() {
        let (service, _temp) = make_service();
        service
            .set_queue(make_songs(&["a", "b", "c", "d", "e"]), Some(3))
            .await
            .expect("set_queue");
        assert_eq!(service.total_count.get(), 5);
        assert_eq!(service.current_index.get(), 3);

        service
            .remove_songs_by_ids(&["a".to_string(), "b".to_string()])
            .await
            .expect("remove_songs_by_ids");

        assert_eq!(
            service.total_count.get(),
            3,
            "remove_songs_by_ids must update total_count to reflect the batch removal"
        );
        assert_eq!(
            service.current_index.get(),
            1,
            "removing two ids before the playhead must shift current_index back by two"
        );
        assert_eq!(service.songs.get().len(), 3);
    }

    /// `move_item` must project all three reactives atomically. When the
    /// playing song itself is moved, `current_index` follows it to the new
    /// position, per `QueueManager::move_item`'s contract.
    #[tokio::test(flavor = "current_thread")]
    async fn move_item_projects_reactives_after_mutation() {
        let (service, _temp) = make_service();
        service
            .set_queue(make_songs(&["a", "b", "c", "d"]), Some(0))
            .await
            .expect("set_queue");
        assert_eq!(service.total_count.get(), 4);
        assert_eq!(service.current_index.get(), 0);

        // Move the playing song (at index 0) to the back. `move_item`
        // computes `insert_at = to - 1` when moving forward, so the playing
        // song lands at index 2 (in a 4-song queue, `to = 3` → `insert_at = 2`).
        service.move_item(0, 3).await.expect("move_item");

        assert_eq!(
            service.total_count.get(),
            4,
            "move_item must not change total_count"
        );
        assert_eq!(
            service.current_index.get(),
            2,
            "moving the playing song must update current_index to its new position"
        );
        assert_eq!(service.songs.get().len(), 4);
    }
}
