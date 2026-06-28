//! Player settings — the live in-memory union of every user-tunable knob.
//!
//! Per-domain enum families live in sub-modules and are re-exported here so
//! callers continue to use `crate::types::player_settings::Foo` paths
//! regardless of which file the type lives in.

mod appearance;
mod artwork;
mod library;
mod navigation;
mod playback;
mod slot_list;
mod strip;
mod verbose;
mod visualizer;

pub use appearance::*;
pub use artwork::*;
pub use library::*;
pub use navigation::*;
pub use playback::*;
pub use slot_list::*;
pub use strip::*;
pub use verbose::*;
pub use visualizer::*;

use crate::audio::eq::EQ_BAND_COUNT;

/// Live, UI-facing player settings — the in-memory shape that
/// `Nokkvi.settings` mirrors and `Message::PlayerSettingsLoaded` carries.
///
/// Constructed from the redb-shaped
/// [`PersistedPlayerSettings`][crate::types::settings::PersistedPlayerSettings]
/// via `SettingsManager::get_player_settings`. Renamed from `PlayerSettings`
/// so it no longer collides with the persisted struct in the adjacent
/// `crate::types::settings` module.
///
/// Note: `light_mode` is stored in config.toml, not redb. See
/// `theme_config::load_light_mode_from_config()`.
#[derive(Debug, Clone, Default)]
pub struct LivePlayerSettings {
    pub volume: f32,
    pub sfx_volume: f32,
    pub sound_effects_enabled: bool,
    pub visualization_mode: VisualizationMode,
    /// Whether scrobbling is enabled
    pub scrobbling_enabled: bool,
    /// Scrobble threshold as a fraction of track duration (0.25–0.90)
    pub scrobble_threshold: f32,
    /// Whether internet-radio tracks scrobble directly to ListenBrainz
    pub radio_scrobbling_enabled: bool,
    /// Absolute seconds a radio track must play before it scrobbles
    pub radio_scrobble_threshold_secs: u32,
    /// Whether to send radio now-playing updates on each ICY track change
    pub radio_now_playing_enabled: bool,
    /// Start view name (e.g. "Queue", "Albums")
    pub start_view: String,
    /// Whether stable viewport mode is enabled
    pub stable_viewport: bool,
    /// Whether auto-follow playing track is enabled
    pub auto_follow_playing: bool,
    /// What Enter does in the Songs view
    pub enter_behavior: EnterBehavior,
    /// Whether plain Enter/click layers a one-shot Shuffle Play on top of `enter_behavior`
    pub enter_shuffle: bool,
    /// Local filesystem prefix to prepend to song paths for file manager (empty = not configured)
    pub local_music_path: String,
    /// Whether rounded corners mode is enabled
    pub rounded_mode: RoundedMode,
    /// Navigation layout mode (top bar vs side bar)
    pub nav_layout: NavLayout,
    /// Navigation display mode (text, icons, or both)
    pub nav_display_mode: NavDisplayMode,
    /// Track info display mode (off / player bar / top bar)
    pub track_info_display: TrackInfoDisplay,
    /// Slot list row density (Compact / Default / Comfortable / Spacious)
    pub slot_row_height: SlotRowHeight,
    /// Whether the opacity gradient on non-center slots is enabled
    pub opacity_gradient: bool,
    /// Whether clickable text links in slot list items are enabled (default: true)
    pub slot_text_links: bool,
    /// How the slot-list scrollbar is shown (default `Always` — a permanent
    /// gutter track). `OnHover` is the transient fade handle; `Hidden` removes
    /// the bar entirely.
    pub scrollbar_visibility: ScrollbarVisibility,
    /// Which icon family the UI renders (default `Phosphor`). `Lucide` is the
    /// original thin-outline alternate.
    pub icon_set: IconSet,
    /// Whether crossfade between tracks is enabled
    pub crossfade_enabled: bool,
    /// Bit-perfect output mode (Off / Strict / Relaxed). Strict and Relaxed
    /// both feed the DAC native-rate, DSP-bypassed; they differ only on whether
    /// same-rate crossfades are allowed.
    pub bit_perfect: BitPerfectMode,
    /// Crossfade duration in seconds (1–12)
    pub crossfade_duration_secs: u32,
    /// Whether the Previous button restarts the current track once it has
    /// played past the threshold (default false).
    pub rewind_on_previous: bool,
    /// Default playlist ID for quick-add (None = no default)
    pub default_playlist_id: Option<String>,
    /// Default playlist display name (for settings UI)
    pub default_playlist_name: String,
    /// Whether to skip the Add to Playlist dialog and use the default playlist directly
    pub quick_add_to_playlist: bool,
    /// Whether the queue view's header shows the default playlist chip
    pub queue_show_default_playlist: bool,
    /// Whether volume sliders in the player bar are horizontal (default: false = vertical)
    pub horizontal_volume: bool,
    /// Whether the view-header toolbar auto-hides to a thin line until hovered
    /// or a sort/search shortcut is used (default: true)
    pub autohide_toolbar: bool,
    /// Collapsed auto-hide toolbar height in px (default: 4)
    pub autohide_toolbar_height: u32,
    /// Whether the collapsed auto-hide toolbar shows a centered accent grip bar (default: true)
    pub autohide_toolbar_grip: bool,
    /// What the collapsed auto-hide toolbar shows (default: Count strip)
    pub autohide_collapsed_appearance: CollapsedAppearance,
    /// Whether the mini-player bar shows the volume slider (default: true).
    /// Only affects `TrackInfoDisplay::MiniPlayer`.
    pub mini_player_show_volume: bool,
    /// Whether the mini-player bar shows the modes menu (default: true).
    /// Only affects `TrackInfoDisplay::MiniPlayer`.
    pub mini_player_show_modes: bool,
    /// Font family override. Empty = system default sans-serif.
    pub font_family: String,
    /// Volume normalization mode (Off / AGC / ReplayGain-track / ReplayGain-album)
    pub volume_normalization: VolumeNormalizationMode,
    /// AGC target level (Quiet / Normal / Loud) — only meaningful when
    /// `volume_normalization == Agc`.
    pub normalization_level: NormalizationLevel,
    /// Pre-amp dB applied on top of the resolved ReplayGain value
    /// (default 0.0; UI clamp -15..=15).
    pub replay_gain_preamp_db: f32,
    /// Fallback dB applied to tracks with no ReplayGain tags
    /// (default 0.0 = unity; UI clamp -15..=15).
    pub replay_gain_fallback_db: f32,
    /// When true, untagged tracks fall through to AGC instead of using
    /// `replay_gain_fallback_db`.
    pub replay_gain_fallback_to_agc: bool,
    /// When true, clamp the resolved gain so `peak * gain <= 1.0` using
    /// the track/album peak tag.
    pub replay_gain_prevent_clipping: bool,
    /// Whether the title field is visible in the track info strip (default: true)
    pub strip_show_title: bool,
    /// Whether the artist field is visible in the track info strip (default: true)
    pub strip_show_artist: bool,
    /// Whether the album field is visible in the track info strip (default: true)
    pub strip_show_album: bool,
    /// Whether format info (codec/kHz/kbps) is visible in the track info strip (default: true)
    pub strip_show_format_info: bool,
    /// Whether the metastrip renders artist/album/title as a single shared
    /// scrolling unit with one set of bookend separators (default: false).
    pub strip_merged_mode: bool,
    /// What happens when clicking the track info strip (default: GoToQueue)
    pub strip_click_action: StripClickAction,
    /// Whether `title:` / `artist:` / `album:` labels are prepended to each
    /// field in the metadata strip (default: true).
    pub strip_show_labels: bool,
    /// Visual character used to join visible fields in merged-mode rendering.
    pub strip_separator: StripSeparator,
    /// Active playlist ID loaded in the queue (None = no playlist context)
    pub active_playlist_id: Option<String>,
    /// Active playlist display name
    pub active_playlist_name: String,
    /// Active playlist comment/description
    pub active_playlist_comment: String,
    /// Active playlist total duration in seconds (0.0 when unknown).
    pub active_playlist_duration: f32,
    /// Active playlist last-updated timestamp (raw ISO-8601; empty when unknown).
    pub active_playlist_updated: String,
    /// Active playlist public/private visibility (drives the strip lock chip).
    pub active_playlist_public: bool,
    /// Active playlist song count (0 when unknown; strip falls back to queue length).
    pub active_playlist_song_count: u32,
    /// Whether the 10-band graphic EQ is enabled (master bypass).
    pub eq_enabled: bool,
    /// Per-band EQ gain values in dB (-12.0 to +12.0). Indexed by band.
    pub eq_gains: [f32; EQ_BAND_COUNT],
    /// User-created custom EQ presets.
    pub custom_eq_presets: Vec<crate::audio::eq::CustomEqPreset>,
    /// How config.toml is written (full / sparse-with-comments / sparse-clean).
    pub verbose_config: VerboseConfig,
    /// Library page size controls how many items are fetched at once.
    pub library_page_size: LibraryPageSize,
    /// Artwork resolution for the large artwork panel.
    pub artwork_resolution: ArtworkResolution,
    /// Whether the Artists view shows only album artists
    pub show_album_artists_only: bool,
    /// Whether to suppress the toast notification shown on Navidrome library-refresh
    /// events. Default false (toasts shown).
    pub suppress_library_refresh_toasts: bool,
    /// Per-view column-visibility toggles — the canonical
    /// [`ViewColumns`][crate::types::view_columns::ViewColumns] struct shared
    /// with `PersistedPlayerSettings` and `TomlSettings`. Its hand-written
    /// `Default` carries the real shipped column defaults, so this struct's
    /// derived `Default` no longer zeroes the columns.
    pub view_columns: crate::types::view_columns::ViewColumns,

