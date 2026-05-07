//! Navigation chrome settings — layout (top/side/none) and display mode.

use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::*;

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
