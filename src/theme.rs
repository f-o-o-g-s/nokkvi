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

/// Radius value applied to container borders when rounded mode is enabled
const ROUNDED_RADIUS: f32 = 6.0;

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

/// Get the UI border radius based on current rounded mode setting.
///
/// Returns uniform `ROUNDED_RADIUS` when enabled, `0.0` when disabled.
/// Use this instead of hardcoding `radius: 0.0.into()` in container styles.
#[inline]
pub(crate) fn ui_border_radius() -> iced::border::Radius {
    if is_rounded_mode() {
        ROUNDED_RADIUS.into()
    } else {
        0.0.into()
    }
}

// ============================================================================
// Active Accent Helper
// ============================================================================

/// Active-tab accent color — uses `accent()` in rounded+light mode for contrast
/// on light backgrounds, `accent_bright()` everywhere else.
///
/// Shared by both the horizontal nav bar and the vertical side nav bar.
#[inline]
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
// 3D Border Helpers
// ============================================================================
// These functions return (highlight, shadow) color pairs that automatically
// flip based on light_mode to maintain correct 3D visual effects.
//
// For RAISED elements (buttons, handles): highlight goes on top/left, shadow on bottom/right
// For INSET elements (tracks, grooves): shadow goes on top/left, highlight on bottom/right

/// Returns (top_left_color, bottom_right_color) for 3D raised elements (buttons, handles)
///
/// In dark mode: light color on top/left (highlight), dark on bottom/right (shadow)
/// In light mode: flipped for correct visual effect
#[inline]
pub(crate) fn border_3d_raised() -> (Color, Color) {
    if is_light_mode() {
        // Light mode: swap to maintain 3D illusion
        (bg0(), bg2())
    } else {
        // Dark mode: standard (highlight=light, shadow=dark)
        (bg2(), bg0())
    }
}

/// Returns (top_left_color, bottom_right_color) for 3D inset elements (tracks, grooves)
///
/// In dark mode: dark color on top/left (shadow), light on bottom/right (highlight)
/// In light mode: flipped for correct visual effect
#[inline]
pub(crate) fn border_3d_inset() -> (Color, Color) {
    if is_light_mode() {
        // Light mode: swap to maintain 3D illusion
        (bg2(), bg0())
    } else {
        // Dark mode: standard (shadow=dark on top/left)
        (bg0(), bg2())
    }
}

// Color blending for natural 3D effects
// Instead of pure black/white overlays (which look metallic), we blend the base
// accent color toward white/black to create tinted highlights/shadows that stay
// in the same color family.

/// Blend a color toward a target color by the given factor (0.0 = base, 1.0 = target)
#[inline]
fn blend_toward(base: Color, target: Color, factor: f32) -> Color {
    Color {
        r: base.r + (target.r - base.r) * factor,
        g: base.g + (target.g - base.g) * factor,
        b: base.b + (target.b - base.b) * factor,
        a: base.a, // Keep original alpha
    }
}

/// Lighten a color by blending it toward white
#[inline]
fn lighten(color: Color, amount: f32) -> Color {
    blend_toward(color, Color::WHITE, amount)
}

/// Darken a color by blending it toward black  
#[inline]
pub(crate) fn darken(color: Color, amount: f32) -> Color {
    blend_toward(color, Color::BLACK, amount)
}

/// Returns (highlight_color, shadow_color) for 3D raised elements derived from
/// an arbitrary accent-family base color.
///
/// Lighten/darken ratios are mode-aware: light mode prefers a subtle highlight
/// and moderate shadow; dark mode prefers a moderate highlight and subtle
/// shadow. Shared between the bright-accent and darker-accent wrappers below.
#[inline]
fn border_3d_accent_from_base(base: Color) -> (Color, Color) {
    if is_light_mode() {
        // Light mode: subtle lighten, moderate darken
        (lighten(base, 0.25), darken(base, 0.35))
    } else {
        // Dark mode: moderate lighten, subtle darken
        (lighten(base, 0.35), darken(base, 0.30))
    }
}

/// Returns (highlight_color, shadow_color) for 3D raised accent-colored elements
///
/// Derives highlight/shadow from the base accent color by blending toward white/black.
/// This creates a cohesive color family instead of metallic pure black/white overlays.
#[inline]
pub(crate) fn border_3d_accent_raised() -> (Color, Color) {
    border_3d_accent_from_base(accent_bright())
}

