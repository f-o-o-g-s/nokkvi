//! Settings view handlers
//!
//! Handles SettingsAction values returned by `SettingsPage::update()`.
//! The main handler dispatches to sub-handlers by action category:
//! - Config writes: theme/visualizer TOML values
//! - Hotkey actions: binding writes, resets, steals
//! - General settings: redb-persisted app preferences
//! - System actions: artwork rebuild, logout

use iced::Task;

use crate::{Nokkvi, app_message::Message};

impl Nokkvi {
    /// Build the settings view data, including the radio-scrobble connection
    /// status (read from `AppService`). This is always read — `data` is handed
    /// to `SettingsPage::update` on every non-nav message and is applied to the
    /// credential rows, so a status-less build would clobber a previously
    /// correct "Saved"/"Connected" badge with "Not set". Plain keystroke nav is
    /// already short-circuited before this is called (the nav-only fast path in
    /// `handle_settings`), so this is never on the per-frame hot path.
    pub(crate) fn build_settings_view_data(&self) -> crate::views::SettingsViewData {
        use nokkvi_data::types::settings_data::{
            GeneralSettingsData, InterfaceSettingsData, PlaybackSettingsData,
        };

        use crate::visualizer_config::SharedVisualizerConfigExt;
        let viz_config = self.visualizer_config.snapshot();
        let theme_file = crate::theme_config::load_active_theme_file();
        let active_theme_stem = nokkvi_data::services::theme_loader::read_theme_name_from_config();

        let nav_layout_label = if crate::theme::is_side_nav() {
            "Side"
        } else if crate::theme::is_none_nav() {
            "None"
        } else {
            "Top"
        };

        let general = GeneralSettingsData {
            server_url: self.login_page.server_url.clone().into(),
            username: self.login_page.username.clone().into(),
            start_view: self.settings.start_view.clone().into(),
            stable_viewport: self.settings.stable_viewport,
            auto_follow_playing: self.settings.auto_follow_playing,
            enter_behavior: self.settings.enter_behavior.as_label().into(),
            enter_shuffle: self.settings.enter_shuffle,
            local_music_path: self.settings.local_music_path.clone().into(),
            verbose_config: self.settings.verbose_config.as_label().into(),
            library_page_size: self.settings.library_page_size.as_label().into(),
            artwork_resolution: self.settings.artwork_resolution.as_label().into(),
            show_album_artists_only: self.settings.show_album_artists_only,
            suppress_library_refresh_toasts: self.settings.suppress_library_refresh_toasts,
            show_tray_icon: self.settings.show_tray_icon,
            close_to_tray: self.settings.close_to_tray,
        };

        let interface = InterfaceSettingsData {
            nav_layout: nav_layout_label.into(),
            nav_display_mode: crate::theme::nav_display_mode().as_label().into(),
            track_info_display: crate::theme::track_info_display().as_label().into(),
            slot_row_height: crate::theme::slot_row_height_variant().as_label().into(),
            horizontal_volume: crate::theme::is_horizontal_volume(),
            autohide_toolbar: crate::theme::is_autohide_toolbar(),
            autohide_toolbar_height: i64::from(crate::theme::autohide_toolbar_height_px()),
            autohide_toolbar_grip: crate::theme::is_autohide_toolbar_grip(),
            autohide_collapsed_appearance: crate::theme::autohide_collapsed_appearance()
                .as_label()
                .into(),
            mini_player_show_volume: crate::theme::mini_player_show_volume(),
            mini_player_show_modes: crate::theme::mini_player_show_modes(),
            slot_text_links: crate::theme::is_slot_text_links(),
            scrollbar_visibility: crate::theme::scrollbar_visibility().as_label().into(),
            icon_set: crate::theme::icon_set().as_label().into(),
            font_family: crate::theme::font_family().into(),
            strip_show_title: crate::theme::strip_show_title(),
            strip_show_artist: crate::theme::strip_show_artist(),
            strip_show_album: crate::theme::strip_show_album(),
            strip_show_format_info: crate::theme::strip_show_format_info(),
            strip_merged_mode: crate::theme::strip_merged_mode(),
            strip_show_labels: crate::theme::strip_show_labels(),
            strip_separator: crate::theme::strip_separator().as_label().into(),
            strip_click_action: crate::theme::strip_click_action().as_label().into(),
            albums_artwork_overlay: crate::theme::albums_artwork_overlay(),
            artists_artwork_overlay: crate::theme::artists_artwork_overlay(),
            songs_artwork_overlay: crate::theme::songs_artwork_overlay(),
            playlists_artwork_overlay: crate::theme::playlists_artwork_overlay(),
            artwork_column_mode: crate::theme::artwork_column_mode().as_label().into(),
            artwork_column_stretch_fit: crate::theme::artwork_column_stretch_fit()
                .as_label()
                .into(),
            artwork_auto_max_pct: f64::from(crate::theme::artwork_auto_max_pct()),
            artwork_vertical_height_pct: f64::from(crate::theme::artwork_vertical_height_pct()),
        };

        // Radio-scrobble connection status, resolved from a SINGLE config.toml
        // read, reporting which layer (env / config.toml / redb) supplies each
        // credential so the rows can show the source and warn on a shadowed GUI
        // clear (review #2 / #11).
        use nokkvi_data::services::radio_scrobble::source::CredSource;
        let (listenbrainz_source, lastfm_credentials_source, lastfm_username) = self
            .app_service
            .as_ref()
            .map_or((CredSource::Unset, CredSource::Unset, String::new()), |s| {
                let creds = s.radio_credentials();
                (
                    creds.listenbrainz_source,
                    creds.lastfm_source,
                    creds.lastfm_username.unwrap_or_default(),
                )
            });

        let playback = PlaybackSettingsData {
            crossfade_enabled: self.engine.crossfade_enabled,
            // The live mirror (flipped synchronously by the player-bar toggle),
            // same pattern as the crossfade engine mirror above.
            lyrics_enabled: self.lyrics.enabled,
            lyrics_fetch_online: self.settings.lyrics_fetch_online,
            lyrics_backdrop_blur: self.settings.lyrics_backdrop_blur.as_label().into(),
            bit_perfect: self.engine.bit_perfect_mode.as_label().into(),
            crossfade_duration_secs: i64::from(self.engine.crossfade_duration_secs),
            crossfade_curve: self.settings.crossfade_curve.as_label().into(),
            crossfade_min_track_secs: i64::from(self.settings.crossfade_min_track_secs),
            crossfade_album_gapless: self.settings.crossfade_album_gapless,
            smooth_track_starts: self.settings.smooth_track_starts,
            fade_on_pause: self.settings.fade_on_pause,
            fade_pause_ms: i64::from(self.settings.fade_pause_ms),
            fade_on_stop: self.settings.fade_on_stop,
            fade_stop_ms: i64::from(self.settings.fade_stop_ms),
            fade_radio_transitions: self.settings.fade_radio_transitions,
            fade_on_skip: self.settings.fade_on_skip.as_label().into(),
            fade_skip_secs: i64::from(self.settings.fade_skip_secs),
            skip_silence: self.settings.skip_silence,
            crossfade_offset_secs: i64::from(self.settings.crossfade_offset_secs),
            crossfade_bar_snap: self.settings.crossfade_bar_snap,
            rewind_on_previous: self.settings.rewind_on_previous,
            volume_normalization: self.engine.volume_normalization.as_label().into(),
            normalization_level: self.engine.normalization_level.as_label().into(),
            replay_gain_preamp_db: self.engine.replay_gain_preamp_db.round() as i64,
            replay_gain_fallback_db: self.engine.replay_gain_fallback_db.round() as i64,
            replay_gain_fallback_to_agc: self.engine.replay_gain_fallback_to_agc,
            replay_gain_prevent_clipping: self.engine.replay_gain_prevent_clipping,
            scrobbling_enabled: self.settings.scrobbling_enabled,
            scrobble_threshold: f64::from(self.settings.scrobble_threshold),
            radio_scrobbling_enabled: self.settings.radio_scrobbling_enabled,
            radio_scrobble_threshold_secs: i64::from(self.settings.radio_scrobble_threshold_secs),
            radio_now_playing_enabled: self.settings.radio_now_playing_enabled,
            listenbrainz_source,
            lastfm_credentials_source,
            lastfm_username: lastfm_username.into(),
            quick_add_to_playlist: self.settings.quick_add_to_playlist,
            default_playlist_name: self.settings.default_playlist_name.clone().into(),
            queue_show_default_playlist: self.settings.queue_show_default_playlist,
            rating_reminder_enabled: self.settings.rating_reminder_enabled,
            rating_change_notification_enabled: self.settings.rating_change_notification_enabled,
            love_change_notification_enabled: self.settings.love_change_notification_enabled,
            rating_reminder_trigger: self.settings.rating_reminder_trigger.as_label().into(),
            rating_reminder_percent: i64::from(self.settings.rating_reminder_percent),
        };

        crate::views::SettingsViewData {
            general,
            interface,
            playback,
            visualizer_config: viz_config,
            theme_file,
            active_theme_stem,
            hotkey_config: self.hotkey_config.clone(),
            is_light_mode: crate::theme::is_light_mode(),
            rounded_mode: crate::theme::rounded_mode(),
            opacity_gradient: crate::theme::is_opacity_gradient(),
        }
    }

