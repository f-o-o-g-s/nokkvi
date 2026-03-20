//! Theme configuration with config.toml support
//!
//! Loads theme colors and font from config.toml at application startup.
//! Supports dual palettes (dark and light) for runtime theme switching.

use iced::Color;
use serde::{Deserialize, Serialize};
use tracing::debug;

// ============================================================================
// Config Structures
// ============================================================================

/// Background colors configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct BackgroundConfig {
    /// Darkest background (#1d2021)
    pub hard: String,
    /// Default dark background (#282828)
    pub default: String,
    /// Soft background (#32302f)
    pub soft: String,
    /// Background level 1 (#3c3836)
    pub level1: String,
    /// Background level 2 (#504945)
    pub level2: String,
    /// Background level 3 (#665c54)
    pub level3: String,
    /// Background level 4 (#7c6f64)
    pub level4: String,
}

impl Default for BackgroundConfig {
    fn default() -> Self {
        Self {
            hard: "#1d2021".to_string(),
            default: "#282828".to_string(),
            soft: "#32302f".to_string(),
            level1: "#3c3836".to_string(),
            level2: "#504945".to_string(),
            level3: "#665c54".to_string(),
            level4: "#7c6f64".to_string(),
        }
    }
}

/// Foreground colors configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct ForegroundConfig {
    /// Brightest foreground (#fbf1c7)
    pub bright: String,
    /// Level 1 foreground (#ebdbb2)
    pub level1: String,
    /// Level 2 foreground (#d5c4a1)
    pub level2: String,
    /// Level 3 foreground (#bdae93)
    pub level3: String,
    /// Level 4 foreground (#a89984)
    pub level4: String,
    /// Gray (#928374)
    pub gray: String,
}

impl Default for ForegroundConfig {
    fn default() -> Self {
        Self {
            bright: "#fbf1c7".to_string(),
            level1: "#ebdbb2".to_string(),
            level2: "#d5c4a1".to_string(),
            level3: "#bdae93".to_string(),
            level4: "#a89984".to_string(),
            gray: "#928374".to_string(),
        }
    }
}

/// Accent colors configuration (blue-based primary accent)
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct AccentConfig {
    /// Primary accent (#458588)
    pub primary: String,
    /// Bright accent (#83a598)
    pub bright: String,
    /// Dark border accent (#5a8a8d)
    pub border_dark: String,
    /// Light border accent (#a8c5c8)
    pub border_light: String,
    /// Now-playing slot highlight color (defaults to primary if empty)
    #[serde(default)]
    pub now_playing: String,
    /// Selected/center slot highlight color (defaults to bright if empty)
    #[serde(default)]
    pub selected: String,
}

impl Default for AccentConfig {
    fn default() -> Self {
        Self {
            primary: "#458588".to_string(),
            bright: "#83a598".to_string(),
            border_dark: "#458588".to_string(),
            border_light: "#83a598".to_string(),
            now_playing: String::new(),
            selected: String::new(),
        }
    }
}

/// Red colors configuration (errors, warnings)
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct RedConfig {
    /// Normal red (#cc241d)
    pub normal: String,
    /// Bright red (#fb4934)
    pub bright: String,
}

impl Default for RedConfig {
    fn default() -> Self {
        Self {
            normal: "#cc241d".to_string(),
            bright: "#fb4934".to_string(),
        }
    }
}

/// Green colors configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct GreenConfig {
    /// Normal green (#98971a)
    pub normal: String,
    /// Bright green (#b8bb26)
    pub bright: String,
}

impl Default for GreenConfig {
    fn default() -> Self {
        Self {
            normal: "#98971a".to_string(),
            bright: "#b8bb26".to_string(),
        }
    }
}

/// Yellow colors configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct YellowConfig {
    /// Normal yellow (#d79921)
    pub normal: String,
    /// Bright yellow (#fabd2f)
    pub bright: String,
}

impl Default for YellowConfig {
    fn default() -> Self {
        Self {
            normal: "#d79921".to_string(),
            bright: "#fabd2f".to_string(),
        }
    }
}

/// Purple colors configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct PurpleConfig {
    /// Normal purple (#b16286)
    pub normal: String,
    /// Bright purple (#d3869b)
    pub bright: String,
}

impl Default for PurpleConfig {
    fn default() -> Self {
        Self {
            normal: "#b16286".to_string(),
            bright: "#d3869b".to_string(),
        }
    }
}

/// Aqua colors configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct AquaConfig {
    /// Normal aqua (#689d6a)
    pub normal: String,
    /// Bright aqua (#8ec07c)
    pub bright: String,
}

impl Default for AquaConfig {
    fn default() -> Self {
        Self {
            normal: "#689d6a".to_string(),
            bright: "#8ec07c".to_string(),
        }
    }
}

/// Orange colors configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct OrangeConfig {
    /// Normal orange (#d65d0e)
    pub normal: String,
    /// Bright orange (#fe8019)
    pub bright: String,
}

impl Default for OrangeConfig {
    fn default() -> Self {
        Self {
            normal: "#d65d0e".to_string(),
            bright: "#fe8019".to_string(),
        }
    }
}

