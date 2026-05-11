//! Per-tab borrow-shaped settings data passed to the macro-emitted items
//! builders.
//!
//! `define_settings!` emits one `build_<tab>_tab_settings_items(data: &<TabData>)`
//! helper per tab; the helper reads each entry's `ui_meta.read_field` closure
//! against the borrow-shaped struct here. UI-crate hand-written builders (in
//! `src/views/settings/items_*.rs`) construct one of these from live config
//! state and pass it to the helper.
//!
//! These types live in the data crate so `define_settings!` (also in the data
//! crate) can reference them at expansion time. They are iced-free — only
//! `&'a str`, primitives, and a single `f64`. The UI crate re-exports each via
//! `pub(crate) use nokkvi_data::types::settings_data::...;` so existing import
//! paths in the items modules keep resolving.

/// Data needed by the General tab builder.
#[derive(Debug, Clone)]
pub struct GeneralSettingsData<'a> {
    pub server_url: &'a str,
    pub username: &'a str,
    pub start_view: &'a str,
    pub stable_viewport: bool,
    pub auto_follow_playing: bool,
    pub enter_behavior: &'a str,
    pub local_music_path: &'a str,
    pub verbose_config: bool,
    pub library_page_size: &'a str,
    pub artwork_resolution: &'a str,
    pub show_album_artists_only: bool,
    pub suppress_library_refresh_toasts: bool,
    pub show_tray_icon: bool,
    pub close_to_tray: bool,
}

/// Data needed by the Interface tab builder.
#[derive(Debug, Clone)]
pub struct InterfaceSettingsData<'a> {
    pub nav_layout: &'a str,
    pub nav_display_mode: &'a str,
    pub track_info_display: &'a str,
    pub slot_row_height: &'a str,
    pub horizontal_volume: bool,
    pub slot_text_links: bool,
    pub font_family: &'a str,
    pub strip_show_title: bool,
    pub strip_show_artist: bool,
    pub strip_show_album: bool,
    pub strip_show_format_info: bool,
    pub strip_merged_mode: bool,
    pub strip_show_labels: bool,
    pub strip_separator: &'a str,
    pub strip_click_action: &'a str,
    pub albums_artwork_overlay: bool,
    pub artists_artwork_overlay: bool,
    pub songs_artwork_overlay: bool,
    pub playlists_artwork_overlay: bool,
    /// Artwork column display mode label (Auto / Always (Native) / Always (Stretched) / Never)
    pub artwork_column_mode: &'a str,
    /// Artwork column stretch fit label (Cover / Fill) — only consumed when mode is stretched.
    pub artwork_column_stretch_fit: &'a str,
    /// Auto-mode max artwork fraction of the window's short axis (0.30..=0.70).
    pub artwork_auto_max_pct: f64,
}

/// Data needed by the Playback tab builder.
#[derive(Debug, Clone)]
pub struct PlaybackSettingsData<'a> {
    pub crossfade_enabled: bool,
    pub crossfade_duration_secs: i64,
    /// Volume-normalization mode label ("Off" / "AGC" / "ReplayGain (Track)" / "ReplayGain (Album)")
    pub volume_normalization: &'a str,
    pub normalization_level: &'a str,
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
    pub default_playlist_name: &'a str,
    pub queue_show_default_playlist: bool,
}