    /// Central dispatcher for every Settings-view message: routes nav /
    /// search / activation to the page state, and every `SettingsAction`
    /// write to its typed handler (general dispatch chain, WriteConfig,
    /// hotkeys, presets, logout).
    pub(crate) fn handle_settings(&mut self, msg: crate::views::SettingsMessage) -> Task<Message> {
        use crate::views::SettingsMessage;

        // Mini-index pill click: precision-jump the detail pane to the
        // clicked section's header. Intercepted before the nav/dispatch
        // paths since it neither rebuilds entries nor goes through
        // `SettingsPage::update`.
        if let SettingsMessage::JumpToSection(header_idx) = msg {
            return self.handle_jump_to_section(header_idx);
        }

        // Keyboard nav and scrollbar seek always auto-scroll the focused row
        // into view; click auto-scrolls only when stable_viewport is off
        // (legacy scroll-on-click). With stable_viewport on, the clicked row
        // is already visible by definition, so the view stays put.
        let is_detail_nav = matches!(
            msg,
            SettingsMessage::SlotListUp
                | SettingsMessage::SlotListDown
                | SettingsMessage::SlotListSetOffset(..)
        ) || (matches!(msg, SettingsMessage::SlotListClickItem(_))
            && !self.settings.stable_viewport);

        // Fast path: pure navigation messages don't need SettingsViewData at all
        // when entries are already cached — avoid disk I/O for arrow key nav.
        let is_nav_only = matches!(
            msg,
            SettingsMessage::SlotListUp
                | SettingsMessage::SlotListDown
                | SettingsMessage::SlotListSetOffset(..)
        );
        if is_nav_only
            && !self.settings_page.cached_entries.is_empty()
            && self.settings_page.sub_list.is_none()
            && self.settings_page.font_sub_list.is_none()
            && self.settings_page.theme_sub_list.is_none()
            && self.settings_page.toggle_cursor.is_none()
            && self.settings_page.editing_index.is_none()
        {
            let total = self.settings_page.cached_entries.len().max(1);
            match msg {
                SettingsMessage::SlotListUp => {
                    self.settings_page.editing_index = None;
                    self.settings_page.toggle_cursor = None;
                    self.settings_page.slot_list.move_up(total);
                    self.settings_page.snap_to_non_header(false);
                }
                SettingsMessage::SlotListDown => {
                    self.settings_page.editing_index = None;
                    self.settings_page.toggle_cursor = None;
                    self.settings_page.slot_list.move_down(total);
                    self.settings_page.snap_to_non_header(true);
                }
                SettingsMessage::SlotListSetOffset(offset, _) => {
                    self.settings_page.editing_index = None;
                    self.settings_page.toggle_cursor = None;
                    self.settings_page.slot_list.set_offset(offset, total);
                    self.settings_page.snap_to_non_header(true);
                }
                _ => unreachable!(),
            }
            return self.detail_pane_scroll_task();
        }

        // Full path: build SettingsViewData (reads from theme system + config.toml)
        let rebuilding =
            self.settings_page.config_dirty || self.settings_page.cached_entries.is_empty();
        let data = self.build_settings_view_data();

        // Only rebuild entries when config has changed or entries are empty
        if rebuilding {
            self.settings_page.refresh_entries(&data);
            self.settings_page.config_dirty = false;
        }
        let action = self.settings_page.update(msg, &data);
        let task = match action {
            crate::views::SettingsAction::None => Task::none(),
            crate::views::SettingsAction::ExitSettings => {
                self.handle_switch_view(crate::View::Queue)
            }
            crate::views::SettingsAction::FocusHexInput => {
                iced::widget::operation::focus(crate::views::settings::HEX_EDITOR_INPUT_ID)
            }
            crate::views::SettingsAction::FocusSearch => {
                iced::widget::operation::focus(crate::views::settings::SETTINGS_SEARCH_INPUT_ID)
            }
            // Config writes (theme/visualizer TOML values)
            crate::views::SettingsAction::WriteConfig {
                key,
                value,
                description,
            } => self.handle_settings_write_config(key, value, description),
            crate::views::SettingsAction::WriteColorEntry {
                key,
                index,
                hex_color,
            } => {
                let is_theme = key.is_theme();
                if let Err(e) = key.write_color(index, &hex_color) {
                    tracing::warn!(" [SETTINGS] Failed to write color entry: {e}");
                } else if is_theme {
                    crate::theme::reload_theme();
                    self.settings_page.config_dirty = true;
                } else {
                    // Non-theme color arrays live in config.toml sub-tables
                    // the behavior config doesn't model — refresh the cached
                    // entries so the GUI reflects the write.
                    self.settings_page.config_dirty = true;
                    self.refresh_settings_entries_if_dirty();
                }
                Task::done(Message::Playback(crate::app_message::PlaybackMessage::Tick))
            }
            crate::views::SettingsAction::ApplyPreset { stem, display_name } => {
                if let Err(e) = crate::views::settings::presets::apply_theme(&stem) {
                    tracing::warn!(" [SETTINGS] Failed to apply theme '{display_name}': {e}");
                    self.toast_warn(format!("Failed to apply theme '{display_name}': {e}"));
                } else {
                    tracing::info!(" [SETTINGS] Applied theme: {display_name}");
                    // Reload theme immediately (watcher suppresses internal writes)
                    crate::theme::reload_theme();
                    self.settings_page.config_dirty = true;
                    self.toast_success(format!("Applied theme: {display_name}"));
                }
                Task::done(Message::Playback(crate::app_message::PlaybackMessage::Tick))
            }
            crate::views::SettingsAction::WriteFontFamily(family) => {
                self.sfx_engine.play(nokkvi_data::audio::SfxType::Backspace);
                // Font is now an app-level setting, not part of the theme
                crate::theme::set_font_family(family.clone());
                let family_owned = family;
                self.shell_spawn("persist_font_family", move |shell| async move {
                    shell.settings().set_font_family(family_owned).await
                });
                self.settings_page.config_dirty = true;
                Task::done(Message::Playback(crate::app_message::PlaybackMessage::Tick))
            }

            // Hotkey actions
            crate::views::SettingsAction::WriteHotkeyBinding { action, combo } => {
                self.handle_settings_write_hotkey(action, combo)
            }
            crate::views::SettingsAction::StealHotkeyBinding {
                action,
                combo,
                conflicting_action,
                old_combo,
            } => self.handle_settings_steal_hotkey(action, combo, conflicting_action, old_combo),
            crate::views::SettingsAction::ResetHotkeyBinding(action) => {
                self.handle_settings_reset_hotkey(action)
            }

            // General settings (redb-persisted app preferences)
            crate::views::SettingsAction::WriteGeneralSetting { key, value } => {
                self.sfx_engine.play(nokkvi_data::audio::SfxType::Backspace);
                self.settings_page.config_dirty = true;
                self.handle_settings_general(key, value)
            }

            // System actions
            crate::views::SettingsAction::Logout => self.handle_settings_logout(),
            crate::views::SettingsAction::OpenTextInput {
                key,
                current_value,
                label,
            } => {
                self.text_input_dialog.open(
                    format!("Edit: {label}"),
                    current_value,
                    "e.g. /music/Library",
                    crate::widgets::text_input_dialog::TextInputDialogAction::WriteGeneralSetting {
                        key,
                    },
                );
                Task::none()
            }
            crate::views::SettingsAction::OpenResetVisualizerDialog => {
                self.text_input_dialog.open_reset_visualizer_confirmation();
                Task::none()
            }
            crate::views::SettingsAction::OpenResetHotkeysDialog => {
                self.text_input_dialog.open_reset_hotkeys_confirmation();
                Task::none()
            }
            crate::views::SettingsAction::OpenDefaultPlaylistPicker => {
                Task::done(Message::DefaultPlaylistPicker(
                    crate::widgets::default_playlist_picker::DefaultPlaylistPickerMessage::Open,
                ))
            }
            crate::views::SettingsAction::OpenListenBrainzTokenDialog => {
                let saved = self
                    .app_service
                    .as_ref()
                    .is_some_and(|s| s.listenbrainz_token().is_some());
                let placeholder = if saved {
                    "A token is saved — paste a new one to replace, or leave empty to disconnect"
                } else {
                    "Paste your token (leave empty to disconnect)"
                };
                self.text_input_dialog.open(
                    "ListenBrainz Token",
                    "",
                    placeholder,
                    crate::widgets::text_input_dialog::TextInputDialogAction::WriteListenBrainzToken,
                );
                // Mask the input — it's a secret.
                self.text_input_dialog.secure = true;
                Task::none()
            }
            crate::views::SettingsAction::VerifyListenBrainz => self.shell_task(
                |shell| async move {
                    let Some(token) = shell.listenbrainz_token() else {
                        return Err("no token set".to_string());
                    };
                    // Shared validate→name mapping; Some(name) = connected.
                    shell
                        .validate_listenbrainz_token_to_name(token)
                        .await
                        .map(Some)
                },
                |result| {
                    Message::Scrobble(crate::app_message::ScrobbleMessage::RadioVerifyResult(
                        result,
                    ))
                },
            ),
            crate::views::SettingsAction::OpenLastfmCredentialsDialog => {
                let saved = self
                    .app_service
                    .as_ref()
                    .is_some_and(|s| s.lastfm_credentials().is_some());
                let (key_ph, secret_ph) = if saved {
                    ("API key (saved — paste to replace)", "API secret (saved)")
                } else {
                    ("API key", "API secret")
                };
                self.text_input_dialog.open_two_fields(
                    "Last.fm API Credentials",
                    "",
                    key_ph,
                    "",
                    secret_ph,
                    crate::widgets::text_input_dialog::TextInputDialogAction::WriteLastfmCredentials,
                );
                self.text_input_dialog.secure = true; // mask both fields
                Task::none()
            }
            crate::views::SettingsAction::ConnectLastfm => self.shell_task(
                |shell| async move { shell.lastfm_begin_auth().await.map_err(|e| e.to_string()) },
                |result| {
                    Message::Scrobble(crate::app_message::ScrobbleMessage::LastfmAuthStarted(
                        result,
                    ))
                },
            ),
            crate::views::SettingsAction::DisconnectLastfm => {
                // Sync redb clear — done inline so the row status refreshes
                // immediately via the tail `refresh_settings_entries_if_dirty`.
                let result = self.app_service.as_ref().map(|s| s.disconnect_lastfm());
                match result {
                    Some(Ok(())) => self.toast_success("Last.fm disconnected"),
                    Some(Err(e)) => self.toast_error(format!("Last.fm: {e}")),
                    None => {}
                }
                self.settings_page.config_dirty = true;
                Task::none()
            }
        };

        self.refresh_settings_entries_if_dirty();

        // Detail-pane nav (Tab / Backspace / click / scrollbar seek): chain
        // a scroll task so the focused row stays in view. The sidebar and
        // sub-list paths skip this — only the right pane needs auto-scroll
        // (an open picker would otherwise re-scroll the dimmed background pane
        // on every cursor move).
        if is_detail_nav
            && self.settings_page.sub_list.is_none()
            && self.settings_page.font_sub_list.is_none()
            && self.settings_page.theme_sub_list.is_none()
        {
            Task::batch([task, self.detail_pane_scroll_task()])
        } else {
            task
        }
    }

