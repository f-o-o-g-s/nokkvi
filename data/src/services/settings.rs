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
    /// When true, `save()` skips writing `config.toml`. Test-only knob — keeps
    /// `cargo test` from clobbering the developer's real settings file when
    /// exercising setters that go through `save()`. Production paths always
    /// take the default `false`.
    skip_toml_writes: bool,
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

        let manager = Self {
            settings,
            storage,
            skip_toml_writes: false,
        };

        // Phase 4: Migration — if config.toml had no [settings], export redb values
        if !has_toml {
            tracing::info!("No [settings] section in config.toml — migrating from redb");
            if let Err(e) = manager.write_all_toml() {
                tracing::error!("Failed to migrate settings to config.toml: {e}");
            }
        }

        Ok(manager)
    }

    /// Test-only constructor — uses defaults for `UserSettings`, an
    /// in-memory `StateStorage`, and skips `config.toml` writes so unit
    /// tests don't trample the developer's real settings file.
    #[cfg(test)]
    pub(crate) fn for_test(storage: StateStorage) -> Self {
        Self {
            settings: UserSettings::default(),
            storage,
            skip_toml_writes: true,
        }
    }

    /// Save to redb (always) + config.toml sections (for user-facing settings).
    fn save(&self) -> Result<()> {
        // 1. Always write to redb (volume, playlist IDs, backward compat)
        self.storage.save("user_settings", &self.settings)?;
        // 2. Write user-facing settings to config.toml (skipped in unit tests)
        if !self.skip_toml_writes {
            self.write_settings_toml()?;
        }
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

    pub fn set_strip_show_labels(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.strip_show_labels = enabled;
        self.save()
    }

    pub fn set_strip_separator(
        &mut self,
        sep: crate::types::player_settings::StripSeparator,
    ) -> Result<()> {
        self.settings.player.strip_separator = sep;
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

    pub fn set_queue_show_index(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.queue_show_index = enabled;
        self.save()
    }

    pub fn set_queue_show_thumbnail(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.queue_show_thumbnail = enabled;
        self.save()
    }

    pub fn set_queue_show_genre(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.queue_show_genre = enabled;
        self.save()
    }

    pub fn set_queue_show_select(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.queue_show_select = enabled;
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

    pub fn set_albums_show_index(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.albums_show_index = enabled;
        self.save()
    }

    pub fn set_albums_show_thumbnail(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.albums_show_thumbnail = enabled;
        self.save()
    }

    pub fn set_albums_show_select(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.albums_show_select = enabled;
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

    pub fn set_songs_show_index(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.songs_show_index = enabled;
        self.save()
    }

    pub fn set_songs_show_thumbnail(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.songs_show_thumbnail = enabled;
        self.save()
    }

    pub fn set_songs_show_genre(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.songs_show_genre = enabled;
        self.save()
    }

    pub fn set_songs_show_select(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.songs_show_select = enabled;
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

    pub fn set_artists_show_index(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.artists_show_index = enabled;
        self.save()
    }

    pub fn set_artists_show_thumbnail(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.artists_show_thumbnail = enabled;
        self.save()
    }

    pub fn set_artists_show_select(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.artists_show_select = enabled;
        self.save()
    }

    pub fn set_genres_show_index(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.genres_show_index = enabled;
        self.save()
    }

    pub fn set_genres_show_thumbnail(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.genres_show_thumbnail = enabled;
        self.save()
    }

    pub fn set_genres_show_albumcount(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.genres_show_albumcount = enabled;
        self.save()
    }

    pub fn set_genres_show_songcount(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.genres_show_songcount = enabled;
        self.save()
    }

    pub fn set_genres_show_select(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.genres_show_select = enabled;
        self.save()
    }

    pub fn set_playlists_show_index(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.playlists_show_index = enabled;
        self.save()
    }

    pub fn set_playlists_show_thumbnail(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.playlists_show_thumbnail = enabled;
        self.save()
    }

    pub fn set_playlists_show_songcount(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.playlists_show_songcount = enabled;
        self.save()
    }

    pub fn set_playlists_show_duration(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.playlists_show_duration = enabled;
        self.save()
    }

    pub fn set_playlists_show_updatedat(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.playlists_show_updatedat = enabled;
        self.save()
    }

    pub fn set_playlists_show_select(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.playlists_show_select = enabled;
        self.save()
    }

    pub fn set_similar_show_index(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.similar_show_index = enabled;
        self.save()
    }

    pub fn set_similar_show_thumbnail(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.similar_show_thumbnail = enabled;
        self.save()
    }

    pub fn set_similar_show_album(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.similar_show_album = enabled;
        self.save()
    }

    pub fn set_similar_show_duration(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.similar_show_duration = enabled;
        self.save()
    }

    pub fn set_similar_show_love(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.similar_show_love = enabled;
        self.save()
    }

    pub fn set_similar_show_select(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.similar_show_select = enabled;
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

    /// Get player settings for Message::PlayerSettingsLoaded.
    ///
    /// Strangler-fig: per-tab `dump_<tab>_tab_player_settings` calls
    /// (macro-emitted from `services/settings_tables/`) own the keys that
    /// have been migrated to `define_settings!`. The hand-written tail below
    /// owns the residual fields; adding a `read:` closure to a per-tab entry
    /// removes its corresponding line here.
    pub fn get_player_settings(&self) -> crate::types::player_settings::PlayerSettings {
        let p = &self.settings.player;
        let mut out = crate::types::player_settings::PlayerSettings {
            // -- Residual: fields not yet migrated to `define_settings!`.
            // Adding a per-tab entry with a `read:` closure removes the
            // matching line here.
            volume: p.volume as f32,
            sfx_volume: p.sfx_volume as f32,
            sound_effects_enabled: p.sound_effects_enabled,
            visualization_mode: p.visualization_mode,
            local_music_path: p.local_music_path.clone(),
            default_playlist_id: p.default_playlist_id.clone(),
            default_playlist_name: p.default_playlist_name.clone(),
            font_family: p.font_family.clone(),
            active_playlist_id: p.active_playlist_id.clone(),
            active_playlist_name: p.active_playlist_name.clone(),
            active_playlist_comment: p.active_playlist_comment.clone(),
            eq_enabled: p.eq_enabled,
            eq_gains: p.eq_gains,
            custom_eq_presets: p.custom_eq_presets.clone(),
            verbose_config: p.verbose_config,
            artwork_resolution: p.artwork_resolution,
            show_album_artists_only: p.show_album_artists_only,
            queue_show_plays: p.queue_show_plays,
            queue_show_index: p.queue_show_index,
            queue_show_thumbnail: p.queue_show_thumbnail,
            queue_show_genre: p.queue_show_genre,
            queue_show_select: p.queue_show_select,
            albums_show_stars: p.albums_show_stars,
            albums_show_songcount: p.albums_show_songcount,
            albums_show_plays: p.albums_show_plays,
            albums_show_love: p.albums_show_love,
            albums_show_index: p.albums_show_index,
            albums_show_thumbnail: p.albums_show_thumbnail,
            albums_show_select: p.albums_show_select,
            songs_show_stars: p.songs_show_stars,
            songs_show_album: p.songs_show_album,
            songs_show_duration: p.songs_show_duration,
            songs_show_plays: p.songs_show_plays,
            songs_show_love: p.songs_show_love,
            songs_show_index: p.songs_show_index,
            songs_show_thumbnail: p.songs_show_thumbnail,
            songs_show_genre: p.songs_show_genre,
            songs_show_select: p.songs_show_select,
            artists_show_stars: p.artists_show_stars,
            artists_show_albumcount: p.artists_show_albumcount,
            artists_show_songcount: p.artists_show_songcount,
            artists_show_plays: p.artists_show_plays,
            artists_show_love: p.artists_show_love,
            artists_show_index: p.artists_show_index,
            artists_show_thumbnail: p.artists_show_thumbnail,
            artists_show_select: p.artists_show_select,
            genres_show_index: p.genres_show_index,
            genres_show_thumbnail: p.genres_show_thumbnail,
            genres_show_albumcount: p.genres_show_albumcount,
            genres_show_songcount: p.genres_show_songcount,
            genres_show_select: p.genres_show_select,
            playlists_show_index: p.playlists_show_index,
            playlists_show_thumbnail: p.playlists_show_thumbnail,
            playlists_show_songcount: p.playlists_show_songcount,
            playlists_show_duration: p.playlists_show_duration,
            playlists_show_updatedat: p.playlists_show_updatedat,
            playlists_show_select: p.playlists_show_select,
            similar_show_index: p.similar_show_index,
            similar_show_thumbnail: p.similar_show_thumbnail,
            similar_show_album: p.similar_show_album,
            similar_show_duration: p.similar_show_duration,
            similar_show_love: p.similar_show_love,
            similar_show_select: p.similar_show_select,
            artwork_column_width_pct: p.artwork_column_width_pct,

            // -- Macro-owned: overwritten by the per-tab dumpers below. The
            // values here are placeholders; the compiler enforces struct
            // completeness, and the dumpers run before this binding is
            // observed.
            scrobbling_enabled: p.scrobbling_enabled,
            scrobble_threshold: 0.0,
            start_view: String::new(),
            stable_viewport: false,
            auto_follow_playing: false,
            enter_behavior: EnterBehavior::default(),
            library_page_size: Default::default(),
            rounded_mode: false,
            nav_layout: NavLayout::default(),
            nav_display_mode: NavDisplayMode::default(),
            track_info_display: TrackInfoDisplay::default(),
            slot_row_height: SlotRowHeight::default(),
            opacity_gradient: false,
            slot_text_links: false,
            crossfade_enabled: false,
            crossfade_duration_secs: 0,
            quick_add_to_playlist: false,
            queue_show_default_playlist: false,
            horizontal_volume: false,
            volume_normalization: VolumeNormalizationMode::default(),
            normalization_level: NormalizationLevel::default(),
            replay_gain_preamp_db: 0.0,
            replay_gain_fallback_db: 0.0,
            replay_gain_fallback_to_agc: false,
            replay_gain_prevent_clipping: false,
            strip_show_title: false,
            strip_show_artist: false,
            strip_show_album: false,
            strip_show_format_info: false,
            strip_merged_mode: false,
            strip_click_action: StripClickAction::default(),
            strip_show_labels: false,
            strip_separator: Default::default(),
            suppress_library_refresh_toasts: false,
            queue_show_stars: false,
            queue_show_album: false,
            queue_show_duration: false,
            queue_show_love: false,
            albums_artwork_overlay: false,
            artists_artwork_overlay: false,
            songs_artwork_overlay: false,
            playlists_artwork_overlay: false,
            artwork_column_mode: ArtworkColumnMode::default(),
            artwork_column_stretch_fit: ArtworkStretchFit::default(),
            show_tray_icon: false,
            close_to_tray: false,
        };

        crate::services::settings_tables::dump_general_tab_player_settings(p, &mut out);
        crate::services::settings_tables::dump_interface_tab_player_settings(p, &mut out);
        crate::services::settings_tables::dump_playback_tab_player_settings(p, &mut out);

        out
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
///
/// Strangler-fig: per-tab `apply_toml_*_tab` calls (macro-generated from
/// `services/settings_tables/`) own keys that have been migrated to
/// `define_settings!` declarations. Keys still in the hand-written body
/// below are pending migration via per-tab follow-up commits.
fn apply_toml_settings_to_internal(
    ts: &TomlSettings,
    p: &mut crate::types::settings::PlayerSettings,
) {
    crate::services::settings_tables::apply_toml_general_tab(ts, p);
    crate::services::settings_tables::apply_toml_interface_tab(ts, p);
    crate::services::settings_tables::apply_toml_playback_tab(ts, p);

    p.local_music_path = ts.local_music_path.clone();
    p.light_mode = ts.light_mode;
    p.font_family = ts.font_family.clone();
    p.visualization_mode = ts.visualization_mode;
    p.sound_effects_enabled = ts.sound_effects_enabled;
    p.sfx_volume = ts.sfx_volume as f64;
    p.eq_enabled = ts.eq_enabled;
    p.eq_gains = ts.eq_gains;
    p.custom_eq_presets = ts.custom_eq_presets.clone();
    p.verbose_config = ts.verbose_config;
    p.artwork_resolution = ts.artwork_resolution;
    p.show_album_artists_only = ts.show_album_artists_only;
    p.queue_show_plays = ts.queue_show_plays;
    p.queue_show_index = ts.queue_show_index;
    p.queue_show_thumbnail = ts.queue_show_thumbnail;
    p.queue_show_select = ts.queue_show_select;
    p.albums_show_stars = ts.albums_show_stars;
    p.albums_show_songcount = ts.albums_show_songcount;
    p.albums_show_plays = ts.albums_show_plays;
    p.albums_show_love = ts.albums_show_love;
    p.albums_show_index = ts.albums_show_index;
    p.albums_show_thumbnail = ts.albums_show_thumbnail;
    p.albums_show_select = ts.albums_show_select;
    p.songs_show_stars = ts.songs_show_stars;
    p.songs_show_album = ts.songs_show_album;
    p.songs_show_duration = ts.songs_show_duration;
    p.songs_show_plays = ts.songs_show_plays;
    p.songs_show_love = ts.songs_show_love;
    p.songs_show_index = ts.songs_show_index;
    p.songs_show_thumbnail = ts.songs_show_thumbnail;
    p.songs_show_select = ts.songs_show_select;
    p.artists_show_stars = ts.artists_show_stars;
    p.artists_show_albumcount = ts.artists_show_albumcount;
    p.artists_show_songcount = ts.artists_show_songcount;
    p.artists_show_plays = ts.artists_show_plays;
    p.artists_show_love = ts.artists_show_love;
    p.artists_show_index = ts.artists_show_index;
    p.artists_show_thumbnail = ts.artists_show_thumbnail;
    p.artists_show_select = ts.artists_show_select;
    p.genres_show_index = ts.genres_show_index;
    p.genres_show_thumbnail = ts.genres_show_thumbnail;
    p.genres_show_albumcount = ts.genres_show_albumcount;
    p.genres_show_songcount = ts.genres_show_songcount;
    p.genres_show_select = ts.genres_show_select;
    p.playlists_show_index = ts.playlists_show_index;
    p.playlists_show_thumbnail = ts.playlists_show_thumbnail;
    p.playlists_show_songcount = ts.playlists_show_songcount;
    p.playlists_show_duration = ts.playlists_show_duration;
    p.playlists_show_updatedat = ts.playlists_show_updatedat;
    p.playlists_show_select = ts.playlists_show_select;
    p.similar_show_index = ts.similar_show_index;
    p.similar_show_thumbnail = ts.similar_show_thumbnail;
    p.similar_show_album = ts.similar_show_album;
    p.similar_show_duration = ts.similar_show_duration;
    p.similar_show_love = ts.similar_show_love;
    p.similar_show_select = ts.similar_show_select;
    p.artwork_column_width_pct = ts.artwork_column_width_pct;
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
