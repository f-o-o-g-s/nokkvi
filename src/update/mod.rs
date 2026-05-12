//! Update function for Nokkvi
//!
//! Contains the central message handler and helper functions.
//! Message handlers are organized into submodules by domain.

/// DRY macro for view message dispatch with scroll-seek timer injection.
/// All non-Queue slot list views share the same pattern: check if the message
/// is a `SlotListScrollSeek`, call the handler, then append scrollbar fade
/// and seek-settled timers when it was a seek event.
///
/// **Why this lives in `mod.rs` rather than getting per-view
/// `dispatch_<view>` extractions.** The other domain dispatchers (Artwork,
/// Playback, Hotkey, Scrobble) live in their own files because each
/// encapsulates a 14–25-arm nested match whose arms gain real locality
/// from sitting next to their domain handlers. The slot-list views are
/// different: each per-view dispatch is *already* a single line through
/// this macro, and the per-view handler bodies (`handle_albums`,
/// `handle_artists`, …) already live in their own files. Wrapping each
/// in a `dispatch_<view>` method would just indirect through this macro
/// one extra layer with no co-location win — strictly more files for
/// strictly less leverage.
macro_rules! dispatch_view_with_seek {
    ($self:ident, $msg:ident, $handler:ident, $seek_pat:pat, $view:expr) => {{
        let is_seek = matches!($msg, $seek_pat);
        let task = $self.$handler($msg);
        if is_seek {
            let view = $view;
            iced::Task::batch([
                task,
                $self.scrollbar_fade_timer(view),
                $self.seek_settled_timer(view),
            ])
        } else {
            task
        }
    }};
}

mod about_modal;
mod albums;
mod artists;
mod artwork;
mod boat;
mod browsing_panel;
pub(crate) mod chrome;
mod collage;
mod components;
mod config;
mod cross_pane_drag;
mod default_playlist_picker;
mod eq_modal;
mod genres;
mod hotkeys;
mod info_modal;
mod library_refresh;
mod loader_target;
mod menus;
mod mpris;
mod navigation;
mod pending_expand_resolve;
mod playback;
mod player_bar;
mod playlists;
mod progressive_queue;
mod queue;
mod radios;
mod roulette;
mod scrobbling;
mod settings;
mod similar;
mod slot_list;
mod songs;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_queue_filter;
#[cfg(test)]
mod tests_star_rating;
mod text_input_dialog;
mod toast;
mod tray;
mod window;

pub(crate) use chrome::dispatch_view_chrome;
use iced::Task;
#[allow(unused_imports)]
pub(crate) use loader_target::{
    AlbumsTarget, ArtistsTarget, GenresTarget, LoaderTarget, PlaylistsTarget, SongsTarget,
};
pub(crate) use pending_expand_resolve::{AlbumSpec, ArtistSpec, GenreSpec, SongSpec};
use tracing::debug;

use crate::{Nokkvi, View, app_message::Message};

impl Nokkvi {
    /// Central message handler
    ///
    /// Routes messages to appropriate handlers organized by domain.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        // Auto-login check on first update
        if self.should_auto_login
            && matches!(
                message,
                Message::Playback(crate::app_message::PlaybackMessage::Tick)
            )
        {
            self.should_auto_login = false;
            debug!(" Triggering session resume...");
            return Task::done(Message::ResumeSession);
        }

