//! Artwork settings — resolution, column display mode, stretch fit.

use serde::{Deserialize, Serialize};

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
