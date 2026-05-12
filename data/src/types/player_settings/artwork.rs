//! Artwork settings — resolution, column display mode, stretch fit.
//!
//! Single source of truth for default and clamp ranges of the three artwork
//! percent knobs. Every consumer (serde defaults in `types/settings.rs` and
//! `types/toml_settings.rs`, the `SettingsManager` setter clamps in
//! `services/settings.rs`, the `define_settings!` ui_meta in
//! `services/settings_tables/interface.rs`, the UI-crate theme atomic init
//! and clamps in `src/theme.rs`) must reference these constants.

use serde::{Deserialize, Serialize};

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
    /// Right-hand column has a user-defined width; image stays square inside
    /// it, letterboxed vertically when the column is taller than wide.
    AlwaysNative,
    /// Right-hand column has a user-defined width; image fills the column
    /// non-square using the configured fit mode (Cover or Fill).
    AlwaysStretched,
    /// Artwork stacked above the slot list with a user-defined height
    /// (`artwork_vertical_height_pct`). Image stays square inside the
    /// allotted rect; letterboxing is allowed (user opted into vertical).
    AlwaysVerticalNative,
    /// Artwork stacked above the slot list with a user-defined height; image
    /// fills the allotted rect via the configured stretch fit. Letterboxing
    /// of source pixels happens inside the image via Cover/Fill rather than
    /// inside the panel via bg0_soft bars.
    AlwaysVerticalStretched,
    /// Column hidden everywhere.
    Never,
}

impl ArtworkColumnMode {
    /// Convert from settings GUI label to enum variant.
    pub fn from_label(label: &str) -> Self {
        match label {
            "Always (Native)" => Self::AlwaysNative,
            "Always (Stretched)" => Self::AlwaysStretched,
            "Always (Vertical Native)" => Self::AlwaysVerticalNative,
            "Always (Vertical Stretched)" => Self::AlwaysVerticalStretched,
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
            Self::AlwaysVerticalNative => "Always (Vertical Native)",
            Self::AlwaysVerticalStretched => "Always (Vertical Stretched)",
            Self::Never => "Never",
        }
    }

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

impl std::fmt::Display for ArtworkColumnMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => write!(f, "auto"),
            Self::AlwaysNative => write!(f, "always_native"),
            Self::AlwaysStretched => write!(f, "always_stretched"),
            Self::AlwaysVerticalNative => write!(f, "always_vertical_native"),
            Self::AlwaysVerticalStretched => write!(f, "always_vertical_stretched"),
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
