use anyhow::Result;

use crate::{
    services::state_storage::StateStorage,
    types::{
        hotkey_config::{HotkeyAction, HotkeyConfig, KeyCombo},
        player_settings::{
            EnterBehavior, NavDisplayMode, NavLayout, NormalizationLevel, SlotRowHeight,
            StripClickAction, TrackInfoDisplay,
        },
        queue::{QueueSortPreferences, SortPreferences},
        queue_sort_mode::QueueSortMode,
        settings::UserSettings,
        sort_mode::SortMode,
    },
};

/// Manages user settings persistence (volume, theme, view preferences, hotkeys)
///
/// Separated from QueueManager to follow Single Responsibility Principle.
/// Settings are stored under the "user_settings" key in StateStorage.
pub struct SettingsManager {
    settings: UserSettings,
    storage: StateStorage,
}

impl std::fmt::Debug for SettingsManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsManager")
            .field("volume", &self.settings.player.volume)
            .field("light_mode", &self.settings.player.light_mode)
            .finish()
    }
}

impl SettingsManager {
    pub fn new(storage: StateStorage) -> Result<Self> {
        // Try to load existing settings; if deserialization fails (e.g. removed
        // enum variants after an update), fall back to defaults and re-persist.
        let settings = match storage.load::<UserSettings>("user_settings") {
            Ok(Some(s)) => s,
            Ok(None) => UserSettings::default(),
            Err(e) => {
                tracing::warn!(" Settings deserialization failed, resetting to defaults: {e}");
                let defaults = UserSettings::default();
                // Overwrite the corrupt stored data so this only happens once
                let _ = storage.save("user_settings", &defaults);
                defaults
            }
        };

        Ok(Self { settings, storage })
    }

