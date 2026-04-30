use serde::{Deserialize, Serialize};

/// Library page size — controls how many items are fetched per API request.
///
/// Affects Songs, Albums, Artists views and progressive queue batching.
/// Serializes to lowercase strings for redb storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LibraryPageSize {
    /// Small pages for constrained environments (100 items)
    Small,
    /// Balanced fetch size (500 items)
    #[default]
    Default,
    /// Large pages for fast connections (1,000 items)
    Large,
    /// Very large pages (5,000 items) — may use significant memory
    Massive,
}

impl LibraryPageSize {
    /// Convert to record count
    pub fn to_usize(self) -> usize {
        match self {
            Self::Small => 100,
            Self::Default => 500,
            Self::Large => 1000,
            Self::Massive => 5000,
        }
    }

    /// Calculate dynamic fetch threshold based on page size
    pub fn fetch_threshold(self) -> usize {
        (self.to_usize() / 5).clamp(20, 500)
    }

    /// Convert from settings GUI label to enum variant
    pub fn from_label(label: &str) -> Self {
        match label {
            "Small (100)" => Self::Small,
            "Large (1,000)" => Self::Large,
            "Massive (5,000)" => Self::Massive,
            _ => Self::Default,
        }
    }

    /// Convert to settings GUI label
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Small => "Small (100)",
            Self::Default => "Default (500)",
            Self::Large => "Large (1,000)",
            Self::Massive => "Massive (5,000)",
        }
    }
}

impl std::fmt::Display for LibraryPageSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Small => write!(f, "small"),
            Self::Default => write!(f, "default"),
            Self::Large => write!(f, "large"),
            Self::Massive => write!(f, "massive"),
        }
    }
}

/// Visualization mode for the audio visualizer.
///
/// Cycles: Off → Bars → Lines → Off via `next()`.
/// Serializes to lowercase strings for redb storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VisualizationMode {
    Off,
    #[default]
    Bars,
    Lines,
}

impl VisualizationMode {
    /// Cycle to the next visualization mode.
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Bars,
            Self::Bars => Self::Lines,
            Self::Lines => Self::Off,
        }
    }
}

impl std::fmt::Display for VisualizationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Off => write!(f, "off"),
            Self::Bars => write!(f, "bars"),
            Self::Lines => write!(f, "lines"),
        }
    }
}

/// What happens when pressing Enter on a song in the Songs view.
///
/// Serializes to snake_case strings for redb storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnterBehavior {
    /// Replace queue with all songs in the current view, play from selected index
    #[default]
    PlayAll,
    /// Replace queue with just the selected song
    PlaySingle,
    /// Append the selected song to the existing queue and start playing it
    AppendAndPlay,
}

impl EnterBehavior {
    /// Convert from settings GUI label to enum variant
    pub fn from_label(label: &str) -> Self {
        match label {
            "Play Single" => Self::PlaySingle,
            "Append & Play" => Self::AppendAndPlay,
            _ => Self::PlayAll,
        }
    }

    /// Convert to settings GUI label
    pub fn as_label(self) -> &'static str {
        match self {
            Self::PlayAll => "Play All",
            Self::PlaySingle => "Play Single",
            Self::AppendAndPlay => "Append & Play",
        }
    }
}

/// Navigation layout mode — controls where the view tabs are displayed.
///
/// Serializes to lowercase strings for redb storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NavLayout {
    /// Navigation tabs in the horizontal top bar (default)
    #[default]
    Top,
    /// Navigation tabs in a vertical sidebar on the left
    Side,
    /// No navigation chrome — only the active page and player bar are rendered
    None,
}

impl NavLayout {
    /// Convert from settings GUI label to enum variant
    pub fn from_label(label: &str) -> Self {
        match label {
            "Side" => Self::Side,
            "None" => Self::None,
            _ => Self::Top,
        }
    }

    /// Convert to settings GUI label
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Top => "Top",
            Self::Side => "Side",
            Self::None => "None",
        }
    }
}

impl std::fmt::Display for NavLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Top => write!(f, "top"),
            Self::Side => write!(f, "side"),
            Self::None => write!(f, "none"),
        }
    }
}