    /// Center the currently-focused detail-pane row in the visible viewport.
    ///
    /// The detail pane has variable-height rows (wrapped subtitles, value
    /// badges, color swatches), so this dispatches a measured widget operation
    /// that reads the focused row's REAL laid-out bounds and the scrollable's
    /// real frame, then scrolls to center it. Any fixed per-row pixel estimate
    /// drifts cumulatively — undershooting on tall rows (Hotkeys) and
    /// overshooting on short ones (Theme) — and walks the focused row out of
    /// view. See [`crate::widgets::scroll_into_view`]. No-op when no row is
    /// focused (e.g. an empty list).
    fn detail_pane_scroll_task(&self) -> Task<Message> {
        crate::widgets::scroll_into_view::center_in_scrollable(
            iced::widget::Id::new(crate::views::settings::DETAIL_SCROLLABLE_ID),
            iced::widget::Id::new(crate::views::settings::DETAIL_FOCUSED_ROW_ID),
        )
    }

    /// Advance the keyboard focus to the first item under the clicked section
    /// header, then center it in the detail pane (which leaves the section
    /// header visible just above). Subsequent Tab/Backspace navigation
    /// continues from where the user landed.
    fn handle_jump_to_section(&mut self, header_idx: usize) -> Task<Message> {
        let entries = &self.settings_page.cached_entries;
        if header_idx >= entries.len() {
            return Task::none();
        }

        let target_focus = (header_idx + 1).min(entries.len() - 1);
        self.settings_page.slot_list.viewport_offset = target_focus;
        self.settings_page.editing_index = None;
        self.settings_page.toggle_cursor = None;

        self.detail_pane_scroll_task()
    }

