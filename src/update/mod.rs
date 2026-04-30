//! Update function for Nokkvi
//!
//! Contains the central message handler and helper functions.
//! Message handlers are organized into submodules by domain.

/// DRY macro for `handle_*_page_loaded` handlers. Songs, albums, and artists
/// all follow the exact same pattern: append to paged buffer on Ok, log on Err.
macro_rules! impl_page_loaded_handler {
    ($self:ident, $field:ident, $label:expr, $result:expr, $total_count:expr) => {{
        match $result {
            Ok(new_items) => {
                let count = new_items.len();
                let loaded_before = $self.library.$field.loaded_count();
                $self.library.$field.append_page(new_items, $total_count);
                tracing::debug!(
                    "📄 {} page loaded: {} new items ({}→{} of {})",
                    $label,
                    count,
                    loaded_before,
                    $self.library.$field.loaded_count(),
                    $total_count,
                );
            }
            Err(e) => {
                if e.contains("Unauthorized") {
                    $self.library.$field.set_loading(false);
                    return $self.handle_session_expired();
                }
                tracing::error!("Error loading {} page: {}", $label, e);
                $self.library.$field.set_loading(false);
                $self.toast_error(format!("Failed to load {}: {}", $label, e));
            }
        }
        iced::Task::none()
    }};
}

/// DRY macro for view message dispatch with scroll-seek timer injection.
/// All non-Queue slot list views share the same pattern: check if the message
/// is a `SlotListScrollSeek`, call the handler, then append scrollbar fade
/// and seek-settled timers when it was a seek event.
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
mod browsing_panel;
mod collage;
mod components;
mod cross_pane_drag;
mod default_playlist_picker;
mod eq_modal;
mod genres;
mod hotkeys;
mod info_modal;
mod library_refresh;
mod menus;
mod mpris;
mod navigation;
mod playback;
mod player_bar;
mod playlists;
mod progressive_queue;
mod queue;
mod radios;
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

use iced::Task;
use tracing::debug;

use crate::{
    Nokkvi, View,
    app_message::{HotkeyMessage, Message, PlaybackMessage, ScrobbleMessage},
};

/// Fetch album IDs for a genre from the API.
/// Used as the `fetch_album_ids_fn` closure for genre collage artwork loading.
async fn load_genre_album_ids(
    client: nokkvi_data::services::api::client::ApiClient,
    server_url: String,
    subsonic_credential: String,
    entity_id: String,
) -> Vec<String> {
    let service = nokkvi_data::services::api::genres::GenresApiService::new_with_client(
        client,
        server_url,
        subsonic_credential,
    );
    service
        .load_genre_albums(&entity_id)
        .await
        .unwrap_or_default()
}

