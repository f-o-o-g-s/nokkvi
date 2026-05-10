//! Slot list navigation handlers

use iced::Task;
use nokkvi_data::audio;
use tracing::debug;

use crate::{
    Nokkvi, View,
    app_message::{Message, SlotListMessage},
    views,
};

impl Nokkvi {
    /// Top-level slot list message dispatcher
    pub(crate) fn handle_slot_list_message(&mut self, msg: SlotListMessage) -> Task<Message> {
        // Default-playlist picker takes priority — when its modal is open,
        // arrow keys / Tab / Enter steer the picker, not the underlying view.
        if self.default_playlist_picker.is_some() {
            use crate::widgets::default_playlist_picker::DefaultPlaylistPickerMessage;
            return match msg {
                SlotListMessage::NavigateUp => {
                    self.handle_default_playlist_picker(DefaultPlaylistPickerMessage::SlotListUp)
                }
                SlotListMessage::NavigateDown => {
                    self.handle_default_playlist_picker(DefaultPlaylistPickerMessage::SlotListDown)
                }
                SlotListMessage::ActivateCenter => self
                    .handle_default_playlist_picker(DefaultPlaylistPickerMessage::ActivateCenter),
                _ => Task::none(),
            };
        }
        match msg {
            SlotListMessage::NavigateUp => {
                let task = self.handle_slot_list_navigate_up();
                Task::batch([task, self.scrollbar_fade_timer(self.current_view)])
            }
            SlotListMessage::NavigateDown => {
                let task = self.handle_slot_list_navigate_down();
                Task::batch([task, self.scrollbar_fade_timer(self.current_view)])
            }
            SlotListMessage::SetOffset(offset) => {
                let task = self.handle_slot_list_set_offset(offset);
                Task::batch([task, self.scrollbar_fade_timer(self.current_view)])
            }
            SlotListMessage::ActivateCenter => self.handle_slot_list_activate_center(),
            SlotListMessage::ToggleSortOrder => self.handle_toggle_sort_order(),
            SlotListMessage::ScrollbarFadeComplete(view, gen_id) => {
                self.handle_scrollbar_fade_complete(view, gen_id)
            }
            SlotListMessage::SeekSettled(view, gen_id) => self.handle_seek_settled(view, gen_id),
        }
    }

