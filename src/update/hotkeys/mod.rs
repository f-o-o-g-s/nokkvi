//! Hotkey action handlers
//!
//! Split into domain-specific submodules:
//! - `star_rating`: Star/favorite and rating handlers
//! - `queue`: Queue management (add, remove, clear, shuffle, save, move)
//! - `navigation`: Search, sort, and center-on-playing

mod navigation;
mod queue;
mod star_rating;

use iced::Task;
use nokkvi_data::types::info_modal::InfoModalItem;
use tracing::{debug, trace};

use crate::{
    Nokkvi, View,
    app_message::{HotkeyMessage, Message},
    views,
    views::expansion::SlotListEntry,
};

impl Nokkvi {
    /// Get the current view as a `&dyn ViewPage` for trait-based dispatch.
    /// Returns None for Settings (which doesn't implement ViewPage).
    ///
    /// In playlist edit mode with browser focus, returns the browsing panel's
    /// active view page so all existing hotkey handlers work on the browser pane.
    pub(crate) fn current_view_page(&self) -> Option<&dyn views::ViewPage> {
        // Pane-aware routing: when editing with browser focus, delegate to the active tab
        if self.browsing_panel.is_some()
            && self.pane_focus == crate::state::PaneFocus::Browser
            && let Some(panel) = &self.browsing_panel
        {
            return match panel.active_view {
                views::BrowsingView::Albums => Some(&self.albums_page),
                views::BrowsingView::Songs => Some(&self.songs_page),
                views::BrowsingView::Artists => Some(&self.artists_page),
                views::BrowsingView::Genres => Some(&self.genres_page),
                views::BrowsingView::Similar => Some(&self.similar_page),
            };
        }

        self.view_page(self.current_view)
    }

    /// Get the current view as a `&mut dyn ViewPage` for trait-based dispatch.
    /// Returns None for Settings (which doesn't implement ViewPage).
    ///
    /// In playlist edit mode with browser focus, returns the browsing panel's
    /// active view page so all existing hotkey handlers work on the browser pane.
    pub(crate) fn current_view_page_mut(&mut self) -> Option<&mut dyn views::ViewPage> {
        // Pane-aware routing: when editing with browser focus, delegate to the active tab
        if self.browsing_panel.is_some()
            && self.pane_focus == crate::state::PaneFocus::Browser
            && let Some(panel) = &self.browsing_panel
        {
            return match panel.active_view {
                views::BrowsingView::Albums => Some(&mut self.albums_page),
                views::BrowsingView::Songs => Some(&mut self.songs_page),
                views::BrowsingView::Artists => Some(&mut self.artists_page),
                views::BrowsingView::Genres => Some(&mut self.genres_page),
                views::BrowsingView::Similar => Some(&mut self.similar_page),
            };
        }

        self.view_page_mut(self.current_view)
    }

    /// Resolve the `View` whose slot list the keyboard is currently steering,
    /// accounting for the split-view browsing panel.
    ///
    /// When the browsing panel is open with browser focus, the focused list is
    /// the panel's active tab — not `self.current_view` (which is pinned to the
    /// host view, e.g. `View::PlaylistEditor` during playlist edit). Maps each
    /// non-`Similar` browser tab to its `View` counterpart.
    ///
    /// Returns `None` when the focused tab is `BrowsingView::Similar`: the
    /// `View` enum has no `Similar` variant, so callers that need a concrete
    /// `View` must treat `None` as "Similar is focused" (e.g. roulette is
    /// intentionally unsupported there). Trait-based dispatch should prefer
    /// `current_view_page()` / `current_view_page_mut()`, which cover Similar.
    pub(crate) fn current_target_view(&self) -> Option<View> {
        if self.pane_focus == crate::state::PaneFocus::Browser
            && let Some(panel) = self.browsing_panel.as_ref()
        {
            return match panel.active_view {
                views::BrowsingView::Albums => Some(View::Albums),
                views::BrowsingView::Songs => Some(View::Songs),
                views::BrowsingView::Artists => Some(View::Artists),
                views::BrowsingView::Genres => Some(View::Genres),
                views::BrowsingView::Similar => None,
            };
        }
        Some(self.current_view)
    }

