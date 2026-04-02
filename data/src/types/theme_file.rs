//! Theme file types — color palettes and visualizer colors.
//!
//! A theme file contains ONLY aesthetic data: palette colors, visualizer colors,
//! and font family. All behavioral settings remain in `config.toml`.
//!
//! Theme files are stored at `~/.config/nokkvi/themes/{name}.toml` and are
//! always written in full (verbose mode). Built-in themes are compiled into
//! the binary and seeded to the user's themes directory on first run.

use serde::{Deserialize, Serialize};

// ============================================================================
// Theme File (top-level)
// ============================================================================

/// Complete theme file — the root struct for `{name}.toml`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ThemeFile {
    /// Human-readable theme name (shown in the UI picker)
    pub name: String,
    /// Dark mode palette + visualizer colors
    pub dark: ThemePalette,
    /// Light mode palette + visualizer colors
    pub light: ThemePalette,
}

impl Default for ThemeFile {
    fn default() -> Self {
        Self {
            name: "Gruvbox".to_string(),
            dark: ThemePalette::default(),
            light: ThemePalette::light_default(),
        }
    }
}

impl ThemeFile {
    /// Parse a theme file from TOML content.
    pub fn load(content: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(content)
    }

    /// Serialize into a TOML string.
    pub fn save(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }
}

// ============================================================================
// Palette (one per mode: dark / light)
// ============================================================================

/// A complete palette for one mode (dark or light).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ThemePalette {
    /// Background colors
    pub background: BackgroundConfig,
    /// Foreground colors
    pub foreground: ForegroundConfig,
    /// Primary accent colors
    pub accent: AccentConfig,
    /// Danger/error state colors (e.g. error text, conflict badges)
    pub danger: SemanticColorConfig,
    /// Success state colors (e.g. enabled indicators, toast success)
    pub success: SemanticColorConfig,
    /// Warning state colors (e.g. capture prompts, toast warnings)
    pub warning: SemanticColorConfig,
    /// Star rating fill colors
    pub star: SemanticColorConfig,
    /// Visualizer bar/peak/border colors
    pub visualizer: VisualizerColors,
}

impl Default for ThemePalette {
    fn default() -> Self {
        Self {
            background: BackgroundConfig::default(),
            foreground: ForegroundConfig::default(),
            accent: AccentConfig::default(),
            danger: SemanticColorConfig {
                base: "#cc241d".to_string(),
                bright: "#fb4934".to_string(),
            },
            success: SemanticColorConfig {
                base: "#98971a".to_string(),
                bright: "#b8bb26".to_string(),
            },
            warning: SemanticColorConfig {
                base: "#d79921".to_string(),
                bright: "#fabd2f".to_string(),
            },
            star: SemanticColorConfig {
                base: "#d79921".to_string(),
                bright: "#fabd2f".to_string(),
            },
            visualizer: VisualizerColors::default(),
        }
    }
}

impl ThemePalette {
    /// Gruvbox light mode defaults.
    pub fn light_default() -> Self {
        Self {
            background: BackgroundConfig {
                hard: "#e7deb7".to_string(),
                default: "#fbf1c7".to_string(),
                soft: "#f1e7bd".to_string(),
                level1: "#e4dab1".to_string(),
                level2: "#dad0a7".to_string(),
                level3: "#d2c89f".to_string(),
                level4: "#cac097".to_string(),
            },
            foreground: ForegroundConfig {
                bright: "#282828".to_string(),
                level1: "#282828".to_string(),
                level2: "#282828".to_string(),
                level3: "#282828".to_string(),
                level4: "#282828".to_string(),
                gray: "#cac097".to_string(),
            },
            accent: AccentConfig {
                primary: "#458588".to_string(),
                bright: "#49a8ac".to_string(),
                border_dark: "#49a8ac".to_string(),
                border_light: "#aebaa5".to_string(),
                now_playing: String::new(),
                selected: "#49a8ac".to_string(),
            },
            danger: SemanticColorConfig {
                base: "#cc241d".to_string(),
                bright: "#ff0000".to_string(),
            },
            success: SemanticColorConfig {
                base: "#98971a".to_string(),
                bright: "#7c7900".to_string(),
            },
            warning: SemanticColorConfig {
                base: "#d79921".to_string(),
                bright: "#b67a00".to_string(),
            },
            star: SemanticColorConfig {
                base: "#d79921".to_string(),
                bright: "#b67a00".to_string(),
            },
            visualizer: VisualizerColors::light_default(),
        }
    }
}

