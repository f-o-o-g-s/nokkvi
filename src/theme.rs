//! Theme colors and styling helpers
//!
//! Colors are loaded from named theme files at `~/.config/nokkvi/themes/`.
//! Light/dark mode can be toggled at runtime.
//!
//! All color accessors are functions (not statics) so they react to hot-reload via `reload_theme()`.

use std::sync::{
    Arc, LazyLock,
    atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering},
};

use arc_swap::ArcSwap;
use iced::{Color, Font};
use nokkvi_data::types::theme_file::{ThemeFile, VisualizerColors};
use parking_lot::RwLock;
use tracing::debug;

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

/// Crate-wide serialization guard for tests that poke the global light-mode
/// atomic (`set_light_mode`). `cargo test` runs multi-threaded, so any two
/// tests that flip the active palette must not interleave — the boat handle
/// cache tests and the themed-SVG tests both lock this. `parking_lot::Mutex`
/// is used so a panic in one test poisons nothing and the group keeps running.
#[cfg(test)]
pub(crate) static TEST_THEME_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

// ============================================================================
// UI Mode Flags (grouped to avoid scattered statics)
// ============================================================================

/// All runtime-togglable UI mode flags, consolidated into one struct.
/// Each flag uses interior atomics for thread-safe lock-free access.
struct UiModeFlags {
    /// Light/dark theme toggle
    light_mode: AtomicBool,
    /// Rounded corner borders — tri-state (Off / On / PlayerOnly). Backed
    /// by `AtomicU8` via the `atomic_u8_enum!` impl on `RoundedMode`.
    rounded_mode: AtomicU8,
    /// Track info display mode (`TrackInfoDisplay` discriminant)
    track_info_display: AtomicU8,
    /// Navigation layout (`NavLayout` discriminant: Top / Side / None)
    nav_layout: AtomicU8,
    /// Navigation display (`NavDisplayMode` discriminant)
    nav_display_mode: AtomicU8,
    /// Target row height for slot lists (`SlotRowHeight` discriminant)
    slot_row_height: AtomicU8,
    /// Whether the opacity gradient on non-center slots is enabled
    opacity_gradient: AtomicBool,
    /// Whether clickable text links in slot list items are enabled
    slot_text_links: AtomicBool,
    /// Whether volume sliders are displayed horizontally in the player bar
    horizontal_volume: AtomicBool,
    /// Whether the view-header toolbar auto-hides to a thin line until hovered
    autohide_toolbar: AtomicBool,
    /// Collapsed auto-hide toolbar height in px (user-configurable)
    autohide_toolbar_height: AtomicU8,
    /// Whether the collapsed auto-hide toolbar shows a centered accent grip bar
    autohide_toolbar_grip: AtomicBool,
    /// What the collapsed auto-hide toolbar shows (Hairline / Hidden / Count strip)
    autohide_collapsed_appearance: AtomicU8,
    /// Whether the mini-player bar shows the volume slider (mini-player mode only)
    mini_player_show_volume: AtomicBool,
    /// Whether the mini-player bar shows the mode toggles / kebab menu
    /// (mini-player mode only)
    mini_player_show_modes: AtomicBool,
    /// Whether the title field is shown in the track info strip
    strip_show_title: AtomicBool,
    /// Whether the artist field is shown in the track info strip
    strip_show_artist: AtomicBool,
    /// Whether the album field is shown in the track info strip
    strip_show_album: AtomicBool,
    /// Whether format info (codec/kHz/kbps) is shown in the track info strip
    strip_show_format_info: AtomicBool,
    /// Whether the metastrip renders artist/album/title as a single shared
    /// scrolling unit with one set of bookends.
    strip_merged_mode: AtomicBool,
    /// Strip click action (`StripClickAction` discriminant)
    strip_click_action: AtomicU8,
    /// Whether `title:` / `artist:` / `album:` labels are prepended to fields
    strip_show_labels: AtomicBool,
    /// Strip merged-mode separator (`StripSeparator` discriminant)
    strip_separator: AtomicU8,
    /// Whether the metadata text overlay is rendered on the large artwork in Albums view
    albums_artwork_overlay: AtomicBool,
    /// Whether the metadata text overlay is rendered on the large artwork in Artists view
    artists_artwork_overlay: AtomicBool,
    /// Whether the metadata text overlay is rendered on the large artwork in Songs view
    songs_artwork_overlay: AtomicBool,
    /// Whether the metadata text overlay is rendered on the large artwork in Playlists view
    playlists_artwork_overlay: AtomicBool,
    /// Artwork column display mode (`ArtworkColumnMode` discriminant)
    artwork_column_mode: AtomicU8,
    /// Artwork stretch fit when column mode is AlwaysStretched or
    /// AlwaysVerticalStretched (`ArtworkStretchFit` discriminant)
    artwork_column_stretch_fit: AtomicU8,
    /// Artwork column width as fraction of window width (f32 bits, 0.05..=0.80)
    artwork_column_width_pct: AtomicU32,
    /// Auto-mode max artwork size as fraction of window short axis
    /// (f32 bits, 0.30..=0.70). Read by the Auto-mode resolver in
    /// base_slot_list_layout.rs to size both the horizontal candidate and the
    /// vertical-portrait fallback.
    artwork_auto_max_pct: AtomicU32,
    /// Always-Vertical artwork height as fraction of window height
    /// (f32 bits, 0.10..=0.80). Read by the Always-Vertical resolver branch
    /// in base_slot_list_layout.rs to size the stacked artwork.
    artwork_vertical_height_pct: AtomicU32,
}

static UI_MODE: UiModeFlags = UiModeFlags {
    light_mode: AtomicBool::new(false),
    // RoundedMode::Off (not the enum's `#[default]`). PlayerSettings load
    // corrects this to the user's preference on first dump.
    rounded_mode: AtomicU8::new(RoundedMode::Off as u8),
    track_info_display: AtomicU8::new(TrackInfoDisplay::Off as u8),
    nav_layout: AtomicU8::new(NavLayout::Top as u8),
    nav_display_mode: AtomicU8::new(NavDisplayMode::TextOnly as u8),
    slot_row_height: AtomicU8::new(SlotRowHeight::Default as u8),
    opacity_gradient: AtomicBool::new(true),
    slot_text_links: AtomicBool::new(true),
    horizontal_volume: AtomicBool::new(false),
    autohide_toolbar: AtomicBool::new(false),
    autohide_toolbar_height: AtomicU8::new(6),
    autohide_toolbar_grip: AtomicBool::new(true),
    autohide_collapsed_appearance: AtomicU8::new(CollapsedAppearance::Hairline as u8),
    mini_player_show_volume: AtomicBool::new(true),
    mini_player_show_modes: AtomicBool::new(true),
    strip_show_title: AtomicBool::new(true),
    strip_show_artist: AtomicBool::new(true),
    strip_show_album: AtomicBool::new(true),
    strip_show_format_info: AtomicBool::new(true),
    strip_merged_mode: AtomicBool::new(false),
    strip_click_action: AtomicU8::new(StripClickAction::GoToQueue as u8),
    strip_show_labels: AtomicBool::new(true),
    strip_separator: AtomicU8::new(StripSeparator::Slash as u8),
    albums_artwork_overlay: AtomicBool::new(true),
    artists_artwork_overlay: AtomicBool::new(true),
    songs_artwork_overlay: AtomicBool::new(true),
    playlists_artwork_overlay: AtomicBool::new(true),
    artwork_column_mode: AtomicU8::new(ArtworkColumnMode::Auto as u8),
    artwork_column_stretch_fit: AtomicU8::new(ArtworkStretchFit::Cover as u8),
    // Initial values mirror the data-crate defaults in
    // `nokkvi_data::types::player_settings::artwork`. `f32::to_bits` is `const`
    // so the bit pattern is derived at compile time — no magic hex.
    artwork_column_width_pct: AtomicU32::new(
        nokkvi_data::types::player_settings::ARTWORK_COLUMN_WIDTH_PCT_DEFAULT.to_bits(),
    ),
    artwork_auto_max_pct: AtomicU32::new(
        nokkvi_data::types::player_settings::ARTWORK_AUTO_MAX_PCT_DEFAULT.to_bits(),
    ),
    artwork_vertical_height_pct: AtomicU32::new(
        nokkvi_data::types::player_settings::ARTWORK_VERTICAL_HEIGHT_PCT_DEFAULT.to_bits(),
    ),
};

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
fn read_color<F: FnOnce(&ResolvedTheme) -> Color>(f: F) -> Color {
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
// Font Configuration (independent of theme — stored in config.toml)
// ============================================================================

/// Font family name, stored separately from themes so theme switches
/// don't change the user's font preference.
static FONT_FAMILY: LazyLock<RwLock<String>> = LazyLock::new(|| RwLock::new(String::new()));

/// Cached font storage to avoid leaking memory on every reload.
/// Stores (font_name, resolved_font) pairs.
static FONT_CACHE: LazyLock<RwLock<(String, Font)>> =
    LazyLock::new(|| RwLock::new((String::new(), Font::DEFAULT)));

/// Get the UI font - loaded from config.toml (hot-reloadable)
/// Default: System sans-serif font (works on all systems)
#[inline]
pub(crate) fn ui_font() -> Font {
    let current_family = { FONT_FAMILY.read().clone() };

    // Fast path: check if cached font matches current config
    {
        let cache = FONT_CACHE.read();
        if cache.0 == current_family {
            return cache.1;
        }
    }

    // Slow path: font changed, update cache
    let new_font = if current_family.is_empty() {
        Font::DEFAULT
    } else {
        // Family::name() interns the string in a global FxHashSet (leaked once, deduped)
        Font::with_family(iced::font::Family::name(&current_family))
    };

    let mut cache = FONT_CACHE.write();
    *cache = (current_family, new_font);

    new_font
}

/// Set the font family (called on startup and when changed in settings).
pub(crate) fn set_font_family(family: String) {
    let mut guard = FONT_FAMILY.write();
    *guard = family;
}

/// Get the current font family name (for settings UI display).
pub(crate) fn font_family() -> String {
    FONT_FAMILY.read().clone()
}

// ----------------------------------------------------------------------------
// Title font (independent of body font — supports italic/serif title
// typefaces like IBM Plex Sans without affecting JetBrains-Mono-style body text)
// ----------------------------------------------------------------------------

// Title-font family was intended to give L1 hero titles + active-breadcrumb
// segments a typographic distinction from the body font, but the
// `set_title_font_family` setter never landed (the L5 settings lane it was
// dead-coded for was descoped). Every `theme::title_font()` call therefore
// returned the body font, shipping the design's distinction disabled. The
// machinery (`TITLE_FONT_FAMILY` static + `TITLE_FONT_CACHE` + the public
// readers) was removed — callers now use `ui_font()` directly. If a future
// design re-introduces the distinction, the simpler shape is a config-side
// `theme.title_font_family` field threaded through `ResolvedTheme` rather
// than a global mutable static.

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
    UI_MODE.light_mode.store(enabled, Ordering::Relaxed);
    THEME_GENERATION.fetch_add(1, Ordering::Relaxed);
    debug!(" Theme mode changed: light_mode={}", enabled);
}

// ============================================================================
// Rounded Mode Control
// ============================================================================

/// Legacy single-radius value (kept for `ui_border_radius()` back-compat).
/// New code prefers the scale helpers (`ui_radius_sm`, `ui_radius_md`, …).
const ROUNDED_RADIUS: f32 = 6.0;

// ----------------------------------------------------------------------------
// Radius scale (flat redesign — rounded-mode values per element role)
// ----------------------------------------------------------------------------
// Each helper returns the corresponding radius in rounded mode, `0.0` in flat
// mode, so call sites stay mode-agnostic.

// Scale constants — every helper below consumes one, so they're all live.

/// xs (4 px) — checkboxes, hex swatches, small pips.
const R_XS: f32 = 4.0;
/// sm (8 px) — mode buttons, badges, hover pills.
const R_SM: f32 = 8.0;
/// md (12 px) — cards, popovers, album art, category tiles.
const R_MD: f32 = 12.0;
/// lg (18 px) — list shells, modal frames, hero panels.
const R_LG: f32 = 18.0;
/// pill (999 px) — tabs, transport buttons, search, sliders.
const R_PILL: f32 = 999.0;