    /// Rebuild the settings page's cached entries when the Settings view is
    /// showing and `config_dirty` is set (or the cache is empty).
    ///
    /// The settings VIEW renders `cached_entries` verbatim — entries are
    /// rebuilt here in the update path, never per frame. Every mutation of
    /// settings-visible state must either run inside `handle_settings`
    /// (covered by its tail call) or mark `config_dirty` and call this
    /// helper (e.g. the default-playlist picker, the chrome light-mode /
    /// crossfade toggles, hot-reload handlers). Off-Settings callers
    /// cheaply no-op — the dirty flag survives until `handle_switch_view`
    /// refreshes on entry.
    pub(crate) fn refresh_settings_entries_if_dirty(&mut self) {
        if self.current_view != crate::View::Settings {
            return;
        }
        if self.settings_page.config_dirty || self.settings_page.cached_entries.is_empty() {
            let new_data = self.build_settings_view_data();
            self.settings_page.refresh_entries(&new_data);
            self.settings_page.config_dirty = false;
        }
    }

    // =========================================================================
    // Config Writes (theme/visualizer TOML values)
    // =========================================================================

    fn handle_settings_write_config(
        &mut self,
        key: crate::config_writer::ConfigKey,
        value: crate::views::settings::items::SettingValue,
        description: Option<String>,
    ) -> Task<Message> {
        self.sfx_engine.play(nokkvi_data::audio::SfxType::Backspace);

        let is_theme = key.is_theme();
        let key_str = key.as_str().to_string();

        // `visualizer.*` keys take the dispatch-first path: mutate the
        // manager's in-memory config, and only persist on success (a
        // dispatch miss or type error must not leave disk ahead of memory
        // until restart).
        if !is_theme && key_str.starts_with("visualizer.") {
            return self.handle_visualizer_config_write(key, key_str, value, description);
        }

        // In `Clean` verbose-config mode the writer adds no `# description`
        // comments (only `[visualizer]` keys carry them — theme writes never
        // pass a comment). `On`/`Off` keep the documentation.
        let comment = if self.settings.verbose_config.writes_comments() {
            description.as_deref()
        } else {
            None
        };

        if let Err(e) = key.write(&value, comment) {
            tracing::warn!(" [SETTINGS] Failed to write config: {e}");
            self.toast_warn(format!("Failed to save setting: {e}"));
        } else if is_theme {
            crate::theme::reload_theme();
            self.settings_page.config_dirty = true;
        }
        Task::done(Message::Playback(crate::app_message::PlaybackMessage::Tick))
    }