    /// Look up a page by explicit `View` — no pane-focus routing.
    /// Used by scrollbar timer handlers that always target a specific view.
    pub(crate) fn view_page(&self, view: View) -> Option<&dyn views::ViewPage> {
        match view {
            View::Albums => Some(&self.albums_page),
            View::Artists => Some(&self.artists_page),
            View::Songs => Some(&self.songs_page),
            View::Genres => Some(&self.genres_page),
            View::Playlists => Some(&self.playlists_page),
            View::Queue => Some(&self.queue_page),
            View::Radios => Some(&self.radios_page),
            // No `ViewPage` impl — the editor routes its slot events through
            // `EditorMessage::SlotList`, not the generic page dispatch.
            View::Settings | View::PlaylistEditor => None,
        }
    }

    /// Look up a page by explicit `View` (mutable) — no pane-focus routing.
    pub(crate) fn view_page_mut(&mut self, view: View) -> Option<&mut dyn views::ViewPage> {
        match view {
            View::Albums => Some(&mut self.albums_page),
            View::Artists => Some(&mut self.artists_page),
            View::Songs => Some(&mut self.songs_page),
            View::Genres => Some(&mut self.genres_page),
            View::Playlists => Some(&mut self.playlists_page),
            View::Queue => Some(&mut self.queue_page),
            View::Radios => Some(&mut self.radios_page),
            View::Settings | View::PlaylistEditor => None,
        }
    }