/// Returns the current rounded-corners mode enum (Off / On / PlayerOnly).
#[inline]
pub(crate) fn rounded_mode() -> RoundedMode {
    RoundedMode::from_u8(UI_MODE.rounded_mode.load(Ordering::Relaxed))
}

/// Returns true when **every** UI surface should render with rounded corners.
///
/// True only when the active mode is [`RoundedMode::On`]. `PlayerOnly` returns
/// `false` from here — only the player chrome rounds in that mode (see
/// [`is_rounded_for_player`]).
#[inline]
pub(crate) fn is_rounded_mode() -> bool {
    rounded_mode() == RoundedMode::On
}

/// Returns true when the **bottom playback chrome** should render with rounded
/// corners. True for [`RoundedMode::On`] and [`RoundedMode::PlayerOnly`].
///
/// Player widgets (player bar, progress bar, volume slider, mode menu) and the
/// bottom track-info strip route their radius helpers through this predicate
/// instead of [`is_rounded_mode`] so the `PlayerOnly` mode keeps the player
/// soft while the rest of the UI stays flat.
#[inline]
pub(crate) fn is_rounded_for_player() -> bool {
    matches!(rounded_mode(), RoundedMode::On | RoundedMode::PlayerOnly)
}

/// Set rounded corners mode (call when user toggles the setting)
#[inline]
pub(crate) fn set_rounded_mode(mode: RoundedMode) {
    UI_MODE.rounded_mode.store(mode.to_u8(), Ordering::Relaxed);
    debug!(" Rounded mode changed: rounded_mode={}", mode);
}

/// Gate a radius value: return `value` when `gate` is set, `0.0` (square) when
/// not. The single shared body behind every `ui_radius_*` / `ui_border_radius*`
/// helper — each public helper supplies only its gate predicate
/// ([`is_rounded_mode`] or [`is_rounded_for_player`]) and its scale constant.
/// `#[inline]` so the per-frame render path pays nothing for the indirection.
#[inline]
fn gated_radius(gate: bool, value: f32) -> iced::border::Radius {
    if gate { value.into() } else { 0.0.into() }
}

/// Get the legacy UI border radius (6 px in rounded mode, 0 in flat).
///
/// Kept for back-compat while widgets migrate to the scale helpers
/// (`ui_radius_xs/sm/md/lg/pill`). New code should call the role-appropriate
/// helper directly. Player widgets must call [`ui_border_radius_player`].
#[inline]
pub(crate) fn ui_border_radius() -> iced::border::Radius {
    gated_radius(is_rounded_mode(), ROUNDED_RADIUS)
}

/// Player-chrome variant of [`ui_border_radius`] — rounds for `On` AND
/// `PlayerOnly`. Use from player_bar / progress_bar / volume_slider /
/// player_modes_menu and the `PlayerBar`-scoped track info strip.
#[inline]
pub(crate) fn ui_border_radius_player() -> iced::border::Radius {
    gated_radius(is_rounded_for_player(), ROUNDED_RADIUS)
}

/// Scale step `xs` — 4 px in rounded mode, 0 in flat. Use for checkboxes,
/// swatches, tiny chips.
#[inline]
pub(crate) fn ui_radius_xs() -> iced::border::Radius {
    gated_radius(is_rounded_mode(), R_XS)
}

/// Scale step `sm` — 8 px in rounded mode, 0 in flat. Use for mode buttons,
/// badges, format pills.
#[inline]
pub(crate) fn ui_radius_sm() -> iced::border::Radius {
    gated_radius(is_rounded_mode(), R_SM)
}

/// Scale step `md` — 12 px in rounded mode, 0 in flat. Use for cards,
/// popovers, album art, category tiles.
#[inline]
pub(crate) fn ui_radius_md() -> iced::border::Radius {
    gated_radius(is_rounded_mode(), R_MD)
}

/// Scale step `lg` — 18 px in rounded mode, 0 in flat. Use for list shells,
/// modal frames, stats strips.
#[inline]
pub(crate) fn ui_radius_lg() -> iced::border::Radius {
    gated_radius(is_rounded_mode(), R_LG)
}

/// Scale step `pill` — 999 px in rounded mode, 0 in flat. Use for tabs,
/// transport buttons, search field, slider handles.
#[inline]
pub(crate) fn ui_radius_pill() -> iced::border::Radius {
    gated_radius(is_rounded_mode(), R_PILL)
}

// ----------------------------------------------------------------------------
// Player-chrome radius helpers — round in `On` AND `PlayerOnly`.
// ----------------------------------------------------------------------------
//
// The parallel `_player` family gates on [`is_rounded_for_player`] instead of
// [`is_rounded_mode`], so the bottom playback chrome stays soft when the user
// picks `RoundedMode::PlayerOnly` even though the rest of the UI is flat.
// Used by player_bar, progress_bar, volume_slider, and player_modes_menu.
// Add the corresponding `_player` variant when a player widget needs the
// `xs`, `md`, or `lg` step; the goal is for every radius decision inside the
// bottom playback chrome to route through a `_player` helper, never the
// global `is_rounded_mode()`-gated set.

/// Player-chrome variant of [`ui_radius_sm`].
#[inline]
pub(crate) fn ui_radius_sm_player() -> iced::border::Radius {
    gated_radius(is_rounded_for_player(), R_SM)
}

/// Player-chrome variant of [`ui_radius_pill`].
#[inline]
pub(crate) fn ui_radius_pill_player() -> iced::border::Radius {
    gated_radius(is_rounded_for_player(), R_PILL)
}

// ----------------------------------------------------------------------------
// Chrome sizing (mode-sensitive)
// ----------------------------------------------------------------------------

/// Top nav-bar content height — 32 px in flat mode, 44 px in rounded mode
/// (rounded mode adds 6 px padding above and below the pill capsules).
#[inline]
pub(crate) fn nav_bar_height() -> f32 {
    if is_rounded_mode() { 44.0 } else { 32.0 }
}

/// Fixed height of the 24 px status strip rendered below the player bar.
/// Consumed by `widgets::track_info_strip::STRIP_HEIGHT` to keep the strip
/// widget's height aligned with the theme's source of truth.
pub(crate) const STATUS_STRIP_HEIGHT: f32 = 24.0;

/// Minimum relative-luminance separation between the light-mode status-strip
/// band and `bg0_hard()`, so the strip reads as its own band instead of
/// vanishing into warm cream chrome. Dark mode keeps the calibrated darken.
const STRIP_BAND_DELTA: f32 = 0.035;

/// Background color for the 24 px status strip — a band distinct from
/// `bg0_hard()` below the player bar.
///
/// Dark mode keeps the original calibrated darken (Everforest target:
/// `bg0_hard=#232A2E` → `status_strip=#1d2326`, a ~17 % darken) — the look the
/// design was tuned around and which already reads well on every dark theme.
/// Light mode cannot darken toward black without muddying warm cream chrome
/// into a dingy grey, so it instead blends `bg0_hard()` toward the theme's own
/// foreground ink (`fg0()`) just until the band clears [`STRIP_BAND_DELTA`],
/// keeping the band on-palette in the theme's own hue.
#[inline]
pub(crate) fn status_strip_bg() -> Color {
    if is_light_mode() {
        strip_band_toward_ink(bg0_hard(), fg0(), STRIP_BAND_DELTA)
    } else {
        darken(bg0_hard(), 0.17)
    }
}

/// Blend `base` toward `ink` just far enough that their relative luminance
/// differs by at least `delta` (capped at a 0.6 blend so the band stays a
/// subtle tint, never a full ink fill). Backs the light-mode status strip.
fn strip_band_toward_ink(base: Color, ink: Color, delta: f32) -> Color {
    let l_base = relative_luminance(base);
    let mut amount = 0.0_f32;
    let mut band = base;
    while amount < 0.60 {
        amount += 0.02;
        band = blend_toward(base, ink, amount);
        if (relative_luminance(band) - l_base).abs() >= delta {
            break;
        }
    }
    band
}

// `active_accent()` was retained as the canonical accent-resolver for any
// future surface that wanted the rounded-light contrast bump (returning
// `accent()` only in `rounded && light` mode, `accent_bright()` otherwise),
// but every redesign tab/cell already calls `accent_bright()` directly and
// nothing surfaced a real need for the mode-conditional shape. Removed
// during the cleanup; a future agent reintroducing the pattern can pull it
// from `git show`.

use nokkvi_data::types::player_settings::{RoundedMode, TrackInfoDisplay};

use crate::atomic_u8_enum::{AtomicU8Enum, atomic_u8_enum};

atomic_u8_enum! {
    TrackInfoDisplay {
        Off,
        PlayerBar,
        TopBar,
        TopBarUnder,
        MiniPlayer,
    } default Off
}

atomic_u8_enum! {
    RoundedMode {
        Off,
        On,
        PlayerOnly,
    } default Off
}

/// Returns the current track info display mode
#[inline]
pub(crate) fn track_info_display() -> TrackInfoDisplay {
    TrackInfoDisplay::from_u8(UI_MODE.track_info_display.load(Ordering::Relaxed))
}

/// Set track info display mode (call when user changes the setting)
#[inline]
pub(crate) fn set_track_info_display(mode: TrackInfoDisplay) {
    UI_MODE
        .track_info_display
        .store(mode.to_u8(), Ordering::Relaxed);
    debug!(" Track info display changed: {}", mode);
}

/// Whether the player bar should show the track info strip below controls.
///
/// True when `TrackInfoDisplay::PlayerBar` is active.
///
/// **Single source of truth** — use this instead of ad-hoc compound checks.
#[inline]
pub(crate) fn show_player_bar_strip() -> bool {
    track_info_display() == TrackInfoDisplay::PlayerBar
}

/// Whether the top bar track info strip should be rendered above content.
///
/// True when the active strip mode renders a strip above the main content in
/// side-nav or none-nav layouts. Both `TopBar` and `TopBarUnder` map to the
/// same above-content position there — they only diverge in top-nav layout
/// (where `TopBar` lives inline in the nav row and `TopBarUnder` becomes its
/// own row below the nav — see [`show_top_bar_under_strip`]).
///
/// **Single source of truth** — use this instead of ad-hoc compound checks.
#[inline]
pub(crate) fn show_top_bar_strip() -> bool {
    matches!(
        track_info_display(),
        TrackInfoDisplay::TopBar | TrackInfoDisplay::TopBarUnder,
    ) && !is_top_nav()
}

/// Whether the player-bar-styled metadata strip should be rendered directly
/// beneath the top nav bar.
///
/// True when `TrackInfoDisplay::TopBarUnder` AND top-nav layout are both
/// active. The other layouts route through [`show_top_bar_strip`] instead.
#[inline]
pub(crate) fn show_top_bar_under_strip() -> bool {
    track_info_display() == TrackInfoDisplay::TopBarUnder && is_top_nav()
}

/// Whether the artwork-elevation feature is *enabled* by the user's theme
/// settings.
///
/// True when the top-nav layout is active AND the metadata strip lives
/// somewhere other than the top bar (i.e. `Off`, `PlayerBar`, or
/// `MiniPlayer`) — in those modes the top nav doesn't carry any
/// now-playing metadata, so its right portion is free real estate that
/// the artwork can take over. `MiniPlayer` keeps its own artwork inside
/// the player bar; that doesn't conflict with the top-nav elevation
/// because they live on different rows.
///
/// `TopBar` keeps the regular column-stacked layout because the metadata
/// strip still needs the full nav width. `TopBarUnder` likewise opts out:
/// its strip sits as its own full-width row directly beneath the nav, so
/// elevating the artwork into that band would either cover the strip or
/// require the strip to span only the slot-list column. Easier to just
/// disable elevation whenever a top-area strip is visible.
///
/// This is only the *theme* gate — `Nokkvi::elevated_artwork_extent`
/// additionally excludes split-view, ineligible views, and the Auto-mode
/// portrait fallback before publishing the result through each `*ViewData`
/// as `BaseSlotListLayoutConfig::elevated`, which `horizontal_layout`
/// finally reads.
#[inline]
pub(crate) fn is_artwork_elevated() -> bool {
    is_top_nav()
        && !matches!(
            track_info_display(),
            TrackInfoDisplay::TopBar | TrackInfoDisplay::TopBarUnder,
        )
}

