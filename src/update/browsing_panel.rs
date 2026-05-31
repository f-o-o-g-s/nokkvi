//! Update handlers for split-view playlist editing mode.
//!
//! Covers browsing panel tab switching, enter/exit edit mode,
//! pane focus switching, and save flow.

use iced::Task;
use tracing::{debug, error, info};

use crate::{
    Nokkvi, View,
    app_message::{EditorMessage, Message, SplitViewMessage},
    state::PaneFocus,
    views::{BrowsingPanel, BrowsingPanelMessage, BrowsingView},
};

/// Outcome of the async playlist-save task.
///
/// Distinguishes a completed save from an aborted one (the server playlist
/// moved on since the editor opened) so the UI can surface a distinct
/// "reload before saving" warning rather than a generic error toast.
enum SaveOutcome {
    /// Save completed (metadata and/or tracks persisted, or a clean no-op).
    Saved,
    /// Aborted: the server playlist changed since the editor opened; the
    /// destructive track overwrite was refused.
    Stale,
}

impl Nokkvi {
    /// Dispatch `SplitViewMessage` sub-enum variants to their per-variant
    /// handlers. Mirrors the per-handler routing pattern other sub-enums
    /// use (e.g. `handle_find_message` in `similar.rs`).
    pub(crate) fn handle_split_view_message(&mut self, msg: SplitViewMessage) -> Task<Message> {
        match msg {
            SplitViewMessage::EnterEditMode {
                playlist_id,
                playlist_name,
                playlist_comment,
                playlist_public,
            } => self.handle_enter_playlist_edit_mode(
                playlist_id,
                playlist_name,
                playlist_comment,
                playlist_public,
            ),
            SplitViewMessage::ExitEditMode => self.handle_exit_playlist_edit_mode(),
            SplitViewMessage::ToggleBrowsingPanel => self.handle_toggle_browsing_panel(),
            SplitViewMessage::SwitchPaneFocus => self.handle_switch_pane_focus(),
            SplitViewMessage::SavePlaylistEdits => self.handle_save_playlist_edits(),
            SplitViewMessage::PlaylistEditsSaved => self.handle_playlist_edits_saved(),
        }
    }

    /// Handle browsing panel messages (tab switching).
    pub(crate) fn handle_browsing_panel_message(
        &mut self,
        msg: BrowsingPanelMessage,
    ) -> Task<Message> {
        match msg {
            BrowsingPanelMessage::SwitchView(view) => {
                if let Some(panel) = &mut self.browsing_panel {
                    debug!(" [BROWSE] Switching browsing panel to {:?}", view);
                    panel.active_view = view;
                }
                // Trigger data load if the view's data hasn't been fetched yet
                // (mirrors the lazy-load pattern in handle_switch_view)
                match view {
                    BrowsingView::Albums if self.library.albums.is_empty() => {
                        Task::done(Message::LoadAlbums)
                    }
                    BrowsingView::Songs if self.library.songs.is_empty() => {
                        Task::done(Message::LoadSongs)
                    }
                    BrowsingView::Artists if self.library.artists.is_empty() => {
                        Task::done(Message::LoadArtists)
                    }
                    BrowsingView::Genres if self.library.genres.is_empty() => {
                        Task::done(Message::LoadGenres)
                    }
                    _ => Task::none(),
                }
            }
            BrowsingPanelMessage::Close => self.handle_toggle_browsing_panel(),
        }
    }

    /// Toggle the browsing panel on/off while on Queue view.
    ///
    /// When in playlist edit mode, the panel cannot be closed (use Discard/Save).
    /// Otherwise, toggles the panel and switches to Queue view if not already there.
    pub(crate) fn handle_toggle_browsing_panel(&mut self) -> Task<Message> {
        // Only allow toggling from Queue view
        if self.current_view != View::Queue {
            debug!(" [BROWSE] Toggle ignored — not on Queue view");
            return Task::none();
        }

        // In edit mode, don't allow closing the panel
        if self.playlist_editor.is_some() {
            debug!(" [BROWSE] Toggle ignored — in playlist edit mode");
            return Task::none();
        }

        if self.browsing_panel.is_some() {
            info!(" [BROWSE] Closing browsing panel");
            self.browsing_panel = None;
            self.pane_focus = PaneFocus::Queue;
            Task::none()
        } else {
            info!(" [BROWSE] Opening browsing panel");
            self.browsing_panel = Some(BrowsingPanel::new());
            self.pane_focus = PaneFocus::Queue;
            // Default tab is Songs — trigger load if data hasn't arrived yet
            // (mirrors the lazy-load pattern in handle_browsing_panel_message::SwitchView)
            if self.library.songs.is_empty() {
                Task::done(Message::LoadSongs)
            } else {
                Task::none()
            }
        }
    }

