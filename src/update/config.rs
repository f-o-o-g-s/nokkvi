//! Config hot-reload + theme/visualizer/settings reaction handlers.
//!
//! These are top-level `Message` variants that fire when configuration
//! changes — either user-driven (light-mode toggle) or file-watcher-driven
//! (theme/visualizer/settings TOML reload). They share the trait that a
//! single config event triggers a chain of UI-state updates (refresh
//! cached entries, rebuild views, push a Tick to repaint, etc.). Bundling
//! them keeps the central dispatcher in mod.rs purely routing.

use iced::Task;
use tracing::debug;

use crate::{
    Nokkvi,
    app_message::{Message, PlaybackMessage},
};

impl Nokkvi {
    /// Handle the user-driven light-mode toggle. Writes to config.toml
    /// (single source of truth); the file watcher then fires
    /// `ThemeConfigReloaded` to actually apply the value.
    pub(super) fn handle_toggle_light_mode(&mut self) -> Task<Message> {
        let new_state = !crate::theme::is_light_mode();
        crate::theme::set_light_mode(new_state);
        debug!(" Light mode set to: {}", new_state);
        // Persist to config.toml — the config file watcher will pick this up
        // and ThemeConfigReloaded will read the correct value.
        //
        // This surgical single-key write bypasses the verbose-aware settings
        // writer, so with verbose_config off a default `light_mode = false` can
        // briefly linger in the otherwise-sparse [settings] section until the
        // next full settings save prunes it. Correctness-neutral — an absent
        // key and an explicit `false` deserialize identically.
        if let Err(e) =
            crate::config_writer::ConfigKey::app_scalar("settings.light_mode".to_string()).write(
                &crate::views::settings::items::SettingValue::Bool(new_state),
                None,
            )
        {
            tracing::warn!(" Failed to write light_mode to config.toml: {e}");
        }
        // The Theme tab's Mode row mirrors the light-mode atomic — this
        // toggle is reachable from the chrome menus while Settings is open,
        // so refresh the cached entries (no-op off-Settings; the dirty flag
        // then covers the next entry).
        self.settings_page.config_dirty = true;
        self.refresh_settings_entries_if_dirty();
        // Force UI refresh
        Task::done(Message::Playback(PlaybackMessage::Tick))
    }

    /// React to a theme TOML hot-reload: pull new colors + light-mode flag,
    /// rebuild settings entries if visible, and push a Tick to repaint.
    pub(super) fn handle_theme_config_reloaded(&mut self) -> Task<Message> {
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
        self.refresh_settings_entries_if_dirty();
        Task::done(Message::Playback(crate::app_message::PlaybackMessage::Tick))
    }

    /// React to a settings TOML hot-reload by pulling fresh view-prefs,
    /// hotkey config, and player settings off the shell, then folding them
    /// back into the live state via `SettingsReloadDataLoaded`.
    pub(super) fn handle_settings_config_reloaded(&mut self) -> Task<Message> {
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

    /// Apply settings reload data (view-prefs, hotkey config, player
    /// settings) to the live state. Triggers a chain of follow-up
    /// messages so each subsystem re-applies its slice.
    pub(super) fn handle_settings_reload_data_loaded(
        &mut self,
        vp: nokkvi_data::types::view_preferences::AllViewPreferences,
        hotkeys: nokkvi_data::types::hotkey_config::HotkeyConfig,
        settings: Box<nokkvi_data::types::player_settings::LivePlayerSettings>,
    ) -> Task<Message> {
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

    /// Apply a hot-reloaded hotkey config to live state.
    pub(super) fn handle_hotkey_config_updated(
        &mut self,
        config: nokkvi_data::types::hotkey_config::HotkeyConfig,
    ) -> Task<Message> {
        tracing::info!(" [SETTINGS] Hotkey config hot-reloaded");
        self.hotkey_config = config;
        // Mark settings entries stale so the Hotkeys tab reflects the new
        // binding instead of the previously cached combo string. Without
        // this, a successful rebind keeps showing the old key on screen
        // even though `config.toml` and `self.hotkey_config` both have the
        // new value, because the view renders `cached_entries` verbatim.
        self.settings_page.config_dirty = true;
        self.refresh_settings_entries_if_dirty();
        Task::none()
    }
}
