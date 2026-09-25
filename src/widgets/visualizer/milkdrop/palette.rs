//! Theme colours for nokkvi's own MilkDrop presets.
//!
//! A preset's JSON may name the active theme's colours with placeholder
//! identifiers, which [`PresetPalette::substitute`] replaces with literals
//! just before the preset is parsed:
//!
//! - in shader code, `NOKKVI_<ROLE>` becomes `vec3(r, g, b)`;
//! - in EEL equations, `NOKKVI_<ROLE>_R` / `_G` / `_B` become numbers;
//! - `NOKKVI_LIGHT` is `1.0` in light mode, `0.0` in dark mode.
//!
//! Roles: `BG` (the darkest background), `SURFACE` (a raised background),
//! `TEXT`, `ACCENT`, `HIGHLIGHT` (the bright accent), `WARM` (the theme's warm
//! warning/star colour) and `RAMP0`..`RAMP5` (the visualizer gradient, dark to
//! light as the theme defines it). They always come from the theme's dark
//! palette (see [`PresetPalette::from_theme`]). Values are display-space
//! channels, which is what the MilkDrop blit shows.

use std::borrow::Cow;

/// The token prefix; a preset without it is returned untouched.
const PREFIX: &str = "NOKKVI_";

/// One colour, as display-space RGB channels in 0..=1.
pub(crate) type Rgb = [f32; 3];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresetPalette {
    pub bg: Rgb,
    pub surface: Rgb,
    pub text: Rgb,
    pub accent: Rgb,
    pub highlight: Rgb,
    pub warm: Rgb,
    pub ramp: [Rgb; 6],
    pub light: bool,
}

fn rgb(c: iced::Color) -> Rgb {
    [c.r, c.g, c.b]
}

impl PresetPalette {
    /// The active theme's DARK palette, in either mode: nokkvi's presets are
    /// light on a dark canvas (additive glows, trails fading to black), which a
    /// light palette would wash out, so light mode shows them as a dark panel
    /// in the theme's own dark colours. `light` still tells a preset the mode.
    pub(crate) fn from_theme() -> Self {
        use crate::theme::{self, read_dark_color};
        let bars: crate::visualizer_config::ThemeBarColors =
            theme::get_visualizer_colors_dark().into();
        let accent = rgb(read_dark_color(|t| t.accent));
        let ramp_src: Vec<Rgb> = bars
            .bar_gradient_colors
            .iter()
            .filter_map(|hex| crate::theme_config::parse_hex_color(hex))
            .map(rgb)
            .collect();
        let ramp = std::array::from_fn(|i| {
            if ramp_src.is_empty() {
                accent
            } else {
                // Stretch a shorter gradient over the six slots.
                ramp_src[i * ramp_src.len() / 6]
            }
        });
        Self {
            bg: rgb(read_dark_color(|t| t.bg0_hard)),
            surface: rgb(read_dark_color(|t| t.bg1)),
            text: rgb(read_dark_color(|t| t.fg0)),
            accent,
            highlight: rgb(read_dark_color(|t| t.accent_bright)),
            warm: rgb(read_dark_color(|t| t.warning)),
            ramp,
            light: theme::is_light_mode(),
        }
    }

