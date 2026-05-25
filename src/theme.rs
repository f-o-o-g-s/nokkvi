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

// ============================================================================
// UI Mode Flags (grouped to avoid scattered statics)
// ============================================================================

/// All runtime-togglable UI mode flags, consolidated into one struct.
/// Each flag uses interior atomics for thread-safe lock-free access.
struct UiModeFlags {
    /// Light/dark theme toggle
    light_mode: AtomicBool,
    /// Rounded corner borders
    rounded_mode: AtomicBool,
    /// Track info display mode: 0 = Off, 1 = PlayerBar, 2 = TopBar
    track_info_display: AtomicU8,
    /// Navigation layout: 0 = Top, 1 = Side, 2 = None
    nav_layout: AtomicU8,
    /// Navigation display: 0 = TextOnly, 1 = TextAndIcons, 2 = IconsOnly
    nav_display_mode: AtomicU8,
    /// Target row height for slot lists (discriminant: 0=Compact 1=Default 2=Comfortable 3=Spacious)
    slot_row_height: AtomicU8,
    /// Whether the opacity gradient on non-center slots is enabled
    opacity_gradient: AtomicBool,
    /// Whether clickable text links in slot list items are enabled
    slot_text_links: AtomicBool,
    /// Whether volume sliders are displayed horizontally in the player bar
    horizontal_volume: AtomicBool,
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
    /// Strip click action: 0=GoToQueue, 1=GoToAlbum, 2=GoToArtist, 3=CopyTrackInfo, 4=DoNothing
    strip_click_action: AtomicU8,
    /// Whether `title:` / `artist:` / `album:` labels are prepended to fields
    strip_show_labels: AtomicBool,
    /// Strip merged-mode separator: 0=Dot, 1=Bullet, 2=Pipe, 3=EmDash, 4=Slash, 5=Bar
    strip_separator: AtomicU8,
    /// Whether the metadata text overlay is rendered on the large artwork in Albums view
    albums_artwork_overlay: AtomicBool,
    /// Whether the metadata text overlay is rendered on the large artwork in Artists view
    artists_artwork_overlay: AtomicBool,
    /// Whether the metadata text overlay is rendered on the large artwork in Songs view
    songs_artwork_overlay: AtomicBool,
    /// Whether the metadata text overlay is rendered on the large artwork in Playlists view
    playlists_artwork_overlay: AtomicBool,
    /// Artwork column display mode: 0=Auto, 1=AlwaysNative, 2=AlwaysStretched,
    /// 3=Never, 4=AlwaysVerticalNative, 5=AlwaysVerticalStretched.
    artwork_column_mode: AtomicU8,
    /// Artwork stretch fit when column mode is AlwaysStretched or
    /// AlwaysVerticalStretched: 0=Cover, 1=Fill.
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
    rounded_mode: AtomicBool::new(false),
    track_info_display: AtomicU8::new(0),
    nav_layout: AtomicU8::new(0),
    nav_display_mode: AtomicU8::new(0),
    slot_row_height: AtomicU8::new(1), // Default variant
    opacity_gradient: AtomicBool::new(true),
    slot_text_links: AtomicBool::new(true),
    horizontal_volume: AtomicBool::new(false),
    strip_show_title: AtomicBool::new(true),
    strip_show_artist: AtomicBool::new(true),
    strip_show_album: AtomicBool::new(true),
    strip_show_format_info: AtomicBool::new(true),
    strip_merged_mode: AtomicBool::new(false),
    strip_click_action: AtomicU8::new(0), // GoToQueue
    strip_show_labels: AtomicBool::new(true),
    strip_separator: AtomicU8::new(0), // Dot
    albums_artwork_overlay: AtomicBool::new(true),
    artists_artwork_overlay: AtomicBool::new(true),
    songs_artwork_overlay: AtomicBool::new(true),
    playlists_artwork_overlay: AtomicBool::new(true),
    artwork_column_mode: AtomicU8::new(0),        // Auto
    artwork_column_stretch_fit: AtomicU8::new(0), // Cover
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

/// Title-font family. When empty, `title_font()` falls back to `ui_font()`
/// so unconfigured themes behave like the body font.
static TITLE_FONT_FAMILY: LazyLock<RwLock<String>> = LazyLock::new(|| RwLock::new(String::new()));

/// Cached title-font storage to avoid leaking memory on every reload.
static TITLE_FONT_CACHE: LazyLock<RwLock<(String, Font)>> =
    LazyLock::new(|| RwLock::new((String::new(), Font::DEFAULT)));

/// Get the title font — used for hero titles, view headers, modal headings.
/// Falls back to `ui_font()` when no title font is configured (default).
#[inline]
pub(crate) fn title_font() -> Font {
    let current_family = { TITLE_FONT_FAMILY.read().clone() };

    if current_family.is_empty() {
        // No title font configured — delegate to body font (which has its own cache).
        return ui_font();
    }

    {
        let cache = TITLE_FONT_CACHE.read();
        if cache.0 == current_family {
            return cache.1;
        }
    }

    // Slow path: title font changed, update cache.
    let new_font = Font::with_family(iced::font::Family::name(&current_family));
    let mut cache = TITLE_FONT_CACHE.write();
    *cache = (current_family, new_font);

    new_font
}

/// Set the title-font family (called from settings / config loading once
/// the title-font setting is wired up).
#[allow(dead_code)] // Wired up by L5 settings lane.
pub(crate) fn set_title_font_family(family: String) {
    let mut guard = TITLE_FONT_FAMILY.write();
    *guard = family;
}

/// Get the current title-font family name (for settings UI display).
#[allow(dead_code)] // Wired up by L5 settings lane.
pub(crate) fn title_font_family() -> String {
    TITLE_FONT_FAMILY.read().clone()
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

// Scale constants are wired up by the Wave-1 widget lanes — silence the
// dead-code warnings until those callers land on `redesign`.

/// xs (4 px) — checkboxes, hex swatches, small pips.
const R_XS: f32 = 4.0;
/// sm (8 px) — mode buttons, badges, hover pills.
const R_SM: f32 = 8.0;
/// md (12 px) — cards, popovers, album art, category tiles.
#[allow(dead_code)]
const R_MD: f32 = 12.0;
/// lg (18 px) — list shells, modal frames, hero panels.
const R_LG: f32 = 18.0;
/// pill (999 px) — tabs, transport buttons, search, sliders.
const R_PILL: f32 = 999.0;

/// Returns true if rounded corners mode is enabled
#[inline]
pub(crate) fn is_rounded_mode() -> bool {
    UI_MODE.rounded_mode.load(Ordering::Relaxed)
}

/// Set rounded corners mode (call when user toggles the setting)
#[inline]
pub(crate) fn set_rounded_mode(enabled: bool) {
    UI_MODE.rounded_mode.store(enabled, Ordering::Relaxed);
    debug!(" Rounded mode changed: rounded_mode={}", enabled);
}

/// Get the legacy UI border radius (6 px in rounded mode, 0 in flat).
///
/// Kept for back-compat while widgets migrate to the scale helpers
/// (`ui_radius_xs/sm/md/lg/pill`). New code should call the role-appropriate
/// helper directly.
#[inline]
pub(crate) fn ui_border_radius() -> iced::border::Radius {
    if is_rounded_mode() {
        ROUNDED_RADIUS.into()
    } else {
        0.0.into()
    }
}

/// Scale step `xs` — 4 px in rounded mode, 0 in flat. Use for checkboxes,
/// swatches, tiny chips.
#[inline]
pub(crate) fn ui_radius_xs() -> iced::border::Radius {
    if is_rounded_mode() {
        R_XS.into()
    } else {
        0.0.into()
    }
}

/// Scale step `sm` — 8 px in rounded mode, 0 in flat. Use for mode buttons,
/// badges, format pills.
#[inline]
pub(crate) fn ui_radius_sm() -> iced::border::Radius {
    if is_rounded_mode() {
        R_SM.into()
    } else {
        0.0.into()
    }
}

/// Scale step `md` — 12 px in rounded mode, 0 in flat. Use for cards,
/// popovers, album art, category tiles.
#[inline]
pub(crate) fn ui_radius_md() -> iced::border::Radius {
    if is_rounded_mode() {
        R_MD.into()
    } else {
        0.0.into()
    }
}

/// Scale step `lg` — 18 px in rounded mode, 0 in flat. Use for list shells,
/// modal frames, stats strips.
#[inline]
pub(crate) fn ui_radius_lg() -> iced::border::Radius {
    if is_rounded_mode() {
        R_LG.into()
    } else {
        0.0.into()
    }
}

/// Scale step `pill` — 999 px in rounded mode, 0 in flat. Use for tabs,
/// transport buttons, search field, slider handles.
#[inline]
pub(crate) fn ui_radius_pill() -> iced::border::Radius {
    if is_rounded_mode() {
        R_PILL.into()
    } else {
        0.0.into()
    }
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

/// Background color for the 24 px status strip — meaningfully darker
/// than `bg0_hard()` so the strip reads as its own band below the player
/// bar. Calibrated to match the design CSS delta (Everforest target:
/// `bg0_hard=#232A2E` → `status_strip=#1d2326`, a ~17 % darken).
#[inline]
pub(crate) fn status_strip_bg() -> Color {
    darken(bg0_hard(), 0.17)
}

// ============================================================================
// Active Accent Helper
// ============================================================================

/// Active-tab accent color — uses `accent()` in rounded+light mode for contrast
/// on light backgrounds, `accent_bright()` everywhere else.
///
/// Shared by both the horizontal nav bar and the vertical side nav bar.
///
/// L2 (nav-chrome) replaced the per-mode underline/text-only active
/// state with a full-cell `accent_bright()` fill, so this helper has
/// no callers in the redesign. Kept as the canonical accent-resolver
/// for any future surface that wants the rounded-light contrast bump.
#[inline]
#[allow(dead_code)]
pub(crate) fn active_accent() -> Color {
    if is_rounded_mode() && is_light_mode() {
        accent()
    } else {
        accent_bright()
    }
}

use nokkvi_data::types::player_settings::TrackInfoDisplay;

use crate::atomic_u8_enum::{AtomicU8Enum, atomic_u8_enum};

atomic_u8_enum! {
    TrackInfoDisplay {
        0 => Off,
        1 => PlayerBar,
        2 => TopBar,
        3 => ProgressTrack,
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
/// True when `TrackInfoDisplay::TopBar` AND side-nav layout are both active.
///
/// **Single source of truth** — use this instead of ad-hoc compound checks.
#[inline]
pub(crate) fn show_top_bar_strip() -> bool {
    track_info_display() == TrackInfoDisplay::TopBar && !is_top_nav()
}

/// Whether the artwork-elevation feature is *enabled* by the user's theme
/// settings.
///
/// True when the top-nav layout is active AND the metadata strip lives
/// somewhere other than the top bar (i.e. `Off`, `PlayerBar`, or
/// `ProgressTrack`) — in those modes the top nav doesn't carry any
/// now-playing metadata, so its right portion is free real estate that the
/// artwork can take over.
///
/// `TopBar` keeps the regular column-stacked layout because the metadata
/// strip still needs the full nav width.
///
/// This is only the *theme* gate — `Nokkvi::elevated_artwork_extent`
/// additionally excludes split-view, ineligible views, and the Auto-mode
/// portrait fallback before publishing the result through each `*ViewData`
/// as `BaseSlotListLayoutConfig::elevated`, which `horizontal_layout`
/// finally reads.
#[inline]
pub(crate) fn is_artwork_elevated() -> bool {
    is_top_nav() && track_info_display() != TrackInfoDisplay::TopBar
}

// ============================================================================
// Nav Layout Control
// ============================================================================

use nokkvi_data::types::player_settings::{NavDisplayMode, NavLayout};

atomic_u8_enum! {
    NavLayout {
        0 => Top,
        1 => Side,
        2 => None,
    } default Top
}

/// Returns true if side navigation layout is active
#[inline]
pub(crate) fn is_side_nav() -> bool {
    UI_MODE.nav_layout.load(Ordering::Relaxed) == 1
}

/// Returns true if the minimalist (no-chrome) layout is active
#[inline]
pub(crate) fn is_none_nav() -> bool {
    UI_MODE.nav_layout.load(Ordering::Relaxed) == 2
}

/// Returns true if the top-bar navigation layout is active (the default)
#[inline]
pub(crate) fn is_top_nav() -> bool {
    UI_MODE.nav_layout.load(Ordering::Relaxed) == 0
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
        0 => TextOnly,
        1 => TextAndIcons,
        2 => IconsOnly,
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
        0 => Compact,
        1 => Default,
        2 => Comfortable,
        3 => Spacious,
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

// ============================================================================
// Strip Field Visibility Controls
// ============================================================================

use nokkvi_data::types::player_settings::{StripClickAction, StripSeparator};

atomic_u8_enum! {
    StripClickAction {
        0 => GoToQueue,
        1 => GoToAlbum,
        2 => GoToArtist,
        3 => CopyTrackInfo,
        4 => DoNothing,
    } default GoToQueue
}

atomic_u8_enum! {
    StripSeparator {
        0 => Dot,
        1 => Bullet,
        2 => Pipe,
        3 => EmDash,
        4 => Slash,
        5 => Bar,
    } default Dot
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

// Encoding NOTE: the atomic uses 0=Auto, 1=AlwaysNative, 2=AlwaysStretched,
// 3=Never (kept where it is for redb back-compat — do not renumber even
// though `Never` sits awkwardly between the two Always-mode clusters),
// 4=AlwaysVerticalNative, 5=AlwaysVerticalStretched. New variants must be
// appended at 6+; the `atomic_u8_enum!` loader falls back to `Auto` for
// unknown values and the store half is enum-exhaustive (so adding a variant
// forces a compile error here).
atomic_u8_enum! {
    ArtworkColumnMode {
        0 => Auto,
        1 => AlwaysNative,
        2 => AlwaysStretched,
        3 => Never,
        4 => AlwaysVerticalNative,
        5 => AlwaysVerticalStretched,
    } default Auto
}

atomic_u8_enum! {
    ArtworkStretchFit {
        0 => Cover,
        1 => Fill,
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

/// Dedicated now-playing slot highlight color.
///
/// Defaults to `accent()` when not explicitly configured, allowing themes
/// to decouple the playing-track highlight from the general accent without
/// affecting nav bars, borders, or other accent-colored UI.
#[inline]
pub(crate) fn now_playing_color() -> Color {
    read_color(|t| t.now_playing)
}

/// Dedicated selected/center slot highlight color.
///
/// Defaults to `accent_bright()` when not explicitly configured, allowing
/// themes to decouple the selected-slot highlight from the general accent
/// without affecting nav bars, borders, or other accent-colored UI.
#[inline]
pub(crate) fn selected_color() -> Color {
    read_color(|t| t.selected)
}

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
#[inline]
#[allow(dead_code)] // Base variant available for future use (bright variant used by star ratings)
pub(crate) fn star() -> Color {
    read_color(|t| t.star)
}
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

/// Themed tooltip container style — dark/light aware with 3D border
pub(crate) fn container_tooltip(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(bg0_hard().into()),
        border: iced::Border {
            color: bg3(),
            width: 1.0,
            radius: 2.0.into(),
        },
        text_color: Some(fg1()),
        ..Default::default()
    }
}

/// Full-width horizontal separator line.
///
/// Renders as a `bg1`-colored container with the given pixel height.
/// Replaces the inline `container(space()).width(Fill).height(Fixed(h)).style(bg1)` pattern
/// that was duplicated across `player_bar.rs`, `track_info_strip.rs`, and `app_view.rs`.
pub(crate) fn horizontal_separator<'a, M: 'a>(height: f32) -> iced::Element<'a, M> {
    use iced::{
        Length,
        widget::{container, space},
    };
    container(space())
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .style(move |_| container::Style {
            background: Some(bg1().into()),
            ..Default::default()
        })
        .into()
}

/// Fixed-height vertical separator line (1px wide, `bg3` colored).
///
/// Used inside info strip rows to delineate fields.
pub(crate) fn vertical_separator<'a, M: 'a>(height: f32) -> iced::Element<'a, M> {
    use iced::{
        Length,
        widget::{container, space},
    };
    container(space())
        .width(Length::Fixed(1.0))
        .height(Length::Fixed(height))
        .style(move |_| container::Style {
            background: Some(bg3().into()),
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

/// Variant flag chosen by `nav_separator` callers — selects between the
/// horizontal nav bar's tab separator (vertical 2-px rule, can hide in
/// rounded mode) and the horizontal cross-bar separator drawn inside the
/// side nav bar (between vertical tabs).
///
/// L2 (nav-chrome) replaced the shared 2-px `bg1()` separator with a
/// 1-px `border()`-colored rule local to each nav bar (different inset
/// rules in rounded mode), so this helper has no callers in the
/// redesign. Kept as the canonical "thick separator" recipe for any
/// future surface that wants the old visual.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavSeparatorAxis {
    /// 2-px wide vertical line, `Length::Fill` tall — used between
    /// horizontal nav-bar tabs.
    Vertical,
    /// `Length::Fill` wide, 2-px tall — used between side-nav vertical tabs.
    Horizontal,
}

/// 2-px nav-bar separator (vertical between top-nav tabs, horizontal between
/// side-nav tabs). When `force_visible` is `false`, the separator is hidden
/// in rounded mode to match the unbordered Material-style chrome; passing
/// `true` keeps it visible (used for trailing/leading separators that
/// bracket the tab strip).
///
/// Replaces `tab_separator` / `info_separator` / the inline `separator()`
/// lambda formerly duplicated across `nav_bar` and `side_nav_bar`.
///
/// L2 (nav-chrome) replaced the shared 2-px `bg1()` separator with a
/// 1-px `theme::border()`-colored rule local to each nav bar.
#[allow(dead_code)]
pub(crate) fn nav_separator<'a, M: 'a>(
    axis: NavSeparatorAxis,
    force_visible: bool,
) -> iced::Element<'a, M> {
    use iced::{
        Length,
        widget::{Space, container},
    };
    let (w, h) = match axis {
        NavSeparatorAxis::Vertical => (Length::Fixed(2.0), Length::Fill),
        NavSeparatorAxis::Horizontal => (Length::Fill, Length::Fixed(2.0)),
    };
    container(Space::new())
        .width(w)
        .height(h)
        .style(move |_| container::Style {
            background: if is_rounded_mode() && !force_visible {
                None
            } else {
                Some(bg1().into())
            },
            ..Default::default()
        })
        .into()
}

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

/// Themed search/filter text input style matching the Gruvbox palette.
///
/// Previously the default for the view-header search bar; the L3 flat
/// redesign owns that default now (see `search_bar::flat_search_input_style`).
/// Kept around for the L5 settings widgets lane and any future caller that
/// wants the legacy 2 px-bordered look — clippy's `dead_code` allow is
/// intentional until a Wave-1 lane wires it back up.
#[allow(dead_code)]
pub(crate) fn search_input_style(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: (bg0_soft()).into(),
        border: iced::Border {
            color: if matches!(status, text_input::Status::Focused { .. }) {
                accent_bright()
            } else {
                iced::Color::TRANSPARENT
            },
            width: 2.0,
            radius: ui_border_radius(),
        },
        icon: fg4(),
        placeholder: fg4(),
        value: fg0(),
        selection: selection_color(),
    }
}

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

    /// Sequential guard for tests that flip globals like `set_light_mode` or
    /// mutate the `UI_MODE` atomics. `parking_lot::Mutex` avoids std-lock
    /// poisoning if one test panics.
    static THEME_MODE_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    // ------------------------------------------------------------------------
    // atomic_u8_enum! macro — verifies that the loader/store impls emitted
    // for every `UiModeFlags` enum preserve the on-disk byte encodings that
    // app.redb depends on, and that unknown bytes fall back to the declared
    // default variant (forward-compat for legacy `app.redb` files written by
    // a future build).
    // ------------------------------------------------------------------------

    /// Roundtrip every variant of two enums (one with a small variant set, one
    /// with a larger one) through `to_u8` / `from_u8`. Exercising the macro
    /// expansion twice guarantees we're testing the macro itself, not just one
    /// hand-written impl.
    #[test]
    fn atomic_u8_enum_macro_emits_roundtrip() {
        // NavLayout: 3 variants, contiguous {0,1,2}.
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

        // StripSeparator: 6 variants, contiguous {0..=5}. Exercises a larger
        // table so we catch any macro misexpansion that only manifests with
        // more arms.
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

    /// An unknown stored byte (e.g. a future variant written by a newer build
    /// then read by an older build) MUST decode to the declared default
    /// variant. This preserves the original `match { _ => Default }` shape and
    /// is the redb on-disk back-compat contract.
    #[test]
    fn atomic_u8_enum_unknown_byte_falls_back_to_default() {
        // TrackInfoDisplay default = Off.
        assert_eq!(TrackInfoDisplay::from_u8(255), TrackInfoDisplay::Off);
        assert_eq!(TrackInfoDisplay::from_u8(99), TrackInfoDisplay::Off);
        // Also verify a byte just past the highest known variant (3) falls back.
        assert_eq!(TrackInfoDisplay::from_u8(4), TrackInfoDisplay::Off);
    }

    /// `ArtworkColumnMode`'s integer encoding has `Never` sitting at byte 3,
    /// awkwardly between the two `Always*` clusters at bytes 1-2 and the two
    /// `AlwaysVertical*` cluster at bytes 4-5 — declaration order and byte
    /// order do not match. This is locked in for redb back-compat and any
    /// renumbering would silently corrupt every existing user's queue/session
    /// state. Roundtrip every variant byte-for-byte so we catch a future
    /// "tidy up the enum" PR that flips Never to byte 5 or 6.
    #[test]
    fn artwork_column_mode_non_contiguous_encoding_preserved() {
        // The full {byte → variant} table the redb on-disk format depends on.
        let table = [
            (0u8, ArtworkColumnMode::Auto),
            (1u8, ArtworkColumnMode::AlwaysNative),
            (2u8, ArtworkColumnMode::AlwaysStretched),
            (3u8, ArtworkColumnMode::Never),
            (4u8, ArtworkColumnMode::AlwaysVerticalNative),
            (5u8, ArtworkColumnMode::AlwaysVerticalStretched),
        ];

        for (byte, variant) in table {
            assert_eq!(
                ArtworkColumnMode::from_u8(byte),
                variant,
                "ArtworkColumnMode byte {byte} must decode to {variant:?}"
            );
            assert_eq!(
                variant.to_u8(),
                byte,
                "ArtworkColumnMode::{variant:?} must encode to byte {byte}"
            );
        }

        // The two "AlwaysVertical" variants specifically must round-trip
        // through byte 5 / byte 4, not through the bytes that the declaration
        // order would suggest (5 is the last variant declared but its byte
        // sits BELOW Never's variant-declaration position).
        assert_eq!(
            ArtworkColumnMode::AlwaysVerticalStretched.to_u8(),
            5,
            "AlwaysVerticalStretched MUST encode to byte 5"
        );
        assert_eq!(
            ArtworkColumnMode::from_u8(5),
            ArtworkColumnMode::AlwaysVerticalStretched,
            "byte 5 MUST decode to AlwaysVerticalStretched"
        );
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

    /// `nav_separator` must compile for both axis variants and produce real
    /// `Element`s ready to be pushed onto a row/column. The exact pixel
    /// dimensions stay verified by the integration-level nav-bar layout
    /// tests; here we just pin the type-level contract so a future
    /// refactor cannot silently change the return type.
    #[test]
    fn nav_separator_compiles_for_both_axes() {
        let _v: iced::Element<'_, ()> = nav_separator(NavSeparatorAxis::Vertical, false);
        let _h: iced::Element<'_, ()> = nav_separator(NavSeparatorAxis::Horizontal, true);
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