    // -- Per-view artwork text overlay toggles --
    /// Whether the metadata text overlay is rendered on the large artwork in Albums view.
    pub albums_artwork_overlay: bool,
    /// Whether the metadata text overlay is rendered on the large artwork in Artists view.
    pub artists_artwork_overlay: bool,
    /// Whether the metadata text overlay is rendered on the large artwork in Songs view.
    pub songs_artwork_overlay: bool,
    /// Whether the metadata text overlay is rendered on the large artwork in Playlists view.
    pub playlists_artwork_overlay: bool,

    // -- Artwork column layout --
    /// Display mode for the large artwork column (auto-hide / always / never).
    pub artwork_column_mode: ArtworkColumnMode,
    /// Fit mode used when `artwork_column_mode == AlwaysStretched`.
    pub artwork_column_stretch_fit: ArtworkStretchFit,
    /// Artwork column width as a fraction of window width (0.05..=0.80).
    /// Only consulted in `AlwaysNative` / `AlwaysStretched` modes.
    pub artwork_column_width_pct: f32,
    /// Auto-mode max artwork fraction of the window's short axis
    /// (0.30..=0.70). Drives both the horizontal candidate and the
    /// portrait-fallback vertical candidate in the Auto resolver.
    pub artwork_auto_max_pct: f32,
    /// Always-Vertical artwork height as a fraction of window height
    /// (0.10..=0.80). Drives `AlwaysVerticalNative` and
    /// `AlwaysVerticalStretched` modes.
    pub artwork_vertical_height_pct: f32,

    // -- System tray --
    /// Whether to register a StatusNotifierItem tray icon on the session bus.
    pub show_tray_icon: bool,
    /// When true and `show_tray_icon` is on, the window's close button hides
    /// the window into the tray instead of quitting the application.
    pub close_to_tray: bool,

    // -- Rating reminder --
    /// Whether the rate-this-track desktop notification is enabled.
    pub rating_reminder_enabled: bool,
    /// Whether a desktop notification fires when a rating changes via a hotkey
    /// or the `nokkvi rate` IPC verb.
    pub rating_change_notification_enabled: bool,
    /// When the rating reminder fires (scrobble-confirmed vs percentage played).
    pub rating_reminder_trigger: RatingReminderTrigger,
    /// For the percentage trigger: fire once this percent of the track has
    /// played (clamped 60–90).
    pub rating_reminder_percent: u32,
}
