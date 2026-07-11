//! Navigation, search, and sort hotkey handlers

use iced::Task;
use nokkvi_data::audio;
use tracing::{debug, trace};

use crate::{
    Nokkvi, View,
    app_message::{Message, SplitViewMessage},
    views, widgets,
};

impl Nokkvi {
    pub(crate) fn handle_clear_search(&mut self) -> Task<Message> {
        trace!(" ClearSearch (Escape) hotkey pressed - unfocusing search");

        // Roulette spin has highest priority — Escape during a spin
        // restores the original viewport without auto-playing. The cancel
        // handler fires its own Escape SFX so we don't play it twice.
        if self.roulette.is_some() {
            return Task::done(Message::Roulette(
                crate::app_message::RouletteMessage::Cancel,
            ));
        }

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

        // Trawl modal closes at the same overlay tier (after the picker — the
        // tiers agree across all three interception points). The crate itself
        // survives; only the editor closes.
        if self.trawl_modal.is_some() {
            return Task::done(Message::TrawlModal(
                crate::widgets::trawl_modal::TrawlModalMessage::Close,
            ));
        }

        // Cancel active cross-pane drag first
        if self.cross_pane_drag.active.is_some() || self.cross_pane_drag.press_origin.is_some() {
            return self.handle_cross_pane_drag_cancel();
        }

        // Text-input dialog Escape takes precedence over the Settings
        // view's own Escape handler. The dialog can be opened from inside
        // Settings (Save-Playlist-style flows, the local-music-path
        // editor), so routing ESC to Settings first would close the
        // settings drill-down instead of cancelling the prompt the user
        // is actively typing into.
        if self.text_input_dialog.visible {
            return Task::done(Message::TextInputDialog(
                crate::widgets::text_input_dialog::TextInputDialogMessage::Cancel,
            ));
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
                if self.playlist_editor.is_some() {
                    return Task::done(Message::SplitView(SplitViewMessage::ExitEditMode));
                }
                return Task::done(Message::SplitView(SplitViewMessage::ToggleBrowsingPanel));
            }
        }

