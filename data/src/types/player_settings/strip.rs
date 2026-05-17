//! Track-info strip settings — display location, click action, separator.

use serde::{Deserialize, Serialize};

use crate::define_labeled_enum;

define_labeled_enum! {
    /// Track info display mode — controls where now-playing track metadata is shown.
    ///
    /// Serializes to snake_case strings for redb storage.
    /// Legacy `true`/`false` values are handled via serde alias on the settings field.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum TrackInfoDisplay {
        /// No track info strip (default)
        #[default]
        Off { label: "Off", wire: "off" },
        /// Track info strip in the player bar (bottom)
        PlayerBar { label: "Player Bar", wire: "player_bar" },
        /// Track info strip at the top of the window (side nav only)
        TopBar { label: "Top Bar", wire: "top_bar" },
        /// Scrolling metadata overlay on the progress bar track
        ProgressTrack { label: "Progress Track", wire: "progress_track" },
    }
}

define_labeled_enum! {
    /// Strip click action — controls what happens when clicking the track info strip.
    ///
    /// Serializes to snake_case strings for redb storage.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum StripClickAction {
        /// Navigate to the Queue view (default)
        #[default]
        GoToQueue { label: "Go to Queue", wire: "go_to_queue" },
        /// Navigate to the album expansion for the currently playing track
        GoToAlbum { label: "Go to Album", wire: "go_to_album" },
        /// Navigate to the artist expansion for the currently playing track
        GoToArtist { label: "Go to Artist", wire: "go_to_artist" },
        /// Copy "Artist — Title" to the system clipboard
        CopyTrackInfo { label: "Copy Track Info", wire: "copy_track_info" },
        /// No action — passive display
        DoNothing { label: "Do Nothing", wire: "do_nothing" },
    }
}

define_labeled_enum! {
    /// Visual character used to separate fields in the metadata strip's merged
    /// scrolling unit (`title:` / `artist:` / `album:` joined into one marquee).
    ///
    /// Serializes to snake_case strings for TOML storage.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum StripSeparator {
        /// Middle dot · (default — matches historical hardcoded join)
        #[default]
        Dot { label: "Dot ·", wire: "dot" },
        /// Bullet •
        Bullet { label: "Bullet •", wire: "bullet" },
        /// Pipe |
        Pipe { label: "Pipe |", wire: "pipe" },
        /// Em dash —
        EmDash { label: "Em dash —", wire: "em_dash" },
        /// Slash /
        Slash { label: "Slash /", wire: "slash" },
        /// Box-drawing vertical bar │ (matches the strip's bookend dividers)
        Bar { label: "Bar │", wire: "bar" },
    }
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
}
