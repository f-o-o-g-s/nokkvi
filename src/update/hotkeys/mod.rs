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
            View::Harbour => Some(&self.harbour_page),
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
            View::Harbour => Some(&mut self.harbour_page),
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
        // the `unwrap_or` falls back to the host view, which the enumerated
        // not-available arm reports as such — correct for Genres/Playlists hosts.
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
                        SlotListEntry::Parent(album) => InfoModalItem::from_album_view_data(
                            album,
                            self.albums_page
                                .expansion
                                .children
                                .first()
                                .map(|s| s.path.clone()),
                        ),
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
                        SlotListEntry::Child(album, _) => {
                            InfoModalItem::from_album_view_data(album, None)
                        }
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
            // Exhaustive on purpose — a new view must decide whether Get Info
            // applies to it rather than silently landing here.
            View::Genres | View::Radios | View::Harbour | View::Settings | View::PlaylistEditor => {
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
            HotkeyMessage::EditCenteredPlaylist => self.handle_edit_centered_playlist(),
            HotkeyMessage::TrawlSaveAsPlaylist => self.handle_trawl_save_as_playlist_hotkey(),
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

    /// The rules-session keyboard grammar — the three-mode machine
    /// (cursor / editing / JSON) plus the sub-pickers' modal-grade key
    /// ownership. Returns `Some(task)` when the session owned the key
    /// (including deliberate swallows) and `None` to fall through to
    /// normal hotkey dispatch (Space→PlayPause etc. keep working from
    /// cursor mode — the session is a view, not a modal).
    fn rules_session_key_intercept(
        &mut self,
        key: &iced::keyboard::Key,
        modifiers: iced::keyboard::Modifiers,
        status: iced::event::Status,
        resolved: &Option<Message>,
    ) -> Option<Task<Message>> {
        use iced::keyboard::key::Named;

        use crate::app_message::{HotkeyMessage, RulesEditorMessage as R, SlotListMessage};

        if self.current_view != crate::View::PlaylistEditor {
            return None;
        }
        let session = self.rules_session()?;
        let mode = session.mode;
        let pane = session.pane;
        let is_blank_create = session.is_blank_create();
        let picker_open = session.sub_picker.is_some();
        let date_picker = matches!(
            session.sub_picker.as_ref().map(|p| &p.kind),
            Some(crate::state::SubPickerKind::DateValue { .. })
        );
        let json_mode = session.json.is_some();
        let is_edit_target = matches!(
            session.target,
            nokkvi_data::types::rules_session::RulesTarget::Edit { .. }
        );
        let _ = is_edit_target;

        let named = match key {
            iced::keyboard::Key::Named(n) => Some(*n),
            iced::keyboard::Key::Character(_) | iced::keyboard::Key::Unidentified => None,
        };
        let is_char = |c: &str| matches!(key, iced::keyboard::Key::Character(s) if s.as_str().eq_ignore_ascii_case(c));
        let captured = status == iced::event::Status::Captured;
        let swallow = || Some(Task::none());

        // --- Sub-picker open: modal-grade ownership -----------------------
        // (a) the outer-gate rule: a non-nav resolved hotkey (t→OpenTrawl,
        // Space→PlayPause) must never fire against the obscured form;
        // (b) Up/Down/Enter/Escape drive the picker. Captured keys already
        // typed into the picker's search input — swallowed, never
        // double-handled (one press, one meaning).
        if picker_open {
            // A calendar date picker has no text search input, so the
            // captured/uncaptured split (which exists only because a text_input
            // swallows typed keys) does not apply — route all its keys
            // uniformly. Left/Right = ±1 day, Up/Down = ∓1 week, PageUp/Down =
            // month, Enter commits the focused day, Escape closes. Still inside
            // the picker_open gate so the outer-gate rule holds.
            if date_picker {
                return match named {
                    Some(Named::ArrowLeft) => {
                        Some(self.update(Message::RulesEditor(R::DatePickerMoveDay { by: -1 })))
                    }
                    Some(Named::ArrowRight) => {
                        Some(self.update(Message::RulesEditor(R::DatePickerMoveDay { by: 1 })))
                    }
                    Some(Named::ArrowUp) => {
                        Some(self.update(Message::RulesEditor(R::DatePickerMoveDay { by: -7 })))
                    }
                    Some(Named::ArrowDown) => {
                        Some(self.update(Message::RulesEditor(R::DatePickerMoveDay { by: 7 })))
                    }
                    Some(Named::PageUp) => {
                        Some(self.update(Message::RulesEditor(R::DatePickerShiftMonth {
                            forward: false,
                        })))
                    }
                    Some(Named::PageDown) => {
                        Some(self.update(Message::RulesEditor(R::DatePickerShiftMonth {
                            forward: true,
                        })))
                    }
                    Some(Named::Enter) => {
                        Some(self.update(Message::RulesEditor(R::DatePickerCommit)))
                    }
                    Some(Named::Escape) => {
                        Some(self.update(Message::RulesEditor(R::SubPickerCancel)))
                    }
                    _ => swallow(),
                };
            }
            if captured {
                return match named {
                    Some(Named::Escape) => {
                        Some(self.update(Message::RulesEditor(R::SubPickerCancel)))
                    }
                    // Enter on the focused input commits via on_submit —
                    // the raw event must not double-commit.
                    _ => swallow(),
                };
            }
            return match named {
                Some(Named::ArrowUp) => {
                    Some(self.update(Message::RulesEditor(R::SubPickerMove { down: false })))
                }
                Some(Named::ArrowDown) => {
                    Some(self.update(Message::RulesEditor(R::SubPickerMove { down: true })))
                }
                Some(Named::Enter) => Some(self.update(Message::RulesEditor(R::SubPickerCommit))),
                Some(Named::Escape) => Some(self.update(Message::RulesEditor(R::SubPickerCancel))),
                _ => swallow(),
            };
        }

        // --- JSON mode: the text_editor owns most keys --------------------
        if json_mode {
            if named == Some(Named::Escape) {
                return Some(self.update(Message::RulesEditor(R::JsonEscape)));
            }
            // Ctrl+Enter: apply the parse, then preview only when clean.
            if named == Some(Named::Enter) && modifiers.control() {
                let apply = self.update(Message::RulesEditor(R::JsonEscape));
                let clean = self.rules_session().is_some_and(|s| s.json.is_none());
                if clean {
                    let preview = self.update(Message::RulesEditor(R::Preview));
                    return Some(Task::batch([apply, preview]));
                }
                return Some(apply);
            }
            // Everything else belongs to the editor (captured) or is
            // swallowed so no hotkey fires under the JSON surface.
            return swallow();
        }

        // --- Editing mode: a value/name input is focused ------------------
        if mode == crate::state::FormMode::Editing {
            if captured {
                return match named {
                    // Escape reverts the CELL only — never the session.
                    Some(Named::Escape) => {
                        Some(self.update(Message::RulesEditor(R::RevertEditing)))
                    }
                    // Enter commits via on_submit; the raw event is
                    // swallowed to avoid the double-commit.
                    _ => swallow(),
                };
            }
            return match named {
                // Tab commits too ("Enter or Tab commits").
                Some(Named::Tab) if !modifiers.shift() => {
                    Some(self.update(Message::RulesEditor(R::CommitEditing)))
                }
                Some(Named::Escape) => Some(self.update(Message::RulesEditor(R::RevertEditing))),
                Some(Named::Enter) => Some(self.update(Message::RulesEditor(R::CommitEditing))),
                // An uncaptured key while Editing means focus was lost
                // (mouse click elsewhere) — commit, then re-run the key now
                // that we're back in Cursor mode so it still navigates. The
                // commit task carries `unfocus_all()`; dropping it (the old
                // `let _ = commit`) stranded a caret on a mixed
                // keyboard/mouse sequence. No recursion loop: the commit
                // flips mode to Cursor, so this arm can't re-fire.
                _ => {
                    let commit = self.update(Message::RulesEditor(R::CommitEditing));
                    let navigate = self.handle_raw_key_event(key.clone(), modifiers, status);
                    Some(Task::batch([commit, navigate]))
                }
            };
        }

        // --- Cursor mode --------------------------------------------------
        // Mouse-heal: a click focused the edit-bar name/comment input
        // without any message, so typing arrives Captured while the mirror
        // still says Cursor. Adopt Editing lazily (cosmetic ring fix) and
        // swallow — the input already consumed the keystroke.
        if captured {
            return match named {
                Some(Named::Escape) => Some(self.update(Message::RulesEditor(R::EscapePressed))),
                Some(Named::Tab) if modifiers.shift() => {
                    Some(self.update(Message::RulesEditor(R::SwitchPane)))
                }
                Some(Named::Tab) => Some(self.update(Message::RulesEditor(R::StepCell))),
                _ => {
                    // Snapshot the edit-bar text so a later Escape can revert
                    // the cell. Best-effort on this path: the click focused the
                    // input natively (no message), so we only see it now on the
                    // first captured keystroke — the baseline may already carry
                    // that one char. Still far better than no revert.
                    let (cur_name, cur_comment) = self
                        .playlist_editor
                        .as_ref()
                        .map(|e| {
                            (
                                e.edit.playlist_name.clone(),
                                e.edit.playlist_comment.clone(),
                            )
                        })
                        .unwrap_or_default();
                    self.with_rules_session(|s| {
                        let revert = if s.cell == crate::state::FormCell::Comment {
                            cur_comment.clone()
                        } else {
                            cur_name.clone()
                        };
                        s.mode = crate::state::FormMode::Editing;
                        s.editing = Some(crate::state::EditingCell {
                            row: crate::state::FormRow::EditBar,
                            cell: s.cell,
                            buffer: String::new(),
                            revert,
                        });
                    });
                    swallow()
                }
            };
        }

        // The discard confirm is showing: Enter confirms, Escape cancels.
        if self.rules_session().is_some_and(|s| s.confirm_discard) {
            return match named {
                Some(Named::Enter) => Some(self.update(Message::RulesEditor(R::ConfirmDiscard))),
                Some(Named::Escape) => Some(self.update(Message::RulesEditor(R::CancelDiscard))),
                _ => swallow(),
            };
        }

        // Blank create shows the Start-empty / Import / preset list in place
        // of the form rows — drive THAT list's cursor with Up/Down/Enter, so
        // the keys never walk the hidden form (where Enter would silently add
        // a phantom rule). Tab/Left/Right fall through untouched.
        if is_blank_create && pane == crate::state::RulesPane::Form {
            match named {
                Some(Named::ArrowUp) => {
                    return Some(
                        self.update(Message::RulesEditor(R::EmptyStateMove { down: false })),
                    );
                }
                Some(Named::ArrowDown) => {
                    return Some(
                        self.update(Message::RulesEditor(R::EmptyStateMove { down: true })),
                    );
                }
                Some(Named::Enter) if !modifiers.control() => {
                    return Some(self.update(Message::RulesEditor(R::EmptyStateActivate)));
                }
                _ => {}
            }
        }

        match named {
            Some(Named::ArrowUp) if modifiers.shift() => {
                return Some(self.update(Message::RulesEditor(R::MoveCursorRow { up: true })));
            }
            Some(Named::ArrowDown) if modifiers.shift() => {
                return Some(self.update(Message::RulesEditor(R::MoveCursorRow { up: false })));
            }
            Some(Named::ArrowUp) => {
                return Some(self.update(Message::RulesEditor(R::CursorMove { down: false })));
            }
            Some(Named::ArrowDown) => {
                return Some(self.update(Message::RulesEditor(R::CursorMove { down: true })));
            }
            Some(Named::ArrowLeft) => {
                return Some(self.update(Message::RulesEditor(R::CycleCell { forward: false })));
            }
            Some(Named::ArrowRight) => {
                return Some(self.update(Message::RulesEditor(R::CycleCell { forward: true })));
            }
            Some(Named::Tab) if modifiers.shift() => {
                return Some(self.update(Message::RulesEditor(R::SwitchPane)));
            }
            Some(Named::Tab) => return Some(self.update(Message::RulesEditor(R::StepCell))),
            Some(Named::Enter) if modifiers.control() => {
                return Some(self.update(Message::RulesEditor(R::Preview)));
            }
            Some(Named::Enter) => {
                if pane == crate::state::RulesPane::Results {
                    // The tweak-preview-HEAR loop: Enter plays the
                    // centered evaluated row.
                    return Some(self.update(Message::RulesEditor(R::PlayPreviewRow)));
                }
                return Some(self.update(Message::RulesEditor(R::EnterOnCursor)));
            }
            Some(Named::Delete | Named::Backspace) if !modifiers.shift() => {
                if pane == crate::state::RulesPane::Form {
                    return Some(self.update(Message::RulesEditor(R::DeleteCursorRow)));
                }
                return swallow();
            }
            Some(Named::Escape) => {
                return Some(self.update(Message::RulesEditor(R::EscapePressed)));
            }
            _ => {}
        }
        // `x` doubles as the row-delete key in the form (the grammar's
        // stated pair with Delete) — intercepted before its global
        // ToggleRandom resolution, form-pane only.
        if is_char("x") && pane == crate::state::RulesPane::Form {
            return Some(self.update(Message::RulesEditor(R::DeleteCursorRow)));
        }

        // Action-level remaps (rebind-safe: keyed on the RESOLVED action,
        // never the literal chord).
        match resolved {
            Some(Message::Hotkey(HotkeyMessage::SaveQueueAsPlaylist)) => {
                return Some(self.update(Message::RulesEditor(R::Save)));
            }
            Some(Message::SlotList(SlotListMessage::ActivateCenterShuffled)) => {
                return Some(self.update(Message::RulesEditor(R::Preview)));
            }
            Some(Message::Hotkey(HotkeyMessage::MoveTrackUp)) => {
                return Some(self.update(Message::RulesEditor(R::MoveCursorRow { up: true })));
            }
            Some(Message::Hotkey(HotkeyMessage::MoveTrackDown)) => {
                return Some(self.update(Message::RulesEditor(R::MoveCursorRow { up: false })));
            }
            Some(Message::Hotkey(HotkeyMessage::CycleSortMode(forward))) => {
                return Some(self.update(Message::RulesEditor(R::CycleCell { forward: *forward })));
            }
            Some(Message::Hotkey(HotkeyMessage::SettingsCategoryMotion(_))) => {
                return Some(self.update(Message::RulesEditor(R::SwitchPane)));
            }
            _ => {}
        }

        // Everything else falls through to normal dispatch — the session
        // is a view, not a modal; Space/transport hotkeys keep working.
        None
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
        // The login screen owns its own keyboard handling: Tab focus traversal
        // arrives via the `login_events` subscription (main.rs) and Enter via
        // the password field's `on_submit`. The global hotkey system has
        // nothing to drive before login — no slot list, queue, or playback — so
        // dispatching here only causes harm: Tab would be DOUBLE-handled (the
        // login form advances focus AND this path drives `SlotList(NavigateDown)`
        // against the off-screen queue, which reads as focus jumping the wrong
        // way), and bare keys like `x`/`c` would toggle random/consume against an
        // idle engine. One guard closes the whole class.
        if self.screen == crate::Screen::Login {
            return Task::none();
        }

        // If settings is in hotkey capture mode, forward the raw event there
        // instead of dispatching it as a normal hotkey action
        if self.settings_page.capturing_hotkey.is_some() {
            return self.handle_settings(crate::views::SettingsMessage::HotkeyCaptured(
                key, modifiers,
            ));
        }

        // A bare modifier / lock keypress — the Shift or CapsLock the user
        // presses to type a capital, or a bare Ctrl/Alt/Super/etc. — is
        // delivered by winit as its own `KeyPressed`. It is NEVER a hotkey
        // (`iced_key_to_keycode` maps every one of them to `None`) and is NEVER
        // captured by a focused `text_input` (which only captures the character
        // it produces), so it always arrives `Status::Ignored`. Its one live
        // effect today is tripping the rules editor's Editing-mode "focus lost"
        // fall-through, which commits the cell and blurs the input — so the
        // capital that follows lands on a blurred field and leaks to a global
        // hotkey (Shift+S → FindSimilar, Shift+D → ClearQueue). Dropping it here,
        // before any dispatch, keeps the field focused so the capital types
        // normally; it is behavior-neutral everywhere else (it resolves to no
        // hotkey regardless of where it lands). Placed AFTER the hotkey-capture
        // forward so rebinding a chord still receives the modifier keydowns.
        if matches!(
            key,
            iced::keyboard::Key::Named(
                iced::keyboard::key::Named::Shift
                    | iced::keyboard::key::Named::Control
                    | iced::keyboard::key::Named::Alt
                    | iced::keyboard::key::Named::AltGraph
                    | iced::keyboard::key::Named::Super
                    | iced::keyboard::key::Named::Meta
                    | iced::keyboard::key::Named::Hyper
                    | iced::keyboard::key::Named::CapsLock
                    | iced::keyboard::key::Named::NumLock
                    | iced::keyboard::key::Named::ScrollLock
                    | iced::keyboard::key::Named::Fn
                    | iced::keyboard::key::Named::FnLock
                    | iced::keyboard::key::Named::Symbol
                    | iced::keyboard::key::Named::SymbolLock
            )
        ) {
            return Task::none();
        }

        // Escape always reaches the dispatcher: it closes overlays / clears
        // search. Hoisted (with Tab) so the guard blocks below can read them.
        let is_escape = matches!(
            key,
            iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
        );
        let is_tab = matches!(
            key,
            iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab)
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
        let resolved = crate::hotkeys::handle_hotkey(key.clone(), modifiers, &self.hotkey_config);

        // A root-level modal open OVER the rules split-view (the Trawl mix
        // builder via `t`, EQ, Info, About, a text-input dialog, the
        // default-playlist picker) owns the keyboard — its own gate below
        // handles it. The rules intercept must NOT run underneath, or
        // Escape/Enter/arrows would drive the hidden form (arming the
        // invisible discard confirm, or discarding the dirty session under
        // the modal) instead of closing/using the modal.
        let any_blocking_modal = self.eq_modal.open
            || self.about_modal.visible
            || self.info_modal.visible
            || self.text_input_dialog.visible
            || self.default_playlist_picker.is_some()
            || self.trawl_modal.is_some();

        // Rules-session grammar: view-gated keys for the smart-playlist
        // rules editor. Intercepted before any SFX/toolbar arms could fire
        // on the resolved actions (the ui-views.md gate-placement
        // invariant), but AFTER the no-modal check — the session is
        // view-hosted, and its own sub-pickers get their modal-grade key
        // ownership inside the intercept.
        if !any_blocking_modal
            && let Some(task) = self.rules_session_key_intercept(&key, modifiers, status, &resolved)
        {
            return task;
        }

        // Modal-open suppression: the EQ / Info / About modals and the
        // default-playlist picker are mouse-opaque but not keyboard-capturing
        // and host no focused text_input, so bare-key hotkeys arrive
        // Status::Ignored and would otherwise drive the obscured view (e.g.
        // Space toggling playback behind an open EQ modal). When any blocking
        // modal is open, only Escape passes (it closes the modal via the
        // existing ClearSearch cascade). The picker additionally lets its own
        // slot-list nav keys through — slot_list.rs already routes those to the
        // picker when it is open.
        if any_blocking_modal {
            let is_picker_nav = self.default_playlist_picker.is_some()
                && matches!(
                    resolved,
                    Some(Message::SlotList(
                        crate::app_message::SlotListMessage::NavigateUp
                            | crate::app_message::SlotListMessage::NavigateDown
                            | crate::app_message::SlotListMessage::ActivateCenter
                    ))
                );
            // The trawl modal additionally admits Ctrl+Enter
            // (ActivateCenterShuffled) and Shift+A (AddToQueue): the slot-list
            // intercept maps the former to PlayMix and handle_add_to_queue
            // routes the latter to AddMixToQueue — the one playable/enqueueable
            // thing inside the modal is the mix.
            // Shift+Tab/Shift+Backspace (SettingsCategoryMotion) and Left/Right
            // (CycleSortMode) drive the tray-controls keyboard cursor. All of
            // these handlers route trawl-first, so admitting them here never
            // leaks to the obscured view. These arms live INSIDE this
            // trawl-gated allowlist on purpose — the EQ/Info/About modals must
            // keep swallowing the same keys.
            let is_trawl_nav = self.trawl_modal.is_some()
                && matches!(
                    resolved,
                    Some(
                        Message::SlotList(
                            crate::app_message::SlotListMessage::NavigateUp
                                | crate::app_message::SlotListMessage::NavigateDown
                                | crate::app_message::SlotListMessage::ActivateCenter
                                | crate::app_message::SlotListMessage::ActivateCenterShuffled
                        ) | Message::Hotkey(
                            crate::app_message::HotkeyMessage::FocusSearch
                                | crate::app_message::HotkeyMessage::CycleSortMode(_)
                                | crate::app_message::HotkeyMessage::SettingsCategoryMotion(_)
                                | crate::app_message::HotkeyMessage::AddToQueue
                                | crate::app_message::HotkeyMessage::TrawlSaveAsPlaylist
                        )
                    )
                );
            if !is_escape && !is_picker_nav && !is_trawl_nav {
                return Task::none();
            }
            // A key the trawl search field CAPTURED already did something in
            // the field — Shift+Backspace deleted a character, a rebound
            // Ctrl+V pasted — and must not ALSO drive the tray or enqueue the
            // mix: one press, one meaning. Keyed on the event status rather
            // than the `search_input_focused` mirror (the flag goes stale
            // around mouse clicks — iced sends no focus events) and matched on
            // EVERY tray action in BOTH directions plus AddToQueue so user
            // rebinds can't open a double-handling hole. Tab is exempt: a
            // focused text_input never captures Tab, and the settings
            // precedent (is_shift_nav + its pinned test) admits even a
            // theoretically-captured Shift+Tab.
            if self.trawl_modal.is_some()
                && status == iced::event::Status::Captured
                && !is_tab
                && matches!(
                    resolved,
                    Some(Message::Hotkey(
                        crate::app_message::HotkeyMessage::SettingsCategoryMotion(_)
                            | crate::app_message::HotkeyMessage::CycleSortMode(_)
                            | crate::app_message::HotkeyMessage::AddToQueue
                            | crate::app_message::HotkeyMessage::TrawlSaveAsPlaylist
                    ))
                )
            {
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
    ///
    /// The trawl branch sits FIRST: with the mix builder open, the same
    /// action steps the tray-controls focus ring instead — `current_view`
    /// still names the obscured view, so falling through would drive it.
    pub(crate) fn handle_settings_category_motion(&mut self, forward: bool) -> Task<Message> {
        if self.trawl_modal.is_some() {
            return self.handle_trawl_tray_focus_move(forward);
        }
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
