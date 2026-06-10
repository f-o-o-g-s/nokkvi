//! Theme colors and styling helpers
//!
//! Colors are loaded from named theme files at `~/.config/nokkvi/themes/`.
//! Light/dark mode can be toggled at runtime.
//!
//! All color accessors are functions (not statics) so they react to hot-reload via `reload_theme()`.

use std::sync::{
    Arc, LazyLock,
    atomic::{AtomicU64, Ordering},
};

use arc_swap::ArcSwap;
use iced::{Color, Font};
#[cfg(test)]
use nokkvi_data::types::player_settings::{
    ArtworkColumnMode, ArtworkStretchFit, NavDisplayMode, NavLayout, SlotRowHeight,
    StripClickAction, StripSeparator, TrackInfoDisplay,
};
use nokkvi_data::types::theme_file::{ThemeFile, VisualizerColors};
use parking_lot::RwLock;
use tracing::debug;

use crate::theme_config::{
    ResolvedDualTheme, ResolvedTheme, load_active_theme_file, load_resolved_dual_theme,
};

mod colors;
mod ui_mode;

pub(crate) use colors::*;
pub(crate) use ui_mode::*;

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

use nokkvi_data::types::player_settings::RoundedMode;

use crate::atomic_u8_enum::{AtomicU8Enum, atomic_u8_enum};

atomic_u8_enum! {
    RoundedMode {
        Off,
        On,
        PlayerOnly,
    } default Off
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