// ============================================================================
// Nav Layout Control
// ============================================================================

use nokkvi_data::types::player_settings::{NavDisplayMode, NavLayout};

atomic_u8_enum! {
    NavLayout {
        Top,
        Side,
        None,
    } default Top
}

/// Returns true if side navigation layout is active
#[inline]
pub(crate) fn is_side_nav() -> bool {
    UI_MODE.nav_layout.load(Ordering::Relaxed) == NavLayout::Side as u8
}

/// Returns true if the minimalist (no-chrome) layout is active
#[inline]
pub(crate) fn is_none_nav() -> bool {
    UI_MODE.nav_layout.load(Ordering::Relaxed) == NavLayout::None as u8
}

/// Returns true if the top-bar navigation layout is active (the default)
#[inline]
pub(crate) fn is_top_nav() -> bool {
    UI_MODE.nav_layout.load(Ordering::Relaxed) == NavLayout::Top as u8
}

/// Current navigation layout — bytes round-trip through `NavLayout::from_u8`
/// (unknown bytes fall back to `Top`, the declared default; see the
/// `atomic_u8_enum!` macro's defensive-fallback contract).
///
/// Test-only: production code uses `is_top_nav()` / `is_side_nav()` /
/// `is_none_nav()` directly; the enum-shaped reader is here so chrome-math
/// tests can save/restore the active variant in a single hop.
#[cfg(test)]
#[inline]
pub(crate) fn nav_layout() -> NavLayout {
    NavLayout::from_u8(UI_MODE.nav_layout.load(Ordering::Relaxed))
}

/// Set the navigation layout from a NavLayout enum value
#[inline]
pub(crate) fn set_nav_layout(layout: NavLayout) {
    UI_MODE.nav_layout.store(layout.to_u8(), Ordering::Relaxed);
    debug!(" Nav layout changed: nav_layout={}", layout);
}

// ============================================================================
// Nav Display Mode Control
// ============================================================================

atomic_u8_enum! {
    NavDisplayMode {
        TextOnly,
        TextAndIcons,
        IconsOnly,
    } default TextOnly
}

/// Get the current navigation display mode
#[inline]
pub(crate) fn nav_display_mode() -> NavDisplayMode {
    NavDisplayMode::from_u8(UI_MODE.nav_display_mode.load(Ordering::Relaxed))
}

/// Set the navigation display mode from a NavDisplayMode enum value
#[inline]
pub(crate) fn set_nav_display_mode(mode: NavDisplayMode) {
    UI_MODE
        .nav_display_mode
        .store(mode.to_u8(), Ordering::Relaxed);
    debug!(" Nav display mode changed: nav_display_mode={}", mode);
}

// ============================================================================
// Slot Row Height Control
// ============================================================================

/// Get the current target row height for slot lists (in pixels)
#[inline]
pub(crate) fn slot_row_height() -> f32 {
    let variant = slot_row_height_variant();
    variant.to_pixels() as f32
}

use nokkvi_data::types::player_settings::SlotRowHeight;

atomic_u8_enum! {
    SlotRowHeight {
        Compact,
        Default,
        Comfortable,
        Spacious,
    } default Default
}

/// Get the current slot row height enum variant
#[inline]
pub(crate) fn slot_row_height_variant() -> SlotRowHeight {
    SlotRowHeight::from_u8(UI_MODE.slot_row_height.load(Ordering::Relaxed))
}

/// Set the target row height for slot lists
#[inline]
pub(crate) fn set_slot_row_height(height: SlotRowHeight) {
    UI_MODE
        .slot_row_height
        .store(height.to_u8(), Ordering::Relaxed);
    debug!(
        " Slot row height changed: {} ({}px)",
        height.as_label(),
        height.to_pixels()
    );
}

// ============================================================================
// Opacity Gradient Control
// ============================================================================

/// Returns true if the distance-based opacity gradient is enabled on slot lists
#[inline]
pub(crate) fn is_opacity_gradient() -> bool {
    UI_MODE.opacity_gradient.load(Ordering::Relaxed)
}

/// Set opacity gradient state (call when user toggles the setting)
#[inline]
pub(crate) fn set_opacity_gradient(enabled: bool) {
    UI_MODE.opacity_gradient.store(enabled, Ordering::Relaxed);
    debug!(" Opacity gradient changed: opacity_gradient={}", enabled);
}

// ============================================================================
// Slot Text Links Control
// ============================================================================

/// Returns true if clickable text links in slot list items are enabled
#[inline]
pub(crate) fn is_slot_text_links() -> bool {
    UI_MODE.slot_text_links.load(Ordering::Relaxed)
}

/// Set slot text links state (call when user toggles the setting)
#[inline]
pub(crate) fn set_slot_text_links(enabled: bool) {
    UI_MODE.slot_text_links.store(enabled, Ordering::Relaxed);
    debug!(" Slot text links changed: slot_text_links={}", enabled);
}

// ============================================================================
// Horizontal Volume Control
// ============================================================================

/// Returns true if horizontal volume sliders are enabled in the player bar
#[inline]
pub(crate) fn is_horizontal_volume() -> bool {
    UI_MODE.horizontal_volume.load(Ordering::Relaxed)
}

/// Set horizontal volume slider mode (call when user toggles the setting)
#[inline]
pub(crate) fn set_horizontal_volume(enabled: bool) {
    UI_MODE.horizontal_volume.store(enabled, Ordering::Relaxed);
    debug!(" Horizontal volume changed: horizontal_volume={}", enabled);
}

/// Returns true if the view-header toolbar auto-hides until hovered / shortcut
#[inline]
pub(crate) fn is_autohide_toolbar() -> bool {
    UI_MODE.autohide_toolbar.load(Ordering::Relaxed)
}

/// Set view-header toolbar auto-hide mode (call when user toggles the setting)
#[inline]
pub(crate) fn set_autohide_toolbar(enabled: bool) {
    UI_MODE.autohide_toolbar.store(enabled, Ordering::Relaxed);
    debug!(" Autohide toolbar changed: autohide_toolbar={}", enabled);
}

/// Collapsed auto-hide toolbar height in px (user-configurable).
#[inline]
pub(crate) fn autohide_toolbar_height_px() -> u8 {
    UI_MODE.autohide_toolbar_height.load(Ordering::Relaxed)
}

/// Set the collapsed auto-hide toolbar height in px.
#[inline]
pub(crate) fn set_autohide_toolbar_height_px(px: u8) {
    UI_MODE.autohide_toolbar_height.store(px, Ordering::Relaxed);
}

/// Returns true if the collapsed auto-hide toolbar shows its accent grip bar.
#[inline]
pub(crate) fn is_autohide_toolbar_grip() -> bool {
    UI_MODE.autohide_toolbar_grip.load(Ordering::Relaxed)
}

/// Set whether the collapsed auto-hide toolbar shows its accent grip bar.
#[inline]
pub(crate) fn set_autohide_toolbar_grip(enabled: bool) {
    UI_MODE
        .autohide_toolbar_grip
        .store(enabled, Ordering::Relaxed);
}

use nokkvi_data::types::player_settings::CollapsedAppearance;

atomic_u8_enum! {
    CollapsedAppearance {
        Hairline,
        Hidden,
        CountStrip,
    } default Hairline
}

/// What the collapsed auto-hide toolbar shows (Hairline / Hidden / Count strip).
#[inline]
pub(crate) fn autohide_collapsed_appearance() -> CollapsedAppearance {
    CollapsedAppearance::from_u8(
        UI_MODE
            .autohide_collapsed_appearance
            .load(Ordering::Relaxed),
    )
}

/// Set the collapsed auto-hide toolbar appearance.
#[inline]
pub(crate) fn set_autohide_collapsed_appearance(mode: CollapsedAppearance) {
    UI_MODE
        .autohide_collapsed_appearance
        .store(mode.to_u8(), Ordering::Relaxed);
}

/// Returns true if the mini-player bar shows the volume slider (mini-player
/// mode only)
#[inline]
pub(crate) fn mini_player_show_volume() -> bool {
    UI_MODE.mini_player_show_volume.load(Ordering::Relaxed)
}

/// Set whether the mini-player bar shows the volume slider (call when the user
/// toggles the setting)
#[inline]
pub(crate) fn set_mini_player_show_volume(shown: bool) {
    UI_MODE
        .mini_player_show_volume
        .store(shown, Ordering::Relaxed);
    debug!("Mini-player show volume changed: mini_player_show_volume={shown}");
}

/// Returns true if the mini-player bar shows the mode toggles / kebab menu
/// (mini-player mode only)
#[inline]
pub(crate) fn mini_player_show_modes() -> bool {
    UI_MODE.mini_player_show_modes.load(Ordering::Relaxed)
}

/// Set whether the mini-player bar shows the mode toggles / kebab menu (call
/// when the user toggles the setting)
#[inline]
pub(crate) fn set_mini_player_show_modes(shown: bool) {
    UI_MODE
        .mini_player_show_modes
        .store(shown, Ordering::Relaxed);
    debug!("Mini-player show modes changed: mini_player_show_modes={shown}");
}

// ============================================================================
// Strip Field Visibility Controls
// ============================================================================

use nokkvi_data::types::player_settings::{StripClickAction, StripSeparator};

atomic_u8_enum! {
    StripClickAction {
        GoToQueue,
        GoToAlbum,
        GoToArtist,
        CopyTrackInfo,
        DoNothing,
    } default GoToQueue
}

atomic_u8_enum! {
    StripSeparator {
        Dot,
        Bullet,
        Pipe,
        EmDash,
        Slash,
        Bar,
    } default Slash
}

/// Returns true if the title field is visible in the track info strip
#[inline]
pub(crate) fn strip_show_title() -> bool {
    UI_MODE.strip_show_title.load(Ordering::Relaxed)
}

/// Set strip title visibility
#[inline]
pub(crate) fn set_strip_show_title(enabled: bool) {
    UI_MODE.strip_show_title.store(enabled, Ordering::Relaxed);
}

/// Returns true if the artist field is visible in the track info strip
#[inline]
pub(crate) fn strip_show_artist() -> bool {
    UI_MODE.strip_show_artist.load(Ordering::Relaxed)
}

/// Set strip artist visibility
#[inline]
pub(crate) fn set_strip_show_artist(enabled: bool) {
    UI_MODE.strip_show_artist.store(enabled, Ordering::Relaxed);
}

/// Returns true if the album field is visible in the track info strip
#[inline]
pub(crate) fn strip_show_album() -> bool {
    UI_MODE.strip_show_album.load(Ordering::Relaxed)
}

/// Set strip album visibility
#[inline]
pub(crate) fn set_strip_show_album(enabled: bool) {
    UI_MODE.strip_show_album.store(enabled, Ordering::Relaxed);
}

/// Returns true if format info (codec/kHz/kbps) is visible in the track info strip
#[inline]
pub(crate) fn strip_show_format_info() -> bool {
    UI_MODE.strip_show_format_info.load(Ordering::Relaxed)
}

/// Set strip format info visibility
#[inline]
pub(crate) fn set_strip_show_format_info(enabled: bool) {
    UI_MODE
        .strip_show_format_info
        .store(enabled, Ordering::Relaxed);
}

/// Returns true if the metastrip renders artist/album/title as a single
/// shared scrolling unit with one set of bookend separators.
#[inline]
pub(crate) fn strip_merged_mode() -> bool {
    UI_MODE.strip_merged_mode.load(Ordering::Relaxed)
}

/// Set strip merged mode
#[inline]
pub(crate) fn set_strip_merged_mode(enabled: bool) {
    UI_MODE.strip_merged_mode.store(enabled, Ordering::Relaxed);
}

/// Returns the current strip click action
#[inline]
pub(crate) fn strip_click_action() -> StripClickAction {
    StripClickAction::from_u8(UI_MODE.strip_click_action.load(Ordering::Relaxed))
}

