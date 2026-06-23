//! Slot list navigation handlers

use iced::Task;
use nokkvi_data::audio;
use tracing::debug;

use crate::{
    Nokkvi, View,
    app_message::{Message, RouletteMessage, SlotListMessage},
    views,
};

impl Nokkvi {
    /// Top-level slot list message dispatcher
    pub(crate) fn handle_slot_list_message(&mut self, msg: SlotListMessage) -> Task<Message> {
        // Roulette in cruise: Enter is what stops the wheel and rolls the
        // landing target. Intercept here so the keypress never reaches the
        // page's ActivateCenter (which would play whatever is mid-spin in
        // the center slot). During the decel walk Enter is swallowed —
        // the spin is committed and the user shouldn't be able to fire a
        // play against a moving target.
        if let Some(state) = self.roulette.as_ref()
            && matches!(
                msg,
                SlotListMessage::ActivateCenter | SlotListMessage::ActivateCenterShuffled
            )
        {
            return if state.decel.is_none() {
                Task::done(Message::Roulette(RouletteMessage::Stop))
            } else {
                Task::none()
            };
        }

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
            SlotListMessage::ActivateCenter => self.handle_slot_list_activate_center(false),
            SlotListMessage::ActivateCenterShuffled => self.handle_slot_list_activate_center(true),
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
        // Settings has no artwork; the playlist editor routes slot events
        // through `EditorMessage::SlotList` and never arms a seek timer —
        // both no-op.
        match view {
            View::Queue => self.load_queue_viewport_artwork(),
            View::Settings | View::PlaylistEditor => Task::none(),
            View::Albums
            | View::Artists
            | View::Songs
            | View::Genres
            | View::Playlists
            | View::Radios => {
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
            self.current_target_view()
        );
        // Play tab navigation sound
        self.sfx_engine.play(audio::SfxType::Tab);
        // Settings is never a browser tab, so a Settings early-return is only
        // reachable when it is the foreground (non-pane) view.
        if self.current_target_view() == Some(View::Settings) {
            return Task::done(Message::Settings(views::SettingsMessage::SlotListUp));
        }
        // Dispatch through the pane-aware accessor so the focused browser tab
        // (incl. Similar) receives the nav, not the host pane.
        self.current_view_page().map_or_else(Task::none, |p| {
            Task::done(p.slot_list_message(crate::widgets::SlotListPageMessage::NavigateUp))
        })
    }

    pub(crate) fn handle_slot_list_navigate_down(&mut self) -> Task<Message> {
        // If search is focused, unfocus it as a side-effect of navigating.
        // SlotListDown (default: Tab) doubles as "exit search" — similar to Escape,
        // but also navigates the slot list in the same keypress.
        // We intentionally don't do this for SlotListUp (default: Backspace) because
        // Backspace is needed for deleting text in the search field.
        let target_view = self.current_target_view();
        let unfocus_task = if target_view == Some(View::Settings) {
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
            target_view
        );
        // Play tab navigation sound
        self.sfx_engine.play(audio::SfxType::Tab);
        let nav_task = if target_view == Some(View::Settings) {
            Task::done(Message::Settings(views::SettingsMessage::SlotListDown))
        } else {
            self.current_view_page().map_or_else(Task::none, |p| {
                Task::done(p.slot_list_message(crate::widgets::SlotListPageMessage::NavigateDown))
            })
        };
        Task::batch([unfocus_task, nav_task])
    }

    fn handle_slot_list_set_offset(&mut self, offset: usize) -> Task<Message> {
        if self.current_target_view() == Some(View::Settings) {
            return Task::done(Message::Settings(
                views::SettingsMessage::SlotListSetOffset(
                    offset,
                    iced::keyboard::Modifiers::default(),
                ),
            ));
        }
        self.current_view_page().map_or_else(Task::none, |p| {
            Task::done(
                p.slot_list_message(crate::widgets::SlotListPageMessage::SetOffset(
                    offset,
                    iced::keyboard::Modifiers::default(),
                )),
            )
        })
    }

    fn handle_slot_list_activate_center(&mut self, shuffled: bool) -> Task<Message> {
        let target_view = self.current_target_view();
        // Play enter/activate sound (settings handles its own SFX)
        if target_view != Some(View::Settings) {
            // Check if the focused list is empty (accounts for search filtering).
            // We use the same filter_* helpers as view() to detect when the
            // results are empty even if the raw library is not. `None` means
            // the Similar browser tab is focused — read its songs list so the
            // SFX matches the focused list rather than the host view.
            let is_empty = match target_view {
                Some(View::Queue) => self.filter_queue_songs().is_empty(),
                Some(View::Albums) => self.filter_albums().is_empty(),
                Some(View::Songs) => {
                    // Filter songs manually since there's no main.rs helper for it yet
                    nokkvi_data::utils::search::filter_items(
                        &self.library.songs,
                        &self.songs_page.common.search_query,
                    )
                    .is_empty()
                }
                Some(View::Artists) => nokkvi_data::utils::search::filter_items(
                    &self.library.artists,
                    &self.artists_page.common.search_query,
                )
                .is_empty(),
                Some(View::Genres) => nokkvi_data::utils::search::filter_items(
                    &self.library.genres,
                    &self.genres_page.common.search_query,
                )
                .is_empty(),
                Some(View::Playlists) => nokkvi_data::utils::search::filter_items(
                    &self.library.playlists,
                    &self.playlists_page.common.search_query,
                )
                .is_empty(),
                Some(View::Radios) => self.filter_radio_stations().is_empty(),
                Some(View::Settings) => false,
                Some(View::PlaylistEditor) => self
                    .playlist_editor
                    .as_ref()
                    .is_none_or(|e| e.songs.is_empty()),
                // Similar browser tab focused (no `View` variant).
                None => self
                    .similar_songs
                    .as_ref()
                    .is_none_or(|s| s.songs.is_empty()),
            };

            if is_empty {
                self.sfx_engine.play(audio::SfxType::Escape);
            } else {
                self.sfx_engine.play(audio::SfxType::Enter);
            }
        }
        if target_view == Some(View::Settings) {
            return Task::done(Message::Settings(views::SettingsMessage::EditActivate));
        }
        let activate = if shuffled {
            crate::widgets::SlotListPageMessage::ActivateCenterShuffled
        } else {
            crate::widgets::SlotListPageMessage::ActivateCenter
        };
        self.current_view_page()
            .map_or_else(Task::none, |p| Task::done(p.slot_list_message(activate)))
    }
}