    /// Handle Get Info hotkey (Shift+I): open info modal for the centered item.
    /// Supports Songs, Albums (parent + child), Artists (three-tier), Playlists (parent + child), and Queue.
    pub(crate) fn handle_get_info(&mut self) -> Task<Message> {
        debug!("ℹ️ GetInfo (Shift+I) hotkey pressed");

        // Toggle: if the modal is already open, close it
        if self.info_modal.visible {
            return self.update(Message::InfoModal(
                crate::widgets::info_modal::InfoModalMessage::Close,
            ));
        }

        #[allow(clippy::collapsible_if)]
        if self.pane_focus == crate::state::PaneFocus::Browser
            && let Some(panel) = self.browsing_panel.as_ref()
            && panel.active_view == crate::views::BrowsingView::Similar
        {
            if let Some(similar) = &self.similar_songs {
                let center_idx = self
                    .similar_page
                    .common
                    .slot_list
                    .get_center_item_index(similar.songs.len());
                if let Some(song) = center_idx.and_then(|idx| similar.songs.get(idx)) {
                    let item = InfoModalItem::from_song(song);
                    return self.update(Message::InfoModal(
                        crate::widgets::info_modal::InfoModalMessage::Open(Box::new(item)),
                    ));
                }
            }
        }
        // Resolve the focused list: under browser focus this is the active tab
        // (Albums/Songs/Artists/Genres), not `self.current_view` (the host view,
        // e.g. PlaylistEditor during edit). The Similar tab is handled by the
        // short-circuit above; `current_target_view()` returns None for it so
        // the `unwrap_or` falls back to the host view, which the catch-all
        // arm reports as "not available" — correct for Genres/Playlists hosts.
        let effective_view = self.current_target_view().unwrap_or(self.current_view);
        match effective_view {
            View::Songs => {
                let center_idx = self
                    .songs_page
                    .common
                    .slot_list
                    .get_center_item_index(self.library.songs.len());
                if let Some(song) = center_idx.and_then(|idx| self.library.songs.get(idx)) {
                    let item = InfoModalItem::from_song_view_data(song);
                    return self.update(Message::InfoModal(
                        crate::widgets::info_modal::InfoModalMessage::Open(Box::new(item)),
                    ));
                }
            }
            View::Albums => {
                let total = self
                    .albums_page
                    .expansion
                    .flattened_len(&self.library.albums);
                let center_idx = self
                    .albums_page
                    .common
                    .slot_list
                    .get_center_item_index(total);
                if let Some(entry) = center_idx.and_then(|idx| {
                    self.albums_page
                        .expansion
                        .get_entry_at(idx, &self.library.albums, |a| &a.id)
                }) {
                    let item = match entry {
                        SlotListEntry::Child(song, _) => InfoModalItem::from_song_view_data(song),
                        SlotListEntry::Parent(album) => InfoModalItem::Album {
                            name: album.name.clone(),
                            album_artist: Some(album.artist.clone()),
                            release_type: album.release_type.clone(),
                            genre: album.genre.clone(),
                            genres: album.genres.clone(),
                            duration: album.duration,
                            year: album.year,
                            song_count: Some(album.song_count),
                            compilation: album.compilation,
                            size: album.size,
                            is_starred: album.is_starred,
                            rating: album.rating,
                            play_count: album.play_count,
                            play_date: album.play_date.clone(),
                            updated_at: album.updated_at.clone(),
                            created_at: album.created_at.clone(),
                            mbz_album_id: album.mbz_album_id.clone(),
                            comment: album.comment.clone(),
                            id: album.id.clone(),
                            tags: album.tags.clone(),
                            participants: album.participants.clone(),
                            representative_path: self
                                .albums_page
                                .expansion
                                .children
                                .first()
                                .map(|s| s.path.clone()),
                        },
                    };
                    return self.update(Message::InfoModal(
                        crate::widgets::info_modal::InfoModalMessage::Open(Box::new(item)),
                    ));
                }
            }
            View::Artists => {
                if let Some(entry) = self.artists_page.expansion.resolve_center(
                    &self.library.artists,
                    &self.artists_page.common,
                    |a| &a.id,
                ) {
                    let item = match entry {
                        SlotListEntry::Child(album, _) => InfoModalItem::Album {
                            name: album.name.clone(),
                            album_artist: Some(album.artist.clone()),
                            release_type: album.release_type.clone(),
                            genre: album.genre.clone(),
                            genres: album.genres.clone(),
                            duration: album.duration,
                            year: album.year,
                            song_count: Some(album.song_count),
                            compilation: album.compilation,
                            size: album.size,
                            is_starred: album.is_starred,
                            rating: album.rating,
                            play_count: album.play_count,
                            play_date: album.play_date.clone(),
                            updated_at: album.updated_at.clone(),
                            created_at: album.created_at.clone(),
                            mbz_album_id: album.mbz_album_id.clone(),
                            comment: album.comment.clone(),
                            id: album.id.clone(),
                            tags: album.tags.clone(),
                            participants: album.participants.clone(),
                            representative_path: None,
                        },
                        SlotListEntry::Parent(artist) => InfoModalItem::Artist {
                            name: artist.name.clone(),
                            song_count: Some(artist.song_count),
                            album_count: Some(artist.album_count),
                            is_starred: artist.is_starred,
                            rating: artist.rating,
                            play_count: artist.play_count,
                            play_date: artist.play_date.clone(),
                            size: artist.size,
                            mbz_artist_id: artist.mbz_artist_id.clone(),
                            biography: artist.biography.clone(),
                            external_url: artist.external_url.clone(),
                            id: artist.id.clone(),
                        },
                    };
                    return self.update(Message::InfoModal(
                        crate::widgets::info_modal::InfoModalMessage::Open(Box::new(item)),
                    ));
                }
            }
            View::Playlists => {
                let total = self
                    .playlists_page
                    .expansion
                    .flattened_len(&self.library.playlists);
                let center_idx = self
                    .playlists_page
                    .common
                    .slot_list
                    .get_center_item_index(total);
                if let Some(entry) = center_idx.and_then(|idx| {
                    self.playlists_page
                        .expansion
                        .get_entry_at(idx, &self.library.playlists, |p| &p.id)
                }) {
                    let item = match entry {
                        SlotListEntry::Child(song, _) => InfoModalItem::from_song_view_data(song),
                        SlotListEntry::Parent(playlist) => InfoModalItem::Playlist {
                            name: playlist.name.clone(),
                            comment: playlist.comment.clone(),
                            duration: playlist.duration,
                            song_count: playlist.song_count,
                            size: 0,
                            owner_name: playlist.owner_name.clone(),
                            public: playlist.public,
                            created_at: String::new(),
                            updated_at: playlist.updated_at.clone(),
                            id: playlist.id.clone(),
                        },
                    };
                    return self.update(Message::InfoModal(
                        crate::widgets::info_modal::InfoModalMessage::Open(Box::new(item)),
                    ));
                }
            }
            View::Queue => {
                // Queue uses async API re-fetch for full Song field coverage
                let filtered = self.filter_queue_songs();
                let center_idx = self
                    .queue_page
                    .common
                    .slot_list
                    .get_center_item_index(filtered.len());
                if let Some(song_id) =
                    center_idx.and_then(|idx| filtered.get(idx).map(|s| s.id.clone()))
                {
                    return self.shell_task(
                        move |shell| async move {
                            let api = shell.songs_api().await?;
                            let song = api.load_song_by_id(&song_id).await?;
                            Ok(InfoModalItem::from_song(&song))
                        },
                        |result: Result<InfoModalItem, anyhow::Error>| match result {
                            Ok(item) => Message::InfoModal(
                                crate::widgets::info_modal::InfoModalMessage::Open(Box::new(item)),
                            ),
                            Err(e) => {
                                tracing::error!("Failed to load song info: {e}");
                                Message::Toast(crate::app_message::ToastMessage::Push(
                                    nokkvi_data::types::toast::Toast::new(
                                        format!("Failed to load song info: {e}"),
                                        nokkvi_data::types::toast::ToastLevel::Error,
                                    ),
                                ))
                            }
                        },
                    );
                }
            }
            _ => {
                self.toast_info("Get Info is not available in this view");
                return Task::none();
            }
        }

        self.toast_warn("No item selected");
        Task::none()
    }