/// Font configuration
/// Empty family string uses system default sans-serif font (works everywhere).
/// Users can override with their preferred font in config.toml.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub(crate) struct FontConfig {
    /// UI font family name
    pub family: String,
}

/// Single palette configuration (colors only, no light_mode flag)
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub(crate) struct PaletteConfig {
    /// Background colors
    pub background: BackgroundConfig,
    /// Foreground colors
    pub foreground: ForegroundConfig,
    /// Primary accent colors (blue)
    pub accent: AccentConfig,
    /// Red colors
    pub red: RedConfig,
    /// Green colors
    pub green: GreenConfig,
    /// Yellow colors
    pub yellow: YellowConfig,
    /// Purple colors
    pub purple: PurpleConfig,
    /// Aqua colors
    pub aqua: AquaConfig,
    /// Orange colors
    pub orange: OrangeConfig,
}

/// Dual theme configuration (dark + light palettes)
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct DualThemeConfig {
    /// Font configuration (shared between modes)
    pub font: FontConfig,
    /// Light mode override from config.toml (hot-reloadable).
    /// NOT intended for users to set permanently — use the in-app toggle instead,
    /// which persists to redb. This exists so external scripts (e.g. visualizer_showcase.py)
    /// can programmatically drive dark/light switching during demos.
    /// If left in config.toml, it will override the in-app toggle on every config reload.
    #[serde(default)]
    pub light_mode: bool,
    /// Dark mode palette
    pub dark: PaletteConfig,
    /// Light mode palette
    pub light: PaletteConfig,
}

impl Default for DualThemeConfig {
    fn default() -> Self {
        Self {
            font: FontConfig::default(),
            light_mode: false,
            dark: PaletteConfig::default(), // Uses dark mode defaults
            light: PaletteConfig {
                // Gruvbox Light mode colors — same pure palette as dark,
                // visual difference comes from inverted background/foreground
                background: BackgroundConfig {
                    hard: "#f9f5d7".to_string(),
                    default: "#fbf1c7".to_string(),
                    soft: "#f2e5bc".to_string(),
                    level1: "#ebdbb2".to_string(),
                    level2: "#d5c4a1".to_string(),
                    level3: "#bdae93".to_string(),
                    level4: "#a89984".to_string(),
                },
                foreground: ForegroundConfig {
                    bright: "#282828".to_string(),
                    level1: "#3c3836".to_string(),
                    level2: "#504945".to_string(),
                    level3: "#665c54".to_string(),
                    level4: "#7c6f64".to_string(),
                    gray: "#928374".to_string(),
                },
                accent: AccentConfig {
                    primary: "#458588".to_string(),
                    bright: "#83a598".to_string(),
                    border_dark: "#458588".to_string(),
                    border_light: "#d5c4a1".to_string(),
                    now_playing: String::new(),
                    selected: String::new(),
                },
                red: RedConfig {
                    normal: "#cc241d".to_string(),
                    bright: "#fb4934".to_string(),
                },
                green: GreenConfig {
                    normal: "#98971a".to_string(),
                    bright: "#b8bb26".to_string(),
                },
                yellow: YellowConfig {
                    normal: "#d79921".to_string(),
                    bright: "#fabd2f".to_string(),
                },
                purple: PurpleConfig {
                    normal: "#b16286".to_string(),
                    bright: "#d3869b".to_string(),
                },
                aqua: AquaConfig {
                    normal: "#689d6a".to_string(),
                    bright: "#8ec07c".to_string(),
                },
                orange: OrangeConfig {
                    normal: "#d65d0e".to_string(),
                    bright: "#fe8019".to_string(),
                },
            },
        }
    }
}

// ============================================================================
// Color Parsing
// ============================================================================

/// Parse a hex color string (e.g., "#458588") to iced::Color
pub(crate) fn parse_hex_color(hex: &str) -> Option<Color> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }

    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;

    Some(Color::from_rgb(
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
    ))
}

/// Parse hex color with fallback
pub(crate) fn parse_hex_or_default(hex: &str, default: Color) -> Color {
    parse_hex_color(hex).unwrap_or(default)
}

// ============================================================================
// Resolved Theme (parsed colors ready for use)
// ============================================================================

/// Resolved theme with parsed Color values
#[derive(Debug, Clone)]
pub(crate) struct ResolvedTheme {
    // Background
    pub bg0_hard: Color,
    pub bg0: Color,
    pub bg0_soft: Color,
    pub bg1: Color,
    pub bg2: Color,
    pub bg3: Color,
    pub bg4: Color,

    // Foreground
    pub fg0: Color,
    pub fg1: Color,
    pub fg2: Color,
    pub fg3: Color,
    pub fg4: Color,

    // Accent (blue)
    pub accent: Color,
    pub accent_bright: Color,
    pub accent_border_light: Color,
    pub now_playing: Color,
    pub selected: Color,

    // Named colors
    pub red: Color,
    pub red_bright: Color,
    pub green: Color,
    pub yellow: Color,
    pub yellow_bright: Color,
}