    /// Enter split-view playlist editing mode.
    ///
    /// Resolves the playlist's songs into the editor's OWN buffer (via
    /// `resolve_playlist_for_editor` → `EditorMessage::SongsLoaded`), sets up
    /// the browsing panel, and switches to Queue view. The live play queue,
    /// audio engine, and persisted state are left entirely untouched — music
    /// keeps playing while the user edits.
    pub(crate) fn handle_enter_playlist_edit_mode(
        &mut self,
        playlist_id: String,
        playlist_name: String,
        playlist_comment: String,
        playlist_public: bool,
    ) -> Task<Message> {
        info!(
            " Entering playlist edit mode: \"{}\" ({}) [public={}]",
            playlist_name, playlist_id, playlist_public
        );

        // Set up edit state — the dirty snapshot gets seeded from the loaded
        // rows once `EditorMessage::SongsLoaded` arrives (see
        // `handle_editor_songs_loaded`). The editor owns its `PlaylistEditState`
        // and its own track buffer; the live queue is never read or written
        // during the edit session.
        let mut edit_state = nokkvi_data::types::playlist_edit::PlaylistEditState::new(
            playlist_id.clone(),
            playlist_name.clone(),
            playlist_comment.clone(),
            playlist_public,
            Vec::new(),
        );
        // Capture the server `updatedAt` from the cached list entry so the save
        // path can detect a concurrent server-side edit (optimistic-concurrency
        // guard). Absent (e.g. a freshly-created empty playlist) → empty, which
        // the staleness check treats as never-stale.
        if let Some(playlist) = self.library.playlists.iter().find(|p| p.id == playlist_id) {
            edit_state.set_loaded_updated_at(playlist.updated_at.clone());
        }
        self.playlist_editor = Some(crate::state::PlaylistEditorState::new(edit_state));
        // Leave `active_playlist_info` (the "Playing From" banner) untouched:
        // editing is decoupled from playback, so the queue keeps playing from
        // whatever it was playing from. Re-anchoring here would leave a stale
        // banner pointing at the edited playlist after the user discards.
        self.browsing_panel = Some(BrowsingPanel::new());
        self.pane_focus = PaneFocus::Queue;
        // Collapse the "Playing From" banner: the view swap below unmounts its
        // hover `mouse_area`, so the `on_exit` that would normally collapse a
        // hover-expanded strip can never fire. Reset the flag here (a reset hook
        // alongside `clear_active_playlist`) so the banner re-mounts collapsed
        // rather than carrying a stale expansion onto the Queue tab.
        self.queue_page.playlist_strip_expanded = false;
        // Remember where the edit was launched from so discard/exit returns
        // there (mirrors `pre_settings_view`). Guard against re-entry from the
        // editor itself, which would trap the return view on the editor.
        self.editor_return_view = if self.current_view == View::PlaylistEditor {
            View::Playlists
        } else {
            self.current_view
        };
        // Navigate to the dedicated editor view. The live Queue tab is left
        // alone, so the user can always switch back to their real queue.
        self.current_view = View::PlaylistEditor;

        // Default browsing tab is Songs — trigger load if data hasn't arrived yet
        let songs_load = if self.library.songs.is_empty() {
            Task::done(Message::LoadSongs)
        } else {
            Task::none()
        };

        // Resolve the playlist's tracks into the editor's OWN buffer WITHOUT
        // touching the queue/engine/redb. The result is dispatched as
        // `EditorMessage::SongsLoaded`, which fills the buffer and seeds the
        // dirty snapshot. The live play queue is left entirely untouched.
        let editor_load = self.shell_task(
            move |shell| async move { shell.resolve_playlist_for_editor(&playlist_id).await },
            |result| match result {
                Ok(rows) => Message::Editor(EditorMessage::SongsLoaded(rows)),
                Err(e) => {
                    // Mark the session Failed AND surface the error: the
                    // `SongsLoadFailed` handler sets the load-state marker (so
                    // save/mutations are gated off) and pushes the error toast.
                    error!(" Failed to resolve playlist for editing: {}", e);
                    Message::Editor(EditorMessage::SongsLoadFailed)
                }
            },
        );

        Task::batch([editor_load, songs_load])
    }