    /// Handle Shift+Enter: expand/collapse inline subgroup for center item.
    pub(crate) fn handle_expand_center(&mut self) -> Task<Message> {
        trace!(" ExpandCenter (Shift+Enter) hotkey pressed");
        // Settings uses drill-down navigation, not inline expand/collapse
        if self.current_view == crate::View::Settings {
            return Task::none();
        }
        if let Some(msg) = self
            .current_view_page()
            .and_then(|p| p.expand_center_message())
        {
            return Task::done(msg);
        }
        Task::none()
    }

    /// Dispatch a `HotkeyMessage` to its handler.
    ///
    /// `ClearSearch` runs inline modal-close logic before delegating, since
    /// Escape's job-cascade (close modal first, then clear search) is part
    /// of the dispatch decision rather than belonging to a single handler.
    pub(super) fn dispatch_hotkey(&mut self, msg: HotkeyMessage) -> Task<Message> {
        match msg {
            HotkeyMessage::ClearSearch => {
                // If EQ modal is visible, Escape closes it first
                if self.eq_modal.open {
                    self.eq_modal.open = false;
                    return Task::none();
                }
                // If about modal is visible, Escape closes it first
                if self.about_modal.visible {
                    self.about_modal.close();
                    return Task::none();
                }
                // If info modal is visible, Escape closes it first
                if self.info_modal.visible {
                    self.info_modal.close();
                    return Task::none();
                }
                self.handle_clear_search()
            }
            HotkeyMessage::CycleSortMode(forward) => self.handle_cycle_sort_mode(forward),
            HotkeyMessage::CenterOnPlaying => self.handle_center_on_playing(),
            HotkeyMessage::ToggleStar => self.handle_toggle_star(),
            HotkeyMessage::SongStarredStatusUpdated(song_id, new_starred_status) => {
                self.handle_song_starred_status_updated(song_id, new_starred_status)
            }
            HotkeyMessage::AlbumStarredStatusUpdated(album_id, new_starred_status) => {
                self.handle_album_starred_status_updated(album_id, new_starred_status)
            }
            HotkeyMessage::ArtistStarredStatusUpdated(artist_id, new_starred_status) => {
                self.handle_artist_starred_status_updated(artist_id, new_starred_status)
            }
            HotkeyMessage::AddToQueue => self.handle_add_to_queue(),
            HotkeyMessage::SaveQueueAsPlaylist => self.handle_save_queue_as_playlist(),
            HotkeyMessage::RemoveFromQueue => self.handle_remove_from_queue(),
            HotkeyMessage::ClearQueue => self.handle_clear_queue(),
            HotkeyMessage::FocusSearch => self.handle_focus_search(),
            HotkeyMessage::IncreaseRating => self.handle_increase_rating(),
            HotkeyMessage::DecreaseRating => self.handle_decrease_rating(),
            HotkeyMessage::SongRatingUpdated(song_id, new_rating) => {
                self.handle_song_rating_updated(song_id, new_rating)
            }
            HotkeyMessage::SongPlayCountIncremented(song_id) => {
                self.handle_song_play_count_incremented(song_id)
            }
            HotkeyMessage::AlbumRatingUpdated(album_id, new_rating) => {
                self.handle_album_rating_updated(album_id, new_rating)
            }
            HotkeyMessage::ArtistRatingUpdated(artist_id, new_rating) => {
                self.handle_artist_rating_updated(artist_id, new_rating)
            }
            HotkeyMessage::ExpandCenter => self.handle_expand_center(),
            HotkeyMessage::MoveTrackUp => self.handle_move_track(true),
            HotkeyMessage::MoveTrackDown => self.handle_move_track(false),
            HotkeyMessage::GetInfo => self.handle_get_info(),
            HotkeyMessage::FindSimilar => self.handle_find_similar_for_playing_track(),
            HotkeyMessage::FindTopSongs => self.handle_find_top_songs_for_playing_track(),
            HotkeyMessage::EditValue(up) => self.handle_edit_value(up),
            HotkeyMessage::SettingsCategoryMotion(forward) => {
                self.handle_settings_category_motion(forward)
            }
            HotkeyMessage::RefreshView => self
                .current_view_page()
                .and_then(|p| p.reload_message())
                .map_or_else(Task::none, Task::done),
            HotkeyMessage::StartRoulette => {
                // Resolve the focused list: under browser-pane focus the visible
                // list is the active tab, not self.current_view (pinned to the
                // PlaylistEditor host during edit, whose roulette total is 0).
                // current_target_view() returns None for the Similar tab, which
                // has no roulette support, so the unwrap_or falls back to the
                // host view and the spin stays a no-op there (intended).
                let view = self.current_target_view().unwrap_or(self.current_view);
                self.handle_roulette_message(crate::app_message::RouletteMessage::Start(view))
            }
        }
    }

