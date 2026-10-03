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
use tracing::debug;

#[cfg(test)]
use super::dynamic_accent::AccentSeed;
use super::{
    UI_MODE, cover_visualizer, cover_visualizer::CoverMilkdrop, dynamic_accent,
    dynamic_accent::CoverPalette,
};
use crate::theme_config::{ResolvedDualTheme, ResolvedTheme, load_active_theme_file};

// ============================================================================
// Global theme state (with hot-reload support via lock-free ArcSwap)
// ============================================================================

/// What the cover recolors, one switch per setting ("Accent From Album Art",
/// "Visualizer From Album Art").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CoverFollow {
    /// The accent tokens and the roles derived from them.
    pub accent: bool,
    /// The visualizer gradients (Bars, Lines, Scope) and the colors themed
    /// MilkDrop presets are compiled with.
    pub visualizer: bool,
}

impl CoverFollow {
    pub(crate) fn any(self) -> bool {
        self.accent || self.visualizer
    }
}

/// The theme file's visualizer colors for both modes: the one copy every
/// visualizer reader goes through (`get_visualizer_colors*`).
#[derive(Debug, Clone)]
struct DualVisualizer {
    dark: VisualizerColors,
    light: VisualizerColors,
}

impl DualVisualizer {
    fn of(file: &ThemeFile) -> Self {
        Self {
            dark: file.dark.visualizer.clone(),
            light: file.light.visualizer.clone(),
        }
    }
}

/// The visualizer colors taken from the cover, for both modes, plus the
/// colors themed MilkDrop presets compile with.
#[derive(Debug, Clone)]
struct CoverVisualizer {
    shown: DualVisualizer,
    milkdrop: CoverMilkdrop,
}

/// The theme file's own palette, the palette on screen, and the cover colors
/// that tell them apart. `shown` is `base` with the accent tokens refitted
/// from the cover (when `follow.accent`); `visualizer` holds the cover's
/// gradients (when `follow.visualizer`). One value behind one `ArcSwap`, so a
/// theme reload and a cover change can never store a mix of the two.
#[derive(Debug, Clone)]
struct ActiveTheme {
    base: ResolvedDualTheme,
    base_visualizer: DualVisualizer,
    shown: ResolvedDualTheme,
    visualizer: Option<CoverVisualizer>,
    cover: Option<CoverPalette>,
    follow: CoverFollow,
}

impl ActiveTheme {
    fn compose(
        base: ResolvedDualTheme,
        base_visualizer: DualVisualizer,
        cover: Option<CoverPalette>,
        follow: CoverFollow,
    ) -> Self {
        let mut shown = base.clone();
        let mut visualizer = None;
        if let Some(palette) = &cover {
            if follow.accent {
                dynamic_accent::apply(palette.primary(), &mut shown);
            }
            if follow.visualizer {
                visualizer = Some(CoverVisualizer {
                    shown: DualVisualizer {
                        dark: cover_visualizer::visualizer_colors(
                            palette,
                            &base.dark,
                            &base_visualizer.dark,
                        ),
                        light: cover_visualizer::visualizer_colors(
                            palette,
                            &base.light,
                            &base_visualizer.light,
                        ),
                    },
                    milkdrop: cover_visualizer::milkdrop_colors(palette, &base.dark),
                });
            }
        }
        Self {
            base,
            base_visualizer,
            shown,
            visualizer,
            cover,
            follow,
        }
    }
}

/// Global resolved dual theme — parsed `iced::Color` values for rendering.
///
/// Uses `ArcSwap` for lock-free reads from the render path. Each color
/// accessor performs an atomic Arc clone (~1 ns) instead of acquiring a
/// reader lock or cloning the whole 22-field struct.
static DUAL_THEME: LazyLock<ArcSwap<ActiveTheme>> = LazyLock::new(|| {
    // Seed any missing built-in themes to ~/.config/nokkvi/themes/ on first access
    if let Err(e) = nokkvi_data::services::theme_loader::seed_builtin_themes() {
        tracing::warn!("Failed to seed built-in themes: {e}");
    }
    let file = load_active_theme_file();
    debug!(" Loaded theme '{}'", file.name);
    ArcSwap::from(Arc::new(ActiveTheme::compose(
        ResolvedDualTheme::from_theme_file(&file),
        DualVisualizer::of(&file),
        None,
        CoverFollow::default(),
    )))
});

/// Monotonic counter bumped every time the active palette changes — by
/// `reload_theme()` (theme file edit, preset switch, color picker),
/// `set_light_mode()` (light/dark toggle) or `set_cover_colors()` (new cover
/// colors, about once per track; the MilkDrop palette check rides on it even
/// when only the visualizer follows the cover). Widgets that cache theme-derived
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
/// Active cover colors are refitted to the new palette.
pub(crate) fn reload_theme() {
    let new_file = load_active_theme_file();
    let new_resolved = ResolvedDualTheme::from_theme_file(&new_file);
    let new_visualizer = DualVisualizer::of(&new_file);

    DUAL_THEME.rcu(|active| {
        Arc::new(ActiveTheme::compose(
            new_resolved.clone(),
            new_visualizer.clone(),
            active.cover.clone(),
            active.follow,
        ))
    });
    THEME_GENERATION.fetch_add(1, Ordering::Relaxed);

    debug!(" Theme hot-reloaded from theme file");
}