    /// Exit split-view playlist editing mode.
    pub(crate) fn handle_exit_playlist_edit_mode(&mut self) -> Task<Message> {
        if let Some(edit_state) = self.playlist_editor.as_ref().map(|e| &e.edit) {
            let current_ids = self.editor_song_ids();
            let is_dirty = edit_state.is_dirty(&current_ids);
            let name = edit_state.playlist_name.clone();
            if is_dirty {
                self.toast_warn("Discarded unsaved playlist changes");
            }
            info!(" Exiting playlist edit mode: \"{}\"", name);
        }

        self.playlist_editor = None;
        self.browsing_panel = None;
        self.pane_focus = PaneFocus::Queue;
        // Symmetric to the enter edge: the queue banner re-mounts here with no
        // cursor over it, so clear any expansion stranded during the session
        // (its hover `on_exit` could not fire while the banner was unmounted).
        self.queue_page.playlist_strip_expanded = false;

        // Return to wherever the edit was launched from (mirrors closing
        // Settings). Cleared the session above first, so the switch-view guard
        // sees no active editor.
        let target = self.editor_return_view;
        self.handle_switch_view(target)
    }

    /// Toggle keyboard focus between queue and browser panes.
    pub(crate) fn handle_switch_pane_focus(&mut self) -> Task<Message> {
        if self.browsing_panel.is_some() {
            self.pane_focus = match self.pane_focus {
                PaneFocus::Queue => PaneFocus::Browser,
                PaneFocus::Browser => PaneFocus::Queue,
            };
            debug!(" [PANE] Focus switched to {:?}", self.pane_focus);
        }
        Task::none()
    }