        match message {
            // -----------------------------------------------------------------
            // Navigation
            // -----------------------------------------------------------------
            Message::SwitchView(view) => self.handle_switch_view(view),
            Message::LibraryChanged {
                album_ids,
                is_wildcard,
            } => self.handle_library_changed(album_ids, is_wildcard),
            Message::NavigateAndFilter(view, filter) => {
                self.handle_navigate_and_filter(view, filter)
            }
            Message::BrowserPaneNavigateAndFilter(view, filter) => {
                self.handle_browser_pane_navigate_and_filter(view, filter)
            }
            Message::NavigateAndExpandAlbum { album_id } => {
                self.handle_navigate_and_expand_album(album_id)
            }
            Message::BrowserPaneNavigateAndExpandAlbum { album_id } => {
                self.handle_browser_pane_navigate_and_expand_album(album_id)
            }
            Message::PendingExpandAlbumTimeout(album_id) => {
                self.handle_pending_expand_album_timeout(album_id)
            }
            Message::NavigateAndExpandArtist { artist_id } => {
                self.handle_navigate_and_expand_artist(artist_id)
            }
            Message::BrowserPaneNavigateAndExpandArtist { artist_id } => {
                self.handle_browser_pane_navigate_and_expand_artist(artist_id)
            }
            Message::PendingExpandArtistTimeout(artist_id) => {
                self.handle_pending_expand_artist_timeout(artist_id)
            }
            Message::NavigateAndExpandGenre { genre_id } => {
                self.handle_navigate_and_expand_genre(genre_id)
            }
            Message::BrowserPaneNavigateAndExpandGenre { genre_id } => {
                self.handle_browser_pane_navigate_and_expand_genre(genre_id)
            }
            Message::PendingExpandGenreTimeout(genre_id) => {
                self.handle_pending_expand_genre_timeout(genre_id)
            }
            Message::PendingExpandSongTimeout(song_id) => {
                self.handle_pending_expand_song_timeout(song_id)
            }
            Message::StripClicked => {
                use nokkvi_data::types::player_settings::StripClickAction;
                match crate::theme::strip_click_action() {
                    StripClickAction::GoToQueue => self.strip_navigate(crate::View::Queue, false),
                    StripClickAction::GoToAlbum => self.strip_navigate(crate::View::Albums, false),
                    StripClickAction::GoToArtist => {
                        self.strip_navigate(crate::View::Artists, false)
                    }
                    StripClickAction::CopyTrackInfo => self.strip_copy_track_info(),
                    StripClickAction::DoNothing => Task::none(),
                }
            }
            Message::StripContextAction(entry) => {
                use crate::widgets::context_menu::StripContextEntry;
                match entry {
                    StripContextEntry::GoToQueue => self.strip_navigate(crate::View::Queue, true),
                    StripContextEntry::GoToAlbum => self.strip_navigate(crate::View::Albums, true),
                    StripContextEntry::GoToArtist => {
                        self.strip_navigate(crate::View::Artists, true)
                    }
                    StripContextEntry::CopyTrackInfo => self.strip_copy_track_info(),
                    StripContextEntry::ToggleStar => self.handle_toggle_star_for_playing_track(),
                    StripContextEntry::ShowInFolder => {
                        self.handle_show_in_folder_for_playing_track()
                    }
                    StripContextEntry::FindSimilar => self.handle_find_similar_for_playing_track(),
                    StripContextEntry::TopSongs => self.handle_find_top_songs_for_playing_track(),
                    StripContextEntry::Separator => Task::none(),
                }
            }
            Message::ToggleSettings => {
                if self.current_view == crate::View::Settings {
                    self.handle_close_settings()
                } else {
                    self.handle_switch_view(crate::View::Settings)
                }
            }
            Message::SetOpenMenu(next) => self.handle_set_open_menu(next),
            Message::Roulette(msg) => self.handle_roulette_message(msg),
            Message::Login(msg) => self.handle_login(msg),
            Message::LoginResult(res) => self.handle_login_result(res),
            Message::ResumeSession => self.handle_resume_session(),
            Message::SessionExpired => self.handle_session_expired(),
            Message::ServerVersionFetched(ver) => {
                if ver.is_some() {
                    self.server_version = ver;
                }
                Task::none()
            }

            // -----------------------------------------------------------------
            // Data Loading: Albums
            // -----------------------------------------------------------------
            Message::LoadAlbums => self.handle_load_albums(false, None),
            // -----------------------------------------------------------------
            // Data Loading: Queue
            //
            // Note: the loader-result variant has migrated to
            // `Message::QueueLoader(QueueLoaderMessage::Loaded(...))`,
            // routed in the "Loader Results" block below.
            // -----------------------------------------------------------------
            Message::LoadQueue => self.handle_load_queue(),
            Message::ProgressiveQueueAppendPage {
                sort_mode,
                sort_order,
                search_query,
                offset,
                total_count,
                generation,
            } => self.handle_progressive_queue_append_page(
                sort_mode,
                sort_order,
                search_query,
                offset,
                total_count,
                generation,
            ),
            Message::ProgressiveQueueDone => {
                self.library.queue_loading_target = None;
                self.handle_load_queue()
            }

            // -----------------------------------------------------------------
            // Loader Results (per-domain *LoaderMessage) — route to
            // `dispatch_<domain>_loader` helpers in `update/<domain>.rs`.
            // -----------------------------------------------------------------
            Message::AlbumsLoader(msg) => self.dispatch_albums_loader(msg),
            Message::ArtistsLoader(msg) => self.dispatch_artists_loader(msg),
            Message::SongsLoader(msg) => self.dispatch_songs_loader(msg),
            Message::GenresLoader(msg) => self.dispatch_genres_loader(msg),
            Message::PlaylistsLoader(msg) => self.dispatch_playlists_loader(msg),
            Message::QueueLoader(msg) => self.dispatch_queue_loader(msg),

            // -----------------------------------------------------------------
            // Data Loading: Artists
            //
            // Note: the loader-result variants have migrated to
            // `Message::ArtistsLoader(ArtistsLoaderMessage::{Loaded,PageLoaded})`,
            // routed in the "Loader Results" block above.
            // -----------------------------------------------------------------
            Message::LoadArtists => self.handle_load_artists(false, None),

            // -----------------------------------------------------------------
            // Data Loading: Songs
            //
            // Note: the loader-result variants have migrated to
            // `Message::SongsLoader(SongsLoaderMessage::{Loaded,PageLoaded}(...))`,
            // routed in the "Loader Results" block above.
            // -----------------------------------------------------------------
            Message::LoadSongs => self.handle_load_songs(false, None),

            // -----------------------------------------------------------------
            // Data Loading: Genres
            //
            // Note: the loader-result variant has migrated to
            // `Message::GenresLoader(GenresLoaderMessage::Loaded(...))`,
            // routed in the "Loader Results" block above. Genres is the
            // proof-of-concept for the per-domain *LoaderMessage refactor;
            // see plan §3 Phase 1 and §11 prototype notes.
            // -----------------------------------------------------------------
            Message::LoadGenres => self.handle_load_genres(),

            // -----------------------------------------------------------------
            // Data Loading: Playlists
            // -----------------------------------------------------------------
            Message::LoadPlaylists => self.handle_load_playlists(),
            Message::LoadRadioStations => self.handle_load_radio_stations(),
            Message::PlaylistMutated(mutation) => {
                // When creating/overwriting a playlist from the queue, set the
                // playlist context header so the queue shows the same header bar
                // as when playing an existing playlist.
                match &mutation {
                    crate::app_message::PlaylistMutation::Created(name, Some(id))
                    | crate::app_message::PlaylistMutation::Overwritten(name, Some(id)) => {
                        self.active_playlist_info = Some(crate::state::ActivePlaylistContext {
                            id: id.clone(),
                            name: name.clone(),
                            comment: String::new(),
                        });
                        self.persist_active_playlist_info();
                    }
                    _ => {}
                }
                self.toast_success(mutation.to_string());
                self.handle_load_playlists()
            }
            Message::PlaylistsFetchedForDialog(playlists) => {
                self.text_input_dialog.open_save_playlist(&playlists);
                Task::none()
            }
            Message::PlaylistsFetchedForAddToPlaylist(playlists, song_ids) => {
                // Quick-add bypass: skip dialog when default playlist is configured
                if self.quick_add_to_playlist
                    && let Some(ref default_id) = self.default_playlist_id
                {
                    let playlist_id = default_id.clone();
                    let playlist_name = self.default_playlist_name.clone();
                    let count = song_ids.len();
                    return self.shell_action_task(
                        move |shell| async move {
                            let service = shell.playlists_api().await?;
                            service.add_songs_to_playlist(&playlist_id, &song_ids).await
                        },
                        Message::PlaylistMutated(crate::app_message::PlaylistMutation::Appended(
                            format!(
                                "{playlist_name}' ({count} song{})",
                                if count == 1 { "" } else { "s" }
                            ),
                        )),
                        "quick-add to default playlist",
                    );
                }
                self.text_input_dialog
                    .open_add_to_playlist(&playlists, song_ids);
                Task::none()
            }

            // -----------------------------------------------------------------
            // Artwork Pipeline (namespaced)
            // -----------------------------------------------------------------
            Message::Artwork(msg) => self.dispatch_artwork(msg),

            // -----------------------------------------------------------------
            // Playback (namespaced under PlaybackMessage)
            // -----------------------------------------------------------------
            Message::Playback(msg) => self.dispatch_playback(msg),
            Message::ViewPreferencesLoaded(prefs) => self.handle_view_preferences_loaded(prefs),

            // -----------------------------------------------------------------
            // Slot List Navigation (namespaced)
            // -----------------------------------------------------------------
            Message::SlotList(msg) => self.handle_slot_list_message(msg),

            // -----------------------------------------------------------------
            // Window Events
            // -----------------------------------------------------------------
            Message::WindowResized(width, height) => self.handle_window_resized(width, height),
            Message::ScaleFactorChanged(scale_factor) => {
                self.handle_scale_factor_changed(scale_factor)
            }
            Message::HotkeyConfigUpdated(config) => self.handle_hotkey_config_updated(config),
            Message::NoOp => Task::none(),
            Message::QuitApp => iced::exit(),
            // Async shutdown sequence completed (or timed out) — now exit.
            // This is the normal completion of WindowCloseRequested → begin_shutdown.
            // QuitApp (tray Quit / hamburger Quit) also lands at iced::exit()
            // directly since those paths don't go through the async sequence.
            Message::ShutdownComplete => iced::exit(),
            Message::PlaySfx(sfx_type) => self.handle_play_sfx(sfx_type),

            // -----------------------------------------------------------------
            // Scrobbling (namespaced)
            // -----------------------------------------------------------------
            Message::Scrobble(msg) => self.dispatch_scrobble(msg),

            // -----------------------------------------------------------------
            // Hotkey Actions (namespaced)
            // -----------------------------------------------------------------
            Message::Hotkey(msg) => self.dispatch_hotkey(msg),

            // -----------------------------------------------------------------
            // Component Message Bubbling
            // -----------------------------------------------------------------
            Message::PlayerBar(msg) => self.handle_player_bar(msg),
            Message::NavBar(_msg) => {
                // NavBar messages are handled via map() in navigation_bar()
                // SwitchView -> Message::SwitchView, ToggleLightMode -> Message::ToggleLightMode, etc.
                Task::none()
            }
            Message::ToggleLightMode => self.handle_toggle_light_mode(),
            Message::Albums(crate::views::AlbumsMessage::ArtworkColumnDrag(ev)) => {
                self.handle_artwork_column_drag(ev)
            }
            Message::Albums(crate::views::AlbumsMessage::ArtworkColumnVerticalDrag(ev)) => {
                self.handle_artwork_vertical_drag(ev)
            }
            Message::Albums(msg) => {
                dispatch_view_with_seek!(
                    self,
                    msg,
                    handle_albums,
                    crate::views::AlbumsMessage::SlotList(
                        crate::widgets::SlotListPageMessage::ScrollSeek(_),
                    ),
                    View::Albums
                )
            }
            Message::Queue(crate::views::QueueMessage::ArtworkColumnDrag(ev)) => {
                self.handle_artwork_column_drag(ev)
            }
            Message::Queue(crate::views::QueueMessage::ArtworkColumnVerticalDrag(ev)) => {
                self.handle_artwork_vertical_drag(ev)
            }
            Message::Queue(msg) => self.handle_queue(msg),
            Message::Artists(crate::views::ArtistsMessage::ArtworkColumnDrag(ev)) => {
                self.handle_artwork_column_drag(ev)
            }
            Message::Artists(crate::views::ArtistsMessage::ArtworkColumnVerticalDrag(ev)) => {
                self.handle_artwork_vertical_drag(ev)
            }
            Message::Artists(msg) => {
                dispatch_view_with_seek!(
                    self,
                    msg,
                    handle_artists,
                    crate::views::ArtistsMessage::SlotList(
                        crate::widgets::SlotListPageMessage::ScrollSeek(_)
                    ),
                    View::Artists
                )
            }
            Message::Songs(crate::views::SongsMessage::ArtworkColumnDrag(ev)) => {
                self.handle_artwork_column_drag(ev)
            }
            Message::Songs(crate::views::SongsMessage::ArtworkColumnVerticalDrag(ev)) => {
                self.handle_artwork_vertical_drag(ev)
            }
            Message::Songs(msg) => {
                dispatch_view_with_seek!(
                    self,
                    msg,
                    handle_songs,
                    crate::views::SongsMessage::SlotList(
                        crate::widgets::SlotListPageMessage::ScrollSeek(_)
                    ),
                    View::Songs
                )
            }
            Message::Genres(crate::views::GenresMessage::ArtworkColumnDrag(ev)) => {
                self.handle_artwork_column_drag(ev)
            }
            Message::Genres(crate::views::GenresMessage::ArtworkColumnVerticalDrag(ev)) => {
                self.handle_artwork_vertical_drag(ev)
            }
            Message::Genres(msg) => {
                dispatch_view_with_seek!(
                    self,
                    msg,
                    handle_genres,
                    crate::views::GenresMessage::SlotList(
                        crate::widgets::SlotListPageMessage::ScrollSeek(_)
                    ),
                    View::Genres
                )
            }
            Message::Playlists(crate::views::PlaylistsMessage::ArtworkColumnDrag(ev)) => {
                self.handle_artwork_column_drag(ev)
            }
            Message::Playlists(crate::views::PlaylistsMessage::ArtworkColumnVerticalDrag(ev)) => {
                self.handle_artwork_vertical_drag(ev)
            }
            Message::Playlists(msg) => {
                dispatch_view_with_seek!(
                    self,
                    msg,
                    handle_playlists,
                    crate::views::PlaylistsMessage::SlotList(
                        crate::widgets::SlotListPageMessage::ScrollSeek(_)
                    ),
                    View::Playlists
                )
            }
            Message::Radios(msg) => {
                dispatch_view_with_seek!(
                    self,
                    msg,
                    handle_radios,
                    crate::views::RadiosMessage::SlotList(
                        crate::widgets::SlotListPageMessage::ScrollSeek(_)
                    ),
                    View::Radios
                )
            }
            Message::Settings(msg) => self.handle_settings(msg),

            // -----------------------------------------------------------------
            // MPRIS D-Bus Integration
            // -----------------------------------------------------------------
            Message::Mpris(event) => self.handle_mpris(event),

            // -----------------------------------------------------------------
            // System Tray (StatusNotifierItem)
            // -----------------------------------------------------------------
            Message::Tray(event) => self.handle_tray(event),
            Message::WindowOpened(id) => self.handle_window_opened(id),
            Message::WindowCloseRequested(id) => self.handle_window_close_requested(id),

            // -----------------------------------------------------------------
            // Visualizer / Theme / Settings Hot-Reload (see update/config.rs)
            // -----------------------------------------------------------------
            Message::VisualizerConfigChanged(config) => {
                self.handle_visualizer_config_changed(config)
            }
            Message::ThemeConfigReloaded => self.handle_theme_config_reloaded(),
            Message::SettingsConfigReloaded => self.handle_settings_config_reloaded(),
            Message::SettingsReloadDataLoaded(vp, hotkeys, settings) => {
                self.handle_settings_reload_data_loaded(vp, hotkeys, settings)
            }

            // -----------------------------------------------------------------
            // Raw Keyboard Events → HotkeyConfig dispatch (see hotkeys/mod.rs)
            // -----------------------------------------------------------------
            Message::RawKeyEvent(key, modifiers, status) => {
                self.handle_raw_key_event(key, modifiers, status)
            }
            Message::ModifiersChanged(modifiers) => self.handle_modifiers_changed(modifiers),

            // -----------------------------------------------------------------
            // Toast Notifications
            // -----------------------------------------------------------------
            Message::Toast(msg) => self.handle_toast(msg),

            // -----------------------------------------------------------------
            // Task Manager Notifications
            // -----------------------------------------------------------------
            Message::TaskStatusChanged(handle, status) => {
                use nokkvi_data::services::task_manager::TaskStatus;
                match status {
                    TaskStatus::Running => {
                        // Optional: update active progress list or show a toast
                        tracing::trace!(" [TASK] {} is running", handle.name);
                    }
                    TaskStatus::Completed => {
                        tracing::debug!(" [TASK] {} completed", handle.name);
                    }
                    TaskStatus::Failed(e) => {
                        self.toast_error(format!("Task failed: {} - {}", handle.name, e));
                    }
                    TaskStatus::Cancelled => {
                        tracing::debug!(" [TASK] {} cancelled", handle.name);
                    }
                }
                Task::none()
            }

            // -----------------------------------------------------------------
            // Text Input Dialog
            // -----------------------------------------------------------------
            Message::TextInputDialog(msg) => self.handle_text_input_dialog(msg),

            // -----------------------------------------------------------------
            // Playlist Edit Mode (split-view)
            // -----------------------------------------------------------------
            Message::BrowsingPanel(msg) => self.handle_browsing_panel_message(msg),
            Message::EnterPlaylistEditMode {
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
            Message::ExitPlaylistEditMode => self.handle_exit_playlist_edit_mode(),
            Message::ToggleBrowsingPanel => self.handle_toggle_browsing_panel(),
            Message::SwitchPaneFocus => self.handle_switch_pane_focus(),
            Message::SavePlaylistEdits => self.handle_save_playlist_edits(),
            Message::PlaylistEditsSaved => self.handle_playlist_edits_saved(),

            // -----------------------------------------------------------------
            // Info Modal
            // -----------------------------------------------------------------
            Message::InfoModal(msg) => self.handle_info_modal(msg),

            // -----------------------------------------------------------------
            // About Modal
            // -----------------------------------------------------------------
            Message::AboutModal(msg) => self.handle_about_modal(msg),

            // -----------------------------------------------------------------
            // EQ Modal
            // -----------------------------------------------------------------
            Message::EqModal(msg) => self.handle_eq_modal(msg),

            // -----------------------------------------------------------------
            // Default Playlist Picker (header chip → modal overlay)
            // -----------------------------------------------------------------
            Message::DefaultPlaylistPicker(msg) => self.handle_default_playlist_picker(msg),

            // -----------------------------------------------------------------
            // Cross-Pane Drag (browsing panel → queue)
            // -----------------------------------------------------------------
            Message::CrossPaneDragPressed => self.handle_cross_pane_drag_pressed(),
            Message::CrossPaneDragMoved(pos) => self.handle_cross_pane_drag_moved(pos),
            Message::CrossPaneDragReleased => self.handle_cross_pane_drag_released(),
            Message::CrossPaneDragCancel => self.handle_cross_pane_drag_cancel(),

            // -----------------------------------------------------------------
            // Show in File Manager
            // -----------------------------------------------------------------
            Message::ShowInFolder(path) => self.handle_show_in_folder(path),

            // -----------------------------------------------------------------
            // Similar Songs
            // -----------------------------------------------------------------
            Message::Similar(crate::views::SimilarMessage::ArtworkColumnDrag(ev)) => {
                self.handle_artwork_column_drag(ev)
            }
            Message::Similar(crate::views::SimilarMessage::ArtworkColumnVerticalDrag(ev)) => {
                self.handle_artwork_vertical_drag(ev)
            }
            Message::Similar(msg) => self.handle_similar_message(msg),
            Message::FindSimilar { id, label } => self.handle_find_similar(id, label),
            Message::FindTopSongs { artist_name, label } => {
                self.handle_find_top_songs(artist_name, label)
            }
            Message::SimilarSongsLoaded(generation, result, label) => {
                self.handle_similar_songs_loaded(generation, result, label)
            }

            // -----------------------------------------------------------------
            // Surfing-Boat Overlay (lines mode)
            // -----------------------------------------------------------------
            Message::BoatTick(now) => boat::handle_boat_tick(self, now),
        }
    }

