//! Theme colors and styling helpers
//!
//! Colors are loaded from named theme files at `~/.config/nokkvi/themes/`.
//! Light/dark mode can be toggled at runtime.
//!
//! All color accessors are functions (not statics) so they react to hot-reload via `reload_theme()`.

mod colors;
mod font;
mod radius;
mod state;
mod style;
mod ui_mode;

pub(crate) use colors::*;
pub(crate) use font::*;
pub(crate) use radius::*;
pub(crate) use state::*;
pub(crate) use style::*;
pub(crate) use ui_mode::*;

#[cfg(test)]
mod tests;