/// Set strip click action (call when user changes the setting)
#[inline]
pub(crate) fn set_strip_click_action(action: StripClickAction) {
    UI_MODE
        .strip_click_action
        .store(action.to_u8(), Ordering::Relaxed);
}

/// Returns true if `title:` / `artist:` / `album:` labels are shown in the strip
#[inline]
pub(crate) fn strip_show_labels() -> bool {
    UI_MODE.strip_show_labels.load(Ordering::Relaxed)
}

/// Set strip label visibility
#[inline]
pub(crate) fn set_strip_show_labels(enabled: bool) {
    UI_MODE.strip_show_labels.store(enabled, Ordering::Relaxed);
}

/// Returns the active strip merged-mode field separator
#[inline]
pub(crate) fn strip_separator() -> StripSeparator {
    StripSeparator::from_u8(UI_MODE.strip_separator.load(Ordering::Relaxed))
}

/// Set strip merged-mode field separator
#[inline]
pub(crate) fn set_strip_separator(sep: StripSeparator) {
    UI_MODE
        .strip_separator
        .store(sep.to_u8(), Ordering::Relaxed);
}

// ============================================================================
// Per-View Artwork Text Overlay Controls
// ============================================================================

/// Returns true if the metadata text overlay is shown on the large artwork in Albums view
#[inline]
pub(crate) fn albums_artwork_overlay() -> bool {
    UI_MODE.albums_artwork_overlay.load(Ordering::Relaxed)
}

/// Set the Albums view artwork text overlay visibility
#[inline]
pub(crate) fn set_albums_artwork_overlay(enabled: bool) {
    UI_MODE
        .albums_artwork_overlay
        .store(enabled, Ordering::Relaxed);
}

/// Returns true if the metadata text overlay is shown on the large artwork in Artists view
#[inline]
pub(crate) fn artists_artwork_overlay() -> bool {
    UI_MODE.artists_artwork_overlay.load(Ordering::Relaxed)
}

/// Set the Artists view artwork text overlay visibility
#[inline]
pub(crate) fn set_artists_artwork_overlay(enabled: bool) {
    UI_MODE
        .artists_artwork_overlay
        .store(enabled, Ordering::Relaxed);
}

/// Returns true if the metadata text overlay is shown on the large artwork in Songs view
#[inline]
pub(crate) fn songs_artwork_overlay() -> bool {
    UI_MODE.songs_artwork_overlay.load(Ordering::Relaxed)
}

/// Set the Songs view artwork text overlay visibility
#[inline]
pub(crate) fn set_songs_artwork_overlay(enabled: bool) {
    UI_MODE
        .songs_artwork_overlay
        .store(enabled, Ordering::Relaxed);
}

/// Returns true if the metadata text overlay is shown on the large artwork in Playlists view
#[inline]
pub(crate) fn playlists_artwork_overlay() -> bool {
    UI_MODE.playlists_artwork_overlay.load(Ordering::Relaxed)
}

/// Set the Playlists view artwork text overlay visibility
#[inline]
pub(crate) fn set_playlists_artwork_overlay(enabled: bool) {
    UI_MODE
        .playlists_artwork_overlay
        .store(enabled, Ordering::Relaxed);
}

// ============================================================================
// Artwork Column Layout
// ============================================================================

use nokkvi_data::types::player_settings::{
    ARTWORK_AUTO_MAX_PCT_MAX, ARTWORK_AUTO_MAX_PCT_MIN, ARTWORK_COLUMN_WIDTH_PCT_MAX,
    ARTWORK_COLUMN_WIDTH_PCT_MIN, ARTWORK_VERTICAL_HEIGHT_PCT_MAX, ARTWORK_VERTICAL_HEIGHT_PCT_MIN,
    ArtworkColumnMode, ArtworkStretchFit,
};

// Encoding NOTE: the bytes are the enum's declaration discriminants and are
// a transient in-process cache encoding only — nothing persists them
// (persistence goes through serde wire strings), so renumbering variants is
// safe. The `atomic_u8_enum!` loader falls back to `Auto` for unknown values
// and the store half is enum-exhaustive (so adding a variant to the
// data-crate enum forces a compile error here).
atomic_u8_enum! {
    ArtworkColumnMode {
        Auto,
        AlwaysNative,
        AlwaysStretched,
        AlwaysVerticalNative,
        AlwaysVerticalStretched,
        Never,
    } default Auto
}

atomic_u8_enum! {
    ArtworkStretchFit {
        Cover,
        Fill,
    } default Cover
}

/// Returns the active artwork column display mode.
#[inline]
pub(crate) fn artwork_column_mode() -> ArtworkColumnMode {
    ArtworkColumnMode::from_u8(UI_MODE.artwork_column_mode.load(Ordering::Relaxed))
}

/// Set the artwork column display mode (call when user changes the setting).
#[inline]
pub(crate) fn set_artwork_column_mode(mode: ArtworkColumnMode) {
    UI_MODE
        .artwork_column_mode
        .store(mode.to_u8(), Ordering::Relaxed);
}

/// Returns the active artwork stretch fit (only meaningful in AlwaysStretched mode).
#[inline]
pub(crate) fn artwork_column_stretch_fit() -> ArtworkStretchFit {
    ArtworkStretchFit::from_u8(UI_MODE.artwork_column_stretch_fit.load(Ordering::Relaxed))
}

/// Set the artwork stretch fit.
#[inline]
pub(crate) fn set_artwork_column_stretch_fit(fit: ArtworkStretchFit) {
    UI_MODE
        .artwork_column_stretch_fit
        .store(fit.to_u8(), Ordering::Relaxed);
}

/// Returns the artwork column width fraction (0.05..=0.80).
#[inline]
pub(crate) fn artwork_column_width_pct() -> f32 {
    f32::from_bits(UI_MODE.artwork_column_width_pct.load(Ordering::Relaxed))
}

/// Set the artwork column width fraction. Clamps into the data-crate range.
#[inline]
pub(crate) fn set_artwork_column_width_pct(pct: f32) {
    let clamped = pct.clamp(ARTWORK_COLUMN_WIDTH_PCT_MIN, ARTWORK_COLUMN_WIDTH_PCT_MAX);
    UI_MODE
        .artwork_column_width_pct
        .store(clamped.to_bits(), Ordering::Relaxed);
}

/// Returns the Auto-mode max artwork fraction (0.30..=0.70). The resolver
/// uses this for both the horizontal candidate and the portrait-fallback
/// vertical candidate.
#[inline]
pub(crate) fn artwork_auto_max_pct() -> f32 {
    f32::from_bits(UI_MODE.artwork_auto_max_pct.load(Ordering::Relaxed))
}

/// Set the Auto-mode max artwork fraction. Clamps into the data-crate range.
#[inline]
pub(crate) fn set_artwork_auto_max_pct(pct: f32) {
    let clamped = pct.clamp(ARTWORK_AUTO_MAX_PCT_MIN, ARTWORK_AUTO_MAX_PCT_MAX);
    UI_MODE
        .artwork_auto_max_pct
        .store(clamped.to_bits(), Ordering::Relaxed);
}

/// Returns the Always-Vertical artwork height fraction (0.10..=0.80). Read
/// by the resolver for AlwaysVerticalNative and AlwaysVerticalStretched.
#[inline]
pub(crate) fn artwork_vertical_height_pct() -> f32 {
    f32::from_bits(UI_MODE.artwork_vertical_height_pct.load(Ordering::Relaxed))
}

/// Set the Always-Vertical artwork height fraction. Clamps into the data-crate range.
#[inline]
pub(crate) fn set_artwork_vertical_height_pct(pct: f32) {
    let clamped = pct.clamp(
        ARTWORK_VERTICAL_HEIGHT_PCT_MIN,
        ARTWORK_VERTICAL_HEIGHT_PCT_MAX,
    );
    UI_MODE
        .artwork_vertical_height_pct
        .store(clamped.to_bits(), Ordering::Relaxed);
}

// ============================================================================
// Background Colors
// ============================================================================

#[inline]
pub(crate) fn bg0_hard() -> Color {
    read_color(|t| t.bg0_hard)
}
#[inline]
pub(crate) fn bg0() -> Color {
    read_color(|t| t.bg0)
}
#[inline]
pub(crate) fn bg0_soft() -> Color {
    read_color(|t| t.bg0_soft)
}
#[inline]
pub(crate) fn bg1() -> Color {
    read_color(|t| t.bg1)
}
#[inline]
pub(crate) fn bg2() -> Color {
    read_color(|t| t.bg2)
}
#[inline]
pub(crate) fn bg3() -> Color {
    read_color(|t| t.bg3)
}

// ============================================================================
// Foreground Colors
// ============================================================================

#[inline]
pub(crate) fn fg4() -> Color {
    read_color(|t| t.fg4)
}
#[inline]
pub(crate) fn fg3() -> Color {
    read_color(|t| t.fg3)
}
#[inline]
pub(crate) fn fg2() -> Color {
    read_color(|t| t.fg2)
}
#[inline]
pub(crate) fn fg1() -> Color {
    read_color(|t| t.fg1)
}
#[inline]
pub(crate) fn fg0() -> Color {
    read_color(|t| t.fg0)
}

// ============================================================================
// Accent Colors
// ============================================================================

#[inline]
pub(crate) fn accent() -> Color {
    read_color(|t| t.accent)
}
#[inline]
pub(crate) fn accent_bright() -> Color {
    read_color(|t| t.accent_bright)
}
#[inline]
pub(crate) fn accent_border_light() -> Color {
    read_color(|t| t.accent_border_light)
}

// The now-playing and selected slot highlights are no longer per-theme stored
// colors. They are derived from the live accent tokens with built-in contrast
// guards — see the "Highlight-fill family" section below
// (`playing_fill` / `selected_fill_resolved` / `legible_text_on`). The
// `accent.now_playing` / `accent.selected` TOML fields are still parsed for
// round-trip compatibility (the `star.base` precedent) but no longer consumed.

/// Semi-transparent accent color for text input selection highlights.
///
/// Iced's `text_input::Style` has no `selected_text_color` field — the text
/// always renders with `style.value` color even during selection. Using an
/// opaque accent background makes text unreadable when theme foreground and
/// accent colors have poor contrast (e.g. peach text on green highlight).
/// A translucent tint lets the underlying background show through, keeping
/// the text readable across all theme combinations.
#[inline]
pub(crate) fn selection_color() -> Color {
    let mut c = accent_bright();
    c.a = 0.35;
    c
}

// ============================================================================
// Semantic Colors
// ============================================================================

#[inline]
pub(crate) fn danger() -> Color {
    read_color(|t| t.danger)
}
#[inline]
pub(crate) fn danger_bright() -> Color {
    read_color(|t| t.danger_bright)
}
#[inline]
pub(crate) fn success() -> Color {
    read_color(|t| t.success)
}
#[inline]
pub(crate) fn warning() -> Color {
    read_color(|t| t.warning)
}
#[inline]
pub(crate) fn warning_bright() -> Color {
    read_color(|t| t.warning_bright)
}
// Base `star()` accessor was retained alongside `star_bright()` for any
// future surface that wanted both ends of the star ratings palette, but
// only `star_bright()` is consumed (slot-list star renders + metadata
// pills). Removed during the cleanup; `palette.star.base` still lives in
// the TOML schema so existing themes round-trip without a migration.
#[inline]
pub(crate) fn star_bright() -> Color {
    read_color(|t| t.star_bright)
}

// ============================================================================
// Chrome Border (1px hairline separators)
// ============================================================================

/// Hairline border color used by chrome dividers (between nav bars, list
/// rows, capsules). Per-theme in TOML, falls back to a darkened
/// `bg0_hard()` when unset. Replaces hard-coded `#1a2024`-style dividers.
#[inline]
pub(crate) fn border() -> Color {
    read_color(|t| t.border)
}

// ============================================================================
// Color Blending Helpers
// ============================================================================
// Used by `darken()` (the border-token fallback in theme_config and the
// `status_strip_bg` derivation), and by `slot_list`'s depth-darkening of
// now-playing rows. The flat redesign removed the 3D bevel chrome, taking
// the old `border_3d_*` / `lighten` helpers with it.