    /// A `visualizer.*` config write, dispatch-FIRST: the table setter
    /// mutates the manager's in-memory config (validating and applying
    /// monstercat↔waves exclusivity); only on success do the config.toml
    /// writes land — the user's raw primary value plus every secondary key
    /// the setter actually changed (`visualizer_secondary_writes`
    /// before/after diff, so the data-crate setter is the single owner of
    /// the exclusivity rule and config.toml receives BOTH keys — config.toml
    /// wins on reload). Finishes through the slim
    /// `apply_visualizer_settings` path.
    fn handle_visualizer_config_write(
        &mut self,
        key: crate::config_writer::ConfigKey,
        key_str: String,
        value: crate::views::settings::items::SettingValue,
        description: Option<String>,
    ) -> Task<Message> {
        let Some(shell) = self.app_service.as_ref() else {
            return Task::none();
        };
        let mgr_arc = shell.settings().settings_manager();
        let result = {
            let mut mgr = mgr_arc.blocking_lock();
            let before = mgr.visualizer().clone();
            nokkvi_data::services::settings_tables::dispatch_visualizer_tab_setting(
                &key_str,
                value.clone(),
                &mut mgr,
            )
            .map(|res| res.map(|effect| (effect, before, mgr.visualizer().clone())))
        };
        match result {
            Some(Ok((effect, before, after))) => {
                let comment = if self.settings.verbose_config.writes_comments() {
                    description.as_deref()
                } else {
                    None
                };
                // Persist the user's RAW primary value (validate may clamp
                // in memory; the read side re-applies the same clamp, so
                // disk and memory stay convergent).
                if let Err(e) = key.write(&value, comment) {
                    tracing::warn!(" [SETTINGS] Failed to write config: {e}");
                    self.toast_warn(format!("Failed to save setting: {e}"));
                }
                for (secondary_key, secondary_value) in
                    visualizer_secondary_writes(&key_str, &before, &after)
                {
                    if let Err(e) =
                        crate::config_writer::ConfigKey::app_scalar(secondary_key.to_string())
                            .write(&secondary_value, None)
                    {
                        tracing::warn!(
                            " [SETTINGS] Failed to write exclusivity companion {secondary_key}: {e}"
                        );
                    }
                }
                let apply_task = self.apply_visualizer_settings(after);
                let side_task = self.dispatch_settings_side_effect(effect);
                Task::batch([apply_task, side_task])
            }
            Some(Err(e)) => {
                tracing::warn!(" [SETTINGS] Visualizer dispatch failed for {key_str}: {e:#}");
                self.toast_warn(format!("Failed to apply setting: {e}"));
                Task::none()
            }
            None => {
                tracing::warn!(" [SETTINGS] Unhandled visualizer key: {key_str}");
                Task::none()
            }
        }
    }

