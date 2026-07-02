//! Global theme state — the hot-reloadable ArcSwap palette, raw theme-file
//! access, the generation counter, light-mode control, logo colors, and the
//! crate-wide test locks for the global atomics.

use std::sync::{
    Arc, LazyLock,
    atomic::{AtomicU64, Ordering},
};

use arc_swap::ArcSwap;
use iced::Color;
use nokkvi_data::types::theme_file::{ThemeFile, VisualizerColors};
use parking_lot::RwLock;
use tracing::debug;

use super::UI_MODE;
use crate::theme_config::{
    ResolvedDualTheme, ResolvedTheme, load_active_theme_file, load_resolved_dual_theme,
};

// ============================================================================
// Global theme state (with hot-reload support via lock-free ArcSwap)
// ============================================================================

/// Global resolved dual theme — parsed `iced::Color` values for rendering.
///
/// Uses `ArcSwap` for lock-free reads from the render path. Each color
/// accessor performs an atomic Arc clone (~1 ns) instead of acquiring a
/// reader lock or cloning the whole 22-field struct.
static DUAL_THEME: LazyLock<ArcSwap<ResolvedDualTheme>> = LazyLock::new(|| {
    // Seed any missing built-in themes to ~/.config/nokkvi/themes/ on first access
    if let Err(e) = nokkvi_data::services::theme_loader::seed_builtin_themes() {
        tracing::warn!("Failed to seed built-in themes: {e}");
    }
    ArcSwap::from(Arc::new(load_resolved_dual_theme()))
});

/// Global raw theme file — hex strings for visualizer colors and UI that
/// needs the original color values (not parsed `iced::Color`).
static THEME_FILE: LazyLock<RwLock<ThemeFile>> =
    LazyLock::new(|| RwLock::new(load_active_theme_file()));

/// Monotonic counter bumped every time the active palette changes — either
/// by `reload_theme()` (theme file edit, preset switch, color picker) or
/// `set_light_mode()` (light/dark toggle). Widgets that cache theme-derived
/// content (e.g. the boat's substituted SVG handle) snapshot this on build
/// and rebuild when it advances. Without this counter, every new code path
/// that mutates the active theme is a fresh chance to leave a stale cache.
static THEME_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Read the current theme generation. Pair with a stored snapshot to detect
/// "active palette changed since I last built my cache."
#[inline]
pub(crate) fn theme_generation() -> u64 {
    THEME_GENERATION.load(Ordering::Relaxed)
}

