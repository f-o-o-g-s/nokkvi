//! Navigation, search, and sort hotkey handlers

use iced::Task;
use nokkvi_data::audio;
use tracing::{debug, trace};

use crate::{Nokkvi, View, app_message::Message, views, widgets};

impl Nokkvi {
    pub(crate) fn handle_clear_search(&mut self) -> Task<Message> {
        trace!(" ClearSearch (Escape) hotkey pressed - unfocusing search");
        // Play escape sound
        self.sfx_engine.play(audio::SfxType::Escape);

        // Default-playlist picker has top priority — closes before any other
        // Escape-handling logic runs, so a stray Esc never bleeds through to
        // the underlying view.
        if self.default_playlist_picker.is_some() {
            return Task::done(Message::DefaultPlaylistPicker(
                crate::widgets::default_playlist_picker::DefaultPlaylistPickerMessage::Close,
            ));
        }

        // Cancel active cross-pane drag first
        if self.cross_pane_drag.is_some() || self.cross_pane_drag_press_origin.is_some() {
            return self.handle_cross_pane_drag_cancel();
        }

        // Settings has its own Escape handling — must be checked before the
        // browsing panel block, because current_view_page() returns None for
        // Settings, making all is_none_or() guards pass true and silently
        // closing the browsing panel instead of exiting settings.
        if self.current_view == View::Settings {
            return Task::done(Message::Settings(views::SettingsMessage::Escape));
        }

        // In playlist edit mode: Escape exits edit mode when search is empty
        // and no expansion is active (i.e. there's nothing else to dismiss).
        // In standalone browsing panel: Escape closes the panel.
        if self.browsing_panel.is_some() {
            let page = self.current_view_page();
            let search_empty = page.is_none_or(|p| p.common().search_query.is_empty());
            let not_expanded = page.is_none_or(|p| !p.is_expanded());
            let not_focused = page.is_none_or(|p| !p.common().search_input_focused);
            if search_empty && not_expanded && not_focused {
                if self.playlist_edit.is_some() {
                    return Task::done(Message::ExitPlaylistEditMode);
                }
                return Task::done(Message::ToggleBrowsingPanel);
            }
        }

        // If the text input dialog is open, Escape always closes it first
        if self.text_input_dialog.visible {
            return Task::done(Message::TextInputDialog(
                crate::widgets::text_input_dialog::TextInputDialogMessage::Cancel,
            ));
        }

        if let Some(page) = self.current_view_page_mut() {
            // If there's an active multi-selection, Escape clears it first
            if !page.common().slot_list.selected_indices.is_empty() {
                page.common_mut().slot_list.selected_indices.clear();
                page.common_mut().slot_list.anchor_index = None;
                return Task::none();
            }

            // If expansion is active, Escape collapses it first
            if page.is_expanded()
                && let Some(msg) = page.collapse_expansion_message()
            {
                return Task::done(msg);
            }
            page.common_mut().search_input_focused = false;
            // If search is empty, reload to ensure full list is shown
            if page.common().search_query.is_empty()
                && let Some(msg) = page.reload_message()
            {
                return Task::done(msg);
            }
        }
        Task::none()
    }

    /// Handle edit value up/down hotkey (default: bare ↑/↓).
    ///
    /// In Settings, routes to `SlotListUp`/`SlotListDown` which the toggle-cursor
    /// logic intercepts to enable/disable the cursored ToggleSet badge.
    /// In other views this is a no-op (arrow up/down are dedicated to MoveTrack
    /// via Shift+Arrow).
    pub(crate) fn handle_edit_value(&mut self, up: bool) -> Task<Message> {
        if self.current_view == View::Settings {
            return if up {
                Task::done(Message::Settings(views::SettingsMessage::SlotListUp))
            } else {
                Task::done(Message::Settings(views::SettingsMessage::SlotListDown))
            };
        }
        Task::none()
    }