/// Blend a color toward a target color by the given factor (0.0 = base, 1.0 = target).
#[inline]
fn blend_toward(base: Color, target: Color, factor: f32) -> Color {
    Color {
        r: base.r + (target.r - base.r) * factor,
        g: base.g + (target.g - base.g) * factor,
        b: base.b + (target.b - base.b) * factor,
        a: base.a, // Keep original alpha
    }
}

/// Darken a color by blending it toward black.
#[inline]
pub(crate) fn darken(color: Color, amount: f32) -> Color {
    blend_toward(color, Color::BLACK, amount)
}

// ============================================================================
// Accent-wash family (single source of truth)
// ============================================================================
//
// The playlist / queue "Playing From" header banner and the `HoverOverlay`
// hover/press feedback are the same recipe — a faint pull of the live theme
// `accent()` over a surface. Both route through `accent_wash` / `hover_tint`
// so a 22nd theme or a new wash site inherits the look for free and cannot
// silently re-fork the recipe (the duplication this consolidated removed).

/// Faint accent-wash factor for the playlist / queue "Playing From" header
/// banner — `bg0_soft()` lerped this far toward `accent()`.
pub(crate) const HEADER_WASH: f32 = 0.07;

/// Blend `base` toward the active theme `accent()` by `factor` (0.0 = base,
/// 1.0 = pure accent). Preserves `base`'s alpha (opaque base → opaque wash).
/// The single home for the accent-wash family.
#[inline]
pub(crate) fn accent_wash(base: Color, factor: f32) -> Color {
    blend_toward(base, accent(), factor)
}

/// Opaque pigment the hover/press overlay deposits over a NEUTRAL surface.
///
/// `HoverOverlay` applies its own hover/press alpha on top, so the live
/// src-over composite equals `lerp(surface, this, alpha)` — the same
/// accent wash as the header, viewed at the overlay's alpha. Light mode
/// pulls toward `accent()`; dark toward the brighter `accent_bright()` so it
/// still reads over dark chrome. Fixes the pre-redesign light-mode no-op
/// (a near-black tint at 10% over a near-`bg0_hard()` surface was invisible).
#[inline]
pub(crate) fn hover_tint() -> Color {
    if is_light_mode() {
        accent()
    } else {
        accent_bright()
    }
}

/// Hover/press pigment for a surface that is ALREADY filled with
/// `accent_bright()` — active nav tabs and active player mode toggles.
///
/// Depositing the accent wash there is a near-no-op (accent over accent), so
/// these surfaces get a CONTRASTING neutral pull instead: `bg0_hard()` in
/// light mode, `fg0()` in dark. Call sites opt in via
/// [`HoverOverlay::on_accent_surface`] with their own active flag.
#[inline]
pub(crate) fn hover_tint_on_accent() -> Color {
    if is_light_mode() { bg0_hard() } else { fg0() }
}

// ============================================================================
// Contrast helpers
// ============================================================================
//
// Several shipped light-mode palettes tune `accent.now_playing` / `selected`
// to muted, low-saturation hues that match the surrounding chrome aesthetic.
// When those colors are reused as *text* (metadata strip title/artist), the
// luminance ends up too close to `bg0_hard()` and the text becomes unreadable
// even though the same accent reads fine as a fill or border. The helpers
// below let a render path nudge such a color back into a legible band
// without disturbing dark-mode behavior or the original theme palette.

/// WCAG 2.1 relative luminance.
#[inline]
fn relative_luminance(c: Color) -> f32 {
    let channel = |v: f32| {
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
}

/// WCAG contrast ratio between two colors. Result is in `[1.0, 21.0]`.
#[inline]
fn contrast_ratio(a: Color, b: Color) -> f32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (light, dark) = if la >= lb { (la, lb) } else { (lb, la) };
    (light + 0.05) / (dark + 0.05)
}

/// Minimum contrast we aim for when used as small (10 px) UI text — WCAG AA
/// for normal text.
const LEGIBLE_TEXT_CONTRAST: f32 = 4.5;

/// Push `color` toward whichever pure extreme (black or white) maximizes its
/// WCAG contrast against `reference`, stopping as soon as the contrast clears
/// `floor` (or at a near-full blend, which is guaranteed to clear ~4.58:1).
///
/// Bidirectional and surface-aware: the target extreme is the one farthest in
/// luminance from `reference` (= [`legible_text_on`] of the reference), so a
/// too-light color over a light surface darkens, and a too-dark color over a
/// dark surface lightens. This single primitive backs legible strip / chrome
/// text in BOTH modes — it replaces the old light-only `darken_until_legible`,
/// which could not lift a too-dark color on a dark theme.
fn legible_against(color: Color, reference: Color, floor: f32) -> Color {
    if contrast_ratio(color, reference) >= floor {
        return color;
    }
    let target = legible_text_on(reference);
    let mut adjusted = color;
    let mut amount: f32 = 0.05;
    while amount < 0.99 {
        adjusted = blend_toward(color, target, amount);
        if contrast_ratio(adjusted, reference) >= floor {
            return adjusted;
        }
        amount += 0.05;
    }
    adjusted
}

/// Make `color` legible as small strip / chrome text over the surface it is
/// ACTUALLY painted on (`status_strip_bg()` for the status strip, `bg0_hard()`
/// for the nav bar / mini-player). Surface-aware and bidirectional — fixes
/// both the old dark-mode no-op and the wrong-surface measurement.
#[inline]
pub(crate) fn legible_strip_text(color: Color, surface: Color) -> Color {
    legible_against(color, surface, LEGIBLE_TEXT_CONTRAST)
}

// ============================================================================
// Highlight-fill family (single source of truth)
// ============================================================================
//
// Selected/center and now-playing/expanded slots render as OPAQUE accent fills
// with the row text forced to a guaranteed-legible color. Unlike the
// translucent hover wash (which leaves the row's own text untouched), the
// readability of a fill is governed by its FORCED TEXT, so this family pairs a
// derived fill with `legible_text_on`. Both fills are derived from the live
// accent tokens — the per-theme `now_playing` / `selected` TOML values are no
// longer consumed — so a new theme inherits readable, mutually-distinct
// highlights for free instead of hand-tuning two colors that could (and did,
// e.g. Everforest light / Kanagawa Dragon dark) land unreadable.

/// Minimum WCAG contrast kept between the now-playing fill and the selected
/// fill, so the playing row stays distinguishable from the keyboard-cursor row
/// when both are visible at once.
const FILL_DISTINCT_CONTRAST: f32 = 1.5;

/// Pick pure black or white as the forced text/ink for an opaque highlight
/// `fill`, choosing whichever yields more WCAG contrast. Provably ≥ 4.58:1
/// against ANY fill (the black/white contrast curves cross at luminance
/// ≈ 0.179, where both equal 4.58), so the forced text always clears AA — even
/// for a future low-contrast accent.
#[inline]
pub(crate) fn legible_text_on(fill: Color) -> Color {
    if contrast_ratio(fill, Color::BLACK) >= contrast_ratio(fill, Color::WHITE) {
        Color::BLACK
    } else {
        Color::WHITE
    }
}

/// A visible ring for an opaque highlight chip: blends `fill` toward its own
/// forced text color (the guaranteed-contrasting extreme), so the border is
/// always perceptible against the fill regardless of theme. `strength` 1.0 =
/// max-contrast ring (center / playing); ~0.55 = subtler ring (plain selected).
#[inline]
pub(crate) fn highlight_border(fill: Color, strength: f32) -> Color {
    blend_toward(fill, legible_text_on(fill), strength)
}

/// Resolve the `(now_playing, selected)` fill pair from the accent tokens,
/// applying the distinctness separator once so the pair is always mutually
/// consistent. `selected` anchors on the louder `accent_bright`; `playing` is
/// the calmer `accent`, receded toward `bg` if needed to clear
/// [`FILL_DISTINCT_CONTRAST`] WITHOUT crossing selected's luminance (crossing
/// would invert the "cursor is loud, playing is ambient" hierarchy). If
/// receding stalls (accent and accent_bright sit at near-equal luminance),
/// `selected` is pushed toward the extreme farthest from `playing`
/// ([`legible_text_on`] of playing) instead — which always reaches the floor.
fn resolve_highlight_fills(accent: Color, accent_bright: Color, bg: Color) -> (Color, Color) {
    let sel = accent_bright;
    let mut play = accent;
    if contrast_ratio(play, sel) >= FILL_DISTINCT_CONTRAST {
        return (play, sel);
    }
    let l_sel = relative_luminance(sel);
    let started_below = relative_luminance(play) < l_sel;
    // Step A: recede playing toward the chrome bg, never crossing selected.
    let mut amount: f32 = 0.05;
    let mut best = play;
    while amount < 0.90 {
        let candidate = blend_toward(play, bg, amount);
        let l_c = relative_luminance(candidate);
        let crossed = if started_below {
            l_c >= l_sel
        } else {
            l_c <= l_sel
        };
        if crossed {
            break;
        }
        best = candidate;
        if contrast_ratio(best, sel) >= FILL_DISTINCT_CONTRAST {
            return (best, sel);
        }
        amount += 0.05;
    }
    play = best;
    // Step B: receding stalled — push selected farther from playing in the
    // direction that PRESERVES the hierarchy (lighter stays lighter, darker
    // stays darker), so the separator never inverts "cursor loud / playing
    // ambient". Saturated accent pairs have ample headroom toward their nearer
    // extreme; the distinctness guard test catches any palette that doesn't.
    let target = if relative_luminance(sel) >= relative_luminance(play) {
        Color::WHITE
    } else {
        Color::BLACK
    };
    let mut s = sel;
    let mut sa: f32 = 0.05;
    while sa < 0.99 {
        s = blend_toward(sel, target, sa);
        if contrast_ratio(s, play) >= FILL_DISTINCT_CONTRAST {
            break;
        }
        sa += 0.05;
    }
    (play, s)
}

/// Now-playing / expanded-parent slot fill — derived, distinctness-resolved.
#[inline]
pub(crate) fn playing_fill() -> Color {
    resolve_highlight_fills(accent(), accent_bright(), bg0_hard()).0
}

/// Selected / center slot fill — derived, distinctness-resolved.
#[inline]
pub(crate) fn selected_fill_resolved() -> Color {
    resolve_highlight_fills(accent(), accent_bright(), bg0_hard()).1
}

// ============================================================================
// Container Style Helpers
// ============================================================================
// These functions can be used directly with `.style(theme::container_bg0_hard)`
// instead of writing inline closures like `.style(|_theme| container::Style { ... })`

use iced::{
    Theme,
    widget::{container, text_input},
};

/// Container with BG0_HARD background (darkest)
pub(crate) fn container_bg0_hard(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(bg0_hard().into()),
        ..Default::default()
    }
}

/// Themed tooltip container style — `bg0_hard` fill, `theme::border()`
/// hairline, and the design's smallest corner radius. Migrated onto the
/// shared chrome tokens so tooltip corners pick up the active theme's
/// per-palette border color and the global flat-vs-rounded toggle.
pub(crate) fn container_tooltip(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(bg0_hard().into()),
        border: iced::Border {
            color: border(),
            width: 1.0,
            radius: ui_radius_xs(),
        },
        text_color: Some(fg1()),
        ..Default::default()
    }
}

/// Full-width horizontal separator line.
///
/// Renders as a `border()`-colored container with the given pixel height.
/// Replaces the inline `container(space()).width(Fill).height(Fixed(h)).style(bg1)`
/// pattern that was duplicated across `player_bar.rs`, `track_info_strip.rs`,
/// and `app_view.rs`. The redesign aligned every 1 px chrome rule onto the
/// shared `theme::border()` token, so this helper now reads the same
/// hairline color as the modal/menu/nav-bar separator family.
pub(crate) fn horizontal_separator<'a, M: 'a>(height: f32) -> iced::Element<'a, M> {
    use iced::{
        Length,
        widget::{container, space},
    };
    container(space())
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .style(move |_| container::Style {
            background: Some(border().into()),
            ..Default::default()
        })
        .into()
}

