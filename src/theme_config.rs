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

/// Parse a hardcoded hex literal. Panics on invalid input — only ever called
/// with compile-time constants (same `.expect` precedent as before the
/// `resolved_palette!` consolidation).
fn hex(s: &str) -> Color {
    parse_hex_color(s).expect("valid hardcoded hex")
}

/// Group fallback for every background token: Gruvbox `bg0`.
fn fallback_bg() -> Color {
    hex("#282828")
}

/// Group fallback for every foreground token: Gruvbox `fg0`.
fn fallback_fg() -> Color {
    hex("#fbf1c7")
}

/// Group fallback for every accent token: Gruvbox `blue`.
fn fallback_accent() -> Color {
    hex("#458588")
}

/// Single source of truth for the `ThemePalette` → `ResolvedTheme` mapping.
///
/// Each `field <= palette.path, fallback;` entry emits BOTH the struct field
/// and its resolution line in `from_theme_palette`, so adding a color is a
/// one-line edit and the struct can never drift from the constructor.
///
/// `border` is special-cased in the transcriber because it is the one token
/// with a non-`parse_hex_or_default` resolution: an explicit TOML hex wins;
/// an empty string derives `theme::darken(bg0_hard, 0.30)`.
macro_rules! resolved_palette {
    (
        $(
            $(#[$meta:meta])*
            $field:ident <= $($seg:ident).+, $fallback:expr;
        )+
    ) => {
        /// Resolved theme with parsed Color values
        #[derive(Debug, Clone)]
        pub(crate) struct ResolvedTheme {
            $(
                $(#[$meta])*
                pub $field: Color,
            )+
            /// Chrome separator (1 px hairline border between bars, rows,
            /// capsules). Read via `theme::border()`; per-theme TOML value or
            /// auto-derived `darken(bg.hard, 30%)` when empty.
            pub border: Color,
        }

        impl ResolvedTheme {
            /// Create resolved theme from the data-crate `ThemePalette`.
            pub(crate) fn from_theme_palette(palette: &ThemePalette) -> Self {
                Self {
                    $(
                        $field: parse_hex_or_default(&palette.$($seg).+, $fallback),
                    )+
                    // Chrome border: explicit hex from TOML, or derived by
                    // darkening `background.hard` by 30 % via `theme::darken`
                    // (the single source for the darkening algorithm).
                    border: if palette.border.is_empty() {
                        crate::theme::darken(
                            parse_hex_or_default(&palette.background.hard, fallback_bg()),
                            0.30,
                        )
                    } else {
                        parse_hex_or_default(&palette.border, fallback_bg())
                    },
                }
            }
        }
    };
}

// `accent.now_playing` / `accent.selected` are not resolved into the theme:
// the now-playing and selected slot highlights are derived from the accent
// tokens with contrast guards (see `theme::playing_fill` /
// `selected_fill_resolved`). The TOML fields are still parsed for round-trip
// compatibility (the `star.base` precedent) but unused here.
//
// Base `star` color was dropped during the redesign cleanup — only
// `star_bright` is consumed (slot-list ratings + metadata pill). The TOML
// `palette.star.base` field stays so existing themes deserialize cleanly;
// it's just not pulled into `ResolvedTheme`.
resolved_palette! {
    // Background
    bg0_hard <= background.hard, fallback_bg();
    bg0 <= background.default, fallback_bg();
    bg0_soft <= background.soft, fallback_bg();
    bg1 <= background.level1, fallback_bg();
    bg2 <= background.level2, fallback_bg();
    bg3 <= background.level3, fallback_bg();

    // Foreground
    fg0 <= foreground.bright, fallback_fg();
    fg1 <= foreground.level1, fallback_fg();
    fg2 <= foreground.level2, fallback_fg();
    fg3 <= foreground.level3, fallback_fg();
    fg4 <= foreground.level4, fallback_fg();

    // Accent (blue)
    accent <= accent.primary, fallback_accent();
    accent_bright <= accent.bright, fallback_accent();
    accent_border_light <= accent.border_light, fallback_accent();

    // Semantic colors
    danger <= danger.base, hex("#cc241d");
    danger_bright <= danger.bright, hex("#fb4934");
    success <= success.base, hex("#98971a");
    warning <= warning.base, hex("#d79921");
    warning_bright <= warning.bright, hex("#fabd2f");
    star_bright <= star.bright, hex("#fabd2f");
}

impl Default for ResolvedTheme {
    fn default() -> Self {
        Self::from_theme_palette(&ThemePalette::default())
    }
}

/// Resolved dual themes (dark + light)
#[derive(Debug, Clone, Default)]
pub(crate) struct ResolvedDualTheme {
    pub dark: ResolvedTheme,
    pub light: ResolvedTheme,
}

impl ResolvedDualTheme {
    /// Create from a `ThemeFile`.
    pub(crate) fn from_theme_file(theme: &ThemeFile) -> Self {
        Self {
            dark: ResolvedTheme::from_theme_palette(&theme.dark),
            light: ResolvedTheme::from_theme_palette(&theme.light),
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

    debug!(" Loaded theme '{}'", theme.name);

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

#[cfg(test)]
mod tests {
    use nokkvi_data::types::theme_file::{
        AccentConfig, BackgroundConfig, ForegroundConfig, SemanticColorConfig, ThemePalette,
        VisualizerColors,
    };

    use super::*;

    /// Palette in which every `resolved_palette!`-consumed field carries its
    /// own distinct hex, so a wrong-path mapping (the one drift the compiler
    /// cannot catch) shows up as a mismatched color in the assertions below.
    fn distinct_palette() -> ThemePalette {
        ThemePalette {
            background: BackgroundConfig {
                hard: "#010101".to_string(),
                default: "#020202".to_string(),
                soft: "#030303".to_string(),
                level1: "#040404".to_string(),
                level2: "#050505".to_string(),
                level3: "#060606".to_string(),
            },
            foreground: ForegroundConfig {
                bright: "#080808".to_string(),
                level1: "#090909".to_string(),
                level2: "#0a0a0a".to_string(),
                level3: "#0b0b0b".to_string(),
                level4: "#0c0c0c".to_string(),
            },
            accent: AccentConfig {
                primary: "#0e0e0e".to_string(),
                bright: "#0f0f0f".to_string(),
                border_light: "#101010".to_string(),
                now_playing: "#111111".to_string(),
                selected: "#121212".to_string(),
            },
            danger: SemanticColorConfig {
                base: "#131313".to_string(),
                bright: "#141414".to_string(),
            },
            success: SemanticColorConfig {
                base: "#151515".to_string(),
                bright: "#161616".to_string(),
            },
            warning: SemanticColorConfig {
                base: "#171717".to_string(),
                bright: "#181818".to_string(),
            },
            star: SemanticColorConfig {
                base: "#191919".to_string(),
                bright: "#1a1a1a".to_string(),
            },
            visualizer: VisualizerColors::default(),
            border: "#1b1b1b".to_string(),
        }
    }

    /// Empty-string palette: every hex field misses, so every resolved field
    /// must take its declared fallback.
    fn empty_palette() -> ThemePalette {
        ThemePalette {
            background: BackgroundConfig {
                hard: String::new(),
                default: String::new(),
                soft: String::new(),
                level1: String::new(),
                level2: String::new(),
                level3: String::new(),
            },
            foreground: ForegroundConfig {
                bright: String::new(),
                level1: String::new(),
                level2: String::new(),
                level3: String::new(),
                level4: String::new(),
            },
            accent: AccentConfig {
                primary: String::new(),
                bright: String::new(),
                border_light: String::new(),
                now_playing: String::new(),
                selected: String::new(),
            },
            danger: SemanticColorConfig {
                base: String::new(),
                bright: String::new(),
            },
            success: SemanticColorConfig {
                base: String::new(),
                bright: String::new(),
            },
            warning: SemanticColorConfig {
                base: String::new(),
                bright: String::new(),
            },
            star: SemanticColorConfig {
                base: String::new(),
                bright: String::new(),
            },
            visualizer: VisualizerColors::default(),
            border: String::new(),
        }
    }

    /// Every `ResolvedTheme` field must resolve from its own palette path —
    /// catches a `resolved_palette!` entry pointing at the wrong field.
    #[test]
    fn from_theme_palette_maps_each_palette_path() {
        let resolved = ResolvedTheme::from_theme_palette(&distinct_palette());

        for (got, want, name) in [
            (resolved.bg0_hard, "#010101", "bg0_hard"),
            (resolved.bg0, "#020202", "bg0"),
            (resolved.bg0_soft, "#030303", "bg0_soft"),
            (resolved.bg1, "#040404", "bg1"),
            (resolved.bg2, "#050505", "bg2"),
            (resolved.bg3, "#060606", "bg3"),
            (resolved.fg0, "#080808", "fg0"),
            (resolved.fg1, "#090909", "fg1"),
            (resolved.fg2, "#0a0a0a", "fg2"),
            (resolved.fg3, "#0b0b0b", "fg3"),
            (resolved.fg4, "#0c0c0c", "fg4"),
            (resolved.accent, "#0e0e0e", "accent"),
            (resolved.accent_bright, "#0f0f0f", "accent_bright"),
            (
                resolved.accent_border_light,
                "#101010",
                "accent_border_light",
            ),
            (resolved.danger, "#131313", "danger"),
            (resolved.danger_bright, "#141414", "danger_bright"),
            (resolved.success, "#151515", "success"),
            (resolved.warning, "#171717", "warning"),
            (resolved.warning_bright, "#181818", "warning_bright"),
            (resolved.star_bright, "#1a1a1a", "star_bright"),
            (resolved.border, "#1b1b1b", "border"),
        ] {
            assert_eq!(got, hex(want), "{name} must resolve from {want}");
        }
    }

    /// Empty palette strings must fall back per group (#282828 backgrounds,
    /// #fbf1c7 foregrounds, #458588 accents) and per-field for the semantic
    /// colors — byte-identical to the pre-macro hardcoded fallbacks.
    #[test]
    fn from_theme_palette_empty_strings_fall_back_per_group() {
        let resolved = ResolvedTheme::from_theme_palette(&empty_palette());

        for (got, name) in [
            (resolved.bg0_hard, "bg0_hard"),
            (resolved.bg0, "bg0"),
            (resolved.bg0_soft, "bg0_soft"),
            (resolved.bg1, "bg1"),
            (resolved.bg2, "bg2"),
            (resolved.bg3, "bg3"),
        ] {
            assert_eq!(got, hex("#282828"), "{name} must fall back to #282828");
        }
        for (got, name) in [
            (resolved.fg0, "fg0"),
            (resolved.fg1, "fg1"),
            (resolved.fg2, "fg2"),
            (resolved.fg3, "fg3"),
            (resolved.fg4, "fg4"),
        ] {
            assert_eq!(got, hex("#fbf1c7"), "{name} must fall back to #fbf1c7");
        }
        for (got, name) in [
            (resolved.accent, "accent"),
            (resolved.accent_bright, "accent_bright"),
            (resolved.accent_border_light, "accent_border_light"),
        ] {
            assert_eq!(got, hex("#458588"), "{name} must fall back to #458588");
        }
        assert_eq!(resolved.danger, hex("#cc241d"));
        assert_eq!(resolved.danger_bright, hex("#fb4934"));
        assert_eq!(resolved.success, hex("#98971a"));
        assert_eq!(resolved.warning, hex("#d79921"));
        assert_eq!(resolved.warning_bright, hex("#fabd2f"));
        assert_eq!(resolved.star_bright, hex("#fabd2f"));
    }

    /// An empty `border` derives through `theme::darken` (the single source
    /// of the darkening algorithm) from the resolved `bg0_hard`.
    #[test]
    fn border_derives_via_theme_darken_when_unset() {
        let mut palette = distinct_palette();
        palette.border = String::new();
        let resolved = ResolvedTheme::from_theme_palette(&palette);
        assert_eq!(
            resolved.border,
            crate::theme::darken(resolved.bg0_hard, 0.30)
        );
    }

    /// An explicit `border` hex wins over the derived value.
    #[test]
    fn border_uses_explicit_hex_when_set() {
        let mut palette = empty_palette();
        palette.border = "#123456".to_string();
        let resolved = ResolvedTheme::from_theme_palette(&palette);
        assert_eq!(resolved.border, hex("#123456"));
    }
}
