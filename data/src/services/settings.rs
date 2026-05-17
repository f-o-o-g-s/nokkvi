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
        let redb_settings =
            match storage.load::<UserSettings>(crate::services::storage_keys::USER_SETTINGS) {
                Ok(Some(s)) => s,
                Ok(None) => UserSettings::default(),
                Err(e) => {
                    tracing::warn!("Settings deserialization failed, resetting to defaults: {e}");
                    let defaults = UserSettings::default();
                    let _ = storage.save(crate::services::storage_keys::USER_SETTINGS, &defaults);
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

    /// Test-only constructor — starts from default `UserSettings` but
    /// overrides the `player` substruct with the caller-supplied value, so
    /// round-trip tests can inject an exhaustive non-default `PlayerSettings`
    /// without driving every setter individually.
    #[cfg(test)]
    pub(crate) fn for_test_with_player(
        storage: StateStorage,
        player: crate::types::settings::PlayerSettings,
    ) -> Self {
        let mut settings = UserSettings::default();
        settings.player = player;
        Self {
            settings,
            storage,
            skip_toml_writes: true,
        }
    }

    /// Save to redb (always) + config.toml sections (for user-facing settings).
    fn save(&self) -> Result<()> {
        // 1. Always write to redb (volume, playlist IDs, backward compat)
        self.storage
            .save(crate::services::storage_keys::USER_SETTINGS, &self.settings)?;
        // 2. Write user-facing settings to config.toml (skipped in unit tests)
        if !self.skip_toml_writes {
            self.write_settings_toml()?;
        }
        Ok(())
    }

    /// Save only to redb — used for high-frequency operations (volume) and
    /// runtime state (active playlist) that don't belong in config.toml.
    fn save_redb_only(&self) -> Result<()> {
        self.storage
            .save(crate::services::storage_keys::USER_SETTINGS, &self.settings)?;
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
        use crate::types::player_settings::{
            ARTWORK_COLUMN_WIDTH_PCT_MAX, ARTWORK_COLUMN_WIDTH_PCT_MIN,
        };
        self.settings.player.artwork_column_width_pct =
            pct.clamp(ARTWORK_COLUMN_WIDTH_PCT_MIN, ARTWORK_COLUMN_WIDTH_PCT_MAX);
        self.save()
    }

    pub fn set_artwork_auto_max_pct(&mut self, pct: f32) -> Result<()> {
        use crate::types::player_settings::{ARTWORK_AUTO_MAX_PCT_MAX, ARTWORK_AUTO_MAX_PCT_MIN};
        self.settings.player.artwork_auto_max_pct =
            pct.clamp(ARTWORK_AUTO_MAX_PCT_MIN, ARTWORK_AUTO_MAX_PCT_MAX);
        self.save()
    }

    pub fn set_artwork_vertical_height_pct(&mut self, pct: f32) -> Result<()> {
        use crate::types::player_settings::{
            ARTWORK_VERTICAL_HEIGHT_PCT_MAX, ARTWORK_VERTICAL_HEIGHT_PCT_MIN,
        };
        self.settings.player.artwork_vertical_height_pct = pct.clamp(
            ARTWORK_VERTICAL_HEIGHT_PCT_MIN,
            ARTWORK_VERTICAL_HEIGHT_PCT_MAX,
        );
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
        self.storage
            .save(crate::services::storage_keys::USER_SETTINGS, &self.settings)?;
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
        self.storage
            .save(crate::services::storage_keys::USER_SETTINGS, &self.settings)?;
        self.write_hotkeys_toml()?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Getters
    // -------------------------------------------------------------------------

    /// Get player settings for Message::PlayerSettingsLoaded.
    ///
    /// Composition: start from default UI-facing `PlayerSettings`, populate
    /// the runtime-only fields (volume, playlist IDs that don't round-trip
    /// through `config.toml`) and the scalar residuals not yet owned by any
    /// per-tab or per-view-column macro, then run the 3 per-tab dumpers and
    /// 7 per-view-column dumpers to cover the migrated entries.
    pub fn get_player_settings(&self) -> crate::types::player_settings::PlayerSettings {
        let p = &self.settings.player;
        let mut out = crate::types::player_settings::PlayerSettings {
            // Runtime-only fields — these live in redb only and never
            // round-trip through config.toml. The dumpers below intentionally
            // do NOT touch these.
            volume: p.volume as f32,
            default_playlist_id: p.default_playlist_id.clone(),
            default_playlist_name: p.default_playlist_name.clone(),
            active_playlist_id: p.active_playlist_id.clone(),
            active_playlist_name: p.active_playlist_name.clone(),
            active_playlist_comment: p.active_playlist_comment.clone(),

            // Scalar residuals — fields not (yet) owned by any per-tab or
            // per-view-column macro (paralleling the same residuals in
            // `apply_toml_settings_to_internal` and
            // `TomlSettings::from_player_settings`).
            sfx_volume: p.sfx_volume as f32,
            sound_effects_enabled: p.sound_effects_enabled,
            visualization_mode: p.visualization_mode,
            font_family: p.font_family.clone(),
            eq_enabled: p.eq_enabled,
            eq_gains: p.eq_gains,
            custom_eq_presets: p.custom_eq_presets.clone(),
            artwork_column_width_pct: p.artwork_column_width_pct,
            ..Default::default()
        };

        // Per-tab macro-emitted dumpers (define_settings! `read:` closures).
        crate::services::settings_tables::dump_general_tab_player_settings(p, &mut out);
        crate::services::settings_tables::dump_interface_tab_player_settings(p, &mut out);
        crate::services::settings_tables::dump_playback_tab_player_settings(p, &mut out);

        // Per-view-column macro-emitted dumpers (define_view_column_toml_helpers!).
        crate::types::view_column_toml::dump_albums_columns_to_player(p, &mut out);
        crate::types::view_column_toml::dump_artists_columns_to_player(p, &mut out);
        crate::types::view_column_toml::dump_genres_columns_to_player(p, &mut out);
        crate::types::view_column_toml::dump_playlists_columns_to_player(p, &mut out);
        crate::types::view_column_toml::dump_similar_columns_to_player(p, &mut out);
        crate::types::view_column_toml::dump_songs_columns_to_player(p, &mut out);
        crate::types::view_column_toml::dump_queue_columns_to_player(p, &mut out);

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
/// Composition: 3 per-tab `apply_toml_<tab>` macro-emitted helpers cover the
/// migrated `define_settings!` entries; 7 per-view `apply_toml_<view>_columns`
/// macro-emitted helpers cover every column-toggle bool — including
/// `queue_show_genre` and `songs_show_genre` that the previous hand-written
/// body silently dropped. The remaining hand-written assignments cover the
/// scalar residuals (`font_family`, visualizer/SFX/EQ fields) that are not
/// yet owned by any per-tab or per-view-column macro invocation.
///
/// `light_mode` and a handful of other fields are still applied here because
/// they round-trip through `TomlSettings` but are not yet routed through a
/// macro entry — see the inline comments.
fn apply_toml_settings_to_internal(
    ts: &TomlSettings,
    p: &mut crate::types::settings::PlayerSettings,
) {
    // Per-tab macro-emitted appliers (define_settings! `toml_apply:` closures).
    crate::services::settings_tables::apply_toml_general_tab(ts, p);
    crate::services::settings_tables::apply_toml_interface_tab(ts, p);
    crate::services::settings_tables::apply_toml_playback_tab(ts, p);

    // Per-view-column macro-emitted appliers (define_view_column_toml_helpers!).
    // These close the silent-drop bug for queue_show_genre / songs_show_genre
    // that the pre-refactor hand-written body did not cover.
    crate::types::view_column_toml::apply_toml_albums_columns(ts, p);
    crate::types::view_column_toml::apply_toml_artists_columns(ts, p);
    crate::types::view_column_toml::apply_toml_genres_columns(ts, p);
    crate::types::view_column_toml::apply_toml_playlists_columns(ts, p);
    crate::types::view_column_toml::apply_toml_similar_columns(ts, p);
    crate::types::view_column_toml::apply_toml_songs_columns(ts, p);
    crate::types::view_column_toml::apply_toml_queue_columns(ts, p);

    // Hand-written residuals — scalar fields not (yet) owned by any per-tab
    // or per-view-column macro invocation:
    //
    // - `font_family` routes through Message::ApplyFont, not a tab dispatcher.
    // - The 3 audio/visualizer scalars (`visualization_mode`,
    //   `sound_effects_enabled`, `sfx_volume`) and 3 EQ fields (`eq_enabled`,
    //   `eq_gains`, `custom_eq_presets`) live on different code paths.
    // - `artwork_column_width_pct` is the pixel-drag-driven slider that
    //   intentionally has no UI dispatch arm.
    p.font_family = ts.font_family.clone();
    p.visualization_mode = ts.visualization_mode;
    p.sound_effects_enabled = ts.sound_effects_enabled;
    p.sfx_volume = ts.sfx_volume as f64;
    p.eq_enabled = ts.eq_enabled;
    p.eq_gains = ts.eq_gains;
    p.custom_eq_presets = ts.custom_eq_presets.clone();
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

/// Implemented by column enums emitted by `define_view_columns!` (with `=> setter` annotations).
/// Routes a column variant + bool to its `SettingsManager` setter so `SettingsService::set_column_visibility`
/// can persist the toggle without per-view boilerplate.
pub trait ColumnPersist: Copy + Send + 'static {
    fn apply_to_settings(self, sm: &mut SettingsManager, value: bool) -> Result<()>;
}

// =============================================================================
// Sentinel round-trip tests (Group G Phase 2 — PlayerSettings TOML compat)
// =============================================================================
//
// These tests pin the `PlayerSettings → TomlSettings → bytes → TomlSettings →
// PlayerSettings` round-trip semantics that the on-disk config.toml contract
// depends on. They guard subsequent commits that extend `define_view_columns!`
// and `define_settings!` to collapse the ~238 lines of hand-written field
// copies across `from_player_settings`, `get_player_settings`, and
// `apply_toml_settings_to_internal`.
//
// `build_exhaustive_internal_player_settings()` deliberately lists every
// persisted field — `..Default::default()` is BANNED so a future field
// addition fails to compile here until the round-trip wiring is added.

#[cfg(test)]
mod sentinel_roundtrip_tests {
    use tempfile::TempDir;

    use super::*;
    use crate::{
        audio::eq::CustomEqPreset,
        types::{
            player_settings::{
                ArtworkColumnMode, ArtworkResolution, ArtworkStretchFit, EnterBehavior,
                LibraryPageSize, NavDisplayMode, NavLayout, NormalizationLevel, SlotRowHeight,
                StripClickAction, StripSeparator, TrackInfoDisplay, VisualizationMode,
                VolumeNormalizationMode,
            },
            settings::PlayerSettings as InternalPlayerSettings,
            toml_settings::TomlSettings,
        },
    };

    fn make_test_manager_with_player(player: InternalPlayerSettings) -> (SettingsManager, TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("test_settings.redb");
        let storage = StateStorage::new(path).expect("StateStorage::new");
        (SettingsManager::for_test_with_player(storage, player), tmp)
    }

    /// Exhaustive internal `PlayerSettings` with every persisted field set to
    /// a non-default sentinel. Listed without `..Default::default()` so a new
    /// field addition surfaces here as a missing-initializer compile error.
    ///
    /// Sentinels are chosen so that `field == Default::default()` is false
    /// for every field: bools flipped from their default; enums set to a
    /// non-`#[default]` variant; numerics offset; strings made non-empty.
    fn build_exhaustive_internal_player_settings() -> InternalPlayerSettings {
        InternalPlayerSettings {
            // Runtime-only fields (excluded from the round-trip assertion):
            volume: 0.42,
            default_playlist_id: Some("playlist-42".to_string()),
            default_playlist_name: "My Default Playlist".to_string(),
            active_playlist_id: Some("playlist-99".to_string()),
            active_playlist_name: "Active Playlist".to_string(),
            active_playlist_comment: "comment text".to_string(),

            // Audio knobs
            sfx_volume: 0.3142,
            sound_effects_enabled: false,                 // default true
            visualization_mode: VisualizationMode::Lines, // default Bars
            light_mode: true, // default false; UI-PS lacks this field — written as false
            scrobbling_enabled: false, // default true
            scrobble_threshold: 0.8123, // default 0.50

            // General
            start_view: "Albums".to_string(), // default "Queue"
            stable_viewport: false,           // default true
            auto_follow_playing: false,       // default true
            enter_behavior: EnterBehavior::AppendAndPlay, // default PlayAll
            local_music_path: "/tmp/sentinel/music".to_string(),
            rounded_mode: true,                           // default false
            nav_layout: NavLayout::Side,                  // default Top
            nav_display_mode: NavDisplayMode::IconsOnly,  // default TextOnly
            track_info_display: TrackInfoDisplay::TopBar, // default Off
            slot_row_height: SlotRowHeight::Spacious,     // default Default
            opacity_gradient: false,                      // default true
            slot_text_links: false,                       // default true

            // Playback / crossfade
            crossfade_enabled: true,    // default false
            crossfade_duration_secs: 9, // default 5

            // Playlists
            quick_add_to_playlist: true,       // default false
            queue_show_default_playlist: true, // default false
            horizontal_volume: true,           // default false
            font_family: "Sentinel Mono".to_string(),

            // Volume normalization
            volume_normalization: VolumeNormalizationMode::ReplayGainAlbum, // default Off
            normalization_level: NormalizationLevel::Loud,                  // default Normal
            replay_gain_preamp_db: -3.2517,                                 // default 0.0
            replay_gain_fallback_db: 1.7384,                                // default 0.0
            replay_gain_fallback_to_agc: true,                              // default false
            replay_gain_prevent_clipping: false,                            // default true

            // Metadata strip
            strip_show_title: false,       // default true
            strip_show_artist: false,      // default true
            strip_show_album: false,       // default true
            strip_show_format_info: false, // default true
            strip_merged_mode: true,       // default false
            strip_click_action: StripClickAction::CopyTrackInfo, // default GoToQueue
            strip_show_labels: false,      // default true
            strip_separator: StripSeparator::Slash, // default Dot

            // EQ
            eq_enabled: true, // default false
            eq_gains: [4.0, 3.5, 1.5, 0.0, -1.5, -0.5, 0.5, 2.0, 3.5, 4.0],
            custom_eq_presets: vec![CustomEqPreset {
                name: "Sentinel Preset".to_string(),
                gains: [0.0, -3.5, 8.5, -8.25, 5.875, 0.0, 1.5, 3.0, 4.0, 5.0],
            }],

            // Verbose / library
            verbose_config: true,                            // default false
            library_page_size: LibraryPageSize::Massive,     // default Default
            artwork_resolution: ArtworkResolution::Original, // default Default
            show_album_artists_only: false,                  // default true
            suppress_library_refresh_toasts: true,           // default false

            // Queue columns
            queue_show_stars: false,     // default true
            queue_show_album: false,     // default true
            queue_show_duration: false,  // default true
            queue_show_love: false,      // default true
            queue_show_plays: true,      // default false
            queue_show_index: false,     // default true
            queue_show_thumbnail: false, // default true
            queue_show_genre: true,      // default false
            queue_show_select: true,     // default false

            // Albums columns
            albums_show_stars: true,      // default false
            albums_show_songcount: false, // default true
            albums_show_plays: true,      // default false
            albums_show_love: false,      // default true
            albums_show_index: false,     // default true
            albums_show_thumbnail: false, // default true
            albums_show_select: true,     // default false

            // Songs columns
            songs_show_stars: true,      // default false
            songs_show_album: false,     // default true
            songs_show_duration: false,  // default true
            songs_show_plays: true,      // default false
            songs_show_love: false,      // default true
            songs_show_index: false,     // default true
            songs_show_thumbnail: false, // default true
            songs_show_genre: true,      // default false
            songs_show_select: true,     // default false

            // Artists columns
            artists_show_stars: false,      // default true
            artists_show_albumcount: false, // default true
            artists_show_songcount: false,  // default true
            artists_show_plays: false,      // default true
            artists_show_love: false,       // default true
            artists_show_index: false,      // default true
            artists_show_thumbnail: false,  // default true
            artists_show_select: true,      // default false

            // Genres columns
            genres_show_index: false,      // default true
            genres_show_thumbnail: false,  // default true
            genres_show_albumcount: false, // default true
            genres_show_songcount: false,  // default true
            genres_show_select: true,      // default false

            // Playlists columns
            playlists_show_index: false,     // default true
            playlists_show_thumbnail: false, // default true
            playlists_show_songcount: true,  // default false
            playlists_show_duration: true,   // default false
            playlists_show_updatedat: true,  // default false
            playlists_show_select: true,     // default false

            // Similar columns
            similar_show_index: false,     // default true
            similar_show_thumbnail: false, // default true
            similar_show_album: false,     // default true
            similar_show_duration: false,  // default true
            similar_show_love: false,      // default true
            similar_show_select: true,     // default false

            // Per-view artwork overlay
            albums_artwork_overlay: false,    // default true
            artists_artwork_overlay: false,   // default true
            songs_artwork_overlay: false,     // default true
            playlists_artwork_overlay: false, // default true

            // Artwork column layout
            artwork_column_mode: ArtworkColumnMode::AlwaysStretched, // default Auto
            artwork_column_stretch_fit: ArtworkStretchFit::Fill,     // default Cover
            artwork_column_width_pct: 0.6543,
            artwork_auto_max_pct: 0.5234,
            artwork_vertical_height_pct: 0.6789,

            // System tray
            show_tray_icon: true, // default false
            close_to_tray: true,  // default false
        }
    }

    /// Full-field round-trip: build exhaustive internal `PlayerSettings`, dump
    /// to UI-facing `PlayerSettings`, convert to `TomlSettings`, serialize,
    /// deserialize, apply back onto a fresh internal `PlayerSettings`, dump
    /// again — and confirm every persisted field survives. The 6 f32 fields
    /// routed through `round_f32` / `round_f32_array` use 1e-4 tolerance
    /// (they are quantized to 4 decimals on TOML emit).
    #[test]
    fn player_settings_toml_roundtrip_full_field_coverage() {
        let internal_src = build_exhaustive_internal_player_settings();

        // Stamp the exhaustive sentinel onto a SettingsManager and dump.
        let (sm, _tmp) = make_test_manager_with_player(internal_src.clone());
        let ui_ps1 = sm.get_player_settings();

        // UI → TOML → bytes → TOML.
        let ts1 = TomlSettings::from_player_settings(&ui_ps1);
        let serialized = toml::to_string(&ts1).expect("serialize TomlSettings");
        let ts2: TomlSettings = toml::from_str(&serialized).expect("deserialize TomlSettings");

        // Apply onto a fresh internal `PlayerSettings`, then dump again via a
        // fresh manager — this exercises the same get_player_settings flow as
        // production startup.
        let mut internal_dst = InternalPlayerSettings::default();
        apply_toml_settings_to_internal(&ts2, &mut internal_dst);
        let (sm2, _tmp2) = make_test_manager_with_player(internal_dst);
        let ui_ps2 = sm2.get_player_settings();

        // Field-by-field — every persisted UI field except the runtime ones
        // (volume / active_playlist_* / default_playlist_*, which live in
        // redb only).

        // Audio knobs
        assert!(
            (ui_ps1.sfx_volume - ui_ps2.sfx_volume).abs() < 1e-4,
            "sfx_volume: {} vs {}",
            ui_ps1.sfx_volume,
            ui_ps2.sfx_volume
        );
        assert_eq!(ui_ps1.sound_effects_enabled, ui_ps2.sound_effects_enabled);
        assert_eq!(ui_ps1.visualization_mode, ui_ps2.visualization_mode);
        assert_eq!(ui_ps1.scrobbling_enabled, ui_ps2.scrobbling_enabled);
        assert!(
            (ui_ps1.scrobble_threshold - ui_ps2.scrobble_threshold).abs() < 1e-4,
            "scrobble_threshold: {} vs {}",
            ui_ps1.scrobble_threshold,
            ui_ps2.scrobble_threshold
        );

        // General
        assert_eq!(ui_ps1.start_view, ui_ps2.start_view);
        assert_eq!(ui_ps1.stable_viewport, ui_ps2.stable_viewport);
        assert_eq!(ui_ps1.auto_follow_playing, ui_ps2.auto_follow_playing);
        assert_eq!(ui_ps1.enter_behavior, ui_ps2.enter_behavior);
        assert_eq!(ui_ps1.local_music_path, ui_ps2.local_music_path);
        assert_eq!(ui_ps1.rounded_mode, ui_ps2.rounded_mode);
        assert_eq!(ui_ps1.nav_layout, ui_ps2.nav_layout);
        assert_eq!(ui_ps1.nav_display_mode, ui_ps2.nav_display_mode);
        assert_eq!(ui_ps1.track_info_display, ui_ps2.track_info_display);
        assert_eq!(ui_ps1.slot_row_height, ui_ps2.slot_row_height);
        assert_eq!(ui_ps1.opacity_gradient, ui_ps2.opacity_gradient);
        assert_eq!(ui_ps1.slot_text_links, ui_ps2.slot_text_links);

        // Playback / crossfade
        assert_eq!(ui_ps1.crossfade_enabled, ui_ps2.crossfade_enabled);
        assert_eq!(
            ui_ps1.crossfade_duration_secs,
            ui_ps2.crossfade_duration_secs
        );

        // Playlists
        assert_eq!(ui_ps1.quick_add_to_playlist, ui_ps2.quick_add_to_playlist);
        assert_eq!(
            ui_ps1.queue_show_default_playlist,
            ui_ps2.queue_show_default_playlist
        );
        assert_eq!(ui_ps1.horizontal_volume, ui_ps2.horizontal_volume);
        assert_eq!(ui_ps1.font_family, ui_ps2.font_family);

        // Volume normalization
        assert_eq!(ui_ps1.volume_normalization, ui_ps2.volume_normalization);
        assert_eq!(ui_ps1.normalization_level, ui_ps2.normalization_level);
        assert!(
            (ui_ps1.replay_gain_preamp_db - ui_ps2.replay_gain_preamp_db).abs() < 1e-4,
            "replay_gain_preamp_db: {} vs {}",
            ui_ps1.replay_gain_preamp_db,
            ui_ps2.replay_gain_preamp_db
        );
        assert!(
            (ui_ps1.replay_gain_fallback_db - ui_ps2.replay_gain_fallback_db).abs() < 1e-4,
            "replay_gain_fallback_db: {} vs {}",
            ui_ps1.replay_gain_fallback_db,
            ui_ps2.replay_gain_fallback_db
        );
        assert_eq!(
            ui_ps1.replay_gain_fallback_to_agc,
            ui_ps2.replay_gain_fallback_to_agc
        );
        assert_eq!(
            ui_ps1.replay_gain_prevent_clipping,
            ui_ps2.replay_gain_prevent_clipping
        );

        // Metadata strip
        assert_eq!(ui_ps1.strip_show_title, ui_ps2.strip_show_title);
        assert_eq!(ui_ps1.strip_show_artist, ui_ps2.strip_show_artist);
        assert_eq!(ui_ps1.strip_show_album, ui_ps2.strip_show_album);
        assert_eq!(ui_ps1.strip_show_format_info, ui_ps2.strip_show_format_info);
        assert_eq!(ui_ps1.strip_merged_mode, ui_ps2.strip_merged_mode);
        assert_eq!(ui_ps1.strip_click_action, ui_ps2.strip_click_action);
        assert_eq!(ui_ps1.strip_show_labels, ui_ps2.strip_show_labels);
        assert_eq!(ui_ps1.strip_separator, ui_ps2.strip_separator);

        // EQ
        assert_eq!(ui_ps1.eq_enabled, ui_ps2.eq_enabled);
        for (i, (a, b)) in ui_ps1
            .eq_gains
            .iter()
            .zip(ui_ps2.eq_gains.iter())
            .enumerate()
        {
            assert!((a - b).abs() < 1e-4, "eq_gains[{i}]: {a} vs {b}");
        }
        assert_eq!(
            ui_ps1.custom_eq_presets.len(),
            ui_ps2.custom_eq_presets.len(),
            "custom_eq_presets length"
        );
        for (idx, (pa, pb)) in ui_ps1
            .custom_eq_presets
            .iter()
            .zip(ui_ps2.custom_eq_presets.iter())
            .enumerate()
        {
            assert_eq!(pa.name, pb.name, "custom_eq_presets[{idx}].name");
            for (band, (a, b)) in pa.gains.iter().zip(pb.gains.iter()).enumerate() {
                assert!(
                    (a - b).abs() < 1e-4,
                    "custom_eq_presets[{idx}].gains[{band}]: {a} vs {b}",
                );
            }
        }

        // Library / verbose
        assert_eq!(ui_ps1.verbose_config, ui_ps2.verbose_config);
        assert_eq!(ui_ps1.library_page_size, ui_ps2.library_page_size);
        assert_eq!(ui_ps1.artwork_resolution, ui_ps2.artwork_resolution);
        assert_eq!(
            ui_ps1.show_album_artists_only,
            ui_ps2.show_album_artists_only
        );
        assert_eq!(
            ui_ps1.suppress_library_refresh_toasts,
            ui_ps2.suppress_library_refresh_toasts
        );

        // Queue columns
        assert_eq!(ui_ps1.queue_show_stars, ui_ps2.queue_show_stars);
        assert_eq!(ui_ps1.queue_show_album, ui_ps2.queue_show_album);
        assert_eq!(ui_ps1.queue_show_duration, ui_ps2.queue_show_duration);
        assert_eq!(ui_ps1.queue_show_love, ui_ps2.queue_show_love);
        assert_eq!(ui_ps1.queue_show_plays, ui_ps2.queue_show_plays);
        assert_eq!(ui_ps1.queue_show_index, ui_ps2.queue_show_index);
        assert_eq!(ui_ps1.queue_show_thumbnail, ui_ps2.queue_show_thumbnail);
        // queue_show_genre was silently dropped on apply prior to commit 5
        // (the field ships in TomlSettings with serde wiring but was missing
        // from both the hand-written apply body and every per-tab macro
        // entry). Commit 5's macro-helper collapse picks it up through the
        // queue-columns helpers, closing the silent-drop bug. This assertion
        // is the fold-in: it now requires a real round-trip rather than
        // pinning the buggy default-snap-back behavior.
        assert!(
            ui_ps1.queue_show_genre,
            "sentinel sets queue_show_genre=true"
        );
        assert_eq!(
            ui_ps1.queue_show_genre, ui_ps2.queue_show_genre,
            "queue_show_genre must round-trip through TOML→internal apply",
        );
        assert_eq!(ui_ps1.queue_show_select, ui_ps2.queue_show_select);

        // Albums columns
        assert_eq!(ui_ps1.albums_show_stars, ui_ps2.albums_show_stars);
        assert_eq!(ui_ps1.albums_show_songcount, ui_ps2.albums_show_songcount);
        assert_eq!(ui_ps1.albums_show_plays, ui_ps2.albums_show_plays);
        assert_eq!(ui_ps1.albums_show_love, ui_ps2.albums_show_love);
        assert_eq!(ui_ps1.albums_show_index, ui_ps2.albums_show_index);
        assert_eq!(ui_ps1.albums_show_thumbnail, ui_ps2.albums_show_thumbnail);
        assert_eq!(ui_ps1.albums_show_select, ui_ps2.albums_show_select);

        // Songs columns
        assert_eq!(ui_ps1.songs_show_stars, ui_ps2.songs_show_stars);
        assert_eq!(ui_ps1.songs_show_album, ui_ps2.songs_show_album);
        assert_eq!(ui_ps1.songs_show_duration, ui_ps2.songs_show_duration);
        assert_eq!(ui_ps1.songs_show_plays, ui_ps2.songs_show_plays);
        assert_eq!(ui_ps1.songs_show_love, ui_ps2.songs_show_love);
        assert_eq!(ui_ps1.songs_show_index, ui_ps2.songs_show_index);
        assert_eq!(ui_ps1.songs_show_thumbnail, ui_ps2.songs_show_thumbnail);
        // songs_show_genre was silently dropped on apply prior to commit 5
        // (same shape as queue_show_genre — the field ships in TomlSettings
        // with serde wiring but was missing from both the hand-written apply
        // body and every per-tab macro entry). The macro-helper collapse
        // picks it up through the songs-columns helpers. Fold-in: real
        // round-trip assertion instead of the buggy-default pin.
        assert!(
            ui_ps1.songs_show_genre,
            "sentinel sets songs_show_genre=true"
        );
        assert_eq!(
            ui_ps1.songs_show_genre, ui_ps2.songs_show_genre,
            "songs_show_genre must round-trip through TOML→internal apply",
        );
        assert_eq!(ui_ps1.songs_show_select, ui_ps2.songs_show_select);

        // Artists columns
        assert_eq!(ui_ps1.artists_show_stars, ui_ps2.artists_show_stars);
        assert_eq!(
            ui_ps1.artists_show_albumcount,
            ui_ps2.artists_show_albumcount
        );
        assert_eq!(ui_ps1.artists_show_songcount, ui_ps2.artists_show_songcount);
        assert_eq!(ui_ps1.artists_show_plays, ui_ps2.artists_show_plays);
        assert_eq!(ui_ps1.artists_show_love, ui_ps2.artists_show_love);
        assert_eq!(ui_ps1.artists_show_index, ui_ps2.artists_show_index);
        assert_eq!(ui_ps1.artists_show_thumbnail, ui_ps2.artists_show_thumbnail);
        assert_eq!(ui_ps1.artists_show_select, ui_ps2.artists_show_select);

        // Genres columns
        assert_eq!(ui_ps1.genres_show_index, ui_ps2.genres_show_index);
        assert_eq!(ui_ps1.genres_show_thumbnail, ui_ps2.genres_show_thumbnail);
        assert_eq!(ui_ps1.genres_show_albumcount, ui_ps2.genres_show_albumcount);
        assert_eq!(ui_ps1.genres_show_songcount, ui_ps2.genres_show_songcount);
        assert_eq!(ui_ps1.genres_show_select, ui_ps2.genres_show_select);

        // Playlists columns
        assert_eq!(ui_ps1.playlists_show_index, ui_ps2.playlists_show_index);
        assert_eq!(
            ui_ps1.playlists_show_thumbnail,
            ui_ps2.playlists_show_thumbnail
        );
        assert_eq!(
            ui_ps1.playlists_show_songcount,
            ui_ps2.playlists_show_songcount
        );
        assert_eq!(
            ui_ps1.playlists_show_duration,
            ui_ps2.playlists_show_duration
        );
        assert_eq!(
            ui_ps1.playlists_show_updatedat,
            ui_ps2.playlists_show_updatedat
        );
        assert_eq!(ui_ps1.playlists_show_select, ui_ps2.playlists_show_select);

        // Similar columns
        assert_eq!(ui_ps1.similar_show_index, ui_ps2.similar_show_index);
        assert_eq!(ui_ps1.similar_show_thumbnail, ui_ps2.similar_show_thumbnail);
        assert_eq!(ui_ps1.similar_show_album, ui_ps2.similar_show_album);
        assert_eq!(ui_ps1.similar_show_duration, ui_ps2.similar_show_duration);
        assert_eq!(ui_ps1.similar_show_love, ui_ps2.similar_show_love);
        assert_eq!(ui_ps1.similar_show_select, ui_ps2.similar_show_select);

        // Per-view artwork overlay
        assert_eq!(ui_ps1.albums_artwork_overlay, ui_ps2.albums_artwork_overlay);
        assert_eq!(
            ui_ps1.artists_artwork_overlay,
            ui_ps2.artists_artwork_overlay
        );
        assert_eq!(ui_ps1.songs_artwork_overlay, ui_ps2.songs_artwork_overlay);
        assert_eq!(
            ui_ps1.playlists_artwork_overlay,
            ui_ps2.playlists_artwork_overlay
        );

        // Artwork column layout
        assert_eq!(ui_ps1.artwork_column_mode, ui_ps2.artwork_column_mode);
        assert_eq!(
            ui_ps1.artwork_column_stretch_fit,
            ui_ps2.artwork_column_stretch_fit
        );
        assert!(
            (ui_ps1.artwork_column_width_pct - ui_ps2.artwork_column_width_pct).abs() < 1e-4,
            "artwork_column_width_pct"
        );
        assert!(
            (ui_ps1.artwork_auto_max_pct - ui_ps2.artwork_auto_max_pct).abs() < 1e-4,
            "artwork_auto_max_pct"
        );
        assert!(
            (ui_ps1.artwork_vertical_height_pct - ui_ps2.artwork_vertical_height_pct).abs() < 1e-4,
            "artwork_vertical_height_pct"
        );

        // System tray
        assert_eq!(ui_ps1.show_tray_icon, ui_ps2.show_tray_icon);
        assert_eq!(ui_ps1.close_to_tray, ui_ps2.close_to_tray);
    }

    /// Current real-world `config.toml` shape (sanitized: paths/font/etc.
    /// redacted to neutral placeholders). The snapshot guards against
    /// field-rename / default-change regressions that would silently break
    /// existing users' config files.
    ///
    /// Two-stage assertion:
    /// 1. Parse → apply to fresh internal `PlayerSettings` → no panic.
    /// 2. Reserialize the parsed `TomlSettings` and re-parse — both rounds
    ///    must produce equal `TomlSettings` (modulo f32 quantization, which
    ///    is absorbed by going through one parse-reparse cycle then comparing).
    #[test]
    fn current_user_config_toml_snapshot_parses() {
        // Sanitized snapshot of the on-disk [settings] table at the time the
        // sentinel test was authored. Personal values redacted to neutral
        // placeholders (path → /tmp/test_library, font → "Default",
        // start_view → "Albums"). Test exists to catch field-name / serde-
        // attribute regressions, not to mirror the user's literal config.
        const SNAPSHOT_TOML: &str = r#"
albums_artwork_overlay = true
albums_show_index = true
albums_show_love = true
albums_show_plays = false
albums_show_select = false
albums_show_songcount = true
albums_show_stars = false
albums_show_thumbnail = true
artists_artwork_overlay = true
artists_show_albumcount = true
artists_show_index = false
artists_show_love = true
artists_show_plays = false
artists_show_select = false
artists_show_songcount = true
artists_show_stars = false
artists_show_thumbnail = true
artwork_auto_max_pct = 0.7
artwork_column_mode = "auto"
artwork_column_stretch_fit = "fill"
artwork_column_width_pct = 0.2345
artwork_resolution = "original"
artwork_vertical_height_pct = 0.4686
auto_follow_playing = true
close_to_tray = false
crossfade_duration_secs = 10
crossfade_enabled = true
enter_behavior = "play_all"
eq_enabled = true
eq_gains = [
    4.0,
    3.5,
    1.5,
    0.0,
    -1.5,
    -0.5,
    0.5,
    2.0,
    3.5,
    4.0,
]
font_family = "Default"
genres_show_albumcount = true
genres_show_index = false
genres_show_select = false
genres_show_songcount = true
genres_show_thumbnail = true
horizontal_volume = false
library_page_size = "massive"
light_mode = false
local_music_path = "/tmp/test_library"
nav_display_mode = "icons_only"
nav_layout = "none"
normalization_level = "normal"
opacity_gradient = false
playlists_artwork_overlay = true
playlists_show_duration = true
playlists_show_index = true
playlists_show_select = false
playlists_show_songcount = true
playlists_show_thumbnail = true
playlists_show_updatedat = true
queue_show_album = false
queue_show_default_playlist = false
queue_show_duration = true
queue_show_genre = false
queue_show_index = true
queue_show_love = true
queue_show_plays = false
queue_show_select = false
queue_show_stars = false
queue_show_thumbnail = true
quick_add_to_playlist = false
replay_gain_fallback_db = 0.0
replay_gain_fallback_to_agc = false
replay_gain_preamp_db = 0.0
replay_gain_prevent_clipping = true
rounded_mode = true
scrobble_threshold = 0.9
scrobbling_enabled = true
sfx_volume = 0.2253
show_album_artists_only = true
show_tray_icon = true
similar_show_album = true
similar_show_duration = true
similar_show_index = true
similar_show_love = true
similar_show_select = false
similar_show_thumbnail = true
slot_row_height = "compact"
slot_text_links = true
songs_artwork_overlay = true
songs_show_album = false
songs_show_duration = true
songs_show_genre = false
songs_show_index = true
songs_show_love = true
songs_show_plays = false
songs_show_select = false
songs_show_stars = true
songs_show_thumbnail = true
sound_effects_enabled = true
stable_viewport = true
start_view = "Albums"
strip_click_action = "go_to_queue"
strip_merged_mode = true
strip_separator = "slash"
strip_show_album = true
strip_show_artist = true
strip_show_format_info = true
strip_show_labels = true
strip_show_title = true
suppress_library_refresh_toasts = true
track_info_display = "top_bar"
verbose_config = true
visualization_mode = "lines"
volume_normalization_mode = "agc"

[[custom_eq_presets]]
gains = [
    0.0,
    -3.113941192626953,
    8.52574348449707,
    -8.307605743408203,
    5.890437126159668,
    0.0,
    1.5,
    3.0,
    4.0,
    5.0,
]
name = "sentinel preset"
"#;

        // 1. Parse must succeed.
        let ts1: TomlSettings =
            toml::from_str(SNAPSHOT_TOML).expect("parse sanitized config.toml [settings] snapshot");

        // 2. Apply must succeed (no field type mismatches).
        let mut internal = InternalPlayerSettings::default();
        apply_toml_settings_to_internal(&ts1, &mut internal);

        // 3. Reserialize and re-parse — produces a stable `TomlSettings`.
        //    The first round absorbs any f32 → 4-decimal quantization; the
        //    second round must be byte-identical to the first round's value.
        let serialized1 = toml::to_string(&ts1).expect("first serialize");
        let ts2: TomlSettings = toml::from_str(&serialized1).expect("reparse first serialize");
        let serialized2 = toml::to_string(&ts2).expect("second serialize");
        assert_eq!(
            serialized1, serialized2,
            "TomlSettings must reach a stable serialized form after one reparse"
        );

        // Spot-check that key sanitized values landed where expected.
        assert_eq!(ts1.start_view, "Albums");
        assert_eq!(ts1.local_music_path, "/tmp/test_library");
        assert_eq!(ts1.font_family, "Default");
        assert_eq!(ts1.library_page_size, LibraryPageSize::Massive);
        assert_eq!(ts1.volume_normalization, VolumeNormalizationMode::Agc);
        assert_eq!(ts1.visualization_mode, VisualizationMode::Lines);
        assert_eq!(ts1.nav_layout, NavLayout::None);
        assert_eq!(ts1.strip_separator, StripSeparator::Slash);
        assert_eq!(ts1.custom_eq_presets.len(), 1);
        assert_eq!(ts1.custom_eq_presets[0].name, "sentinel preset");
    }

    /// Pin the current `light_mode` write-as-false asymmetry: `TomlSettings::
    /// from_player_settings` takes UI-facing `PlayerSettings`, which has no
    /// `light_mode` field. The function therefore always emits
    /// `light_mode = false` regardless of the internal redb-backed value.
    /// The on-disk truth is maintained separately by the `SetLightModeAtomic`
    /// side-effect handler in the UI crate, which writes `settings.light_mode`
    /// via `update_config_value` (a targeted toml_edit write that doesn't go
    /// through `from_player_settings`).
    ///
    /// This test pins the current behavior. If a future refactor closes the
    /// asymmetry (e.g. by routing through internal-PS instead of UI-PS), this
    /// test should be updated to assert the new behavior.
    #[test]
    fn from_player_settings_writes_light_mode_false_regardless_of_internal_value() {
        let mut internal = InternalPlayerSettings::default();
        internal.light_mode = true;

        let (sm, _tmp) = make_test_manager_with_player(internal);
        let ui_ps = sm.get_player_settings();
        let ts = TomlSettings::from_player_settings(&ui_ps);
        assert!(
            !ts.light_mode,
            "from_player_settings hard-codes light_mode = false (UI-PS lacks the field)"
        );
    }
}
