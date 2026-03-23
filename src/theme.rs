//! Gruvbox Dark/Light theme colors and styling helpers
//! Colors from: https://github.com/morhetz/gruvbox
//!
//! This module provides centralized color definitions matching the Slint/QML reference implementations.
//! Colors are loaded from config.toml at startup. Light/dark mode can be toggled at runtime.
//!
//! All color accessors are functions (not statics) so they react to hot-reload via `reload_theme()`.

use std::sync::{
    LazyLock,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

use iced::{Color, Font};
use parking_lot::RwLock;
use tracing::debug;

use crate::theme_config::{ResolvedDualTheme, ResolvedTheme, load_resolved_dual_theme};

// ============================================================================
// LazyLock Theme Loading (with hot-reload support via RwLock)
// ============================================================================

/// Global resolved dual theme (can be reloaded at runtime)
static DUAL_THEME: LazyLock<RwLock<ResolvedDualTheme>> =
    LazyLock::new(|| RwLock::new(load_resolved_dual_theme()));

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
    /// Navigation layout: 0 = Top, 1 = Side
    nav_layout: AtomicU8,
    /// Navigation display: 0 = TextOnly, 1 = TextAndIcons, 2 = IconsOnly
    nav_display_mode: AtomicU8,
    /// Target row height for slot lists (discriminant: 0=Compact 1=Default 2=Comfortable 3=Spacious)
    slot_row_height: AtomicU8,
    /// Whether the opacity gradient on non-center slots is enabled
    opacity_gradient: AtomicBool,
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
    /// Strip click action: 0=GoToQueue, 1=GoToAlbum, 2=GoToArtist, 3=CopyTrackInfo, 4=DoNothing
    strip_click_action: AtomicU8,
}

static UI_MODE: UiModeFlags = UiModeFlags {
    light_mode: AtomicBool::new(false),
    rounded_mode: AtomicBool::new(false),
    track_info_display: AtomicU8::new(0),
    nav_layout: AtomicU8::new(0),
    nav_display_mode: AtomicU8::new(0),
    slot_row_height: AtomicU8::new(1), // Default variant
    opacity_gradient: AtomicBool::new(true),
    horizontal_volume: AtomicBool::new(false),
    strip_show_title: AtomicBool::new(true),
    strip_show_artist: AtomicBool::new(true),
    strip_show_album: AtomicBool::new(true),
    strip_show_format_info: AtomicBool::new(true),
    strip_click_action: AtomicU8::new(0), // GoToQueue
};

/// Reload theme from config.toml (hot-reload support)
/// Call this when config file changes to update colors without restart
pub(crate) fn reload_theme() {
    let new_theme = load_resolved_dual_theme();
    let mut theme = DUAL_THEME.write();
    *theme = new_theme;
    debug!(" Theme hot-reloaded from config.toml");
}

/// Get a clone of current theme based on light mode
/// (Clones to avoid holding RwLock read guard across function calls)
#[inline]
fn current_theme() -> ResolvedTheme {
    let guard = DUAL_THEME.read();
    if UI_MODE.light_mode.load(Ordering::Relaxed) {
        guard.light.clone()
    } else {
        guard.dark.clone()
    }
}

// ============================================================================
// Font Configuration
// ============================================================================

/// Cached font storage to avoid leaking memory on every reload.
/// Stores (font_name, resolved_font) pairs.
static FONT_CACHE: LazyLock<RwLock<(String, Font)>> =
    LazyLock::new(|| RwLock::new((String::new(), Font::DEFAULT)));