    /// `(role, colour)` pairs, longest names first so `RAMP1` never shadows a
    /// longer token and `BG` never eats `BG_R`.
    fn roles(&self) -> [(&'static str, Rgb); 12] {
        [
            ("HIGHLIGHT", self.highlight),
            ("SURFACE", self.surface),
            ("ACCENT", self.accent),
            ("RAMP0", self.ramp[0]),
            ("RAMP1", self.ramp[1]),
            ("RAMP2", self.ramp[2]),
            ("RAMP3", self.ramp[3]),
            ("RAMP4", self.ramp[4]),
            ("RAMP5", self.ramp[5]),
            ("TEXT", self.text),
            ("WARM", self.warm),
            ("BG", self.bg),
        ]
    }

    /// Replace every `NOKKVI_*` token in `text`. Text without the prefix (the
    /// whole Butterchurn pack) is borrowed, not copied.
    pub(crate) fn substitute<'a>(&self, text: &'a str) -> Cow<'a, str> {
        if !text.contains(PREFIX) {
            return Cow::Borrowed(text);
        }
        let mut out = String::with_capacity(text.len() + 256);
        let mut rest = text;
        while let Some(at) = rest.find(PREFIX) {
            out.push_str(&rest[..at]);
            let after = &rest[at + PREFIX.len()..];
            let ident_len = after
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(after.len());
            let ident = &after[..ident_len];
            match self.value_of(ident) {
                Some(value) => out.push_str(&value),
                // Unknown token: leave it so the preset fails loudly in the log.
                None => {
                    out.push_str(PREFIX);
                    out.push_str(ident);
                }
            }
            rest = &after[ident_len..];
        }
        out.push_str(rest);
        Cow::Owned(out)
    }

    fn value_of(&self, ident: &str) -> Option<String> {
        if ident == "LIGHT" {
            return Some(if self.light { "1.0" } else { "0.0" }.to_string());
        }
        for (role, [r, g, b]) in self.roles() {
            if ident == role {
                return Some(format!("vec3({r:.4}, {g:.4}, {b:.4})"));
            }
            if let Some(channel) = ident.strip_prefix(role).and_then(|s| s.strip_prefix('_')) {
                return match channel {
                    "R" => Some(format!("{r:.4}")),
                    "G" => Some(format!("{g:.4}")),
                    "B" => Some(format!("{b:.4}")),
                    _ => None,
                };
            }
        }
        None
    }
}

/// Whether a preset's text uses the theme (so a theme change must reload it).
pub(crate) fn uses_theme(text: &str) -> bool {
    text.contains(PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> PresetPalette {
        PresetPalette {
            bg: [0.1, 0.2, 0.3],
            surface: [0.15, 0.25, 0.35],
            text: [0.9, 0.9, 0.9],
            accent: [0.4, 0.6, 0.5],
            highlight: [0.6, 0.7, 0.7],
            warm: [0.8, 0.6, 0.4],
            ramp: [[0.0; 3], [0.2; 3], [0.4; 3], [0.6; 3], [0.8; 3], [1.0; 3]],
            light: false,
        }
    }

    #[test]
    fn text_without_tokens_is_borrowed() {
        let text = "shader_body { ret = texture(sampler_main, uv).xyz; }";
        assert!(matches!(palette().substitute(text), Cow::Borrowed(_)));
        assert!(!uses_theme(text));
    }

    #[test]
    fn shader_tokens_become_vec3_and_eel_tokens_become_numbers() {
        let text = "col = mix(NOKKVI_BG, NOKKVI_ACCENT, t) + NOKKVI_RAMP5; wave_r = NOKKVI_WARM_R;";
        let out = palette().substitute(text);
        assert_eq!(
            out,
            "col = mix(vec3(0.1000, 0.2000, 0.3000), vec3(0.4000, 0.6000, 0.5000), t) \
             + vec3(1.0000, 1.0000, 1.0000); wave_r = 0.8000;"
        );
        assert!(uses_theme(text));
    }

    #[test]
    fn channels_and_light_flag_resolve() {
        let p = palette();
        assert_eq!(p.substitute("NOKKVI_BG_B"), "0.3000");
        assert_eq!(p.substitute("NOKKVI_HIGHLIGHT_G"), "0.7000");
        assert_eq!(p.substitute("x = NOKKVI_LIGHT;"), "x = 0.0;");
        let light = PresetPalette { light: true, ..p };
        assert_eq!(light.substitute("NOKKVI_LIGHT"), "1.0");
    }

    #[test]
    fn unknown_tokens_are_left_in_place() {
        assert_eq!(palette().substitute("NOKKVI_NOPE + 1"), "NOKKVI_NOPE + 1");
        assert_eq!(palette().substitute("NOKKVI_BG_Q"), "NOKKVI_BG_Q");
    }
}