    /// Slim apply path for a visualizer-only change: mirror it onto
    /// `self.settings.visualizer`, push the shared render config
    /// (change-gated), and refresh the settings entries. Deliberately NOT
    /// `handle_player_settings_loaded` — that startup-grade handler
    /// re-applies playback/SFX volume, engine modes, and column state from
    /// the manager's snapshot, and would clobber an in-flight async persist
    /// (e.g. a just-released volume slider) with a stale value.
    pub(crate) fn apply_visualizer_settings(
        &mut self,
        visualizer: nokkvi_data::types::visualizer_config::VisualizerConfig,
    ) -> Task<Message> {
        self.push_visualizer_to_shared(&visualizer);
        self.settings.visualizer = visualizer;
        self.settings_page.config_dirty = true;
        self.refresh_settings_entries_if_dirty();
        Task::done(Message::Playback(crate::app_message::PlaybackMessage::Tick))
    }

    // =========================================================================
    // Hotkey Actions
    // =========================================================================

    /// Shared result mapper for hotkey shell_task calls: Ok → HotkeyConfigUpdated,
    /// Err → warn! + toast. `label` describes the operation for log/toast messages.
    fn hotkey_result_handler(
        label: &'static str,
    ) -> impl Fn(
        anyhow::Result<nokkvi_data::types::hotkey_config::HotkeyConfig>,
    ) -> crate::app_message::Message {
        move |result| match result {
            Ok(config) => crate::app_message::Message::HotkeyConfigUpdated(config),
            Err(e) => {
                tracing::warn!(" [SETTINGS] Failed to {label}: {e}");
                crate::app_message::Message::Toast(crate::app_message::ToastMessage::Push(
                    nokkvi_data::types::toast::Toast::new(
                        format!("Failed to {label}: {e}"),
                        nokkvi_data::types::toast::ToastLevel::Warning,
                    ),
                ))
            }
        }
    }

    fn handle_settings_write_hotkey(
        &mut self,
        action: nokkvi_data::types::hotkey_config::HotkeyAction,
        combo: nokkvi_data::types::hotkey_config::KeyCombo,
    ) -> Task<Message> {
        tracing::info!(
            " [SETTINGS] WriteHotkeyBinding: {:?} -> {:?}",
            action,
            combo
        );
        self.sfx_engine.play(nokkvi_data::audio::SfxType::Backspace);
        self.shell_task(
            move |shell| async move { shell.settings().set_hotkey_binding(action, combo).await },
            Self::hotkey_result_handler("save hotkey binding"),
        )
    }

    fn handle_settings_steal_hotkey(
        &mut self,
        action: nokkvi_data::types::hotkey_config::HotkeyAction,
        combo: nokkvi_data::types::hotkey_config::KeyCombo,
        conflicting_action: nokkvi_data::types::hotkey_config::HotkeyAction,
        old_combo: nokkvi_data::types::hotkey_config::KeyCombo,
    ) -> Task<Message> {
        tracing::info!(
            " [SETTINGS] SwapHotkeyBinding: {:?} -> {:?}, {:?} -> {:?}",
            action,
            combo,
            conflicting_action,
            old_combo
        );
        self.sfx_engine.play(nokkvi_data::audio::SfxType::Backspace);
        self.shell_task(
            move |shell| async move {
                shell
                    .settings()
                    .set_hotkey_binding(conflicting_action, old_combo)
                    .await?;
                shell.settings().set_hotkey_binding(action, combo).await
            },
            Self::hotkey_result_handler("swap hotkey bindings"),
        )
    }

    fn handle_settings_reset_hotkey(
        &mut self,
        action: nokkvi_data::types::hotkey_config::HotkeyAction,
    ) -> Task<Message> {
        self.sfx_engine.play(nokkvi_data::audio::SfxType::Backspace);
        self.shell_task(
            move |shell| async move { shell.settings().reset_hotkey(&action).await },
            Self::hotkey_result_handler("reset hotkey"),
        )
    }

    pub(crate) fn handle_settings_reset_all_hotkeys(&mut self) -> Task<Message> {
        self.sfx_engine.play(nokkvi_data::audio::SfxType::Backspace);
        let reset_task = self.shell_task(
            |shell| async move { shell.settings().reset_all_hotkeys().await },
            Self::hotkey_result_handler("reset hotkeys"),
        );
        self.toast_success("All hotkeys reset to defaults".to_string());
        reset_task
    }

