//! Queue management hotkey handlers

use iced::Task;
use tracing::{debug, error, info};

use crate::{Nokkvi, View, app_message::Message};

impl Nokkvi {
    pub(crate) fn handle_add_to_queue(&mut self) -> Task<Message> {
        debug!(" AddToQueue (Shift+A) hotkey pressed");
        if let Some(msg) = self
            .current_view_page()
            .and_then(|p| p.add_to_queue_message())
        {
            return Task::done(msg);
        }
        self.toast_warn("No item selected");
        Task::none()
    }

    pub(crate) fn handle_remove_from_queue(&mut self) -> Task<Message> {
        debug!("  RemoveFromQueue (Ctrl+D) hotkey pressed");

        // Only works in Queue view
        if self.current_view != View::Queue {
            self.toast_warn("Not in queue view");
            return Task::none();
        }

        // Use filtered queue since the slot list indices are relative to the filtered list
        let filtered_queue = self.filter_queue_songs();

        // Get the center item from the FILTERED list
        let center_idx = match self
            .queue_page
            .common
            .slot_list
            .get_center_item_index(filtered_queue.len())
        {
            Some(idx) => idx,
            None => {
                debug!("No center item to remove");
                self.toast_warn("No song selected");
                return Task::none();
            }
        };

        // Look up the song from the filtered list, then find its position in the
        // unfiltered queue for the actual removal
        let song = match filtered_queue.get(center_idx) {
            Some(s) => s,
            None => return Task::none(),
        };
        let song_id = song.id.clone();
        debug!(
            "Removing: {} - {} (filtered index {})",
            song.title, song.artist, center_idx
        );

        let unfiltered_idx = match self
            .library
            .queue_songs
            .iter()
            .position(|s| s.id == song_id)
        {
            Some(idx) => idx,
            None => {
                error!("Song {} not found in unfiltered queue", song_id);
                return Task::none();
            }
        };

        let song_title = song.title.clone();
        self.toast_success(format!("Removed '{song_title}' from queue"));
        self.shell_task(
            move |shell| async move {
                let queue_vm = shell.queue().clone();
                let binding = queue_vm.queue_manager();
                let mut qm = binding.lock().await;
                qm.remove_song(unfiltered_idx)?;
                Ok::<_, anyhow::Error>(())
            },
            |result| match result {
                Ok(()) => {
                    info!(" Item removed from queue");
                    Message::LoadQueue
                }
                Err(e) => {
                    error!(" Failed to remove from queue: {}", e);
                    Message::Toast(crate::app_message::ToastMessage::Push(
                        nokkvi_data::types::toast::Toast::new(
                            format!("Failed to remove from queue: {e}"),
                            nokkvi_data::types::toast::ToastLevel::Error,
                        ),
                    ))
                }
            },
        )
    }

    pub(crate) fn handle_clear_queue(&mut self) -> Task<Message> {
        debug!("  ClearQueue (Shift+D) hotkey pressed");

        // Only works in Queue view
        if self.current_view != View::Queue {
            self.toast_warn("Not in queue view");
            return Task::none();
        }

        // Reset visualizer to clear bars (same as stop)
        if let Some(ref viz) = self.visualizer {
            viz.reset();
        }

        // Clear playlist context bar (same pattern as guard_play_action)
        self.active_playlist_info = None;
        self.persist_active_playlist_info();

        self.toast_success("Queue cleared");
        self.shell_task(
            move |shell| async move {
                let queue_vm = shell.queue().clone();
                let audio_engine = shell.audio_engine().clone();
                // 1. Stop playback and clear source
                let mut engine = audio_engine.lock().await;
                let _ = engine.stop().await;
                engine.set_source(String::new()).await; // Clear source to prevent resuming
                drop(engine);

                // 2. Clear the queue
                queue_vm.set_queue(Vec::new(), None).await?;

                Ok::<_, anyhow::Error>(())
            },
            |result| match result {
                Ok(()) => {
                    info!(" Queue cleared successfully");
                    Message::LoadQueue
                }
                Err(e) => {
                    error!(" Failed to clear queue: {}", e);
                    Message::Toast(crate::app_message::ToastMessage::Push(
                        nokkvi_data::types::toast::Toast::new(
                            format!("Failed to clear queue: {e}"),
                            nokkvi_data::types::toast::ToastLevel::Error,
                        ),
                    ))
                }
            },
        )
    }

    pub(crate) fn handle_shuffle_queue(&mut self) -> Task<Message> {
        debug!(" ShuffleQueue button clicked");

        self.toast_success("Queue shuffled");
        self.shell_task(
            move |shell| async move {
                let queue_vm = shell.queue().clone();
                // Call shuffle_queue on the queue manager
                let binding = queue_vm.queue_manager();
                let mut qm = binding.lock().await;
                qm.shuffle_queue()?;
                Ok::<_, anyhow::Error>(())
            },
            |result| match result {
                Ok(()) => {
                    info!(" Queue shuffled successfully");
                    Message::LoadQueue
                }
                Err(e) => {
                    error!(" Failed to shuffle queue: {}", e);
                    Message::Toast(crate::app_message::ToastMessage::Push(
                        nokkvi_data::types::toast::Toast::new(
                            format!("Failed to shuffle queue: {e}"),
                            nokkvi_data::types::toast::ToastLevel::Error,
                        ),
                    ))
                }
            },
        )
    }