// ============================================================================
// Color group structs
// ============================================================================

/// Background colors (7 levels from darkest to lightest).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct BackgroundConfig {
    /// Darkest background
    pub hard: String,
    /// Default background
    pub default: String,
    /// Soft background
    pub soft: String,
    /// Background level 1
    pub level1: String,
    /// Background level 2
    pub level2: String,
    /// Background level 3
    pub level3: String,
    /// Background level 4
    pub level4: String,
}

impl Default for BackgroundConfig {
    fn default() -> Self {
        Self {
            hard: "#1e1e1e".to_string(),
            default: "#282828".to_string(),
            soft: "#2f2f2f".to_string(),
            level1: "#3a3a3a".to_string(),
            level2: "#414141".to_string(),
            level3: "#484848".to_string(),
            level4: "#4f4f4f".to_string(),
        }
    }
}

/// Foreground colors (5 levels + gray).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ForegroundConfig {
    /// Brightest foreground
    pub bright: String,
    /// Level 1 foreground
    pub level1: String,
    /// Level 2 foreground
    pub level2: String,
    /// Level 3 foreground
    pub level3: String,
    /// Level 4 foreground
    pub level4: String,
    /// Gray
    pub gray: String,
}

impl Default for ForegroundConfig {
    fn default() -> Self {
        Self {
            bright: "#fbf1c7".to_string(),
            level1: "#fbf1c7".to_string(),
            level2: "#fbf1c7".to_string(),
            level3: "#fbf1c7".to_string(),
            level4: "#fbf1c7".to_string(),
            gray: "#4f4f4f".to_string(),
        }
    }
}

/// Accent colors (primary UI accent, borders, highlights).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AccentConfig {
    /// Primary accent
    pub primary: String,
    /// Bright accent
    pub bright: String,
    /// Dark border accent
    pub border_dark: String,
    /// Light border accent
    pub border_light: String,
    /// Now-playing slot highlight (defaults to primary if empty)
    #[serde(default)]
    pub now_playing: String,
    /// Selected/center slot highlight (defaults to bright if empty)
    #[serde(default)]
    pub selected: String,
}

impl Default for AccentConfig {
    fn default() -> Self {
        Self {
            primary: "#458588".to_string(),
            bright: "#49a8ac".to_string(),
            border_dark: "#49a8ac".to_string(),
            border_light: "#3b6c6e".to_string(),
            now_playing: String::new(),
            selected: "#49a8ac".to_string(),
        }
    }
}

/// A semantic color pair (base + bright variant).
/// Used for danger, success, warning, star.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SemanticColorConfig {
    /// Base variant
    pub base: String,
    /// Bright variant
    pub bright: String,
}

impl Default for SemanticColorConfig {
    fn default() -> Self {
        // Generic fallback — matches Gruvbox danger
        Self {
            base: "#cc241d".to_string(),
            bright: "#ff0000".to_string(),
        }
    }
}

// ============================================================================
// Visualizer colors (per mode)
// ============================================================================

/// Default border opacity for dark mode (used by serde).
fn default_border_opacity() -> f32 {
    1.0
}

/// Visualizer bar/peak/border colors for one mode.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct VisualizerColors {
    /// Border color as hex string
    pub border_color: String,
    /// Border opacity for regular bars (0.0–1.0)
    #[serde(default = "default_border_opacity")]
    pub border_opacity: f32,
    /// Border opacity in LED mode (0.0–1.0)
    #[serde(default = "default_border_opacity")]
    pub led_border_opacity: f32,
    /// Gradient colors for bars (hex strings, 1–8 entries)
    pub bar_gradient_colors: Vec<String>,
    /// Gradient colors for peaks (hex strings, 1–8 entries)
    pub peak_gradient_colors: Vec<String>,
}