    // =========================================================================
    // General Settings (redb-persisted app preferences)
    // =========================================================================

    pub(super) fn handle_settings_general(
        &mut self,
        key: String,
        value: crate::views::settings::items::SettingValue,
    ) -> Task<Message> {
        // Every general/interface/playback key is now declared via
        // `define_settings!` in `nokkvi_data::services::settings_tables`. We
        // lock the manager mutex synchronously (`blocking_lock`) and
        // dispatch + sync the UI cache on this same frame so the
        // toggle/arrow input gives immediate visual feedback — the legacy
        // match arms mutated `Nokkvi.<field>` synchronously and any async
        // hop showed one frame of stale state, which read as "the click did
        // nothing." The setters and `save()` are fast (redb write); the UI
        // thread blocks for sub-millisecond at most. Setters that need
        // iced-aware follow-up work (toasts, atomic flips, follow-up
        // `Message` dispatch, the verbose-config writer chain) declare an
        // `on_dispatch:` closure that returns a `SettingsSideEffect`; the
        // UI handler maps that to a `Task<Message>` and `Task::batch`es it
        // alongside `handle_player_settings_loaded` so both effects land in
        // the same frame.
        //
        // WATCHPOINT — sync setter contract.
        // Every `SettingsManager::set_*` reachable via the dispatch chain stays
        // synchronous so this `blocking_lock()` finishes in sub-ms. Adding
        // `.await` inside a setter would jank the iced UI thread for the
        // duration of that work. The `sync_setters_only_under_blocking_lock`
        // test in `data/src/services/settings_tables/lock_watchpoint_test.rs`
        // greps the `SettingsManager` source and fails the build if a
        // `pub async fn set_` slips in — when that test trips, audit this
        // dispatch block before relaxing it.
        let Some(shell) = self.app_service.as_ref() else {
            return Task::none();
        };

        use nokkvi_data::types::player_settings::BitPerfectMode;

        use crate::views::settings::items::SettingValue;

        let mgr_arc = shell.settings().settings_manager();
        // Crossfade and bit-perfect are mutually-exclusive modes: whichever the
        // user just turned ON wins, clearing the sibling. Compute the intent
        // BEFORE `value` is moved into dispatch; apply the sibling clear inside
        // the same lock so the `get_player_settings` snapshot carries it.
        let enabling_crossfade =
            key == "general.crossfade_enabled" && matches!(value, SettingValue::Bool(true));
        let enabling_bit_perfect = key == "general.bit_perfect"
            && matches!(&value, SettingValue::Enum { val, .. }
                if BitPerfectMode::from_label(val) != BitPerfectMode::Off);
        let result = {
            let mut mgr = mgr_arc.blocking_lock();
            nokkvi_data::services::settings_tables::dispatch_general_tab_setting(
                &key,
                value.clone(),
                &mut mgr,
            )
            .or_else(|| {
                nokkvi_data::services::settings_tables::dispatch_interface_tab_setting(
                    &key,
                    value.clone(),
                    &mut mgr,
                )
            })
            .or_else(|| {
                nokkvi_data::services::settings_tables::dispatch_playback_tab_setting(
                    &key, value, &mut mgr,
                )
            })
            // Deliberately NO dispatch_visualizer_tab_setting here:
            // visualizer.* keys route through WriteConfig →
            // handle_visualizer_config_write (config.toml is their only
            // store — this redb-persisting chain would mutate in-memory
            // state that silently reverts on the next reload).
            .map(|res| {
                res.map(|effect| {
                    if enabling_crossfade {
                        let _ = mgr.set_bit_perfect(BitPerfectMode::Off);
                    }
                    if enabling_bit_perfect {
                        let _ = mgr.set_crossfade_enabled(false);
                    }
                    (effect, mgr.get_player_settings())
                })
            })
        };
        match result {
            Some(Ok((effect, p))) => {
                let state_task = self.handle_player_settings_loaded(p);
                let side_task = self.dispatch_settings_side_effect(effect);
                Task::batch([state_task, side_task])
            }
            Some(Err(e)) => {
                tracing::warn!(" [SETTINGS] Macro dispatch failed for {key}: {e:#}");
                Task::none()
            }
            None => {
                tracing::warn!(" [SETTINGS] Unhandled general setting key: {key}");
                Task::none()
            }
        }
    }

