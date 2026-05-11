//! Player settings — the live in-memory union of every user-tunable knob.
//!
//! Per-domain enum families live in sub-modules and are re-exported here so
//! callers continue to use `crate::types::player_settings::Foo` paths
//! regardless of which file the type lives in.

mod artwork;
mod library;
mod navigation;
mod playback;
mod slot_list;
mod strip;
mod visualizer;

pub use artwork::*;
pub use library::*;
pub use navigation::*;
pub use playback::*;
pub use slot_list::*;
pub use strip::*;
pub use visualizer::*;

/// Player settings loaded from persistence (redb).
///
/// Note: `light_mode` is stored in config.toml, not redb.
/// See `theme_config::load_light_mode_from_config()`.
#[derive(Debug, Clone, Default)]
pub struct PlayerSettings {
    pub volume: f32,
    pub sfx_volume: f32,
    pub sound_effects_enabled: bool,
    pub visualization_mode: VisualizationMode,
    /// Whether scrobbling is enabled
    pub scrobbling_enabled: bool,
    /// Scrobble threshold as a fraction of track duration (0.25–0.90)
    pub scrobble_threshold: f32,
    /// Start view name (e.g. "Queue", "Albums")
    pub start_view: String,
    /// Whether stable viewport mode is enabled
    pub stable_viewport: bool,
    /// Whether auto-follow playing track is enabled
    pub auto_follow_playing: bool,
    /// What Enter does in the Songs view
    pub enter_behavior: EnterBehavior,
    /// Local filesystem prefix to prepend to song paths for file manager (empty = not configured)
    pub local_music_path: String,
    /// Whether rounded corners mode is enabled
    pub rounded_mode: bool,
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
    /// Whether crossfade between tracks is enabled
    pub crossfade_enabled: bool,
    /// Crossfade duration in seconds (1–12)
    pub crossfade_duration_secs: u32,
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
    /// Whether the 10-band graphic EQ is enabled (master bypass).
    pub eq_enabled: bool,
    /// Per-band EQ gain values in dB (-12.0 to +12.0). Indexed by band.
    pub eq_gains: [f32; 10],
    /// User-created custom EQ presets.
    pub custom_eq_presets: Vec<crate::audio::eq::CustomEqPreset>,
    /// When true, all settings (including defaults) are written to config.toml.
    pub verbose_config: bool,
    /// Library page size controls how many items are fetched at once.
    pub library_page_size: LibraryPageSize,
    /// Artwork resolution for the large artwork panel.
    pub artwork_resolution: ArtworkResolution,
    /// Whether the Artists view shows only album artists
    pub show_album_artists_only: bool,
    /// Whether to suppress the toast notification shown on Navidrome library-refresh
    /// events. Default false (toasts shown).
    pub suppress_library_refresh_toasts: bool,
    /// Whether the queue's stars rating column is visible (subject to a
    /// separate responsive width gate — see queue.rs).
    pub queue_show_stars: bool,
    /// Whether the queue's album column is visible.
    pub queue_show_album: bool,
    /// Whether the queue's duration column is visible.
    pub queue_show_duration: bool,
    /// Whether the queue's love (heart) column is visible.
    pub queue_show_love: bool,
    /// Whether the queue's plays column is visible (default: false).
    /// Auto-shown when sort = MostPlayed regardless of this toggle.
    pub queue_show_plays: bool,
    /// Whether the queue's leading row-index column is visible.
    pub queue_show_index: bool,
    /// Whether the queue's leading thumbnail column is visible.
    pub queue_show_thumbnail: bool,
    /// Whether the queue's genre is shown stacked under the album in the
    /// album column slot. Auto-shown when sort = Genre regardless of this
    /// toggle. Falls into the album slot at album-size font when the album
    /// column is hidden.
    pub queue_show_genre: bool,
    /// Whether the queue's leading multi-select checkbox column is visible.
    pub queue_show_select: bool,

    // -- Albums view column toggles --
    pub albums_show_stars: bool,
    pub albums_show_songcount: bool,
    pub albums_show_plays: bool,
    pub albums_show_love: bool,
    pub albums_show_index: bool,
    pub albums_show_thumbnail: bool,
    /// Whether the albums view's leading multi-select checkbox column is visible.
    pub albums_show_select: bool,

    // -- Songs view column toggles --
    pub songs_show_stars: bool,
    pub songs_show_album: bool,
    pub songs_show_duration: bool,
    pub songs_show_plays: bool,
    pub songs_show_love: bool,
    pub songs_show_index: bool,
    pub songs_show_thumbnail: bool,
    /// Genre stacked under album in the album column slot. Auto-shown when
    /// sort = Genre regardless of this toggle. Replaces the album slot at
    /// album-size font when the album column is hidden.
    pub songs_show_genre: bool,
    /// Whether the songs view's leading multi-select checkbox column is visible.
    pub songs_show_select: bool,

    // -- Artists view column toggles --
    pub artists_show_stars: bool,
    pub artists_show_albumcount: bool,
    pub artists_show_songcount: bool,
    pub artists_show_plays: bool,
    pub artists_show_love: bool,
    pub artists_show_index: bool,
    pub artists_show_thumbnail: bool,
    /// Whether the artists view's leading multi-select checkbox column is visible.
    pub artists_show_select: bool,

    // -- Genres view column toggles --
    pub genres_show_index: bool,
    pub genres_show_thumbnail: bool,
    pub genres_show_albumcount: bool,
    pub genres_show_songcount: bool,
    /// Whether the genres view's leading multi-select checkbox column is visible.
    pub genres_show_select: bool,

    // -- Playlists view column toggles --
    pub playlists_show_index: bool,
    pub playlists_show_thumbnail: bool,
    pub playlists_show_songcount: bool,
    pub playlists_show_duration: bool,
    pub playlists_show_updatedat: bool,
    /// Whether the playlists view's leading multi-select checkbox column is visible.
    pub playlists_show_select: bool,

    // -- Similar view column toggles --
    pub similar_show_index: bool,
    pub similar_show_thumbnail: bool,
    pub similar_show_album: bool,
    pub similar_show_duration: bool,
    pub similar_show_love: bool,
    /// Whether the similar view's leading multi-select checkbox column is visible.
    pub similar_show_select: bool,

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

    // -- System tray --
    /// Whether to register a StatusNotifierItem tray icon on the session bus.
    pub show_tray_icon: bool,
    /// When true and `show_tray_icon` is on, the window's close button hides
    /// the window into the tray instead of quitting the application.
    pub close_to_tray: bool,
}
