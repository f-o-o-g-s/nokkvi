//! Theme colors and styling helpers
//!
//! Colors are loaded from named theme files at `~/.config/nokkvi/themes/`.
//! Light/dark mode can be toggled at runtime.
//!
//! All color accessors are functions (not statics) so they react to hot-reload via `reload_theme()`.

mod colors;
mod cover_visualizer;
mod dynamic_accent;
mod font;
mod radius;
mod state;
mod style;
mod ui_mode;

pub(crate) use colors::*;
#[cfg(test)]
pub(crate) use dynamic_accent::AccentSeed;
pub(crate) use dynamic_accent::{CoverPalette, palette_from_encoded, palette_from_rgba};
pub(crate) use font::*;
pub(crate) use radius::*;
pub(crate) use state::*;
pub(crate) use style::*;
pub(crate) use ui_mode::*;

#[cfg(test)]
mod tests;
