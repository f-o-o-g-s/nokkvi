//! Track-info strip settings — display location, click action, separator.

use serde::{Deserialize, Serialize};

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

/// Visual character used to separate fields in the metadata strip's merged
/// scrolling unit (`title:` / `artist:` / `album:` joined into one marquee).
///
/// Serializes to snake_case strings for TOML storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StripSeparator {
    /// Middle dot · (default — matches historical hardcoded join)
    #[default]
    Dot,
    /// Bullet •
    Bullet,
    /// Pipe |
    Pipe,
    /// Em dash —
    EmDash,
    /// Slash /
    Slash,
    /// Box-drawing vertical bar │ (matches the strip's bookend dividers)
    Bar,
}

impl StripSeparator {
    /// Returns the rendered string used to join visible fields. Includes the
    /// surrounding spaces so the join is visually balanced.
    pub fn as_join_str(self) -> &'static str {
        match self {
            Self::Dot => "  ·  ",
            Self::Bullet => "  •  ",
            Self::Pipe => "  |  ",
            Self::EmDash => "  —  ",
            Self::Slash => "  /  ",
            Self::Bar => "  │  ",
        }
    }

    /// Convert from settings GUI label to enum variant.
    pub fn from_label(label: &str) -> Self {
        match label {
            "Bullet •" => Self::Bullet,
            "Pipe |" => Self::Pipe,
            "Em dash —" => Self::EmDash,
            "Slash /" => Self::Slash,
            "Bar │" => Self::Bar,
            _ => Self::Dot,
        }
    }

    /// Convert to settings GUI label.
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Dot => "Dot ·",
            Self::Bullet => "Bullet •",
            Self::Pipe => "Pipe |",
            Self::EmDash => "Em dash —",
            Self::Slash => "Slash /",
            Self::Bar => "Bar │",
        }
    }
}

impl std::fmt::Display for StripSeparator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dot => write!(f, "dot"),
            Self::Bullet => write!(f, "bullet"),
            Self::Pipe => write!(f, "pipe"),
            Self::EmDash => write!(f, "em_dash"),
            Self::Slash => write!(f, "slash"),
            Self::Bar => write!(f, "bar"),
        }
    }
}