/// Fetch album IDs for a playlist from the API.
/// Used as the `fetch_album_ids_fn` closure for playlist collage artwork loading.
async fn load_playlist_album_ids(
    client: nokkvi_data::services::api::client::ApiClient,
    server_url: String,
    subsonic_credential: String,
    entity_id: String,
) -> Vec<String> {
    let service = nokkvi_data::services::api::playlists::PlaylistsApiService::new_with_client(
        client,
        server_url,
        subsonic_credential,
    );
    service
        .load_playlist_albums(&entity_id)
        .await
        .unwrap_or_default()
}

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
            Message::Albums(crate::views::AlbumsMessage::AlbumsLoaded {
                result,
                total_count,
                background,
                anchor_id,
            }) => self.handle_albums_loaded(result, total_count, background, anchor_id),
            Message::Albums(crate::views::AlbumsMessage::AlbumsPageLoaded(result, total_count)) => {
                self.handle_albums_page_loaded(result, total_count)
            }
            // -----------------------------------------------------------------
            // Data Loading: Queue
            // -----------------------------------------------------------------
            Message::LoadQueue => self.handle_load_queue(),
            Message::Queue(crate::views::QueueMessage::QueueLoaded(result)) => {
                self.handle_queue_loaded(result)
            }
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
            // Data Loading: Artists
            // -----------------------------------------------------------------
            Message::LoadArtists => self.handle_load_artists(false, None),
            Message::Artists(crate::views::ArtistsMessage::ArtistsLoaded {
                result,
                total_count,
                background,
                anchor_id,
            }) => self.handle_artists_loaded(result, total_count, background, anchor_id),
            Message::Artists(crate::views::ArtistsMessage::ArtistsPageLoaded(
                result,
                total_count,
            )) => self.handle_artists_page_loaded(result, total_count),

            // -----------------------------------------------------------------
            // Data Loading: Songs
            // -----------------------------------------------------------------
            Message::LoadSongs => self.handle_load_songs(false, None),
            Message::Songs(crate::views::SongsMessage::SongsLoaded {
                result,
                total_count,
                background,
                anchor_id,
            }) => self.handle_songs_loaded(result, total_count, background, anchor_id),
            Message::Songs(crate::views::SongsMessage::SongsPageLoaded(result, total_count)) => {
                self.handle_songs_page_loaded(result, total_count)
            }

            // -----------------------------------------------------------------
            // Data Loading: Genres
            // -----------------------------------------------------------------
            Message::LoadGenres => self.handle_load_genres(),
            Message::Genres(crate::views::GenresMessage::GenresLoaded(result, total_count)) => {
                self.handle_genres_loaded(result, total_count)
            }

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
            Message::Playlists(crate::views::PlaylistsMessage::PlaylistsLoaded(
                result,
                total_count,
            )) => self.handle_playlists_loaded(result, total_count),

            // -----------------------------------------------------------------
            // Artwork Pipeline (namespaced)
            // -----------------------------------------------------------------
            Message::Artwork(msg) => {
                use crate::app_message::{ArtworkMessage, CollageTarget};
                match msg {
                    // Shared album artwork
                    ArtworkMessage::Loaded(id, handle) => self.handle_artwork_loaded(id, handle),
                    ArtworkMessage::LoadLarge(album_id) => self.handle_load_large_artwork(album_id),
                    ArtworkMessage::LargeLoaded(id, handle) => {
                        self.handle_large_artwork_loaded(id, handle)
                    }
                    ArtworkMessage::LargeArtistLoaded(id, handle, color) => {
                        if let Some(h) = handle {
                            self.artwork.large_artwork.put(id.clone(), h);
                            self.artwork.refresh_large_artwork_snapshot();
                        }
                        if let Some(c) = color {
                            self.artwork.album_dominant_colors.put(id, c);
                            self.artwork.refresh_dominant_colors_snapshot();
                        }
                        self.artwork.loading_large_artwork = None;
                        Task::none()
                    }
                    ArtworkMessage::DominantColorCalculated(id, color) => {
                        self.artwork.album_dominant_colors.put(id, color);
                        self.artwork.refresh_dominant_colors_snapshot();
                        Task::none()
                    }
                    ArtworkMessage::RefreshAlbumArtwork(album_id) => {
                        self.handle_refresh_album_artwork(album_id)
                    }
                    ArtworkMessage::RefreshAlbumArtworkSilent(album_id) => {
                        self.handle_refresh_album_artwork_silent(album_id)
                    }
                    ArtworkMessage::RefreshComplete(album_id, thumb, large, silent) => {
                        self.handle_refresh_complete(album_id, thumb, large, silent)
                    }
                    // Collage artwork pipeline (genre / playlist)
                    ArtworkMessage::LoadCollage(target, id, server_url, cred, album_ids) => {
                        match target {
                            CollageTarget::Genre => self.handle_load_collage_artwork(
                                target,
                                id,
                                server_url,
                                cred,
                                album_ids,
                                load_genre_album_ids,
                            ),
                            CollageTarget::Playlist => self.handle_load_collage_artwork(
                                target,
                                id,
                                server_url,
                                cred,
                                album_ids,
                                load_playlist_album_ids,
                            ),
                        }
                    }
                    ArtworkMessage::StartCollagePrefetch(target) => {
                        // Collect items needing album IDs from the appropriate library
                        let items_needing_ids: Vec<(String, String)> = match target {
                            CollageTarget::Genre => self
                                .library
                                .genres
                                .iter()
                                .filter(|g| g.artwork_album_ids.is_empty())
                                .map(|g| (g.id.clone(), g.name.clone()))
                                .collect(),
                            CollageTarget::Playlist => self
                                .library
                                .playlists
                                .iter()
                                .filter(|p| p.artwork_album_ids.is_empty())
                                .map(|p| (p.id.clone(), p.name.clone()))
                                .collect(),
                        };
                        match target {
                            CollageTarget::Genre => self.handle_start_collage_prefetch(
                                target,
                                items_needing_ids,
                                load_genre_album_ids,
                            ),
                            CollageTarget::Playlist => self.handle_start_collage_prefetch(
                                target,
                                items_needing_ids,
                                load_playlist_album_ids,
                            ),
                        }
                    }
                    ArtworkMessage::CollageAlbumIdsLoaded(target, results) => {
                        self.handle_collage_album_ids_loaded(target, results)
                    }
                    ArtworkMessage::LoadCollageFromIds(target) => {
                        self.handle_load_collage_artwork_from_ids(target)
                    }
                    ArtworkMessage::CollageMiniLoaded(target, id, handle_opt) => {
                        self.handle_collage_mini_loaded(target, id, handle_opt)
                    }
                    ArtworkMessage::CollageLoaded(
                        target,
                        id,
                        handle_opt,
                        collage_handles,
                        album_ids,
                    ) => self.handle_collage_artwork_loaded(
                        target,
                        id,
                        handle_opt,
                        collage_handles,
                        album_ids,
                    ),
                    ArtworkMessage::CollageBatchLoaded(target, results) => {
                        self.handle_collage_batch_loaded(target, results)
                    }
                    ArtworkMessage::CollageBatchReady(target, ids, server_url, cred) => {
                        Task::batch(ids.into_iter().map(|id| {
                            Task::done(Message::Artwork(ArtworkMessage::LoadCollage(
                                target,
                                id,
                                server_url.clone(),
                                cred.clone(),
                                Vec::new(),
                            )))
                        }))
                    }
                    // Song artwork
                    ArtworkMessage::SongMiniLoaded(album_id, handle) => {
                        self.handle_song_artwork_loaded(album_id, handle)
                    }
                }
            }

            // -----------------------------------------------------------------
            // Playback (namespaced under PlaybackMessage)
            // -----------------------------------------------------------------
            Message::Playback(msg) => {
                use crate::app_message::PlaybackMessage;
                match msg {
                    PlaybackMessage::Tick => {
                        // ── Stale-loading watchdog ──────────────────────────────
                        // Safety net: if a buffer has been in "loading" state for
                        // more than 30 seconds, something went wrong (network
                        // timeout, dropped task, etc). Auto-clear so the view is
                        // usable and warn so the root cause can be investigated.
                        let stale_timeout = std::time::Duration::from_secs(30);
                        let stale_views: Vec<&str> = [
                            (
                                "albums",
                                self.library.albums.is_stale_loading(stale_timeout),
                            ),
                            (
                                "artists",
                                self.library.artists.is_stale_loading(stale_timeout),
                            ),
                            ("songs", self.library.songs.is_stale_loading(stale_timeout)),
                            (
                                "genres",
                                self.library.genres.is_stale_loading(stale_timeout),
                            ),
                            (
                                "playlists",
                                self.library.playlists.is_stale_loading(stale_timeout),
                            ),
                        ]
                        .iter()
                        .filter(|(_, stale)| *stale)
                        .map(|(name, _)| *name)
                        .collect();

                        for view_name in &stale_views {
                            tracing::warn!(
                                "⚠️ Stale loading state detected for {} (loading for >30s), auto-clearing",
                                view_name
                            );
                        }
                        if !stale_views.is_empty() {
                            // Clear all stale buffers
                            self.library.albums.set_loading(false);
                            self.library.artists.set_loading(false);
                            self.library.songs.set_loading(false);
                            self.library.genres.set_loading(false);
                            self.library.playlists.set_loading(false);
                            self.toast_warn(format!(
                                "Loading timed out for: {}. Please retry.",
                                stale_views.join(", ")
                            ));
                        }

                        // Poll active progress handles and update sticky toasts.
                        // Collect snapshots first to avoid borrow conflicts with self.
                        let snapshots: Vec<_> = self
                            .active_progress
                            .iter()
                            .map(|h| (h.toast_key(), h.snapshot()))
                            .collect();

                        let mut completed_indices = Vec::new();
                        for (i, (toast_key, snap)) in snapshots.iter().enumerate() {
                            if snap.done {
                                self.toast.dismiss_key(toast_key);
                                self.toast_success(format!("{} ✓", snap.label));
                                completed_indices.push(i);
                            } else if snap.total > 0 {
                                let pct = snap.percent();
                                let msg = format!("{}… {}%", snap.label, pct);
                                self.toast.push(nokkvi_data::types::toast::Toast::keyed(
                                    toast_key.clone(),
                                    msg,
                                    nokkvi_data::types::toast::ToastLevel::Info,
                                ));
                            }
                        }
                        // Remove completed handles (iterate in reverse to preserve indices)
                        for i in completed_indices.into_iter().rev() {
                            self.active_progress.remove(i);
                        }

                        self.handle_tick()
                    }
                    PlaybackMessage::PlaybackStateUpdated(update) => {
                        self.handle_playback_state_updated(*update)
                    }
                    PlaybackMessage::TogglePlay => self.handle_toggle_play(),
                    PlaybackMessage::Play => self.handle_play(),
                    PlaybackMessage::Pause => self.handle_pause(),
                    PlaybackMessage::Stop => self.handle_stop(),
                    PlaybackMessage::NextTrack => self.handle_next_track(),
                    PlaybackMessage::PrevTrack => self.handle_prev_track(),
                    PlaybackMessage::ToggleRandom => self.handle_toggle_random(),
                    PlaybackMessage::RandomToggled(random) => self.handle_random_toggled(random),
                    PlaybackMessage::ToggleRepeat => self.handle_toggle_repeat(),
                    PlaybackMessage::RepeatToggled(repeat, repeat_queue) => {
                        self.handle_repeat_toggled(repeat, repeat_queue)
                    }
                    PlaybackMessage::ToggleConsume => self.handle_toggle_consume(),
                    PlaybackMessage::ConsumeToggled(consume) => {
                        self.handle_consume_toggled(consume)
                    }
                    PlaybackMessage::ToggleSoundEffects => self.handle_toggle_sound_effects(),
                    PlaybackMessage::SfxVolumeChanged(vol) => self.handle_sfx_volume_changed(vol),
                    PlaybackMessage::CycleVisualization => self.handle_cycle_visualization(),
                    PlaybackMessage::ToggleCrossfade => self.handle_toggle_crossfade(),
                    PlaybackMessage::Seek(val) => self.handle_seek(val),
                    PlaybackMessage::VolumeChanged(val) => self.handle_volume_changed(val),
                    PlaybackMessage::PrepareNextForGapless => {
                        self.handle_prepare_next_for_gapless()
                    }
                    PlaybackMessage::PlayerSettingsLoaded(settings) => {
                        self.handle_player_settings_loaded(*settings)
                    }
                    PlaybackMessage::InitializeScrobbleState(song_id) => {
                        self.handle_initialize_scrobble_state(song_id)
                    }
                    PlaybackMessage::RadioMetadataUpdate(artist, title) => {
                        self.handle_radio_metadata_update(artist, title, None)
                    }
                }
            }
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
            Message::HotkeyConfigUpdated(config) => {
                tracing::info!(" [SETTINGS] Hotkey config hot-reloaded");
                self.hotkey_config = config;
                Task::none()
            }
            Message::NoOp => Task::none(),
            Message::QuitApp => iced::exit(),
            Message::PlaySfx(sfx_type) => self.handle_play_sfx(sfx_type),

            // -----------------------------------------------------------------
            // Scrobbling (namespaced)
            // -----------------------------------------------------------------
            Message::Scrobble(msg) => match msg {
                ScrobbleMessage::NowPlaying(timer_id, song_id) => {
                    self.handle_scrobble_now_playing(timer_id, song_id)
                }
                ScrobbleMessage::Submit(song_id) => self.handle_scrobble_submit(song_id),
                ScrobbleMessage::SubmissionResult(result) => {
                    self.handle_scrobble_submission_result(result)
                }
                ScrobbleMessage::NowPlayingResult(result) => {
                    self.handle_scrobble_now_playing_result(result)
                }
                ScrobbleMessage::TrackLooped(song_id) => self.handle_scrobble_track_looped(song_id),
            },

            // -----------------------------------------------------------------
            // Hotkey Actions (namespaced)
            // -----------------------------------------------------------------
            Message::Hotkey(msg) => match msg {
                HotkeyMessage::ClearSearch => {
                    // If EQ modal is visible, Escape closes it first
                    if self.window.eq_modal_open {
                        self.window.eq_modal_open = false;
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
                HotkeyMessage::ShuffleQueue => self.handle_shuffle_queue(),
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
                HotkeyMessage::RefreshView => match self.current_view {
                    crate::View::Albums => Task::done(Message::LoadAlbums),
                    crate::View::Artists => Task::done(Message::LoadArtists),
                    crate::View::Songs => Task::done(Message::LoadSongs),
                    crate::View::Genres => Task::done(Message::LoadGenres),
                    crate::View::Playlists => Task::done(Message::LoadPlaylists),
                    crate::View::Radios => Task::done(Message::LoadRadioStations),
                    crate::View::Queue | crate::View::Settings => Task::none(),
                },
            },

            // -----------------------------------------------------------------
            // Component Message Bubbling
            // -----------------------------------------------------------------
            Message::PlayerBar(msg) => self.handle_player_bar(msg),
            Message::NavBar(_msg) => {
                // NavBar messages are handled via map() in navigation_bar()
                // SwitchView -> Message::SwitchView, ToggleLightMode -> Message::ToggleLightMode, etc.
                Task::none()
            }
            Message::ToggleLightMode => {
                // Toggle light mode: write to config.toml (single source of truth)
                let new_state = !crate::theme::is_light_mode();
                crate::theme::set_light_mode(new_state);
                debug!(" Light mode set to: {}", new_state);
                // Persist to config.toml — the config file watcher will pick this up
                // and ThemeConfigReloaded will read the correct value
                if let Err(e) = crate::config_writer::update_config_value(
                    "settings.light_mode",
                    &crate::views::settings::items::SettingValue::Bool(new_state),
                    None,
                ) {
                    tracing::warn!(" Failed to write light_mode to config.toml: {e}");
                }
                // Force UI refresh
                Task::done(Message::Playback(PlaybackMessage::Tick))
            }
            Message::Albums(crate::views::AlbumsMessage::ArtworkColumnDrag(ev)) => {
                self.handle_artwork_column_drag(ev)
            }
            Message::Albums(msg) => {
                dispatch_view_with_seek!(
                    self,
                    msg,
                    handle_albums,
                    crate::views::AlbumsMessage::SlotListScrollSeek(_),
                    View::Albums
                )
            }
            Message::Queue(crate::views::QueueMessage::ArtworkColumnDrag(ev)) => {
                self.handle_artwork_column_drag(ev)
            }
            Message::Queue(msg) => self.handle_queue(msg),
            Message::Artists(crate::views::ArtistsMessage::ArtworkColumnDrag(ev)) => {
                self.handle_artwork_column_drag(ev)
            }
            Message::Artists(msg) => {
                dispatch_view_with_seek!(
                    self,
                    msg,
                    handle_artists,
                    crate::views::ArtistsMessage::SlotListScrollSeek(_),
                    View::Artists
                )
            }
            Message::Songs(crate::views::SongsMessage::ArtworkColumnDrag(ev)) => {
                self.handle_artwork_column_drag(ev)
            }
            Message::Songs(msg) => {
                dispatch_view_with_seek!(
                    self,
                    msg,
                    handle_songs,
                    crate::views::SongsMessage::SlotListScrollSeek(_),
                    View::Songs
                )
            }
            Message::Genres(crate::views::GenresMessage::ArtworkColumnDrag(ev)) => {
                self.handle_artwork_column_drag(ev)
            }
            Message::Genres(msg) => {
                dispatch_view_with_seek!(
                    self,
                    msg,
                    handle_genres,
                    crate::views::GenresMessage::SlotListScrollSeek(_),
                    View::Genres
                )
            }
            Message::Playlists(crate::views::PlaylistsMessage::ArtworkColumnDrag(ev)) => {
                self.handle_artwork_column_drag(ev)
            }
            Message::Playlists(msg) => {
                dispatch_view_with_seek!(
                    self,
                    msg,
                    handle_playlists,
                    crate::views::PlaylistsMessage::SlotListScrollSeek(_),
                    View::Playlists
                )
            }
            Message::Radios(msg) => {
                dispatch_view_with_seek!(
                    self,
                    msg,
                    handle_radios,
                    crate::views::RadiosMessage::SlotListScrollSeek(_),
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
            // Visualizer Hot-Reload
            // -----------------------------------------------------------------
            Message::VisualizerConfigChanged(config) => {
                // Update shared config state
                {
                    let mut cfg = self.visualizer_config.write();
                    debug!(
                        " Applying new visualizer config: noise_reduction={:.2}, waves={}, bar_spacing={:.1}",
                        config.noise_reduction, config.waves, config.bars.bar_spacing
                    );
                    *cfg = config;
                }
                // Apply config to visualizer (reinitializes spectrum engine with new params)
                if let Some(ref vis) = self.visualizer {
                    vis.apply_config();
                }
                // Mark settings dirty so entries show updated values
                self.settings_page.config_dirty = true;
                Task::none()
            }

            // -----------------------------------------------------------------
            // Theme Hot-Reload
            // -----------------------------------------------------------------
            Message::ThemeConfigReloaded => {
                // Reload theme colors from config.toml
                crate::theme::reload_theme();
                // Also apply light_mode from config — this is for script-driven
                // demos (visualizer_showcase.py --both-modes), not user-facing config.
                // The in-app toggle + redb is the intended user mechanism.
                let config_light_mode = crate::theme_config::load_light_mode_from_config();
                if config_light_mode != crate::theme::is_light_mode() {
                    crate::theme::set_light_mode(config_light_mode);
                    debug!(" Light mode set to {} from config.toml", config_light_mode);
                }
                // Force UI refresh so all widgets pick up new colors
                self.settings_page.config_dirty = true;
                if self.current_view == View::Settings {
                    let new_data = self.build_settings_view_data();
                    self.settings_page.refresh_entries(&new_data);
                    self.settings_page.config_dirty = false;
                }
                Task::done(Message::Playback(crate::app_message::PlaybackMessage::Tick))
            }

            // -----------------------------------------------------------------
            // Settings Hot-Reload
            // -----------------------------------------------------------------
            Message::SettingsConfigReloaded => {
                tracing::info!(" [SETTINGS] Config file modified, reloading settings");
                self.shell_task(
                    |shell| async move {
                        shell.settings().reload_from_toml().await;
                        let vp = shell.settings().get_view_preferences().await;
                        let hotkeys = shell
                            .settings()
                            .settings_manager()
                            .lock()
                            .await
                            .get_hotkey_config_owned();
                        let settings = shell
                            .settings()
                            .settings_manager()
                            .lock()
                            .await
                            .get_player_settings();
                        Ok((vp, hotkeys, settings))
                    },
                    |result: Result<_, anyhow::Error>| match result {
                        Ok((vp, hotkeys, settings)) => {
                            Message::SettingsReloadDataLoaded(vp, hotkeys, Box::new(settings))
                        }
                        Err(e) => {
                            tracing::error!("Failed to reload settings: {}", e);
                            Message::NoOp
                        }
                    },
                )
            }
            Message::SettingsReloadDataLoaded(vp, hotkeys, settings) => {
                // Settings loaded from TOML re-apply to the UI
                self.settings_page.config_dirty = true;
                Task::batch([
                    self.handle_view_preferences_loaded(vp),
                    self.update(Message::HotkeyConfigUpdated(hotkeys)),
                    self.update(Message::Playback(
                        crate::app_message::PlaybackMessage::PlayerSettingsLoaded(settings),
                    )),
                ])
            }

            // -----------------------------------------------------------------
            // Raw Keyboard Events → HotkeyConfig dispatch
            // -----------------------------------------------------------------
            Message::RawKeyEvent(key, modifiers, status) => {
                // If settings is in hotkey capture mode, forward the raw event there
                // instead of dispatching it as a normal hotkey action
                if self.settings_page.capturing_hotkey.is_some() {
                    return self.handle_settings(crate::views::SettingsMessage::HotkeyCaptured(
                        key, modifiers,
                    ));
                }

                // When a widget (e.g. text_input search bar) has captured the
                // key event, suppress hotkey dispatch to avoid triggering actions
                // while the user is typing. Exceptions:
                //   - Escape: always allowed (close overlays, clear search)
                //   - Ctrl+key: always allowed (intentional shortcuts like Ctrl+S)
                if status == iced::event::Status::Captured {
                    let is_escape = matches!(
                        key,
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
                    );
                    let is_tab = matches!(
                        key,
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab)
                    );
                    if !is_escape && !is_tab && !modifiers.control() {
                        return Task::none();
                    }
                }

                // Look up the key event against the user's hotkey config
                match crate::hotkeys::handle_hotkey(key, modifiers, &self.hotkey_config) {
                    Some(msg) => self.update(msg),
                    None => Task::none(),
                }
            }
            Message::ModifiersChanged(modifiers) => {
                self.window.keyboard_modifiers = modifiers;
                Task::none()
            }

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
                        tracing::debug!(" [TASK] {} is running", handle.name);
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
            } => self.handle_enter_playlist_edit_mode(playlist_id, playlist_name, playlist_comment),
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
            Message::Similar(msg) => self.handle_similar_message(msg),
            Message::FindSimilar { id, label } => self.handle_find_similar(id, label),
            Message::FindTopSongs { artist_name, label } => {
                self.handle_find_top_songs(artist_name, label)
            }
            Message::SimilarSongsLoaded(generation, result, label) => {
                self.handle_similar_songs_loaded(generation, result, label)
            }
            Message::ArtworkColumnDragChange(pct) => self.handle_artwork_column_drag(
                crate::widgets::artwork_split_handle::DragEvent::Change(pct),
            ),
            Message::ArtworkColumnDragCommit(pct) => self.handle_artwork_column_drag(
                crate::widgets::artwork_split_handle::DragEvent::Commit(pct),
            ),
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
}
