//! Window event handlers

use std::collections::HashSet;

use iced::Task;
use nokkvi_data::{audio, utils::artwork_url::THUMBNAIL_SIZE};

use super::components::{prefetch_album_artwork_tasks, prefetch_song_artwork_tasks};
use crate::{
    Nokkvi, View,
    app_message::{ArtworkMessage, Message},
};

impl Nokkvi {
    pub(crate) fn handle_window_resized(&mut self, width: f32, height: f32) -> Task<Message> {
        self.window.width = width;
        self.window.height = height;
        // Anchored overlays (context menus, checkbox dropdowns) point at
        // pixel positions captured pre-resize; close them so they don't end
        // up in the wrong spot.
        self.open_menu = None;

        // Recompute the player bar's responsive layout with per-mode
        // hysteresis. Done here (not in view()) so view() stays pure — it
        // just reads the already-computed layout off self.
        self.player_bar_layout =
            crate::widgets::player_bar::compute_layout(width, self.player_bar_layout);

        // Recompute dynamic slot count for the new window size and sync it
        // to every view's SlotListView. Without this, prefetch_indices() uses
        // a stale slot_count (default 9) and under-fetches artwork for tall windows.
        self.resync_slot_counts();

        // Re-prefetch mini artwork for the active view since more slots may
        // now be visible.
        self.prefetch_viewport_artwork()
    }

    /// Recompute every page's `slot_count` to match what the view actually
    /// renders given the current window dimensions AND the active artwork
    /// column mode. Vertical artwork modes (and Auto's portrait fallback)
    /// stack the artwork above the slot list, eating ~`layout.extent` + pad
    /// pixels from the available height — without re-syncing here, the
    /// stored slot_count stays at the horizontal-layout value and
    /// `pending_expand_resolve` lands the auto-expanded row above the visible
    /// viewport (the row only the user can rescue with a manual scroll).
    pub(crate) fn resync_slot_counts(&mut self) {
        use crate::widgets::{
            base_slot_list_layout::{BaseSlotListLayoutConfig, vertical_artwork_chrome},
            slot_list::{SlotListConfig, chrome_height_with_header},
        };

        let standard_chrome = chrome_height_with_header();
        let layout = BaseSlotListLayoutConfig {
            window_width: self.content_pane_width(),
            window_height: self.window.height,
            show_artwork_column: true,
            slot_list_chrome: standard_chrome,
        };
        let vertical = vertical_artwork_chrome(&layout);
        let sc = SlotListConfig::with_dynamic_slots(self.window.height, standard_chrome + vertical)
            .slot_count;

        self.albums_page.common.slot_list.slot_count = sc;
        self.artists_page.common.slot_list.slot_count = sc;
        self.songs_page.common.slot_list.slot_count = sc;
        self.genres_page.common.slot_list.slot_count = sc;
        self.playlists_page.common.slot_list.slot_count = sc;
        self.queue_page.common.slot_list.slot_count = sc;
    }

    /// Prefetch mini artwork for whatever slots are currently visible in the
    /// active view, plus the centered slot's large artwork. Called on window
    /// resize (slot_count change), on view switch, and on every roulette tick
    /// once the spin advances to a new offset (throttled at the call site).
    ///
    /// The center-large dispatch matches the per-view tab/click flow so
    /// scroll-driven offset changes (window resize, roulette) keep the
    /// right-side panel updating in step with the wheel — without it,
    /// Albums/Artists/Songs/Queue would show whatever was cached when the
    /// spin started, since their normal-flow `LoadLargeArtwork` is wired to
    /// `NavigateUp/Down`/`SetOffset` actions, not to viewport prefetch.
    pub(crate) fn prefetch_viewport_artwork(&mut self) -> Task<Message> {
        let shell = match &self.app_service {
            Some(s) => s,
            None => return Task::none(),
        };

        match self.current_view {
            View::Albums => {
                let albums_vm = shell.albums().clone();
                let cached: HashSet<&String> =
                    self.artwork.album_art.iter().map(|(k, _)| k).collect();
                let mut tasks = prefetch_album_artwork_tasks(
                    &self.albums_page.common.slot_list,
                    &self.library.albums,
                    &cached,
                    albums_vm,
                    |album| (album.id.clone(), album.artwork_url.clone()),
                );
                if let Some(task) = self.center_large_artwork_load_task(View::Albums) {
                    tasks.push(task);
                }
                Task::batch(tasks)
            }
            View::Queue => {
                let albums_vm = shell.albums().clone();
                let cached: HashSet<&String> =
                    self.artwork.album_art.iter().map(|(k, _)| k).collect();
                let items = self.filter_queue_songs();
                let mut tasks = prefetch_album_artwork_tasks(
                    &self.queue_page.common.slot_list,
                    &items,
                    &cached,
                    albums_vm,
                    |song| (song.album_id.clone(), song.artwork_url.clone()),
                );
                if let Some(task) = self.center_large_artwork_load_task(View::Queue) {
                    tasks.push(task);
                }
                Task::batch(tasks)
            }
            View::Songs => {
                let albums_vm = shell.albums().clone();
                let cached: HashSet<&String> =
                    self.artwork.album_art.iter().map(|(k, _)| k).collect();
                let mut tasks = prefetch_song_artwork_tasks(
                    &self.songs_page.common.slot_list,
                    &self.library.songs,
                    &cached,
                    albums_vm,
                    |s| s.album_id.as_ref(),
                );
                if let Some(task) = self.center_large_artwork_load_task(View::Songs) {
                    tasks.push(task);
                }
                Task::batch(tasks)
            }
            View::Artists => {
                let mut tasks = vec![self.prefetch_artist_mini_artwork_tasks()];
                if let Some(task) = self.center_large_artwork_load_task(View::Artists) {
                    tasks.push(task);
                }
                Task::batch(tasks)
            }
            View::Genres if !self.library.genres.is_empty() => {
                // Re-dispatch a SetOffset to trigger collage artwork loading
                let offset = self.genres_page.common.slot_list.viewport_offset;
                Task::done(Message::Genres(crate::views::GenresMessage::SlotList(
                    crate::widgets::SlotListPageMessage::SetOffset(
                        offset,
                        iced::keyboard::Modifiers::default(),
                    ),
                )))
            }
            View::Playlists if !self.library.playlists.is_empty() => {
                // Re-dispatch a SetOffset to trigger collage artwork loading
                let offset = self.playlists_page.common.slot_list.viewport_offset;
                Task::done(Message::Playlists(
                    crate::views::PlaylistsMessage::SlotList(
                        crate::widgets::SlotListPageMessage::SetOffset(
                            offset,
                            iced::keyboard::Modifiers::default(),
                        ),
                    ),
                ))
            }
            View::Genres | View::Playlists | View::Radios | View::Settings => Task::none(),
        }
    }