impl Default for ResolvedTheme {
    fn default() -> Self {
        Self::from_palette(&PaletteConfig::default())
    }
}

impl ResolvedTheme {
    /// Create resolved theme from a palette config
    pub(crate) fn from_palette(config: &PaletteConfig) -> Self {
        // Gruvbox exact hex fallbacks
        let fallback_bg = parse_hex_color("#282828").unwrap(); // bg0
        let fallback_fg = parse_hex_color("#fbf1c7").unwrap(); // fg0
        let fallback_accent = parse_hex_color("#458588").unwrap(); // blue

        Self {
            // Background
            bg0_hard: parse_hex_or_default(&config.background.hard, fallback_bg),
            bg0: parse_hex_or_default(&config.background.default, fallback_bg),
            bg0_soft: parse_hex_or_default(&config.background.soft, fallback_bg),
            bg1: parse_hex_or_default(&config.background.level1, fallback_bg),
            bg2: parse_hex_or_default(&config.background.level2, fallback_bg),
            bg3: parse_hex_or_default(&config.background.level3, fallback_bg),
            bg4: parse_hex_or_default(&config.background.level4, fallback_bg),

            // Foreground
            fg0: parse_hex_or_default(&config.foreground.bright, fallback_fg),
            fg1: parse_hex_or_default(&config.foreground.level1, fallback_fg),
            fg2: parse_hex_or_default(&config.foreground.level2, fallback_fg),
            fg3: parse_hex_or_default(&config.foreground.level3, fallback_fg),
            fg4: parse_hex_or_default(&config.foreground.level4, fallback_fg),

            // Accent
            accent: parse_hex_or_default(&config.accent.primary, fallback_accent),
            accent_bright: parse_hex_or_default(&config.accent.bright, fallback_accent),
            accent_border_light: parse_hex_or_default(&config.accent.border_light, fallback_accent),
            now_playing: if config.accent.now_playing.is_empty() {
                parse_hex_or_default(&config.accent.primary, fallback_accent)
            } else {
                parse_hex_or_default(&config.accent.now_playing, fallback_accent)
            },
            selected: if config.accent.selected.is_empty() {
                parse_hex_or_default(&config.accent.bright, fallback_accent)
            } else {
                parse_hex_or_default(&config.accent.selected, fallback_accent)
            },

            // Named colors - exact Gruvbox hex values
            red: parse_hex_or_default(&config.red.normal, parse_hex_color("#cc241d").unwrap()),
            red_bright: parse_hex_or_default(
                &config.red.bright,
                parse_hex_color("#fb4934").unwrap(),
            ),
            green: parse_hex_or_default(&config.green.normal, parse_hex_color("#98971a").unwrap()),
            yellow: parse_hex_or_default(
                &config.yellow.normal,
                parse_hex_color("#d79921").unwrap(),
            ),
            yellow_bright: parse_hex_or_default(
                &config.yellow.bright,
                parse_hex_color("#fabd2f").unwrap(),
            ),
        }
    }
}

/// Resolved dual themes (dark + light) with font
#[derive(Debug, Clone, Default)]
pub(crate) struct ResolvedDualTheme {
    pub dark: ResolvedTheme,
    pub light: ResolvedTheme,
    pub font_family: String,
}

impl ResolvedDualTheme {
    /// Create from dual theme config
    pub(crate) fn from_config(config: &DualThemeConfig) -> Self {
        Self {
            dark: ResolvedTheme::from_palette(&config.dark),
            light: ResolvedTheme::from_palette(&config.light),
            font_family: config.font.family.clone(),
        }
    }
}

// ============================================================================
// Config Loading
// ============================================================================

/// Full config file structure (includes theme section with dark/light)
#[derive(Debug, Deserialize, Serialize, Default)]
struct ConfigFile {
    #[serde(default)]
    theme: DualThemeConfig,
}

/// Load dual theme config from config.toml
pub(crate) fn load_dual_theme_config() -> DualThemeConfig {
    let config_path = match nokkvi_data::utils::paths::get_config_path() {
        Ok(path) => path,
        Err(_) => return DualThemeConfig::default(),
    };

    if !config_path.exists() {
        return DualThemeConfig::default();
    }

    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return DualThemeConfig::default(),
    };

    let config_file: ConfigFile = toml::from_str(&content).unwrap_or_default();

    config_file.theme
}

/// Load and resolve dual themes from config.toml
pub(crate) fn load_resolved_dual_theme() -> ResolvedDualTheme {
    let config = load_dual_theme_config();
    let resolved = ResolvedDualTheme::from_config(&config);

    debug!(
        " Loaded dual theme config: font=\"{}\"",
        resolved.font_family
    );

    resolved
}

/// Load just the light_mode flag from config.toml (for hot-reload).
/// This is used by the ThemeConfigReloaded handler to let scripts
/// (e.g. visualizer_showcase.py --both-modes) toggle dark/light mode
/// programmatically without user interaction.
pub(crate) fn load_light_mode_from_config() -> bool {
    load_dual_theme_config().light_mode
}
