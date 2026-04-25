//! Update handler for Similar Songs feature.
//!
//! Handles SimilarMessage routing, FindSimilar/FindTopSongs API dispatch,
//! and SimilarSongsLoaded response processing with generation counter.

use iced::Task;
use tracing::{debug, info, warn};

use crate::{
    Nokkvi,
    app_message::Message,
    state::SimilarSongsState,
    views::{BrowsingPanel, BrowsingView, SimilarAction, SimilarMessage},
};

impl Nokkvi {
    /// Route SimilarMessage to the page and handle returned actions.
    pub(crate) fn handle_similar_message(&mut self, msg: SimilarMessage) -> Task<Message> {
        let songs = self
            .similar_songs
            .as_ref()
            .map_or(&[][..], |s| s.songs.as_slice());

        let (task, action) = self.similar_page.update(msg, songs);
        let task = task.map(Message::Similar);

        let action_task = match action {
            SimilarAction::AddBatchToQueue(payload) => {
                let len = payload.items.len();
                debug!(" Adding batch of {} similar items to queue", len);
                if let Some(pos) = self.pending_queue_insert_position.take() {
                    self.shell_fire_and_forget_task(
                        move |shell| async move {
                            shell.insert_batch_at_position(payload, pos).await
                        },
                        format!("Inserted {} items at position {}", len, pos + 1),
                        "insert similar batch to queue",
                    )
                } else {
                    self.shell_fire_and_forget_task(
                        move |shell| async move { shell.add_batch_to_queue(payload).await },
                        format!("Added {len} similar items to queue"),
                        "add similar batch to queue",
                    )
                }
            }
            SimilarAction::PlayBatch(payload) => {
                let len = payload.items.len();
                debug!(" Playing batch of {} similar items", len);
                self.shell_fire_and_forget_task(
                    move |shell| async move { shell.play_batch(payload).await },
                    format!("Playing batch of {len} items"),
                    "play similar batch",
                )
            }
            SimilarAction::AddBatchToPlaylist(payload) => {
                self.handle_add_batch_to_playlist(payload)
            }
            SimilarAction::ToggleStar(song_id, starred) => {
                let optimistic_msg = Self::starred_revert_message(song_id.clone(), "song", starred);
                Task::batch(vec![
                    Task::done(optimistic_msg),
                    self.star_item_task(song_id, "song", starred),
                ])
            }

            SimilarAction::LoadLargeArtwork(album_id) => {
                let mut tasks = vec![Task::done(Message::Artwork(
                    crate::app_message::ArtworkMessage::LoadLarge(album_id),
                ))];

                if let Some(shell) = &self.app_service {
                    let cached: std::collections::HashSet<&String> =
                        self.artwork.album_art.iter().map(|(k, _)| k).collect();
                    if let Some(state) = &self.similar_songs {
                        let prefetch_tasks =
                            crate::update::components::prefetch_raw_song_artwork_tasks(
                                &self.similar_page.common.slot_list,
                                &state.songs,
                                &cached,
                                shell.albums().clone(),
                            );
                        tasks.extend(prefetch_tasks);
                    }
                }

                Task::batch(tasks)
            }
            SimilarAction::ShowInfo(item) => {
                self.info_modal.open(*item);
                Task::none()
            }
            SimilarAction::ShowInFolder(path) => self.handle_show_in_folder(path),
            SimilarAction::FindSimilar(id, title) => {
                // Recursive discovery — find similar from within similar results
                Task::done(Message::FindSimilar {
                    id,
                    label: format!("Similar to: {title}"),
                })
            }
            SimilarAction::FindTopSongs(artist_name, label) => {
                // Top songs for artist — from within similar results
                Task::done(Message::FindTopSongs { artist_name, label })
            }
            SimilarAction::None => Task::none(),
        };

        Task::batch([task, action_task])
    }

    /// Handle "Find Similar" — opens browsing panel on Similar tab and fires API.
    pub(crate) fn handle_find_similar(&mut self, id: String, label: String) -> Task<Message> {
        info!("🎵 Finding similar songs for id={}", id);

        // Ensure browsing panel is open and on Similar tab
        self.ensure_browsing_panel_on_similar();

        // Bump generation + set loading
        self.similar_songs_generation += 1;
        let generation = self.similar_songs_generation;
        self.similar_songs = Some(SimilarSongsState {
            songs: Vec::new(),
            label: label.clone(),
            loading: true,
        });

        // Reset slot list to top
        self.similar_page.common.slot_list.set_offset(0, 0);

        self.shell_task(
            move |shell| async move {
                let api = shell.similar_api().await?;
                api.get_similar_songs(&id, 500).await
            },
            move |result| {
                Message::SimilarSongsLoaded(generation, result.map_err(|e| e.to_string()), label)
            },
        )
    }

