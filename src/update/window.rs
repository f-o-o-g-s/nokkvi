//! Window event handlers

use std::collections::HashSet;

use iced::Task;
use nokkvi_data::audio;

use super::components::{prefetch_album_artwork_tasks, prefetch_song_artwork_tasks};
use crate::{Nokkvi, View, app_message::Message};

impl Nokkvi {
    pub(crate) fn handle_window_resized(&mut self, width: f32, height: f32) -> Task<Message> {
        self.window.width = width;
        self.window.height = height;

        // Recompute dynamic slot count for the new window size and sync it
        // to every view's SlotListView. Without this, prefetch_indices() uses
        // a stale slot_count (default 9) and under-fetches artwork for tall windows.
        use crate::widgets::slot_list::{SlotListConfig, chrome_height_with_header};
        let config = SlotListConfig::with_dynamic_slots(height, chrome_height_with_header());
        let sc = config.slot_count;
        self.albums_page.common.slot_list.slot_count = sc;
        self.artists_page.common.slot_list.slot_count = sc;
        self.songs_page.common.slot_list.slot_count = sc;
        self.genres_page.common.slot_list.slot_count = sc;
        self.playlists_page.common.slot_list.slot_count = sc;
        self.queue_page.common.slot_list.slot_count = sc;

        // Re-prefetch mini artwork for the active view since more slots may
        // now be visible.
        self.prefetch_viewport_artwork()
    }

    /// Prefetch mini artwork for whatever slots are currently visible in the
    /// active view. Called on window resize (slot_count change) and on view
    /// switch to fill artwork for newly visible slots.
    pub(crate) fn prefetch_viewport_artwork(&mut self) -> Task<Message> {
        let shell = match &self.app_service {
            Some(s) => s,
            None => return Task::none(),
        };

        match self.current_view {
            View::Albums => {
                let albums_vm = shell.albums().clone();
                let cached: HashSet<&String> = self.artwork.album_art.keys().collect();
                let tasks = prefetch_album_artwork_tasks(
                    &self.albums_page.common.slot_list,
                    &self.library.albums,
                    &cached,
                    albums_vm,
                    |album| (album.id.clone(), album.artwork_url.clone()),
                );
                Task::batch(tasks)
            }
            View::Queue => {
                let albums_vm = shell.albums().clone();
                let cached: HashSet<&String> = self.artwork.album_art.keys().collect();
                let items = self.filter_queue_songs();
                let tasks = prefetch_album_artwork_tasks(
                    &self.queue_page.common.slot_list,
                    &items,
                    &cached,
                    albums_vm,
                    |song| (song.album_id.clone(), song.artwork_url.clone()),
                );
                Task::batch(tasks)
            }
            View::Songs => {
                let albums_vm = shell.albums().clone();
                let cached: HashSet<&String> = self.artwork.album_art.keys().collect();
                let tasks = prefetch_song_artwork_tasks(
                    &self.songs_page.common.slot_list,
                    &self.library.songs,
                    &cached,
                    albums_vm,
                );
                Task::batch(tasks)
            }
            View::Artists => {
                self.load_artist_mini_artwork_from_cache();
                Task::none()
            }
            View::Genres if !self.library.genres.is_empty() => {
                // Re-dispatch a SetOffset to trigger collage artwork loading
                let offset = self.genres_page.common.slot_list.viewport_offset;
                Task::done(Message::Genres(
                    crate::views::GenresMessage::SlotListSetOffset(
                        offset,
                        iced::keyboard::Modifiers::default(),
                    ),
                ))
            }
            View::Playlists if !self.library.playlists.is_empty() => {
                // Re-dispatch a SetOffset to trigger collage artwork loading
                let offset = self.playlists_page.common.slot_list.viewport_offset;
                Task::done(Message::Playlists(
                    crate::views::PlaylistsMessage::SlotListSetOffset(
                        offset,
                        iced::keyboard::Modifiers::default(),
                    ),
                ))
            }
            _ => Task::none(),
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

    /// Load mini artist artwork from disk cache for all prefetch-visible slots.
    ///
    /// Shared by: `handle_artists_loaded`, `handle_artists` (slot list change),
    /// and `prefetch_viewport_artwork` (window resize / view switch).
    pub(crate) fn load_artist_mini_artwork_from_cache(&mut self) {
        let total = self.library.artists.len();
        if total == 0 {
            return;
        }
        let cache_ref = self.artwork.artist_disk_cache.as_ref();
        for idx in self.artists_page.common.slot_list.prefetch_indices(total) {
            if let Some(artist) = self.library.artists.get(idx)
                && !self.artwork.album_art.contains_key(&artist.id)
                && let Some(cache) = cache_ref
            {
                let cache_key = format!("ar-{}_80", artist.id);
                if cache.contains(&cache_key) {
                    self.artwork.album_art.insert(
                        artist.id.clone(),
                        iced::widget::image::Handle::from_path(cache.get_path(&cache_key)),
                    );
                }
            }
        }
    }
}
