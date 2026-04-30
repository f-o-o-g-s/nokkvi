use anyhow::Result;

use crate::{
    services::{
        state_storage::StateStorage,
        toml_settings_io::{
            read_toml_hotkeys, read_toml_settings, read_toml_views, write_all_toml_sections,
            write_toml_hotkeys, write_toml_settings, write_toml_views,
        },
    },
    types::{
        hotkey_config::{HotkeyAction, HotkeyConfig, KeyCombo},
        player_settings::{
            ArtworkColumnMode, ArtworkResolution, ArtworkStretchFit, EnterBehavior, NavDisplayMode,
            NavLayout, NormalizationLevel, SlotRowHeight, StripClickAction, TrackInfoDisplay,
            VolumeNormalizationMode,
        },
        queue::{QueueSortPreferences, SortPreferences},
        queue_sort_mode::QueueSortMode,
        settings::UserSettings,
        sort_mode::SortMode,
        toml_settings::TomlSettings,
        toml_views::TomlViewPreferences,
    },
};

/// Manages user settings persistence with hybrid storage:
///
/// - **config.toml** (`[settings]`, `[hotkeys]`, `[views]`): Source of truth for
///   user-facing preferences. Human-readable, hot-reloadable, version-controllable.
/// - **redb** (`user_settings` key): Backward compat + high-frequency data (volume,
///   active playlist, credentials).
///
/// On startup, config.toml is read first. If absent, redb values are auto-exported
/// (one-time migration). All writes go to both stores (dual-write).
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
        // Phase 1: Try to read from config.toml (new source of truth)
        let toml_settings = read_toml_settings().unwrap_or_else(|e| {
            tracing::warn!("Error reading [settings] from config.toml: {e}");
            None
        });
        let toml_hotkeys = read_toml_hotkeys().unwrap_or_else(|e| {
            tracing::warn!("Error reading [hotkeys] from config.toml: {e}");
            None
        });
        let toml_views = read_toml_views().unwrap_or_else(|e| {
            tracing::warn!("Error reading [views] from config.toml: {e}");
            None
        });

        let has_toml = toml_settings.is_some();
        tracing::debug!(
            "⚙️ [SETTINGS] TOML sections: [settings]={}, [hotkeys]={}, [views]={}",
            toml_settings.is_some(),
            toml_hotkeys.is_some(),
            toml_views.is_some()
        );

        // Phase 2: Load from redb (always needed for volume, playlist IDs, etc.)
        let redb_settings = match storage.load::<UserSettings>("user_settings") {
            Ok(Some(s)) => s,
            Ok(None) => UserSettings::default(),
            Err(e) => {
                tracing::warn!("Settings deserialization failed, resetting to defaults: {e}");
                let defaults = UserSettings::default();
                let _ = storage.save("user_settings", &defaults);
                defaults
            }
        };

        // Phase 3: Merge — TOML overrides redb for user-facing settings,
        // redb retains volume, playlist IDs, and other runtime state.
        let mut settings = redb_settings;

        if let Some(ts) = toml_settings {
            apply_toml_settings_to_internal(&ts, &mut settings.player);
        }
        if let Some(hk) = toml_hotkeys {
            settings.hotkeys = hk;
        }
        if let Some(tv) = toml_views {
            settings.views = tv.to_all_view_prefs().into();
        }

        let manager = Self { settings, storage };

        // Phase 4: Migration — if config.toml had no [settings], export redb values
        if !has_toml {
            tracing::info!("No [settings] section in config.toml — migrating from redb");
            if let Err(e) = manager.write_all_toml() {
                tracing::error!("Failed to migrate settings to config.toml: {e}");
            }
        }

        Ok(manager)
    }

    /// Save to redb (always) + config.toml sections (for user-facing settings).
    fn save(&self) -> Result<()> {
        // 1. Always write to redb (volume, playlist IDs, backward compat)
        self.storage.save("user_settings", &self.settings)?;
        // 2. Write user-facing settings to config.toml
        self.write_settings_toml()?;
        Ok(())
    }

    /// Save only to redb — used for high-frequency operations (volume) and
    /// runtime state (active playlist) that don't belong in config.toml.
    fn save_redb_only(&self) -> Result<()> {
        self.storage.save("user_settings", &self.settings)?;
        Ok(())
    }

    /// Write [settings] section to config.toml from current internal state.
    fn write_settings_toml(&self) -> Result<()> {
        let ts = TomlSettings::from_player_settings(&self.get_player_settings());
        write_toml_settings(&ts)
    }

    /// Write [hotkeys] section to config.toml from current internal state.
    fn write_hotkeys_toml(&self) -> Result<()> {
        write_toml_hotkeys(&self.settings.hotkeys, self.is_verbose_config())
    }

    /// Write [views] section to config.toml from current internal state.
    fn write_views_toml(&self) -> Result<()> {
        let tv = TomlViewPreferences::from_all_view_prefs(&self.get_view_preferences());
        write_toml_views(&tv)
    }

    /// Write all three TOML sections at once (used during migration).
    fn write_all_toml(&self) -> Result<()> {
        let ts = TomlSettings::from_player_settings(&self.get_player_settings());
        let tv = TomlViewPreferences::from_all_view_prefs(&self.get_view_preferences());
        write_all_toml_sections(&ts, &self.settings.hotkeys, &tv, self.is_verbose_config())
    }

    /// Public entry point for writing all TOML sections (used by verbose_config toggle).
    pub fn write_all_toml_public(&self) -> Result<()> {
        self.write_all_toml()
    }

    /// Hot-reload settings from config.toml and update the in-memory state.
    /// Does NOT save to redb, to prevent feedback loops where a TOML read
    /// triggers a database write. The new values will be propagated to redb
    /// automatically whenever the user next modifies a setting.
    pub fn reload_from_toml(&mut self) {
        if let Some(ts) = read_toml_settings().unwrap_or(None) {
            apply_toml_settings_to_internal(&ts, &mut self.settings.player);
        }
        if let Some(hk) = read_toml_hotkeys().unwrap_or(None) {
            self.settings.hotkeys = hk;
        }
        if let Some(tv) = read_toml_views().unwrap_or(None) {
            self.settings.views = tv.to_all_view_prefs().into();
        }
        tracing::debug!(" [SETTINGS] Manager state hot-reloaded from config.toml");
    }

    // -------------------------------------------------------------------------
    // Player Settings
    // -------------------------------------------------------------------------

    pub fn set_volume(&mut self, volume: f64) -> Result<()> {
        self.settings.player.volume = volume;
        self.save_redb_only()
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

    pub fn set_show_album_artists_only(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.show_album_artists_only = enabled;
        self.save()
    }

    pub fn set_suppress_library_refresh_toasts(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.suppress_library_refresh_toasts = enabled;
        self.save()
    }

    pub fn set_show_tray_icon(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.show_tray_icon = enabled;
        self.save()
    }

    pub fn set_close_to_tray(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.close_to_tray = enabled;
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

    pub fn set_slot_text_links(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.slot_text_links = enabled;
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

    pub fn set_queue_show_default_playlist(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.queue_show_default_playlist = enabled;
        self.save()
    }

    pub fn set_horizontal_volume(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.horizontal_volume = enabled;
        self.save()
    }

    pub fn set_font_family(&mut self, family: String) -> Result<()> {
        self.settings.player.font_family = family;
        self.save()
    }

    pub fn set_volume_normalization(&mut self, mode: VolumeNormalizationMode) -> Result<()> {
        self.settings.player.volume_normalization = mode;
        self.save()
    }

    pub fn set_normalization_level(&mut self, level: NormalizationLevel) -> Result<()> {
        self.settings.player.normalization_level = level;
        self.save()
    }

    pub fn set_replay_gain_preamp_db(&mut self, db: f32) -> Result<()> {
        self.settings.player.replay_gain_preamp_db = db;
        self.save()
    }

    pub fn set_replay_gain_fallback_db(&mut self, db: f32) -> Result<()> {
        self.settings.player.replay_gain_fallback_db = db;
        self.save()
    }

    pub fn set_replay_gain_fallback_to_agc(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.replay_gain_fallback_to_agc = enabled;
        self.save()
    }

    pub fn set_replay_gain_prevent_clipping(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.replay_gain_prevent_clipping = enabled;
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

    pub fn set_strip_merged_mode(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.strip_merged_mode = enabled;
        self.save()
    }

    pub fn set_albums_artwork_overlay(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.albums_artwork_overlay = enabled;
        self.save()
    }

    pub fn set_artists_artwork_overlay(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.artists_artwork_overlay = enabled;
        self.save()
    }

    pub fn set_songs_artwork_overlay(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.songs_artwork_overlay = enabled;
        self.save()
    }

    pub fn set_playlists_artwork_overlay(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.playlists_artwork_overlay = enabled;
        self.save()
    }

    pub fn set_artwork_column_mode(&mut self, mode: ArtworkColumnMode) -> Result<()> {
        self.settings.player.artwork_column_mode = mode;
        self.save()
    }

    pub fn set_artwork_column_stretch_fit(&mut self, fit: ArtworkStretchFit) -> Result<()> {
        self.settings.player.artwork_column_stretch_fit = fit;
        self.save()
    }

    pub fn set_artwork_column_width_pct(&mut self, pct: f32) -> Result<()> {
        self.settings.player.artwork_column_width_pct = pct.clamp(0.05, 0.80);
        self.save()
    }

    pub fn set_strip_click_action(&mut self, action: StripClickAction) -> Result<()> {
        self.settings.player.strip_click_action = action;
        self.save()
    }

    pub fn set_queue_show_stars(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.queue_show_stars = enabled;
        self.save()
    }

    pub fn set_queue_show_album(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.queue_show_album = enabled;
        self.save()
    }

    pub fn set_queue_show_duration(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.queue_show_duration = enabled;
        self.save()
    }

    pub fn set_queue_show_love(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.queue_show_love = enabled;
        self.save()
    }

    pub fn set_queue_show_plays(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.queue_show_plays = enabled;
        self.save()
    }

    pub fn set_albums_show_stars(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.albums_show_stars = enabled;
        self.save()
    }

    pub fn set_albums_show_songcount(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.albums_show_songcount = enabled;
        self.save()
    }

    pub fn set_albums_show_plays(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.albums_show_plays = enabled;
        self.save()
    }

    pub fn set_albums_show_love(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.albums_show_love = enabled;
        self.save()
    }

    pub fn set_songs_show_stars(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.songs_show_stars = enabled;
        self.save()
    }

    pub fn set_songs_show_album(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.songs_show_album = enabled;
        self.save()
    }

    pub fn set_songs_show_duration(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.songs_show_duration = enabled;
        self.save()
    }

    pub fn set_songs_show_plays(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.songs_show_plays = enabled;
        self.save()
    }

    pub fn set_songs_show_love(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.songs_show_love = enabled;
        self.save()
    }

    pub fn set_artists_show_stars(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.artists_show_stars = enabled;
        self.save()
    }

    pub fn set_artists_show_albumcount(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.artists_show_albumcount = enabled;
        self.save()
    }

    pub fn set_artists_show_songcount(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.artists_show_songcount = enabled;
        self.save()
    }

    pub fn set_artists_show_plays(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.artists_show_plays = enabled;
        self.save()
    }

    pub fn set_artists_show_love(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.artists_show_love = enabled;
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
        self.save_redb_only()
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

    pub fn set_verbose_config(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.verbose_config = enabled;
        // Only persist to redb — the UI handler writes all TOML sections
        // in a single atomic pass to avoid racing with write_full_theme_and_visualizer.
        self.save_redb_only()
    }

    pub fn set_library_page_size(
        &mut self,
        size: crate::types::player_settings::LibraryPageSize,
    ) -> Result<()> {
        self.settings.player.library_page_size = size;
        self.save()
    }

    pub fn set_artwork_resolution(&mut self, resolution: ArtworkResolution) -> Result<()> {
        self.settings.player.artwork_resolution = resolution;
        self.save()
    }

    pub fn is_verbose_config(&self) -> bool {
        self.settings.player.verbose_config
    }

    // -------------------------------------------------------------------------
    // View Sort Preferences
    // -------------------------------------------------------------------------

    pub fn set_albums_prefs(&mut self, sort_mode: SortMode, sort_ascending: bool) -> Result<()> {
        self.settings.views.albums = SortPreferences::new(sort_mode, sort_ascending);
        self.save_with_views()
    }

    pub fn set_artists_prefs(&mut self, sort_mode: SortMode, sort_ascending: bool) -> Result<()> {
        self.settings.views.artists = SortPreferences::new(sort_mode, sort_ascending);
        self.save_with_views()
    }

    pub fn set_songs_prefs(&mut self, sort_mode: SortMode, sort_ascending: bool) -> Result<()> {
        self.settings.views.songs = SortPreferences::new(sort_mode, sort_ascending);
        self.save_with_views()
    }

    pub fn set_genres_prefs(&mut self, sort_mode: SortMode, sort_ascending: bool) -> Result<()> {
        self.settings.views.genres = SortPreferences::new(sort_mode, sort_ascending);
        self.save_with_views()
    }

    pub fn set_playlists_prefs(&mut self, sort_mode: SortMode, sort_ascending: bool) -> Result<()> {
        self.settings.views.playlists = SortPreferences::new(sort_mode, sort_ascending);
        self.save_with_views()
    }

    pub fn set_queue_prefs(
        &mut self,
        sort_mode: QueueSortMode,
        sort_ascending: bool,
    ) -> Result<()> {
        self.settings.views.queue = QueueSortPreferences::new(sort_mode, sort_ascending);
        self.save_with_views()
    }

    /// Save redb + [views] section in config.toml.
    fn save_with_views(&self) -> Result<()> {
        self.storage.save("user_settings", &self.settings)?;
        self.write_views_toml()?;
        Ok(())
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
        self.save_with_hotkeys()
    }

    /// Reset a single hotkey to its default binding and persist.
    pub fn reset_hotkey(&mut self, action: &HotkeyAction) -> Result<()> {
        self.settings.hotkeys.reset_binding(action);
        self.save_with_hotkeys()
    }

    /// Reset all hotkeys to defaults and persist.
    pub fn reset_all_hotkeys(&mut self) -> Result<()> {
        self.settings.hotkeys.reset_all();
        self.save_with_hotkeys()
    }

    /// Save redb + [hotkeys] section in config.toml.
    fn save_with_hotkeys(&self) -> Result<()> {
        self.storage.save("user_settings", &self.settings)?;
        self.write_hotkeys_toml()?;
        Ok(())
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
            library_page_size: p.library_page_size,
            rounded_mode: p.rounded_mode,
            nav_layout: p.nav_layout,
            nav_display_mode: p.nav_display_mode,
            track_info_display: p.track_info_display,
            slot_row_height: p.slot_row_height,
            opacity_gradient: p.opacity_gradient,
            slot_text_links: p.slot_text_links,
            crossfade_enabled: p.crossfade_enabled,
            crossfade_duration_secs: p.crossfade_duration_secs,
            default_playlist_id: p.default_playlist_id.clone(),
            default_playlist_name: p.default_playlist_name.clone(),
            quick_add_to_playlist: p.quick_add_to_playlist,
            queue_show_default_playlist: p.queue_show_default_playlist,
            horizontal_volume: p.horizontal_volume,
            font_family: p.font_family.clone(),
            volume_normalization: p.volume_normalization,
            normalization_level: p.normalization_level,
            replay_gain_preamp_db: p.replay_gain_preamp_db,
            replay_gain_fallback_db: p.replay_gain_fallback_db,
            replay_gain_fallback_to_agc: p.replay_gain_fallback_to_agc,
            replay_gain_prevent_clipping: p.replay_gain_prevent_clipping,
            strip_show_title: p.strip_show_title,
            strip_show_artist: p.strip_show_artist,
            strip_show_album: p.strip_show_album,
            strip_show_format_info: p.strip_show_format_info,
            strip_merged_mode: p.strip_merged_mode,
            strip_click_action: p.strip_click_action,
            active_playlist_id: p.active_playlist_id.clone(),
            active_playlist_name: p.active_playlist_name.clone(),
            active_playlist_comment: p.active_playlist_comment.clone(),
            eq_enabled: p.eq_enabled,
            eq_gains: p.eq_gains,
            custom_eq_presets: p.custom_eq_presets.clone(),
            verbose_config: p.verbose_config,
            artwork_resolution: p.artwork_resolution,
            show_album_artists_only: p.show_album_artists_only,
            suppress_library_refresh_toasts: p.suppress_library_refresh_toasts,
            queue_show_stars: p.queue_show_stars,
            queue_show_album: p.queue_show_album,
            queue_show_duration: p.queue_show_duration,
            queue_show_love: p.queue_show_love,
            queue_show_plays: p.queue_show_plays,
            albums_show_stars: p.albums_show_stars,
            albums_show_songcount: p.albums_show_songcount,
            albums_show_plays: p.albums_show_plays,
            albums_show_love: p.albums_show_love,
            songs_show_stars: p.songs_show_stars,
            songs_show_album: p.songs_show_album,
            songs_show_duration: p.songs_show_duration,
            songs_show_plays: p.songs_show_plays,
            songs_show_love: p.songs_show_love,
            artists_show_stars: p.artists_show_stars,
            artists_show_albumcount: p.artists_show_albumcount,
            artists_show_songcount: p.artists_show_songcount,
            artists_show_plays: p.artists_show_plays,
            artists_show_love: p.artists_show_love,
            albums_artwork_overlay: p.albums_artwork_overlay,
            artists_artwork_overlay: p.artists_artwork_overlay,
            songs_artwork_overlay: p.songs_artwork_overlay,
            playlists_artwork_overlay: p.playlists_artwork_overlay,
            artwork_column_mode: p.artwork_column_mode,
            artwork_column_stretch_fit: p.artwork_column_stretch_fit,
            artwork_column_width_pct: p.artwork_column_width_pct,
            show_tray_icon: p.show_tray_icon,
            close_to_tray: p.close_to_tray,
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

// =============================================================================
// Helpers
// =============================================================================

/// Apply TOML settings values onto the internal redb `PlayerSettings` struct.
///
/// Only overwrites user-facing preference fields — volume, playlist IDs, and
/// other runtime state are left untouched.
fn apply_toml_settings_to_internal(
    ts: &TomlSettings,
    p: &mut crate::types::settings::PlayerSettings,
) {
    p.start_view = ts.start_view.clone();
    p.enter_behavior = ts.enter_behavior;
    p.local_music_path = ts.local_music_path.clone();
    p.stable_viewport = ts.stable_viewport;
    p.auto_follow_playing = ts.auto_follow_playing;
    p.light_mode = ts.light_mode;
    p.rounded_mode = ts.rounded_mode;
    p.nav_layout = ts.nav_layout;
    p.nav_display_mode = ts.nav_display_mode;
    p.track_info_display = ts.track_info_display;
    p.slot_row_height = ts.slot_row_height;
    p.opacity_gradient = ts.opacity_gradient;
    p.slot_text_links = ts.slot_text_links;
    p.horizontal_volume = ts.horizontal_volume;
    p.font_family = ts.font_family.clone();
    p.strip_show_title = ts.strip_show_title;
    p.strip_show_artist = ts.strip_show_artist;
    p.strip_show_album = ts.strip_show_album;
    p.strip_show_format_info = ts.strip_show_format_info;
    p.strip_merged_mode = ts.strip_merged_mode;
    p.strip_click_action = ts.strip_click_action;
    p.crossfade_enabled = ts.crossfade_enabled;
    p.crossfade_duration_secs = ts.crossfade_duration_secs;
    p.volume_normalization = ts.volume_normalization;
    p.normalization_level = ts.normalization_level;
    p.replay_gain_preamp_db = ts.replay_gain_preamp_db;
    p.replay_gain_fallback_db = ts.replay_gain_fallback_db;
    p.replay_gain_fallback_to_agc = ts.replay_gain_fallback_to_agc;
    p.replay_gain_prevent_clipping = ts.replay_gain_prevent_clipping;
    p.visualization_mode = ts.visualization_mode;
    p.sound_effects_enabled = ts.sound_effects_enabled;
    p.sfx_volume = ts.sfx_volume as f64;
    p.scrobbling_enabled = ts.scrobbling_enabled;
    p.scrobble_threshold = ts.scrobble_threshold as f64;
    p.quick_add_to_playlist = ts.quick_add_to_playlist;
    p.queue_show_default_playlist = ts.queue_show_default_playlist;
    p.eq_enabled = ts.eq_enabled;
    p.eq_gains = ts.eq_gains;
    p.custom_eq_presets = ts.custom_eq_presets.clone();
    p.verbose_config = ts.verbose_config;
    p.library_page_size = ts.library_page_size;
    p.artwork_resolution = ts.artwork_resolution;
    p.show_album_artists_only = ts.show_album_artists_only;
    p.suppress_library_refresh_toasts = ts.suppress_library_refresh_toasts;
    p.queue_show_stars = ts.queue_show_stars;
    p.queue_show_album = ts.queue_show_album;
    p.queue_show_duration = ts.queue_show_duration;
    p.queue_show_love = ts.queue_show_love;
    p.queue_show_plays = ts.queue_show_plays;
    p.albums_show_stars = ts.albums_show_stars;
    p.albums_show_songcount = ts.albums_show_songcount;
    p.albums_show_plays = ts.albums_show_plays;
    p.albums_show_love = ts.albums_show_love;
    p.songs_show_stars = ts.songs_show_stars;
    p.songs_show_album = ts.songs_show_album;
    p.songs_show_duration = ts.songs_show_duration;
    p.songs_show_plays = ts.songs_show_plays;
    p.songs_show_love = ts.songs_show_love;
    p.artists_show_stars = ts.artists_show_stars;
    p.artists_show_albumcount = ts.artists_show_albumcount;
    p.artists_show_songcount = ts.artists_show_songcount;
    p.artists_show_plays = ts.artists_show_plays;
    p.artists_show_love = ts.artists_show_love;
    p.albums_artwork_overlay = ts.albums_artwork_overlay;
    p.artists_artwork_overlay = ts.artists_artwork_overlay;
    p.songs_artwork_overlay = ts.songs_artwork_overlay;
    p.playlists_artwork_overlay = ts.playlists_artwork_overlay;
    p.artwork_column_mode = ts.artwork_column_mode;
    p.artwork_column_stretch_fit = ts.artwork_column_stretch_fit;
    p.artwork_column_width_pct = ts.artwork_column_width_pct;
    p.show_tray_icon = ts.show_tray_icon;
    p.close_to_tray = ts.close_to_tray;
}

/// Convert `AllViewPreferences` into the internal `ViewPreferences` for redb storage.
impl From<crate::types::view_preferences::AllViewPreferences>
    for crate::types::settings::ViewPreferences
{
    fn from(avp: crate::types::view_preferences::AllViewPreferences) -> Self {
        Self {
            albums: avp.albums,
            artists: avp.artists,
            songs: avp.songs,
            genres: avp.genres,
            playlists: avp.playlists,
            queue: avp.queue,
        }
    }
}