/// Navigation display mode — controls what content is shown in navigation tabs.
///
/// Serializes to snake_case strings for redb storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavDisplayMode {
    /// Show only text labels (default)
    #[default]
    TextOnly,
    /// Show icons alongside text labels
    TextAndIcons,
    /// Show only icons (no text)
    IconsOnly,
}

impl NavDisplayMode {
    /// Convert from settings GUI label to enum variant
    pub fn from_label(label: &str) -> Self {
        match label {
            "Text + Icons" => Self::TextAndIcons,
            "Icons Only" => Self::IconsOnly,
            _ => Self::TextOnly,
        }
    }

    /// Convert to settings GUI label
    pub fn as_label(self) -> &'static str {
        match self {
            Self::TextOnly => "Text Only",
            Self::TextAndIcons => "Text + Icons",
            Self::IconsOnly => "Icons Only",
        }
    }
}

impl std::fmt::Display for NavDisplayMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TextOnly => write!(f, "text_only"),
            Self::TextAndIcons => write!(f, "text_and_icons"),
            Self::IconsOnly => write!(f, "icons_only"),
        }
    }
}

/// Track info display mode — controls where now-playing track metadata is shown.
///
/// Serializes to snake_case strings for redb storage.
/// Legacy `true`/`false` values are handled via serde alias on the settings field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackInfoDisplay {
    /// No track info strip (default)
    #[default]
    Off,
    /// Track info strip in the player bar (bottom)
    PlayerBar,
    /// Track info strip at the top of the window (side nav only)
    TopBar,
    /// Scrolling metadata overlay on the progress bar track
    ProgressTrack,
}

impl TrackInfoDisplay {
    /// Convert from settings GUI label to enum variant
    pub fn from_label(label: &str) -> Self {
        match label {
            "Player Bar" => Self::PlayerBar,
            "Top Bar" => Self::TopBar,
            "Progress Track" => Self::ProgressTrack,
            _ => Self::Off,
        }
    }

    /// Convert to settings GUI label
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::PlayerBar => "Player Bar",
            Self::TopBar => "Top Bar",
            Self::ProgressTrack => "Progress Track",
        }
    }
}

impl std::fmt::Display for TrackInfoDisplay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Off => write!(f, "off"),
            Self::PlayerBar => write!(f, "player_bar"),
            Self::TopBar => write!(f, "top_bar"),
            Self::ProgressTrack => write!(f, "progress_track"),
        }
    }
}

/// Strip click action — controls what happens when clicking the track info strip.
///
/// Serializes to snake_case strings for redb storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StripClickAction {
    /// Navigate to the Queue view (default)
    #[default]
    GoToQueue,
    /// Navigate to the album expansion for the currently playing track
    GoToAlbum,
    /// Navigate to the artist expansion for the currently playing track
    GoToArtist,
    /// Copy "Artist — Title" to the system clipboard
    CopyTrackInfo,
    /// No action — passive display
    DoNothing,
}

impl StripClickAction {
    /// Convert from settings GUI label to enum variant
    pub fn from_label(label: &str) -> Self {
        match label {
            "Go to Album" => Self::GoToAlbum,
            "Go to Artist" => Self::GoToArtist,
            "Copy Track Info" => Self::CopyTrackInfo,
            "Do Nothing" => Self::DoNothing,
            _ => Self::GoToQueue,
        }
    }

    /// Convert to settings GUI label
    pub fn as_label(self) -> &'static str {
        match self {
            Self::GoToQueue => "Go to Queue",
            Self::GoToAlbum => "Go to Album",
            Self::GoToArtist => "Go to Artist",
            Self::CopyTrackInfo => "Copy Track Info",
            Self::DoNothing => "Do Nothing",
        }
    }
}

impl std::fmt::Display for StripClickAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GoToQueue => write!(f, "go_to_queue"),
            Self::GoToAlbum => write!(f, "go_to_album"),
            Self::GoToArtist => write!(f, "go_to_artist"),
            Self::CopyTrackInfo => write!(f, "copy_track_info"),
            Self::DoNothing => write!(f, "do_nothing"),
        }
    }
}