    /// Translate a raw keyboard event into a hotkey action via the user's
    /// `HotkeyConfig`, or forward it to settings when hotkey-capture mode
    /// is active. Suppresses dispatch when a widget has captured the key
    /// event (typing into a text input), with Escape/Tab/Ctrl+key exceptions.
    pub(super) fn handle_raw_key_event(
        &mut self,
        key: iced::keyboard::Key,
        modifiers: iced::keyboard::Modifiers,
        status: iced::event::Status,
    ) -> Task<Message> {
        // If settings is in hotkey capture mode, forward the raw event there
        // instead of dispatching it as a normal hotkey action
        if self.settings_page.capturing_hotkey.is_some() {
            return self.handle_settings(crate::views::SettingsMessage::HotkeyCaptured(
                key, modifiers,
            ));
        }

        // Escape always reaches the dispatcher: it closes overlays / clears
        // search. Hoisted so both guard blocks below can read it.
        let is_escape = matches!(
            key,
            iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
        );

        // When a widget (e.g. text_input search bar) has captured the
        // key event, suppress hotkey dispatch to avoid triggering actions
        // while the user is typing. Exceptions:
        //   - Escape: always allowed (close overlays, clear search)
        //   - Tab: always allowed (slot-list navigation)
        //   - Ctrl+key: always allowed (intentional shortcuts like Ctrl+S)
        //   - Shift+Tab / Shift+Backspace: allowed for the settings-sidebar
        //     category nav. The exception is scoped to these two named keys
        //     only — plain Shift+character (a capital letter while typing)
        //     must stay suppressed, otherwise e.g. a capital D fires
        //     ClearQueue (destructive) mid-edit.
        if status == iced::event::Status::Captured {
            let is_tab = matches!(
                key,
                iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab)
            );
            let is_shift_nav = modifiers.shift()
                && matches!(
                    key,
                    iced::keyboard::Key::Named(
                        iced::keyboard::key::Named::Tab | iced::keyboard::key::Named::Backspace
                    )
                );
            if !is_escape && !is_tab && !modifiers.control() && !is_shift_nav {
                return Task::none();
            }
        }

