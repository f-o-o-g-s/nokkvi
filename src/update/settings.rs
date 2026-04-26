//! Settings view handlers
//!
//! Handles SettingsAction values returned by `SettingsPage::update()`.
//! The main handler dispatches to sub-handlers by action category:
//! - Config writes: theme/visualizer TOML values
//! - Hotkey actions: binding writes, resets, steals
//! - General settings: redb-persisted app preferences
//! - System actions: artwork rebuild, logout

use iced::Task;
use nokkvi_data::backend::app_service::AppService;

use crate::{Nokkvi, app_message::Message};

impl Nokkvi {
    pub(crate) fn build_settings_view_data(&self) -> crate::views::SettingsViewData {
        let viz_config = self.visualizer_config.read().clone();
        let theme_file = crate::theme_config::load_active_theme_file();
        let active_theme_stem = nokkvi_data::services::theme_loader::read_theme_name_from_config();
        crate::views::SettingsViewData {
            visualizer_config: viz_config,
            theme_file,
            active_theme_stem,
            window_height: self.window.height,
            hotkey_config: self.hotkey_config.clone(),
            server_url: self.login_page.server_url.clone(),
            username: self.login_page.username.clone(),
            is_light_mode: crate::theme::is_light_mode(),
            scrobbling_enabled: self.scrobbling_enabled,
            scrobble_threshold: self.scrobble_threshold,
            start_view: self.start_view.clone(),
            stable_viewport: self.stable_viewport,
            auto_follow_playing: self.auto_follow_playing,
            enter_behavior: self.enter_behavior.as_label(),
            local_music_path: self.local_music_path.clone(),
            library_page_size: self.library_page_size.as_label(),
            show_album_artists_only: self.show_album_artists_only,
            suppress_library_refresh_toasts: self.suppress_library_refresh_toasts,
            rounded_mode: crate::theme::is_rounded_mode(),
            nav_layout: if crate::theme::is_side_nav() {
                "Side"
            } else if crate::theme::is_none_nav() {
                "None"
            } else {
                "Top"
            },
            nav_display_mode: crate::theme::nav_display_mode().as_label(),
            track_info_display: crate::theme::track_info_display().as_label(),
            slot_row_height: crate::theme::slot_row_height_variant().as_label(),
            opacity_gradient: crate::theme::is_opacity_gradient(),
            slot_text_links: crate::theme::is_slot_text_links(),
            crossfade_enabled: self.engine.crossfade_enabled,
            crossfade_duration_secs: self.engine.crossfade_duration_secs,
            volume_normalization: self.engine.volume_normalization,
            normalization_level: self.engine.normalization_level.as_label(),
            default_playlist_name: self.default_playlist_name.clone(),
            quick_add_to_playlist: self.quick_add_to_playlist,
            horizontal_volume: crate::theme::is_horizontal_volume(),
            font_family: crate::theme::font_family(),
            strip_show_title: crate::theme::strip_show_title(),
            strip_show_artist: crate::theme::strip_show_artist(),
            strip_show_album: crate::theme::strip_show_album(),
            strip_show_format_info: crate::theme::strip_show_format_info(),
            strip_merged_mode: crate::theme::strip_merged_mode(),
            strip_click_action: crate::theme::strip_click_action().as_label(),
            albums_artwork_overlay: crate::theme::albums_artwork_overlay(),
            artists_artwork_overlay: crate::theme::artists_artwork_overlay(),
            songs_artwork_overlay: crate::theme::songs_artwork_overlay(),
            playlists_artwork_overlay: crate::theme::playlists_artwork_overlay(),
            verbose_config: self.verbose_config,
            artwork_resolution: self.artwork_resolution.as_label(),
        }
    }

