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
    pub(crate) fn handle_settings(&mut self, msg: crate::views::SettingsMessage) -> Task<Message> {
        use crate::views::SettingsMessage;

        // Fast path: pure navigation messages don't need SettingsViewData at all
        // when entries are already cached — avoid disk I/O for arrow key nav.
        let is_nav_only = matches!(
            msg,
            SettingsMessage::SlotListUp
                | SettingsMessage::SlotListDown
                | SettingsMessage::SlotListSetOffset(_)
        );
        if is_nav_only
            && !self.settings_page.cached_entries.is_empty()
            && self.settings_page.sub_list.is_none()
            && self.settings_page.font_sub_list.is_none()
        {
            let total = self.settings_page.cached_entries.len().max(1);
            match msg {
                SettingsMessage::SlotListUp => {
                    self.settings_page.editing_index = None;
                    self.settings_page.slot_list.move_up(total);
                    self.settings_page.snap_to_non_header(false);
                    self.settings_page.update_description();
                }
                SettingsMessage::SlotListDown => {
                    self.settings_page.editing_index = None;
                    self.settings_page.slot_list.move_down(total);
                    self.settings_page.snap_to_non_header(true);
                    self.settings_page.update_description();
                }
                SettingsMessage::SlotListSetOffset(offset) => {
                    self.settings_page.editing_index = None;
                    self.settings_page.slot_list.set_offset(offset, total);
                    self.settings_page.snap_to_non_header(true);
                    self.settings_page.update_description();
                }
                _ => unreachable!(),
            }
            return Task::none();
        }

        // Full path: build SettingsViewData (reads config.toml from disk)
        let viz_config = self.visualizer_config.read().clone();
        let theme_config = crate::theme_config::load_dual_theme_config();
        let data = crate::views::SettingsViewData {
            visualizer_config: viz_config,
            theme_config,
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
            rounded_mode: crate::theme::is_rounded_mode(),
            nav_layout: if crate::theme::is_side_nav() {
                "Side"
            } else {
                "Top"
            },
            nav_display_mode: crate::theme::nav_display_mode().as_label(),
            track_info_display: crate::theme::track_info_display().as_label(),
            slot_row_height: crate::theme::slot_row_height_variant().as_label(),
            opacity_gradient: crate::theme::is_opacity_gradient(),
            crossfade_enabled: self.engine.crossfade_enabled,
            crossfade_duration_secs: self.engine.crossfade_duration_secs,
            volume_normalization: self.engine.volume_normalization,
            normalization_level: self.engine.normalization_level.as_label(),
            default_playlist_name: self.default_playlist_name.clone(),
            quick_add_to_playlist: self.quick_add_to_playlist,
            horizontal_volume: crate::theme::is_horizontal_volume(),
            strip_show_title: crate::theme::strip_show_title(),
            strip_show_artist: crate::theme::strip_show_artist(),
            strip_show_album: crate::theme::strip_show_album(),
            strip_show_format_info: crate::theme::strip_show_format_info(),
            strip_click_action: crate::theme::strip_click_action().as_label(),
        };
        // Only rebuild entries when config has changed or entries are empty
        if self.settings_page.config_dirty || self.settings_page.cached_entries.is_empty() {
            self.settings_page.refresh_entries(&data);
            self.settings_page.config_dirty = false;
        }
        let action = self.settings_page.update(msg, &data);
        match action {
            crate::views::SettingsAction::None => Task::none(),
            crate::views::SettingsAction::ExitSettings => {
                self.handle_switch_view(crate::View::Queue)
            }
            crate::views::SettingsAction::PlayEnter => {
                self.sfx_engine.play(nokkvi_data::audio::SfxType::Enter);
                Task::none()
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
                if let Err(e) =
                    crate::config_writer::update_color_array_entry(&key, index, &hex_color)
                {
                    tracing::warn!(" [SETTINGS] Failed to write color entry: {e}");
                }
                Task::none()
            }
            crate::views::SettingsAction::ApplyPreset(preset_index) => {
                let presets = crate::views::settings::presets::all_presets();
                if let Some(preset) = presets.get(preset_index) {
                    if let Err(e) = crate::views::settings::presets::apply_preset(preset) {
                        tracing::warn!(" [SETTINGS] Failed to apply preset '{}': {e}", preset.name);
                        self.toast_warn(format!("Failed to apply preset '{}': {e}", preset.name));
                    } else {
                        tracing::info!(" [SETTINGS] Applied preset: {}", preset.name);
                        self.toast_success(format!("Applied preset: {}", preset.name));
                    }
                }
                Task::none()
            }
            crate::views::SettingsAction::RestoreColorGroup { entries } => {
                self.sfx_engine.play(nokkvi_data::audio::SfxType::Backspace);
                for (key, default_hex) in &entries {
                    let value =
                        crate::views::settings::items::SettingValue::HexColor(default_hex.clone());
                    if let Err(e) = crate::config_writer::update_config_value(key, &value, None) {
                        tracing::warn!(" [SETTINGS] Failed to restore default for {key}: {e}");
                    }
                }
                Task::none()
            }
            crate::views::SettingsAction::WriteFontFamily(family) => {
                self.sfx_engine.play(nokkvi_data::audio::SfxType::Backspace);
                let value = crate::views::settings::items::SettingValue::Text(family);
                if let Err(e) =
                    crate::config_writer::update_config_value("theme.font.family", &value, None)
                {
                    tracing::warn!(" [SETTINGS] Failed to write font family: {e}");
                }
                Task::none()
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
            crate::views::SettingsAction::RebuildArtworkCache => {
                self.handle_settings_rebuild_artwork()
            }
            crate::views::SettingsAction::RebuildArtistCache => {
                self.handle_settings_rebuild_artist()
            }
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
        }
    }

    // =========================================================================
    // Config Writes (theme/visualizer TOML values)
    // =========================================================================

    fn handle_settings_write_config(
        &mut self,
        key: String,
        value: crate::views::settings::items::SettingValue,
        description: Option<String>,
    ) -> Task<Message> {
        self.sfx_engine.play(nokkvi_data::audio::SfxType::Backspace);
        if let Err(e) =
            crate::config_writer::update_config_value(&key, &value, description.as_deref())
        {
            tracing::warn!(" [SETTINGS] Failed to write config: {e}");
            self.toast_warn(format!("Failed to save setting: {e}"));
        }
        // Mutual exclusivity: waves and monstercat can't both be active.
        // When one is enabled, auto-disable the other in config AND update the
        // cached settings entry in-place so the GUI reflects the change immediately
        // (without clearing search state).
        match key.as_str() {
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
        Task::none()
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

    fn handle_settings_general(
        &mut self,
        key: String,
        value: crate::views::settings::items::SettingValue,
    ) -> Task<Message> {
        match key.as_str() {
            "general.light_mode" => {
                let new_state = matches!(value, crate::views::settings::items::SettingValue::Enum { ref val, .. } if val == "Light");
                crate::theme::set_light_mode(new_state);
                if let Err(e) = crate::config_writer::update_config_value(
                    "theme.light_mode",
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
                        shell.settings().set_local_music_path(path).await
                    });
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
            other => {
                tracing::warn!(" [SETTINGS] Unhandled general setting key: {other}");
                Task::none()
            }
        }
    }

    // =========================================================================
    // System Actions
    // =========================================================================

    fn handle_settings_rebuild_artwork(&mut self) -> Task<Message> {
        self.sfx_engine.play(nokkvi_data::audio::SfxType::Enter);
        // Clear in-memory artwork handles so they get re-fetched
        self.artwork.album_art.clear();
        self.artwork.large_artwork.clear();
        self.artwork.genre.mini.clear();
        self.artwork.genre.collage.clear();
        self.artwork.genre.pending.clear();
        self.artwork.playlist.mini.clear();
        self.artwork.playlist.collage.clear();
        self.artwork.playlist.pending.clear();
        self.artwork.album_prefetch_triggered = false;
        // Clear genre/playlist disk caches
        if let Some(ref cache) = self.artwork.genre_disk_cache {
            let removed = cache.clear();
            tracing::info!(" [SETTINGS] Cleared {removed} files from genre artwork cache");
        }
        if let Some(ref cache) = self.artwork.playlist_disk_cache {
            let removed = cache.clear();
            tracing::info!(" [SETTINGS] Cleared {removed} files from playlist artwork cache");
        }
        // Create progress handle for album artwork rebuild
        let album_progress =
            nokkvi_data::types::progress::ProgressHandle::new("Rebuilding artwork", 0);
        self.active_progress.push(album_progress.clone());
        self.toast.push(nokkvi_data::types::toast::Toast::keyed(
            album_progress.toast_key(),
            "Rebuilding artwork…",
            nokkvi_data::types::toast::ToastLevel::Info,
        ));
        // Clear album disk cache + in-memory LRU via AlbumsService
        let album_task = self.shell_task(
            move |shell| async move {
                let albums_vm = shell.albums().clone();
                let removed = albums_vm.clear_and_reset_cache().await;
                tracing::info!(" [SETTINGS] Cleared {removed} files from album artwork cache");
                albums_vm.start_artwork_prefetch(Some(album_progress)).await;
                Ok::<(), anyhow::Error>(())
            },
            |_result| Message::NoOp,
        );
        // Also rebuild artist artwork cache (reuse dedicated handler to avoid duplication
        // and ensure artist disk cache is actually cleared — was previously missed)
        let artist_task = self.handle_settings_rebuild_artist();
        Task::batch([album_task, artist_task])
    }

    fn handle_settings_rebuild_artist(&mut self) -> Task<Message> {
        self.sfx_engine.play(nokkvi_data::audio::SfxType::Enter);
        if let Some(ref cache) = self.artwork.artist_disk_cache {
            let removed = cache.clear();
            tracing::info!(" [SETTINGS] Cleared {removed} files from artist artwork cache");
        }
        self.artwork.artist_prefetch_triggered = false;
        let artist_progress =
            nokkvi_data::types::progress::ProgressHandle::new("Rebuilding artist artwork", 0);
        self.active_progress.push(artist_progress.clone());
        self.toast.push(nokkvi_data::types::toast::Toast::keyed(
            artist_progress.toast_key(),
            "Rebuilding artist artwork…",
            nokkvi_data::types::toast::ToastLevel::Info,
        ));
        if self.library.artists.is_empty() {
            Task::done(Message::LoadArtists)
        } else {
            self.handle_start_artist_prefetch(Some(artist_progress))
        }
    }

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