/// Slot list row density — controls the target row height for all slot lists.
///
/// Each variant is spaced far enough apart (~20px) to guarantee a different
/// slot count at any reasonable window height, eliminating the dead-zone
/// problem of the old continuous slider.
///
/// Serializes to lowercase strings for redb storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SlotRowHeight {
    /// Maximum density — smallest comfortable rows (50px target)
    Compact,
    /// Balanced (70px target)
    #[default]
    Default,
    /// Fewer, taller rows (90px target)
    Comfortable,
    /// Maximum row height (110px target)
    Spacious,
}

impl SlotRowHeight {
    /// Target pixel height for this density level.
    pub fn to_pixels(self) -> u8 {
        match self {
            Self::Compact => 50,
            Self::Default => 70,
            Self::Comfortable => 90,
            Self::Spacious => 110,
        }
    }

    /// Convert from settings GUI label to enum variant.
    pub fn from_label(label: &str) -> Self {
        match label {
            "Compact" => Self::Compact,
            "Comfortable" => Self::Comfortable,
            "Spacious" => Self::Spacious,
            _ => Self::Default,
        }
    }

    /// Convert to settings GUI label.
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Default => "Default",
            Self::Comfortable => "Comfortable",
            Self::Spacious => "Spacious",
        }
    }
}

impl std::fmt::Display for SlotRowHeight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Compact => write!(f, "compact"),
            Self::Default => write!(f, "default"),
            Self::Comfortable => write!(f, "comfortable"),
            Self::Spacious => write!(f, "spacious"),
        }
    }
}

/// Volume normalization level — controls the AGC target loudness.
///
/// Maps to rodio's `AutomaticGainControlSettings::target_level`.
/// Serializes to lowercase strings for redb storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NormalizationLevel {
    /// Reduced loudness, maximum headroom (target_level = 0.6)
    Quiet,
    /// Maintain original perceived level (target_level = 1.0)
    #[default]
    Normal,
    /// Boost quiet tracks more aggressively (target_level = 1.4)
    Loud,
}

impl NormalizationLevel {
    /// AGC target level for this normalization level.
    pub fn target_level(self) -> f32 {
        match self {
            Self::Quiet => 0.6,
            Self::Normal => 1.0,
            Self::Loud => 1.4,
        }
    }

    /// Convert from settings GUI label to enum variant.
    pub fn from_label(label: &str) -> Self {
        match label {
            "Quiet" => Self::Quiet,
            "Loud" => Self::Loud,
            _ => Self::Normal,
        }
    }

    /// Convert to settings GUI label.
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Quiet => "Quiet",
            Self::Normal => "Normal",
            Self::Loud => "Loud",
        }
    }
}

impl std::fmt::Display for NormalizationLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Quiet => write!(f, "quiet"),
            Self::Normal => write!(f, "normal"),
            Self::Loud => write!(f, "loud"),
        }
    }
}

/// Volume normalization mode — selects between off, real-time AGC, or
/// static ReplayGain (track or album scope).
///
/// AGC is rodio's `automatic_gain_control` source; ReplayGain modes use
/// pre-computed loudness tags (`replay_gain.track_gain` /
/// `replay_gain.album_gain`) read from the Subsonic API.
///
/// Serializes to snake_case strings for redb storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VolumeNormalizationMode {
    /// No normalization. Decoded audio passes through unchanged.
    #[default]
    Off,
    /// Real-time automatic gain control.
    Agc,
    /// Static gain from per-track ReplayGain tag.
    ReplayGainTrack,
    /// Static gain from per-album ReplayGain tag (preserves within-album dynamics).
    ReplayGainAlbum,
}

impl VolumeNormalizationMode {
    pub fn is_off(self) -> bool {
        matches!(self, Self::Off)
    }

    pub fn is_agc(self) -> bool {
        matches!(self, Self::Agc)
    }

    pub fn is_replay_gain(self) -> bool {
        matches!(self, Self::ReplayGainTrack | Self::ReplayGainAlbum)
    }

    pub fn prefers_album(self) -> bool {
        matches!(self, Self::ReplayGainAlbum)
    }

