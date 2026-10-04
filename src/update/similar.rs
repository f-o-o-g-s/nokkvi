//! Update handler for Similar Songs feature.
//!
//! Handles SimilarMessage routing, FindSimilar/FindTopSongs API dispatch,
//! and SimilarSongsLoaded response processing with generation counter.

use iced::Task;
use nokkvi_data::types::ItemKind;
use tracing::{debug, info, warn};

use crate::{
    Nokkvi,
    app_message::{FindMessage, Message},
    state::{SimilarSongsState, SimilarSource},
    views::{BrowsingPanel, BrowsingView, SimilarAction, SimilarMessage},
};

impl Nokkvi {
    /// Route SimilarMessage to the page and handle returned actions.
    pub(crate) fn handle_similar_message(&mut self, msg: SimilarMessage) -> Task<Message> {
        // Similar lives in the browsing panel and has no top-level `View`
        // variant — the only chrome paths that matter here are SetOpenMenu
        // and the artwork-drag interceptors. `View::Queue` is a placeholder
        // for the Roulette arm, which Similar's `is_roulette` always vetoes.
        if let Some(task) = crate::update::dispatch_view_chrome(self, &msg, crate::View::Queue) {
            return task;
        }
        let songs = self
            .similar_songs
            .as_ref()
            .map_or(&[][..], |s| s.songs.as_slice());

        let (task, action) = self.similar_page.update(msg, songs);
        let task = task.map(Message::Similar);

        let action_task = match action {
            SimilarAction::AddBatchToQueue(payload) => {
                self.add_or_insert_batch_to_queue_task(payload)
            }
            SimilarAction::PlayBatch(payload) => self.play_batch_in_place_task(payload),
            SimilarAction::AddBatchToPlaylist(payload) => {
                self.handle_add_batch_to_playlist(payload)
            }
            SimilarAction::AddBatchToMix(seeds) => self.add_seeds_to_mix(seeds),
            SimilarAction::ToggleStar(song_id, starred) => {
                self.toggle_star_with_revert_task(song_id, ItemKind::Song, starred)
            }

            SimilarAction::LoadLargeArtwork(album_id) => {
                let mut tasks = vec![Task::done(Message::Artwork(
                    crate::app_message::ArtworkMessage::LoadLarge(album_id),
                ))];

                if let Some(shell) = &self.app_service {
                    let cached: std::collections::HashSet<&String> =
                        self.artwork.album_art.iter().map(|(k, _)| k).collect();
                    if let Some(state) = &self.similar_songs {
                        let prefetch_tasks = crate::update::components::prefetch_song_artwork_tasks(
                            &self.similar_page.common.slot_list,
                            &state.songs,
                            &cached,
                            &self.artwork.album_art_versions,
                            &self.artwork.failed_art,
                            shell.albums().clone(),
                            |s| {
                                s.album_id.as_ref().map(|id| {
                                    (
                                        id,
                                        crate::update::components::passive_artwork_version(
                                            &s.updated_at,
                                        ),
                                    )
                                })
                            },
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
            SimilarAction::FindSimilar(id, seed_name) => {
                // Recursive discovery — find similar from within similar results
                Task::done(Message::Find(FindMessage::Similar { id, seed_name }))
            }
            SimilarAction::FindTopSongs(artist_name) => {
                // Top songs for artist — from within similar results
                Task::done(Message::Find(FindMessage::TopSongs { artist_name }))
            }
            SimilarAction::ColumnVisibilityChanged(col, value) => {
                self.persist_column_visibility(col, value)
            }
            SimilarAction::None => Task::none(),
        };

        Task::batch([task, action_task])
    }

    /// Dispatch `FindMessage` variants to the per-variant handlers below.
    ///
    /// Cross-cutting carrier for the find/similar lookup cluster — separate
    /// from `handle_similar_message`, which handles the Similar VIEW's own
    /// per-view interaction messages.
    pub(crate) fn handle_find_message(&mut self, msg: FindMessage) -> Task<Message> {
        match msg {
            FindMessage::Similar { id, seed_name } => self.handle_find_similar(id, seed_name),
            FindMessage::TopSongs { artist_name } => self.handle_find_top_songs(artist_name),
            FindMessage::Loaded(generation, result, source) => {
                self.handle_similar_songs_loaded(generation, result, source)
            }
        }
    }

    /// Handle "Find Similar" — opens browsing panel on Similar tab and fires API.
    /// `seed_name` is the song/album/artist's display name for the header.
    pub(crate) fn handle_find_similar(&mut self, id: String, seed_name: String) -> Task<Message> {
        info!("🎵 Finding similar songs for id={}", id);
        self.open_similar_results(
            SimilarSource::SimilarTo(seed_name),
            move |shell| async move {
                let api = shell.similar_api().await?;
                api.get_similar_songs(&id, 500).await
            },
        )
    }

    /// Handle "Top Songs" — opens browsing panel on Similar tab and fires API.
    pub(crate) fn handle_find_top_songs(&mut self, artist_name: String) -> Task<Message> {
        info!("🎵 Finding top songs for artist='{}'", artist_name);
        let source = SimilarSource::TopSongs(artist_name.clone());
        self.open_similar_results(source, move |shell| async move {
            let api = shell.similar_api().await?;
            api.get_top_songs(&artist_name, 500).await
        })
    }

    /// Shared front half of Find Similar / Top Songs: show the Similar tab in
    /// its loading state for `source`, then run `fetch` under a fresh
    /// stale-drop generation.
    fn open_similar_results<F, Fut>(&mut self, source: SimilarSource, fetch: F) -> Task<Message>
    where
        F: FnOnce(nokkvi_data::backend::app_service::AppService) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = anyhow::Result<Vec<nokkvi_data::types::song::Song>>>
            + Send,
    {
        // Results land in the split view, so Theater Mode leaves first (the
        // route backstop cannot see a re-run on an already-open Similar tab).
        let exit_theater = self.exit_theater();

        // Ensure browsing panel is open and on Similar tab
        self.ensure_browsing_panel_on_similar();

        // Bump generation + set loading
        let generation = self.similar_songs_generation.bump();
        self.similar_songs = Some(SimilarSongsState {
            songs: Vec::new(),
            source: source.clone(),
            loading: true,
        });

        // Reset slot list to top
        self.similar_page.common.slot_list.set_offset(0, 0);

        let fetch = self.shell_task(fetch, move |result| {
            Message::Find(FindMessage::Loaded(
                generation,
                result.map_err(|e| e.to_string()),
                source,
            ))
        });
        Task::batch([exit_theater, fetch])
    }

    /// Handle API response for similar/top songs.
    pub(crate) fn handle_similar_songs_loaded(
        &mut self,
        generation: u64,
        result: Result<Vec<nokkvi_data::types::song::Song>, String>,
        source: SimilarSource,
    ) -> Task<Message> {
        // Reject stale responses
        if !self.similar_songs_generation.accepts(generation) {
            debug!(
                "🎵 Ignoring stale similar songs response (gen {} vs current {})",
                generation,
                self.similar_songs_generation.current()
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
                        source,
                        loading: false,
                    });
                    return Task::none();
                }

                info!("🎵 Loaded {} similar/top songs", count);

                // Update state FIRST so that scrolling offset operates on valid data
                self.similar_songs = Some(SimilarSongsState {
                    songs,
                    source,
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
                        let prefetch_tasks = crate::update::components::prefetch_song_artwork_tasks(
                            &self.similar_page.common.slot_list,
                            &state.songs,
                            &cached,
                            &self.artwork.album_art_versions,
                            &self.artwork.failed_art,
                            shell.albums().clone(),
                            |s| {
                                s.album_id.as_ref().map(|id| {
                                    (
                                        id,
                                        crate::update::components::passive_artwork_version(
                                            &s.updated_at,
                                        ),
                                    )
                                })
                            },
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
                if nokkvi_data::types::error::NokkviError::is_unauthorized_str(&e) {
                    return self.handle_session_expired();
                }
                warn!("🎵 Failed to load similar songs: {}", e);
                self.toast_error(format!("Failed to load similar songs: {e}"));
                self.similar_songs = Some(SimilarSongsState {
                    songs: Vec::new(),
                    source,
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