    /// Save the current queue as the edited playlist's tracks.
    /// Also renames the playlist if the name was changed.
    pub(crate) fn handle_save_playlist_edits(&mut self) -> Task<Message> {
        // Gate the save on a successfully-loaded buffer: a still-loading or
        // failed resolve leaves an empty/partial buffer that is NOT the real
        // playlist, so persisting it would full-overwrite the server playlist
        // with garbage. Discard/Exit remain available (they never touch the
        // server). The editor stays mounted — no auto-abort.
        match self.playlist_editor.as_ref().map(|e| e.load_state) {
            Some(crate::state::EditorLoadState::Loaded) => {}
            Some(crate::state::EditorLoadState::Loading) => {
                self.toast_warn("Playlist hasn't finished loading");
                return Task::none();
            }
            Some(crate::state::EditorLoadState::Failed) => {
                self.toast_warn("Playlist failed to load — reload before saving");
                return Task::none();
            }
            None => return Task::none(),
        }

        let Some(edit_state) = self.playlist_editor.as_ref().map(|e| &e.edit) else {
            return Task::none();
        };

        let playlist_id = edit_state.playlist_id.clone();
        let playlist_name = edit_state.playlist_name.clone();
        let playlist_comment = edit_state.playlist_comment.clone();
        let playlist_public = edit_state.playlist_public;
        // Serialize the editor's OWN full ordered buffer (never the filtered
        // subset, never the live queue) — the editor buffer is the source of
        // truth for what gets persisted.
        let song_ids = self.editor_song_ids();
        let name_changed = edit_state.is_name_dirty();
        let comment_changed = edit_state.is_comment_dirty();
        let public_changed = edit_state.is_public_dirty();
        let metadata_changed = edit_state.has_metadata_changes();
        // Track-dirty gate: skip the destructive full-overwrite entirely when
        // the buffer matches the loaded snapshot (no track edits).
        let tracks_changed = edit_state.is_dirty(&song_ids);
        // Optimistic-concurrency token captured at editor open; empty = never
        // stale (freshly-created playlist).
        let loaded_updated_at = edit_state.loaded_updated_at().to_string();

        info!(
            " Saving playlist \"{}\" with {} tracks{}{}{}{}",
            playlist_name,
            song_ids.len(),
            if name_changed { " (renamed)" } else { "" },
            if comment_changed {
                " (comment changed)"
            } else {
                ""
            },
            if public_changed {
                " (visibility changed)"
            } else {
                ""
            },
            if tracks_changed {
                " (tracks changed)"
            } else {
                ""
            },
        );

        self.shell_task(
            move |shell| async move {
                let service = shell.playlists_api().await?;
                // Update name/comment/visibility if any of them changed. Send
                // ONLY the dirty fields (Navidrome's nil-means-unchanged
                // contract): a comment-only edit no longer re-writes the name,
                // and an unchanged `public` flag is never replayed — so a
                // concurrent server-side visibility change is not silently
                // reverted.
                if metadata_changed {
                    let name_arg = name_changed.then_some(playlist_name.as_str());
                    let comment_arg = comment_changed.then_some(playlist_comment.as_str());
                    let public_arg = public_changed.then_some(playlist_public);
                    service
                        .update_playlist(&playlist_id, name_arg, comment_arg, public_arg)
                        .await?;
                }
                // The track overwrite is destructive (full createPlaylist
                // replace). Run it ONLY when tracks actually changed, and only
                // after confirming the server playlist has not moved on since
                // the editor opened — otherwise a concurrent server-side track
                // edit would be silently destroyed with nothing to roll back to.
                if tracks_changed {
                    if !loaded_updated_at.is_empty() {
                        let current = service.get_playlist_updated_at(&playlist_id).await?;
                        if current != loaded_updated_at {
                            return Ok::<SaveOutcome, anyhow::Error>(SaveOutcome::Stale);
                        }
                    }
                    service
                        .replace_playlist_tracks(&playlist_id, &song_ids)
                        .await?;
                }
                Ok(SaveOutcome::Saved)
            },
            |result| match result {
                Ok(SaveOutcome::Saved) => Message::SplitView(SplitViewMessage::PlaylistEditsSaved),
                Ok(SaveOutcome::Stale) => {
                    tracing::warn!(" Aborting playlist save — server changed since edit opened");
                    Message::Toast(crate::app_message::ToastMessage::Push(
                        nokkvi_data::types::toast::Toast::new(
                            "Playlist changed on the server — reload before saving".to_string(),
                            nokkvi_data::types::toast::ToastLevel::Warning,
                        ),
                    ))
                }
                Err(e) => {
                    if let Some(msg) = crate::update::components::session_expired_message(&e) {
                        return msg;
                    }
                    error!(" Failed to save playlist edits: {}", e);
                    Message::Toast(crate::app_message::ToastMessage::Push(
                        nokkvi_data::types::toast::Toast::new(
                            format!("Failed to save playlist edits: {e}"),
                            nokkvi_data::types::toast::ToastLevel::Error,
                        ),
                    ))
                }
            },
        )
    }

    /// Handle successful playlist save — update snapshot and show toast.
    pub(crate) fn handle_playlist_edits_saved(&mut self) -> Task<Message> {
        let current_ids = self.editor_song_ids();

        if let Some(edit_state) = self.playlist_editor.as_mut().map(|e| &mut e.edit) {
            let name = edit_state.playlist_name.clone();
            let comment = edit_state.playlist_comment.clone();
            let id = edit_state.playlist_id.clone();
            edit_state.update_snapshot(current_ids);
            self.toast_success(format!("Playlist \"{name}\" saved"));

            // Only refresh the "Playing From" banner when the live queue is
            // actually playing from the playlist just saved — then a rename
            // shows immediately. Saving any other playlist leaves the banner
            // pointed at whatever is really playing.
            if self
                .active_playlist_info
                .as_ref()
                .is_some_and(|ctx| ctx.id == id)
            {
                self.active_playlist_info = Some(
                    self.library
                        .playlists
                        .iter()
                        .find(|p| p.id == id)
                        .map_or_else(
                            || crate::state::ActivePlaylistContext::minimal(id, name, comment),
                            crate::state::ActivePlaylistContext::from_playlist,
                        ),
                );
                self.persist_active_playlist_info();
            }
        }

        // Reload playlists so the Playlists view reflects any rename immediately
        Task::done(Message::LoadPlaylists)
    }
}
