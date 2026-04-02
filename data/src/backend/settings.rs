//! Settings service — view preferences, player settings, hotkey bindings
//!
//! Wraps `SettingsManager` for persisting sort preferences, volume,
//! visualization mode, and user-configurable hotkey bindings via redb.
//! Light mode is stored in config.toml (see `config_writer`).

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::services::settings::SettingsManager;

/// Service wrapper for SettingsManager
///
/// Provides Arc<Mutex<>> access pattern consistent with other ViewModels.
#[derive(Clone)]
pub struct SettingsService {
    settings_manager: Arc<Mutex<SettingsManager>>,
}

impl std::fmt::Debug for SettingsService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsService").finish()
    }
}

impl SettingsService {
    pub fn new(storage: crate::services::state_storage::StateStorage) -> anyhow::Result<Self> {
        let settings_manager = SettingsManager::new(storage)?;
        Ok(Self {
            settings_manager: Arc::new(Mutex::new(settings_manager)),
        })
    }

    /// Get reference to SettingsManager
    pub fn settings_manager(&self) -> Arc<Mutex<SettingsManager>> {
        self.settings_manager.clone()
    }

    // =========================================================================
    // View Sort Preferences
    // =========================================================================

    pub async fn get_view_preferences(&self) -> crate::types::view_preferences::AllViewPreferences {
        let sm = self.settings_manager.lock().await;
        sm.get_view_preferences()
    }

    /// Reload settings from config.toml
    pub async fn reload_from_toml(&self) {
        let mut sm = self.settings_manager.lock().await;
        sm.reload_from_toml();
    }

