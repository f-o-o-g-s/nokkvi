//! Artwork settings — resolution, column display mode, stretch fit.
//!
//! Single source of truth for default and clamp ranges of the three artwork
//! percent knobs. Every consumer (serde defaults in `types/settings.rs` and
//! `types/toml_settings.rs`, the `SettingsManager` setter clamps in
//! `services/settings.rs`, the `define_settings!` ui_meta in
//! `services/settings_tables/interface.rs`, the UI-crate theme atomic init
//! and clamps in `src/theme.rs`) must reference these constants.

use serde::{Deserialize, Serialize};

use crate::define_labeled_enum;

// Column-width slider (drives `AlwaysNative` / `AlwaysStretched` modes).
pub const ARTWORK_COLUMN_WIDTH_PCT_DEFAULT: f32 = 0.40;
pub const ARTWORK_COLUMN_WIDTH_PCT_MIN: f32 = 0.05;
pub const ARTWORK_COLUMN_WIDTH_PCT_MAX: f32 = 0.80;

// Auto-mode max-percent slider.
pub const ARTWORK_AUTO_MAX_PCT_DEFAULT: f32 = 0.40;
pub const ARTWORK_AUTO_MAX_PCT_MIN: f32 = 0.30;
pub const ARTWORK_AUTO_MAX_PCT_MAX: f32 = 0.70;

// Always-Vertical height slider (drives `AlwaysVerticalNative` /
// `AlwaysVerticalStretched` modes).
pub const ARTWORK_VERTICAL_HEIGHT_PCT_DEFAULT: f32 = 0.40;
pub const ARTWORK_VERTICAL_HEIGHT_PCT_MIN: f32 = 0.10;
pub const ARTWORK_VERTICAL_HEIGHT_PCT_MAX: f32 = 0.80;

define_labeled_enum! {
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
        Default { label: "Default (1000px)", wire: "default" },
        /// High quality for HiDPI displays (1500px)
        High { label: "High (1500px)", wire: "high" },
        /// Ultra quality for 4K displays (2000px)
        Ultra { label: "Ultra (2000px)", wire: "ultra" },
        /// Server original — no resize, max fidelity, large cache
        Original { label: "Original (Full Size)", wire: "original" },
    }
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
}

define_labeled_enum! {
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
        Auto { label: "Auto", wire: "auto" },
        /// Right-hand column has a user-defined width; image stays square inside
        /// it, letterboxed vertically when the column is taller than wide.
        AlwaysNative { label: "Always (Native)", wire: "always_native" },
        /// Right-hand column has a user-defined width; image fills the column
        /// non-square using the configured fit mode (Cover or Fill).
        AlwaysStretched { label: "Always (Stretched)", wire: "always_stretched" },
        /// Artwork stacked above the slot list with a user-defined height
        /// (`artwork_vertical_height_pct`). Image stays square inside the
        /// allotted rect; letterboxing is allowed (user opted into vertical).
        AlwaysVerticalNative {
            label: "Always (Vertical Native)",
            wire: "always_vertical_native",
        },
        /// Artwork stacked above the slot list with a user-defined height; image
        /// fills the allotted rect via the configured stretch fit. Letterboxing
        /// of source pixels happens inside the image via Cover/Fill rather than
        /// inside the panel via bg0_soft bars.
        AlwaysVerticalStretched {
            label: "Always (Vertical Stretched)",
            wire: "always_vertical_stretched",
        },
        /// Column hidden everywhere.
        Never { label: "Never", wire: "never" },
    }
}

impl ArtworkColumnMode {
    /// True for any "Stretched" variant — the image fills the panel via the
    /// configured `ArtworkStretchFit` (Cover or Fill) rather than being
    /// rendered square with letterboxing.
    pub fn is_stretched(self) -> bool {
        matches!(self, Self::AlwaysStretched | Self::AlwaysVerticalStretched)
    }

    /// True for any "Vertical" variant — artwork stacks above the slot list
    /// instead of sitting to its right.
    pub fn is_vertical(self) -> bool {
        matches!(
            self,
            Self::AlwaysVerticalNative | Self::AlwaysVerticalStretched
        )
    }

    /// True for any non-vertical "Always" variant — artwork sits to the
    /// right of the slot list at a user-defined column width.
    pub fn is_always_horizontal(self) -> bool {
        matches!(self, Self::AlwaysNative | Self::AlwaysStretched)
    }

    /// True for any "Always" variant (horizontal or vertical, native or
    /// stretched) — i.e. the artwork panel is forced visible and the resize
    /// handle is drawn.
    pub fn is_always_visible(self) -> bool {
        self.is_always_horizontal() || self.is_vertical()
    }
}

define_labeled_enum! {
    /// Fit mode for `ArtworkColumnMode::AlwaysStretched` — picks how the image
    /// fills the non-square column. Other modes ignore this value.
    ///
    /// Serializes to lowercase strings for redb storage.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum ArtworkStretchFit {
        /// `iced::ContentFit::Cover` — preserves aspect ratio, crops to fill.
        #[default]
        Cover { label: "Cover", wire: "cover" },
        /// `iced::ContentFit::Fill` — true stretch, distorts album art.
        Fill { label: "Fill", wire: "fill" },
    }
}
