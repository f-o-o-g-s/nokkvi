//! Update handlers for split-view playlist editing mode.
//!
//! Covers browsing panel tab switching, enter/exit edit mode,
//! pane focus switching, and save flow.

use iced::Task;
use tracing::{debug, info};

use crate::{
    Nokkvi, View,
    app_message::Message,
    state::PaneFocus,
    views::{BrowsingPanel, BrowsingPanelMessage, BrowsingView},
};

impl Nokkvi {
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
        if self.playlist_edit.is_some() {
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
    /// Loads the playlist's songs into the queue, creates a snapshot for
    /// dirty detection, sets up the browsing panel, and switches to Queue view.
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

        // Set up edit state — snapshot gets populated after QueueLoaded arrives
        self.playlist_edit = Some(nokkvi_data::types::playlist_edit::PlaylistEditState::new(
            playlist_id.clone(),
            playlist_name.clone(),
            playlist_comment.clone(),
            playlist_public,
            Vec::new(),
        ));
        // Re-anchor `active_playlist_info` to the playlist being edited.
        // The queue is about to be replaced with this playlist's tracks
        // (possibly zero, for a freshly-created playlist), so the read-only
        // header that reappears after exiting edit mode must reflect the
        // *edited* playlist — not whatever was playing before. The edit bar
        // takes priority while editing (`edit_mode_info` checked first in the
        // view), so this assignment isn't visible until the user saves or
        // discards.
        self.active_playlist_info = Some(crate::state::ActivePlaylistContext {
            id: playlist_id.clone(),
            name: playlist_name,
            comment: playlist_comment,
        });
        self.persist_active_playlist_info();
        self.browsing_panel = Some(BrowsingPanel::new());
        self.pane_focus = PaneFocus::Queue;
        self.current_view = View::Queue;

        // Default browsing tab is Songs — trigger load if data hasn't arrived yet
        let songs_load = if self.library.songs.is_empty() {
            Task::done(Message::LoadSongs)
        } else {
            Task::none()
        };

        // Load playlist songs into the queue without starting playback
        let queue_load = self.shell_action_task(
            move |shell| async move { shell.load_playlist_into_queue(&playlist_id).await },
            Message::LoadQueue,
            "load playlist for editing",
        );

        Task::batch([queue_load, songs_load])
    }

    /// Exit split-view playlist editing mode.
    pub(crate) fn handle_exit_playlist_edit_mode(&mut self) -> Task<Message> {
        if let Some(edit_state) = &self.playlist_edit {
            let current_ids = self.queue_song_ids();
            let is_dirty = edit_state.is_dirty(&current_ids);
            let name = edit_state.playlist_name.clone();
            if is_dirty {
                self.toast_warn("Discarded unsaved playlist changes");
            }
            info!(" Exiting playlist edit mode: \"{}\"", name);
        }

        self.playlist_edit = None;
        self.browsing_panel = None;
        self.pane_focus = PaneFocus::Queue;

        Task::none()
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
        let Some(edit_state) = &self.playlist_edit else {
            return Task::none();
        };

        let playlist_id = edit_state.playlist_id.clone();
        let playlist_name = edit_state.playlist_name.clone();
        let playlist_comment = edit_state.playlist_comment.clone();
        let playlist_public = edit_state.playlist_public;
        let song_ids = self.queue_song_ids();
        let name_changed = edit_state.is_name_dirty();
        let comment_changed = edit_state.is_comment_dirty();
        let public_changed = edit_state.is_public_dirty();
        let metadata_changed = edit_state.has_metadata_changes();

        info!(
            " Saving playlist \"{}\" with {} tracks{}{}{}",
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
        );

        self.shell_action_task(
            move |shell| async move {
                let service = shell.playlists_api().await?;
                // Update name/comment/visibility if any of them changed
                if metadata_changed {
                    let comment_arg = if comment_changed {
                        Some(playlist_comment.as_str())
                    } else {
                        None
                    };
                    service
                        .update_playlist(&playlist_id, &playlist_name, comment_arg, playlist_public)
                        .await?;
                }
                service
                    .replace_playlist_tracks(&playlist_id, &song_ids)
                    .await
            },
            Message::PlaylistEditsSaved,
            "save playlist edits",
        )
    }

    /// Handle successful playlist save — update snapshot and show toast.
    pub(crate) fn handle_playlist_edits_saved(&mut self) -> Task<Message> {
        let current_ids = self.queue_song_ids();

        if let Some(edit_state) = &mut self.playlist_edit {
            let name = edit_state.playlist_name.clone();
            let comment = edit_state.playlist_comment.clone();
            let id = edit_state.playlist_id.clone();
            edit_state.update_snapshot(current_ids);
            self.toast_success(format!("Playlist \"{name}\" saved"));

            // Sync edited name/comment back to active_playlist_info so the
            // read-only context bar shows updated values after exiting edit mode.
            self.active_playlist_info =
                Some(crate::state::ActivePlaylistContext { id, name, comment });
            self.persist_active_playlist_info();
        }

        // Reload playlists so the Playlists view reflects any rename immediately
        Task::done(Message::LoadPlaylists)
    }
}
