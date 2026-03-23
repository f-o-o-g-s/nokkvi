use serde::{Deserialize, Serialize};

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
}

impl NavLayout {
    /// Convert from settings GUI label to enum variant
    pub fn from_label(label: &str) -> Self {
        match label {
            "Side" => Self::Side,
            _ => Self::Top,
        }
    }

    /// Convert to settings GUI label
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Top => "Top",
            Self::Side => "Side",
        }
    }
}

impl std::fmt::Display for NavLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Top => write!(f, "top"),
            Self::Side => write!(f, "side"),
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
    /// Whether volume sliders in the player bar are horizontal (default: false = vertical)
    pub horizontal_volume: bool,
    /// Whether volume normalization (AGC) is enabled
    pub volume_normalization: bool,
    /// Volume normalization target level (Quiet / Normal / Loud)
    pub normalization_level: NormalizationLevel,
    /// Whether the title field is visible in the track info strip (default: true)
    pub strip_show_title: bool,
    /// Whether the artist field is visible in the track info strip (default: true)
    pub strip_show_artist: bool,
    /// Whether the album field is visible in the track info strip (default: true)
    pub strip_show_album: bool,
    /// Whether format info (codec/kHz/kbps) is visible in the track info strip (default: true)
    pub strip_show_format_info: bool,
    /// What happens when clicking the track info strip (default: GoToQueue)
    pub strip_click_action: StripClickAction,
    /// Active playlist ID loaded in the queue (None = no playlist context)
    pub active_playlist_id: Option<String>,
    /// Active playlist display name
    pub active_playlist_name: String,
    /// Active playlist comment/description
    pub active_playlist_comment: String,
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
        let layouts = [NavLayout::Top, NavLayout::Side];
        for layout in layouts {
            let json = serde_json::to_string(&layout).unwrap();
            let deserialized: NavLayout = serde_json::from_str(&json).unwrap();
            assert_eq!(layout, deserialized);
        }
    }

    #[test]
    fn nav_layout_label_roundtrip() {
        for layout in [NavLayout::Top, NavLayout::Side] {
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