    /// Handle "Top Songs" — opens browsing panel on Similar tab and fires API.
    pub(crate) fn handle_find_top_songs(
        &mut self,
        artist_name: String,
        label: String,
    ) -> Task<Message> {
        info!("🎵 Finding top songs for artist='{}'", artist_name);

        // Ensure browsing panel is open and on Similar tab
        self.ensure_browsing_panel_on_similar();

        // Bump generation + set loading
        self.similar_songs_generation += 1;
        let generation = self.similar_songs_generation;
        self.similar_songs = Some(SimilarSongsState {
            songs: Vec::new(),
            label: label.clone(),
            loading: true,
        });

        // Reset slot list to top
        self.similar_page.common.slot_list.set_offset(0, 0);

        self.shell_task(
            move |shell| async move {
                let api = shell.similar_api().await?;
                api.get_top_songs(&artist_name, 500).await
            },
            move |result| {
                Message::SimilarSongsLoaded(generation, result.map_err(|e| e.to_string()), label)
            },
        )
    }

    /// Handle API response for similar/top songs.
    pub(crate) fn handle_similar_songs_loaded(
        &mut self,
        generation: u64,
        result: Result<Vec<nokkvi_data::types::song::Song>, String>,
        label: String,
    ) -> Task<Message> {
        // Reject stale responses
        if generation != self.similar_songs_generation {
            debug!(
                "🎵 Ignoring stale similar songs response (gen {} vs current {})",
                generation, self.similar_songs_generation
            );
            return Task::none();
        }

        match result {
            Ok(songs) => {
                let count = songs.len();

                if songs.is_empty() {
                    self.toast_info("No similar songs found");
                    self.similar_songs = Some(SimilarSongsState {
                        songs: Vec::new(),
                        label,
                        loading: false,
                    });
                    return Task::none();
                }

                info!("🎵 Loaded {} similar/top songs", count);

                // Update state FIRST so that scrolling offset operates on valid data
                self.similar_songs = Some(SimilarSongsState {
                    songs,
                    label,
                    loading: false,
                });

                // Reset slot list for new result set
                let total = self.similar_songs.as_ref().map_or(0, |s| s.songs.len());
                self.similar_page.common.slot_list.set_offset(0, total);

                // Prefetch visible viewport miniature artwork!
                let mut tasks = Vec::new();

                // Select the first item (center) to seed the large artwork panel immediately
                if let Some(state) = &self.similar_songs {
                    #[allow(clippy::collapsible_if)]
                    if let Some(first_song) = state.songs.first() {
                        if let Some(album_id) = &first_song.album_id {
                            tasks.push(Task::done(Message::Artwork(
                                crate::app_message::ArtworkMessage::LoadLarge(album_id.clone()),
                            )));
                        }
                    }

                    if let Some(shell) = &self.app_service {
                        let cached: std::collections::HashSet<&String> =
                            self.artwork.album_art.iter().map(|(k, _)| k).collect();
                        let prefetch_tasks =
                            crate::update::components::prefetch_raw_song_artwork_tasks(
                                &self.similar_page.common.slot_list,
                                &state.songs,
                                &cached,
                                shell.albums().clone(),
                            );
                        tasks.extend(prefetch_tasks);
                    }
                }

                if tasks.is_empty() {
                    Task::none()
                } else {
                    Task::batch(tasks)
                }
            }
            Err(e) => {
                if e.contains("Unauthorized") {
                    return self.handle_session_expired();
                }
                warn!("🎵 Failed to load similar songs: {}", e);
                self.toast_error(format!("Failed to load similar songs: {e}"));
                self.similar_songs = Some(SimilarSongsState {
                    songs: Vec::new(),
                    label,
                    loading: false,
                });
                Task::none()
            }
        }
    }

    /// Ensure the browsing panel is open and focused on the Similar tab.
    fn ensure_browsing_panel_on_similar(&mut self) {
        // Switch to Queue view if not already there (browsing panel only shows with Queue)
        if self.current_view != crate::View::Queue {
            self.current_view = crate::View::Queue;
        }

        // Open browsing panel if not open
        if self.browsing_panel.is_none() {
            self.browsing_panel = Some(BrowsingPanel::new());
        }

        // Switch to Similar tab
        if let Some(panel) = &mut self.browsing_panel {
            panel.active_view = BrowsingView::Similar;
        }

        // Focus the browser pane
        self.pane_focus = crate::state::PaneFocus::Browser;
    }
}
