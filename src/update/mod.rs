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
mod editor;
mod eq_modal;
mod genres;
mod hotkeys;
mod info_modal;
mod ipc;
mod library_filter;
mod library_refresh;
mod loader_target;
mod menus;
mod mpris;
mod navigation;
mod notifications;
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
mod tests_player_settings;
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
pub(crate) use ipc::{CLI_ARGS as IPC_CLI_ARGS, CliArgType, KNOWN_COMMANDS as IPC_KNOWN_COMMANDS};
pub(crate) use loader_target::{
    AlbumsTarget, ArtistsTarget, GenresTarget, PlaylistsTarget, SongsTarget,
};
pub(crate) use pending_expand_resolve::{AlbumSpec, ArtistSpec, GenreSpec, SongSpec};

use crate::{Nokkvi, View, app_message::Message};

impl Nokkvi {
    /// Central message handler
    ///
    /// Routes messages to appropriate handlers organized by domain.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // -----------------------------------------------------------------
            // Navigation
            // -----------------------------------------------------------------
            Message::Navigation(nav) => self.handle_navigation(nav),
            Message::LibraryChanged(change) => self.handle_library_changed(change),
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
                filter,
                offset,
                total_count,
                generation,
            } => self.handle_progressive_queue_append_page(
                sort_mode,
                sort_order,
                search_query,
                filter,
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
                        self.active_playlist_info = Some(
                            self.library
                                .playlists
                                .iter()
                                .find(|p| p.id == *id)
                                .map_or_else(
                                    || {
                                        crate::state::ActivePlaylistContext::minimal(
                                            id.clone(),
                                            name.clone(),
                                            String::new(),
                                        )
                                    },
                                    crate::state::ActivePlaylistContext::from_playlist,
                                ),
                        );
                        self.persist_active_playlist_info();
                        // The queue IS the just-saved playlist's content —
                        // freeze the strip quad from it now; no queue reload
                        // will fire to do it for us.
                        self.snapshot_strip_quad_ids();
                    }
                    _ => {}
                }
                self.toast_success(mutation.to_string());
                // When the append targeted the playlist currently open in the
                // editor, re-resolve its decoupled buffer so the new tracks
                // appear (the editor never re-fetches on its own after the
                // initial load). Gated on a clean, loaded session so unsaved
                // staged edits are not discarded.
                let editor_reload = match &mutation {
                    crate::app_message::PlaylistMutation::Appended { id, .. } => {
                        self.editor_reload_task_if_clean_match(id)
                    }
                    _ => Task::none(),
                };
                Task::batch([editor_reload, self.handle_load_playlists()])
            }
            Message::PlaylistsFetchedForDialog(playlists) => {
                self.text_input_dialog.open_save_playlist(&playlists);
                Task::none()
            }
            Message::PlaylistsFetchedForAddToPlaylist(playlists, song_ids) => {
                // Quick-add bypass: skip dialog when default playlist is configured
                if self.settings.quick_add_to_playlist
                    && let Some(ref default_id) = self.settings.default_playlist_id
                {
                    let playlist_id = default_id.clone();
                    let id_for_msg = playlist_id.clone();
                    let playlist_name = self.settings.default_playlist_name.clone();
                    let count = song_ids.len();
                    return self.shell_action_task(
                        move |shell| async move {
                            let service = shell.playlists_api().await?;
                            service.add_songs_to_playlist(&playlist_id, &song_ids).await
                        },
                        Message::PlaylistMutated(crate::app_message::PlaylistMutation::Appended {
                            name: format!(
                                "{playlist_name}' ({count} song{})",
                                if count == 1 { "" } else { "s" }
                            ),
                            id: id_for_msg,
                        }),
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
            Message::Library(msg) => self.handle_library_message(msg),

            // -----------------------------------------------------------------
            // Component Message Bubbling
            // -----------------------------------------------------------------
            Message::PlayerBar(msg) => self.handle_player_bar(msg),
            Message::NavBar(_msg) => {
                // NavBar messages are handled via map() in navigation_bar()
                // SwitchView -> Message::Navigation(NavigationMessage::SwitchView(..)),
                // ToggleLightMode -> Message::ToggleLightMode, etc.
                Task::none()
            }
            Message::ToggleLightMode => self.handle_toggle_light_mode(),
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
            Message::Queue(msg) => self.handle_queue(msg),
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
            // Rating-reminder desktop notifications
            // -----------------------------------------------------------------
            Message::Notification(event) => self.handle_notification(event),

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
                self.handle_task_status_changed(handle, status)
            }

            // -----------------------------------------------------------------
            // Text Input Dialog
            // -----------------------------------------------------------------
            Message::TextInputDialog(msg) => self.handle_text_input_dialog(msg),

            // -----------------------------------------------------------------
            // Playlist Edit Mode (split-view)
            // -----------------------------------------------------------------
            Message::BrowsingPanel(msg) => self.handle_browsing_panel_message(msg),
            Message::SplitView(msg) => self.handle_split_view_message(msg),
            // Phase 1: no-op stub; real handling lands in Phase 3+.
            Message::Editor(msg) => self.handle_editor_message(msg),

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
            Message::CrossPaneDrag(msg) => self.handle_cross_pane_drag_message(msg),

            // -----------------------------------------------------------------
            // Show in File Manager
            // -----------------------------------------------------------------
            Message::ShowInFolder(path) => self.handle_show_in_folder(path),

            // -----------------------------------------------------------------
            // Similar Songs
            // -----------------------------------------------------------------
            Message::Similar(msg) => self.handle_similar_message(msg),
            Message::Find(msg) => self.handle_find_message(msg),

            // -----------------------------------------------------------------
            // Surfing-Boat Overlay (lines mode)
            // -----------------------------------------------------------------
            Message::BoatTick(now) => boat::handle_boat_tick(self, now),

            // -----------------------------------------------------------------
            // IPC (nokkvi-ipc workspace crate)
            // -----------------------------------------------------------------
            Message::Ipc(incoming) => ipc::handle(self, *incoming),
        }
    }

    /// Shared handler for artwork-column drag events emitted by every view's
    /// drag handle. `Change` only updates the live atomic; `Commit` also
    /// persists to TOML via the settings backend.
    pub(crate) fn handle_artwork_column_drag(
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
    pub(crate) fn handle_artwork_vertical_drag(
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
        // The Interface tab mirrors this atomic as a settings row
        // (`general.artwork_vertical_height_pct`); the drag happens in a
        // library view, so a dirty mark is enough — the entry refreshes on
        // the next Settings entry. (`artwork_column_width_pct` has no
        // settings row, so `handle_artwork_column_drag` stays unmarked.)
        self.settings_page.config_dirty = true;
        Task::none()
    }
}