/// Fixed-height vertical separator line (1px wide, `border()` colored).
///
/// Used inside info strip rows to delineate fields. Shares the same
/// `theme::border()` hairline color as `horizontal_separator` and the
/// rest of the chrome separator family.
pub(crate) fn vertical_separator<'a, M: 'a>(height: f32) -> iced::Element<'a, M> {
    use iced::{
        Length,
        widget::{container, space},
    };
    container(space())
        .width(Length::Fixed(1.0))
        .height(Length::Fixed(height))
        .style(move |_| container::Style {
            background: Some(border().into()),
            ..Default::default()
        })
        .into()
}

// ----------------------------------------------------------------------------
// Modal separators
// ----------------------------------------------------------------------------
// Both helpers consolidate the eight near-identical separator lambdas that
// previously lived in `about_modal`, `info_modal`, `eq_modal`, `nav_bar`
// (twice), and `side_nav_bar`. After the flat redesign they share the same
// `border()` token — the design CSS uses the same `#1a2024` for modal-head,
// modal-actions, row separators, popover head, and pop-row borders.

/// 1-px horizontal separator between rows inside a modal.
///
/// Replaces the inline `row_separator` lambdas in `about_modal::info_row`
/// and `info_modal`'s property table.
pub(crate) fn modal_row_separator<'a, M: 'a>() -> iced::Element<'a, M> {
    use iced::{
        Length,
        widget::{container, space},
    };
    container(space::horizontal())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|_| container::Style {
            background: Some(border().into()),
            ..Default::default()
        })
        .into()
}

/// 1-px horizontal separator under a modal's header.
///
/// Replaces the inline `separator_line` lambdas in `about_modal`, `info_modal`,
/// and `eq_modal`.
pub(crate) fn modal_header_separator<'a, M: 'a>() -> iced::Element<'a, M> {
    use iced::{
        Length,
        widget::{container, space},
    };
    container(space::horizontal())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|_| container::Style {
            background: Some(border().into()),
            ..Default::default()
        })
        .into()
}

// `NavSeparatorAxis` + `nav_separator` were the canonical "2-px nav-bar
// separator" recipe — both axes, optional `force_visible` to defeat the
// rounded-mode hide. L2 (nav-chrome) replaced them with a 1-px
// `theme::border()`-colored rule local to each nav bar, so the helpers had
// no callers in the redesign. Removed during the cleanup; recover from
// `git show` if a future surface wants the old thick visual.

// ----------------------------------------------------------------------------
// Modal scaffolding
// ----------------------------------------------------------------------------

/// Wrap a modal dialog box in the canonical backdrop + opaque scaffold.
///
/// Produces the `mouse_area(opaque(container(...).style(backdrop)))` Element
/// that all four overlay modals (`about`, `info`, `eq`, `text_input_dialog`)
/// previously open-coded. The backdrop is a semi-transparent `bg0_hard` wash
/// (alpha = `backdrop_alpha`, conventionally `0.6`); clicking it emits
/// `on_backdrop_press` (Close / Cancel depending on caller); `opaque()`
/// blocks pointer events from reaching widgets behind the modal.
///
/// Restraint: only the backdrop layer is consolidated here. The dialog box
/// itself (border color, max_height, fixed width, etc.) stays at each call
/// site because those genuinely diverge between modals.
pub(crate) fn modal_scaffold<'a, M: Clone + 'a>(
    dialog_box: iced::Element<'a, M>,
    on_backdrop_press: M,
    backdrop_alpha: f32,
) -> iced::Element<'a, M> {
    use iced::{
        Alignment, Length,
        widget::{container, mouse_area, opaque},
    };
    let backdrop = mouse_area(
        container(opaque(dialog_box))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(move |_| {
                let mut bg = bg0_hard();
                bg.a = backdrop_alpha;
                container::Style {
                    background: Some(bg.into()),
                    ..Default::default()
                }
            }),
    )
    .on_press(on_backdrop_press);
    opaque(backdrop)
}

/// Conventional backdrop alpha used by every overlay modal.
pub(crate) const MODAL_BACKDROP_ALPHA: f32 = 0.6;

/// Shared `container::Style` for overlay modal panels — flat `bg0_hard()`
/// fill, 1 px `accent_bright()` outline, `ui_radius_lg()` corners.
///
/// Five overlay modals (`about`, `info`, `eq`, `text_input_dialog`,
/// `default_playlist_picker`) open-coded this exact block. Routing them
/// through one function means a future per-theme tweak to the modal frame
/// (e.g. swapping the outline onto `border()` for the chrome-quiet variant)
/// only touches this body — and the radius / fill / border are all
/// guaranteed to stay in lockstep across the modal family.
pub(crate) fn modal_frame_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(bg0_hard().into()),
        border: iced::Border {
            color: accent_bright(),
            width: 1.0,
            radius: ui_radius_lg(),
        },
        ..Default::default()
    }
}

// ----------------------------------------------------------------------------
// Transparent button style
// ----------------------------------------------------------------------------

/// Borderless button style: no background when idle, `bg1` on hover, no
/// outline, `ui_border_radius()` corners. Hoisted from
/// `default_playlist_picker::transparent_button_style` so future callers can
/// find it without re-inventing.
pub(crate) fn transparent_button_style(
    _theme: &Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    use iced::widget::button;
    button::Style {
        background: match status {
            button::Status::Hovered => Some(bg1().into()),
            _ => None,
        },
        text_color: fg0(),
        border: iced::Border {
            radius: ui_border_radius(),
            ..Default::default()
        },
        ..Default::default()
    }
}

// `theme::search_input_style` was the legacy 2 px-bordered Gruvbox view-header
// style. The L3 flat redesign moved view-header callers to
// `search_bar::flat_search_input_style` and the L5 settings UI runs through
// `settings_search_input_style` below; the original helper had no remaining
// callers and was removed during the cleanup. Recover from `git show` if a
// future caller wants the old visual.

/// Specialized search style for settings panels so it doesn't blend into bg0_soft.
pub(crate) fn settings_search_input_style(
    _theme: &Theme,
    status: text_input::Status,
) -> text_input::Style {
    text_input::Style {
        background: (bg0_hard()).into(),
        border: iced::Border {
            color: if matches!(status, text_input::Status::Focused { .. }) {
                accent_bright()
            } else {
                bg2()
            },
            width: 1.0,
            radius: ui_border_radius(),
        },
        icon: fg4(),
        placeholder: fg4(),
        value: fg0(),
        selection: selection_color(),
    }
}

/// Themed scrollbar style for the settings detail pane: `bg2` rail, `fg4`
/// scroller resting, `accent_bright` scroller on hover/drag. Matches the
/// info-modal scrollable's chrome so all in-settings scrollable surfaces
/// read consistently against the flat-redesign palette.
pub(crate) fn settings_scrollable_style(
    _theme: &Theme,
    status: iced::widget::scrollable::Status,
) -> iced::widget::scrollable::Style {
    use iced::widget::{container, scrollable};

    let rail = scrollable::Rail {
        background: Some(bg2().into()),
        border: iced::Border {
            radius: ui_border_radius(),
            ..Default::default()
        },
        scroller: scrollable::Scroller {
            background: fg4().into(),
            border: iced::Border {
                radius: ui_border_radius(),
                ..Default::default()
            },
        },
    };
    let hot_rail = scrollable::Rail {
        scroller: scrollable::Scroller {
            background: accent_bright().into(),
            ..rail.scroller
        },
        ..rail
    };
    let auto_scroll = scrollable::AutoScroll {
        background: iced::Color::TRANSPARENT.into(),
        border: iced::Border::default(),
        shadow: iced::Shadow::default(),
        icon: iced::Color::TRANSPARENT,
    };

    match status {
        scrollable::Status::Active { .. } => scrollable::Style {
            container: container::Style::default(),
            vertical_rail: rail,
            horizontal_rail: rail,
            gap: None,
            auto_scroll,
        },
        scrollable::Status::Hovered {
            is_vertical_scrollbar_hovered,
            is_horizontal_scrollbar_hovered,
            ..
        } => scrollable::Style {
            container: container::Style::default(),
            vertical_rail: if is_vertical_scrollbar_hovered {
                hot_rail
            } else {
                rail
            },
            horizontal_rail: if is_horizontal_scrollbar_hovered {
                hot_rail
            } else {
                rail
            },
            gap: None,
            auto_scroll,
        },
        scrollable::Status::Dragged {
            is_vertical_scrollbar_dragged,
            is_horizontal_scrollbar_dragged,
            ..
        } => scrollable::Style {
            container: container::Style::default(),
            vertical_rail: if is_vertical_scrollbar_dragged {
                hot_rail
            } else {
                rail
            },
            horizontal_rail: if is_horizontal_scrollbar_dragged {
                hot_rail
            } else {
                rail
            },
            gap: None,
            auto_scroll,
        },
    }
}

// ============================================================================
// Iced Theme Integration
// ============================================================================

/// Build a custom `iced::Theme` from the current live Gruvbox colors.
///
/// This maps the Gruvbox palette into an `iced::Palette` so that widgets
/// relying on the default Iced catalog styles (e.g. the scrollbar inside
/// `combo_box` menus) pick up Gruvbox colors instead of the built-in defaults.
///
/// Since all other widgets in the app use closure-based `.style()` that ignore
/// the `&Theme` parameter, this only affects widgets that fall through to the
/// Iced catalog default — notably the combo_box dropdown scrollbar.
pub(crate) fn iced_theme() -> Theme {
    use iced::theme::palette::Seed;

    let palette = Seed {
        background: bg0_hard(),
        text: fg0(),
        primary: accent_bright(),
        success: success(),
        warning: warning(),
        danger: danger(),
    };

    Theme::custom("Nokkvi".to_string(), palette)
}

// ============================================================================
// Toast Level Colors
// ============================================================================

/// Map toast notification level to a theme-appropriate text color.
/// Uses the `base` (non-bright) color variants because:
/// - In dark themes, `base` colors are still vivid and readable
/// - In light themes, `bright` colors wash out against light backgrounds
/// - Theme authors set `base` variants to be readable against their chosen bg colors
pub(crate) fn toast_level_color(level: nokkvi_data::types::toast::ToastLevel) -> Color {
    use nokkvi_data::types::toast::ToastLevel;
    match level {
        ToastLevel::Info => fg1(),
        ToastLevel::Success => success(),
        ToastLevel::Warning => warning(),
        ToastLevel::Error => danger(),
    }
}