    /// Save albums view sort preferences
    pub async fn set_albums_prefs(
        &self,
        sort_mode: crate::types::sort_mode::SortMode,
        sort_ascending: bool,
    ) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_albums_prefs(sort_mode, sort_ascending)
    }

    /// Save artists view sort preferences
    pub async fn set_artists_prefs(
        &self,
        sort_mode: crate::types::sort_mode::SortMode,
        sort_ascending: bool,
    ) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_artists_prefs(sort_mode, sort_ascending)
    }

    /// Save songs view sort preferences
    pub async fn set_songs_prefs(
        &self,
        sort_mode: crate::types::sort_mode::SortMode,
        sort_ascending: bool,
    ) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_songs_prefs(sort_mode, sort_ascending)
    }

    /// Save genres view sort preferences
    pub async fn set_genres_prefs(
        &self,
        sort_mode: crate::types::sort_mode::SortMode,
        sort_ascending: bool,
    ) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_genres_prefs(sort_mode, sort_ascending)
    }

    /// Save playlists view sort preferences
    pub async fn set_playlists_prefs(
        &self,
        sort_mode: crate::types::sort_mode::SortMode,
        sort_ascending: bool,
    ) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_playlists_prefs(sort_mode, sort_ascending)
    }

    /// Save queue view sort preferences
    pub async fn set_queue_prefs(
        &self,
        sort_mode: crate::types::queue_sort_mode::QueueSortMode,
        sort_ascending: bool,
    ) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_queue_prefs(sort_mode, sort_ascending)
    }

    // =========================================================================
    // Player Settings
    // =========================================================================

    /// Get all persisted player settings
    pub async fn get_player_settings(&self) -> crate::types::player_settings::PlayerSettings {
        let sm = self.settings_manager.lock().await;
        sm.get_player_settings()
    }

    /// Set SFX volume (0.0 to 1.0) and persist
    pub async fn set_sfx_volume(&self, volume: f32) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_sfx_volume(volume as f64)
    }

    /// Set sound effects enabled and persist
    pub async fn set_sound_effects_enabled(&self, enabled: bool) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_sound_effects_enabled(enabled)
    }

    /// Set visualization mode and persist
    pub async fn set_visualization_mode(
        &self,
        mode: crate::types::player_settings::VisualizationMode,
    ) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_visualization_mode(mode)
    }

    /// Set scrobbling enabled and persist
    pub async fn set_scrobbling_enabled(&self, enabled: bool) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_scrobbling_enabled(enabled)
    }

    /// Set scrobble threshold (fraction of track duration) and persist
    pub async fn set_scrobble_threshold(&self, threshold: f64) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_scrobble_threshold(threshold)
    }

    /// Set start view name and persist
    pub async fn set_start_view(&self, view: &str) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_start_view(view)
    }

    /// Set stable viewport mode and persist
    pub async fn set_stable_viewport(&self, enabled: bool) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_stable_viewport(enabled)
    }

    /// Set rounded corners mode and persist
    pub async fn set_rounded_mode(&self, enabled: bool) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_rounded_mode(enabled)
    }

    /// Set navigation layout mode and persist
    pub async fn set_nav_layout(
        &self,
        layout: crate::types::player_settings::NavLayout,
    ) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_nav_layout(layout)
    }

    /// Set navigation display mode and persist
    pub async fn set_nav_display_mode(
        &self,
        mode: crate::types::player_settings::NavDisplayMode,
    ) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_nav_display_mode(mode)
    }

    /// Set track info display mode and persist
    pub async fn set_track_info_display(
        &self,
        mode: crate::types::player_settings::TrackInfoDisplay,
    ) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_track_info_display(mode)
    }

    /// Set auto-follow playing track and persist
    pub async fn set_auto_follow_playing(&self, enabled: bool) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_auto_follow_playing(enabled)
    }

    /// Set songs enter behavior and persist
    pub async fn set_enter_behavior(
        &self,
        behavior: crate::types::player_settings::EnterBehavior,
    ) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_enter_behavior(behavior)
    }

    /// Set local music path prefix and persist
    pub async fn set_local_music_path(&self, path: String) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_local_music_path(path)
    }

    pub async fn set_eq_enabled(&self, enabled: bool) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_eq_enabled(enabled)
    }

    pub async fn set_eq_gains(&self, gains: [f32; 10]) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_eq_gains(gains)
    }

    pub async fn save_custom_eq_preset(
        &self,
        name: String,
        gains: [f32; 10],
    ) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.save_custom_eq_preset(name, gains)
    }

    pub async fn delete_custom_eq_preset(&self, index: usize) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.delete_custom_eq_preset(index)
    }
    /// Set slot row height density and persist
    pub async fn set_slot_row_height(
        &self,
        height: crate::types::player_settings::SlotRowHeight,
    ) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_slot_row_height(height)
    }

    /// Set opacity gradient enabled and persist
    pub async fn set_opacity_gradient(&self, enabled: bool) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_opacity_gradient(enabled)
    }

    /// Set crossfade enabled and persist
    pub async fn set_crossfade_enabled(&self, enabled: bool) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_crossfade_enabled(enabled)
    }

    /// Set crossfade duration in seconds and persist
    pub async fn set_crossfade_duration(&self, duration_secs: u32) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_crossfade_duration(duration_secs)
    }

    /// Set default playlist for quick-add and persist
    pub async fn set_default_playlist(
        &self,
        id: Option<String>,
        name: String,
    ) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_default_playlist(id, name)
    }

    /// Set active playlist context (for queue header bar) and persist
    pub async fn set_active_playlist(
        &self,
        id: Option<String>,
        name: String,
        comment: String,
    ) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_active_playlist(id, name, comment)
    }

    /// Set quick-add to playlist enabled and persist
    pub async fn set_quick_add_to_playlist(&self, enabled: bool) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_quick_add_to_playlist(enabled)
    }

    /// Set horizontal volume controls and persist
    pub async fn set_horizontal_volume(&self, enabled: bool) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_horizontal_volume(enabled)
    }

    /// Set font family and persist
    pub async fn set_font_family(&self, family: String) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_font_family(family)
    }

    /// Set strip show title and persist
    pub async fn set_strip_show_title(&self, enabled: bool) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_strip_show_title(enabled)
    }

    /// Set strip show artist and persist
    pub async fn set_strip_show_artist(&self, enabled: bool) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_strip_show_artist(enabled)
    }

    /// Set strip show album and persist
    pub async fn set_strip_show_album(&self, enabled: bool) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_strip_show_album(enabled)
    }

    /// Set strip show format info and persist
    pub async fn set_strip_show_format_info(&self, enabled: bool) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_strip_show_format_info(enabled)
    }

    /// Set strip click action and persist
    pub async fn set_strip_click_action(
        &self,
        action: crate::types::player_settings::StripClickAction,
    ) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_strip_click_action(action)
    }

    /// Set volume normalization enabled and persist
    pub async fn set_volume_normalization(&self, enabled: bool) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_volume_normalization(enabled)
    }

    /// Set normalization level and persist
    pub async fn set_normalization_level(
        &self,
        level: crate::types::player_settings::NormalizationLevel,
    ) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_normalization_level(level)
    }

    /// Set verbose config mode and persist
    pub async fn set_verbose_config(&self, enabled: bool) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_verbose_config(enabled)
    }

    // =========================================================================
    // Hotkey Bindings
    // =========================================================================

    /// Set a single hotkey binding, persist, and return updated config
    pub async fn set_hotkey_binding(
        &self,
        action: crate::types::hotkey_config::HotkeyAction,
        combo: crate::types::hotkey_config::KeyCombo,
    ) -> anyhow::Result<crate::types::hotkey_config::HotkeyConfig> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_hotkey_binding(action, combo)?;
        Ok(sm.get_hotkey_config_owned())
    }

    /// Reset a single hotkey to default, persist, and return updated config
    pub async fn reset_hotkey(
        &self,
        action: &crate::types::hotkey_config::HotkeyAction,
    ) -> anyhow::Result<crate::types::hotkey_config::HotkeyConfig> {
        let mut sm = self.settings_manager.lock().await;
        sm.reset_hotkey(action)?;
        Ok(sm.get_hotkey_config_owned())
    }

    /// Reset all hotkeys to defaults, persist, and return updated config
    pub async fn reset_all_hotkeys(
        &self,
    ) -> anyhow::Result<crate::types::hotkey_config::HotkeyConfig> {
        let mut sm = self.settings_manager.lock().await;
        sm.reset_all_hotkeys()?;
        Ok(sm.get_hotkey_config_owned())
    }

    /// Get the current hotkey configuration
    pub async fn get_hotkey_config(&self) -> crate::types::hotkey_config::HotkeyConfig {
        let sm = self.settings_manager.lock().await;
        sm.get_hotkey_config_owned()
    }

    /// Persist volume setting
    pub async fn set_volume(&self, volume: f32) -> anyhow::Result<()> {
        let mut sm = self.settings_manager.lock().await;
        sm.set_volume(volume as f64)
    }
}