    pub(crate) fn handle_cycle_sort_mode(&mut self, forward: bool) -> Task<Message> {
        // Play backspace navigation sound for combobox cycling (settings handles its own SFX)
        if self.current_view != View::Settings {
            self.sfx_engine.play(audio::SfxType::Backspace);
        }

        use widgets::view_header::SortMode;

        // Settings routes Left/Right to its edit mode
        if self.current_view == View::Settings {
            return if forward {
                Task::done(Message::Settings(views::SettingsMessage::EditRight))
            } else {
                Task::done(Message::Settings(views::SettingsMessage::EditLeft))
            };
        }

        // Queue uses QueueSortMode (separate enum), handle it explicitly
        if self.current_view == View::Queue {
            self.queue_page.common.search_input_focused = false;
            use views::QueueSortMode;
            let types = QueueSortMode::all();
            let current_idx = types
                .iter()
                .position(|t| *t == self.queue_page.queue_sort_mode)
                .unwrap_or(0);
            let new_idx = if forward {
                (current_idx + 1) % types.len()
            } else {
                (current_idx + types.len() - 1) % types.len()
            };
            debug!(
                "🔄 CycleSortMode (Queue): {:?} -> {:?}",
                self.queue_page.queue_sort_mode, types[new_idx]
            );
            return Task::done(Message::Queue(views::QueueMessage::SortModeSelected(
                types[new_idx],
            )));
        }

        // Standard views: use ViewPage trait dispatch
        let current_view = self.current_view;
        if let Some(page) = self.current_view_page_mut() {
            // INVARIANT: If this handler runs, text_input is NOT focused (it would have consumed arrow keys)
            // Clear stale flag from Escape-unfocused state
            page.common_mut().search_input_focused = false;
            if let Some(options) = page.sort_mode_options() {
                let current = page.common().current_sort_mode;
                let new_type = SortMode::cycle(current, options, forward);
                debug!(
                    "🔄 CycleSortMode ({:?}): {:?} -> {:?}",
                    current_view, current, new_type
                );
                return if let Some(msg) = page.sort_mode_selected_message(new_type) {
                    Task::done(msg)
                } else {
                    Task::none()
                };
            }
        }
        Task::none()
    }

    pub(crate) fn handle_center_on_playing(&mut self) -> Task<Message> {
        trace!(" CenterOnPlaying hotkey pressed on {:?}", self.current_view);

        // Get the current song ID from scrobble state (already tracked)
        let song_id = match self.scrobble.current_song_id.as_deref() {
            Some(id) => id,
            None => {
                trace!(" CenterOnPlaying: No current song playing");
                self.toast_info("No song is currently playing");
                return Task::none();
            }
        };

        // Look up the current song in queue for album_id/artist/genre fields
        let current_queue_song = self.library.queue_songs.iter().find(|s| s.id == song_id);

        let idx = match self.current_view {
            View::Queue => {
                // Use filtered queue since slot list is sized to filtered results
                let filtered = self.filter_queue_songs();
                filtered.iter().position(|s| s.id == song_id)
            }
            View::Albums => {
                // Collapse any expansion before centering
                self.albums_page.expansion.clear();
                current_queue_song
                    .and_then(|qs| self.library.albums.iter().position(|a| a.id == qs.album_id))
            }
            View::Artists => {
                // Collapse any expansion before centering
                self.artists_page.expansion.clear();
                // Match by artist name (queue songs have name, not artist_id)
                current_queue_song.and_then(|qs| {
                    self.library
                        .artists
                        .iter()
                        .position(|a| a.name == qs.artist)
                })
            }
            View::Songs => self.library.songs.iter().position(|s| s.id == song_id),
            View::Genres => current_queue_song.and_then(|qs| {
                self.library
                    .genres
                    .iter()
                    .position(|g| g.name.eq_ignore_ascii_case(&qs.genre))
            }),
            View::Radios => {
                if let crate::state::ActivePlayback::Radio(radio_state) = &self.active_playback {
                    self.library
                        .radio_stations
                        .iter()
                        .position(|r| r.id == radio_state.station.id)
                } else {
                    None
                }
            }
            View::Playlists | View::Settings => {
                // No meaningful match for playlists or settings
                trace!(" CenterOnPlaying: No-op for {:?} view", self.current_view);
                return Task::none();
            }
        };

        if let Some(i) = idx {
            debug!(
                " CenterOnPlaying: Found item at index {} in {:?}",
                i, self.current_view
            );
            // Directly set the viewport offset on the page's slot list.
            // We can't use SlotListMessage::SetOffset because that routes through
            // handle_select_offset (click-to-highlight) when stable_viewport is on.
            // CenterOnPlaying is a deliberate user action that must always scroll.
            // Set the offset directly, then dispatch the page's SlotListSetOffset so
            // the handler's post-update artwork loading code still runs (mini +
            // large artwork from disk cache, network fetch, pagination, etc.).
            //
            // Clear any active multi-selection first — CenterOnPlaying is a deliberate
            // navigation action and stale selected_indices would keep
            // `has_multi_selection` true, suppressing the center slot highlight.
            if let Some(page) = self.current_view_page_mut() {
                page.common_mut().clear_multi_selection();
            }
            match self.current_view {
                View::Queue => {
                    let total = self.filter_queue_songs().len();
                    self.queue_page.common.handle_set_offset(i, total);
                    Task::done(Message::Queue(views::QueueMessage::SlotListSetOffset(
                        i,
                        iced::keyboard::Modifiers::default(),
                    )))
                }
                View::Albums => {
                    self.albums_page.expansion.handle_set_offset(
                        i,
                        &self.library.albums,
                        &mut self.albums_page.common,
                    );
                    Task::done(Message::Albums(views::AlbumsMessage::SlotListSetOffset(
                        i,
                        iced::keyboard::Modifiers::default(),
                    )))
                }
                View::Artists => {
                    self.artists_page.expansion.handle_set_offset(
                        i,
                        &self.library.artists,
                        &mut self.artists_page.common,
                    );
                    Task::done(Message::Artists(views::ArtistsMessage::SlotListSetOffset(
                        i,
                        iced::keyboard::Modifiers::default(),
                    )))
                }
                View::Songs => {
                    let total = self.library.songs.len();
                    self.songs_page.common.handle_set_offset(i, total);
                    Task::done(Message::Songs(views::SongsMessage::SlotListSetOffset(
                        i,
                        iced::keyboard::Modifiers::default(),
                    )))
                }
                View::Genres => {
                    self.genres_page.expansion.handle_set_offset(
                        i,
                        &self.library.genres,
                        &mut self.genres_page.common,
                    );
                    Task::done(Message::Genres(views::GenresMessage::SlotListSetOffset(
                        i,
                        iced::keyboard::Modifiers::default(),
                    )))
                }
                View::Radios => {
                    let total = self.library.radio_stations.len();
                    self.radios_page.common.handle_set_offset(i, total);
                    Task::done(Message::Radios(views::RadiosMessage::SlotListSetOffset(
                        i,
                        iced::keyboard::Modifiers::default(),
                    )))
                }
                _ => Task::none(),
            }
        } else {
            // Item not in loaded buffer — start a center-only find chain that
            // clears the active search, restarts pagination from offset 0,
            // and walks the library until the playing item appears, then
            // centers it without dispatching FocusAndExpand. This avoids
            // overwriting the user's search query (the previous fallback
            // typed the item title into the search box).
            let Some(qs) = current_queue_song else {
                return Task::none();
            };
            debug!(
                " CenterOnPlaying: item not in loaded {:?} buffer — starting center-only find chain",
                self.current_view
            );
            match self.current_view {
                View::Albums => self.start_center_on_playing_album_chain(qs.album_id.clone()),
                View::Artists => self.start_center_on_playing_artist_chain(qs.artist_id.clone()),
                View::Songs => self.start_center_on_playing_song_chain(song_id.to_string()),
                View::Genres => self.start_center_on_playing_genre_chain(qs.genre.clone()),
                _ => Task::none(),
            }
        }
    }