    fn save(&self) -> Result<()> {
        self.storage.save("user_settings", &self.settings)?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Player Settings
    // -------------------------------------------------------------------------

    pub fn set_volume(&mut self, volume: f64) -> Result<()> {
        self.settings.player.volume = volume;
        self.save()
    }

    pub fn set_sfx_volume(&mut self, sfx_volume: f64) -> Result<()> {
        self.settings.player.sfx_volume = sfx_volume;
        self.save()
    }

    pub fn set_sound_effects_enabled(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.sound_effects_enabled = enabled;
        self.save()
    }

    pub fn set_visualization_mode(
        &mut self,
        mode: crate::types::player_settings::VisualizationMode,
    ) -> Result<()> {
        self.settings.player.visualization_mode = mode;
        self.save()
    }

    pub fn set_scrobbling_enabled(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.scrobbling_enabled = enabled;
        self.save()
    }

    pub fn set_scrobble_threshold(&mut self, threshold: f64) -> Result<()> {
        self.settings.player.scrobble_threshold = threshold.clamp(0.25, 0.90);
        self.save()
    }

    pub fn set_start_view(&mut self, view: &str) -> Result<()> {
        self.settings.player.start_view = view.to_string();
        self.save()
    }

    pub fn set_stable_viewport(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.stable_viewport = enabled;
        self.save()
    }

    pub fn set_rounded_mode(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.rounded_mode = enabled;
        self.save()
    }

    pub fn set_nav_layout(&mut self, layout: NavLayout) -> Result<()> {
        self.settings.player.nav_layout = layout;
        self.save()
    }

    pub fn set_nav_display_mode(&mut self, mode: NavDisplayMode) -> Result<()> {
        self.settings.player.nav_display_mode = mode;
        self.save()
    }

    pub fn set_track_info_display(&mut self, mode: TrackInfoDisplay) -> Result<()> {
        self.settings.player.track_info_display = mode;
        self.save()
    }

    pub fn set_auto_follow_playing(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.auto_follow_playing = enabled;
        self.save()
    }

    pub fn set_enter_behavior(&mut self, behavior: EnterBehavior) -> Result<()> {
        self.settings.player.enter_behavior = behavior;
        self.save()
    }

    pub fn set_local_music_path(&mut self, path: String) -> Result<()> {
        self.settings.player.local_music_path = path;
        self.save()
    }

    pub fn set_slot_row_height(&mut self, height: SlotRowHeight) -> Result<()> {
        self.settings.player.slot_row_height = height;
        self.save()
    }

    pub fn set_opacity_gradient(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.opacity_gradient = enabled;
        self.save()
    }

    pub fn set_crossfade_enabled(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.crossfade_enabled = enabled;
        self.save()
    }

    pub fn set_crossfade_duration(&mut self, duration_secs: u32) -> Result<()> {
        self.settings.player.crossfade_duration_secs = duration_secs.clamp(1, 12);
        self.save()
    }

    pub fn set_default_playlist(&mut self, id: Option<String>, name: String) -> Result<()> {
        self.settings.player.default_playlist_id = id;
        self.settings.player.default_playlist_name = name;
        self.save()
    }

    pub fn set_quick_add_to_playlist(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.quick_add_to_playlist = enabled;
        self.save()
    }

    pub fn set_horizontal_volume(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.horizontal_volume = enabled;
        self.save()
    }

    pub fn set_volume_normalization(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.volume_normalization = enabled;
        self.save()
    }

    pub fn set_normalization_level(&mut self, level: NormalizationLevel) -> Result<()> {
        self.settings.player.normalization_level = level;
        self.save()
    }

    pub fn set_strip_show_title(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.strip_show_title = enabled;
        self.save()
    }

    pub fn set_strip_show_artist(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.strip_show_artist = enabled;
        self.save()
    }

    pub fn set_strip_show_album(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.strip_show_album = enabled;
        self.save()
    }

    pub fn set_strip_show_format_info(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.strip_show_format_info = enabled;
        self.save()
    }

    pub fn set_strip_click_action(&mut self, action: StripClickAction) -> Result<()> {
        self.settings.player.strip_click_action = action;
        self.save()
    }

    pub fn set_active_playlist(
        &mut self,
        id: Option<String>,
        name: String,
        comment: String,
    ) -> Result<()> {
        self.settings.player.active_playlist_id = id;
        self.settings.player.active_playlist_name = name;
        self.settings.player.active_playlist_comment = comment;
        self.save()
    }

    pub fn set_eq_enabled(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.eq_enabled = enabled;
        self.save()
    }

    pub fn set_eq_gains(&mut self, gains: [f32; 10]) -> Result<()> {
        self.settings.player.eq_gains = gains;
        self.save()
    }

    pub fn save_custom_eq_preset(&mut self, name: String, gains: [f32; 10]) -> Result<()> {
        self.settings
            .player
            .custom_eq_presets
            .push(crate::audio::eq::CustomEqPreset { name, gains });
        self.save()
    }

    pub fn delete_custom_eq_preset(&mut self, index: usize) -> Result<()> {
        if index < self.settings.player.custom_eq_presets.len() {
            self.settings.player.custom_eq_presets.remove(index);
            self.save()
        } else {
            Ok(())
        }
    }

    pub fn get_custom_eq_presets(&self) -> Vec<crate::audio::eq::CustomEqPreset> {
        self.settings.player.custom_eq_presets.clone()
    }

    // -------------------------------------------------------------------------
    // View Sort Preferences
    // -------------------------------------------------------------------------

    pub fn set_albums_prefs(&mut self, sort_mode: SortMode, sort_ascending: bool) -> Result<()> {
        self.settings.views.albums = SortPreferences::new(sort_mode, sort_ascending);
        self.save()
    }

    pub fn set_artists_prefs(&mut self, sort_mode: SortMode, sort_ascending: bool) -> Result<()> {
        self.settings.views.artists = SortPreferences::new(sort_mode, sort_ascending);
        self.save()
    }

    pub fn set_songs_prefs(&mut self, sort_mode: SortMode, sort_ascending: bool) -> Result<()> {
        self.settings.views.songs = SortPreferences::new(sort_mode, sort_ascending);
        self.save()
    }

    pub fn set_genres_prefs(&mut self, sort_mode: SortMode, sort_ascending: bool) -> Result<()> {
        self.settings.views.genres = SortPreferences::new(sort_mode, sort_ascending);
        self.save()
    }

    pub fn set_playlists_prefs(&mut self, sort_mode: SortMode, sort_ascending: bool) -> Result<()> {
        self.settings.views.playlists = SortPreferences::new(sort_mode, sort_ascending);
        self.save()
    }

    pub fn set_queue_prefs(
        &mut self,
        sort_mode: QueueSortMode,
        sort_ascending: bool,
    ) -> Result<()> {
        self.settings.views.queue = QueueSortPreferences::new(sort_mode, sort_ascending);
        self.save()
    }

    // -------------------------------------------------------------------------
    // Hotkey Settings
    // -------------------------------------------------------------------------

    /// Get a reference to the current hotkey configuration.
    pub fn get_hotkey_config(&self) -> &HotkeyConfig {
        &self.settings.hotkeys
    }

    /// Get an owned clone of the hotkey configuration (for passing to the dispatcher).
    pub fn get_hotkey_config_owned(&self) -> HotkeyConfig {
        self.settings.hotkeys.clone()
    }

    /// Set a single hotkey binding and persist.
    pub fn set_hotkey_binding(&mut self, action: HotkeyAction, combo: KeyCombo) -> Result<()> {
        self.settings.hotkeys.set_binding(action, combo);
        self.save()
    }

    /// Reset a single hotkey to its default binding and persist.
    pub fn reset_hotkey(&mut self, action: &HotkeyAction) -> Result<()> {
        self.settings.hotkeys.reset_binding(action);
        self.save()
    }

    /// Reset all hotkeys to defaults and persist.
    pub fn reset_all_hotkeys(&mut self) -> Result<()> {
        self.settings.hotkeys.reset_all();
        self.save()
    }

    // -------------------------------------------------------------------------
    // Getters
    // -------------------------------------------------------------------------

    /// Get player settings for Message::PlayerSettingsLoaded
    pub fn get_player_settings(&self) -> crate::types::player_settings::PlayerSettings {
        let p = &self.settings.player;
        crate::types::player_settings::PlayerSettings {
            volume: p.volume as f32,
            sfx_volume: p.sfx_volume as f32,
            sound_effects_enabled: p.sound_effects_enabled,
            visualization_mode: p.visualization_mode,
            scrobbling_enabled: p.scrobbling_enabled,
            scrobble_threshold: p.scrobble_threshold as f32,
            start_view: p.start_view.clone(),
            stable_viewport: p.stable_viewport,
            auto_follow_playing: p.auto_follow_playing,
            enter_behavior: p.enter_behavior,
            local_music_path: p.local_music_path.clone(),
            rounded_mode: p.rounded_mode,
            nav_layout: p.nav_layout,
            nav_display_mode: p.nav_display_mode,
            track_info_display: p.track_info_display,
            slot_row_height: p.slot_row_height,
            opacity_gradient: p.opacity_gradient,
            crossfade_enabled: p.crossfade_enabled,
            crossfade_duration_secs: p.crossfade_duration_secs,
            default_playlist_id: p.default_playlist_id.clone(),
            default_playlist_name: p.default_playlist_name.clone(),
            quick_add_to_playlist: p.quick_add_to_playlist,
            horizontal_volume: p.horizontal_volume,
            volume_normalization: p.volume_normalization,
            normalization_level: p.normalization_level,
            strip_show_title: p.strip_show_title,
            strip_show_artist: p.strip_show_artist,
            strip_show_album: p.strip_show_album,
            strip_show_format_info: p.strip_show_format_info,
            strip_click_action: p.strip_click_action,
            active_playlist_id: p.active_playlist_id.clone(),
            active_playlist_name: p.active_playlist_name.clone(),
            active_playlist_comment: p.active_playlist_comment.clone(),
            eq_enabled: p.eq_enabled,
            eq_gains: p.eq_gains,
            custom_eq_presets: p.custom_eq_presets.clone(),
        }
    }

    /// Get all view preferences
    pub fn get_view_preferences(&self) -> crate::types::view_preferences::AllViewPreferences {
        let v = &self.settings.views;
        crate::types::view_preferences::AllViewPreferences {
            albums: v.albums.clone(),
            artists: v.artists.clone(),
            songs: v.songs.clone(),
            genres: v.genres.clone(),
            playlists: v.playlists.clone(),
            queue: v.queue.clone(),
        }
    }
}