    /// Run the iced-side follow-up work that a setting's `on_dispatch:`
    /// hook requested. Returning `Task::none()` for [`SettingsSideEffect::None`]
    /// is the common case — only the legacy strangler-fig keys
    /// (`light_mode`, `show_album_artists_only`, `artwork_resolution`,
    /// `verbose_config`) emit non-`None` variants today. Visible to
    /// `crate::update::tests::settings` for direct unit coverage of the
    /// toast / light-mode / `LoadArtists` / verbose-config routing without
    /// having to stand up a full `AppService`.
    pub(crate) fn dispatch_settings_side_effect(
        &mut self,
        effect: nokkvi_data::services::settings_tables::SettingsSideEffect,
    ) -> Task<Message> {
        use nokkvi_data::{
            services::settings_tables::SettingsSideEffect, types::toast::ToastLevel,
        };

        match effect {
            SettingsSideEffect::None => Task::none(),
            SettingsSideEffect::SetLightModeAtomic(on) => {
                crate::theme::set_light_mode(on);
                if let Err(e) =
                    crate::config_writer::ConfigKey::app_scalar("settings.light_mode".to_string())
                        .write(&crate::views::settings::items::SettingValue::Bool(on), None)
                {
                    tracing::warn!(" [SETTINGS] Failed to write light_mode to config.toml: {e}");
                }
                // `handle_player_settings_loaded` already rebuilt the cached
                // entries this frame — but against the OLD atomic (it re-reads
                // config.toml from BEFORE the write above), consuming the
                // dirty flag. Rebuild here against the just-flipped atomic so
                // the Mode row and the `dark.`/`light.` palette key prefixes
                // track the new mode — same pattern as the chrome-menu path
                // (`handle_toggle_light_mode`). The config write is
                // watcher-suppressed, so no reload fixes this up later.
                self.settings_page.config_dirty = true;
                self.refresh_settings_entries_if_dirty();
                Task::done(Message::Playback(crate::app_message::PlaybackMessage::Tick))
            }
            SettingsSideEffect::Toast { level, message } => {
                match level {
                    ToastLevel::Info => self.toast_info(message),
                    ToastLevel::Success => self.toast_success(message),
                    ToastLevel::Warning => self.toast_warn(message),
                    ToastLevel::Error => self.toast_error(message),
                }
                Task::none()
            }
            SettingsSideEffect::LoadArtists => Task::done(Message::LoadArtists),
            SettingsSideEffect::WriteVerboseConfig { mode } => {
                use nokkvi_data::types::player_settings::VerboseConfig;
                // The redb side has already been persisted by the macro's
                // setter (`set_verbose_config` → `save_redb_only`). We only
                // own the synchronous TOML write/strip + the deferred
                // `write_all_toml_public` flush.
                match mode {
                    VerboseConfig::On => {
                        use crate::visualizer_config::SharedVisualizerConfigExt;
                        let viz_config = self.visualizer_config.snapshot();
                        if let Err(e) = crate::config_writer::write_full_visualizer(&viz_config) {
                            tracing::warn!(" [SETTINGS] Failed to write full config: {e}");
                            self.toast_warn(format!("Failed to write verbose config: {e}"));
                        } else {
                            self.toast_success(
                                "Config expanded — all defaults written".to_string(),
                            );
                        }
                    }
                    // `Off` keeps the descriptive comments; `Clean` strips them.
                    VerboseConfig::Off | VerboseConfig::Clean => {
                        let clear_comments = matches!(mode, VerboseConfig::Clean);
                        if let Err(e) = crate::config_writer::strip_to_sparse(clear_comments) {
                            tracing::warn!(" [SETTINGS] Failed to strip config: {e}");
                            self.toast_warn(format!("Failed to strip config: {e}"));
                        } else if clear_comments {
                            self.toast_success(
                                "Config minimized — non-default values only, no comments"
                                    .to_string(),
                            );
                        } else {
                            self.toast_success(
                                "Config stripped — only non-default values remain".to_string(),
                            );
                        }
                    }
                }

                self.shell_spawn(
                    "write_all_toml_after_verbose_toggle",
                    move |shell| async move {
                        let mgr = shell.settings().settings_manager();
                        let sm = mgr.lock().await;
                        sm.write_all_toml_public()
                    },
                );
                Task::none()
            }
        }
    }

    // =========================================================================
    // System Actions
    // =========================================================================

    pub(crate) fn handle_settings_logout(&mut self) -> Task<Message> {
        tracing::info!(" [SETTINGS] Logout requested");
        // Shared session teardown — see `Nokkvi::reset_session_state` in
        // `update/components.rs`. Logout is silent (user took the action,
        // no toast); the helper handles the engine-stop Task + redb
        // session clear + storage caching for re-login.
        self.reset_session_state()
    }
}

/// Derive the secondary `[visualizer]` config.toml writes for a primary key
/// dispatch from the manager state BEFORE and AFTER the setter ran — the
/// data-crate setter is the single owner of the monstercat↔waves exclusivity
/// rule; whatever sibling field it changed must ALSO be persisted (invariant:
/// config.toml wins on reload, so an in-memory-only sibling change would be
/// resurrected by the next reload/restart).
///
/// Scope: only fields whose setters CROSS-MUTATE a sibling (the exclusivity
/// pair). Plain validate() clamps (e.g. `higher_cutoff_freq` tracking
/// `lower_cutoff_freq`) are deliberately NOT persisted — the read side
/// re-derives them from the raw on-disk values, so disk and memory converge
/// without a write.
pub(crate) fn visualizer_secondary_writes(
    primary_key: &str,
    before: &nokkvi_data::types::visualizer_config::VisualizerConfig,
    after: &nokkvi_data::types::visualizer_config::VisualizerConfig,
) -> Vec<(&'static str, crate::views::settings::items::SettingValue)> {
    use nokkvi_data::types::visualizer_config::keys;

    use crate::views::settings::items::SettingValue;

    let mut writes = Vec::new();
    if primary_key != keys::WAVES && after.waves != before.waves {
        writes.push((keys::WAVES, SettingValue::Bool(after.waves)));
    }
    if primary_key != keys::MONSTERCAT && after.monstercat != before.monstercat {
        writes.push((
            keys::MONSTERCAT,
            SettingValue::Float {
                val: after.monstercat,
                min: 0.0,
                max: 10.0,
                step: 0.1,
                unit: "",
            },
        ));
    }
    writes
}