        if let Some(page) = self.current_view_page_mut() {
            // If there's an active multi-selection, Escape clears it first
            if !page.common().slot_list.selected_indices.is_empty() {
                page.common_mut().slot_list.clear_multi_selection();
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

    /// Reveal the current view's auto-hide toolbar (pane-aware). Called by
    /// header-interacting hotkeys that alter toolbar content (sort cycle/toggle,
    /// focus search) so a keyboard-driven change surfaces the toolbar even when
    /// the cursor isn't on it. Center-on-playing deliberately does NOT call this
    /// — it only scrolls the list. No-op on views without a slot-list page
    /// (e.g. Settings).
    pub(crate) fn reveal_current_toolbar(&mut self) {
        if let Some(page) = self.current_view_page_mut() {
            page.common_mut().reveal_toolbar();
        }
    }

    pub(crate) fn handle_cycle_sort_mode(&mut self, forward: bool) -> Task<Message> {
        // Trawl modal open: Left/Right cycle the focused tray control's value.
        // This branch MUST stay the first statement — one line lower and the
        // OBSCURED background view gets a stray Backspace SFX plus a stranded
        // auto-hide toolbar reveal-lock from the two calls below.
        if self.trawl_modal.is_some() {
            return self.handle_trawl_tray_cycle_value(forward);
        }
        // Play backspace navigation sound for combobox cycling (settings handles its own SFX)
        if self.current_view != View::Settings {
            self.sfx_engine.play(audio::SfxType::Backspace);
        }
        // Surface the auto-hide toolbar for the keyboard-driven sort change.
        self.reveal_current_toolbar();

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
            let target = next_queue_sort_target(
                self.queue_page.queue_sort_mode,
                self.queue_page.queue_sorted,
                forward,
            );
            debug!(
                "🔄 CycleSortMode (Queue): {:?} (sorted={}) -> {:?}",
                self.queue_page.queue_sort_mode, self.queue_page.queue_sorted, target
            );
            return Task::done(Message::Queue(views::QueueMessage::SortModeSelected(
                target,
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
        // Deliberately does NOT reveal the auto-hide toolbar: Shift+C only
        // scrolls the list to centre the playing track — it alters nothing in
        // the toolbar (unlike sort cycle / focus search), so surfacing it is
        // gratuitous. It also avoids stranding the 2.5s reveal window if the
        // user immediately focuses another OS window mid-reveal.

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
            View::Playlists | View::Harbour | View::Settings | View::PlaylistEditor => {
                // No meaningful match for playlists, Harbour, settings, or the editor
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
            // Pre-mutation phase: each view computes its own total / runs
            // its expansion-aware offset adjustment. This stays per-view —
            // not addressed by `slot_list_message`.
            match self.current_view {
                View::Queue => {
                    let total = self.filter_queue_songs().len();
                    self.queue_page.common.handle_set_offset(i, total);
                }
                View::Albums => {
                    self.albums_page.expansion.handle_set_offset(
                        i,
                        &self.library.albums,
                        &mut self.albums_page.common,
                    );
                }
                View::Artists => {
                    self.artists_page.expansion.handle_set_offset(
                        i,
                        &self.library.artists,
                        &mut self.artists_page.common,
                    );
                }
                View::Songs => {
                    let total = self.library.songs.len();
                    self.songs_page.common.handle_set_offset(i, total);
                }
                View::Genres => {
                    self.genres_page.expansion.handle_set_offset(
                        i,
                        &self.library.genres,
                        &mut self.genres_page.common,
                    );
                }
                View::Radios => {
                    let total = self.library.radio_stations.len();
                    self.radios_page.common.handle_set_offset(i, total);
                }
                View::Playlists | View::Harbour | View::Settings | View::PlaylistEditor => {
                    return Task::none();
                }
            }
            // Dispatch phase: uniform via `slot_list_message` so the
            // per-view handler's artwork-loading code still runs.
            self.view_page(self.current_view)
                .map_or_else(Task::none, |p| {
                    Task::done(
                        p.slot_list_message(crate::widgets::SlotListPageMessage::SetOffset(
                            i,
                            iced::keyboard::Modifiers::default(),
                        )),
                    )
                })
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
            let pending = match self.current_view {
                View::Albums => crate::state::PendingExpand::Album {
                    album_id: qs.album_id.clone(),
                    for_browsing_pane: false,
                },
                View::Artists => crate::state::PendingExpand::Artist {
                    artist_id: qs.artist_id.clone(),
                    for_browsing_pane: false,
                },
                View::Songs => crate::state::PendingExpand::Song {
                    song_id: song_id.to_string(),
                    for_browsing_pane: false,
                },
                View::Genres => crate::state::PendingExpand::Genre {
                    genre_id: qs.genre.clone(),
                    for_browsing_pane: false,
                },
                View::Queue
                | View::Playlists
                | View::Radios
                | View::Harbour
                | View::Settings
                | View::PlaylistEditor => {
                    return Task::none();
                }
            };
            self.start_center_on_playing_chain(pending)
        }
    }

    pub(crate) fn handle_focus_search(&mut self) -> Task<Message> {
        trace!(" FocusSearch (/) hotkey pressed - focusing search field");
        // Trawl modal open — `/` returns the keyboard to its search field
        // (the complement of Tab-to-exit; without it a keyboard user who
        // tabbed into the list can't type again without the mouse). The tray
        // focus ring clears with it: the ring must never show while the
        // search field owns the arrow keys. This branch sits ABOVE the
        // toolbar reveal below — the modal's search field always renders, and
        // revealing the OBSCURED view's toolbar would strand a stateful
        // reveal-lock on it (same class as the tray branches' first-statement
        // rule in handle_cycle_sort_mode / handle_settings_category_motion).
        if let Some(state) = self.trawl_modal.as_mut() {
            state.search_input_focused = true;
            state.tray_cursor = None;
            return iced::widget::operation::focus(
                crate::widgets::trawl_modal::TRAWL_SEARCH_INPUT_ID,
            );
        }

        // Surface the auto-hide toolbar so the search field exists to receive
        // focus (the focus operation below targets a widget that only renders
        // while the toolbar is revealed). No-op for Settings (its own search).
        self.reveal_current_toolbar();

        // Settings has its own search — must be checked before current_view_page_mut()
        // which would incorrectly route to the browsing panel's page when it's open
        // with browser focus (same priority pattern as handle_clear_search).

        if self.current_view == View::Settings && self.settings_page.theme_sub_list.is_some() {
            // Theme picker is open — focus its search field
            return iced::widget::operation::focus(crate::views::settings::THEME_SEARCH_INPUT_ID);
        } else if self.current_view == View::Settings && self.settings_page.font_sub_list.is_some()
        {
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
        // Surface the auto-hide toolbar for the keyboard-driven sort change.
        self.reveal_current_toolbar();
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

/// Pure: the queue sort mode a cycle-sort hotkey press should apply.
///
/// While unsorted with a *deterministic* remembered mode, the first press
/// applies it (the mode shown grayed as "Unsorted") rather than skipping ahead.
/// Otherwise — already sorted, or unsorted with `Random` remembered (re-applying
/// `Random` would only reshuffle and stay unsorted, looping forever) — it
/// advances to the adjacent mode in `QueueSortMode::all()` order.
fn next_queue_sort_target(
    current: views::QueueSortMode,
    sorted: bool,
    forward: bool,
) -> views::QueueSortMode {
    use views::QueueSortMode;

    if !sorted && current != QueueSortMode::Random {
        return current;
    }
    nokkvi_data::utils::cycle::cycle_wrapping(&QueueSortMode::all(), current, forward)
}

#[cfg(test)]
mod tests {
    use super::next_queue_sort_target;
    use crate::views::QueueSortMode;

    #[test]
    fn unsorted_deterministic_first_press_applies_remembered_mode() {
        assert_eq!(
            next_queue_sort_target(QueueSortMode::Title, false, true),
            QueueSortMode::Title
        );
        assert_eq!(
            next_queue_sort_target(QueueSortMode::Rating, false, false),
            QueueSortMode::Rating
        );
    }

    #[test]
    fn sorted_advances_to_adjacent_mode() {
        let all = QueueSortMode::all();
        let album_idx = all.iter().position(|m| *m == QueueSortMode::Album).unwrap();
        assert_eq!(
            next_queue_sort_target(QueueSortMode::Album, true, true),
            all[(album_idx + 1) % all.len()]
        );
        assert_eq!(
            next_queue_sort_target(QueueSortMode::Album, true, false),
            all[(album_idx + all.len() - 1) % all.len()]
        );
    }

    #[test]
    fn unsorted_random_remembered_advances_instead_of_reshuffling() {
        // Regression (review finding): unsorted + Random must NOT re-apply
        // Random (which would reshuffle and loop forever) — it advances to a
        // deterministic neighbor.
        assert_ne!(
            next_queue_sort_target(QueueSortMode::Random, false, true),
            QueueSortMode::Random,
            "forward press must not loop on Random"
        );
        assert_ne!(
            next_queue_sort_target(QueueSortMode::Random, false, false),
            QueueSortMode::Random,
            "backward press must not loop on Random"
        );
    }
}