        // Look up the key event against the user's hotkey config once — reused
        // by the modal-open guard below and the final dispatch.
        let resolved = crate::hotkeys::handle_hotkey(key, modifiers, &self.hotkey_config);

        // Modal-open suppression: the EQ / Info / About modals and the
        // default-playlist picker are mouse-opaque but not keyboard-capturing
        // and host no focused text_input, so bare-key hotkeys arrive
        // Status::Ignored and would otherwise drive the obscured view (e.g.
        // Space toggling playback behind an open EQ modal). When any blocking
        // modal is open, only Escape passes (it closes the modal via the
        // existing ClearSearch cascade). The picker additionally lets its own
        // slot-list nav keys through — slot_list.rs already routes those to the
        // picker when it is open.
        if self.eq_modal.open
            || self.about_modal.visible
            || self.info_modal.visible
            || self.text_input_dialog.visible
            || self.default_playlist_picker.is_some()
        {
            let is_picker_nav = self.default_playlist_picker.is_some()
                && matches!(
                    resolved,
                    Some(Message::SlotList(
                        crate::app_message::SlotListMessage::NavigateUp
                            | crate::app_message::SlotListMessage::NavigateDown
                            | crate::app_message::SlotListMessage::ActivateCenter
                    ))
                );
            if !is_escape && !is_picker_nav {
                return Task::none();
            }
        }

        // Dispatch the resolved hotkey (Escape + allowed picker nav fall here).
        match resolved {
            Some(msg) => self.update(msg),
            None => Task::none(),
        }
    }

    /// Settings sidebar category motion: forward = next category, backward =
    /// previous. Routes to `SettingsMessage::SidebarDown`/`SidebarUp` when the
    /// settings view is active; no-op everywhere else (the hotkey config can
    /// bind these globally without bleeding into other views).
    pub(crate) fn handle_settings_category_motion(&mut self, forward: bool) -> Task<Message> {
        if self.current_view != View::Settings {
            return Task::none();
        }
        let msg = if forward {
            crate::views::SettingsMessage::SidebarDown
        } else {
            crate::views::SettingsMessage::SidebarUp
        };
        self.handle_settings(msg)
    }

    /// Track the current keyboard modifier state so views can read it
    /// without subscribing to per-event updates themselves.
    pub(super) fn handle_modifiers_changed(
        &mut self,
        modifiers: iced::keyboard::Modifiers,
    ) -> Task<Message> {
        self.window.keyboard_modifiers = modifiers;
        Task::none()
    }
}