impl Default for VisualizerColors {
    fn default() -> Self {
        Self {
            border_color: "#1e1e1e".to_string(),
            border_opacity: 1.0,
            led_border_opacity: 1.0,
            bar_gradient_colors: vec![
                "#83a598".to_string(), // Blue
                "#8ec07c".to_string(), // Aqua
                "#b8bb26".to_string(), // Green
                "#fabd2f".to_string(), // Yellow
                "#fe8019".to_string(), // Orange
                "#fb4934".to_string(), // Red
            ],
            peak_gradient_colors: vec![
                "#83a598".to_string(),
                "#83a598".to_string(),
                "#b8bb26".to_string(),
                "#b8bb26".to_string(),
                "#fb4934".to_string(),
                "#fb4934".to_string(),
            ],
        }
    }
}

impl VisualizerColors {
    /// Gruvbox light mode default colors.
    pub fn light_default() -> Self {
        Self {
            border_color: "#e7deb7".to_string(),
            border_opacity: 0.0,
            led_border_opacity: 0.0,
            bar_gradient_colors: vec![
                "#83a598".to_string(),
                "#8ec07c".to_string(),
                "#b8bb26".to_string(),
                "#fabd2f".to_string(),
                "#fe8019".to_string(),
                "#fb4934".to_string(),
            ],
            peak_gradient_colors: vec![
                "#83a598".to_string(),
                "#83a598".to_string(),
                "#b8bb26".to_string(),
                "#b8bb26".to_string(),
                "#fb4934".to_string(),
                "#fb4934".to_string(),
            ],
        }
    }
}

// ============================================================================
// Semantic color defaults (used by ThemePalette::default)
// ============================================================================

/// The `Default` impl on `ThemePalette` uses Gruvbox dark values
/// for all semantic colors.

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_round_trip() {
        let theme = ThemeFile::default();
        let toml_str = theme.save().expect("serialize");
        let parsed = ThemeFile::load(&toml_str).expect("deserialize");
        assert_eq!(parsed.name, "Gruvbox");
        assert_eq!(parsed.dark.background.hard, "#1d2021");
        assert_eq!(parsed.light.background.hard, "#f9f5d7");
        assert_eq!(parsed.dark.visualizer.bar_gradient_colors.len(), 6);
        assert_eq!(parsed.light.visualizer.border_opacity, 0.0);
    }

    #[test]
    fn test_partial_theme_uses_defaults() {
        let partial = r##"
            name = "Minimal"
            [dark.background]
            hard = "#000000"
        "##;
        let theme = ThemeFile::load(partial).expect("parse partial");
        assert_eq!(theme.name, "Minimal");
        assert_eq!(theme.dark.background.hard, "#000000");
        // Everything else falls back to Gruvbox defaults
        assert_eq!(theme.dark.background.default, "#282828");
        assert_eq!(theme.dark.foreground.bright, "#fbf1c7");
        assert_eq!(theme.dark.accent.primary, "#458588");
    }

    #[test]
    fn test_visualizer_colors_round_trip() {
        let colors = VisualizerColors::default();
        assert_eq!(colors.bar_gradient_colors.len(), 6);
        assert_eq!(colors.peak_gradient_colors.len(), 6);
        assert_eq!(colors.border_opacity, 1.0);
        assert_eq!(colors.led_border_opacity, 1.0);

        let light = VisualizerColors::light_default();
        assert_eq!(light.border_opacity, 0.0);
        assert_eq!(light.led_border_opacity, 0.0);
    }

    #[test]
    fn test_semantic_color_default() {
        let palette = ThemePalette::default();
        assert_eq!(palette.danger.base, "#cc241d");
        assert_eq!(palette.success.base, "#98971a");
        assert_eq!(palette.warning.base, "#d79921");
        assert_eq!(palette.star.base, "#d79921");
        assert_eq!(palette.star.bright, "#fabd2f");
    }
}