    /// Helper for boolean settings that follow the pattern:
    /// extract Bool → optionally mutate local state → persist via shell_spawn → optionally force UI tick.
    ///
    /// `apply` is called with the extracted value to perform any local/theme mutations.
    /// `persist` receives `(AppService, bool)` and returns a future that persists the value.
    /// Returns a `Tick` task when `force_refresh` is true, `Task::none()` otherwise.
    fn persist_bool_setting<F, Fut>(
        &mut self,
        value: &crate::views::settings::items::SettingValue,
        name: &'static str,
        apply: impl FnOnce(&mut Self, bool),
        persist: F,
        force_refresh: bool,
    ) -> Task<Message>
    where
        F: FnOnce(AppService, bool) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        if let crate::views::settings::items::SettingValue::Bool(enabled) = *value {
            apply(self, enabled);
            self.shell_spawn(
                name,
                move |shell| async move { persist(shell, enabled).await },
            );
        }
        if force_refresh {
            Task::done(Message::Playback(crate::app_message::PlaybackMessage::Tick))
        } else {
            Task::none()
        }
    }

    /// Reload the visualizer config from disk and apply it to the live engine.
    /// Used after any visualizer TOML write to keep the UI and audio in sync.
    pub(crate) fn reload_visualizer_config(&mut self) {
        if let Ok(new_config) = crate::visualizer_config::load_visualizer_config() {
            *self.visualizer_config.write() = new_config;
            self.settings_page.config_dirty = true;
            if let Some(ref vis) = self.visualizer {
                vis.apply_config();
            }
        }
    }

    pub(crate) fn handle_settings(&mut self, msg: crate::views::SettingsMessage) -> Task<Message> {
        use crate::views::SettingsMessage;

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
                    self.settings_page.update_description();
                }
                SettingsMessage::SlotListDown => {
                    self.settings_page.editing_index = None;
                    self.settings_page.toggle_cursor = None;
                    self.settings_page.slot_list.move_down(total);
                    self.settings_page.snap_to_non_header(true);
                    self.settings_page.update_description();
                }
                SettingsMessage::SlotListSetOffset(offset, _) => {
                    self.settings_page.editing_index = None;
                    self.settings_page.toggle_cursor = None;
                    self.settings_page.slot_list.set_offset(offset, total);
                    self.settings_page.snap_to_non_header(true);
                    self.settings_page.update_description();
                }
                _ => unreachable!(),
            }
            return Task::none();
        }

        // Full path: build SettingsViewData (reads from theme system + config.toml)
        let data = self.build_settings_view_data();

        // Only rebuild entries when config has changed or entries are empty
        if self.settings_page.config_dirty || self.settings_page.cached_entries.is_empty() {
            self.settings_page.refresh_entries(&data);
            self.settings_page.config_dirty = false;
        }
        let action = self.settings_page.update(msg, &data);
        let task = match action {
            crate::views::SettingsAction::None => Task::none(),
            crate::views::SettingsAction::ExitSettings => {
                self.handle_switch_view(crate::View::Queue)
            }
            crate::views::SettingsAction::PlayEnter => {
                self.sfx_engine.play(nokkvi_data::audio::SfxType::Enter);
                Task::none()
            }
            crate::views::SettingsAction::FocusHexInput => {
                self.sfx_engine.play(nokkvi_data::audio::SfxType::Enter);
                iced::widget::operation::focus(crate::views::settings::HEX_EDITOR_INPUT_ID)
            }
            crate::views::SettingsAction::FocusSearch => Task::none(), // Config writes (theme/visualizer TOML values)
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
                    self.reload_visualizer_config();
                }
                Task::done(Message::Playback(crate::app_message::PlaybackMessage::Tick))
            }
            crate::views::SettingsAction::ApplyPreset(preset_index) => {
                let themes = crate::views::settings::presets::all_themes();
                if let Some(info) = themes.get(preset_index) {
                    if let Err(e) = crate::views::settings::presets::apply_theme(&info.stem) {
                        tracing::warn!(
                            " [SETTINGS] Failed to apply theme '{}': {e}",
                            info.display_name
                        );
                        self.toast_warn(format!(
                            "Failed to apply theme '{}': {e}",
                            info.display_name
                        ));
                    } else {
                        tracing::info!(" [SETTINGS] Applied theme: {}", info.display_name);
                        // Reload theme immediately (watcher suppresses internal writes)
                        crate::theme::reload_theme();
                        self.settings_page.config_dirty = true;
                        self.toast_success(format!("Applied theme: {}", info.display_name));
                    }
                }
                Task::done(Message::Playback(crate::app_message::PlaybackMessage::Tick))
            }
            crate::views::SettingsAction::RestoreColorGroup { entries } => {
                self.sfx_engine.play(nokkvi_data::audio::SfxType::Backspace);
                for (key, default_hex) in &entries {
                    let value =
                        crate::views::settings::items::SettingValue::HexColor(default_hex.clone());
                    // Color keys are theme-file-relative (e.g. dark.background.hard)
                    if let Err(e) = crate::config_writer::update_theme_value(key, &value) {
                        tracing::warn!(" [SETTINGS] Failed to restore default for {key}: {e}");
                    }
                }
                crate::theme::reload_theme();
                self.settings_page.config_dirty = true;
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
        };

        // If a settings action dirtied the config (like applying a theme), refresh the entries
        // immediately so the view uses the freshest data without waiting for the next interaction
        if self.settings_page.config_dirty {
            let new_data = self.build_settings_view_data();
            self.settings_page.refresh_entries(&new_data);
            self.settings_page.config_dirty = false;
        }

        task
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

        if let Err(e) = key.write(&value, description.as_deref()) {
            tracing::warn!(" [SETTINGS] Failed to write config: {e}");
            self.toast_warn(format!("Failed to save setting: {e}"));
        } else if is_theme {
            crate::theme::reload_theme();
            self.settings_page.config_dirty = true;
        } else if key_str.starts_with("visualizer.") {
            self.reload_visualizer_config();
        }
        // Mutual exclusivity: waves and monstercat can't both be active.
        // When one is enabled, auto-disable the other in config AND update the
        // cached settings entry in-place so the GUI reflects the change immediately
        // (without clearing search state).
        match key_str.as_str() {
            "visualizer.monstercat" => {
                if matches!(value, crate::views::settings::items::SettingValue::Float { val, .. } if val >= crate::visualizer_config::MONSTERCAT_MIN_EFFECTIVE)
                {
                    let _ = crate::config_writer::update_config_value(
                        "visualizer.waves",
                        &crate::views::settings::items::SettingValue::Bool(false),
                        None,
                    );
                    // Patch the waves entry in the cached list
                    Self::patch_cached_entry(
                        &mut self.settings_page.cached_entries,
                        "visualizer.waves",
                        crate::views::settings::items::SettingValue::Bool(false),
                    );
                }
            }
            "visualizer.waves" => {
                if matches!(
                    value,
                    crate::views::settings::items::SettingValue::Bool(true)
                ) {
                    let _ = crate::config_writer::update_config_value(
                        "visualizer.monstercat",
                        &crate::views::settings::items::SettingValue::Float {
                            val: 0.0,
                            min: 0.0,
                            max: 10.0,
                            step: 0.1,
                            unit: "",
                        },
                        None,
                    );
                    // Patch the monstercat entry in the cached list
                    Self::patch_cached_entry(
                        &mut self.settings_page.cached_entries,
                        "visualizer.monstercat",
                        crate::views::settings::items::SettingValue::Float {
                            val: 0.0,
                            min: 0.0,
                            max: 10.0,
                            step: 0.1,
                            unit: "",
                        },
                    );
                }
            }
            _ => {}
        }
        Task::done(Message::Playback(crate::app_message::PlaybackMessage::Tick))
    }

    /// Patch a single cached settings entry by key, updating its value in-place.
    /// Preserves search state and scroll position.
    fn patch_cached_entry(
        entries: &mut [crate::views::settings::items::SettingsEntry],
        key: &str,
        new_value: crate::views::settings::items::SettingValue,
    ) {
        for entry in entries.iter_mut() {
            if let crate::views::settings::items::SettingsEntry::Item(item) = entry
                && item.key == key
            {
                item.value = new_value;
                return;
            }
        }
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
        match key.as_str() {
            "general.light_mode" => {
                let new_state = matches!(value, crate::views::settings::items::SettingValue::Enum { ref val, .. } if val == "Light");
                crate::theme::set_light_mode(new_state);
                if let Err(e) = crate::config_writer::update_config_value(
                    "settings.light_mode",
                    &crate::views::settings::items::SettingValue::Bool(new_state),
                    None,
                ) {
                    tracing::warn!(" [SETTINGS] Failed to write light_mode to config.toml: {e}");
                }
                Task::done(Message::Playback(crate::app_message::PlaybackMessage::Tick))
            }
            "general.scrobbling_enabled" => {
                self.persist_bool_setting(
                    &value,
                    "persist_scrobbling_enabled",
                    |s, v| s.scrobbling_enabled = v,
                    |shell: AppService, v| async move {
                        shell.settings().set_scrobbling_enabled(v).await
                    },
                    false,
                )
            }
            "general.scrobble_threshold" => {
                if let crate::views::settings::items::SettingValue::Int { val, .. } = value {
                    let fraction = val as f64 / 100.0;
                    self.scrobble_threshold = fraction as f32;
                    self.shell_spawn("persist_scrobble_threshold", move |shell| async move {
                        shell.settings().set_scrobble_threshold(fraction).await
                    });
                }
                Task::none()
            }
            "general.start_view" => {
                if let crate::views::settings::items::SettingValue::Enum { ref val, .. } = value {
                    self.start_view = val.clone();
                    let view_name = val.clone();
                    self.shell_spawn("persist_start_view", move |shell| async move {
                        shell.settings().set_start_view(&view_name).await
                    });
                }
                Task::none()
            }
            "general.stable_viewport" => self.persist_bool_setting(
                &value,
                "persist_stable_viewport",
                |s, v| s.stable_viewport = v,
                |shell: AppService, v| async move { shell.settings().set_stable_viewport(v).await },
                false,
            ),
            "general.rounded_mode" => self.persist_bool_setting(
                &value,
                "persist_rounded_mode",
                |_s, v| crate::theme::set_rounded_mode(v),
                |shell: AppService, v| async move { shell.settings().set_rounded_mode(v).await },
                true,
            ),
            "general.nav_layout" => {
                if let crate::views::settings::items::SettingValue::Enum { ref val, .. } = value {
                    let layout = nokkvi_data::types::player_settings::NavLayout::from_label(val);
                    crate::theme::set_nav_layout(layout);
                    self.shell_spawn("persist_nav_layout", move |shell| async move {
                        shell.settings().set_nav_layout(layout).await
                    });
                }
                Task::done(Message::Playback(crate::app_message::PlaybackMessage::Tick))
            }
            "general.nav_display_mode" => {
                if let crate::views::settings::items::SettingValue::Enum { ref val, .. } = value {
                    let mode =
                        nokkvi_data::types::player_settings::NavDisplayMode::from_label(val);
                    crate::theme::set_nav_display_mode(mode);
                    self.shell_spawn("persist_nav_display_mode", move |shell| async move {
                        shell.settings().set_nav_display_mode(mode).await
                    });
                }
                Task::done(Message::Playback(crate::app_message::PlaybackMessage::Tick))
            }
            "general.auto_follow_playing" => {
                self.persist_bool_setting(
                    &value,
                    "persist_auto_follow_playing",
                    |s, v| s.auto_follow_playing = v,
                    |shell: AppService, v| async move {
                        shell.settings().set_auto_follow_playing(v).await
                    },
                    false,
                )
            }
            "general.enter_behavior" => {
                if let crate::views::settings::items::SettingValue::Enum { ref val, .. } = value {
                    let behavior =
                        nokkvi_data::types::player_settings::EnterBehavior::from_label(val);
                    self.enter_behavior = behavior;
                    self.shell_spawn("persist_enter_behavior", move |shell| async move {
                        shell.settings().set_enter_behavior(behavior).await
                    });
                }
                Task::none()
            }
            "general.local_music_path" => {
                if let crate::views::settings::items::SettingValue::Text(ref path) = value {
                    let path = path.trim().to_string();
                    self.local_music_path = path.clone();
                    self.shell_spawn("persist_local_music_path", move |shell| async move {
                        shell.settings().set_local_music_path(path).await?;
                        Ok(())
                    });
                }
                Task::none()
            }
            "general.library_page_size" => {
                if let crate::views::settings::items::SettingValue::Enum { ref val, .. } = value {
                    let size = nokkvi_data::types::player_settings::LibraryPageSize::from_label(val);
                    self.library_page_size = size;
                    self.shell_spawn("persist_library_page_size", move |shell| async move {
                        shell.settings().set_library_page_size(size).await?;
                        Ok(())
                    });
                }
                Task::none()
            }
            "general.show_album_artists_only" => {
                if let crate::views::settings::items::SettingValue::Bool(enabled) = value {
                    self.show_album_artists_only = enabled;
                    self.shell_spawn("persist_show_album_artists_only", move |shell| async move {
                        shell.settings().set_show_album_artists_only(enabled).await?;
                        Ok(())
                    });
                    return Task::done(Message::LoadArtists);
                }
                Task::none()
            }
            "general.suppress_library_refresh_toasts" => self.persist_bool_setting(
                &value,
                "persist_suppress_library_refresh_toasts",
                |s, v| s.suppress_library_refresh_toasts = v,
                |shell: AppService, v| async move {
                    shell.settings().set_suppress_library_refresh_toasts(v).await
                },
                false,
            ),
            "general.artwork_resolution" => {
                if let crate::views::settings::items::SettingValue::Enum { ref val, .. } = value {
                    let res =
                        nokkvi_data::types::player_settings::ArtworkResolution::from_label(val);
                    self.artwork_resolution = res;
                    self.shell_spawn("persist_artwork_resolution", move |shell| async move {
                        shell.settings().set_artwork_resolution(res).await?;
                        Ok(())
                    });
                    self.toast_info(
                        "Artwork resolution changed — rebuild artwork cache to apply",
                    );
                }
                Task::none()
            }
            "general.track_info_display" => {
                if let crate::views::settings::items::SettingValue::Enum {
                    val: ref label, ..
                } = value
                {
                    let mode =
                        nokkvi_data::types::player_settings::TrackInfoDisplay::from_label(label);
                    crate::theme::set_track_info_display(mode);
                    self.shell_spawn("persist_track_info_display", move |shell| async move {
                        shell.settings().set_track_info_display(mode).await
                    });
                }
                Task::done(Message::Playback(crate::app_message::PlaybackMessage::Tick))
            }
            "general.slot_row_height" => {
                if let crate::views::settings::items::SettingValue::Enum { ref val, .. } = value {
                    let height =
                        nokkvi_data::types::player_settings::SlotRowHeight::from_label(val);
                    crate::theme::set_slot_row_height(height);
                    self.shell_spawn("persist_slot_row_height", move |shell| async move {
                        shell.settings().set_slot_row_height(height).await
                    });
                }
                Task::done(Message::Playback(crate::app_message::PlaybackMessage::Tick))
            }
            "general.opacity_gradient" => self.persist_bool_setting(
                &value,
                "persist_opacity_gradient",
                |_s, v| crate::theme::set_opacity_gradient(v),
                |shell: AppService, v| async move {
                    shell.settings().set_opacity_gradient(v).await
                },
                true,
            ),
            "general.crossfade_enabled" => {
                if let crate::views::settings::items::SettingValue::Bool(enabled) = value {
                    self.engine.crossfade_enabled = enabled;
                    self.shell_spawn(
                        "persist_crossfade_enabled",
                        move |shell| async move {
                            shell.settings().set_crossfade_enabled(enabled).await?;
                            // Also update the audio engine
                            let engine = shell.audio_engine();
                            let mut guard = engine.lock().await;
                            guard.set_crossfade_enabled(enabled);
                            Ok::<(), anyhow::Error>(())
                        },
                    );
                }
                Task::none()
            }
            "general.crossfade_duration" => {
                if let crate::views::settings::items::SettingValue::Int { val, .. } = value {
                    let dur = val as u32;
                    self.engine.crossfade_duration_secs = dur;
                    self.shell_spawn(
                        "persist_crossfade_duration",
                        move |shell| async move {
                            shell.settings().set_crossfade_duration(dur).await?;
                            // Also update the audio engine
                            let engine = shell.audio_engine();
                            let mut guard = engine.lock().await;
                            guard.set_crossfade_duration(dur);
                            Ok::<(), anyhow::Error>(())
                        },
                    );
                }
                Task::none()
            }
            "general.volume_normalization" => {
                if let crate::views::settings::items::SettingValue::Bool(enabled) = value {
                    self.engine.volume_normalization = enabled;
                    let target_level = self.engine.normalization_level.target_level();
                    self.shell_spawn(
                        "persist_volume_normalization",
                        move |shell| async move {
                            shell
                                .settings()
                                .set_volume_normalization(enabled)
                                .await?;
                            let engine = shell.audio_engine();
                            let mut guard = engine.lock().await;
                            guard.set_volume_normalization(enabled, target_level);
                            Ok::<(), anyhow::Error>(())
                        },
                    );
                }
                Task::none()
            }
            "general.normalization_level" => {
                if let crate::views::settings::items::SettingValue::Enum { val, .. } = value {
                    let level =
                        nokkvi_data::types::player_settings::NormalizationLevel::from_label(&val);
                    self.engine.normalization_level = level;
                    let enabled = self.engine.volume_normalization;
                    let target_level = level.target_level();
                    self.shell_spawn(
                        "persist_normalization_level",
                        move |shell| async move {
                            shell.settings().set_normalization_level(level).await?;
                            let engine = shell.audio_engine();
                            let mut guard = engine.lock().await;
                            guard.set_volume_normalization(enabled, target_level);
                            Ok::<(), anyhow::Error>(())
                        },
                    );
                }
                Task::none()
            }
            "general.quick_add_to_playlist" => self.persist_bool_setting(
                &value,
                "persist_quick_add_to_playlist",
                |s, v| s.quick_add_to_playlist = v,
                |shell: AppService, v| async move {
                    shell.settings().set_quick_add_to_playlist(v).await
                },
                false,
            ),
            "general.horizontal_volume" => self.persist_bool_setting(
                &value,
                "persist_horizontal_volume",
                |_s, v| crate::theme::set_horizontal_volume(v),
                |shell: AppService, v| async move {
                    shell.settings().set_horizontal_volume(v).await
                },
                true,
            ),
            "general.slot_text_links" => self.persist_bool_setting(
                &value,
                "persist_slot_text_links",
                |_s, v| crate::theme::set_slot_text_links(v),
                |shell: AppService, v| async move {
                    shell.settings().set_slot_text_links(v).await
                },
                true,
            ),
            "general.strip_show_title" => self.persist_bool_setting(
                &value,
                "persist_strip_show_title",
                |_s, v| crate::theme::set_strip_show_title(v),
                |shell: AppService, v| async move {
                    shell.settings().set_strip_show_title(v).await
                },
                true,
            ),
            "general.strip_show_artist" => self.persist_bool_setting(
                &value,
                "persist_strip_show_artist",
                |_s, v| crate::theme::set_strip_show_artist(v),
                |shell: AppService, v| async move {
                    shell.settings().set_strip_show_artist(v).await
                },
                true,
            ),
            "general.strip_show_album" => self.persist_bool_setting(
                &value,
                "persist_strip_show_album",
                |_s, v| crate::theme::set_strip_show_album(v),
                |shell: AppService, v| async move {
                    shell.settings().set_strip_show_album(v).await
                },
                true,
            ),
            "general.strip_show_format_info" => self.persist_bool_setting(
                &value,
                "persist_strip_show_format_info",
                |_s, v| crate::theme::set_strip_show_format_info(v),
                |shell: AppService, v| async move {
                    shell.settings().set_strip_show_format_info(v).await
                },
                true,
            ),
            "general.strip_merged_mode" => self.persist_bool_setting(
                &value,
                "persist_strip_merged_mode",
                |_s, v| crate::theme::set_strip_merged_mode(v),
                |shell: AppService, v| async move {
                    shell.settings().set_strip_merged_mode(v).await
                },
                true,
            ),
            "general.albums_artwork_overlay" => self.persist_bool_setting(
                &value,
                "persist_albums_artwork_overlay",
                |_s, v| crate::theme::set_albums_artwork_overlay(v),
                |shell: AppService, v| async move {
                    shell.settings().set_albums_artwork_overlay(v).await
                },
                true,
            ),
            "general.artists_artwork_overlay" => self.persist_bool_setting(
                &value,
                "persist_artists_artwork_overlay",
                |_s, v| crate::theme::set_artists_artwork_overlay(v),
                |shell: AppService, v| async move {
                    shell.settings().set_artists_artwork_overlay(v).await
                },
                true,
            ),
            "general.songs_artwork_overlay" => self.persist_bool_setting(
                &value,
                "persist_songs_artwork_overlay",
                |_s, v| crate::theme::set_songs_artwork_overlay(v),
                |shell: AppService, v| async move {
                    shell.settings().set_songs_artwork_overlay(v).await
                },
                true,
            ),
            "general.playlists_artwork_overlay" => self.persist_bool_setting(
                &value,
                "persist_playlists_artwork_overlay",
                |_s, v| crate::theme::set_playlists_artwork_overlay(v),
                |shell: AppService, v| async move {
                    shell.settings().set_playlists_artwork_overlay(v).await
                },
                true,
            ),
            "general.strip_click_action" => {
                if let crate::views::settings::items::SettingValue::Enum { val, .. } = value {
                    let action =
                        nokkvi_data::types::player_settings::StripClickAction::from_label(&val);
                    crate::theme::set_strip_click_action(action);
                    self.shell_spawn(
                        "persist_strip_click_action",
                        move |shell| async move {
                            shell.settings().set_strip_click_action(action).await
                        },
                    );
                }
                Task::none()
            }
            "general.verbose_config" => {
                if let crate::views::settings::items::SettingValue::Bool(enabled) = value {
                    self.verbose_config = enabled;

                    // Write [visualizer] synchronously (doesn't need settings_manager)
                    if enabled {
                        let viz_config = self.visualizer_config.read().clone();
                        if let Err(e) =
                            crate::config_writer::write_full_visualizer(&viz_config) {
                            tracing::warn!(" [SETTINGS] Failed to write full config: {e}");
                            self.toast_warn(format!("Failed to write verbose config: {e}"));
                        } else {
                            self.toast_success(
                                "Config expanded — all defaults written".to_string(),
                            );
                        }
                    } else {
                        // Strip default values from theme + visualizer sections
                        if let Err(e) = crate::config_writer::strip_to_sparse() {
                            tracing::warn!(" [SETTINGS] Failed to strip config: {e}");
                            self.toast_warn(format!("Failed to strip config: {e}"));
                        } else {
                            self.toast_success(
                                "Config stripped — only non-default values remain".to_string(),
                            );
                        }
                    }

                    // Single async task: persist to redb THEN write all TOML sections.
                    // Must be one task so the verbose flag is set before write_all_toml
                    // reads it via is_verbose_config().
                    self.shell_spawn(
                        "persist_and_write_verbose_config",
                        move |shell| async move {
                            shell.settings().set_verbose_config(enabled).await?;
                            let mgr = shell.settings().settings_manager();
                            let sm = mgr.lock().await;
                            sm.write_all_toml_public()
                        },
                    );
                }
                Task::none()
            }
            other => {
                tracing::warn!(" [SETTINGS] Unhandled general setting key: {other}");
                Task::none()
            }
        }
    }

    // =========================================================================
    // System Actions
    // =========================================================================

    fn handle_settings_logout(&mut self) -> Task<Message> {
        tracing::info!(" [SETTINGS] Logout requested");
        let stop_task = if let Some(ref shell) = self.app_service {
            shell.task_manager().shutdown();
            if let Err(e) = nokkvi_data::credentials::clear_session(shell.storage()) {
                tracing::warn!(" [SETTINGS] Failed to clear session: {e}");
            }
            self.cached_storage = Some(shell.storage().clone());

            // Clone the engine Arc before dropping AppService so we can stop
            // it asynchronously. This kills PipeWire streams, the decode loop,
            // and the render thread — preventing orphaned audio after logout.
            let engine = shell.audio_engine();
            Task::perform(
                async move {
                    let mut guard = engine.lock().await;
                    guard.stop().await;
                    tracing::info!(" [SETTINGS] Audio engine stopped on logout");
                },
                |_| Message::NoOp,
            )
        } else {
            Task::none()
        };
        self.app_service = None;
        self.stored_session = None;
        self.should_auto_login = false;
        self.screen = crate::Screen::Login;
        stop_task
    }
}