/// Advance the theme generation, invalidating every theme-derived cache that
/// snapshots `theme_generation()` (e.g. the boat's substituted SVG handles).
/// Used by non-palette mutations whose result the caches still depend on —
/// currently the icon-set switch, which changes the boat's anchor sprite.
#[inline]
pub(crate) fn bump_theme_generation() {
    THEME_GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Reload theme from theme file (hot-reload support).
/// Call this when the theme file or `theme` key in config.toml changes.
pub(crate) fn reload_theme() {
    let new_file = load_active_theme_file();
    let new_resolved = ResolvedDualTheme::from_theme_file(&new_file);

    DUAL_THEME.store(Arc::new(new_resolved));
    {
        let mut file = THEME_FILE.write();
        *file = new_file;
    }
    THEME_GENERATION.fetch_add(1, Ordering::Relaxed);

    debug!(" Theme hot-reloaded from theme file");
}

/// Get the active mode's visualizer colors (hex strings).
/// Returns a clone — safe to call from the render loop.
#[inline]
pub(crate) fn get_visualizer_colors() -> VisualizerColors {
    let guard = THEME_FILE.read();
    if is_light_mode() {
        guard.light.visualizer.clone()
    } else {
        guard.dark.visualizer.clone()
    }
}

/// Get the **dark** palette's visualizer colors regardless of the active
/// light/dark mode. The boat doodad (hull outline, anchor, rope) uses this so
/// it stays well-defined: light themes drop `border_opacity` (e.g. Svalbard
/// `1.0` → `0.5`), which faded the boat's thin outline to near-invisible. The
/// boat still recolors across *themes* (each theme's dark visualizer border),
/// it just no longer fades on the light/dark toggle — mirroring the mode-stable
/// logo fills. The wave itself keeps `get_visualizer_colors()` so it still
/// honors the light-mode styling.
#[inline]
pub(crate) fn get_visualizer_colors_dark() -> VisualizerColors {
    THEME_FILE.read().dark.visualizer.clone()
}

/// Read a single color field from the active mode's theme without cloning the
/// 22-field `ResolvedTheme`. The closure receives a borrow of the active
/// palette (dark or light) and returns the desired `Color`. `ArcSwap::load`
/// is lock-free (one atomic Arc clone), so this is safe to call from the
/// render path at any frequency.
#[inline]
pub(super) fn read_color<F: FnOnce(&ResolvedTheme) -> Color>(f: F) -> Color {
    let dual = DUAL_THEME.load();
    let theme = if UI_MODE.light_mode.load(Ordering::Relaxed) {
        &dual.light
    } else {
        &dual.dark
    };
    f(theme)
}

/// Read a single color field from the **dark** palette regardless of the active
/// light/dark mode. The app logo uses this so the mark keeps one stable look:
/// the bright-body longship reads on both light and dark backgrounds (its fixed
/// dark outline carries the definition), whereas tracking light mode inverts the
/// body to dark ink and turns the mark into an unreadable blob on a light
/// background. The logo still recolors across *themes* (each theme's dark
/// palette) — it just no longer flips with the light/dark toggle.
#[inline]
fn read_dark_color<F: FnOnce(&ResolvedTheme) -> Color>(f: F) -> Color {
    f(&DUAL_THEME.load().dark)
}

/// Logo body fill (sail + hull): the active theme's dark `fg0`, mode-stable.
#[inline]
pub(crate) fn logo_body() -> Color {
    read_dark_color(|t| t.fg0)
}

/// Logo shield/bar fill (the three blocks): the active theme's dark `accent`.
#[inline]
pub(crate) fn logo_shields() -> Color {
    read_dark_color(|t| t.accent)
}

/// Logo wood (mast + yard): the active theme's dark `warning`, mode-stable.
#[inline]
pub(crate) fn logo_wood() -> Color {
    read_dark_color(|t| t.warning)
}

// ============================================================================
// Light Mode Control
// ============================================================================

/// Returns true if light mode is enabled
#[inline]
pub(crate) fn is_light_mode() -> bool {
    UI_MODE.light_mode.load(Ordering::Relaxed)
}

/// Set light mode state (call to toggle theme at runtime)
#[inline]
pub(crate) fn set_light_mode(enabled: bool) {
    let was = UI_MODE.light_mode.swap(enabled, Ordering::Relaxed);
    THEME_GENERATION.fetch_add(1, Ordering::Relaxed);
    // Log only on a REAL flip — this setter is called unconditionally on
    // every settings reload (handle_player_settings_loaded re-applies the
    // config.toml value), and an unconditional "changed" line sent a
    // light-mode forensics session down the wrong path.
    if was != enabled {
        debug!(" Theme mode changed: light_mode={}", enabled);
    }
}

/// Crate-wide serialization guard for tests that flip process-global theme
/// state: `set_light_mode`, the `UI_MODE` atomics (rounded mode, nav layout,
/// track-info display, artwork column, ...), and the handler paths that
/// persist them. The SINGLE lock for every such test family — chrome-math
/// (`update::tests::redesign_chrome`), player-bar strip, themed-SVG + boat
/// handle-cache, artwork-column layout, slot-count resync, the
/// player-settings-loaded mirror tests, and the update-handler light-mode
/// tests all take this same guard, so no two can interleave. `parking_lot`
/// avoids std-lock poisoning if one test panics.
#[cfg(test)]
pub(crate) static THEME_MODE_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