/// Lay the cover's colors over the theme (`Some`), or return to the theme
/// file's own (`None`), recoloring what `follow` names: the accent tokens and
/// the roles derived from them (see `theme_config::AccentRoles`), and/or the
/// visualizer gradients. Both modes, each fitted to its own backgrounds;
/// nothing is written to disk. A no-op when nothing changed, so callers may
/// level-set it.
pub(crate) fn set_cover_colors(cover: Option<CoverPalette>, follow: CoverFollow) {
    let cover = cover.filter(|_| follow.any());
    let follow = if cover.is_some() {
        follow
    } else {
        CoverFollow::default()
    };
    let previous = DUAL_THEME.rcu(|active| {
        if active.cover == cover && active.follow == follow {
            Arc::clone(active)
        } else {
            Arc::new(ActiveTheme::compose(
                active.base.clone(),
                active.base_visualizer.clone(),
                cover.clone(),
                follow,
            ))
        }
    });
    if previous.cover != cover || previous.follow != follow {
        THEME_GENERATION.fetch_add(1, Ordering::Relaxed);
        debug!(" Cover colors changed: {follow:?} {cover:?}");
    }
}

/// Test shorthand: a one-hue cover on the accent only (`None` clears).
#[cfg(test)]
pub(crate) fn set_dynamic_accent(seed: Option<AccentSeed>) {
    set_cover_colors(
        seed.map(CoverPalette::single),
        CoverFollow {
            accent: true,
            visualizer: false,
        },
    );
}

/// Whether the accent currently follows the playing cover.
#[inline]
pub(crate) fn dynamic_accent_active() -> bool {
    let active = DUAL_THEME.load();
    active.follow.accent && active.cover.is_some()
}

/// The colors themed MilkDrop presets take from the cover while the
/// visualizer follows it; `None` otherwise (they keep the theme's).
pub(crate) fn cover_milkdrop() -> Option<CoverMilkdrop> {
    DUAL_THEME
        .load()
        .visualizer
        .as_ref()
        .map(|v| v.milkdrop.clone())
}

/// The cover-derived accent seed laid over the theme, if any.
#[cfg(test)]
pub(crate) fn dynamic_accent_seed() -> Option<AccentSeed> {
    let active = DUAL_THEME.load();
    active
        .cover
        .as_ref()
        .filter(|_| active.follow.accent)
        .map(CoverPalette::primary)
}

/// Get the active mode's visualizer colors (hex strings): the cover's while
/// the visualizer follows it, else the theme file's. Returns a clone — safe
/// to call from the render loop.
#[inline]
pub(crate) fn get_visualizer_colors() -> VisualizerColors {
    if let Some(cover) = &DUAL_THEME.load().visualizer {
        return if is_light_mode() {
            cover.shown.light.clone()
        } else {
            cover.shown.dark.clone()
        };
    }
    let active = DUAL_THEME.load();
    if is_light_mode() {
        active.base_visualizer.light.clone()
    } else {
        active.base_visualizer.dark.clone()
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
    DUAL_THEME.load().base_visualizer.dark.clone()
}

/// Read a single color field from the active mode's theme without cloning the
/// 22-field `ResolvedTheme`. The closure receives a borrow of the active
/// palette (dark or light) and returns the desired `Color`. `ArcSwap::load`
/// is lock-free (one atomic Arc clone), so this is safe to call from the
/// render path at any frequency.
#[inline]
pub(super) fn read_color<F: FnOnce(&ResolvedTheme) -> Color>(f: F) -> Color {
    let active = DUAL_THEME.load();
    let theme = if UI_MODE.light_mode.load(Ordering::Relaxed) {
        &active.shown.light
    } else {
        &active.shown.dark
    };
    f(theme)
}

/// Read a single color field from the theme file's own **dark** palette,
/// regardless of the active light/dark mode and of any dynamic accent. The app
/// logo uses this so the mark keeps one stable look: the bright-body longship
/// reads on both light and dark backgrounds (its fixed dark outline carries the
/// definition), whereas tracking light mode inverts the body to dark ink and
/// turns the mark into an unreadable blob on a light background. The logo still
/// recolors across *themes* (each theme's dark palette) — it just no longer
/// flips with the light/dark toggle.
///
/// Reading the base palette is what keeps a themed MilkDrop preset from
/// rebuilding on every track change: its `PresetPalette` compares these
/// colors, and a cover-derived accent must not count as a new theme.
#[inline]
pub(crate) fn read_dark_color<F: FnOnce(&ResolvedTheme) -> Color>(f: F) -> Color {
    f(&DUAL_THEME.load().base.dark)
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
