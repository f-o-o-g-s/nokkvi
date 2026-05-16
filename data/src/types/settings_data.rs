//! Per-tab settings data passed to the macro-emitted items builders.
//!
//! `define_settings!` emits one `build_<tab>_tab_settings_items(data: &<TabData>)`
//! helper per tab; the helper reads each entry's `ui_meta.read_field` closure
//! against the data struct here. UI-crate hand-written builders (in
//! `src/views/settings/items_*.rs`) construct one of these from live config
//! state and pass it to the helper.
//!
//! These types live in the data crate so `define_settings!` (also in the data
//! crate) can reference them at expansion time. They are iced-free —
//! `Cow<'static, str>` string slots, primitives, and `f64` only. The UI crate
//! re-exports each via `pub(crate) use nokkvi_data::types::settings_data::...;`
//! so existing import paths in the items modules keep resolving.
//!
//! String fields use `Cow<'static, str>` so the same struct accepts either a
//! `&'static str` literal (zero-cost `Cow::Borrowed` in test fixtures) or an
//! owned `String` (live config snapshot in production). `Default` is
//! implemented with a recognizable `"test-default"` sentinel so any path that
//! accidentally reads it in production is obvious.

use std::borrow::Cow;

/// Data needed by the General tab builder.
#[derive(Debug, Clone)]
pub struct GeneralSettingsData {
    pub server_url: Cow<'static, str>,
    pub username: Cow<'static, str>,
    pub start_view: Cow<'static, str>,
    pub stable_viewport: bool,
    pub auto_follow_playing: bool,
    pub enter_behavior: Cow<'static, str>,
    pub local_music_path: Cow<'static, str>,
    pub verbose_config: bool,
    pub library_page_size: Cow<'static, str>,
    pub artwork_resolution: Cow<'static, str>,
    pub show_album_artists_only: bool,
    pub suppress_library_refresh_toasts: bool,
    pub show_tray_icon: bool,
    pub close_to_tray: bool,
}

impl Default for GeneralSettingsData {
    fn default() -> Self {
        Self {
            server_url: Cow::Borrowed("test-default"),
            username: Cow::Borrowed("test-default"),
            start_view: Cow::Borrowed("test-default"),
            stable_viewport: false,
            auto_follow_playing: false,
            enter_behavior: Cow::Borrowed("test-default"),
            local_music_path: Cow::Borrowed("test-default"),
            verbose_config: false,
            library_page_size: Cow::Borrowed("test-default"),
            artwork_resolution: Cow::Borrowed("test-default"),
            show_album_artists_only: false,
            suppress_library_refresh_toasts: false,
            show_tray_icon: false,
            close_to_tray: false,
        }
    }
}

/// Data needed by the Interface tab builder.
#[derive(Debug, Clone)]
pub struct InterfaceSettingsData {
    pub nav_layout: Cow<'static, str>,
    pub nav_display_mode: Cow<'static, str>,
    pub track_info_display: Cow<'static, str>,
    pub slot_row_height: Cow<'static, str>,
    pub horizontal_volume: bool,
    pub slot_text_links: bool,
    pub font_family: Cow<'static, str>,
    pub strip_show_title: bool,
    pub strip_show_artist: bool,
    pub strip_show_album: bool,
    pub strip_show_format_info: bool,
    pub strip_merged_mode: bool,
    pub strip_show_labels: bool,
    pub strip_separator: Cow<'static, str>,
    pub strip_click_action: Cow<'static, str>,
    pub albums_artwork_overlay: bool,
    pub artists_artwork_overlay: bool,
    pub songs_artwork_overlay: bool,
    pub playlists_artwork_overlay: bool,
    /// Artwork column display mode label (Auto / Always (Native) / Always (Stretched) / Never)
    pub artwork_column_mode: Cow<'static, str>,
    /// Artwork column stretch fit label (Cover / Fill) — only consumed when mode is stretched.
    pub artwork_column_stretch_fit: Cow<'static, str>,
    /// Auto-mode max artwork fraction of the window's short axis (0.30..=0.70).
    pub artwork_auto_max_pct: f64,
    /// Always-Vertical artwork height as a fraction of window height (0.10..=0.80).
    pub artwork_vertical_height_pct: f64,
}

impl Default for InterfaceSettingsData {
    fn default() -> Self {
        Self {
            nav_layout: Cow::Borrowed("test-default"),
            nav_display_mode: Cow::Borrowed("test-default"),
            track_info_display: Cow::Borrowed("test-default"),
            slot_row_height: Cow::Borrowed("test-default"),
            horizontal_volume: false,
            slot_text_links: false,
            font_family: Cow::Borrowed("test-default"),
            strip_show_title: false,
            strip_show_artist: false,
            strip_show_album: false,
            strip_show_format_info: false,
            strip_merged_mode: false,
            strip_show_labels: false,
            strip_separator: Cow::Borrowed("test-default"),
            strip_click_action: Cow::Borrowed("test-default"),
            albums_artwork_overlay: false,
            artists_artwork_overlay: false,
            songs_artwork_overlay: false,
            playlists_artwork_overlay: false,
            artwork_column_mode: Cow::Borrowed("test-default"),
            artwork_column_stretch_fit: Cow::Borrowed("test-default"),
            artwork_auto_max_pct: 0.0,
            artwork_vertical_height_pct: 0.0,
        }
    }
}