/// Sequential guard shared across the workspace's tests that flip globals
/// like `set_light_mode` or mutate the `UI_MODE` atomics. `parking_lot`
/// avoids std-lock poisoning if one test panics. Exposed `pub(crate)`
/// (test-only) so chrome-math regression tests in `widgets::*` can pin
/// the same atomics under the same lock.
#[cfg(test)]
pub(crate) static THEME_MODE_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    /// Micro-bench: measures cumulative cost of `theme::fg0()` over 10,000
    /// calls. Numbers print to stderr (use `cargo test -- --nocapture` to view).
    ///
    /// Recorded baselines (release build, this machine):
    /// - Pre-2A `RwLock<ResolvedDualTheme>` + 352-byte struct clone: ~13.1 ns/call.
    /// - Post-2A `ArcSwap<ResolvedDualTheme>` + lock-free Guard load: ~12.5 ns/call.
    ///
    /// The raw per-call delta is small because both paths bottleneck on a few
    /// atomic ops; the durable win of 2A is **lock-freedom** — the visualizer
    /// FFT thread no longer competes with the render thread for a theme lock.
    /// The upper bound is generous (regression net, not wall-clock guarantee).
    #[test]
    fn theme_accessor_microbench_fg0_x10000() {
        // Touch the theme once so any first-call setup (DUAL_THEME LazyLock
        // init, builtin theme seeding) is excluded from the measurement.
        let _warm = fg0();

        let iters = 10_000;
        let start = Instant::now();
        let mut acc_r = 0.0f32;
        for _ in 0..iters {
            // Use the result so the optimizer can't dead-code the call.
            acc_r += fg0().r;
        }
        let elapsed = start.elapsed();

        eprintln!(
            "theme::fg0() x{iters} = {:?} ({:.1} ns/call), accumulator={acc_r}",
            elapsed,
            (elapsed.as_nanos() as f64) / (iters as f64)
        );

        assert!(
            elapsed.as_millis() < 1_000,
            "fg0() x{iters} unexpectedly slow: {elapsed:?}"
        );
    }

    // `THEME_MODE_LOCK` is now defined at module scope so other test modules
    // in the crate can share the same guard.

    // ------------------------------------------------------------------------
    // Contrast helpers — luminance/contrast math + the light-mode darkening
    // routine that keeps muted theme accents readable as strip text.
    // ------------------------------------------------------------------------

    #[test]
    fn relative_luminance_at_endpoints() {
        assert!((relative_luminance(Color::WHITE) - 1.0).abs() < 0.001);
        assert!(relative_luminance(Color::BLACK).abs() < 0.001);
    }

    #[test]
    fn contrast_ratio_extremes() {
        assert!((contrast_ratio(Color::WHITE, Color::BLACK) - 21.0).abs() < 0.1);
        assert!((contrast_ratio(Color::BLACK, Color::WHITE) - 21.0).abs() < 0.1);
        assert!((contrast_ratio(Color::WHITE, Color::WHITE) - 1.0).abs() < 0.001);
    }

    fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color::from_rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
    }

    /// Every shipped palette as `(name, mode, ResolvedTheme)` — for theme-wide
    /// contrast guards. Reads embedded built-in TOML; no disk, no global theme
    /// state, so these sweeps are deterministic and lock-free.
    fn all_builtin_palettes() -> Vec<(String, &'static str, ResolvedTheme)> {
        let mut out = Vec::new();
        for stem in nokkvi_data::services::theme_loader::builtin_theme_stems() {
            let tf = nokkvi_data::services::theme_loader::load_builtin_theme(stem)
                .unwrap_or_else(|| panic!("built-in theme {stem} must parse"));
            let dual = ResolvedDualTheme::from_theme_file(&tf);
            out.push((stem.to_string(), "dark", dual.dark));
            out.push((stem.to_string(), "light", dual.light));
        }
        out
    }

    /// `legible_text_on` is provably ≥ 4.58:1 against any fill (the black/white
    /// contrast curves cross at luminance ≈ 0.179). Spot-check the hardest fills
    /// near the crossover plus the real problem colors.
    #[test]
    fn legible_text_on_is_always_legible() {
        let samples = [
            Color::BLACK,
            Color::WHITE,
            Color::from_rgb(0.5, 0.5, 0.5),
            Color::from_rgb(0.45, 0.45, 0.45),
            Color::from_rgb(0.40, 0.40, 0.40),
            rgb(0x22, 0x32, 0x49), // Kanagawa Dragon primary (dark navy)
            rgb(0x93, 0xB2, 0x59), // Everforest light accent_bright
            rgb(0xA6, 0xB0, 0xA0), // Everforest light old `selected` grey
        ];
        for fill in samples {
            let cr = contrast_ratio(fill, legible_text_on(fill));
            assert!(
                cr >= LEGIBLE_TEXT_CONTRAST,
                "forced text on {fill:?} only reached {cr:.2}:1"
            );
        }
    }

    /// The forced row text reads against BOTH derived highlight fills on every
    /// shipped theme/mode — the systemic fix for the unreadable Everforest-light
    /// selection and Kanagawa-Dragon-dark now-playing rows.
    #[test]
    fn forced_text_reads_on_every_highlight_fill() {
        for (name, mode, t) in all_builtin_palettes() {
            let (play, sel) = resolve_highlight_fills(t.accent, t.accent_bright, t.bg0_hard);
            for (which, fill) in [("playing", play), ("selected", sel)] {
                let cr = contrast_ratio(fill, legible_text_on(fill));
                assert!(
                    cr >= LEGIBLE_TEXT_CONTRAST,
                    "{name}/{mode} {which} fill forced-text contrast {cr:.2}:1 below AA"
                );
            }
        }
    }

    /// Now-playing and selected fills stay perceptibly distinct on every shipped
    /// theme/mode, so a playing row and a cursor row are tellable apart at once.
    #[test]
    fn playing_and_selected_fills_stay_distinct() {
        for (name, mode, t) in all_builtin_palettes() {
            let (play, sel) = resolve_highlight_fills(t.accent, t.accent_bright, t.bg0_hard);
            let cr = contrast_ratio(play, sel);
            assert!(
                cr >= FILL_DISTINCT_CONTRAST,
                "{name}/{mode} playing-vs-selected contrast {cr:.2}:1 < {FILL_DISTINCT_CONTRAST}"
            );
        }
    }

    /// The distinctness separator never inverts the hierarchy: when playing
    /// starts darker than selected, the resolved playing fill stays no lighter
    /// than the resolved selected fill (cursor = loud/bright, playing = ambient).
    #[test]
    fn playing_fill_does_not_invert_hierarchy() {
        for (name, mode, t) in all_builtin_palettes() {
            if relative_luminance(t.accent) < relative_luminance(t.accent_bright) {
                let (play, sel) = resolve_highlight_fills(t.accent, t.accent_bright, t.bg0_hard);
                assert!(
                    relative_luminance(play) <= relative_luminance(sel) + 1e-4,
                    "{name}/{mode} playing fill became lighter than selected (hierarchy inverted)"
                );
            }
        }
    }

    /// The highlight border is perceptible against its fill at both strengths
    /// (regression for the old center-slot border that matched the fill 1:1 on
    /// the 17 themes that didn't customize `selected`).
    #[test]
    fn highlight_border_contrasts_its_fill() {
        for (name, mode, t) in all_builtin_palettes() {
            let (play, sel) = resolve_highlight_fills(t.accent, t.accent_bright, t.bg0_hard);
            for (which, fill) in [("playing", play), ("selected", sel)] {
                assert!(
                    contrast_ratio(highlight_border(fill, 1.0), fill) > 1.3,
                    "{name}/{mode} {which} max-strength border indistinct from fill"
                );
                assert!(
                    contrast_ratio(highlight_border(fill, 0.55), fill) > 1.05,
                    "{name}/{mode} {which} subtle border indistinct from fill"
                );
            }
        }
    }

    /// `legible_against` is bidirectional: too-dark text on a dark surface is
    /// LIFTED (the old `darken_until_legible` could not), and too-light text on
    /// a light surface is darkened — both reaching WCAG AA.
    #[test]
    fn legible_against_is_bidirectional() {
        // Kanagawa Dragon dark: navy "Hemp Dub" text over the near-black strip.
        let navy = rgb(0x22, 0x32, 0x49);
        let dark_surface = rgb(0x0f, 0x0e, 0x0e);
        let lifted = legible_against(navy, dark_surface, LEGIBLE_TEXT_CONTRAST);
        assert!(
            contrast_ratio(lifted, dark_surface) >= LEGIBLE_TEXT_CONTRAST,
            "dark text on dark surface should reach AA"
        );
        assert!(
            relative_luminance(lifted) > relative_luminance(navy),
            "dark-on-dark fix must LIGHTEN (the old light-only path could not)"
        );

        // Everforest light: muted green text over cream chrome.
        let green = rgb(0x93, 0xB2, 0x59);
        let light_surface = rgb(0xEF, 0xEB, 0xD4);
        let darkened = legible_against(green, light_surface, LEGIBLE_TEXT_CONTRAST);
        assert!(
            contrast_ratio(darkened, light_surface) >= LEGIBLE_TEXT_CONTRAST,
            "light-ish text on a light surface should reach AA"
        );
        assert!(
            relative_luminance(darkened) < relative_luminance(green),
            "a fix on a light surface must DARKEN"
        );
    }

    /// `legible_against` is a no-op when the input already clears the floor.
    #[test]
    fn legible_against_returns_input_when_already_legible() {
        let r = legible_against(Color::BLACK, Color::WHITE, LEGIBLE_TEXT_CONTRAST);
        assert_eq!((r.r, r.g, r.b), (0.0, 0.0, 0.0));
    }

    /// The light-mode status strip is a perceptible band on every light palette
    /// (the old fixed darken-toward-black muddied warm cream into dingy grey).
    #[test]
    fn light_status_strip_band_separates_from_chrome() {
        for (name, mode, t) in all_builtin_palettes() {
            if mode != "light" {
                continue;
            }
            let band = strip_band_toward_ink(t.bg0_hard, t.fg0, STRIP_BAND_DELTA);
            let delta = (relative_luminance(band) - relative_luminance(t.bg0_hard)).abs();
            assert!(
                delta >= STRIP_BAND_DELTA - 1e-3,
                "{name}/{mode} status strip band only Δ{delta:.4} from chrome"
            );
        }
    }

    /// Strip text tiers (`fg2`/`fg3`) made legible over the painted strip
    /// surface clear WCAG AA on every theme/mode — including Kanagawa Dragon
    /// dark, where the old now_playing/selected-as-text path was unreadable.
    #[test]
    fn strip_text_tiers_read_on_their_surface() {
        for (name, mode, t) in all_builtin_palettes() {
            let surface = if mode == "light" {
                strip_band_toward_ink(t.bg0_hard, t.fg0, STRIP_BAND_DELTA)
            } else {
                darken(t.bg0_hard, 0.17)
            };
            for (tier, color) in [("fg2", t.fg2), ("fg3", t.fg3)] {
                let txt = legible_against(color, surface, LEGIBLE_TEXT_CONTRAST);
                let cr = contrast_ratio(txt, surface);
                assert!(
                    cr >= LEGIBLE_TEXT_CONTRAST,
                    "{name}/{mode} strip {tier} text only {cr:.2}:1 over its surface"
                );
            }
        }
    }

    /// `hover_tint()` must read against the neutral chrome surface it sits over
    /// (`bg0_hard()`) in both modes — the fix for the pre-redesign light-mode
    /// no-op where a near-black tint at 10% over a near-`bg0_hard()` surface
    /// was effectively invisible.
    #[test]
    fn hover_tint_reads_over_neutral_chrome() {
        let _guard = THEME_MODE_LOCK.lock();
        let saved = UI_MODE.light_mode.load(Ordering::Relaxed);

        for light in [true, false] {
            set_light_mode(light);
            let delta = (relative_luminance(hover_tint()) - relative_luminance(bg0_hard())).abs();
            assert!(
                delta > 0.02,
                "hover_tint() must differ perceptibly from neutral chrome (light={light}); \
                 luminance delta={delta:.4}"
            );
        }

        UI_MODE.light_mode.store(saved, Ordering::Relaxed);
    }

    /// Regression guard for the active-tab no-op. An accent-derived hover over
    /// a surface already filled with `accent_bright()` (active nav tab / mode
    /// toggle) is a near-no-op — in dark mode `accent_bright()` over
    /// `accent_bright()` is exactly zero. `hover_tint_on_accent()` must instead
    /// contrast against the accent fill so hovering an active tab still reads.
    #[test]
    fn hover_tint_on_accent_contrasts_with_accent_fill() {
        let _guard = THEME_MODE_LOCK.lock();
        let saved = UI_MODE.light_mode.load(Ordering::Relaxed);

        for light in [true, false] {
            set_light_mode(light);
            let delta = (relative_luminance(hover_tint_on_accent())
                - relative_luminance(accent_bright()))
            .abs();
            assert!(
                delta > 0.02,
                "hover_tint_on_accent() must contrast with the accent_bright() fill \
                 (light={light}); luminance delta={delta:.4}"
            );
        }

        UI_MODE.light_mode.store(saved, Ordering::Relaxed);
    }

    // ------------------------------------------------------------------------
    // Radius helpers — every `ui_radius_*` / `ui_border_radius*` helper now
    // delegates to `gated_radius`. Sweep all three RoundedMode states and pin
    // each helper's gate predicate + value, so a wrong gate or a forked
    // non-zero fallback in a future hand-added `_player` variant breaks here.
    // ------------------------------------------------------------------------

    #[test]
    fn radius_helpers_gate_and_value_across_modes() {
        let _guard = THEME_MODE_LOCK.lock();
        let saved = UI_MODE.rounded_mode.load(Ordering::Relaxed);

        // Flatten a Radius to its four corners; the `f32 -> Radius` `From` sets
        // all four equal, so one expected value covers them all.
        let corners = |radius: iced::border::Radius| {
            [
                radius.top_left,
                radius.top_right,
                radius.bottom_right,
                radius.bottom_left,
            ]
        };
        let all = |v: f32| [v, v, v, v];

        // Off: every helper is square (0.0), player variants included.
        set_rounded_mode(RoundedMode::Off);
        for got in [
            ui_border_radius(),
            ui_border_radius_player(),
            ui_radius_xs(),
            ui_radius_sm(),
            ui_radius_md(),
            ui_radius_lg(),
            ui_radius_pill(),
            ui_radius_sm_player(),
            ui_radius_pill_player(),
        ] {
            assert_eq!(corners(got), all(0.0), "Off mode must be square");
        }

        // On: every helper returns its scale value.
        set_rounded_mode(RoundedMode::On);
        assert_eq!(corners(ui_border_radius()), all(ROUNDED_RADIUS));
        assert_eq!(corners(ui_border_radius_player()), all(ROUNDED_RADIUS));
        assert_eq!(corners(ui_radius_xs()), all(R_XS));
        assert_eq!(corners(ui_radius_sm()), all(R_SM));
        assert_eq!(corners(ui_radius_md()), all(R_MD));
        assert_eq!(corners(ui_radius_lg()), all(R_LG));
        assert_eq!(corners(ui_radius_pill()), all(R_PILL));
        assert_eq!(corners(ui_radius_sm_player()), all(R_SM));
        assert_eq!(corners(ui_radius_pill_player()), all(R_PILL));

        // PlayerOnly: only the `_player` (is_rounded_for_player) helpers round;
        // the global `is_rounded_mode` helpers stay square.
        set_rounded_mode(RoundedMode::PlayerOnly);
        assert_eq!(corners(ui_border_radius()), all(0.0));
        assert_eq!(corners(ui_radius_xs()), all(0.0));
        assert_eq!(corners(ui_radius_sm()), all(0.0));
        assert_eq!(corners(ui_radius_md()), all(0.0));
        assert_eq!(corners(ui_radius_lg()), all(0.0));
        assert_eq!(corners(ui_radius_pill()), all(0.0));
        assert_eq!(corners(ui_border_radius_player()), all(ROUNDED_RADIUS));
        assert_eq!(corners(ui_radius_sm_player()), all(R_SM));
        assert_eq!(corners(ui_radius_pill_player()), all(R_PILL));

        UI_MODE.rounded_mode.store(saved, Ordering::Relaxed);
    }

    // ------------------------------------------------------------------------
    // atomic_u8_enum! macro — verifies that the loader/store impls emitted
    // for every `UiModeFlags` enum round-trip each variant through its
    // declaration discriminant, and that unknown bytes fall back to the
    // declared default variant. The bytes are a transient in-process cache
    // encoding (nothing persists them — persistence is serde wire strings),
    // so the fallback is purely defensive.
    // ------------------------------------------------------------------------

    /// Roundtrip every variant of two enums (one with a small variant set, one
    /// with a larger one) through `to_u8` / `from_u8`. Exercising the macro
    /// expansion twice guarantees we're testing the macro itself, not just one
    /// hand-written impl.
    #[test]
    fn atomic_u8_enum_macro_emits_roundtrip() {
        // NavLayout: 3 variants, declaration discriminants {0,1,2}.
        for (byte, variant) in [
            (0u8, NavLayout::Top),
            (1u8, NavLayout::Side),
            (2u8, NavLayout::None),
        ] {
            assert_eq!(
                NavLayout::from_u8(byte),
                variant,
                "NavLayout::from_u8({byte})"
            );
            assert_eq!(variant.to_u8(), byte, "NavLayout::{variant:?}.to_u8()");
        }

        // StripSeparator: 6 variants, declaration discriminants {0..=5}.
        // Exercises a larger variant list so we catch any macro misexpansion
        // that only manifests with more arms.
        for (byte, variant) in [
            (0u8, StripSeparator::Dot),
            (1u8, StripSeparator::Bullet),
            (2u8, StripSeparator::Pipe),
            (3u8, StripSeparator::EmDash),
            (4u8, StripSeparator::Slash),
            (5u8, StripSeparator::Bar),
        ] {
            assert_eq!(
                StripSeparator::from_u8(byte),
                variant,
                "StripSeparator::from_u8({byte})"
            );
            assert_eq!(variant.to_u8(), byte, "StripSeparator::{variant:?}.to_u8()");
        }
    }

    /// An unknown stored byte MUST decode to the declared default variant.
    /// This is purely defensive: the bytes live only inside the in-process
    /// `UI_MODE` atomics (nothing persists them), so an unknown byte can only
    /// come from a corrupted atomic — the fallback keeps the render thread
    /// from ever panicking on one.
    #[test]
    fn atomic_u8_enum_unknown_byte_falls_back_to_default() {
        // TrackInfoDisplay default = Off.
        assert_eq!(TrackInfoDisplay::from_u8(255), TrackInfoDisplay::Off);
        assert_eq!(TrackInfoDisplay::from_u8(99), TrackInfoDisplay::Off);
        // Also verify a byte just past the highest known variant (4) falls back.
        assert_eq!(TrackInfoDisplay::from_u8(5), TrackInfoDisplay::Off);
        // StripSeparator default is Slash (byte 4); unknown bytes fall back to it.
        assert_eq!(StripSeparator::from_u8(255), StripSeparator::Slash);
        assert_eq!(StripSeparator::from_u8(6), StripSeparator::Slash);
    }

    /// `ArtworkColumnMode` is the largest enum behind a `UI_MODE` atomic —
    /// round-trip every variant through `to_u8` / `from_u8` and pin that each
    /// byte equals the variant's declaration discriminant. The bytes are an
    /// in-memory cache encoding only (persistence is serde wire strings), so
    /// the discriminants are free to follow declaration order; this test
    /// catches a macro misexpansion that maps a variant to the wrong byte.
    #[test]
    fn artwork_column_mode_encoding_roundtrips_every_variant() {
        // {declaration discriminant → variant}, in declaration order.
        let table = [
            (0u8, ArtworkColumnMode::Auto),
            (1u8, ArtworkColumnMode::AlwaysNative),
            (2u8, ArtworkColumnMode::AlwaysStretched),
            (3u8, ArtworkColumnMode::AlwaysVerticalNative),
            (4u8, ArtworkColumnMode::AlwaysVerticalStretched),
            (5u8, ArtworkColumnMode::Never),
        ];

        for (byte, variant) in table {
            assert_eq!(
                ArtworkColumnMode::from_u8(variant.to_u8()),
                variant,
                "ArtworkColumnMode::{variant:?} must survive a to_u8/from_u8 roundtrip"
            );
            assert_eq!(
                variant.to_u8(),
                byte,
                "ArtworkColumnMode::{variant:?} must encode to its declaration discriminant {byte}"
            );
            assert_eq!(
                ArtworkColumnMode::from_u8(byte),
                variant,
                "ArtworkColumnMode byte {byte} must decode to {variant:?}"
            );
        }
    }

    /// End-to-end test through the actual `Theme` get/set helpers (not just
    /// the macro impls in isolation): write a known variant via `set_*`,
    /// then read it back via the matching getter and confirm the variant
    /// survives a full store-then-load cycle through the live `AtomicU8`.
    /// Exercises every migrated site at least once.
    #[test]
    fn store_then_load_preserves_variant_per_enum() {
        let _guard = THEME_MODE_LOCK.lock();

        // Snapshot every UI_MODE u8 we're about to mutate so neighboring
        // tests don't observe leaked state.
        let saved_tid = UI_MODE.track_info_display.load(Ordering::Relaxed);
        let saved_nav = UI_MODE.nav_layout.load(Ordering::Relaxed);
        let saved_ndm = UI_MODE.nav_display_mode.load(Ordering::Relaxed);
        let saved_srh = UI_MODE.slot_row_height.load(Ordering::Relaxed);
        let saved_sca = UI_MODE.strip_click_action.load(Ordering::Relaxed);
        let saved_sep = UI_MODE.strip_separator.load(Ordering::Relaxed);
        let saved_acm = UI_MODE.artwork_column_mode.load(Ordering::Relaxed);
        let saved_asf = UI_MODE.artwork_column_stretch_fit.load(Ordering::Relaxed);

        set_track_info_display(TrackInfoDisplay::TopBar);
        assert_eq!(track_info_display(), TrackInfoDisplay::TopBar);

        set_nav_layout(NavLayout::Side);
        assert!(is_side_nav());
        assert!(!is_top_nav());

        set_nav_display_mode(NavDisplayMode::IconsOnly);
        assert_eq!(nav_display_mode(), NavDisplayMode::IconsOnly);

        set_slot_row_height(SlotRowHeight::Spacious);
        assert_eq!(slot_row_height_variant(), SlotRowHeight::Spacious);

        set_strip_click_action(StripClickAction::CopyTrackInfo);
        assert_eq!(strip_click_action(), StripClickAction::CopyTrackInfo);

        set_strip_separator(StripSeparator::EmDash);
        assert_eq!(strip_separator(), StripSeparator::EmDash);

        // Hit the non-contiguous slot specifically.
        set_artwork_column_mode(ArtworkColumnMode::AlwaysVerticalStretched);
        assert_eq!(
            artwork_column_mode(),
            ArtworkColumnMode::AlwaysVerticalStretched
        );

        set_artwork_column_stretch_fit(ArtworkStretchFit::Fill);
        assert_eq!(artwork_column_stretch_fit(), ArtworkStretchFit::Fill);

        // Restore every mutated atomic so the next test sees the baseline state.
        UI_MODE
            .track_info_display
            .store(saved_tid, Ordering::Relaxed);
        UI_MODE.nav_layout.store(saved_nav, Ordering::Relaxed);
        UI_MODE.nav_display_mode.store(saved_ndm, Ordering::Relaxed);
        UI_MODE.slot_row_height.store(saved_srh, Ordering::Relaxed);
        UI_MODE
            .strip_click_action
            .store(saved_sca, Ordering::Relaxed);
        UI_MODE.strip_separator.store(saved_sep, Ordering::Relaxed);
        UI_MODE
            .artwork_column_mode
            .store(saved_acm, Ordering::Relaxed);
        UI_MODE
            .artwork_column_stretch_fit
            .store(saved_asf, Ordering::Relaxed);
    }

    // ------------------------------------------------------------------------
    // Modal / nav separator helpers — pin that the consolidated helpers
    // still compile, produce real Elements, and select the right axis
    // dimensions for `nav_separator`. The row-vs-header alpha pair is
    // documented in the helper bodies; the consolidation kept those values
    // intact, so the regression risk we guard against here is "future agent
    // accidentally swaps axes or returns the wrong type."
    // ------------------------------------------------------------------------

    /// Both modal separator helpers must produce real `Element`s — a
    /// characterization that the consolidation kept the lambdas building
    /// elements that wire into a `Column`. Regression risk we guard: a
    /// future refactor accidentally changes the return type to e.g. `Rule`
    /// (which has different default styling).
    #[test]
    fn modal_separators_produce_elements() {
        let _row: iced::Element<'_, ()> = modal_row_separator();
        let _header: iced::Element<'_, ()> = modal_header_separator();
    }

    /// `modal_scaffold` must accept any `M: Clone` and return an Element of
    /// the same message type. Pin the type-level contract so a future
    /// refactor of the scaffold helper can't accidentally drop the
    /// generic-message parameter (the audit explicitly calls out that
    /// each modal uses a different `Message::Close` / `Message::Cancel`).
    #[test]
    fn modal_scaffold_threads_message_type_through() {
        use iced::widget::Space;

        #[derive(Debug, Clone, PartialEq)]
        enum FakeMsg {
            Closed,
        }
        let dialog: iced::Element<'_, FakeMsg> =
            iced::Element::from(Space::new().width(100.0).height(60.0));
        let _scaffold: iced::Element<'_, FakeMsg> =
            modal_scaffold(dialog, FakeMsg::Closed, MODAL_BACKDROP_ALPHA);
    }
}