    /// Convert from settings GUI label to enum variant.
    pub fn from_label(label: &str) -> Self {
        match label {
            "AGC" => Self::Agc,
            "ReplayGain (Track)" => Self::ReplayGainTrack,
            "ReplayGain (Album)" => Self::ReplayGainAlbum,
            _ => Self::Off,
        }
    }

    /// Convert to settings GUI label.
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Agc => "AGC",
            Self::ReplayGainTrack => "ReplayGain (Track)",
            Self::ReplayGainAlbum => "ReplayGain (Album)",
        }
    }
}

impl std::fmt::Display for VolumeNormalizationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Off => write!(f, "off"),
            Self::Agc => write!(f, "agc"),
            Self::ReplayGainTrack => write!(f, "replay_gain_track"),
            Self::ReplayGainAlbum => write!(f, "replay_gain_album"),
        }
    }
}

/// Artwork resolution for the large artwork panel.
///
/// Controls what size image is requested from Navidrome for the artwork panel.
/// Higher resolutions look sharper on HiDPI/4K displays but consume more disk
/// cache space. Navidrome performs high-quality Lanczos resampling server-side.
///
/// Serializes to lowercase strings for redb storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtworkResolution {
    /// Standard quality (1000px) — matches 1080p/1440p panels
    #[default]
    Default,
    /// High quality for HiDPI displays (1500px)
    High,
    /// Ultra quality for 4K displays (2000px)
    Ultra,
    /// Server original — no resize, max fidelity, large cache
    Original,
}

impl ArtworkResolution {
    /// Convert to the pixel size to request from the server.
    ///
    /// Returns `None` for `Original` — meaning "don't pass a size parameter"
    /// so Navidrome sends the unresized source image.
    pub fn to_size(self) -> Option<u32> {
        match self {
            Self::Default => Some(1000),
            Self::High => Some(1500),
            Self::Ultra => Some(2000),
            Self::Original => None,
        }
    }

    /// Convert from settings GUI label to enum variant.
    pub fn from_label(label: &str) -> Self {
        match label {
            "High (1500px)" => Self::High,
            "Ultra (2000px)" => Self::Ultra,
            "Original (Full Size)" => Self::Original,
            _ => Self::Default,
        }
    }

    /// Convert to settings GUI label.
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Default => "Default (1000px)",
            Self::High => "High (1500px)",
            Self::Ultra => "Ultra (2000px)",
            Self::Original => "Original (Full Size)",
        }
    }
}

impl std::fmt::Display for ArtworkResolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Default => write!(f, "default"),
            Self::High => write!(f, "high"),
            Self::Ultra => write!(f, "ultra"),
            Self::Original => write!(f, "original"),
        }
    }
}

/// Artwork column display mode — controls visibility and sizing of the
/// large artwork column rendered alongside slot lists in albums/songs/queue/
/// artists/genres/playlists/similar views.
///
/// Serializes to snake_case strings for redb storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtworkColumnMode {
    /// Width derived from window size; column auto-hides when leftover slot
    /// list width drops below 800px. Panel always square. (Default.)
    #[default]
    Auto,
    /// Column has a user-defined width; image stays square inside it,
    /// letterboxed vertically when the column is taller than wide.
    AlwaysNative,
    /// Column has a user-defined width; image fills the column non-square
    /// using the configured fit mode (Cover or Fill).
    AlwaysStretched,
    /// Column hidden everywhere.
    Never,
}

impl ArtworkColumnMode {
    /// Convert from settings GUI label to enum variant.
    pub fn from_label(label: &str) -> Self {
        match label {
            "Always (Native)" => Self::AlwaysNative,
            "Always (Stretched)" => Self::AlwaysStretched,
            "Never" => Self::Never,
            _ => Self::Auto,
        }
    }

    /// Convert to settings GUI label.
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::AlwaysNative => "Always (Native)",
            Self::AlwaysStretched => "Always (Stretched)",
            Self::Never => "Never",
        }
    }
}

impl std::fmt::Display for ArtworkColumnMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => write!(f, "auto"),
            Self::AlwaysNative => write!(f, "always_native"),
            Self::AlwaysStretched => write!(f, "always_stretched"),
            Self::Never => write!(f, "never"),
        }
    }
}