/// Returns (highlight_color, shadow_color) for 3D raised accent elements using darker accent
///
/// Same approach but uses the darker accent as the base color.
#[inline]
pub(crate) fn border_3d_accent_darker_raised() -> (Color, Color) {
    border_3d_accent_from_base(accent())
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

/// Themed search/filter text input style matching the Gruvbox palette.
/// Used in view headers and settings sub-lists.
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

    // ------------------------------------------------------------------------
    // border_3d_accent_from_base — helper extracted from the two public
    // wrappers (`border_3d_accent_raised` / `border_3d_accent_darker_raised`).
    // These tests pin both the lighten/darken ratios (so a future "tweak the
    // 3D look" PR can't silently regress them) and the wrapper equivalence
    // (so a wrapper edit can't accidentally diverge from the helper).
    // ------------------------------------------------------------------------

    /// Sequential guard for tests that flip `theme::set_light_mode` (a global
    /// atomic). Mirrors the pattern in `widgets/boat_tests.rs` so the helper
    /// tests below don't race with each other under multi-threaded
    /// `cargo test`. `parking_lot::Mutex` avoids std-lock poisoning if one
    /// test panics.
    static THEME_MODE_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    /// Approximate-equality helper for `Color` channel comparisons. Floating-
    /// point blend math means `0.5 + (1.0 - 0.5) * 0.35` is unlikely to be
    /// exactly representable, so we tolerate a tiny epsilon.
    fn assert_color_eq(actual: Color, expected: Color, what: &str) {
        let eps = 1e-6_f32;
        assert!(
            (actual.r - expected.r).abs() < eps
                && (actual.g - expected.g).abs() < eps
                && (actual.b - expected.b).abs() < eps
                && (actual.a - expected.a).abs() < eps,
            "{what}: expected {expected:?}, got {actual:?}"
        );
    }

    /// Lighten/darken ratios MUST stay at (0.25, 0.35) under light mode and
    /// (0.35, 0.30) under dark mode. A regression here would shift every 3D
    /// accent button's highlight/shadow contrast simultaneously, which the
    /// human owner would not catch without a side-by-side palette diff.
    #[test]
    fn border_3d_accent_from_base_pins_lighten_darken_ratios() {
        let _guard = THEME_MODE_LOCK.lock();
        let initial_mode = is_light_mode();

        let base = Color::from_rgb(0.5, 0.3, 0.7);

        set_light_mode(false);
        let (hi_dark, lo_dark) = border_3d_accent_from_base(base);
        assert_color_eq(hi_dark, lighten(base, 0.35), "dark-mode highlight");
        assert_color_eq(lo_dark, darken(base, 0.30), "dark-mode shadow");

        set_light_mode(true);
        let (hi_light, lo_light) = border_3d_accent_from_base(base);
        assert_color_eq(hi_light, lighten(base, 0.25), "light-mode highlight");
        assert_color_eq(lo_light, darken(base, 0.35), "light-mode shadow");

        // Restore the global atomic before exit so we don't bleed state into
        // any other test in this binary that observes `is_light_mode()`.
        set_light_mode(initial_mode);
    }

    /// The two public wrappers MUST be one-line delegations to the helper:
    /// bright-accent base and darker-accent base, respectively. Pinning this
    /// equivalence prevents a future edit from re-introducing copy-pasted
    /// math in either wrapper.
    #[test]
    fn border_3d_accent_wrappers_delegate_to_helper() {
        let _guard = THEME_MODE_LOCK.lock();

        let (raised_hi, raised_lo) = border_3d_accent_raised();
        let (expected_raised_hi, expected_raised_lo) = border_3d_accent_from_base(accent_bright());
        assert_color_eq(raised_hi, expected_raised_hi, "raised highlight");
        assert_color_eq(raised_lo, expected_raised_lo, "raised shadow");

        let (darker_hi, darker_lo) = border_3d_accent_darker_raised();
        let (expected_darker_hi, expected_darker_lo) = border_3d_accent_from_base(accent());
        assert_color_eq(darker_hi, expected_darker_hi, "darker-raised highlight");
        assert_color_eq(darker_lo, expected_darker_lo, "darker-raised shadow");
    }

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
}
