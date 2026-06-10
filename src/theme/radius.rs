//! Rounded-mode control, the radius scale, player-chrome radius variants,
//! and mode-sensitive chrome sizing (nav bar height, status strip).

use std::sync::atomic::Ordering;

use iced::Color;
use tracing::debug;

use super::{UI_MODE, bg0_hard, blend_toward, darken, fg0, is_light_mode, relative_luminance};

// ============================================================================
// Rounded Mode Control
// ============================================================================

/// Legacy single-radius value (kept for `ui_border_radius()` back-compat).
/// New code prefers the scale helpers (`ui_radius_sm`, `ui_radius_md`, …).
pub(super) const ROUNDED_RADIUS: f32 = 6.0;

// ----------------------------------------------------------------------------
// Radius scale (flat redesign — rounded-mode values per element role)
// ----------------------------------------------------------------------------
// Each helper returns the corresponding radius in rounded mode, `0.0` in flat
// mode, so call sites stay mode-agnostic.

// Scale constants — every helper below consumes one, so they're all live.

/// xs (4 px) — checkboxes, hex swatches, small pips.
pub(super) const R_XS: f32 = 4.0;
/// sm (8 px) — mode buttons, badges, hover pills.
pub(super) const R_SM: f32 = 8.0;
/// md (12 px) — cards, popovers, album art, category tiles.
pub(super) const R_MD: f32 = 12.0;
/// lg (18 px) — list shells, modal frames, hero panels.
pub(super) const R_LG: f32 = 18.0;
/// pill (999 px) — tabs, transport buttons, search, sliders.
pub(super) const R_PILL: f32 = 999.0;

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
pub(super) const STRIP_BAND_DELTA: f32 = 0.035;

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
pub(super) fn strip_band_toward_ink(base: Color, ink: Color, delta: f32) -> Color {
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