#[cfg(test)]
mod tests {
    use crate::views::settings::items::{SettingItem, SettingValue, SettingsEntry};

    #[test]
    fn test_patch_cached_entry_mutual_exclusivity_updates_in_place() {
        let mut entries = vec![
            SettingsEntry::Item(SettingItem {
                key: "visualizer.monstercat".into(),
                label: "Monstercat".to_string(),
                category: "Test",
                value: SettingValue::Float {
                    val: 1.0,
                    min: 0.0,
                    max: 2.0,
                    step: 0.1,
                    unit: "",
                },
                default: SettingValue::Float {
                    val: 1.0,
                    min: 0.0,
                    max: 2.0,
                    step: 0.1,
                    unit: "",
                },
                label_icon: None,
                subtitle: None,
            }),
            SettingsEntry::Item(SettingItem {
                key: "visualizer.waves".into(),
                label: "Waves".to_string(),
                category: "Test",
                value: SettingValue::Bool(false),
                default: SettingValue::Bool(false),
                label_icon: None,
                subtitle: None,
            }),
        ];

        // Simulate logic where waves is activated, so we must clamp monstercat locally
        crate::Nokkvi::patch_cached_entry(
            &mut entries,
            "visualizer.monstercat",
            SettingValue::Float {
                val: 0.0,
                min: 0.0,
                max: 10.0,
                step: 0.1,
                unit: "",
            },
        );

        if let SettingsEntry::Item(item) = &entries[0] {
            assert_eq!(item.key, "visualizer.monstercat");
            if let SettingValue::Float { val, .. } = item.value {
                assert_eq!(
                    val, 0.0,
                    "Patching must successfully update the float value in place"
                );
            } else {
                panic!("Wrong value type after patching");
            }
        } else {
            panic!("Wrong entry type after patching");
        }
    }
}