/// Fit mode for `ArtworkColumnMode::AlwaysStretched` — picks how the image
/// fills the non-square column. Other modes ignore this value.
///
/// Serializes to lowercase strings for redb storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtworkStretchFit {
    /// `iced::ContentFit::Cover` — preserves aspect ratio, crops to fill.
    #[default]
    Cover,
    /// `iced::ContentFit::Fill` — true stretch, distorts album art.
    Fill,
}

impl ArtworkStretchFit {
    /// Convert from settings GUI label to enum variant.
    pub fn from_label(label: &str) -> Self {
        match label {
            "Fill" => Self::Fill,
            _ => Self::Cover,
        }
    }

    /// Convert to settings GUI label.
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Cover => "Cover",
            Self::Fill => "Fill",
        }
    }
}

impl std::fmt::Display for ArtworkStretchFit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cover => write!(f, "cover"),
            Self::Fill => write!(f, "fill"),
        }
    }
}

/// Player settings loaded from persistence (redb).
///
/// Note: `light_mode` is stored in config.toml, not redb.
/// See `theme_config::load_light_mode_from_config()`.
#[derive(Debug, Clone)]
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

    // -- Albums view column toggles --
    pub albums_show_stars: bool,
    pub albums_show_songcount: bool,
    pub albums_show_plays: bool,
    pub albums_show_love: bool,

    // -- Songs view column toggles --
    pub songs_show_stars: bool,
    pub songs_show_album: bool,
    pub songs_show_duration: bool,
    pub songs_show_plays: bool,
    pub songs_show_love: bool,

    // -- Artists view column toggles --
    pub artists_show_stars: bool,
    pub artists_show_albumcount: bool,
    pub artists_show_songcount: bool,
    pub artists_show_plays: bool,
    pub artists_show_love: bool,

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

    // -- System tray --
    /// Whether to register a StatusNotifierItem tray icon on the session bus.
    pub show_tray_icon: bool,
    /// When true and `show_tray_icon` is on, the window's close button hides
    /// the window into the tray instead of quitting the application.
    pub close_to_tray: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visualization_mode_cycles() {
        assert_eq!(VisualizationMode::Off.next(), VisualizationMode::Bars);
        assert_eq!(VisualizationMode::Bars.next(), VisualizationMode::Lines);
        assert_eq!(VisualizationMode::Lines.next(), VisualizationMode::Off);
    }

    #[test]
    fn visualization_mode_default_is_bars() {
        assert_eq!(VisualizationMode::default(), VisualizationMode::Bars);
    }

    #[test]
    fn visualization_mode_serde_roundtrip() {
        let modes = [
            VisualizationMode::Off,
            VisualizationMode::Bars,
            VisualizationMode::Lines,
        ];
        for mode in modes {
            let json = serde_json::to_string(&mode).unwrap();
            let deserialized: VisualizationMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, deserialized);
        }
    }

    #[test]
    fn visualization_mode_deserializes_from_lowercase_strings() {
        assert_eq!(
            serde_json::from_str::<VisualizationMode>("\"off\"").unwrap(),
            VisualizationMode::Off
        );
        assert_eq!(
            serde_json::from_str::<VisualizationMode>("\"bars\"").unwrap(),
            VisualizationMode::Bars
        );
        assert_eq!(
            serde_json::from_str::<VisualizationMode>("\"lines\"").unwrap(),
            VisualizationMode::Lines
        );
    }

    #[test]
    fn volume_normalization_mode_default_is_off() {
        assert_eq!(
            VolumeNormalizationMode::default(),
            VolumeNormalizationMode::Off
        );
    }

    #[test]
    fn volume_normalization_mode_serde_roundtrip() {
        let modes = [
            VolumeNormalizationMode::Off,
            VolumeNormalizationMode::Agc,
            VolumeNormalizationMode::ReplayGainTrack,
            VolumeNormalizationMode::ReplayGainAlbum,
        ];
        for mode in modes {
            let json = serde_json::to_string(&mode).unwrap();
            let deserialized: VolumeNormalizationMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, deserialized);
        }
    }

    #[test]
    fn volume_normalization_mode_serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&VolumeNormalizationMode::Off).unwrap(),
            "\"off\""
        );
        assert_eq!(
            serde_json::to_string(&VolumeNormalizationMode::Agc).unwrap(),
            "\"agc\""
        );
        assert_eq!(
            serde_json::to_string(&VolumeNormalizationMode::ReplayGainTrack).unwrap(),
            "\"replay_gain_track\""
        );
        assert_eq!(
            serde_json::to_string(&VolumeNormalizationMode::ReplayGainAlbum).unwrap(),
            "\"replay_gain_album\""
        );
    }

    #[test]
    fn volume_normalization_mode_label_roundtrip() {
        for mode in [
            VolumeNormalizationMode::Off,
            VolumeNormalizationMode::Agc,
            VolumeNormalizationMode::ReplayGainTrack,
            VolumeNormalizationMode::ReplayGainAlbum,
        ] {
            assert_eq!(VolumeNormalizationMode::from_label(mode.as_label()), mode);
        }
    }

    #[test]
    fn volume_normalization_mode_classifiers() {
        assert!(VolumeNormalizationMode::Off.is_off());
        assert!(VolumeNormalizationMode::Agc.is_agc());
        assert!(VolumeNormalizationMode::ReplayGainTrack.is_replay_gain());
        assert!(VolumeNormalizationMode::ReplayGainAlbum.is_replay_gain());
        assert!(VolumeNormalizationMode::ReplayGainAlbum.prefers_album());
        assert!(!VolumeNormalizationMode::ReplayGainTrack.prefers_album());
    }

    #[test]
    fn enter_behavior_default_is_play_all() {
        assert_eq!(EnterBehavior::default(), EnterBehavior::PlayAll);
    }

    #[test]
    fn enter_behavior_serde_roundtrip() {
        let behaviors = [
            EnterBehavior::PlayAll,
            EnterBehavior::PlaySingle,
            EnterBehavior::AppendAndPlay,
        ];
        for behavior in behaviors {
            let json = serde_json::to_string(&behavior).unwrap();
            let deserialized: EnterBehavior = serde_json::from_str(&json).unwrap();
            assert_eq!(behavior, deserialized);
        }
    }

    #[test]
    fn enter_behavior_label_roundtrip() {
        for behavior in [
            EnterBehavior::PlayAll,
            EnterBehavior::PlaySingle,
            EnterBehavior::AppendAndPlay,
        ] {
            assert_eq!(EnterBehavior::from_label(behavior.as_label()), behavior);
        }
    }

    #[test]
    fn nav_layout_default_is_top() {
        assert_eq!(NavLayout::default(), NavLayout::Top);
    }

    #[test]
    fn nav_layout_serde_roundtrip() {
        let layouts = [NavLayout::Top, NavLayout::Side, NavLayout::None];
        for layout in layouts {
            let json = serde_json::to_string(&layout).unwrap();
            let deserialized: NavLayout = serde_json::from_str(&json).unwrap();
            assert_eq!(layout, deserialized);
        }
    }

    #[test]
    fn nav_layout_label_roundtrip() {
        for layout in [NavLayout::Top, NavLayout::Side, NavLayout::None] {
            assert_eq!(NavLayout::from_label(layout.as_label()), layout);
        }
    }

    #[test]
    fn nav_display_mode_default_is_text_only() {
        assert_eq!(NavDisplayMode::default(), NavDisplayMode::TextOnly);
    }

    #[test]
    fn nav_display_mode_serde_roundtrip() {
        let modes = [
            NavDisplayMode::TextOnly,
            NavDisplayMode::TextAndIcons,
            NavDisplayMode::IconsOnly,
        ];
        for mode in modes {
            let json = serde_json::to_string(&mode).unwrap();
            let deserialized: NavDisplayMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, deserialized);
        }
    }

    #[test]
    fn nav_display_mode_label_roundtrip() {
        for mode in [
            NavDisplayMode::TextOnly,
            NavDisplayMode::TextAndIcons,
            NavDisplayMode::IconsOnly,
        ] {
            assert_eq!(NavDisplayMode::from_label(mode.as_label()), mode);
        }
    }
}
