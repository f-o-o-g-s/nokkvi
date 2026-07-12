use anyhow::Result;

use crate::{
    services::{
        state_storage::StateStorage,
        toml_settings_io::{
            write_all_toml_sections, write_toml_hotkeys, write_toml_settings, write_toml_views,
        },
    },
    types::{
        hotkey_config::{HotkeyAction, HotkeyConfig, KeyCombo},
        player_settings::{
            ArtworkColumnMode, ArtworkResolution, ArtworkStretchFit, BitPerfectMode, EnterBehavior,
            NavDisplayMode, NavLayout, NormalizationLevel, RatingReminderTrigger, RoundedMode,
            SlotRowHeight, StripClickAction, TrackInfoDisplay, VolumeNormalizationMode,
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
    /// In-memory visualizer config, sourced from config.toml `[visualizer]`
    /// ONLY (startup phase 1 + `reload_from_toml`). NEVER serialized to redb —
    /// it is deliberately not part of `UserSettings`, and
    /// `persisted_player_settings_json_has_no_visualizer_key` pins that.
    /// Mirrored wholesale onto `LivePlayerSettings.visualizer` by
    /// `get_player_settings`.
    visualizer: crate::types::visualizer_config::VisualizerConfig,
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
        // Phase 1: Try to read from config.toml (new source of truth) —
        // one file read + one parse for all four sections (review #10).
        let sections =
            crate::services::toml_settings_io::read_all_toml_sections().unwrap_or_else(|e| {
                tracing::warn!("Error reading config.toml: {e}");
                Default::default()
            });
        let toml_settings = sections.settings;
        let toml_hotkeys = sections.hotkeys;
        let toml_views = sections.views;
        let toml_visualizer = sections.visualizer;

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
            visualizer: toml_visualizer.unwrap_or_default(),
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
            visualizer: crate::types::visualizer_config::VisualizerConfig::default(),
            storage,
            skip_toml_writes: true,
        }
    }

    /// Test-only constructor — starts from default `UserSettings` but
    /// overrides the `player` substruct with the caller-supplied value, so
    /// round-trip tests can inject an exhaustive non-default
    /// `PersistedPlayerSettings` without driving every setter individually.
    #[cfg(test)]
    pub(crate) fn for_test_with_player(
        storage: StateStorage,
        player: crate::types::settings::PersistedPlayerSettings,
    ) -> Self {
        let mut settings = UserSettings::default();
        settings.player = player;
        Self {
            settings,
            visualizer: crate::types::visualizer_config::VisualizerConfig::default(),
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
        write_toml_settings(&ts, self.is_verbose_config())
    }

    /// Write [hotkeys] section to config.toml from current internal state.
    fn write_hotkeys_toml(&self) -> Result<()> {
        write_toml_hotkeys(&self.settings.hotkeys, self.is_verbose_config())
    }

    /// Write [views] section to config.toml from current internal state.
    fn write_views_toml(&self) -> Result<()> {
        let tv = TomlViewPreferences::from_all_view_prefs(&self.get_view_preferences());
        write_toml_views(&tv, self.is_verbose_config())
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

    /// Read access to the in-memory visualizer config (config.toml
    /// `[visualizer]`-sourced; never redb).
    pub fn visualizer(&self) -> &crate::types::visualizer_config::VisualizerConfig {
        &self.visualizer
    }

    /// Mutate the in-memory visualizer config under the standard contract:
    /// apply the closure, then re-validate (the same range clamps the legacy
    /// hot-reload applied on read-back). Every Visualizer-table setter routes
    /// through here. Deliberately NO redb write — `[visualizer]` persists in
    /// config.toml only, via the UI's surgical writer.
    pub fn with_visualizer(
        &mut self,
        f: impl FnOnce(&mut crate::types::visualizer_config::VisualizerConfig),
    ) -> Result<()> {
        f(&mut self.visualizer);
        self.visualizer.validate();
        Ok(())
    }

    /// Re-read ONLY the `[visualizer]` section from config.toml into the
    /// in-memory field (absent section resets to defaults, matching
    /// `reload_from_toml`). Used by the reset-defaults flow, whose scope is
    /// the visualizer alone — a full `reload_from_toml` would re-apply every
    /// section and roll back concurrently in-flight, not-yet-flushed
    /// settings.
    pub fn reload_visualizer_from_toml(&mut self) {
        self.visualizer = crate::services::toml_settings_io::read_toml_visualizer()
            .unwrap_or(None)
            .unwrap_or_default();
    }

    /// Hot-reload settings from config.toml and update the in-memory state.
    /// Does NOT save to redb, to prevent feedback loops where a TOML read
    /// triggers a database write. The new values will be propagated to redb
    /// automatically whenever the user next modifies a setting.
    pub fn reload_from_toml(&mut self) {
        // One file read + one parse for all four sections (the per-section
        // readers each re-parse the whole file).
        let sections =
            crate::services::toml_settings_io::read_all_toml_sections().unwrap_or_default();
        if let Some(ts) = sections.settings {
            apply_toml_settings_to_internal(&ts, &mut self.settings.player);
        }
        if let Some(hk) = sections.hotkeys {
            self.settings.hotkeys = hk;
        }
        if let Some(tv) = sections.views {
            self.settings.views = tv.to_all_view_prefs().into();
        }
        // [visualizer] is config.toml-only: a deleted/absent section resets
        // the in-memory config to defaults (matching the legacy
        // load_visualizer_config hot-reload behavior).
        self.visualizer = sections.visualizer.unwrap_or_default();
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

    pub fn set_radio_scrobbling_enabled(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.radio_scrobbling_enabled = enabled;
        self.save()
    }

    pub fn set_radio_scrobble_threshold_secs(&mut self, secs: i64) -> Result<()> {
        use crate::types::settings::{RADIO_SCROBBLE_THRESHOLD_MAX, RADIO_SCROBBLE_THRESHOLD_MIN};
        self.settings.player.radio_scrobble_threshold_secs = secs.clamp(
            i64::from(RADIO_SCROBBLE_THRESHOLD_MIN),
            i64::from(RADIO_SCROBBLE_THRESHOLD_MAX),
        ) as u32;
        self.save()
    }

    pub fn set_radio_now_playing_enabled(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.radio_now_playing_enabled = enabled;
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

    pub fn set_enter_shuffle(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.enter_shuffle = enabled;
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

    pub fn set_rounded_mode(&mut self, mode: RoundedMode) -> Result<()> {
        self.settings.player.rounded_mode = mode;
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

    pub fn set_bit_perfect(&mut self, mode: BitPerfectMode) -> Result<()> {
        self.settings.player.bit_perfect = mode;
        self.save()
    }

    pub fn set_crossfade_duration(&mut self, duration_secs: u32) -> Result<()> {
        use crate::types::player_settings::{
            CROSSFADE_DURATION_MAX_SECS, CROSSFADE_DURATION_MIN_SECS,
        };
        self.settings.player.crossfade_duration_secs =
            duration_secs.clamp(CROSSFADE_DURATION_MIN_SECS, CROSSFADE_DURATION_MAX_SECS);
        self.save()
    }

    pub fn set_crossfade_curve(
        &mut self,
        curve: crate::types::player_settings::CrossfadeCurve,
    ) -> Result<()> {
        self.settings.player.crossfade_curve = curve;
        self.save()
    }

    pub fn set_crossfade_min_track(&mut self, secs: u32) -> Result<()> {
        use crate::types::player_settings::{
            CROSSFADE_MIN_TRACK_MAX_SECS, CROSSFADE_MIN_TRACK_MIN_SECS,
        };
        self.settings.player.crossfade_min_track_secs =
            secs.clamp(CROSSFADE_MIN_TRACK_MIN_SECS, CROSSFADE_MIN_TRACK_MAX_SECS);
        self.save()
    }

    pub fn set_crossfade_album_gapless(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.crossfade_album_gapless = enabled;
        self.save()
    }

    pub fn set_smooth_track_starts(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.smooth_track_starts = enabled;
        self.save()
    }

    pub fn set_fade_on_pause(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.fade_on_pause = enabled;
        self.save()
    }

    pub fn set_fade_pause_ms(&mut self, ms: u32) -> Result<()> {
        use crate::types::player_settings::{TRANSPORT_FADE_MS_MAX, TRANSPORT_FADE_MS_MIN};
        self.settings.player.fade_pause_ms = ms.clamp(TRANSPORT_FADE_MS_MIN, TRANSPORT_FADE_MS_MAX);
        self.save()
    }

    pub fn set_fade_on_stop(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.fade_on_stop = enabled;
        self.save()
    }

    pub fn set_fade_stop_ms(&mut self, ms: u32) -> Result<()> {
        use crate::types::player_settings::{TRANSPORT_FADE_MS_MAX, TRANSPORT_FADE_MS_MIN};
        self.settings.player.fade_stop_ms = ms.clamp(TRANSPORT_FADE_MS_MIN, TRANSPORT_FADE_MS_MAX);
        self.save()
    }

    pub fn set_fade_radio_transitions(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.fade_radio_transitions = enabled;
        self.save()
    }

    pub fn set_fade_on_skip(
        &mut self,
        mode: crate::types::player_settings::FadeOnSkip,
    ) -> Result<()> {
        self.settings.player.fade_on_skip = mode;
        self.save()
    }

    pub fn set_fade_skip_secs(&mut self, secs: u32) -> Result<()> {
        use crate::types::player_settings::{FADE_SKIP_SECS_MAX, FADE_SKIP_SECS_MIN};
        self.settings.player.fade_skip_secs = secs.clamp(FADE_SKIP_SECS_MIN, FADE_SKIP_SECS_MAX);
        self.save()
    }

    pub fn set_skip_silence(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.skip_silence = enabled;
        self.save()
    }

    pub fn set_crossfade_offset_secs(&mut self, secs: i32) -> Result<()> {
        use crate::types::player_settings::{CROSSFADE_OFFSET_MAX_SECS, CROSSFADE_OFFSET_MIN_SECS};
        self.settings.player.crossfade_offset_secs =
            secs.clamp(CROSSFADE_OFFSET_MIN_SECS, CROSSFADE_OFFSET_MAX_SECS);
        self.save()
    }

    pub fn set_crossfade_bar_snap(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.crossfade_bar_snap = enabled;
        self.save()
    }

    pub fn set_rating_reminder_enabled(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.rating_reminder_enabled = enabled;
        self.save()
    }

    pub fn set_rating_change_notification_enabled(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.rating_change_notification_enabled = enabled;
        self.save()
    }

    pub fn set_love_change_notification_enabled(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.love_change_notification_enabled = enabled;
        self.save()
    }

    pub fn set_rating_reminder_trigger(&mut self, trigger: RatingReminderTrigger) -> Result<()> {
        self.settings.player.rating_reminder_trigger = trigger;
        self.save()
    }

    pub fn set_rating_reminder_percent(&mut self, percent: u32) -> Result<()> {
        self.settings.player.rating_reminder_percent = percent.clamp(60, 90);
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

    pub fn set_rewind_on_previous(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.rewind_on_previous = enabled;
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

    pub fn set_autohide_toolbar(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.autohide_toolbar = enabled;
        self.save()
    }

    pub fn set_autohide_toolbar_height(&mut self, px: u32) -> Result<()> {
        self.settings.player.autohide_toolbar_height = px.clamp(4, 24);
        self.save()
    }

    pub fn set_autohide_toolbar_grip(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.autohide_toolbar_grip = enabled;
        self.save()
    }

    pub fn set_autohide_collapsed_appearance(
        &mut self,
        mode: crate::types::player_settings::CollapsedAppearance,
    ) -> Result<()> {
        self.settings.player.autohide_collapsed_appearance = mode;
        self.save()
    }

    pub fn set_scrollbar_visibility(
        &mut self,
        mode: crate::types::player_settings::ScrollbarVisibility,
    ) -> Result<()> {
        self.settings.player.scrollbar_visibility = mode;
        self.save()
    }

    pub fn set_icon_set(&mut self, set: crate::types::player_settings::IconSet) -> Result<()> {
        self.settings.player.icon_set = set;
        self.save()
    }

    pub fn set_mini_player_show_volume(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.mini_player_show_volume = enabled;
        self.save()
    }

    pub fn set_mini_player_show_modes(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.mini_player_show_modes = enabled;
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
        self.settings.player.view_columns.queue_show_stars = enabled;
        self.save()
    }

    pub fn set_queue_show_album(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.queue_show_album = enabled;
        self.save()
    }

    pub fn set_queue_show_duration(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.queue_show_duration = enabled;
        self.save()
    }

    pub fn set_queue_show_love(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.queue_show_love = enabled;
        self.save()
    }

    pub fn set_queue_show_plays(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.queue_show_plays = enabled;
        self.save()
    }

    pub fn set_queue_show_index(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.queue_show_index = enabled;
        self.save()
    }

    pub fn set_queue_show_thumbnail(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.queue_show_thumbnail = enabled;
        self.save()
    }

    pub fn set_queue_show_genre(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.queue_show_genre = enabled;
        self.save()
    }

    pub fn set_queue_show_select(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.queue_show_select = enabled;
        self.save()
    }

    pub fn set_albums_show_stars(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.albums_show_stars = enabled;
        self.save()
    }

    pub fn set_albums_show_songcount(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.albums_show_songcount = enabled;
        self.save()
    }

    pub fn set_albums_show_plays(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.albums_show_plays = enabled;
        self.save()
    }

    pub fn set_albums_show_love(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.albums_show_love = enabled;
        self.save()
    }

    pub fn set_albums_show_index(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.albums_show_index = enabled;
        self.save()
    }

    pub fn set_albums_show_thumbnail(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.albums_show_thumbnail = enabled;
        self.save()
    }

    pub fn set_albums_show_select(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.albums_show_select = enabled;
        self.save()
    }

    pub fn set_songs_show_stars(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.songs_show_stars = enabled;
        self.save()
    }

    pub fn set_songs_show_album(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.songs_show_album = enabled;
        self.save()
    }

    pub fn set_songs_show_duration(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.songs_show_duration = enabled;
        self.save()
    }

    pub fn set_songs_show_plays(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.songs_show_plays = enabled;
        self.save()
    }

    pub fn set_songs_show_love(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.songs_show_love = enabled;
        self.save()
    }

    pub fn set_songs_show_index(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.songs_show_index = enabled;
        self.save()
    }

    pub fn set_songs_show_thumbnail(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.songs_show_thumbnail = enabled;
        self.save()
    }

    pub fn set_songs_show_genre(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.songs_show_genre = enabled;
        self.save()
    }

    pub fn set_songs_show_select(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.songs_show_select = enabled;
        self.save()
    }

    pub fn set_artists_show_stars(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.artists_show_stars = enabled;
        self.save()
    }

    pub fn set_artists_show_albumcount(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.artists_show_albumcount = enabled;
        self.save()
    }

    pub fn set_artists_show_songcount(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.artists_show_songcount = enabled;
        self.save()
    }

    pub fn set_artists_show_plays(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.artists_show_plays = enabled;
        self.save()
    }

    pub fn set_artists_show_love(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.artists_show_love = enabled;
        self.save()
    }

    pub fn set_artists_show_index(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.artists_show_index = enabled;
        self.save()
    }

    pub fn set_artists_show_thumbnail(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.artists_show_thumbnail = enabled;
        self.save()
    }

    pub fn set_artists_show_select(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.artists_show_select = enabled;
        self.save()
    }

    pub fn set_genres_show_index(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.genres_show_index = enabled;
        self.save()
    }

    pub fn set_genres_show_thumbnail(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.genres_show_thumbnail = enabled;
        self.save()
    }

    pub fn set_genres_show_albumcount(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.genres_show_albumcount = enabled;
        self.save()
    }

    pub fn set_genres_show_songcount(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.genres_show_songcount = enabled;
        self.save()
    }

    pub fn set_genres_show_select(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.genres_show_select = enabled;
        self.save()
    }

    pub fn set_playlists_show_index(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.playlists_show_index = enabled;
        self.save()
    }

    pub fn set_playlists_show_thumbnail(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.playlists_show_thumbnail = enabled;
        self.save()
    }

    pub fn set_playlists_show_songcount(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.playlists_show_songcount = enabled;
        self.save()
    }

    pub fn set_playlists_show_duration(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.playlists_show_duration = enabled;
        self.save()
    }

    pub fn set_playlists_show_updatedat(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.playlists_show_updatedat = enabled;
        self.save()
    }

    pub fn set_playlists_show_select(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.playlists_show_select = enabled;
        self.save()
    }

    pub fn set_similar_show_index(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.similar_show_index = enabled;
        self.save()
    }

    pub fn set_similar_show_thumbnail(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.similar_show_thumbnail = enabled;
        self.save()
    }

    pub fn set_similar_show_album(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.similar_show_album = enabled;
        self.save()
    }

    pub fn set_similar_show_duration(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.similar_show_duration = enabled;
        self.save()
    }

    pub fn set_similar_show_love(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.similar_show_love = enabled;
        self.save()
    }

    pub fn set_similar_show_select(&mut self, enabled: bool) -> Result<()> {
        self.settings.player.view_columns.similar_show_select = enabled;
        self.save()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_active_playlist(
        &mut self,
        id: Option<String>,
        name: String,
        comment: String,
        duration: f32,
        updated: String,
        public: bool,
        song_count: u32,
    ) -> Result<()> {
        self.settings.player.active_playlist_id = id;
        self.settings.player.active_playlist_name = name;
        self.settings.player.active_playlist_comment = comment;
        self.settings.player.active_playlist_duration = duration;
        self.settings.player.active_playlist_updated = updated;
        self.settings.player.active_playlist_public = public;
        self.settings.player.active_playlist_song_count = song_count;
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

    pub fn set_verbose_config(
        &mut self,
        mode: crate::types::player_settings::VerboseConfig,
    ) -> Result<()> {
        self.settings.player.verbose_config = mode;
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

    /// Whether the TOML section writers should emit every key (including
    /// unchanged defaults) rather than pruning to the non-default set. True
    /// only for `VerboseConfig::On`; both `Off` and `Clean` write sparse.
    pub(crate) fn is_verbose_config(&self) -> bool {
        self.settings.player.verbose_config.writes_all_defaults()
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
    /// Composition: start from default UI-facing `LivePlayerSettings`,
    /// populate the runtime-only fields (volume, playlist IDs that don't
    /// round-trip through `config.toml`), then run the 3 per-tab dumpers
    /// (whose `read:`/copy-only closures cover every user-facing field)
    /// plus the consolidated view-columns dumper.
    pub fn get_player_settings(&self) -> crate::types::player_settings::LivePlayerSettings {
        let p = &self.settings.player;
        let mut out = crate::types::player_settings::LivePlayerSettings {
            // Runtime-only fields — these live in redb only and never
            // round-trip through config.toml. The dumpers below intentionally
            // do NOT touch these.
            volume: p.volume as f32,
            default_playlist_id: p.default_playlist_id.clone(),
            default_playlist_name: p.default_playlist_name.clone(),
            active_playlist_id: p.active_playlist_id.clone(),
            active_playlist_name: p.active_playlist_name.clone(),
            active_playlist_comment: p.active_playlist_comment.clone(),
            active_playlist_duration: p.active_playlist_duration,
            active_playlist_updated: p.active_playlist_updated.clone(),
            active_playlist_public: p.active_playlist_public,
            active_playlist_song_count: p.active_playlist_song_count,

            // In-memory visualizer config (config.toml `[visualizer]`-only;
            // never redb) — mirrored wholesale, not via a dumper.
            visualizer: self.visualizer.clone(),

            ..Default::default()
        };

        // Per-tab macro-emitted dumpers (define_settings! `read:` closures).
        crate::services::settings_tables::dump_general_tab_player_settings(p, &mut out);
        crate::services::settings_tables::dump_interface_tab_player_settings(p, &mut out);
        crate::services::settings_tables::dump_playback_tab_player_settings(p, &mut out);

        // Consolidated view-column dumper (define_settings! `view_columns:`
        // clause) — all 50 column booleans across the 7 slot-list views.
        crate::services::settings_tables::dump_columns_tab_player_settings(p, &mut out);

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

/// Apply TOML settings values onto the redb-backed
/// `PersistedPlayerSettings` struct.
///
/// Only overwrites user-facing preference fields — volume, playlist IDs, and
/// other runtime state are left untouched.
///
/// Composition: 3 per-tab `apply_toml_<tab>` macro-emitted helpers cover
/// every `define_settings!` entry (dispatchable and copy-only alike); the
/// consolidated `apply_toml_columns_tab` covers every column-toggle bool —
/// including `queue_show_genre` and `songs_show_genre` that the pre-macro
/// hand-written body silently dropped.
fn apply_toml_settings_to_internal(
    ts: &TomlSettings,
    p: &mut crate::types::settings::PersistedPlayerSettings,
) {
    // Per-tab macro-emitted appliers (define_settings! `toml_apply:` closures).
    crate::services::settings_tables::apply_toml_general_tab(ts, p);
    crate::services::settings_tables::apply_toml_interface_tab(ts, p);
    crate::services::settings_tables::apply_toml_playback_tab(ts, p);

    // Consolidated view-column applier (define_settings! `view_columns:`
    // clause) — all 50 column booleans, including queue_show_genre /
    // songs_show_genre whose silent drop the original hand-written body
    // caused (pinned by queue_and_songs_genre_columns_apply_correctly).
    crate::services::settings_tables::apply_toml_columns_tab(ts, p);
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
// Sentinel round-trip tests (Group G Phase 2 — PersistedPlayerSettings TOML compat)
// =============================================================================
//
// These tests pin the `PersistedPlayerSettings → TomlSettings → bytes →
// TomlSettings → PersistedPlayerSettings` round-trip semantics that the
// on-disk config.toml contract depends on. They guard subsequent commits
// that extend `define_view_columns!` and `define_settings!` to collapse the
// ~238 lines of hand-written field copies across `from_player_settings`,
// `get_player_settings`, and `apply_toml_settings_to_internal`.
//
// `build_exhaustive_persisted_player_settings()` deliberately lists every
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
                ArtworkColumnMode, ArtworkResolution, ArtworkStretchFit, CrossfadeCurve,
                EnterBehavior, LibraryPageSize, NavDisplayMode, NavLayout, NormalizationLevel,
                RoundedMode, SlotRowHeight, StripClickAction, StripSeparator, TrackInfoDisplay,
                VisualizationMode, VolumeNormalizationMode,
            },
            settings::PersistedPlayerSettings,
            toml_settings::TomlSettings,
        },
    };

    fn make_test_manager_with_player(
        player: PersistedPlayerSettings,
    ) -> (SettingsManager, TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("test_settings.redb");
        let storage = StateStorage::new(path).expect("StateStorage::new");
        (SettingsManager::for_test_with_player(storage, player), tmp)
    }

    /// Exhaustive `PersistedPlayerSettings` with every persisted field set to
    /// a non-default sentinel. Listed without `..Default::default()` so a new
    /// field addition surfaces here as a missing-initializer compile error.
    ///
    /// Sentinels are chosen so that `field == Default::default()` is false
    /// for every field: bools flipped from their default; enums set to a
    /// non-`#[default]` variant; numerics offset; strings made non-empty.
    fn build_exhaustive_persisted_player_settings() -> PersistedPlayerSettings {
        PersistedPlayerSettings {
            // Runtime-only fields (excluded from the round-trip assertion):
            volume: 0.42,
            default_playlist_id: Some("playlist-42".to_string()),
            default_playlist_name: "My Default Playlist".to_string(),
            active_playlist_id: Some("playlist-99".to_string()),
            active_playlist_name: "Active Playlist".to_string(),
            active_playlist_comment: "comment text".to_string(),
            active_playlist_duration: 1234.5,
            active_playlist_updated: "2026-05-27T20:19:59-06:00".to_string(),
            active_playlist_public: true,
            active_playlist_song_count: 29,

            // Audio knobs
            sfx_volume: 0.3142,
            sound_effects_enabled: false,                 // default true
            visualization_mode: VisualizationMode::Lines, // default Bars
            light_mode: true, // default false; UI-PS lacks this field — excluded from the round-trip
            scrobbling_enabled: false, // default true
            scrobble_threshold: 0.8123, // default 0.50
            radio_scrobbling_enabled: true, // default false
            radio_scrobble_threshold_secs: 90, // default 60
            radio_now_playing_enabled: false, // default true

            // General
            start_view: "Albums".to_string(), // default "Harbour"
            stable_viewport: false,           // default true
            auto_follow_playing: false,       // default true
            enter_behavior: EnterBehavior::AppendAndPlay, // default PlayAll
            enter_shuffle: true,              // default false
            local_music_path: "/tmp/sentinel/music".to_string(),
            rounded_mode: RoundedMode::PlayerOnly, // default On
            nav_layout: NavLayout::Side,           // default Top
            nav_display_mode: NavDisplayMode::IconsOnly, // default TextOnly
            track_info_display: TrackInfoDisplay::TopBar, // default Mini Player
            slot_row_height: SlotRowHeight::Spacious, // default Default
            opacity_gradient: false,               // default true
            slot_text_links: true,                 // default false
            scrollbar_visibility: crate::types::player_settings::ScrollbarVisibility::Hidden, // default Always
            icon_set: crate::types::player_settings::IconSet::Lucide, // default Phosphor

            // Playback / crossfade
            crossfade_enabled: true,                 // default false
            bit_perfect: BitPerfectMode::Strict,     // default Off
            crossfade_duration_secs: 9,              // default 5
            crossfade_curve: CrossfadeCurve::Linear, // default EqualPower
            crossfade_min_track_secs: 25,            // default 10
            crossfade_album_gapless: true,           // default false
            smooth_track_starts: false,              // default true
            fade_on_pause: true,                     // default false
            fade_pause_ms: 250,                      // default 100
            fade_on_stop: true,                      // default false
            fade_stop_ms: 350,                       // default 100
            fade_radio_transitions: true,            // default false
            fade_on_skip: crate::types::player_settings::FadeOnSkip::Crossfade, // default Off
            fade_skip_secs: 3,                       // default 2
            skip_silence: true,                      // default false
            crossfade_offset_secs: -2,               // default 0
            crossfade_bar_snap: true,                // default false
            rewind_on_previous: true,                // default false

            // Playlists
            quick_add_to_playlist: true,       // default false
            queue_show_default_playlist: true, // default false
            horizontal_volume: true,           // default false
            autohide_toolbar: false,           // default true
            autohide_toolbar_height: 12,       // default 4
            autohide_toolbar_grip: false,      // default true
            autohide_collapsed_appearance:
                crate::types::player_settings::CollapsedAppearance::Hidden, // default Count strip
            mini_player_show_volume: false,    // default true
            mini_player_show_modes: false,     // default true
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
            strip_separator: StripSeparator::Dot, // default Slash

            // EQ
            eq_enabled: true, // default false
            eq_gains: [4.0, 3.5, 1.5, 0.0, -1.5, -0.5, 0.5, 2.0, 3.5, 4.0],
            custom_eq_presets: vec![CustomEqPreset {
                name: "Sentinel Preset".to_string(),
                gains: [0.0, -3.5, 8.5, -8.25, 5.875, 0.0, 1.5, 3.0, 4.0, 5.0],
            }],

            // Verbose / library
            verbose_config: crate::types::player_settings::VerboseConfig::On, // default Off
            library_page_size: LibraryPageSize::Massive,                      // default Default
            artwork_resolution: ArtworkResolution::Original,                  // default Default
            show_album_artists_only: false,                                   // default true
            suppress_library_refresh_toasts: true,                            // default false

            // Per-view columns — nested exhaustive literal (no
            // `..Default::default()`) so a new column added to `ViewColumns`
            // surfaces here as a missing-initializer compile error too.
            view_columns: crate::types::view_columns::ViewColumns {
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
            },

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

            // Rating reminder
            rating_reminder_enabled: true,            // default false
            rating_change_notification_enabled: true, // default false
            love_change_notification_enabled: true,   // default false
            rating_reminder_trigger: RatingReminderTrigger::PercentagePlayed, // default OnScrobble
            rating_reminder_percent: 85,              // default 75
        }
    }

    /// Full-field round-trip: build exhaustive `PersistedPlayerSettings`, dump
    /// to UI-facing `LivePlayerSettings`, convert to `TomlSettings`,
    /// serialize, deserialize, apply back onto a fresh
    /// `PersistedPlayerSettings`, dump again — and confirm every persisted
    /// field survives. The 6 f32 fields routed through `round_f32` /
    /// `round_f32_array` use 1e-4 tolerance (they are quantized to 4 decimals
    /// on TOML emit).
    #[test]
    fn player_settings_toml_roundtrip_full_field_coverage() {
        let internal_src = build_exhaustive_persisted_player_settings();

        // Stamp the exhaustive sentinel onto a SettingsManager and dump.
        let (sm, _tmp) = make_test_manager_with_player(internal_src.clone());
        let ui_ps1 = sm.get_player_settings();

        // UI → TOML → bytes → TOML.
        // The seedable `_with_existing(.., None)` variant keeps the round-trip
        // hermetic: the no-arg entry point reads `[settings].light_mode` off
        // the real on-disk config (see the light_mode no-leak test below).
        let ts1 = TomlSettings::from_player_settings_with_existing(&ui_ps1, None);
        let serialized = toml::to_string(&ts1).expect("serialize TomlSettings");
        let ts2: TomlSettings = toml::from_str(&serialized).expect("deserialize TomlSettings");

        // Apply onto a fresh `PersistedPlayerSettings`, then dump again via a
        // fresh manager — this exercises the same get_player_settings flow as
        // production startup.
        let mut internal_dst = PersistedPlayerSettings::default();
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
        assert_eq!(ui_ps1.scrollbar_visibility, ui_ps2.scrollbar_visibility);
        assert_eq!(ui_ps1.icon_set, ui_ps2.icon_set);

        // Playback / crossfade
        assert_eq!(ui_ps1.crossfade_enabled, ui_ps2.crossfade_enabled);
        assert_eq!(
            ui_ps1.crossfade_duration_secs,
            ui_ps2.crossfade_duration_secs
        );
        assert_eq!(ui_ps1.crossfade_curve, ui_ps2.crossfade_curve);
        assert_eq!(
            ui_ps1.crossfade_min_track_secs,
            ui_ps2.crossfade_min_track_secs
        );
        assert_eq!(
            ui_ps1.crossfade_album_gapless,
            ui_ps2.crossfade_album_gapless
        );

        // Transport fades / smooth starts (M5)
        assert_eq!(ui_ps1.smooth_track_starts, ui_ps2.smooth_track_starts);
        assert_eq!(ui_ps1.fade_on_pause, ui_ps2.fade_on_pause);
        assert_eq!(ui_ps1.fade_pause_ms, ui_ps2.fade_pause_ms);
        assert_eq!(ui_ps1.fade_on_stop, ui_ps2.fade_on_stop);
        assert_eq!(ui_ps1.fade_stop_ms, ui_ps2.fade_stop_ms);
        assert_eq!(ui_ps1.fade_radio_transitions, ui_ps2.fade_radio_transitions);
        assert_eq!(ui_ps1.fade_on_skip, ui_ps2.fade_on_skip);
        assert_eq!(ui_ps1.fade_skip_secs, ui_ps2.fade_skip_secs);

        // Content-aware overlap (M8)
        assert_eq!(ui_ps1.skip_silence, ui_ps2.skip_silence);
        assert_eq!(ui_ps1.crossfade_offset_secs, ui_ps2.crossfade_offset_secs);
        assert_eq!(ui_ps1.crossfade_bar_snap, ui_ps2.crossfade_bar_snap);

        // Rating reminder
        assert_eq!(
            ui_ps1.rating_reminder_enabled,
            ui_ps2.rating_reminder_enabled
        );
        assert_eq!(
            ui_ps1.rating_change_notification_enabled,
            ui_ps2.rating_change_notification_enabled
        );
        assert_eq!(
            ui_ps1.love_change_notification_enabled,
            ui_ps2.love_change_notification_enabled
        );
        assert_eq!(
            ui_ps1.rating_reminder_trigger,
            ui_ps2.rating_reminder_trigger
        );
        assert_eq!(
            ui_ps1.rating_reminder_percent,
            ui_ps2.rating_reminder_percent
        );

        // Playlists
        assert_eq!(ui_ps1.quick_add_to_playlist, ui_ps2.quick_add_to_playlist);
        assert_eq!(
            ui_ps1.queue_show_default_playlist,
            ui_ps2.queue_show_default_playlist
        );
        assert_eq!(ui_ps1.horizontal_volume, ui_ps2.horizontal_volume);
        assert_eq!(ui_ps1.autohide_toolbar, ui_ps2.autohide_toolbar);
        assert_eq!(
            ui_ps1.autohide_toolbar_height,
            ui_ps2.autohide_toolbar_height
        );
        assert_eq!(ui_ps1.autohide_toolbar_grip, ui_ps2.autohide_toolbar_grip);
        assert_eq!(
            ui_ps1.autohide_collapsed_appearance,
            ui_ps2.autohide_collapsed_appearance
        );
        assert_eq!(
            ui_ps1.mini_player_show_volume,
            ui_ps2.mini_player_show_volume
        );
        assert_eq!(ui_ps1.mini_player_show_modes, ui_ps2.mini_player_show_modes);
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
        assert_eq!(
            ui_ps1.view_columns.queue_show_stars,
            ui_ps2.view_columns.queue_show_stars
        );
        assert_eq!(
            ui_ps1.view_columns.queue_show_album,
            ui_ps2.view_columns.queue_show_album
        );
        assert_eq!(
            ui_ps1.view_columns.queue_show_duration,
            ui_ps2.view_columns.queue_show_duration
        );
        assert_eq!(
            ui_ps1.view_columns.queue_show_love,
            ui_ps2.view_columns.queue_show_love
        );
        assert_eq!(
            ui_ps1.view_columns.queue_show_plays,
            ui_ps2.view_columns.queue_show_plays
        );
        assert_eq!(
            ui_ps1.view_columns.queue_show_index,
            ui_ps2.view_columns.queue_show_index
        );
        assert_eq!(
            ui_ps1.view_columns.queue_show_thumbnail,
            ui_ps2.view_columns.queue_show_thumbnail
        );
        // queue_show_genre was silently dropped on apply prior to commit 5
        // (the field ships in TomlSettings with serde wiring but was missing
        // from both the hand-written apply body and every per-tab macro
        // entry). Commit 5's macro-helper collapse picks it up through the
        // queue-columns helpers, closing the silent-drop bug. This assertion
        // is the fold-in: it now requires a real round-trip rather than
        // pinning the buggy default-snap-back behavior.
        assert!(
            ui_ps1.view_columns.queue_show_genre,
            "sentinel sets queue_show_genre=true"
        );
        assert_eq!(
            ui_ps1.view_columns.queue_show_genre, ui_ps2.view_columns.queue_show_genre,
            "queue_show_genre must round-trip through TOML→internal apply",
        );
        assert_eq!(
            ui_ps1.view_columns.queue_show_select,
            ui_ps2.view_columns.queue_show_select
        );

        // Albums columns
        assert_eq!(
            ui_ps1.view_columns.albums_show_stars,
            ui_ps2.view_columns.albums_show_stars
        );
        assert_eq!(
            ui_ps1.view_columns.albums_show_songcount,
            ui_ps2.view_columns.albums_show_songcount
        );
        assert_eq!(
            ui_ps1.view_columns.albums_show_plays,
            ui_ps2.view_columns.albums_show_plays
        );
        assert_eq!(
            ui_ps1.view_columns.albums_show_love,
            ui_ps2.view_columns.albums_show_love
        );
        assert_eq!(
            ui_ps1.view_columns.albums_show_index,
            ui_ps2.view_columns.albums_show_index
        );
        assert_eq!(
            ui_ps1.view_columns.albums_show_thumbnail,
            ui_ps2.view_columns.albums_show_thumbnail
        );
        assert_eq!(
            ui_ps1.view_columns.albums_show_select,
            ui_ps2.view_columns.albums_show_select
        );

        // Songs columns
        assert_eq!(
            ui_ps1.view_columns.songs_show_stars,
            ui_ps2.view_columns.songs_show_stars
        );
        assert_eq!(
            ui_ps1.view_columns.songs_show_album,
            ui_ps2.view_columns.songs_show_album
        );
        assert_eq!(
            ui_ps1.view_columns.songs_show_duration,
            ui_ps2.view_columns.songs_show_duration
        );
        assert_eq!(
            ui_ps1.view_columns.songs_show_plays,
            ui_ps2.view_columns.songs_show_plays
        );
        assert_eq!(
            ui_ps1.view_columns.songs_show_love,
            ui_ps2.view_columns.songs_show_love
        );
        assert_eq!(
            ui_ps1.view_columns.songs_show_index,
            ui_ps2.view_columns.songs_show_index
        );
        assert_eq!(
            ui_ps1.view_columns.songs_show_thumbnail,
            ui_ps2.view_columns.songs_show_thumbnail
        );
        // songs_show_genre was silently dropped on apply prior to commit 5
        // (same shape as queue_show_genre — the field ships in TomlSettings
        // with serde wiring but was missing from both the hand-written apply
        // body and every per-tab macro entry). The macro-helper collapse
        // picks it up through the songs-columns helpers. Fold-in: real
        // round-trip assertion instead of the buggy-default pin.
        assert!(
            ui_ps1.view_columns.songs_show_genre,
            "sentinel sets songs_show_genre=true"
        );
        assert_eq!(
            ui_ps1.view_columns.songs_show_genre, ui_ps2.view_columns.songs_show_genre,
            "songs_show_genre must round-trip through TOML→internal apply",
        );
        assert_eq!(
            ui_ps1.view_columns.songs_show_select,
            ui_ps2.view_columns.songs_show_select
        );

        // Artists columns
        assert_eq!(
            ui_ps1.view_columns.artists_show_stars,
            ui_ps2.view_columns.artists_show_stars
        );
        assert_eq!(
            ui_ps1.view_columns.artists_show_albumcount,
            ui_ps2.view_columns.artists_show_albumcount
        );
        assert_eq!(
            ui_ps1.view_columns.artists_show_songcount,
            ui_ps2.view_columns.artists_show_songcount
        );
        assert_eq!(
            ui_ps1.view_columns.artists_show_plays,
            ui_ps2.view_columns.artists_show_plays
        );
        assert_eq!(
            ui_ps1.view_columns.artists_show_love,
            ui_ps2.view_columns.artists_show_love
        );
        assert_eq!(
            ui_ps1.view_columns.artists_show_index,
            ui_ps2.view_columns.artists_show_index
        );
        assert_eq!(
            ui_ps1.view_columns.artists_show_thumbnail,
            ui_ps2.view_columns.artists_show_thumbnail
        );
        assert_eq!(
            ui_ps1.view_columns.artists_show_select,
            ui_ps2.view_columns.artists_show_select
        );

        // Genres columns
        assert_eq!(
            ui_ps1.view_columns.genres_show_index,
            ui_ps2.view_columns.genres_show_index
        );
        assert_eq!(
            ui_ps1.view_columns.genres_show_thumbnail,
            ui_ps2.view_columns.genres_show_thumbnail
        );
        assert_eq!(
            ui_ps1.view_columns.genres_show_albumcount,
            ui_ps2.view_columns.genres_show_albumcount
        );
        assert_eq!(
            ui_ps1.view_columns.genres_show_songcount,
            ui_ps2.view_columns.genres_show_songcount
        );
        assert_eq!(
            ui_ps1.view_columns.genres_show_select,
            ui_ps2.view_columns.genres_show_select
        );

        // Playlists columns
        assert_eq!(
            ui_ps1.view_columns.playlists_show_index,
            ui_ps2.view_columns.playlists_show_index
        );
        assert_eq!(
            ui_ps1.view_columns.playlists_show_thumbnail,
            ui_ps2.view_columns.playlists_show_thumbnail
        );
        assert_eq!(
            ui_ps1.view_columns.playlists_show_songcount,
            ui_ps2.view_columns.playlists_show_songcount
        );
        assert_eq!(
            ui_ps1.view_columns.playlists_show_duration,
            ui_ps2.view_columns.playlists_show_duration
        );
        assert_eq!(
            ui_ps1.view_columns.playlists_show_updatedat,
            ui_ps2.view_columns.playlists_show_updatedat
        );
        assert_eq!(
            ui_ps1.view_columns.playlists_show_select,
            ui_ps2.view_columns.playlists_show_select
        );

        // Similar columns
        assert_eq!(
            ui_ps1.view_columns.similar_show_index,
            ui_ps2.view_columns.similar_show_index
        );
        assert_eq!(
            ui_ps1.view_columns.similar_show_thumbnail,
            ui_ps2.view_columns.similar_show_thumbnail
        );
        assert_eq!(
            ui_ps1.view_columns.similar_show_album,
            ui_ps2.view_columns.similar_show_album
        );
        assert_eq!(
            ui_ps1.view_columns.similar_show_duration,
            ui_ps2.view_columns.similar_show_duration
        );
        assert_eq!(
            ui_ps1.view_columns.similar_show_love,
            ui_ps2.view_columns.similar_show_love
        );
        assert_eq!(
            ui_ps1.view_columns.similar_show_select,
            ui_ps2.view_columns.similar_show_select
        );

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
    /// 1. Parse → apply to fresh `PersistedPlayerSettings` → no panic.
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
mini_player_show_modes = true
mini_player_show_volume = true
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
        let mut internal = PersistedPlayerSettings::default();
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

    /// The UI-facing `LivePlayerSettings` carries no `light_mode` field (it
    /// lives on a theme atomic + `config.toml`, not on `LivePlayerSettings`),
    /// so an internal redb-backed `light_mode = true` must NOT leak through
    /// the `LivePlayerSettings -> TomlSettings` conversion.
    ///
    /// We assert that through `from_player_settings_with_existing(.., None)`:
    /// with no on-disk override, `light_mode` stays at its
    /// `TomlSettings::default()` value (`false`) even though the source redb
    /// value was `true`. The `_with_existing` variant is used DELIBERATELY —
    /// the no-arg `from_player_settings` reads `[settings].light_mode` off the
    /// real `config.toml` (`read_toml_settings`) to preserve it across
    /// whole-section writes, so calling it from a test makes the assertion
    /// depend on the shared on-disk config and flake under parallel
    /// `cargo test` (it reads whatever a sibling test last wrote there). The
    /// on-disk truth is owned by the `SetLightModeAtomic` side-effect handler
    /// in the UI crate (a targeted `update_config_value` write).
    #[test]
    fn from_player_settings_writes_light_mode_false_regardless_of_internal_value() {
        let mut internal = PersistedPlayerSettings::default();
        internal.light_mode = true;

        let (sm, _tmp) = make_test_manager_with_player(internal);
        let ui_ps = sm.get_player_settings();
        let ts = TomlSettings::from_player_settings_with_existing(&ui_ps, None);
        assert!(
            !ts.light_mode,
            "internal redb light_mode=true must not leak through UI-PS (no on-disk override)"
        );
    }

    /// Pin the persistence invariant for `PersistedPlayerSettings`: the
    /// rename in Group 3.5 Lane 1 is byte-stable because `state_storage::
    /// StateStorage::save` writes `serde_json::to_vec(&UserSettings)` and
    /// serde_json keys by field name, never by struct name. This test
    /// exercises the production save → reopen → load path with a
    /// non-default sentinel and asserts every persisted field survives.
    ///
    /// If a future change reroutes `UserSettings` persistence through
    /// `save_binary` (bincode-next) instead of `save` (serde_json), bincode
    /// IS sensitive to struct names — this test will then need to be paired
    /// with a migration. Today the path is JSON, so the rename is safe.
    #[test]
    fn persisted_player_settings_redb_roundtrip() {
        use crate::types::settings::UserSettings;

        let sentinel = build_exhaustive_persisted_player_settings();
        let user = UserSettings {
            player: sentinel.clone(),
            ..UserSettings::default()
        };

        // Save through a real StateStorage, reopen, load. Mirrors the
        // production startup path in SettingsManager::new() at line 81.
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("persisted_roundtrip.redb");
        {
            let storage = StateStorage::new(path.clone()).expect("create redb");
            storage
                .save(crate::services::storage_keys::USER_SETTINGS, &user)
                .expect("save UserSettings");
        }
        let storage = StateStorage::new(path).expect("reopen redb");
        let loaded: UserSettings = storage
            .load(crate::services::storage_keys::USER_SETTINGS)
            .expect("load result")
            .expect("UserSettings present after save");

        let lhs = &user.player;
        let rhs = &loaded.player;

        // Spot-check fields drawn from every "section" of the exhaustive
        // sentinel — covers bools, strings, enums, f32/f64, Option, Vec,
        // and the array-of-f32 (eq_gains). A full field-by-field walk
        // already exists in player_settings_toml_roundtrip_full_field_coverage;
        // here we just need to prove the redb wire format survives the
        // struct rename.
        assert_eq!(lhs.volume, rhs.volume, "volume (f64)");
        assert_eq!(lhs.sfx_volume, rhs.sfx_volume, "sfx_volume");
        assert_eq!(lhs.scrobbling_enabled, rhs.scrobbling_enabled);
        assert_eq!(lhs.scrobble_threshold, rhs.scrobble_threshold);
        assert_eq!(lhs.start_view, rhs.start_view);
        assert_eq!(lhs.stable_viewport, rhs.stable_viewport);
        assert_eq!(lhs.light_mode, rhs.light_mode);
        assert_eq!(lhs.enter_behavior, rhs.enter_behavior);
        assert_eq!(lhs.nav_layout, rhs.nav_layout);
        assert_eq!(lhs.volume_normalization, rhs.volume_normalization);
        assert_eq!(lhs.default_playlist_id, rhs.default_playlist_id);
        assert_eq!(lhs.local_music_path, rhs.local_music_path);
        assert_eq!(lhs.font_family, rhs.font_family);
        assert_eq!(lhs.eq_gains, rhs.eq_gains, "eq_gains [f32; 10]");
        assert_eq!(
            lhs.custom_eq_presets.len(),
            rhs.custom_eq_presets.len(),
            "custom_eq_presets Vec<>"
        );
        assert_eq!(
            lhs.custom_eq_presets[0].name, rhs.custom_eq_presets[0].name,
            "custom_eq_presets[0].name"
        );
        assert_eq!(
            lhs.view_columns.queue_show_genre, rhs.view_columns.queue_show_genre,
            "queue_show_genre (the silent-drop sentinel)"
        );
        assert_eq!(lhs.artwork_column_mode, rhs.artwork_column_mode);
        assert_eq!(lhs.show_tray_icon, rhs.show_tray_icon);
        assert_eq!(lhs.rating_reminder_trigger, rhs.rating_reminder_trigger);
        assert_eq!(lhs.rating_reminder_percent, rhs.rating_reminder_percent);
    }

    /// M3-S3: `get_player_settings` mirrors the manager's in-memory
    /// `visualizer` field (config.toml-sourced) onto
    /// `LivePlayerSettings.visualizer` wholesale.
    #[test]
    fn visualizer_mirrors_to_live_player_settings() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("test_settings.redb");
        let storage = StateStorage::new(path).expect("StateStorage::new");
        let mut sm = SettingsManager::for_test(storage);

        sm.visualizer.noise_reduction = 0.42;
        sm.visualizer.waves = true;
        sm.visualizer.bars.led_bars = true;
        sm.visualizer.scope.point_count = 128;

        let live = sm.get_player_settings();
        assert_eq!(live.visualizer.noise_reduction, 0.42);
        assert!(live.visualizer.waves);
        assert!(live.visualizer.bars.led_bars);
        assert_eq!(live.visualizer.scope.point_count, 128);
    }

    /// M3-S3: visualizer config NEVER lands in redb — the serialized
    /// `PersistedPlayerSettings` JSON has no `visualizer` key (and no
    /// `visualizer.`-prefixed key). `visualization_mode` (the audio-visualizer
    /// on/off selector) is a different, legitimate key.
    #[test]
    fn persisted_player_settings_json_has_no_visualizer_key() {
        let sentinel = build_exhaustive_persisted_player_settings();
        let json = serde_json::to_value(&sentinel).expect("serialize to JSON");
        let obj = json.as_object().expect("JSON object");
        assert!(
            obj.get("visualizer").is_none(),
            "PersistedPlayerSettings must NOT gain a visualizer key (config.toml is the sole store)"
        );
        for key in obj.keys() {
            assert!(
                !key.starts_with("visualizer."),
                "unexpected visualizer sub-key {key} in PersistedPlayerSettings JSON"
            );
        }
    }

    /// M3-S8g per-mode round-trip helper: parse a `[visualizer]` TOML
    /// document, seed a test manager's in-memory config with it, and return
    /// the LivePlayerSettings projection (the full config.toml → in-memory →
    /// Live chain; redb is deliberately not involved — visualizer config
    /// never lands there).
    fn visualizer_toml_to_live(
        toml_doc: &str,
    ) -> crate::types::player_settings::LivePlayerSettings {
        let cf: crate::types::visualizer_config::ConfigFile =
            toml::from_str(toml_doc).expect("parse [visualizer] document");
        let mut parsed = cf.visualizer;
        parsed.validate();

        let tmp = tempfile::tempdir().expect("tempdir");
        let storage =
            StateStorage::new(tmp.path().join("test_settings.redb")).expect("StateStorage::new");
        let mut sm = SettingsManager::for_test(storage);
        sm.visualizer = parsed;
        sm.get_player_settings()
    }

    /// M3-S8g: a non-default Bars config survives config.toml → in-memory →
    /// LivePlayerSettings.
    #[test]
    fn bars_mode_config_toml_live_roundtrip() {
        let live = visualizer_toml_to_live(
            "[visualizer.bars]\nmax_bars = 128\nled_bars = true\ngradient_mode = \"static\"\npeak_mode = \"fade\"\nbar_width_min = 4\ntrails = 0.5\nplacement = \"bottom_band\"\n",
        );
        let bars = &live.visualizer.bars;
        assert_eq!(bars.max_bars, 128);
        assert!(bars.led_bars);
        assert_eq!(
            bars.gradient_mode,
            crate::types::visualizer_config::BarsGradientMode::Static
        );
        assert_eq!(
            bars.peak_mode,
            crate::types::visualizer_config::BarsPeakMode::Fade
        );
        assert_eq!(bars.bar_width_min, 4.0);
        assert_eq!(bars.trails, 0.5);
        assert_eq!(
            bars.placement,
            crate::types::visualizer_config::VisualizerPlacement::BottomBand
        );
    }

    /// M3-S8g: a non-default Lines config survives config.toml → in-memory →
    /// LivePlayerSettings.
    #[test]
    fn lines_mode_config_toml_live_roundtrip() {
        let live = visualizer_toml_to_live(
            "[visualizer.lines]\npoint_count = 256\nstyle = \"angular\"\ngradient_mode = \"position\"\nmirror = true\nboat = false\nfill_opacity = 0.25\n",
        );
        let lines = &live.visualizer.lines;
        assert_eq!(lines.point_count, 256);
        assert_eq!(
            lines.style,
            crate::types::visualizer_config::LinesStyle::Angular
        );
        assert_eq!(
            lines.gradient_mode,
            crate::types::visualizer_config::LinesGradientMode::Position
        );
        assert!(lines.mirror);
        assert!(!lines.boat);
        assert_eq!(lines.fill_opacity, 0.25);
    }

    /// M3-S8g: a non-default Scope config survives config.toml → in-memory →
    /// LivePlayerSettings (validate() clamps ride along: 9000 points → 512).
    #[test]
    fn scope_mode_config_toml_live_roundtrip() {
        let live = visualizer_toml_to_live(
            "[visualizer.scope]\npoint_count = 9000\nradius = 0.5\nsensitivity = 2.5\nbeam = false\nparticle_count = 1024\necho = 0.25\ngradient_mode = \"breathing\"\n",
        );
        let scope = &live.visualizer.scope;
        assert_eq!(scope.point_count, 512, "validate() must clamp on read");
        assert_eq!(scope.radius, 0.5);
        assert_eq!(scope.sensitivity, 2.5);
        assert!(!scope.beam);
        assert_eq!(scope.particle_count, 1024);
        assert_eq!(scope.echo, 0.25);
        assert_eq!(
            scope.gradient_mode,
            crate::types::visualizer_config::LinesGradientMode::Breathing
        );
    }

    // ── M4 golden-bytes harness ─────────────────────────────────────────
    //
    // These four tests pin the EXACT serde_json bytes PersistedPlayerSettings
    // writes to redb, captured on the post-M3 tree. NOTE: they make struct
    // FIELD DECLARATION ORDER load-bearing (serde_json emits in declaration
    // order; the #[serde(flatten)] view_columns routes through serde's
    // Content buffer). An order-only golden diff is a CONSERVATIVE tripwire,
    // NOT a real redb-read compat break (serde_json reads are name-keyed).
    // How to tell: a diff that is purely a reordering of identical
    // "name": value pairs is order-only and safe (fix the declaration order,
    // do NOT regenerate the golden to mask it); a changed key name, changed
    // default value, or dropped/added key is a REAL break.

    const SENTINEL_GOLDEN: &str = include_str!("testdata/persisted_sentinel.golden.json");
    const DEFAULT_GOLDEN: &str = include_str!("testdata/persisted_default.golden.json");

    /// The exhaustive sentinel serializes to the exact golden bytes — every
    /// field name, serde attr, and declaration position pinned.
    #[test]
    fn persisted_sentinel_serializes_to_golden_json() {
        let sentinel = build_exhaustive_persisted_player_settings();
        assert_eq!(
            serde_json::to_string_pretty(&sentinel).expect("serialize sentinel"),
            SENTINEL_GOLDEN,
            "PersistedPlayerSettings sentinel bytes drifted from the golden (see harness note)"
        );
    }

    /// The golden bytes parse back and re-serialize byte-identically — a
    /// rename/attr change that survives serialization is caught on the read
    /// side too.
    #[test]
    fn golden_json_reserializes_byte_identical() {
        let parsed: PersistedPlayerSettings =
            serde_json::from_str(SENTINEL_GOLDEN).expect("golden must deserialize");
        assert_eq!(
            serde_json::to_string_pretty(&parsed).expect("re-serialize"),
            SENTINEL_GOLDEN,
            "golden re-serialization drifted (rename or attr change)"
        );
    }

    /// `PersistedPlayerSettings::default()` serializes to the default golden —
    /// pins every default VALUE (the compat surface for old configs missing
    /// keys).
    #[test]
    fn persisted_default_serializes_to_default_golden_json() {
        assert_eq!(
            serde_json::to_string_pretty(&PersistedPlayerSettings::default())
                .expect("serialize default"),
            DEFAULT_GOLDEN,
            "a default VALUE changed (compat surface for old stores)"
        );
    }

    /// A legacy partial redb JSON (old app version: few keys, legacy bool
    /// forms for the three shimmed enums) fills every missing field from
    /// Default and maps the legacy bools through their compat shims.
    #[test]
    fn legacy_partial_redb_json_fills_defaults() {
        let legacy = r#"{
            "volume": 0.5,
            "start_view": "Artists",
            "rounded_mode": true,
            "bit_perfect": true,
            "verbose_config": true,
            "queue_show_genre": true
        }"#;
        let parsed: PersistedPlayerSettings =
            serde_json::from_str(legacy).expect("legacy partial JSON must parse");
        assert_eq!(parsed.volume, 0.5);
        assert_eq!(parsed.start_view, "Artists");
        // Legacy bools map through the compat shims.
        assert_eq!(parsed.rounded_mode, RoundedMode::On);
        assert_eq!(
            parsed.bit_perfect,
            crate::types::player_settings::BitPerfectMode::Strict
        );
        assert_eq!(
            parsed.verbose_config,
            crate::types::player_settings::VerboseConfig::On
        );
        // Flattened view-column key lands on the flattened struct.
        assert!(parsed.view_columns.queue_show_genre);
        // Missing fields fill from Default.
        let d = PersistedPlayerSettings::default();
        assert_eq!(parsed.scrobble_threshold, d.scrobble_threshold);
        assert_eq!(parsed.font_family, d.font_family);
        assert_eq!(parsed.eq_gains, d.eq_gains);
        assert!(!parsed.light_mode);
    }

    /// M4: both twins emit from the ONE `player_settings_schema!` table with
    /// the declared divergences — a `same` field exists on both, each split
    /// field carries f64 on Persisted / f32 on Live, `light_mode` exists on
    /// Persisted only and `visualizer` on Live only (reading them here is the
    /// compile-time proof; the golden + overlap tests pin the rest).
    #[test]
    fn twin_schema_field_parity_compiles() {
        let p = PersistedPlayerSettings::default();
        let l = crate::types::player_settings::LivePlayerSettings::default();

        // same field on both.
        assert_eq!(p.start_view, "Harbour");
        assert_eq!(l.start_view, "");

        // splits: f64 on Persisted, f32 on Live.
        let _pv: f64 = p.volume;
        let _lv: f32 = l.volume;
        let _ps: f64 = p.sfx_volume;
        let _ls: f32 = l.sfx_volume;
        let _pt: f64 = p.scrobble_threshold;
        let _lt: f32 = l.scrobble_threshold;

        // divergences: persist_only / live_only.
        let _light: bool = p.light_mode;
        let _viz: crate::types::visualizer_config::VisualizerConfig = l.visualizer;
    }

    /// M2 structural sentinel (Part A): every residual key is published in
    /// its tab's copy-only registry, is NOT a dispatchable settings-table
    /// entry, and is invisible to the containment helpers — proving the
    /// residuals are macro-owned without phantom dispatch arms or UI rows.
    #[test]
    fn residual_fields_are_macro_owned() {
        use crate::services::settings_tables::{
            general::tab_general_contains,
            interface::{TAB_INTERFACE_COPY_ONLY_KEYS, tab_interface_contains},
            playback::{TAB_PLAYBACK_COPY_ONLY_KEYS, tab_playback_contains},
        };

        const RESIDUAL_KEYS: &[&str] = &[
            "interface.font_family",
            "interface.artwork_column_width_pct",
            "playback.visualization_mode",
            "playback.sound_effects_enabled",
            "playback.sfx_volume",
            "playback.eq_enabled",
            "playback.eq_gains",
            "playback.custom_eq_presets",
        ];

        let registry: Vec<&str> = TAB_INTERFACE_COPY_ONLY_KEYS
            .iter()
            .chain(TAB_PLAYBACK_COPY_ONLY_KEYS.iter())
            .copied()
            .collect();
        for key in RESIDUAL_KEYS {
            assert!(
                registry.contains(key),
                "{key} must be published in a TAB_*_COPY_ONLY_KEYS registry"
            );
            let in_settings_table = crate::services::settings_tables::TAB_GENERAL_SETTINGS
                .iter()
                .chain(crate::services::settings_tables::TAB_INTERFACE_SETTINGS.iter())
                .chain(crate::services::settings_tables::TAB_PLAYBACK_SETTINGS.iter())
                .any(|d| d.key == *key);
            assert!(
                !in_settings_table,
                "{key} must NOT appear in any TAB_*_SETTINGS table (phantom dispatch/UI row)"
            );
            assert!(
                !tab_general_contains(key)
                    && !tab_interface_contains(key)
                    && !tab_playback_contains(key),
                "{key} must be invisible to every tab containment helper"
            );
        }
        assert_eq!(
            registry.len(),
            RESIDUAL_KEYS.len(),
            "copy-only registries must contain exactly the 8 residual keys"
        );
    }

    /// M2 behavioral sentinel (Part B): the 8 residual fields survive the
    /// FULL orchestrator round-trip, asserted against the SOURCE sentinel
    /// values — NOT pipeline-vs-pipeline (a copy-step deleted from both dump
    /// passes would make ui_ps1 == ui_ps2 vacuously equal; anchoring on the
    /// exhaustive builder's values catches exactly that). 1e-4 tolerance for
    /// the `round_f32`-quantized `sfx_volume` / `eq_gains`.
    #[test]
    fn residual_fields_round_trip() {
        let src = build_exhaustive_persisted_player_settings();

        let (sm, _tmp) = make_test_manager_with_player(src.clone());
        let live = sm.get_player_settings();
        let ts = TomlSettings::from_player_settings_with_existing(&live, None);
        let serialized = toml::to_string(&ts).expect("serialize TomlSettings");
        let parsed: TomlSettings = toml::from_str(&serialized).expect("parse TomlSettings");
        let mut dst = PersistedPlayerSettings::default();
        apply_toml_settings_to_internal(&parsed, &mut dst);
        let (sm2, _tmp2) = make_test_manager_with_player(dst);
        let out = sm2.get_player_settings();

        // 1. font_family (String, Interface).
        assert_eq!(out.font_family, src.font_family, "font_family residual");
        // 2. artwork_column_width_pct (f32, Interface).
        assert!(
            (out.artwork_column_width_pct - src.artwork_column_width_pct).abs() < 1e-4,
            "artwork_column_width_pct residual: {} vs {}",
            out.artwork_column_width_pct,
            src.artwork_column_width_pct
        );
        // 3. visualization_mode (Copy enum, Playback).
        assert_eq!(
            out.visualization_mode, src.visualization_mode,
            "visualization_mode residual"
        );
        // 4. sound_effects_enabled (bool, Playback).
        assert_eq!(
            out.sound_effects_enabled, src.sound_effects_enabled,
            "sound_effects_enabled residual"
        );
        // 5. sfx_volume (f64 Persisted / f32 Live+Toml, Playback).
        assert!(
            (f64::from(out.sfx_volume) - src.sfx_volume).abs() < 1e-4,
            "sfx_volume residual: {} vs {}",
            out.sfx_volume,
            src.sfx_volume
        );
        // 6. eq_enabled (bool, Playback).
        assert_eq!(out.eq_enabled, src.eq_enabled, "eq_enabled residual");
        // 7. eq_gains ([f32; 10], Playback).
        for (i, (a, b)) in out.eq_gains.iter().zip(src.eq_gains.iter()).enumerate() {
            assert!((a - b).abs() < 1e-4, "eq_gains[{i}] residual: {a} vs {b}");
        }
        // 8. custom_eq_presets (Vec<CustomEqPreset>, Playback).
        assert_eq!(
            out.custom_eq_presets.len(),
            src.custom_eq_presets.len(),
            "custom_eq_presets residual length"
        );
        for (o, s) in out
            .custom_eq_presets
            .iter()
            .zip(src.custom_eq_presets.iter())
        {
            assert_eq!(o.name, s.name, "custom_eq_presets name");
            for (i, (a, b)) in o.gains.iter().zip(s.gains.iter()).enumerate() {
                assert!(
                    (a - b).abs() < 1e-4,
                    "custom_eq_presets gains[{i}]: {a} vs {b}"
                );
            }
        }

        // Bool residuals in BOTH polarities: the exhaustive builder flips
        // each bool from its Persisted default, but a flipped value can
        // coincide with LivePlayerSettings' DERIVED default (false) — e.g.
        // sound_effects_enabled's sentinel false == Live's default false, so
        // a dropped copy-step would pass vacuously on one polarity. Running
        // the chain with each bool at both values closes that hole.
        for polarity in [false, true] {
            let mut src2 = build_exhaustive_persisted_player_settings();
            src2.sound_effects_enabled = polarity;
            src2.eq_enabled = polarity;
            let (sm, _tmp) = make_test_manager_with_player(src2);
            let live = sm.get_player_settings();
            let ts = TomlSettings::from_player_settings_with_existing(&live, None);
            let serialized = toml::to_string(&ts).expect("serialize TomlSettings");
            let parsed: TomlSettings = toml::from_str(&serialized).expect("parse TomlSettings");
            let mut dst = PersistedPlayerSettings::default();
            apply_toml_settings_to_internal(&parsed, &mut dst);
            let (sm2, _tmp2) = make_test_manager_with_player(dst);
            let out = sm2.get_player_settings();
            assert_eq!(
                out.sound_effects_enabled, polarity,
                "sound_effects_enabled residual (polarity {polarity})"
            );
            assert_eq!(
                out.eq_enabled, polarity,
                "eq_enabled residual (polarity {polarity})"
            );
        }
    }

    /// Every `ViewColumns` field flipped from its shipped default. The full
    /// struct literal (no `..`) is deliberate: adding a `ViewColumns` field
    /// without flipping it here is a missing-initializer compile error.
    fn flipped_view_columns() -> crate::types::view_columns::ViewColumns {
        let d = crate::types::view_columns::ViewColumns::default();
        crate::types::view_columns::ViewColumns {
            queue_show_stars: !d.queue_show_stars,
            queue_show_album: !d.queue_show_album,
            queue_show_duration: !d.queue_show_duration,
            queue_show_love: !d.queue_show_love,
            queue_show_plays: !d.queue_show_plays,
            queue_show_index: !d.queue_show_index,
            queue_show_thumbnail: !d.queue_show_thumbnail,
            queue_show_genre: !d.queue_show_genre,
            queue_show_select: !d.queue_show_select,
            albums_show_stars: !d.albums_show_stars,
            albums_show_songcount: !d.albums_show_songcount,
            albums_show_plays: !d.albums_show_plays,
            albums_show_love: !d.albums_show_love,
            albums_show_index: !d.albums_show_index,
            albums_show_thumbnail: !d.albums_show_thumbnail,
            albums_show_select: !d.albums_show_select,
            songs_show_stars: !d.songs_show_stars,
            songs_show_album: !d.songs_show_album,
            songs_show_duration: !d.songs_show_duration,
            songs_show_plays: !d.songs_show_plays,
            songs_show_love: !d.songs_show_love,
            songs_show_index: !d.songs_show_index,
            songs_show_thumbnail: !d.songs_show_thumbnail,
            songs_show_genre: !d.songs_show_genre,
            songs_show_select: !d.songs_show_select,
            artists_show_stars: !d.artists_show_stars,
            artists_show_albumcount: !d.artists_show_albumcount,
            artists_show_songcount: !d.artists_show_songcount,
            artists_show_plays: !d.artists_show_plays,
            artists_show_love: !d.artists_show_love,
            artists_show_index: !d.artists_show_index,
            artists_show_thumbnail: !d.artists_show_thumbnail,
            artists_show_select: !d.artists_show_select,
            genres_show_index: !d.genres_show_index,
            genres_show_thumbnail: !d.genres_show_thumbnail,
            genres_show_albumcount: !d.genres_show_albumcount,
            genres_show_songcount: !d.genres_show_songcount,
            genres_show_select: !d.genres_show_select,
            playlists_show_index: !d.playlists_show_index,
            playlists_show_thumbnail: !d.playlists_show_thumbnail,
            playlists_show_songcount: !d.playlists_show_songcount,
            playlists_show_duration: !d.playlists_show_duration,
            playlists_show_updatedat: !d.playlists_show_updatedat,
            playlists_show_select: !d.playlists_show_select,
            similar_show_index: !d.similar_show_index,
            similar_show_thumbnail: !d.similar_show_thumbnail,
            similar_show_album: !d.similar_show_album,
            similar_show_duration: !d.similar_show_duration,
            similar_show_love: !d.similar_show_love,
            similar_show_select: !d.similar_show_select,
        }
    }

    /// M1 characterization: every one of the 50 view-column booleans survives
    /// the FULL orchestrator round-trip — redb-backed Persisted →
    /// `get_player_settings` (dump) → `from_player_settings_with_existing`
    /// (write) → TOML bytes → parse → `apply_toml_settings_to_internal`
    /// (apply) → `get_player_settings` again. Written against the pre-rewire
    /// 7-helper path and kept green across the consolidated single-call
    /// rewire; `ViewColumns` derives `PartialEq`, so each whole-struct
    /// equality assert covers all 50 fields.
    #[test]
    fn all_view_columns_survive_full_orchestrator_roundtrip() {
        let flipped = flipped_view_columns();
        let mut persisted = PersistedPlayerSettings::default();
        persisted.view_columns = flipped.clone();

        // Persisted → Live (dump direction).
        let (sm, _tmp) = make_test_manager_with_player(persisted);
        let live = sm.get_player_settings();
        assert_eq!(
            live.view_columns, flipped,
            "dump direction must carry all 50 flipped columns onto LivePlayerSettings"
        );

        // Live → TOML (write direction) → bytes → parse.
        let ts = TomlSettings::from_player_settings_with_existing(&live, None);
        let toml_str = toml::to_string(&ts).expect("serialize TomlSettings");
        let parsed: TomlSettings = toml::from_str(&toml_str).expect("parse TomlSettings");
        assert_eq!(
            parsed.view_columns, flipped,
            "TOML round-trip must preserve all 50 flipped columns"
        );

        // TOML → Persisted (apply direction) → Live again.
        let mut p2 = PersistedPlayerSettings::default();
        apply_toml_settings_to_internal(&parsed, &mut p2);
        let (sm2, _tmp2) = make_test_manager_with_player(p2);
        let live2 = sm2.get_player_settings();
        assert_eq!(
            live2.view_columns, flipped,
            "apply direction must land all 50 flipped columns back on the live view"
        );
    }

    /// Field-mapping integrity test for the `define_settings!` `read:`
    /// closures: build a `PersistedPlayerSettings` with three sentinel
    /// values, run it through `get_player_settings()` (which exercises
    /// every per-tab `dump_*_player_settings` and the consolidated
    /// `dump_columns_tab_player_settings`), and assert the corresponding fields
    /// land on the resulting `LivePlayerSettings`. Pins that the rename
    /// didn't silently mismap any field across the redb↔UI boundary.
    #[test]
    fn live_and_persisted_field_overlap() {
        let mut persisted = PersistedPlayerSettings::default();
        // Pick one field from each of the 3 tabs the macro owns:
        //   General  — start_view
        //   Interface — nav_display_mode
        //   Playback  — crossfade_duration_secs
        // Plus one view-column field (Queue) so the consolidated
        // dump_columns_tab_player_settings path is also exercised end-to-end:
        //   queue_show_select
        persisted.start_view = "Albums".to_string();
        persisted.nav_display_mode = NavDisplayMode::IconsOnly;
        persisted.crossfade_duration_secs = 11;
        persisted.view_columns.queue_show_select = true;

        let (sm, _tmp) = make_test_manager_with_player(persisted);
        let live = sm.get_player_settings();

        assert_eq!(
            live.start_view, "Albums",
            "General tab read: closure must copy start_view"
        );
        assert_eq!(
            live.nav_display_mode,
            NavDisplayMode::IconsOnly,
            "Interface tab read: closure must copy nav_display_mode"
        );
        assert_eq!(
            live.crossfade_duration_secs, 11,
            "Playback tab read: closure must copy crossfade_duration_secs"
        );
        assert!(
            live.view_columns.queue_show_select,
            "Queue view-column dump must copy queue_show_select"
        );
    }
}
