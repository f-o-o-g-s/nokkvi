//! Theme configuration — color resolution and loading
//!
//! Loads theme colors from named theme files at `~/.config/nokkvi/themes/`.
//! Resolves hex strings into `iced::Color` values for use by the rendering layer.

use iced::Color;
use nokkvi_data::types::theme_file::{ThemeFile, ThemePalette};
use tracing::debug;

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
        Self::from_theme_palette(&ThemePalette::default())
    }
}

impl ResolvedTheme {
    /// Create resolved theme from the data-crate `ThemePalette`.
    pub(crate) fn from_theme_palette(palette: &ThemePalette) -> Self {
        let fallback_bg = parse_hex_color("#282828").unwrap();
        let fallback_fg = parse_hex_color("#fbf1c7").unwrap();
        let fallback_accent = parse_hex_color("#458588").unwrap();

        Self {
            bg0_hard: parse_hex_or_default(&palette.background.hard, fallback_bg),
            bg0: parse_hex_or_default(&palette.background.default, fallback_bg),
            bg0_soft: parse_hex_or_default(&palette.background.soft, fallback_bg),
            bg1: parse_hex_or_default(&palette.background.level1, fallback_bg),
            bg2: parse_hex_or_default(&palette.background.level2, fallback_bg),
            bg3: parse_hex_or_default(&palette.background.level3, fallback_bg),

            fg0: parse_hex_or_default(&palette.foreground.bright, fallback_fg),
            fg1: parse_hex_or_default(&palette.foreground.level1, fallback_fg),
            fg2: parse_hex_or_default(&palette.foreground.level2, fallback_fg),
            fg3: parse_hex_or_default(&palette.foreground.level3, fallback_fg),
            fg4: parse_hex_or_default(&palette.foreground.level4, fallback_fg),

            accent: parse_hex_or_default(&palette.accent.primary, fallback_accent),
            accent_bright: parse_hex_or_default(&palette.accent.bright, fallback_accent),
            accent_border_light: parse_hex_or_default(
                &palette.accent.border_light,
                fallback_accent,
            ),
            now_playing: if palette.accent.now_playing.is_empty() {
                parse_hex_or_default(&palette.accent.primary, fallback_accent)
            } else {
                parse_hex_or_default(&palette.accent.now_playing, fallback_accent)
            },
            selected: if palette.accent.selected.is_empty() {
                parse_hex_or_default(&palette.accent.bright, fallback_accent)
            } else {
                parse_hex_or_default(&palette.accent.selected, fallback_accent)
            },

            red: parse_hex_or_default(&palette.red.normal, parse_hex_color("#cc241d").unwrap()),
            red_bright: parse_hex_or_default(
                &palette.red.bright,
                parse_hex_color("#fb4934").unwrap(),
            ),
            green: parse_hex_or_default(
                &palette.green.normal,
                parse_hex_color("#98971a").unwrap(),
            ),
            yellow: parse_hex_or_default(
                &palette.yellow.normal,
                parse_hex_color("#d79921").unwrap(),
            ),
            yellow_bright: parse_hex_or_default(
                &palette.yellow.bright,
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
    /// Create from a `ThemeFile`.
    pub(crate) fn from_theme_file(theme: &ThemeFile) -> Self {
        Self {
            dark: ResolvedTheme::from_theme_palette(&theme.dark),
            light: ResolvedTheme::from_theme_palette(&theme.light),
            font_family: theme.font_family.clone(),
        }
    }
}

// ============================================================================
// Config Loading — delegates to theme_loader
// ============================================================================

/// Load the active `ThemeFile` via theme_loader.
pub(crate) fn load_active_theme_file() -> ThemeFile {
    let name = nokkvi_data::services::theme_loader::read_theme_name_from_config();
    nokkvi_data::services::theme_loader::load_theme(&name)
}

/// Load and resolve dual themes from theme file.
pub(crate) fn load_resolved_dual_theme() -> ResolvedDualTheme {
    let theme = load_active_theme_file();
    let resolved = ResolvedDualTheme::from_theme_file(&theme);

    debug!(
        " Loaded theme '{}': font=\"{}\"",
        theme.name, resolved.font_family
    );

    resolved
}

/// Load light_mode from [settings] in config.toml (for hot-reload).
/// This is used by the ThemeConfigReloaded handler to let scripts
/// (e.g. visualizer_showcase.py --both-modes) toggle dark/light mode
/// programmatically without user interaction.
pub(crate) fn load_light_mode_from_config() -> bool {
    // light_mode is now in [settings], not in the theme.
    // Read from settings directly.
    match nokkvi_data::services::toml_settings_io::read_toml_settings() {
        Ok(Some(settings)) => settings.light_mode,
        _ => false,
    }
}