/// Get the UI font - loaded from config.toml (hot-reloadable)
/// Default: System sans-serif font (works on all systems)
#[inline]
pub(crate) fn ui_font() -> Font {
    let current_family = {
        let guard = DUAL_THEME.read();
        guard.font_family.clone()
    };

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

/// Returns the current track info display mode
#[inline]
pub(crate) fn track_info_display() -> TrackInfoDisplay {
    match UI_MODE.track_info_display.load(Ordering::Relaxed) {
        1 => TrackInfoDisplay::PlayerBar,
        2 => TrackInfoDisplay::TopBar,
        3 => TrackInfoDisplay::ProgressTrack,
        _ => TrackInfoDisplay::Off,
    }
}

/// Set track info display mode (call when user changes the setting)
#[inline]
pub(crate) fn set_track_info_display(mode: TrackInfoDisplay) {
    let val = match mode {
        TrackInfoDisplay::Off => 0,
        TrackInfoDisplay::PlayerBar => 1,
        TrackInfoDisplay::TopBar => 2,
        TrackInfoDisplay::ProgressTrack => 3,
    };
    UI_MODE.track_info_display.store(val, Ordering::Relaxed);
    debug!(" Track info display changed: {}", mode);
}

/// Whether the player bar should show the track info strip below controls.
///
/// True when `TrackInfoDisplay::PlayerBar` AND side-nav layout are both active.
/// In top-nav mode the nav bar already surfaces track/format info, so the strip
/// is suppressed to avoid redundancy.
///
/// **Single source of truth** — use this instead of ad-hoc compound checks.
#[inline]
pub(crate) fn show_player_bar_strip() -> bool {
    track_info_display() == TrackInfoDisplay::PlayerBar && is_side_nav()
}

/// Whether the top bar track info strip should be rendered above content.
///
/// True when `TrackInfoDisplay::TopBar` AND side-nav layout are both active.
///
/// **Single source of truth** — use this instead of ad-hoc compound checks.
#[inline]
pub(crate) fn show_top_bar_strip() -> bool {
    track_info_display() == TrackInfoDisplay::TopBar && is_side_nav()
}

// ============================================================================
// Nav Layout Control
// ============================================================================

use nokkvi_data::types::player_settings::{NavDisplayMode, NavLayout};

/// Returns true if side navigation layout is active
#[inline]
pub(crate) fn is_side_nav() -> bool {
    UI_MODE.nav_layout.load(Ordering::Relaxed) == 1
}

/// Set the navigation layout from a NavLayout enum value
#[inline]
pub(crate) fn set_nav_layout(layout: NavLayout) {
    let val = match layout {
        NavLayout::Top => 0,
        NavLayout::Side => 1,
    };
    UI_MODE.nav_layout.store(val, Ordering::Relaxed);
    debug!(" Nav layout changed: nav_layout={}", layout);
}

// ============================================================================
// Nav Display Mode Control
// ============================================================================

/// Get the current navigation display mode
#[inline]
pub(crate) fn nav_display_mode() -> NavDisplayMode {
    match UI_MODE.nav_display_mode.load(Ordering::Relaxed) {
        1 => NavDisplayMode::TextAndIcons,
        2 => NavDisplayMode::IconsOnly,
        _ => NavDisplayMode::TextOnly,
    }
}

/// Set the navigation display mode from a NavDisplayMode enum value
#[inline]
pub(crate) fn set_nav_display_mode(mode: NavDisplayMode) {
    let val = match mode {
        NavDisplayMode::TextOnly => 0,
        NavDisplayMode::TextAndIcons => 1,
        NavDisplayMode::IconsOnly => 2,
    };
    UI_MODE.nav_display_mode.store(val, Ordering::Relaxed);
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

/// Get the current slot row height enum variant
#[inline]
pub(crate) fn slot_row_height_variant() -> nokkvi_data::types::player_settings::SlotRowHeight {
    use nokkvi_data::types::player_settings::SlotRowHeight;
    match UI_MODE.slot_row_height.load(Ordering::Relaxed) {
        0 => SlotRowHeight::Compact,
        2 => SlotRowHeight::Comfortable,
        3 => SlotRowHeight::Spacious,
        _ => SlotRowHeight::Default,
    }
}

/// Set the target row height for slot lists
#[inline]
pub(crate) fn set_slot_row_height(height: nokkvi_data::types::player_settings::SlotRowHeight) {
    use nokkvi_data::types::player_settings::SlotRowHeight;
    let discriminant = match height {
        SlotRowHeight::Compact => 0,
        SlotRowHeight::Default => 1,
        SlotRowHeight::Comfortable => 2,
        SlotRowHeight::Spacious => 3,
    };
    UI_MODE
        .slot_row_height
        .store(discriminant, Ordering::Relaxed);
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

use nokkvi_data::types::player_settings::StripClickAction;

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

/// Returns the current strip click action
#[inline]
pub(crate) fn strip_click_action() -> StripClickAction {
    match UI_MODE.strip_click_action.load(Ordering::Relaxed) {
        1 => StripClickAction::GoToAlbum,
        2 => StripClickAction::GoToArtist,
        3 => StripClickAction::CopyTrackInfo,
        4 => StripClickAction::DoNothing,
        _ => StripClickAction::GoToQueue,
    }
}

/// Set strip click action (call when user changes the setting)
#[inline]
pub(crate) fn set_strip_click_action(action: StripClickAction) {
    let val = match action {
        StripClickAction::GoToQueue => 0,
        StripClickAction::GoToAlbum => 1,
        StripClickAction::GoToArtist => 2,
        StripClickAction::CopyTrackInfo => 3,
        StripClickAction::DoNothing => 4,
    };
    UI_MODE.strip_click_action.store(val, Ordering::Relaxed);
}

// ============================================================================
// Background Colors
// ============================================================================

#[inline]
pub(crate) fn bg0_hard() -> Color {
    current_theme().bg0_hard
}
#[inline]
pub(crate) fn bg0() -> Color {
    current_theme().bg0
}
#[inline]
pub(crate) fn bg0_soft() -> Color {
    current_theme().bg0_soft
}
#[inline]
pub(crate) fn bg1() -> Color {
    current_theme().bg1
}
#[inline]
pub(crate) fn bg2() -> Color {
    current_theme().bg2
}
#[inline]
pub(crate) fn bg3() -> Color {
    current_theme().bg3
}

// ============================================================================
// Foreground Colors
// ============================================================================

#[inline]
pub(crate) fn fg4() -> Color {
    current_theme().fg4
}
#[inline]
pub(crate) fn fg3() -> Color {
    current_theme().fg3
}
#[inline]
pub(crate) fn fg2() -> Color {
    current_theme().fg2
}
#[inline]
pub(crate) fn fg1() -> Color {
    current_theme().fg1
}
#[inline]
pub(crate) fn fg0() -> Color {
    current_theme().fg0
}

// ============================================================================
// Accent Colors
// ============================================================================

#[inline]
pub(crate) fn accent() -> Color {
    current_theme().accent
}
#[inline]
pub(crate) fn accent_bright() -> Color {
    current_theme().accent_bright
}
#[inline]
pub(crate) fn accent_border_light() -> Color {
    current_theme().accent_border_light
}

/// Dedicated now-playing slot highlight color.
///
/// Defaults to `accent()` when not explicitly configured, allowing themes
/// to decouple the playing-track highlight from the general accent without
/// affecting nav bars, borders, or other accent-colored UI.
#[inline]
pub(crate) fn now_playing_color() -> Color {
    current_theme().now_playing
}

/// Dedicated selected/center slot highlight color.
///
/// Defaults to `accent_bright()` when not explicitly configured, allowing
/// themes to decouple the selected-slot highlight from the general accent
/// without affecting nav bars, borders, or other accent-colored UI.
#[inline]
pub(crate) fn selected_color() -> Color {
    current_theme().selected
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
// Named Palette Colors
// ============================================================================

#[inline]
pub(crate) fn red() -> Color {
    current_theme().red
}
#[inline]
pub(crate) fn red_bright() -> Color {
    current_theme().red_bright
}
#[inline]
pub(crate) fn green() -> Color {
    current_theme().green
}
#[inline]
pub(crate) fn yellow() -> Color {
    current_theme().yellow
}
#[inline]
pub(crate) fn yellow_bright() -> Color {
    current_theme().yellow_bright
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
fn darken(color: Color, amount: f32) -> Color {
    blend_toward(color, Color::BLACK, amount)
}

/// Returns (highlight_color, shadow_color) for 3D raised accent-colored elements
///
/// Derives highlight/shadow from the base accent color by blending toward white/black.
/// This creates a cohesive color family instead of metallic pure black/white overlays.
#[inline]
pub(crate) fn border_3d_accent_raised() -> (Color, Color) {
    let base = accent_bright();

    if is_light_mode() {
        // Light mode: subtle lighten, moderate darken
        (lighten(base, 0.25), darken(base, 0.35))
    } else {
        // Dark mode: moderate lighten, subtle darken
        (lighten(base, 0.35), darken(base, 0.30))
    }
}

/// Returns (highlight_color, shadow_color) for 3D raised accent elements using darker accent
///
/// Same approach but uses the darker accent as the base color.
#[inline]
pub(crate) fn border_3d_accent_darker_raised() -> (Color, Color) {
    let base = accent(); // Darker accent

    if is_light_mode() {
        (lighten(base, 0.25), darken(base, 0.35))
    } else {
        (lighten(base, 0.35), darken(base, 0.30))
    }
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
        success: green(),
        warning: yellow(),
        danger: red(),
    };

    Theme::custom("Gruvbox".to_string(), palette)
}

// ============================================================================
// Toast Level Colors
// ============================================================================

/// Map toast notification level to a theme-appropriate text color.
/// Uses the `normal` (non-bright) color variants because:
/// - In Gruvbox dark, `normal` colors (#98971a, #d79921) are still vivid and readable
/// - In Gruvbox light, `bright` colors (#b8bb26, #fabd2f) wash out against light backgrounds
/// - Theme authors set `normal` variants to be readable against their chosen bg colors
pub(crate) fn toast_level_color(level: nokkvi_data::types::toast::ToastLevel) -> Color {
    use nokkvi_data::types::toast::ToastLevel;
    match level {
        ToastLevel::Info => fg1(),
        ToastLevel::Success => green(),
        ToastLevel::Warning => yellow(),
        ToastLevel::Error => red(),
    }
}