    /// Fire a delayed task that will clear the scrollbar after the fade period.
    /// Uses the same generation-ID guard pattern as `create_percentage_hide_timer`.
    pub(crate) fn scrollbar_fade_timer(&self, view: View) -> Task<Message> {
        let gen_id = self
            .view_page(view)
            .map_or(0, |p| p.common().slot_list.scroll_generation_id);

        Task::perform(
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                gen_id
            },
            move |id| Message::SlotList(SlotListMessage::ScrollbarFadeComplete(view, id)),
        )
    }

    /// Fire a short debounced task to trigger artwork prefetch after scrollbar
    /// seek settles. Uses the same generation-ID guard — only the last seek's
    /// timer passes the check.
    pub(crate) fn seek_settled_timer(&self, view: View) -> Task<Message> {
        let gen_id = self
            .view_page(view)
            .map_or(0, |p| p.common().slot_list.scroll_generation_id);

        Task::perform(
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                gen_id
            },
            move |id| Message::SlotList(SlotListMessage::SeekSettled(view, id)),
        )
    }

    fn handle_scrollbar_fade_complete(&mut self, view: View, gen_id: u64) -> Task<Message> {
        if let Some(page) = self.view_page_mut(view) {
            let sl = &mut page.common_mut().slot_list;
            if sl.scroll_generation_id == gen_id {
                sl.last_scrolled = None;
            }
        }
        Task::none()
    }

    /// When the scrollbar seek settles (150ms idle), trigger artwork prefetch
    /// for the target viewport. This runs the same artwork-loading logic that
    /// the normal navigation path uses, but only once instead of per-event.
    fn handle_seek_settled(&mut self, view: View, gen_id: u64) -> Task<Message> {
        // Generation-ID guard: only the most recent seek's timer fires
        let current_gen = self
            .view_page(view)
            .map_or(0, |p| p.common().slot_list.scroll_generation_id);
        if current_gen != gen_id {
            return Task::none();
        }

        // Dispatch to the target view's artwork-loading path.
        // For queue: dedicated zero-clone helper.
        // For library views: dispatch a synthetic SetOffset(current_offset) via
        // the trait method, which flows through the normal handler and triggers
        // artwork prefetch via the LoadLargeArtwork / prefetch_album_artwork_tasks path.
        // For settings: no-op.
        match view {
            View::Queue => self.load_queue_viewport_artwork(),
            View::Settings => Task::none(),
            _ => {
                let msg = self.view_page(view).and_then(|page| {
                    let offset = page.common().slot_list.viewport_offset;
                    page.synth_set_offset_message(offset)
                });
                msg.map_or_else(Task::none, |m| self.update(m))
            }
        }
    }

    fn handle_slot_list_navigate_up(&mut self) -> Task<Message> {
        debug!(
            " [SLOT_LIST] SlotListNavigateUp triggered on {:?}",
            self.current_view
        );
        // Play tab navigation sound
        self.sfx_engine.play(audio::SfxType::Tab);
        match self.current_view {
            View::Albums => Task::done(Message::Albums(views::AlbumsMessage::SlotList(
                crate::widgets::SlotListPageMessage::NavigateUp,
            ))),
            View::Artists => Task::done(Message::Artists(views::ArtistsMessage::SlotList(
                crate::widgets::SlotListPageMessage::NavigateUp,
            ))),
            View::Queue => Task::done(Message::Queue(views::QueueMessage::SlotListNavigateUp)),
            View::Songs => Task::done(Message::Songs(views::SongsMessage::SlotList(
                crate::widgets::SlotListPageMessage::NavigateUp,
            ))),
            View::Genres => Task::done(Message::Genres(views::GenresMessage::SlotList(
                crate::widgets::SlotListPageMessage::NavigateUp,
            ))),
            View::Playlists => Task::done(Message::Playlists(
                views::PlaylistsMessage::SlotListNavigateUp,
            )),
            View::Radios => Task::done(Message::Radios(views::RadiosMessage::SlotListNavigateUp)),
            View::Settings => Task::done(Message::Settings(views::SettingsMessage::SlotListUp)),
        }
    }

    pub(crate) fn handle_slot_list_navigate_down(&mut self) -> Task<Message> {
        // If search is focused, unfocus it as a side-effect of navigating.
        // SlotListDown (default: Tab) doubles as "exit search" — similar to Escape,
        // but also navigates the slot list in the same keypress.
        // We intentionally don't do this for SlotListUp (default: Backspace) because
        // Backspace is needed for deleting text in the search field.
        let unfocus_task = if self.current_view == View::Settings {
            if self.settings_page.search_active {
                self.settings_page.search_active = false;
                // Keep search_query and slot_list intact so the filtered results
                // remain visible and navigable (Tab moves into search results,
                // Escape fully clears search).
            }
            iced::widget::operation::focus("__unfocus_all__")
        } else if let Some(page) = self.current_view_page_mut()
            && page.common().search_input_focused
        {
            page.common_mut().search_input_focused = false;
            iced::widget::operation::focus("__unfocus_all__")
        } else {
            Task::none()
        };

        debug!(
            " [SLOT_LIST] SlotListNavigateDown triggered on {:?}",
            self.current_view
        );
        // Play tab navigation sound
        self.sfx_engine.play(audio::SfxType::Tab);
        let nav_task = match self.current_view {
            View::Albums => Task::done(Message::Albums(views::AlbumsMessage::SlotList(
                crate::widgets::SlotListPageMessage::NavigateDown,
            ))),
            View::Artists => Task::done(Message::Artists(views::ArtistsMessage::SlotList(
                crate::widgets::SlotListPageMessage::NavigateDown,
            ))),
            View::Queue => Task::done(Message::Queue(views::QueueMessage::SlotListNavigateDown)),
            View::Songs => Task::done(Message::Songs(views::SongsMessage::SlotList(
                crate::widgets::SlotListPageMessage::NavigateDown,
            ))),
            View::Genres => Task::done(Message::Genres(views::GenresMessage::SlotList(
                crate::widgets::SlotListPageMessage::NavigateDown,
            ))),
            View::Playlists => Task::done(Message::Playlists(
                views::PlaylistsMessage::SlotListNavigateDown,
            )),
            View::Radios => Task::done(Message::Radios(views::RadiosMessage::SlotListNavigateDown)),
            View::Settings => Task::done(Message::Settings(views::SettingsMessage::SlotListDown)),
        };
        Task::batch([unfocus_task, nav_task])
    }

    fn handle_slot_list_set_offset(&mut self, offset: usize) -> Task<Message> {
        match self.current_view {
            View::Albums => Task::done(Message::Albums(views::AlbumsMessage::SlotList(
                crate::widgets::SlotListPageMessage::SetOffset(
                    offset,
                    iced::keyboard::Modifiers::default(),
                ),
            ))),
            View::Artists => Task::done(Message::Artists(views::ArtistsMessage::SlotList(
                crate::widgets::SlotListPageMessage::SetOffset(
                    offset,
                    iced::keyboard::Modifiers::default(),
                ),
            ))),
            View::Queue => Task::done(Message::Queue(views::QueueMessage::SlotListSetOffset(
                offset,
                iced::keyboard::Modifiers::default(),
            ))),
            View::Songs => Task::done(Message::Songs(views::SongsMessage::SlotList(
                crate::widgets::SlotListPageMessage::SetOffset(
                    offset,
                    iced::keyboard::Modifiers::default(),
                ),
            ))),
            View::Genres => Task::done(Message::Genres(views::GenresMessage::SlotList(
                crate::widgets::SlotListPageMessage::SetOffset(
                    offset,
                    iced::keyboard::Modifiers::default(),
                ),
            ))),
            View::Playlists => Task::done(Message::Playlists(
                views::PlaylistsMessage::SlotListSetOffset(
                    offset,
                    iced::keyboard::Modifiers::default(),
                ),
            )),
            View::Radios => Task::done(Message::Radios(views::RadiosMessage::SlotListSetOffset(
                offset,
                iced::keyboard::Modifiers::default(),
            ))),
            View::Settings => Task::done(Message::Settings(
                views::SettingsMessage::SlotListSetOffset(
                    offset,
                    iced::keyboard::Modifiers::default(),
                ),
            )),
        }
    }

    fn handle_slot_list_activate_center(&mut self) -> Task<Message> {
        // Play enter/activate sound (settings handles its own SFX)
        if self.current_view != View::Settings {
            // Check if the current view is empty (accounts for search filtering).
            // We use the same filter_* helpers as view() to detect when the
            // results are empty even if the raw library is not.
            let is_empty = match self.current_view {
                View::Queue => self.filter_queue_songs().is_empty(),
                View::Albums => self.filter_albums().is_empty(),
                View::Songs => {
                    // Filter songs manually since there's no main.rs helper for it yet
                    nokkvi_data::utils::search::filter_items(
                        &self.library.songs,
                        &self.songs_page.common.search_query,
                    )
                    .is_empty()
                }
                View::Artists => nokkvi_data::utils::search::filter_items(
                    &self.library.artists,
                    &self.artists_page.common.search_query,
                )
                .is_empty(),
                View::Genres => nokkvi_data::utils::search::filter_items(
                    &self.library.genres,
                    &self.genres_page.common.search_query,
                )
                .is_empty(),
                View::Playlists => nokkvi_data::utils::search::filter_items(
                    &self.library.playlists,
                    &self.playlists_page.common.search_query,
                )
                .is_empty(),
                View::Radios => self.filter_radio_stations().is_empty(),
                View::Settings => false,
            };

            if is_empty {
                self.sfx_engine.play(audio::SfxType::Escape);
            } else {
                self.sfx_engine.play(audio::SfxType::Enter);
            }
        }
        match self.current_view {
            View::Albums => Task::done(Message::Albums(views::AlbumsMessage::SlotList(
                crate::widgets::SlotListPageMessage::ActivateCenter,
            ))),
            View::Artists => Task::done(Message::Artists(views::ArtistsMessage::SlotList(
                crate::widgets::SlotListPageMessage::ActivateCenter,
            ))),
            View::Queue => Task::done(Message::Queue(views::QueueMessage::SlotListActivateCenter)),
            View::Songs => Task::done(Message::Songs(views::SongsMessage::SlotList(
                crate::widgets::SlotListPageMessage::ActivateCenter,
            ))),
            View::Genres => Task::done(Message::Genres(views::GenresMessage::SlotList(
                crate::widgets::SlotListPageMessage::ActivateCenter,
            ))),
            View::Playlists => Task::done(Message::Playlists(
                views::PlaylistsMessage::SlotListActivateCenter,
            )),
            View::Radios => Task::done(Message::Radios(
                views::RadiosMessage::SlotListActivateCenter,
            )),
            View::Settings => Task::done(Message::Settings(views::SettingsMessage::EditActivate)),
        }
    }
}
