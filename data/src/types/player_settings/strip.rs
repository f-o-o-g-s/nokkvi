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
        /// Track info strip rendered in its own row directly beneath the
        /// nav bar (top-nav layout) — uses the same separator-above
        /// styling as the player-bar strip instead of being merged into
        /// the nav row. In side-nav and none-nav layouts (where there
        /// is no horizontal nav row), behaves the same as `TopBar`:
        /// the strip sits above the main content.
        TopBarUnder { label: "Top Bar Under", wire: "top_bar_under" },
        /// Artwork + title/artist/album stacked to the left of the
        /// transport controls in the player bar (an inline track-info
        /// column instead of a separate strip).
        ///
        /// `#[serde(alias = "progress_track")]` keeps backwards
        /// compatibility with older TOML files written before the
        /// rename (the variant used to be called `ProgressTrack` when
        /// it rendered as a scrolling overlay on the progress-bar
        /// track).
        #[serde(alias = "progress_track")]
        MiniPlayer { label: "Mini Player", wire: "mini_player" },
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
        /// Middle dot · (matches the historical hardcoded join)
        Dot { label: "Dot ·", wire: "dot" },
        /// Bullet •
        Bullet { label: "Bullet •", wire: "bullet" },
        /// Pipe |
        Pipe { label: "Pipe |", wire: "pipe" },
        /// Em dash —
        EmDash { label: "Em dash —", wire: "em_dash" },
        /// Slash / (default — aligns the enum default with the shipped
        /// struct/persisted default chosen in the first-launch-UX retune;
        /// keeps an absent `strip_separator` key reading back as Slash)
        #[default]
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped default separator is Slash — every struct/persisted default
    /// (`TomlSettings::default()`, `PersistedPlayerSettings::default()`) uses it.
    /// Aligning the enum `#[default]` to Slash makes `StripSeparator::default()`
    /// agree with the on-disk default, so an absent `strip_separator` key (e.g.
    /// after a sparse-config strip) reads back as Slash instead of silently
    /// flipping to Dot.
    #[test]
    fn strip_separator_default_is_slash() {
        assert_eq!(StripSeparator::default(), StripSeparator::Slash);
    }

    /// Pin the `#[serde(alias = "progress_track")]` migration path.
    ///
    /// The variant was renamed from `ProgressTrack` to `MiniPlayer` during
    /// the redesign (the strip used to render as a scrolling overlay on
    /// the progress-bar track; the new design pulls the artwork/metadata
    /// into a dedicated section of the player bar). Pre-redesign TOML
    /// configs persist the wire string `"progress_track"`. Without the
    /// alias the deserializer would fail; the test fires loud if a
    /// future serde update drops alias support or a refactor removes
    /// the attribute.
    #[test]
    fn track_info_display_deserializes_progress_track_as_mini_player() {
        let mode: TrackInfoDisplay = serde_json::from_str("\"progress_track\"").unwrap();
        assert_eq!(mode, TrackInfoDisplay::MiniPlayer);
    }

    /// And the canonical `"mini_player"` wire string still works too — the
    /// alias must not displace the primary deserialization name.
    #[test]
    fn track_info_display_deserializes_mini_player_as_mini_player() {
        let mode: TrackInfoDisplay = serde_json::from_str("\"mini_player\"").unwrap();
        assert_eq!(mode, TrackInfoDisplay::MiniPlayer);
    }

    /// `MiniPlayer` serializes back as `"mini_player"` (not the alias) so
    /// configs written by the current build round-trip cleanly.
    #[test]
    fn track_info_display_serializes_mini_player_as_canonical_wire() {
        let wire = serde_json::to_string(&TrackInfoDisplay::MiniPlayer).unwrap();
        assert_eq!(wire, "\"mini_player\"");
    }
}