    /// Shared handler for artwork-column drag events emitted by every view's
    /// drag handle. `Change` only updates the live atomic; `Commit` also
    /// persists to TOML via the settings backend.
    fn handle_artwork_column_drag(
        &mut self,
        ev: crate::widgets::artwork_split_handle::DragEvent,
    ) -> Task<Message> {
        use crate::widgets::artwork_split_handle::DragEvent;
        match ev {
            DragEvent::Change(pct) => {
                crate::theme::set_artwork_column_width_pct(pct);
            }
            DragEvent::Commit(pct) => {
                crate::theme::set_artwork_column_width_pct(pct);
                let final_pct = crate::theme::artwork_column_width_pct();
                self.shell_spawn("persist_artwork_column_width", move |shell| async move {
                    shell
                        .settings()
                        .set_artwork_column_width_pct(final_pct)
                        .await
                });
            }
        }
        Task::none()
    }

    /// Shared handler for Always-Vertical artwork drag events. Mirrors
    /// `handle_artwork_column_drag` but stores into the vertical-height
    /// atomic / SettingsManager setter.
    fn handle_artwork_vertical_drag(
        &mut self,
        ev: crate::widgets::artwork_split_handle::DragEvent,
    ) -> Task<Message> {
        use crate::widgets::artwork_split_handle::DragEvent;
        match ev {
            DragEvent::Change(pct) => {
                crate::theme::set_artwork_vertical_height_pct(pct);
            }
            DragEvent::Commit(pct) => {
                crate::theme::set_artwork_vertical_height_pct(pct);
                let final_pct = crate::theme::artwork_vertical_height_pct();
                self.shell_spawn("persist_artwork_vertical_height", move |shell| async move {
                    shell
                        .settings()
                        .set_artwork_vertical_height_pct(final_pct)
                        .await
                });
            }
        }
        Task::none()
    }
}
