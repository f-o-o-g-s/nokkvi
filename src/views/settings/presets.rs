//! Theme presets — delegates to theme_loader for discovery & application.
//!
//! Replaces the old compile-time preset system. Themes are now `.toml` files
//! in `~/.config/nokkvi/themes/`, seeded from built-in defaults on first run.
//! Applying a preset sets `theme = "stem"` in config.toml.

use anyhow::Result;
use nokkvi_data::services::theme_loader::{self, ThemeInfo};
use tracing::debug;

// ============================================================================
// Theme Discovery
// ============================================================================

/// Discover all available themes from `~/.config/nokkvi/themes/`.
///
/// Returns metadata sorted by display name. Safe to call from the UI —
/// falls back to an empty list on errors.
pub(crate) fn all_themes() -> Vec<ThemeInfo> {
    theme_loader::discover_themes().unwrap_or_else(|e| {
        tracing::warn!("Failed to discover themes: {e}");
        Vec::new()
    })
}

// ============================================================================
// Theme Application
// ============================================================================

/// Apply a theme by writing `theme = "{stem}"` to config.toml.
///
/// The config watcher picks up the change and triggers `ThemeConfigReloaded`.
pub(crate) fn apply_theme(stem: &str) -> Result<()> {
    theme_loader::write_theme_name_to_config(stem)?;
    debug!(" [PRESETS] Applied theme: {stem}");
    Ok(())
}

/// Restore a built-in theme to its original defaults.
///
/// Overwrites the user's theme file with the compiled-in version,
/// then reloads. Only works for built-in themes.
pub(crate) fn restore_theme(stem: &str) -> Result<()> {
    theme_loader::restore_builtin(stem)?;
    debug!(" [PRESETS] Restored built-in theme: {stem}");
    Ok(())
}

/// Get the currently active theme stem name.
pub(crate) fn active_theme_stem() -> String {
    theme_loader::read_theme_name_from_config()
}