    pub(crate) fn handle_scale_factor_changed(&mut self, scale_factor: f32) -> Task<Message> {
        self.window.scale_factor = scale_factor;
        Task::none()
    }

    pub(crate) fn handle_play_sfx(&mut self, sfx_type: audio::SfxType) -> Task<Message> {
        self.sfx_engine.play(sfx_type);
        Task::none()
    }

    /// Dispatch the per-view "load large artwork for the centered slot"
    /// task, mirroring what each view's normal `NavigateUp`/`NavigateDown`/
    /// `SetOffset` action emits. Returns `None` when the view doesn't have
    /// a per-slot large-artwork concept (Genres/Playlists drive their
    /// collage panel through a separate `LoadArtwork` action that
    /// `prefetch_viewport_artwork` re-synthesizes via `SetOffset`), when
    /// the centered item's artwork is already cached, or when there is no
    /// active session.
    pub(crate) fn center_large_artwork_load_task(&mut self, view: View) -> Option<Task<Message>> {
        self.app_service.as_ref()?;

        match view {
            View::Albums => {
                let total = self.library.albums.len();
                let center_idx = self
                    .albums_page
                    .common
                    .slot_list
                    .get_center_item_index(total)?;
                let album_id = self.library.albums.get(center_idx)?.id.clone();
                if self.artwork.large_artwork.peek(&album_id).is_some() {
                    return None;
                }
                Some(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(
                    album_id,
                ))))
            }
            View::Songs => {
                let total = self.library.songs.len();
                let center_idx = self
                    .songs_page
                    .common
                    .slot_list
                    .get_center_item_index(total)?;
                let album_id = self.library.songs.get(center_idx)?.album_id.clone()?;
                if self.artwork.large_artwork.peek(&album_id).is_some() {
                    return None;
                }
                Some(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(
                    album_id,
                ))))
            }
            View::Queue => {
                // queue/view.rs falls back to the centered song's artwork
                // whenever the playing song's isn't cached, so we always
                // want the centered slot's large artwork ready (matches
                // `load_queue_viewport_artwork`'s unconditional dispatch).
                let songs = self.filter_queue_songs();
                let total = songs.len();
                let center_idx = self
                    .queue_page
                    .common
                    .slot_list
                    .get_center_item_index(total)?;
                let album_id = songs.get(center_idx)?.album_id.clone();
                if self.artwork.large_artwork.peek(&album_id).is_some() {
                    return None;
                }
                Some(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(
                    album_id,
                ))))
            }
            View::Artists => {
                let total = self.library.artists.len();
                let center_idx = self
                    .artists_page
                    .common
                    .slot_list
                    .get_center_item_index(total)?;
                let artist_id = self.library.artists.get(center_idx)?.id.clone();
                if self.artwork.large_artwork.peek(&artist_id).is_some() {
                    return None;
                }
                Some(self.handle_load_artist_large_artwork(artist_id))
            }
            View::Genres | View::Playlists | View::Radios | View::Settings => None,
        }
    }

    /// Load mini artist artwork from disk cache for all prefetch-visible slots.
    ///
    /// Dispatch async fetches for any uncached artist mini artwork in the
    /// current viewport. Returns a batch of tasks producing
    /// `ArtworkMessage::Loaded`.
    ///
    /// Shared by: `handle_artists_loaded`, `handle_artists` (slot list change),
    /// and `prefetch_viewport_artwork` (window resize / view switch).
    pub(crate) fn prefetch_artist_mini_artwork_tasks(&self) -> Task<Message> {
        use iced::widget::image;

        let total = self.library.artists.len();
        if total == 0 {
            return Task::none();
        }
        let albums_vm = match self.app_service.as_ref() {
            Some(svc) => svc.albums().clone(),
            None => return Task::none(),
        };

        let mut tasks = Vec::new();
        for idx in self.artists_page.common.slot_list.prefetch_indices(total) {
            if let Some(artist) = self.library.artists.get(idx)
                && !self.artwork.album_art.contains(&artist.id)
            {
                let id = artist.id.clone();
                let art_id = format!("ar-{id}");
                let vm = albums_vm.clone();
                tasks.push(Task::perform(
                    async move {
                        let bytes = vm
                            .fetch_album_artwork(&art_id, Some(THUMBNAIL_SIZE), None)
                            .await
                            .ok();
                        (id, bytes.map(image::Handle::from_bytes))
                    },
                    |(id, handle)| {
                        Message::Artwork(crate::app_message::ArtworkMessage::Loaded(id, handle))
                    },
                ));
            }
        }
        Task::batch(tasks)
    }
}