/// Data needed by the Playback tab builder.
#[derive(Debug, Clone)]
pub struct PlaybackSettingsData {
    pub crossfade_enabled: bool,
    pub crossfade_duration_secs: i64,
    /// Volume-normalization mode label ("Off" / "AGC" / "ReplayGain (Track)" / "ReplayGain (Album)")
    pub volume_normalization: Cow<'static, str>,
    pub normalization_level: Cow<'static, str>,
    /// Pre-amp dB applied on top of resolved ReplayGain (rounded to int for UI).
    pub replay_gain_preamp_db: i64,
    /// Fallback dB for tracks with no ReplayGain tags.
    pub replay_gain_fallback_db: i64,
    /// Whether untagged tracks fall through to AGC.
    pub replay_gain_fallback_to_agc: bool,
    /// Whether the resolver clamps gain so peak·gain ≤ 1.0.
    pub replay_gain_prevent_clipping: bool,
    pub scrobbling_enabled: bool,
    /// Scrobble threshold as a fraction (0.25–0.90).
    pub scrobble_threshold: f64,
    pub quick_add_to_playlist: bool,
    pub default_playlist_name: Cow<'static, str>,
    pub queue_show_default_playlist: bool,
}

impl Default for PlaybackSettingsData {
    fn default() -> Self {
        Self {
            crossfade_enabled: false,
            crossfade_duration_secs: 0,
            volume_normalization: Cow::Borrowed("test-default"),
            normalization_level: Cow::Borrowed("test-default"),
            replay_gain_preamp_db: 0,
            replay_gain_fallback_db: 0,
            replay_gain_fallback_to_agc: false,
            replay_gain_prevent_clipping: false,
            scrobbling_enabled: false,
            scrobble_threshold: 0.0,
            quick_add_to_playlist: false,
            default_playlist_name: Cow::Borrowed("test-default"),
            queue_show_default_playlist: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn general_settings_data_default_uses_test_sentinel() {
        let data = GeneralSettingsData::default();
        assert_eq!(data.server_url.as_ref(), "test-default");
        assert_eq!(data.username.as_ref(), "test-default");
        assert_eq!(data.start_view.as_ref(), "test-default");
        assert_eq!(data.enter_behavior.as_ref(), "test-default");
        assert_eq!(data.local_music_path.as_ref(), "test-default");
        assert_eq!(data.library_page_size.as_ref(), "test-default");
        assert_eq!(data.artwork_resolution.as_ref(), "test-default");
        assert!(!data.stable_viewport);
        assert!(!data.auto_follow_playing);
        assert!(!data.verbose_config);
        assert!(!data.show_album_artists_only);
        assert!(!data.suppress_library_refresh_toasts);
        assert!(!data.show_tray_icon);
        assert!(!data.close_to_tray);
    }

    #[test]
    fn interface_settings_data_default_uses_test_sentinel() {
        let data = InterfaceSettingsData::default();
        assert_eq!(data.nav_layout.as_ref(), "test-default");
        assert_eq!(data.nav_display_mode.as_ref(), "test-default");
        assert_eq!(data.track_info_display.as_ref(), "test-default");
        assert_eq!(data.slot_row_height.as_ref(), "test-default");
        assert_eq!(data.font_family.as_ref(), "test-default");
        assert_eq!(data.strip_separator.as_ref(), "test-default");
        assert_eq!(data.strip_click_action.as_ref(), "test-default");
        assert_eq!(data.artwork_column_mode.as_ref(), "test-default");
        assert_eq!(data.artwork_column_stretch_fit.as_ref(), "test-default");
        assert_eq!(data.artwork_auto_max_pct, 0.0);
        assert_eq!(data.artwork_vertical_height_pct, 0.0);
        assert!(!data.horizontal_volume);
        assert!(!data.slot_text_links);
    }

    #[test]
    fn playback_settings_data_default_uses_test_sentinel() {
        let data = PlaybackSettingsData::default();
        assert_eq!(data.volume_normalization.as_ref(), "test-default");
        assert_eq!(data.normalization_level.as_ref(), "test-default");
        assert_eq!(data.default_playlist_name.as_ref(), "test-default");
        assert_eq!(data.crossfade_duration_secs, 0);
        assert_eq!(data.scrobble_threshold, 0.0);
        assert_eq!(data.replay_gain_preamp_db, 0);
        assert_eq!(data.replay_gain_fallback_db, 0);
        assert!(!data.crossfade_enabled);
        assert!(!data.replay_gain_fallback_to_agc);
        assert!(!data.replay_gain_prevent_clipping);
        assert!(!data.scrobbling_enabled);
        assert!(!data.quick_add_to_playlist);
        assert!(!data.queue_show_default_playlist);
    }
}