    pub(crate) fn handle_focus_search(&mut self) -> Task<Message> {
        trace!(" FocusSearch (/) hotkey pressed - focusing search field");

        // Settings has its own search — must be checked before current_view_page_mut()
        // which would incorrectly route to the browsing panel's page when it's open
        // with browser focus (same priority pattern as handle_clear_search).
        if self.current_view == View::Settings && self.settings_page.font_sub_list.is_some() {
            // Font picker is open — focus its search field
            return iced::widget::operation::focus(crate::views::settings::FONT_SEARCH_INPUT_ID);
        } else if self.current_view == View::Settings {
            // Toggle search overlay and focus the input
            let toggle = Task::done(Message::Settings(views::SettingsMessage::ToggleSearch));
            let focus =
                iced::widget::operation::focus(crate::views::settings::SETTINGS_SEARCH_INPUT_ID);
            return Task::batch([toggle, focus]);
        }

        if let Some(page) = self.current_view_page_mut() {
            let search_id = page.search_input_id();
            page.common_mut().search_input_focused = true;
            iced::widget::operation::focus(search_id)
        } else {
            Task::none()
        }
    }

    pub(crate) fn handle_toggle_sort_order(&mut self) -> Task<Message> {
        self.sfx_engine.play(audio::SfxType::Backspace);
        if let Some(page) = self.current_view_page() {
            let ascending = page.common().sort_ascending;
            debug!(
                "🔄 ToggleSortOrder ({:?}): {} -> {}",
                self.current_view,
                if ascending { "ASC" } else { "DESC" },
                if !ascending { "ASC" } else { "DESC" }
            );
            Task::done(page.toggle_sort_order_message())
        } else {
            Task::none()
        }
    }
}