    pub(crate) fn handle_save_queue_as_playlist(&mut self) -> Task<Message> {
        debug!(" SaveQueueAsPlaylist (Ctrl+S) hotkey pressed");

        if self.library.queue_songs.is_empty() {
            self.toast_warn("Queue is empty");
            return Task::none();
        }

        // Fetch all playlists from server before opening the dialog
        self.shell_task(
            |shell| async move {
                let service = shell.playlists_api().await?;
                let (playlists, _) = service.load_playlists("name", "ASC", None).await?;
                Ok(playlists
                    .into_iter()
                    .map(|p| (p.id, p.name))
                    .collect::<Vec<_>>())
            },
            |result: Result<Vec<(String, String)>, anyhow::Error>| match result {
                Ok(playlists) => Message::PlaylistsFetchedForDialog(playlists),
                Err(e) => {
                    tracing::error!("Failed to fetch playlists for dialog: {e}");
                    Message::Toast(crate::app_message::ToastMessage::Push(
                        nokkvi_data::types::toast::Toast::new(
                            format!("Failed to load playlists: {e}"),
                            nokkvi_data::types::toast::ToastLevel::Error,
                        ),
                    ))
                }
            },
        )
    }

    /// Fetch playlists from server and open the "Add to Playlist" dialog.
    ///
    /// Used by views when song IDs are already resolved (e.g., single song).
    pub(crate) fn fetch_playlists_for_add_to_playlist(
        &mut self,
        song_ids: Vec<String>,
    ) -> Task<Message> {
        self.shell_task(
            |shell| async move {
                let service = shell.playlists_api().await?;
                let (playlists, _) = service.load_playlists("name", "ASC", None).await?;
                Ok(playlists
                    .into_iter()
                    .map(|p| (p.id, p.name))
                    .collect::<Vec<_>>())
            },
            move |result: Result<Vec<(String, String)>, anyhow::Error>| match result {
                Ok(playlists) => Message::PlaylistsFetchedForAddToPlaylist(playlists, song_ids),
                Err(e) => {
                    tracing::error!("Failed to fetch playlists for add to playlist dialog: {e}");
                    Message::Toast(crate::app_message::ToastMessage::Push(
                        nokkvi_data::types::toast::Toast::new(
                            format!("Failed to load playlists: {e}"),
                            nokkvi_data::types::toast::ToastLevel::Error,
                        ),
                    ))
                }
            },
        )
    }

    /// Handle Shift+↑ / Shift+↓: move the centered queue track up or down.
    /// Shares the same preconditions as drag reorder: Queue view,
    /// no active search. Reuses the `QueueAction::MoveItem` backend persist path.
    pub(crate) fn handle_move_track(&mut self, up: bool) -> Task<Message> {
        let direction = if up { "up" } else { "down" };
        debug!("📦 MoveTrack {} hotkey pressed", direction);

        // Guard: queue view only
        if self.current_view != View::Queue {
            return Task::none();
        }

        // Guard: no active search
        if !self.queue_page.common.search_query.is_empty() {
            self.toast_info("Clear search to reorder queue");
            return Task::none();
        }

        let queue_len = self.library.queue_songs.len();
        let center_idx = match self
            .queue_page
            .common
            .slot_list
            .get_center_item_index(queue_len)
        {
            Some(idx) => idx,
            None => return Task::none(),
        };

        // Boundary check
        if up && center_idx == 0 {
            return Task::none();
        }
        if !up && center_idx >= queue_len.saturating_sub(1) {
            return Task::none();
        }

        // Compute from/to using the same insert-before semantics as drag MoveItem.
        // MoveItem { from, to } removes `from` then inserts at `to`
        // (with the adjustment `insert_at = if from < to { to - 1 } else { to }`).
        let (from, to) = if up {
            (center_idx, center_idx - 1)
        } else {
            (center_idx, center_idx + 2)
        };

        debug!(
            "📦 [QUEUE] Hotkey reorder: {} item {} → {} (queue_len={})",
            direction, from, to, queue_len
        );

        // Optimistic local reorder (same logic as QueueAction::MoveItem in update/queue.rs)
        let item = self.library.queue_songs.remove(from);
        let insert_at = if from < to { to - 1 } else { to };
        self.library.queue_songs.insert(insert_at, item);

        // Move the slot list cursor to follow the item
        self.queue_page
            .common
            .slot_list
            .set_offset(insert_at, queue_len);

        // Play tab SFX for feedback
        self.sfx_engine.play(nokkvi_data::audio::SfxType::Tab);

        // Persist to backend
        self.shell_spawn("queue_move_item", move |shell| async move {
            shell.queue().move_item(from, to).await?;
            shell.queue().refresh_from_queue().await
        });

        Task::none()
    }
}
