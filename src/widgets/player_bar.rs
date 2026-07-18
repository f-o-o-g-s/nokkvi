//! Player Bar Component
//!
//! Self-contained player controls bar with message bubbling pattern.
//! Receives pure view data and emits actions for root to process.

use iced::{
    Alignment, Color, Element, Length, Theme,
    advanced::svg::Handle,
    font::Weight,
    mouse::ScrollDelta,
    widget::{Svg, button, column, container, mouse_area, row, svg, text, tooltip},
};

use crate::{
    theme, widgets,
    widgets::{hover_overlay::HoverOverlay, progress_bar::CapLabel},
};

// Player bar dimensions (flat redesign). 72 px in both modes — the design
// CSS specified 64 in flat mode and 72 in rounded, but the 8 px difference
// makes the 44 px mode buttons feel cramped (10 px gap each side) in flat
// vs. floating (14 px gap each side) in rounded. Using 72 in both modes
// gives the transport + mode buttons the same airy breathing room across
// the two visual languages.
const BASE_PLAYER_BAR_HEIGHT: f32 = 72.0;
/// 1 px separator line (theme `border()`) framing the MiniPlayer capsule scrub
/// on its top and bottom edges, matching the app's divider language.
const SCRUB_SEPARATOR_HEIGHT: f32 = 1.0;
/// MiniPlayer bar height — a connected "capsule" progress scrub (with elapsed /
/// duration end-caps) rides the top as its own row, framed by a 1 px separator
/// above and below, directly over the content row (sized to the artwork). The
/// capsule is a real layout row, so the bar is taller than the base.
/// `sep + capsule + sep + artwork`.
const MINI_PLAYER_BAR_HEIGHT: f32 =
    2.0 * SCRUB_SEPARATOR_HEIGHT + CAPSULE_SCRUB_HEIGHT + MINI_PLAYER_ARTWORK_SIZE;
const CONTROL_ROW_HEIGHT: f32 = 44.0;
/// Transport button (prev/play/pause/stop/next) — 40×40, borderless flat icon.
const TRANSPORT_SIZE: f32 = 40.0;
/// Mode toggle button (repeat/shuffle/consume/EQ/SFX/crossfade/visualizer).
/// Flat: 38×44; rounded: 40×44.
const MODE_BUTTON_HEIGHT: f32 = 44.0;
/// Height of the track info strip below the player bar in PlayerBar display mode.
/// Re-uses the canonical constant from `track_info_strip.rs` to avoid drift.
use super::track_info_strip::STRIP_HEIGHT_WITH_SEPARATOR as INFO_STRIP_WITH_SEPARATOR;

/// Height of the MiniPlayer "capsule" seek scrub — a full-width connected
/// progress bar across the top of the bar, with elapsed/duration time end-caps
/// butted against the track so they read as one continuous progress element.
const CAPSULE_SCRUB_HEIGHT: f32 = 20.0;
/// Height of the vertical divider between the volume utility and the transports
/// in the MiniPlayer right cluster.
const MINI_DIVIDER_HEIGHT: f32 = 24.0;

#[inline]
fn mode_button_width() -> f32 {
    if theme::is_rounded_for_player() {
        40.0
    } else {
        38.0
    }
}

/// Intra-section button gap (between transport buttons, between mode buttons,
/// between the two vertical volume bars).
const SECTION_BUTTON_GAP: f32 = 4.0;

/// Inter-section gap inside the player bar's main row (between transport and
/// progress, progress and modes, modes and volume).
const MAIN_ROW_INNER_GAP: f32 = 6.0;

/// Width of the transport-controls section — always the 3-button modern set
/// (prev / play-or-pause toggle / next). The section sizes to fit exactly those,
/// so the progress track can claim the rest of the row.
#[inline]
pub(crate) fn transport_section_width() -> f32 {
    const N: f32 = 3.0;
    N * TRANSPORT_SIZE + (N - 1.0) * SECTION_BUTTON_GAP
}

/// Width of the mode-toggles section for the currently-rendered layout —
/// `inline_count` mode buttons (7 minus `kebab_mode_count`) plus a kebab
/// when any modes are culled, plus the hamburger button in `NavLayout::None`.
/// Returns 0 when no modes are inline and no kebab/hamburger renders.
#[inline]
pub(crate) fn mode_section_width(layout: PlayerBarLayout, has_hamburger: bool) -> f32 {
    let mode_btn_w = mode_button_width();
    let chrome_btn_w = super::sizes::TOOLBAR_BUTTON_SIZE;

    let inline_count = (CULL_ORDER.len() as u8).saturating_sub(layout.kebab_mode_count);
    let has_kebab = layout.kebab_mode_count > 0;

    let mut widgets = inline_count as f32 * mode_btn_w;
    let mut count = inline_count as u32;
    if has_kebab {
        widgets += chrome_btn_w;
        count += 1;
    }
    if has_hamburger {
        widgets += chrome_btn_w;
        count += 1;
    }
    if count == 0 {
        return 0.0;
    }
    widgets + (count - 1) as f32 * SECTION_BUTTON_GAP
}

/// Width of the volume-control section for the currently-rendered widgets.
/// Vertical layout sizes for one bar (music only) or two bars (music + SFX
/// when `show_sfx_slider` is true); horizontal layout always uses the
/// horizontal track length since stacking SFX above music doesn't widen it.
#[inline]
pub(crate) fn volume_section_width(show_sfx_slider: bool) -> f32 {
    if crate::theme::is_horizontal_volume() {
        super::volume_slider::HORIZONTAL_LENGTH
    } else if show_sfx_slider {
        2.0 * super::volume_slider::BAR_WIDTH + SECTION_BUTTON_GAP
    } else {
        super::volume_slider::BAR_WIDTH
    }
}

/// Side length of the artwork thumbnail at the left of the `MiniPlayer`
/// content row. The capsule seek scrub is its own row above the content row
/// (framed by 1 px separators), so the content row is sized to this artwork and
/// the bar height is `sep + capsule + sep + artwork` ([`MINI_PLAYER_BAR_HEIGHT`]).
pub(crate) const MINI_PLAYER_ARTWORK_SIZE: f32 = 56.0;
/// Gap between the artwork and the flexible text column inside the section.
const MINI_PLAYER_INNER_GAP: f32 = 8.0;
/// Minimum horizontal room the metadata section needs (artwork + gap + a small
/// legible text slice) before it hides entirely in the COMPACT regime. The text
/// column is `Length::Fill` and each line ellipsizes (no marquee), so above this
/// width it simply fills the available room — see [`show_mini_player_section`].
const MINI_PLAYER_MIN_METADATA_WIDTH: f32 = MINI_PLAYER_ARTWORK_SIZE + MINI_PLAYER_INNER_GAP + 50.0;

/// Minimum width the metadata section + right cluster need to coexist in the
/// COMPACT MiniPlayer regime, for the given layout. The capsule seek scrub is a
/// separate row above, so this concerns only the content row
/// `[metadata (flexible) | transports | divider | volume | kebab]`. The metadata
/// text is `Length::Fill` and ellipsizes; below this width even a minimal slice
/// can no longer sit beside the cluster, so the whole section hides (replaced by
/// a Fill spacer). Computed from the ACTUAL current cluster (kebab + optional
/// hamburger, the live volume orientation, current transport collapse) rather
/// than a fixed worst-case constant, so metadata stays visible as long as it
/// genuinely fits. Used ONLY by the compact regime; the three-section regime
/// renders metadata unconditionally (its Fill half is always far wider).
fn mini_player_min_width(width: f32, layout: PlayerBarLayout) -> f32 {
    let has_hamburger = theme::is_none_nav();
    let show_sfx = width >= BREAKPOINT_HIDE_SFX_SLIDER;
    // Only reserve room for the controls actually rendered. `mini_player_show_modes`
    // gates the ENTIRE mode_toggles row (inline buttons, kebab, AND the
    // NavLayout::None hamburger), so zero the whole mode-section term when off.
    let show_modes = theme::mini_player_show_modes();
    let show_volume = theme::mini_player_show_volume();
    let mode_w = if show_modes {
        mode_section_width(layout, has_hamburger)
    } else {
        0.0
    };
    let volume_w = if show_volume {
        volume_section_width(show_sfx)
    } else {
        0.0
    };
    let cluster = mode_w + volume_w + transport_section_width();
    // Approx inter-section gaps for the compact content row
    // [metadata | transports | (divider) | volume | kebab]; a heuristic kept at 3
    // to match the original threshold tuning (not a per-element exact count).
    let gaps = 3.0 * MAIN_ROW_INNER_GAP;
    // Heuristic edge inset — a proxy for the conventional padding, NOT the real
    // mini outer_padding ({ right: SECTION_BUTTON_GAP, ..ZERO }); kept at the
    // original 12/6 values so the tuned hide threshold doesn't shift.
    let horizontal_padding = if theme::is_rounded_for_player() {
        2.0 * 12.0
    } else {
        2.0 * 6.0
    };
    MINI_PLAYER_MIN_METADATA_WIDTH + cluster + gaps + horizontal_padding
}

/// Whether the mini-player left-of-cluster section should render for the active
/// `TrackInfoDisplay`, window width, and layout. Hidden when the metadata + the
/// current right cluster no longer fit (see [`mini_player_min_width`]).
#[inline]
pub(crate) fn show_mini_player_section(width: f32, layout: PlayerBarLayout) -> bool {
    use nokkvi_data::types::player_settings::TrackInfoDisplay;
    theme::track_info_display() == TrackInfoDisplay::MiniPlayer
        && width >= mini_player_min_width(width, layout)
}

/// Volume change per scroll line (e.g. mouse wheel notch)
const SCROLL_VOLUME_STEP_LINES: f32 = 0.01;
/// Volume change per scroll pixel (e.g. trackpad smooth scrolling)
const SCROLL_VOLUME_STEP_PIXELS: f32 = 0.001;

/// Base player-bar height: 72 px in both modes (see
/// `BASE_PLAYER_BAR_HEIGHT` rationale). Kept as a function so future
/// mode-conditional changes don't need to chase call sites.
#[inline]
fn base_player_bar_height() -> f32 {
    BASE_PLAYER_BAR_HEIGHT
}

/// Dynamic player bar height. `PlayerBar` strip mode adds the strip chrome
/// above/below the base row. `MiniPlayer` mode is TALLER than the base bar
/// ([`MINI_PLAYER_BAR_HEIGHT`] = 78) — the capsule seek scrub is its own row
/// (framed by 1 px separators) above the artwork-sized content row. This is
/// regime-independent: both the wide three-section and the compact layouts are
/// horizontal-only variations of the same 78 px bar. Every other mode uses the
/// base 72 px.
///
/// When the strip is on, the rendered widget tree is:
/// `column![top_separator(1), main_row(base), strip(STRIP_HEIGHT_WITH_SEPARATOR)]`.
/// `STRIP_HEIGHT_WITH_SEPARATOR` already accounts for the strip's own
/// 1 px separator-above, so the chrome math here must add 1 more px for
/// the player-bar's own top separator.
pub(crate) fn player_bar_height() -> f32 {
    use nokkvi_data::types::player_settings::TrackInfoDisplay;
    let base = base_player_bar_height();
    if crate::theme::show_player_bar_strip() {
        base + 1.0 + INFO_STRIP_WITH_SEPARATOR
    } else if theme::track_info_display() == TrackInfoDisplay::MiniPlayer {
        MINI_PLAYER_BAR_HEIGHT
    } else {
        base
    }
}

// SFX volume slider has its own breakpoint (independent of mode-toggle tier
// because the slider is wider than a button).
const BREAKPOINT_HIDE_SFX_SLIDER: f32 = 840.0;

/// One of the mode toggles the player bar exposes. Used to tag a mode
/// for cull-priority and in-kebab queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModeId {
    Lyrics,
    Visualizer,
    Crossfade,
    BitPerfect,
    Sfx,
    Eq,
    Consume,
    Shuffle,
    Repeat,
}

/// Per-render presentation of one mode toggle: dynamic icon + the two label
/// strings (full tooltip, terse kebab label) + the message its press emits.
/// Single source of truth for the strings that were previously spelled twice
/// (the inline tooltip block and the kebab-label block). Widget-type
/// (icon vs text toggle for EQ/SFX), enabled-policy, the SFX `sound_effects_enabled`
/// gate, and the three orderings (CULL_ORDER / inline / kebab) stay at the
/// render sites — the descriptor owns only icon + the two strings + message.
struct ModeDescriptor {
    /// SVG path for the 5 icon-based modes; `None` for EQ/SFX (text toggles).
    icon: Option<&'static str>,
    tooltip: &'static str,
    kebab_label: &'static str,
    message: PlayerBarMessage,
}

/// Build the per-render descriptor for one mode from the current view state.
/// Labels/icons are runtime-derived and message ctors are unit variants, so
/// the descriptor is constructed fresh per render rather than from a const
/// table. Note: the `active`/enabled args at the render sites (repeat_active,
/// is_random_mode, mode_controls_enabled, the SFX gate) are NOT owned here —
/// they are the enabled/active-policy surface the audit deliberately scopes
/// out and stay render-side.
fn mode_descriptor(mode: ModeId, data: &PlayerBarViewData) -> ModeDescriptor {
    use nokkvi_data::types::player_settings::VisualizationMode;
    match mode {
        ModeId::Repeat => {
            let queue = data.is_repeat_queue_mode;
            let track = data.is_repeat_mode;
            ModeDescriptor {
                icon: Some(if queue {
                    "assets/icons/repeat-2.svg"
                } else {
                    "assets/icons/repeat-1.svg"
                }),
                tooltip: if queue {
                    "Repeat Queue: Restart queue when it ends"
                } else if track {
                    "Repeat Track: Loop the current track"
                } else {
                    "Repeat: Off"
                },
                kebab_label: if queue {
                    "Repeat: Queue"
                } else if track {
                    "Repeat: Track"
                } else {
                    "Repeat: Off"
                },
                message: PlayerBarMessage::ToggleRepeat,
            }
        }
        ModeId::Shuffle => ModeDescriptor {
            icon: Some("assets/icons/shuffle.svg"),
            tooltip: if data.is_random_mode {
                "Shuffle: Playing in random order"
            } else {
                "Shuffle: Off"
            },
            kebab_label: if data.is_random_mode {
                "Shuffle: On"
            } else {
                "Shuffle: Off"
            },
            message: PlayerBarMessage::ToggleRandom,
        },
        ModeId::Consume => ModeDescriptor {
            icon: Some("assets/icons/cookie.svg"),
            tooltip: if data.is_consume_mode {
                "Consume: Tracks removed from queue after playing"
            } else {
                "Consume: Off"
            },
            kebab_label: if data.is_consume_mode {
                "Consume: On"
            } else {
                "Consume: Off"
            },
            message: PlayerBarMessage::ToggleConsume,
        },
        ModeId::Eq => ModeDescriptor {
            // Text toggle "EQ" at the render site.
            icon: None,
            tooltip: if data.eq_enabled {
                "Equalizer: Active"
            } else {
                "Equalizer: Disabled"
            },
            kebab_label: if data.eq_enabled {
                "Equalizer: On"
            } else {
                "Equalizer: Off"
            },
            message: PlayerBarMessage::ToggleEq,
        },
        ModeId::Sfx => ModeDescriptor {
            // Text toggle "SFX" at the render site.
            icon: None,
            // The inline SFX button only renders when SFX is on, so its
            // tooltip is a static "enabled" string — intentionally asymmetric
            // with the kebab label (which flips On/Off). A test pins this so a
            // future agent doesn't silently unify the two.
            tooltip: "Sound Effects: UI sounds enabled",
            kebab_label: if data.sound_effects_enabled {
                "UI Sound Effects: On"
            } else {
                "UI Sound Effects: Off"
            },
            message: PlayerBarMessage::ToggleSoundEffects,
        },
        ModeId::Crossfade => ModeDescriptor {
            icon: Some("assets/icons/blend.svg"),
            tooltip: if data.crossfade_enabled {
                "Crossfade: Active — overlaps every track (turns off Bit-Perfect)"
            } else {
                "Crossfade: Off — turning it on switches off Bit-Perfect"
            },
            kebab_label: if data.crossfade_enabled {
                "Crossfade: On"
            } else {
                "Crossfade: Off"
            },
            message: PlayerBarMessage::ToggleCrossfade,
        },
        ModeId::BitPerfect => {
            use nokkvi_data::types::player_settings::BitPerfectMode;
            ModeDescriptor {
                // Relaxed gets its own icon; Off/Strict share `binary` (Off is
                // dimmed via the inactive flag at the render site, like Repeat).
                icon: Some(match data.bit_perfect_mode {
                    BitPerfectMode::Relaxed => "assets/icons/combine.svg",
                    BitPerfectMode::Strict | BitPerfectMode::Off => "assets/icons/binary.svg",
                }),
                tooltip: match data.bit_perfect_mode {
                    BitPerfectMode::Off => "Bit-Perfect: Off",
                    BitPerfectMode::Strict => {
                        "Bit-Perfect: Strict — untouched, hard-cut between tracks (turns off Crossfade)"
                    }
                    BitPerfectMode::Relaxed => {
                        "Bit-Perfect: Relaxed — untouched, crossfades same-rate tracks (turns off Crossfade)"
                    }
                },
                kebab_label: match data.bit_perfect_mode {
                    BitPerfectMode::Off => "Bit-Perfect: Off",
                    BitPerfectMode::Strict => "Bit-Perfect: Strict",
                    BitPerfectMode::Relaxed => "Bit-Perfect: Relaxed",
                },
                message: PlayerBarMessage::ToggleBitPerfect,
            }
        }
        ModeId::Lyrics => ModeDescriptor {
            icon: Some("assets/icons/captions.svg"),
            tooltip: if data.lyrics_enabled {
                "Lyrics: On"
            } else {
                "Lyrics: Off"
            },
            kebab_label: if data.lyrics_enabled {
                "Lyrics: On"
            } else {
                "Lyrics: Off"
            },
            message: PlayerBarMessage::ToggleLyrics,
        },
        ModeId::Visualizer => ModeDescriptor {
            icon: Some(match data.visualization_mode {
                VisualizationMode::Lines => "assets/icons/audio-waveform.svg",
                VisualizationMode::Scope => "assets/icons/radar.svg",
                VisualizationMode::Bars | VisualizationMode::Off => "assets/icons/audio-lines.svg",
            }),
            tooltip: match data.visualization_mode {
                VisualizationMode::Off => "Visualizer: Off",
                VisualizationMode::Lines => "Visualizer: Waveform",
                VisualizationMode::Bars => "Visualizer: Bars",
                VisualizationMode::Scope => "Visualizer: Scope",
            },
            // Equals the tooltip verbatim today, but kept as a separate field
            // so a future divergence has a home — do not collapse to one.
            kebab_label: match data.visualization_mode {
                VisualizationMode::Off => "Visualizer: Off",
                VisualizationMode::Lines => "Visualizer: Waveform",
                VisualizationMode::Bars => "Visualizer: Bars",
                VisualizationMode::Scope => "Visualizer: Scope",
            },
            message: PlayerBarMessage::CycleVisualization,
        },
    }
}

/// Cull priority — index `i` is the i-th mode to fold into the kebab as the
/// window narrows. Ordered to match the inline row's right-to-left disappear
/// (rightmost-first) so gaps close cleanly from the right edge.
pub(crate) const CULL_ORDER: [ModeId; 9] = [
    ModeId::Lyrics,
    ModeId::Visualizer,
    ModeId::Crossfade,
    ModeId::BitPerfect,
    ModeId::Sfx,
    ModeId::Eq,
    ModeId::Consume,
    ModeId::Shuffle,
    ModeId::Repeat,
];

/// Width below which the mode at `CULL_ORDER[i]` folds into the kebab.
/// Hysteresis on the way back out: a culled mode pops back inline only once
/// width ≥ this threshold + `CULL_HYSTERESIS_PX`, preventing drag-resize
/// flicker at the boundary.
pub(crate) const CULL_ENTER_WIDTHS: [f32; 9] = [
    1130.0, // Lyrics
    1070.0, // Visualizer
    1010.0, // Crossfade
    980.0,  // Bit-Perfect
    950.0,  // SFX
    890.0,  // EQ
    830.0,  // Consume
    750.0,  // Shuffle
    670.0,  // Repeat
];

// Interlock: `CULL_ORDER` and `CULL_ENTER_WIDTHS` are coupled purely by index
// (CULL_ORDER[i] culls at CULL_ENTER_WIDTHS[i]). Adding a mode to one array but
// not the other would silently desync the wide-regime cull (caps at the shorter
// len) from the compact force-fold — fail the build instead.
const _: () = assert!(CULL_ORDER.len() == CULL_ENTER_WIDTHS.len());

pub(crate) const CULL_HYSTERESIS_PX: f32 = 40.0;

/// Width thresholds for the MiniPlayer three-section content row — a roomier
/// layout (metadata left, transports CENTERED, modes + volume right) where the
/// mode toggles are EXPANDED and width-cull into the kebab one-by-one exactly
/// like the normal bar (the wide regime passes the width-driven `kebab_mode_count`
/// straight through — see [`effective_player_bar_layout`]). Below the band the bar
/// falls back to the compact single-cluster row (every mode in one permanent
/// kebab — the original MiniPlayer look) only when the window is genuinely tight.
///
/// The band sits BELOW the mode-cull range (`CULL_ENTER_WIDTHS`, 670..1070) on
/// purpose: that way the whole one-mode-at-a-time cull sequence happens WHILE the
/// transports stay centered, instead of the regime only existing above the cull
/// band (which would defeat the point of expanding the modes). The compact flip
/// is the floor where centered transports + a metadata slice + the (by now mostly
/// culled) right cluster stop sitting comfortably.
///
/// Hysteretic: flip to three-section only once width clears the higher `EXIT`,
/// fall back to compact once width drops below the lower `ENTER`, holding the
/// regime across the band so drag-resize doesn't thrash the whole layout.
pub(crate) const MINI_THREE_SECTION_ENTER: f32 = 720.0;
pub(crate) const MINI_THREE_SECTION_EXIT: f32 = 770.0;

// Interlocks (compile-time desync guards): the band is a sane enter/exit pair,
// and sits below the first cull threshold so the mode toggles cull one-by-one
// WITHIN the centered three-section regime rather than the regime only existing
// above the cull range.
const _: () = assert!(MINI_THREE_SECTION_ENTER < MINI_THREE_SECTION_EXIT);
const _: () = assert!(MINI_THREE_SECTION_EXIT < CULL_ENTER_WIDTHS[0]);

/// Snapshot of how the player bar should currently lay out, derived from the
/// window width with hysteresis applied per-mode. Replaces the previous
/// 3-stage tier enum so that mode toggles cull one at a time as the window
/// shrinks instead of in 2–3-mode batches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlayerBarLayout {
    /// Number of modes folded into the kebab menu. The first `kebab_mode_count`
    /// entries of [`CULL_ORDER`] are inside the menu; the rest render inline.
    /// `0` means the kebab itself is hidden.
    pub kebab_mode_count: u8,
    /// `true` when the window is wide enough for the MiniPlayer three-section
    /// content row (metadata | centered transports | modes + volume). Width-only
    /// and hysteretic (see [`update_three_section`]); mode-agnostic, so
    /// [`compute_layout`] never reads `track_info_display`. Consumed only by the
    /// MiniPlayer render path: [`effective_player_bar_layout`] reads it to decide
    /// whether to pass the width-driven `kebab_mode_count` through (wide, modes
    /// expand/cull like the normal bar) or force every mode into one kebab
    /// (compact), and the render arm reads it to pick the three-section vs compact
    /// subtree. NOT an input to `player_bar_height()` — the bar is 78 px in both
    /// regimes (a horizontal-only change), so chrome math stays regime-blind.
    pub wide_for_three_section: bool,
}

impl PlayerBarLayout {
    /// Whether the given mode is currently folded into the kebab menu.
    pub(crate) fn is_in_kebab(self, mode: ModeId) -> bool {
        let n = self.kebab_mode_count as usize;
        CULL_ORDER.iter().take(n).any(|m| *m == mode)
    }
}

/// Recompute the layout for a new window width given the previous layout
/// (for hysteresis). Each mode has its own enter/exit threshold pair, and the
/// three-section regime has its own threshold pair, all width-driven.
pub(crate) fn compute_layout(width: f32, prev: PlayerBarLayout) -> PlayerBarLayout {
    PlayerBarLayout {
        kebab_mode_count: update_kebab_count(width, prev.kebab_mode_count),
        wide_for_three_section: update_three_section(width, prev.wide_for_three_section),
    }
}

fn update_kebab_count(width: f32, prev_count: u8) -> u8 {
    let mut count = prev_count.min(CULL_ORDER.len() as u8);

    // Pop modes back inline (width going up) — only when width clears the
    // hysteresis-shifted exit threshold for the most recently culled mode.
    while count > 0 {
        let idx = (count - 1) as usize;
        if width >= CULL_ENTER_WIDTHS[idx] + CULL_HYSTERESIS_PX {
            count -= 1;
        } else {
            break;
        }
    }

    // Push modes into kebab (width going down) — when width drops below the
    // next-to-cull mode's enter threshold.
    while (count as usize) < CULL_ENTER_WIDTHS.len() {
        let idx = count as usize;
        if width < CULL_ENTER_WIDTHS[idx] {
            count += 1;
        } else {
            break;
        }
    }

    count
}

/// Hysteretic width gate for the MiniPlayer three-section content row: flip ON
/// (three-section) only once width clears the higher `EXIT`, and hold until width
/// drops below the lower `ENTER`, so the layout doesn't thrash across the band
/// during drag-resize.
fn update_three_section(width: f32, prev: bool) -> bool {
    if prev {
        // Already three-section — stay until width drops below the drop point.
        width >= MINI_THREE_SECTION_ENTER
    } else {
        // Compact — flip to three-section only once width clears the entry point.
        width >= MINI_THREE_SECTION_EXIT
    }
}

/// Resolve the layout actually used for rendering from the width-driven base
/// layout produced by [`compute_layout`]. Width culling owns
/// `Nokkvi.player_bar_layout` for every mode. `MiniPlayer` keys off the
/// width-driven `wide_for_three_section` regime flag: in the WIDE regime the
/// mode toggles expand and cull individually (pass the width-driven
/// `kebab_mode_count` through, exactly like the normal bar); in the COMPACT
/// regime every mode is folded into one permanent kebab (the Gelly compact bar).
/// The regime flag itself is width-driven + hysteretic and is passed through
/// untouched — the render arm depends on its true value to pick the subtree.
/// Applied at the view-data construction site so `compute_layout` stays
/// width-only and mode-agnostic (and is not clobbered every resize).
pub(crate) fn effective_player_bar_layout(base: PlayerBarLayout) -> PlayerBarLayout {
    use nokkvi_data::types::player_settings::TrackInfoDisplay;
    if theme::track_info_display() == TrackInfoDisplay::MiniPlayer {
        PlayerBarLayout {
            kebab_mode_count: if base.wide_for_three_section {
                base.kebab_mode_count
            } else {
                CULL_ORDER.len() as u8
            },
            wide_for_three_section: base.wide_for_three_section,
        }
    } else {
        base
    }
}

/// Pure view data passed from root (no direct VM access)
#[derive(Debug, Clone)]
pub(crate) struct PlayerBarViewData {
    pub playback_position: u32,
    pub playback_duration: u32,
    pub playback_playing: bool,
    pub playback_paused: bool, // Distinguish paused from stopped
    pub volume: f32,
    pub has_queue: bool,
    pub is_radio: bool,
    // Mode states
    pub is_random_mode: bool,
    pub is_repeat_mode: bool,
    pub is_repeat_queue_mode: bool,
    pub is_consume_mode: bool,
    pub eq_enabled: bool,
    pub sound_effects_enabled: bool,
    pub sfx_volume: f32, // 0.0-1.0 for sound effects volume
    pub crossfade_enabled: bool,
    /// Synced-lyrics overlay toggle (the live mirror of
    /// `general.lyrics_enabled`). Drives the Lyrics mode button's active state.
    pub lyrics_enabled: bool,
    /// Bit-perfect output mode (the setting: Off / Strict / Relaxed). Drives the
    /// Bit-Perfect mode toggle's icon + active state. Distinct from
    /// `bit_perfect_status` (the honest device-rate badge).
    pub bit_perfect_mode: nokkvi_data::types::player_settings::BitPerfectMode,
    pub visualization_mode: nokkvi_data::types::player_settings::VisualizationMode,
    pub window_width: f32,
    pub layout: PlayerBarLayout,
    pub is_light_mode: bool,
    // Track metadata — consumed by the `MiniPlayer` left-of-transport
    // column. `track_title` / `track_artist` / `track_album` carry the
    // current queue song; `radio_name` is `Some` when a radio stream is
    // active (artist/title then carry the ICY values).
    pub track_title: String,
    pub track_artist: String,
    pub track_album: String,
    /// Codec / sample-rate / bitrate for the current song, threaded from
    /// playback state (same source as the track info strip). The `MiniPlayer`
    /// capsule scrub tucks these into its time end-caps —
    /// `3:40 · FLAC 44.1kHz` (left) and `1411kbps · 8:00` (right) — for parity
    /// with the other strip modes. Gated on `strip_show_format_info()`.
    pub format_suffix: String,
    pub sample_rate: u32,
    pub bitrate: u32,
    /// Honest bit-perfect status — tucked into the MiniPlayer scrub cap after
    /// the codec (`… · BIT-PERFECT`).
    pub bit_perfect_status: crate::state::BitPerfectStatus,
    /// When resampled, the app holding the device (`… · RESAMPLED→96k · Zen`).
    pub bit_perfect_holder: Option<String>,
    pub radio_name: Option<String>,
    /// Album artwork for the currently playing song. Populated by
    /// `app_view.rs` from the artwork LRU (large preferred, falls back
    /// to mini). Rendered as the leading thumbnail in `MiniPlayer`
    /// mode; ignored in other modes.
    pub artwork_handle: Option<iced::widget::image::Handle>,
    /// Whether the player-bar hamburger menu is currently open (controlled state).
    pub hamburger_open: bool,
    /// Whether the player-bar kebab "modes" menu is currently open
    /// (controlled state).
    pub player_modes_open: bool,
}

/// Messages emitted by player bar interactions
#[derive(Debug, Clone)]
pub enum PlayerBarMessage {
    Play,
    Pause,
    NextTrack,
    PrevTrack,
    Seek(f32),
    VolumeChanged(f32),
    /// Discrete user-committed volume value from the music slider — drag
    /// release or wheel notch. Routed to `PlaybackMessage::VolumeCommitted`
    /// so the playback handler can force-persist past the `VolumeChanged`
    /// throttle.
    VolumeCommitted(f32),
    ToggleRandom,
    ToggleRepeat,
    ToggleConsume,
    ToggleEq,
    ToggleSoundEffects,
    SfxVolumeChanged(f32),
    CycleVisualization,
    ToggleCrossfade,
    ToggleBitPerfect,
    ToggleLyrics,
    ScrollVolume(f32),
    /// Wheel-scroll delta over the SFX slider — handler reads the
    /// current SFX volume from app state and clamps, avoiding the
    /// stale-base bug that drove the wheel-scroll fallback deletion.
    ScrollSfxVolume(f32),
    OpenSettings,
    ToggleLightMode,
    GoToQueue,
    /// Track info strip was clicked — dispatch depends on strip_click_action setting
    StripClicked,
    StripContextAction(super::context_menu::StripContextEntry),
    /// Hamburger / kebab menu open/close request — bubbled to root
    /// `Message::SetOpenMenu`.
    SetOpenMenu(Option<crate::app_message::OpenMenu>),
    About,
    Quit,
}

/// Style for a flat borderless transport button: no border, an optional accent
/// fill when the button is in its active state (play/pause toggled on), and no
/// background otherwise. Hover/press feedback is owned by the wrapping
/// `HoverOverlay` (the accent-wash helpers), matching the nav-bar convention —
/// this style encodes only active-vs-idle.
fn transport_button_style(
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    move |_theme, _status| {
        // `ui_radius_pill()` returns `0.0.into()` in flat mode.
        let radius = theme::ui_radius_pill_player();
        let background = if active {
            Some(theme::accent_bright().into())
        } else {
            None
        };
        button::Style {
            background,
            text_color: if active {
                theme::bg0_hard()
            } else {
                theme::fg0()
            },
            border: iced::Border {
                radius,
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

/// Style for a 1px-bordered mode toggle (idle = `bg0()` fill with `border()`
/// outline; active = `accent_bright()` fill + `bg0_hard()` text). Hover/press
/// feedback is owned by the wrapping `HoverOverlay` (the accent-wash helpers),
/// matching the nav-bar convention — this style encodes only active-vs-idle.
/// Rounded mode applies `ui_radius_sm()`.
fn mode_toggle_style(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    move |_theme, _status| {
        // `ui_radius_sm()` returns `0.0.into()` in flat mode.
        let radius = theme::ui_radius_sm_player();
        let (bg, fg, border_color) = if active {
            (
                theme::accent_bright(),
                theme::bg0_hard(),
                theme::accent_bright(),
            )
        } else {
            // Idle `bg0()` fill in every state; hover is the overlay's job.
            (theme::bg0(), theme::fg0(), theme::border())
        };
        button::Style {
            background: Some(bg.into()),
            text_color: fg,
            border: iced::Border {
                color: border_color,
                width: 1.0,
                radius,
            },
            ..Default::default()
        }
    }
}

/// Build a tinted SVG element sized for an inline icon button.
fn svg_icon(icon_path: &'static str, size: f32, color: Color) -> Svg<'static, Theme> {
    let svg_content = crate::embedded_svg::get_svg(icon_path);
    let handle = Handle::from_memory(svg_content.as_bytes());
    svg(handle)
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_: &Theme, _| svg::Style { color: Some(color) })
}

/// Centers a child element inside a fixed-size container with no padding/border.
///
/// Uses `align_x`/`align_y` rather than `center_x`/`center_y` because the
/// latter pair set the container's width/height to the passed `Length`,
/// silently overriding the `Length::Fixed(width)`/`Length::Fixed(height)` set
/// just above. We want a truly fixed-size container so the wrapping button
/// reports `Shrink` and doesn't stretch when placed inside a non-Shrink
/// (e.g. fixed-width section) parent.
fn fixed_centered<'a, M: 'a>(child: Element<'a, M>, width: f32, height: f32) -> Element<'a, M> {
    container(child)
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
}

/// Wrap a section child in a fixed-width, full-height, vertically-centered
/// container with the given horizontal alignment. The non-mini strip / normal
/// arms each build their `[transports | modes | volume]` triplet from this, so
/// the width-flex pattern lives in one place. A free fn (not a closure) so each
/// arm can call it with its own moved widgets without unifying borrow lifetimes.
fn fixed_section<'a>(
    child: impl Into<Element<'a, PlayerBarMessage>>,
    width: f32,
    align_x: Alignment,
) -> iced::widget::Container<'a, PlayerBarMessage> {
    container(child)
        .width(Length::Fixed(width))
        .height(Length::Fill)
        .align_x(align_x)
        .center_y(Length::Fill)
}

/// A 1 px `theme::border()`-colored hairline (used for the player-bar top
/// separator, the MiniPlayer capsule scrub separators, and the compact-cluster
/// vertical divider). Single home for the shared fill style.
fn hairline(width: Length, height: Length) -> iced::widget::Container<'static, PlayerBarMessage> {
    container(iced::widget::Space::new())
        .width(width)
        .height(height)
        .style(|_: &Theme| container::Style {
            background: Some(theme::border().into()),
            ..Default::default()
        })
}

/// Active transport-button side length — a constant 40 px in every display
/// mode (MiniPlayer keeps native sizes in its single-row right cluster).
#[inline]
pub(crate) fn transport_button_size() -> f32 {
    TRANSPORT_SIZE
}

#[inline]
fn transport_icon_size() -> f32 {
    20.0
}

/// Helper function to create a flat transport icon button (prev / play / pause
/// / stop / next), wrapped in `HoverOverlay` for the press scale feedback.
fn player_control_button(
    icon_path: &'static str,
    message: PlayerBarMessage,
    icon_color: Color,
    active: bool,
) -> Element<'static, PlayerBarMessage> {
    let size = transport_button_size();
    let icon = svg_icon(icon_path, transport_icon_size(), icon_color);
    let inner = fixed_centered(icon.into(), size, size);
    let btn = button(inner)
        .padding(0)
        .style(transport_button_style(active))
        .on_press(message);
    HoverOverlay::new(btn)
        .border_radius(theme::ui_radius_pill_player())
        .on_accent_surface(active)
        .into()
}

/// Build a flat text-labeled mode toggle (used by EQ / SFX inline buttons).
fn mode_text_toggle(
    label: &'static str,
    on_press: PlayerBarMessage,
    active: bool,
    tooltip_text: &str,
) -> Element<'static, PlayerBarMessage> {
    let label_widget = text(label)
        .size(10.0)
        .font(theme::weighted_ui_font(Weight::Bold));
    let inner = fixed_centered(label_widget.into(), mode_button_width(), MODE_BUTTON_HEIGHT);
    let btn = button(inner)
        .padding(0)
        .style(mode_toggle_style(active))
        .on_press(on_press);
    HoverOverlay::new(
        tooltip(
            btn,
            container(
                text(tooltip_text.to_owned())
                    .size(11.0)
                    .font(theme::ui_font()),
            )
            .padding(4),
            tooltip::Position::Top,
        )
        .gap(4)
        .style(theme::container_tooltip),
    )
    .border_radius(theme::ui_radius_sm_player())
    .on_accent_surface(active)
    .into()
}

/// Build a flat icon-based mode toggle (repeat / shuffle / consume / crossfade
/// / visualizer). 38×44 in flat mode, 40×44 in rounded mode.
///
/// When `enabled` is false the button renders inert: the icon dims to the
/// disabled foreground and no `on_press` is attached, so the control is
/// honestly non-interactive. Used to gate the queue-flow trio (Shuffle /
/// Repeat / Consume) during radio playback, where they would otherwise mutate
/// the dormant library queue with no audible effect.
fn mode_toggle_button<'a>(
    icon_path: &'static str,
    message: PlayerBarMessage,
    active: bool,
    label: &'a str,
    enabled: bool,
) -> Element<'a, PlayerBarMessage> {
    let icon_color = if !enabled {
        theme::fg4()
    } else if active {
        theme::bg0_hard()
    } else {
        theme::fg0()
    };
    let icon = svg_icon(icon_path, 18.0, icon_color);
    let inner = fixed_centered(icon.into(), mode_button_width(), MODE_BUTTON_HEIGHT);
    let mut btn = button(inner)
        .padding(0)
        .style(mode_toggle_style(active && enabled));
    if enabled {
        btn = btn.on_press(message);
    }
    HoverOverlay::new(
        tooltip(
            btn,
            container(text(label).size(11.0).font(theme::ui_font())).padding(4),
            tooltip::Position::Top,
        )
        .gap(4)
        .style(theme::container_tooltip),
    )
    .border_radius(theme::ui_radius_sm_player())
    .on_accent_surface(active)
    .into()
}

/// Compose the `MiniPlayer` capsule scrub's two end-cap labels, tucking the
/// codec / sample-rate / bitrate inside the elapsed / duration for parity with
/// the other strip modes. With `show_format` on and codec metadata present:
///   left  = `"{elapsed} · {CODEC kHz}"`  e.g. `"3:40 · FLAC 44.1kHz"`
///   right = `"{kbps} · {duration}"`       e.g. `"1411kbps · 8:00"`
/// Each [`CapLabel`] keeps the bare time separate from the full string so the
/// renderer can draw the codec / bitrate dimmer than the time. When format
/// display is off, the suffix is empty, or there's no bitrate, the affected cap
/// falls back to time-only (fully opaque). Pure (takes `show_format` as an
/// argument rather than reading the theme atomic) so it stays unit-testable.
// Many display inputs (time/codec/rate/bitrate/bit-perfect) — a builder for one
// strip, not a candidate for a params struct.
#[allow(clippy::too_many_arguments)]
fn capsule_scrub_labels(
    elapsed: &str,
    duration: &str,
    format_suffix: &str,
    sample_rate_khz: f32,
    bitrate_kbps: u32,
    show_format: bool,
    bit_perfect_status: crate::state::BitPerfectStatus,
    bit_perfect_holder: Option<&str>,
) -> (CapLabel, CapLabel) {
    let bare = || (CapLabel::time_only(elapsed), CapLabel::time_only(duration));
    if !show_format {
        return bare();
    }
    // Tuck the honest bit-perfect badge in after the codec, sharing the codec's
    // dim cap styling (the cap is plain text — it can't carry the accent color
    // the wider strips use). Reuses the single-sourced `bit_perfect_badge` label,
    // which already folds the holder in as "RESAMPLED→96k · Zen".
    let badge_suffix =
        match super::track_info_strip::bit_perfect_badge(bit_perfect_status, bit_perfect_holder) {
            Some((label, _color)) => format!(" · {label}"),
            None => String::new(),
        };
    match super::format_info::format_audio_info_split(format_suffix, sample_rate_khz, bitrate_kbps)
    {
        Some((codec, Some(kbps))) => (
            CapLabel::new(format!("{elapsed} · {codec}{badge_suffix}"), elapsed),
            CapLabel::new(format!("{kbps} · {duration}"), duration),
        ),
        Some((codec, None)) => (
            CapLabel::new(format!("{elapsed} · {codec}{badge_suffix}"), elapsed),
            CapLabel::time_only(duration),
        ),
        None => bare(),
    }
}

/// Build the left-of-transport artwork + 3-line metadata column rendered
/// in `TrackInfoDisplay::MiniPlayer` mode.
///
/// Layout: [56 px artwork] [8 px gap] [flexible text column with
/// `title` / `artist` / `album` stacked vertically]. The text column is
/// `Length::Fill`, so it expands into the available width and each line
/// ellipsizes (trailing ellipsis, `Wrapping::None` — no marquee scroll) when
/// its content overflows that width.
///
/// In radio mode the three slots carry `station name` / `ICY title` / `ICY artist`
/// (mapped by `app_view.rs`); the artwork slot falls back to a tinted
/// `radio-tower` glyph on `theme::bg1()` when no per-station artwork is
/// available.
///
/// The whole section is wrapped in a `mouse_area` that emits
/// `StripClicked` so the user's configured `strip_click_action` (go to
/// queue / album / artist / copy info) routes the same as a click on the
/// regular player-bar strip.
fn mini_player_section(data: &PlayerBarViewData) -> Element<'static, PlayerBarMessage> {
    let radius = theme::ui_border_radius_player();

    let artwork: Element<'static, PlayerBarMessage> =
        if let Some(handle) = data.artwork_handle.clone() {
            container(
                iced::widget::image(handle)
                    .content_fit(iced::ContentFit::Cover)
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .width(Length::Fixed(MINI_PLAYER_ARTWORK_SIZE))
            .height(Length::Fixed(MINI_PLAYER_ARTWORK_SIZE))
            .clip(true)
            .style(move |_| container::Style {
                background: Some(theme::bg2().into()),
                border: iced::Border {
                    radius,
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
        } else if data.is_radio {
            container(svg_icon(
                super::track_info_strip::RADIO_TOWER_ICON_PATH,
                MINI_PLAYER_ARTWORK_SIZE * 0.55,
                theme::fg2(),
            ))
            .width(Length::Fixed(MINI_PLAYER_ARTWORK_SIZE))
            .height(Length::Fixed(MINI_PLAYER_ARTWORK_SIZE))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(move |_| container::Style {
                background: Some(theme::bg1().into()),
                border: iced::Border {
                    radius,
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
        } else {
            container(iced::widget::Space::new())
                .width(Length::Fixed(MINI_PLAYER_ARTWORK_SIZE))
                .height(Length::Fixed(MINI_PLAYER_ARTWORK_SIZE))
                .style(move |_| container::Style {
                    background: Some(theme::bg1().into()),
                    border: iced::Border {
                        radius,
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .into()
        };

    // MiniPlayer metadata lines truncate with a trailing ellipsis (no marquee
    // scroll) — each line fills the column width and ellipsizes when it
    // overflows.
    let make_line =
        |value: String, color: Color, bold: bool| -> Element<'static, PlayerBarMessage> {
            let weight = if bold { Weight::Bold } else { Weight::Medium };
            text(value)
                .size(12.0)
                .font(theme::weighted_ui_font(weight))
                .color(color)
                .width(Length::Fill)
                .wrapping(iced::widget::text::Wrapping::None)
                .ellipsis(iced::widget::text::Ellipsis::End)
                .into()
        };

    // Slot mapping
    //   queue:  title / artist / album
    //   radio:  station / ICY title / ICY artist
    // app_view already routes ICY values through track_title / track_artist
    // for radio playback, so the only swap here is the leading station name
    // taking the title slot.
    let (line1, line2, line3) = if let Some(station) = data.radio_name.clone() {
        (station, data.track_title.clone(), data.track_artist.clone())
    } else {
        (
            data.track_title.clone(),
            data.track_artist.clone(),
            data.track_album.clone(),
        )
    };

    // Mini-player text sits on the player bar's `bg0_hard()` chrome (the bar's
    // `main_content` uses `container_bg0_hard`), so each line is made legible
    // against it. Title is the brighter fg tier, artist/album the secondary.
    let title_line = make_line(
        line1,
        theme::legible_strip_text(theme::fg2(), theme::bg0_hard()),
        true,
    );
    let artist_line = make_line(
        line2,
        theme::legible_strip_text(theme::fg3(), theme::bg0_hard()),
        false,
    );
    let album_line = make_line(
        line3,
        theme::legible_strip_text(theme::fg2(), theme::bg0_hard()),
        false,
    );

    // Elapsed / duration is shown in the capsule scrub's time end-caps (not in
    // the metadata), so the text column is just title / artist / album.
    let text_column = container(
        column![title_line, artist_line, album_line]
            .spacing(2)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fixed(MINI_PLAYER_ARTWORK_SIZE))
    .align_y(Alignment::Center);

    let inner = row![artwork, text_column]
        .spacing(MINI_PLAYER_INNER_GAP)
        .width(Length::Fill)
        .align_y(Alignment::Center);

    mouse_area(inner)
        .on_press(PlayerBarMessage::StripClicked)
        .into()
}

/// Build the player bar view.
///
/// If `info_strip` is `Some`, the player bar renders in "track display" mode:
/// controls + progress on top, info strip below (with separator).
/// If `None`, the player bar renders in normal single-row mode.
///
/// The caller (`app_view.rs`) is responsible for building the strip element
/// from `TrackInfoStripData` — the player bar doesn't know about track metadata.
pub(crate) fn player_bar<'a>(
    data: &PlayerBarViewData,
    info_strip: Option<Element<'a, PlayerBarMessage>>,
) -> Element<'a, PlayerBarMessage> {
    // Player controls with SVG icons
    let has_queue = data.has_queue && !data.is_radio;
    let controls_active = has_queue || data.is_radio;
    let playback_playing = data.playback_playing;
    let playback_paused = data.playback_paused;

    let prev_icon_color = if controls_active {
        theme::fg0()
    } else {
        theme::fg4()
    };
    let next_icon_color = prev_icon_color;
    let prev_button = player_control_button(
        "assets/icons/skip-back.svg",
        PlayerBarMessage::PrevTrack,
        prev_icon_color,
        false,
    );
    let next_button = player_control_button(
        "assets/icons/skip-forward.svg",
        PlayerBarMessage::NextTrack,
        next_icon_color,
        false,
    );

    // Modern 3-button transport in every layout: prev / play-or-pause toggle /
    // next. The middle button is a fixed 40 px so its hit target stays put when
    // the glyph swaps between play and pause. (Stop has no inline button — it
    // stays reachable via MPRIS media keys and the `nokkvi stop` CLI/IPC verb.)
    let player_controls: Element<'_, PlayerBarMessage> = {
        let middle_active = playback_playing || playback_paused;
        let (middle_icon, middle_message) = if playback_playing {
            ("assets/icons/pause.svg", PlayerBarMessage::Pause)
        } else {
            ("assets/icons/play.svg", PlayerBarMessage::Play)
        };
        let middle_icon_color = if middle_active {
            theme::bg0_hard()
        } else {
            theme::fg0()
        };
        row![
            prev_button,
            player_control_button(
                middle_icon,
                middle_message,
                middle_icon_color,
                middle_active
            ),
            next_button,
        ]
        .spacing(4)
        .into()
    };

    // Progress bar section
    let duration = data.playback_duration as f32;
    let position = data.playback_position as f32;

    let pos_str = format!(
        "{}:{:02}",
        position.floor() as u32 / 60,
        position.floor() as u32 % 60
    );

    let dur_str = if data.is_radio {
        "--:--".to_string()
    } else {
        format!(
            "{}:{:02}",
            duration.floor() as u32 / 60,
            duration.floor() as u32 % 60
        )
    };

    // The inline progress row (with time end-caps + drag handle) used by every
    // mode EXCEPT MiniPlayer. MiniPlayer instead floats a bare seek scrub as a
    // top-edge overlay (built in the assembly below). Radio is non-seekable, so
    // it hides the handle and disables interaction.
    use nokkvi_data::types::player_settings::TrackInfoDisplay;
    let is_mini_player_mode = theme::track_info_display() == TrackInfoDisplay::MiniPlayer;

    let custom_progress_bar =
        widgets::progress_bar::progress_bar(position, duration, PlayerBarMessage::Seek)
            .is_playing(data.playback_playing && !data.playback_paused)
            .hide_handle(data.is_radio)
            .interactive(!data.is_radio)
            .width(Length::Fill)
            .height(24.0);

    let progress_row = row![
        text(pos_str.clone())
            .size(11.0)
            .font(theme::ui_font())
            .color(theme::fg4())
            .width(Length::Fixed(40.0))
            .align_x(Alignment::End)
            .align_y(Alignment::Center),
        custom_progress_bar,
        text(dur_str.clone())
            .size(11.0)
            .font(theme::ui_font())
            .color(theme::fg4())
            .width(Length::Fixed(40.0))
            .align_y(Alignment::Center),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .height(Length::Fixed(CONTROL_ROW_HEIGHT))
    .width(Length::Fill);

    // Mode toggle buttons with SVG icons
    let is_random_mode = data.is_random_mode;
    let is_repeat_mode = data.is_repeat_mode;
    let is_repeat_queue_mode = data.is_repeat_queue_mode;
    let is_consume_mode = data.is_consume_mode;
    let eq_enabled = data.eq_enabled;
    let sound_effects_enabled = data.sound_effects_enabled;
    let sfx_volume = data.sfx_volume;
    let visualization_mode = data.visualization_mode;

    let repeat_active = is_repeat_mode || is_repeat_queue_mode;
    use nokkvi_data::types::player_settings::VisualizationMode;
    let vis_active = visualization_mode != VisualizationMode::Off;
    let window_width = data.window_width;

    // SFX volume slider keeps its own width-based gate (independent of the
    // mode-toggle tier — the slider is genuinely wider than a button so it
    // deserves a separate threshold).
    let show_sfx_slider = window_width >= BREAKPOINT_HIDE_SFX_SLIDER;

    // Single source of truth for each mode's icon + the two label strings
    // (full inline tooltip, terse kebab label) + the toggle message. Built
    // once per render; the inline row and kebab construction below both pull
    // from these. Widget-type, enabled-policy, the SFX gate, and the three
    // orderings stay at the render sites.
    let repeat = mode_descriptor(ModeId::Repeat, data);
    let shuffle = mode_descriptor(ModeId::Shuffle, data);
    let consume = mode_descriptor(ModeId::Consume, data);
    let eq = mode_descriptor(ModeId::Eq, data);
    let sfx = mode_descriptor(ModeId::Sfx, data);
    let crossfade = mode_descriptor(ModeId::Crossfade, data);
    let bit_perfect = mode_descriptor(ModeId::BitPerfect, data);
    let visualizer = mode_descriptor(ModeId::Visualizer, data);
    let lyrics = mode_descriptor(ModeId::Lyrics, data);

    // Per-mode kebab membership — derived once from the layout snapshot so
    // the inline row and kebab construction stay in sync.
    let layout = data.layout;
    let repeat_in_kebab = layout.is_in_kebab(ModeId::Repeat);
    let shuffle_in_kebab = layout.is_in_kebab(ModeId::Shuffle);
    let consume_in_kebab = layout.is_in_kebab(ModeId::Consume);
    let eq_in_kebab = layout.is_in_kebab(ModeId::Eq);
    let sfx_in_kebab = layout.is_in_kebab(ModeId::Sfx);
    let crossfade_in_kebab = layout.is_in_kebab(ModeId::Crossfade);
    let bit_perfect_in_kebab = layout.is_in_kebab(ModeId::BitPerfect);
    let visualizer_in_kebab = layout.is_in_kebab(ModeId::Visualizer);
    let lyrics_in_kebab = layout.is_in_kebab(ModeId::Lyrics);

    let mut mode_toggles_row = iced::widget::Row::new().spacing(4);

    // Inline mode toggles, in the historical visual order. Each mode renders
    // here only when it's NOT in the kebab. SFX has the additional gate of
    // `sound_effects_enabled` (preserves the long-standing "no SFX button
    // when SFX is off" behavior at wide widths).
    // Queue-flow mode toggles are inert during radio (no queue to act on).
    // The audio-output modes (crossfade / EQ / visualizer / SFX) stay live.
    let mode_controls_enabled = !data.is_radio;
    if !repeat_in_kebab {
        mode_toggles_row = mode_toggles_row.push(mode_toggle_button(
            repeat.icon.unwrap_or("assets/icons/repeat-1.svg"),
            repeat.message.clone(),
            repeat_active,
            repeat.tooltip,
            mode_controls_enabled,
        ));
    }
    if !shuffle_in_kebab {
        mode_toggles_row = mode_toggles_row.push(mode_toggle_button(
            shuffle.icon.unwrap_or("assets/icons/shuffle.svg"),
            shuffle.message.clone(),
            is_random_mode,
            shuffle.tooltip,
            mode_controls_enabled,
        ));
    }
    if !consume_in_kebab {
        mode_toggles_row = mode_toggles_row.push(mode_toggle_button(
            consume.icon.unwrap_or("assets/icons/cookie.svg"),
            consume.message.clone(),
            is_consume_mode,
            consume.tooltip,
            mode_controls_enabled,
        ));
    }
    if !eq_in_kebab {
        // EQ inline button — flat text-labeled toggle.
        mode_toggles_row = mode_toggles_row.push(mode_text_toggle(
            "EQ",
            eq.message.clone(),
            eq_enabled,
            eq.tooltip,
        ));
    }
    if !sfx_in_kebab && sound_effects_enabled {
        // SFX inline button — flat text-labeled toggle. Only renders when
        // SFX is on AND not yet folded into the kebab. `sfx.tooltip` is the
        // static "enabled" string by design (asymmetric with the kebab label).
        mode_toggles_row = mode_toggles_row.push(mode_text_toggle(
            "SFX",
            sfx.message.clone(),
            true,
            sfx.tooltip,
        ));
    }
    // Bit-Perfect renders to the LEFT of Crossfade inline so the row's
    // right-to-left disappear order matches CULL_ORDER (Visualizer, then
    // Crossfade, then BitPerfect) — gaps close cleanly from the right edge.
    if !bit_perfect_in_kebab {
        mode_toggles_row = mode_toggles_row.push(mode_toggle_button(
            bit_perfect.icon.unwrap_or("assets/icons/binary.svg"),
            bit_perfect.message.clone(),
            data.bit_perfect_mode != nokkvi_data::types::player_settings::BitPerfectMode::Off,
            bit_perfect.tooltip,
            true,
        ));
    }
    if !crossfade_in_kebab {
        mode_toggles_row = mode_toggles_row.push(mode_toggle_button(
            crossfade.icon.unwrap_or("assets/icons/blend.svg"),
            crossfade.message.clone(),
            data.crossfade_enabled,
            crossfade.tooltip,
            true,
        ));
    }
    if !visualizer_in_kebab {
        mode_toggles_row = mode_toggles_row.push(mode_toggle_button(
            visualizer.icon.unwrap_or("assets/icons/audio-lines.svg"),
            visualizer.message.clone(),
            vis_active,
            visualizer.tooltip,
            true,
        ));
    }
    // Rightmost inline mode (CULL_ORDER[0] — first to fold as the window
    // narrows). Queue-only surface, so inert during radio like the queue-flow
    // group.
    if !lyrics_in_kebab {
        mode_toggles_row = mode_toggles_row.push(mode_toggle_button(
            lyrics.icon.unwrap_or("assets/icons/captions.svg"),
            lyrics.message.clone(),
            data.lyrics_enabled,
            lyrics.tooltip,
            mode_controls_enabled,
        ));
    }

    // Kebab menu — built only when at least one mode has folded in. Rows
    // render in the user-specified display order: queue-flow group first
    // [Shuffle, Repeat, Consume], then audio-output group [Crossfade, EQ,
    // Visualizer, SFX]. The separator between groups appears only when both
    // groups have at least one item (so it doesn't dangle as the kebab
    // fills up gradually).
    if layout.kebab_mode_count > 0 {
        use crate::widgets::player_modes_menu::{
            PlayerModesMenu, mode_menu_item, mode_menu_separator,
        };
        let queue_group_has_items = shuffle_in_kebab || repeat_in_kebab || consume_in_kebab;
        let audio_group_has_items = crossfade_in_kebab
            || bit_perfect_in_kebab
            || eq_in_kebab
            || visualizer_in_kebab
            || sfx_in_kebab;

        let mut kebab_rows = Vec::with_capacity(layout.kebab_mode_count as usize + 1);
        if shuffle_in_kebab {
            kebab_rows.push(mode_menu_item(
                shuffle.kebab_label,
                is_random_mode,
                shuffle.message.clone(),
            ));
        }
        if repeat_in_kebab {
            kebab_rows.push(mode_menu_item(
                repeat.kebab_label,
                repeat_active,
                repeat.message.clone(),
            ));
        }
        if consume_in_kebab {
            kebab_rows.push(mode_menu_item(
                consume.kebab_label,
                is_consume_mode,
                consume.message.clone(),
            ));
        }
        if queue_group_has_items && audio_group_has_items {
            kebab_rows.push(mode_menu_separator());
        }
        if crossfade_in_kebab {
            kebab_rows.push(mode_menu_item(
                crossfade.kebab_label,
                data.crossfade_enabled,
                crossfade.message.clone(),
            ));
        }
        if bit_perfect_in_kebab {
            kebab_rows.push(mode_menu_item(
                bit_perfect.kebab_label,
                data.bit_perfect_mode != nokkvi_data::types::player_settings::BitPerfectMode::Off,
                bit_perfect.message.clone(),
            ));
        }
        if eq_in_kebab {
            kebab_rows.push(mode_menu_item(
                eq.kebab_label,
                eq_enabled,
                eq.message.clone(),
            ));
        }
        if visualizer_in_kebab {
            kebab_rows.push(mode_menu_item(
                visualizer.kebab_label,
                vis_active,
                visualizer.message.clone(),
            ));
        }
        if sfx_in_kebab {
            kebab_rows.push(mode_menu_item(
                sfx.kebab_label,
                sound_effects_enabled,
                sfx.message.clone(),
            ));
        }
        if lyrics_in_kebab {
            kebab_rows.push(mode_menu_item(
                lyrics.kebab_label,
                data.lyrics_enabled,
                lyrics.message.clone(),
            ));
        }

        mode_toggles_row = mode_toggles_row.push(Element::from(
            HoverOverlay::new(PlayerModesMenu::new(
                kebab_rows,
                |open| {
                    PlayerBarMessage::SetOpenMenu(
                        open.then_some(crate::app_message::OpenMenu::PlayerModes),
                    )
                },
                data.player_modes_open,
            ))
            .border_radius(theme::ui_radius_sm_player()),
        ));
    }

    // Application menu — only visible in NavLayout::None (no nav chrome of
    // any kind). Top has the hamburger in the top nav bar; Side has it in
    // the side nav column.
    if crate::theme::is_none_nav() {
        use crate::widgets::hamburger_menu::{HamburgerMenu, MenuAction};
        let is_light = data.is_light_mode;
        let hamburger_open = data.hamburger_open;
        let hamburger = HamburgerMenu::new(
            |action| match action {
                MenuAction::ToggleLightMode => PlayerBarMessage::ToggleLightMode,
                MenuAction::OpenSettings => PlayerBarMessage::OpenSettings,
                MenuAction::About => PlayerBarMessage::About,
                MenuAction::Quit => PlayerBarMessage::Quit,
            },
            |open| {
                PlayerBarMessage::SetOpenMenu(
                    open.then_some(crate::app_message::OpenMenu::Hamburger),
                )
            },
            hamburger_open,
            is_light,
        )
        .player_bar_style();
        mode_toggles_row = mode_toggles_row.push(Element::from(
            HoverOverlay::new(hamburger).border_radius(theme::ui_radius_sm_player()),
        ));
    }

    let mode_toggles = mode_toggles_row;

    // Volume control - horizontal layout with conditional sfx visibility
    // SFX slider is also hidden at narrow widths (show_sfx_slider flag).
    // Hover percentage was removed: every volume change now emits a unified
    // toast (see handle_volume_changed / handle_sfx_volume_changed).
    let volume = data.volume;

    let is_horizontal = crate::theme::is_horizontal_volume();
    let stacked = is_horizontal && sound_effects_enabled && show_sfx_slider;
    // When both horizontal sliders stack, size each so combined height matches buttons.
    let stacked_spacing = 4.0;
    let stacked_thickness = 19.0;

    let mut vol = widgets::volume_slider(volume, PlayerBarMessage::VolumeChanged)
        .on_release(PlayerBarMessage::VolumeCommitted)
        .on_scroll(PlayerBarMessage::ScrollVolume)
        .horizontal(is_horizontal);
    if is_horizontal {
        vol = vol.thickness(stacked_thickness);
    }
    let vol_slider: Element<'_, PlayerBarMessage> = vol.into();

    let mut sfx = widgets::volume_slider(sfx_volume, PlayerBarMessage::SfxVolumeChanged)
        .variant(widgets::SliderVariant::Sfx)
        .on_scroll(PlayerBarMessage::ScrollSfxVolume)
        .horizontal(is_horizontal);
    if stacked {
        sfx = sfx.thickness(stacked_thickness);
    }
    let sfx_slider: Element<'_, PlayerBarMessage> = sfx.into();

    let volume_control: Element<'_, PlayerBarMessage> = if is_horizontal {
        // Horizontal mode: stack sliders vertically (SFX on top, volume below),
        // wrapped in a centering container so they sit mid-height in the bar.
        let stacked_el: Element<'_, PlayerBarMessage> = if stacked {
            column![sfx_slider, vol_slider]
                .spacing(stacked_spacing)
                .align_x(Alignment::Center)
                .into()
        } else {
            column![vol_slider].align_x(Alignment::Center).into()
        };
        container(stacked_el)
            .height(Length::Fill)
            .center_y(Length::Fill)
            .into()
    } else {
        // Vertical mode (default): side-by-side in a row
        if sound_effects_enabled && show_sfx_slider {
            row![vol_slider, sfx_slider]
                .spacing(4)
                .align_y(Alignment::Center)
                .into()
        } else {
            row![vol_slider]
                .spacing(4)
                .align_y(Alignment::Center)
                .into()
        }
    };

    // =========================================================================
    // Layout: choose between normal and track-display mode
    // =========================================================================

    let base_height = base_player_bar_height();
    // MiniPlayer runs its CONTENT ROW nearly flush to the window edges (the
    // capsule scrub + separators live outside this padding, in the outer column,
    // so they stay full-width edge-to-edge). The artwork sits hard against the
    // left edge; a small right gap (= the inter-button gap) keeps the rightmost
    // control (volume) off the window edge. The other display modes keep their
    // conventional symmetric inset (rounded floats content inward to clear the
    // corners; flat keeps a small margin).
    let outer_padding: iced::Padding = if is_mini_player_mode {
        iced::Padding {
            right: SECTION_BUTTON_GAP,
            ..iced::Padding::ZERO
        }
    } else if theme::is_rounded_for_player() {
        iced::Padding::from([10u16, 12u16])
    } else {
        iced::Padding::from([0u16, 6u16])
    };

    // The non-mini single-row arms wrap each section in a Length::Fixed
    // container so the progress track flexes into the remainder; those wrappers
    // are built inside each arm below. MiniPlayer instead consumes the raw
    // `mode_toggles`, `player_controls`, and `volume_control` widgets directly —
    // the is_mini_player_mode arm below builds either the three-section row
    // [metadata | centered transports | modes+volume] or the compact
    // single-cluster row from them (no Length::Fixed section wrappers).
    let has_hamburger = crate::theme::is_none_nav();

    let mut main_row = iced::widget::Row::new()
        .spacing(MAIN_ROW_INNER_GAP)
        .padding(outer_padding)
        .align_y(Alignment::Center);

    // Pre-compute the non-mini section widths; the wrappers themselves are
    // built inside the strip / normal arms (a shared closure can't unify the
    // borrowed widget lifetimes, and the MiniPlayer arm needs the raw widgets).
    let transport_section_w = transport_section_width();
    let mode_section_w = mode_section_width(data.layout, has_hamburger);
    let volume_section_w = volume_section_width(show_sfx_slider);

    let main_content: Element<'_, PlayerBarMessage> = if is_mini_player_mode {
        // --- MINIPLAYER MODE ---
        // A capsule seek scrub rides the top (full-width filled progress bar with
        // color-aware elapsed/duration + dimmed codec/bitrate end-caps, no handle
        // — click/drag still seeks), framed by 1 px separators, directly above a
        // content row whose SHAPE depends on the width-driven regime flag
        // `data.layout.wide_for_three_section`:
        //   WIDE    — three-section [ metadata (Fill, Start) | transports
        //             (CENTER) | modes + volume (Fill, End) ] with the modes
        //             EXPANDED and width-culling individually like the normal bar.
        //   COMPACT — the single trailing cluster [ metadata (or Space) |
        //             transports | divider | volume | kebab ] with every mode
        //             folded into one kebab (the Gelly compact bar).
        // Both subtrees feed the SAME outer column so the capsule's drag state +
        // seek tooltip survive a mid-drag regime flip. The capsule's end-caps
        // tuck the codec + bitrate inside (e.g. `3:40 · FLAC 44.1kHz` /
        // `1411kbps · 8:00`) for parity with the other strip modes — see
        // `capsule_scrub_labels`.
        let (cap_left, cap_right) = capsule_scrub_labels(
            &pos_str,
            &dur_str,
            &data.format_suffix,
            data.sample_rate as f32 / 1000.0,
            data.bitrate,
            theme::strip_show_format_info(),
            data.bit_perfect_status,
            data.bit_perfect_holder.as_deref(),
        );
        let capscrub =
            widgets::progress_bar::progress_bar(position, duration, PlayerBarMessage::Seek)
                .is_playing(data.playback_playing && !data.playback_paused)
                // filled (capsule) mode is inherently handle-less; no need to
                // also pass hide_handle. Radio is non-seekable.
                .filled(true)
                .time_labels(cap_left, cap_right)
                .interactive(!data.is_radio)
                .width(Length::Fill)
                .height(CAPSULE_SCRUB_HEIGHT);

        // Transports are shared by both regimes: a raw container with NO width
        // wrapper so it sizes to its content. In the WIDE regime the two equal
        // Fill siblings on either side center it; in COMPACT it leads the cluster.
        let transports_section = container(player_controls)
            .height(Length::Fill)
            .center_y(Length::Fill);

        if data.layout.wide_for_three_section {
            // --- WIDE three-section row ---
            // Metadata always renders here: even at the >= 770 px entry width its
            // Fill half (~315 px) dwarfs MINI_PLAYER_MIN_METADATA_WIDTH (114), so
            // it never needs the compact regime's `show_mini_player_section` gate
            // (which would risk a Fill-half collapse that lurches the centered
            // transports).
            main_row = main_row.push(
                container(mini_player_section(data))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::Start)
                    .center_y(Length::Fill),
            );
            main_row = main_row.push(transports_section);
            let show_modes = theme::mini_player_show_modes();
            let show_volume = theme::mini_player_show_volume();
            if show_modes || show_volume {
                // Right cluster: modes then volume (volume pinned far right,
                // mirroring normal mode), each shown per its own setting. The
                // wrapper MUST be Length::Fill, NOT Length::Fixed — the two equal
                // Fill siblings are what center the transports; a Fixed wrapper
                // would shift the center as modes cull. No divider here (the
                // centered Fill gaps are the seam).
                let mut cluster = iced::widget::Row::new()
                    .spacing(MAIN_ROW_INNER_GAP)
                    .align_y(Alignment::Center);
                if show_modes {
                    cluster = cluster.push(mode_toggles);
                }
                if show_volume {
                    cluster = cluster.push(volume_control);
                }
                main_row = main_row.push(
                    container(cluster)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .align_x(Alignment::End)
                        .center_y(Length::Fill),
                );
            } else {
                // Both controls hidden: a Fill placeholder holds the right half so
                // the lone metadata Fill doesn't shove the transports off-center.
                main_row = main_row.push(iced::widget::Space::new().width(Length::Fill));
            }
        } else {
            // --- COMPACT single-cluster row (the current MiniPlayer look) ---
            let divider = container(hairline(
                Length::Fixed(1.0),
                Length::Fixed(MINI_DIVIDER_HEIGHT),
            ))
            .height(Length::Fill)
            .center_y(Length::Fill);
            let volume_section = container(volume_control)
                .height(Length::Fill)
                .center_y(Length::Fill);

            // Metadata shows only when it + the right cluster fit (each line
            // ellipsizes; below this it's replaced by a Fill spacer). Computed
            // here in the compact branch only — the wide regime renders metadata
            // unconditionally.
            let mini_player_visible = show_mini_player_section(data.window_width, data.layout);
            if mini_player_visible {
                main_row = main_row.push(
                    container(mini_player_section(data))
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .align_x(Alignment::Start)
                        .center_y(Length::Fill),
                );
            } else {
                main_row = main_row.push(iced::widget::Space::new().width(Length::Fill));
            }
            main_row = main_row.push(transports_section);
            // The mode menu and volume each show per their own setting. Order is
            // [divider | kebab | volume] so the volume control is the rightmost
            // control in BOTH regimes (matches the wide [modes | volume] cluster),
            // instead of swapping past the kebab when the layout collapses. The
            // divider shows only when at least one trailing control is visible.
            let show_modes = theme::mini_player_show_modes();
            let show_volume = theme::mini_player_show_volume();
            if show_modes || show_volume {
                main_row = main_row.push(divider);
                if show_modes {
                    main_row = main_row.push(mode_toggles);
                }
                if show_volume {
                    main_row = main_row.push(volume_section);
                }
            }
        }
        main_row = main_row.height(Length::Fill);

        // 1 px separator lines framing the capsule scrub top and bottom, in the
        // app's divider color — matching the separator language elsewhere.
        let scrub_separator = || hairline(Length::Fill, Length::Fixed(SCRUB_SEPARATOR_HEIGHT));

        column![scrub_separator(), capscrub, scrub_separator(), main_row]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else if let Some(strip) = info_strip {
        // --- TRACK DISPLAY (PlayerBar strip) MODE ---
        // Main row on top, info strip below (separator built into the
        // strip). `player_bar_height()` reserves
        // `base + 1 + STRIP_HEIGHT_WITH_SEPARATOR`, the outer column
        // consumes the leading 1 px for `top_separator`, and the strip
        // accounts for its own separator-above — so this row gets the
        // bare `base_height`.
        let transports_section =
            fixed_section(player_controls, transport_section_w, Alignment::Start);
        let mode_toggles = fixed_section(mode_toggles, mode_section_w, Alignment::End);
        let volume_control = fixed_section(volume_control, volume_section_w, Alignment::End);
        main_row = main_row
            .push(transports_section)
            .push(progress_row)
            .push(mode_toggles)
            .push(volume_control);
        column![
            container(main_row)
                .width(Length::Fill)
                .height(Length::Fixed(base_height))
                .center_y(Length::Fill),
            strip,
        ]
        .into()
    } else {
        // --- NORMAL MODE ---
        let transports_section =
            fixed_section(player_controls, transport_section_w, Alignment::Start);
        let mode_toggles = fixed_section(mode_toggles, mode_section_w, Alignment::End);
        let volume_control = fixed_section(volume_control, volume_section_w, Alignment::End);
        main_row
            .push(transports_section)
            .push(progress_row)
            .push(mode_toggles)
            .push(volume_control)
            .into()
    };

    // Bar body. Non-mini modes draw a 1 px top separator (the chrome divider
    // between the page content and the bar) over the centered content row.
    // MiniPlayer drops the separator — its capsule scrub already caps the top
    // edge — and lets its `column![capscrub, content_row]` fill the bar.
    let bar_body: Element<'_, PlayerBarMessage> = if is_mini_player_mode {
        container(main_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::container_bg0_hard)
            .into()
    } else {
        let top_separator: Element<'_, PlayerBarMessage> =
            hairline(Length::Fill, Length::Fixed(1.0)).into();
        column![
            top_separator,
            container(main_content)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_y(Length::Fill)
                .style(theme::container_bg0_hard),
        ]
        .into()
    };

    // Wrapped in a mouse_area so scrolling anywhere on the bar adjusts volume.
    let bar = container(bar_body).height(Length::Fixed(player_bar_height()));

    mouse_area(bar)
        .on_scroll(|delta| {
            let y = match delta {
                ScrollDelta::Lines { y, .. } => y * SCROLL_VOLUME_STEP_LINES,
                ScrollDelta::Pixels { y, .. } => y * SCROLL_VOLUME_STEP_PIXELS,
            };
            PlayerBarMessage::ScrollVolume(y)
        })
        .into()
}

#[cfg(test)]
mod player_bar_height_tests {
    use nokkvi_data::types::player_settings::TrackInfoDisplay;

    use super::{
        super::track_info_strip::STRIP_HEIGHT_WITH_SEPARATOR, BASE_PLAYER_BAR_HEIGHT,
        player_bar_height,
    };
    use crate::theme::{THEME_MODE_LOCK, set_track_info_display, track_info_display};

    /// Player bar with no strip reports just the base 72 px footprint.
    #[test]
    fn player_bar_height_without_strip_is_base() {
        let _guard = THEME_MODE_LOCK.lock();
        let saved = track_info_display();
        set_track_info_display(TrackInfoDisplay::Off);
        let got = player_bar_height();
        set_track_info_display(saved);
        assert!(
            (got - BASE_PLAYER_BAR_HEIGHT).abs() < f32::EPSILON,
            "strip-off height drifted: got {got}, expected {BASE_PLAYER_BAR_HEIGHT}",
        );
    }

    /// When the PlayerBar strip is active, the rendered widget is
    /// `column![top_separator(1), main_row(base), strip(STRIP_HEIGHT_WITH_SEPARATOR)]`.
    /// `player_bar_height()` must account for all three rows so the
    /// `chrome_height_with_header()` math (and the iced layout) lines up.
    #[test]
    fn player_bar_height_with_strip_includes_top_separator_and_strip_separator() {
        let _guard = THEME_MODE_LOCK.lock();
        let saved = track_info_display();
        set_track_info_display(TrackInfoDisplay::PlayerBar);
        let got = player_bar_height();
        set_track_info_display(saved);
        let expected = BASE_PLAYER_BAR_HEIGHT + 1.0 + STRIP_HEIGHT_WITH_SEPARATOR;
        assert!(
            (got - expected).abs() < f32::EPSILON,
            "strip-on height drifted: got {got}, expected {expected}",
        );
    }
}

#[cfg(test)]
mod mini_player_layout_tests {
    use nokkvi_data::types::player_settings::TrackInfoDisplay;

    use super::{
        CULL_ORDER, MINI_PLAYER_BAR_HEIGHT, PlayerBarLayout, effective_player_bar_layout,
        mini_player_min_width, player_bar_height, show_mini_player_section,
    };
    use crate::theme::{THEME_MODE_LOCK, set_track_info_display, track_info_display};

    fn layout(kebab: u8) -> PlayerBarLayout {
        layout_w(kebab, false)
    }

    fn layout_w(kebab: u8, wide: bool) -> PlayerBarLayout {
        PlayerBarLayout {
            kebab_mode_count: kebab,
            wide_for_three_section: wide,
        }
    }

    /// `player_bar_height()` returns the MiniPlayer capsule bar height in
    /// MiniPlayer mode and the base height otherwise. The capsule scrub is a
    /// real layout row, so MiniPlayer is TALLER than the base bar — the chrome /
    /// slot-list / visualizer math keys off this, so the difference must hold.
    #[test]
    fn player_bar_height_is_capsule_height_in_mini_player() {
        let _guard = THEME_MODE_LOCK.lock();
        let saved = track_info_display();

        set_track_info_display(TrackInfoDisplay::MiniPlayer);
        let mini_h = player_bar_height();
        set_track_info_display(TrackInfoDisplay::Off);
        let base_h = player_bar_height();

        set_track_info_display(saved);

        assert_eq!(mini_h, MINI_PLAYER_BAR_HEIGHT);
        assert!(
            mini_h > base_h,
            "MiniPlayer capsule bar {mini_h} should be taller than base {base_h}",
        );
    }

    /// COMPACT MiniPlayer regime (wide_for_three_section = false): every mode is
    /// folded into one permanent kebab. Non-MiniPlayer modes pass through.
    #[test]
    fn effective_forces_all_modes_into_kebab_in_compact_mini() {
        let _guard = THEME_MODE_LOCK.lock();
        let saved = track_info_display();

        set_track_info_display(TrackInfoDisplay::MiniPlayer);
        let compact = effective_player_bar_layout(layout_w(0, false));

        set_track_info_display(TrackInfoDisplay::PlayerBar);
        let other = effective_player_bar_layout(layout_w(3, false));

        set_track_info_display(saved);

        assert_eq!(
            compact.kebab_mode_count,
            CULL_ORDER.len() as u8,
            "compact MiniPlayer must fold every mode into the kebab",
        );
        assert_eq!(
            other,
            layout_w(3, false),
            "non-MiniPlayer modes pass the layout through unchanged",
        );
    }

    /// WIDE MiniPlayer regime (wide_for_three_section = true): the width-driven
    /// kebab_mode_count passes THROUGH (modes expand / cull like the normal bar),
    /// NOT force-folded into one kebab.
    #[test]
    fn effective_passes_kebab_through_in_wide_mini() {
        let _guard = THEME_MODE_LOCK.lock();
        let saved = track_info_display();

        set_track_info_display(TrackInfoDisplay::MiniPlayer);
        let all_inline = effective_player_bar_layout(layout_w(0, true));
        let some_culled = effective_player_bar_layout(layout_w(2, true));

        set_track_info_display(saved);

        assert_eq!(
            all_inline.kebab_mode_count, 0,
            "wide MiniPlayer keeps all modes inline when width-culling says 0",
        );
        assert_eq!(
            some_culled.kebab_mode_count, 2,
            "wide MiniPlayer passes the width-driven kebab count through, not force-7",
        );
    }

    /// `effective_player_bar_layout` passes the regime flag through unchanged in
    /// BOTH regimes — the render arm depends on its true value to pick the
    /// subtree, so clobbering it would desync the layout from the modes.
    #[test]
    fn effective_preserves_wide_flag() {
        let _guard = THEME_MODE_LOCK.lock();
        let saved = track_info_display();

        set_track_info_display(TrackInfoDisplay::MiniPlayer);
        let wide = effective_player_bar_layout(layout_w(0, true));
        let compact = effective_player_bar_layout(layout_w(0, false));

        set_track_info_display(saved);

        assert!(wide.wide_for_three_section);
        assert!(!compact.wide_for_three_section);
    }

    /// Non-MiniPlayer modes ignore the regime flag entirely — they pass the base
    /// layout through verbatim regardless of `wide_for_three_section`.
    #[test]
    fn non_mini_modes_ignore_wide_flag() {
        let _guard = THEME_MODE_LOCK.lock();
        let saved = track_info_display();

        for mode in [
            TrackInfoDisplay::Off,
            TrackInfoDisplay::PlayerBar,
            TrackInfoDisplay::TopBar,
        ] {
            set_track_info_display(mode);
            assert_eq!(
                effective_player_bar_layout(layout_w(3, true)),
                layout_w(3, true),
                "{mode:?} must pass the layout through unchanged (wide)",
            );
            assert_eq!(
                effective_player_bar_layout(layout_w(3, false)),
                layout_w(3, false),
                "{mode:?} must pass the layout through unchanged (compact)",
            );
        }

        set_track_info_display(saved);
    }

    /// The metadata section shows at generous widths and hides only when it can
    /// no longer fit beside the cluster; it never renders outside MiniPlayer.
    #[test]
    fn show_section_tracks_mode_and_width() {
        let _guard = THEME_MODE_LOCK.lock();
        let saved = track_info_display();

        set_track_info_display(TrackInfoDisplay::MiniPlayer);
        let wide = show_mini_player_section(4000.0, layout(7));
        let narrow = show_mini_player_section(120.0, layout(7));

        set_track_info_display(TrackInfoDisplay::Off);
        let off = show_mini_player_section(4000.0, layout(7));

        set_track_info_display(saved);

        assert!(wide, "metadata should show when there is ample width");
        assert!(
            !narrow,
            "metadata should hide when too narrow for the cluster",
        );
        assert!(!off, "metadata only renders in MiniPlayer mode");
    }

    /// Hiding the volume / mode controls shrinks the compact-regime metadata-hide
    /// threshold, so metadata stays visible at narrower widths when the cluster
    /// it has to coexist with is smaller.
    #[test]
    fn min_width_shrinks_when_controls_hidden() {
        let _guard = THEME_MODE_LOCK.lock();
        let saved_v = crate::theme::mini_player_show_volume();
        let saved_m = crate::theme::mini_player_show_modes();

        crate::theme::set_mini_player_show_volume(true);
        crate::theme::set_mini_player_show_modes(true);
        let both = mini_player_min_width(500.0, layout(0));

        crate::theme::set_mini_player_show_volume(false);
        crate::theme::set_mini_player_show_modes(false);
        let neither = mini_player_min_width(500.0, layout(0));

        crate::theme::set_mini_player_show_volume(saved_v);
        crate::theme::set_mini_player_show_modes(saved_m);

        assert!(
            neither < both,
            "hiding controls should lower the metadata-hide threshold ({neither} !< {both})",
        );
    }
}

#[cfg(test)]
mod transport_hover_convention_tests {
    use iced::widget::button;

    use super::{mode_toggle_style, transport_button_style};
    use crate::theme::THEME_MODE_LOCK;

    /// Hover/press feedback on the transport buttons is owned entirely by the
    /// wrapping `HoverOverlay` (the accent-wash helpers), matching the nav-bar
    /// convention spelled out at `nav_bar::flat_tab_container_style`: the
    /// `button::Style` closure encodes only active-vs-idle, so an INACTIVE
    /// button keeps the same background across `Active` (idle), `Hovered`, and
    /// `Pressed`. Guards against re-introducing the pre-redesign neutral
    /// `bg1()` hover fill underneath the overlay.
    #[test]
    fn inactive_transport_button_background_is_hover_invariant() {
        let _guard = THEME_MODE_LOCK.lock();
        let style = transport_button_style(false);
        let idle = style(&iced::Theme::Dark, button::Status::Active).background;
        let hovered = style(&iced::Theme::Dark, button::Status::Hovered).background;
        let pressed = style(&iced::Theme::Dark, button::Status::Pressed).background;
        assert_eq!(
            idle, hovered,
            "inactive transport button changed background on hover; hover must \
             be delegated to HoverOverlay, not painted by the button style",
        );
        assert_eq!(
            idle, pressed,
            "inactive transport button changed background on press; press must \
             be delegated to HoverOverlay, not painted by the button style",
        );
    }

    /// Same convention for the bordered mode toggles (repeat / shuffle /
    /// consume / EQ / SFX): the inactive background is hover-invariant and the
    /// overlay owns the hover affordance.
    #[test]
    fn inactive_mode_toggle_background_is_hover_invariant() {
        let _guard = THEME_MODE_LOCK.lock();
        let style = mode_toggle_style(false);
        let idle = style(&iced::Theme::Dark, button::Status::Active).background;
        let hovered = style(&iced::Theme::Dark, button::Status::Hovered).background;
        let pressed = style(&iced::Theme::Dark, button::Status::Pressed).background;
        assert_eq!(
            idle, hovered,
            "inactive mode toggle changed background on hover; hover must be \
             delegated to HoverOverlay, not painted by the button style",
        );
        assert_eq!(
            idle, pressed,
            "inactive mode toggle changed background on press; press must be \
             delegated to HoverOverlay, not painted by the button style",
        );
    }
}

#[cfg(test)]
mod section_width_tests {
    use super::{
        CULL_ORDER, PlayerBarLayout, SECTION_BUTTON_GAP, TRANSPORT_SIZE, mode_button_width,
        mode_section_width, transport_section_width, volume_section_width,
    };

    fn layout(kebab: u8) -> PlayerBarLayout {
        PlayerBarLayout {
            kebab_mode_count: kebab,
            wide_for_three_section: false,
        }
    }

    #[test]
    fn transport_width_is_the_three_button_set() {
        // Always the modern 3-button set: 3 × 40 + 2 gaps × 4 = 128.
        assert_eq!(
            transport_section_width(),
            3.0 * TRANSPORT_SIZE + 2.0 * SECTION_BUTTON_GAP
        );
        assert_eq!(transport_section_width(), 128.0);
    }

    #[test]
    fn mode_width_tracks_inline_count_and_kebab() {
        let mode_btn_w = mode_button_width();
        let chrome_w = crate::widgets::sizes::TOOLBAR_BUTTON_SIZE;
        let total_modes = CULL_ORDER.len() as f32;

        // All inline (kebab_count=0): 9 buttons + 8 gaps, no kebab.
        let all_inline = mode_section_width(layout(0), false);
        assert_eq!(
            all_inline,
            total_modes * mode_btn_w + (total_modes - 1.0) * SECTION_BUTTON_GAP
        );

        // Some culled (kebab_count=5): with 9 modes, 4 inline + kebab + 4 gaps.
        let some_culled = mode_section_width(layout(5), false);
        assert_eq!(
            some_culled,
            4.0 * mode_btn_w + chrome_w + 4.0 * SECTION_BUTTON_GAP
        );

        // Hamburger adds one more button + gap.
        let with_hamburger = mode_section_width(layout(5), true);
        assert_eq!(with_hamburger, some_culled + chrome_w + SECTION_BUTTON_GAP);
    }

    #[test]
    fn volume_width_tracks_sfx_visibility() {
        if crate::theme::is_horizontal_volume() {
            // Horizontal: same width regardless of SFX (SFX stacks vertically).
            assert_eq!(
                volume_section_width(false),
                crate::widgets::volume_slider::HORIZONTAL_LENGTH
            );
            assert_eq!(
                volume_section_width(true),
                crate::widgets::volume_slider::HORIZONTAL_LENGTH
            );
        } else {
            assert_eq!(
                volume_section_width(false),
                crate::widgets::volume_slider::BAR_WIDTH
            );
            assert_eq!(
                volume_section_width(true),
                2.0 * crate::widgets::volume_slider::BAR_WIDTH + SECTION_BUTTON_GAP
            );
        }
    }
}

#[cfg(test)]
mod layout_tests {
    use super::{
        CULL_ENTER_WIDTHS, CULL_HYSTERESIS_PX, CULL_ORDER, MINI_THREE_SECTION_ENTER,
        MINI_THREE_SECTION_EXIT, ModeId, PlayerBarLayout, compute_layout,
    };

    fn empty() -> PlayerBarLayout {
        PlayerBarLayout::default()
    }

    fn layout(count: u8) -> PlayerBarLayout {
        layout3(count, false)
    }

    fn layout3(count: u8, wide: bool) -> PlayerBarLayout {
        PlayerBarLayout {
            kebab_mode_count: count,
            wide_for_three_section: wide,
        }
    }

    // ---- mode culling ----

    #[test]
    fn wide_width_keeps_all_modes_inline() {
        // Far above any threshold — no culling.
        let result = compute_layout(1200.0, empty());
        assert_eq!(result.kebab_mode_count, 0);
    }

    #[test]
    fn at_exact_first_threshold_no_culling() {
        // Visualizer enters when width *strictly* < threshold[0], so a width
        // sitting exactly on the threshold leaves it inline.
        let result = compute_layout(CULL_ENTER_WIDTHS[0], empty());
        assert_eq!(result.kebab_mode_count, 0);
    }

    /// Tripwire for the CULL_ORDER ↔ inline-row coupling. Modes fold
    /// rightmost-first, so the inline row's order (left→right) REVERSED must
    /// equal CULL_ORDER — otherwise a mode folds from the wrong end and the gap
    /// doesn't close cleanly from the right edge (a real bug an earlier
    /// Bit-Perfect insertion hit). `INLINE_ORDER` mirrors the `if !X_in_kebab {
    /// push }` sequence in `player_bar()`; keep the two in lockstep when adding
    /// or reordering a mode.
    #[test]
    fn inline_row_order_reversed_matches_cull_order() {
        const INLINE_ORDER: [ModeId; 9] = [
            ModeId::Repeat,
            ModeId::Shuffle,
            ModeId::Consume,
            ModeId::Eq,
            ModeId::Sfx,
            ModeId::BitPerfect,
            ModeId::Crossfade,
            ModeId::Visualizer,
            ModeId::Lyrics,
        ];
        let mut reversed = INLINE_ORDER;
        reversed.reverse();
        assert_eq!(
            reversed, CULL_ORDER,
            "inline render order reversed must equal CULL_ORDER (update both \
             together) so modes fold cleanly from the right edge",
        );
    }

    #[test]
    fn one_pixel_below_first_threshold_culls_visualizer() {
        let result = compute_layout(CULL_ENTER_WIDTHS[0] - 1.0, empty());
        assert_eq!(result.kebab_mode_count, 1);
    }

    #[test]
    fn each_threshold_culls_exactly_one_more_mode() {
        // Walk down past every cull threshold; each step adds exactly one
        // mode to the kebab — the bug the granular cull is fixing.
        for (i, &threshold) in CULL_ENTER_WIDTHS.iter().enumerate() {
            let just_below = threshold - 1.0;
            let result = compute_layout(just_below, empty());
            assert_eq!(
                result.kebab_mode_count,
                (i + 1) as u8,
                "width {just_below} should cull exactly {} modes",
                i + 1
            );
        }
    }

    #[test]
    fn extremely_narrow_width_culls_all_modes() {
        let result = compute_layout(100.0, empty());
        assert_eq!(result.kebab_mode_count, CULL_ENTER_WIDTHS.len() as u8);
    }

    // ---- mode hysteresis ----

    #[test]
    fn culled_mode_stays_culled_inside_hysteresis_band() {
        // Visualizer was culled at < threshold[0]; pops out only once width
        // reaches threshold[0] + hysteresis. One pixel inside the band, the
        // count stays at 1.
        let prev = layout(1);
        let inside_band = CULL_ENTER_WIDTHS[0] + CULL_HYSTERESIS_PX - 1.0;
        assert_eq!(compute_layout(inside_band, prev).kebab_mode_count, 1);
    }

    #[test]
    fn culled_mode_pops_inline_at_exit_threshold() {
        // Width hits threshold[0] + hysteresis exactly → visualizer pops back
        // inline.
        let prev = layout(1);
        let exit = CULL_ENTER_WIDTHS[0] + CULL_HYSTERESIS_PX;
        assert_eq!(compute_layout(exit, prev).kebab_mode_count, 0);
    }

    #[test]
    fn hysteresis_applies_to_each_cull_index_independently() {
        // For every cull index, verify the hysteresis band keeps it inside
        // the kebab and clearing the band pops it out.
        for (i, &threshold) in CULL_ENTER_WIDTHS.iter().enumerate() {
            let count_before = (i + 1) as u8;
            let exit = threshold + CULL_HYSTERESIS_PX;

            // Inside the band — count stays.
            let prev = layout(count_before);
            assert_eq!(
                compute_layout(exit - 1.0, prev).kebab_mode_count,
                count_before,
                "cull idx {i}: width {} should keep count at {count_before}",
                exit - 1.0,
            );

            // At/above exit — count drops by one.
            assert_eq!(
                compute_layout(exit, prev).kebab_mode_count,
                count_before - 1,
                "cull idx {i}: width {exit} should drop count to {}",
                count_before - 1,
            );
        }
    }

    // ---- multi-step jumps from rapid resize ----

    #[test]
    fn jump_from_wide_to_very_narrow_culls_all_modes_at_once() {
        let result = compute_layout(100.0, empty());
        assert_eq!(result.kebab_mode_count, CULL_ENTER_WIDTHS.len() as u8);
    }

    #[test]
    fn jump_from_narrow_to_wide_pops_all_modes_back_inline() {
        let prev = layout(7);
        let result = compute_layout(1200.0, prev);
        assert_eq!(result.kebab_mode_count, 0);
    }

    // ---- three-section regime (MiniPlayer wide vs compact) ----

    #[test]
    fn three_section_enters_only_at_exit_threshold() {
        // From compact (wide=false): flip ON only once width clears the higher
        // EXIT entry point; one pixel below stays compact.
        assert!(
            compute_layout(MINI_THREE_SECTION_EXIT, empty()).wide_for_three_section,
            "should enter three-section at >= EXIT",
        );
        assert!(
            !compute_layout(MINI_THREE_SECTION_EXIT - 1.0, empty()).wide_for_three_section,
            "must NOT enter three-section just below EXIT",
        );
    }

    #[test]
    fn three_section_holds_inside_hysteresis_band() {
        // From three-section (wide=true): hold across the band, fall back only
        // below the lower ENTER drop point. Asymmetric, mirroring transport
        // collapse — flip ON at the higher EXIT, OFF at the lower ENTER.
        let wide_prev = layout3(0, true);
        assert!(
            compute_layout(MINI_THREE_SECTION_ENTER, wide_prev).wide_for_three_section,
            "should stay three-section at the ENTER drop point",
        );
        let mid = (MINI_THREE_SECTION_ENTER + MINI_THREE_SECTION_EXIT) / 2.0;
        assert!(
            compute_layout(mid, wide_prev).wide_for_three_section,
            "should hold three-section inside the hysteresis band",
        );
        assert!(
            !compute_layout(MINI_THREE_SECTION_ENTER - 1.0, wide_prev).wide_for_three_section,
            "should fall back to compact below ENTER",
        );
    }

    #[test]
    fn three_section_culls_modes_within_the_regime() {
        // The whole point: the three-section band sits below the cull range, so
        // modes fold into the kebab one-by-one WHILE the transports stay centered
        // (rather than the regime only existing above the cull band). A mid-band
        // width is still three-section AND has a partial kebab.
        let mid = compute_layout(900.0, empty());
        assert!(
            mid.wide_for_three_section,
            "900px is inside the three-section regime",
        );
        assert!(
            mid.kebab_mode_count > 0 && mid.kebab_mode_count < CULL_ORDER.len() as u8,
            "modes cull one-by-one within the centered three-section layout",
        );
    }

    #[test]
    fn three_section_flag_independent_of_kebab() {
        // Far above the cull range: three-section with every mode inline (the
        // roomy look).
        let wide = compute_layout(1300.0, empty());
        assert!(wide.wide_for_three_section);
        assert_eq!(wide.kebab_mode_count, 0);

        // Below the band: compact (the floor where centered stops fitting).
        let narrow = compute_layout(MINI_THREE_SECTION_ENTER - 1.0, empty());
        assert!(!narrow.wide_for_three_section);
    }

    // ---- cull-table drift guards ----

    #[test]
    fn cull_order_contains_every_mode_exactly_once() {
        // Complements the CULL_ORDER.len() == CULL_ENTER_WIDTHS.len() const-assert:
        // catches a mode being dropped/duplicated within CULL_ORDER (which keeps
        // the length but leaves some mode inline-only, never culling).
        let all = [
            ModeId::Lyrics,
            ModeId::Visualizer,
            ModeId::Crossfade,
            ModeId::BitPerfect,
            ModeId::Sfx,
            ModeId::Eq,
            ModeId::Consume,
            ModeId::Shuffle,
            ModeId::Repeat,
        ];
        for m in all {
            assert_eq!(
                CULL_ORDER.iter().filter(|&&x| x == m).count(),
                1,
                "{m:?} must appear in CULL_ORDER exactly once",
            );
        }
        assert_eq!(CULL_ORDER.len(), all.len(), "CULL_ORDER length drifted");
    }

    #[test]
    fn cull_enter_widths_strictly_decreasing() {
        // The one-mode-at-a-time cull (update_kebab_count) relies on the
        // thresholds decreasing monotonically.
        for w in CULL_ENTER_WIDTHS.windows(2) {
            assert!(
                w[0] > w[1],
                "CULL_ENTER_WIDTHS must strictly decrease: {} !> {}",
                w[0],
                w[1],
            );
        }
    }

    // ---- is_in_kebab ----

    #[test]
    fn is_in_kebab_false_when_count_is_zero() {
        let l = empty();
        for mode in [
            ModeId::Visualizer,
            ModeId::Crossfade,
            ModeId::Sfx,
            ModeId::Eq,
            ModeId::Consume,
            ModeId::Shuffle,
            ModeId::Repeat,
        ] {
            assert!(!l.is_in_kebab(mode));
        }
    }

    #[test]
    fn is_in_kebab_first_culled_is_lyrics() {
        // Lyrics heads CULL_ORDER (it folds away first as the bar narrows).
        let l = layout(1);
        assert!(l.is_in_kebab(ModeId::Lyrics));
        assert!(!l.is_in_kebab(ModeId::Visualizer));
        assert!(!l.is_in_kebab(ModeId::Repeat));
    }

    #[test]
    fn is_in_kebab_all_modes_at_full_count() {
        let l = layout(9);
        for mode in [
            ModeId::Lyrics,
            ModeId::Visualizer,
            ModeId::Crossfade,
            ModeId::BitPerfect,
            ModeId::Sfx,
            ModeId::Eq,
            ModeId::Consume,
            ModeId::Shuffle,
            ModeId::Repeat,
        ] {
            assert!(l.is_in_kebab(mode));
        }
    }
}

#[cfg(test)]
mod mode_descriptor_tests {
    use nokkvi_data::types::player_settings::VisualizationMode;

    use super::{
        CULL_ORDER, ModeId, PlayerBarLayout, PlayerBarMessage, PlayerBarViewData,
        capsule_scrub_labels, mode_descriptor,
    };

    const BP_OFF: crate::state::BitPerfectStatus = crate::state::BitPerfectStatus::Off;

    /// Baseline view data with every mode toggled off / neutral. Individual
    /// tests flip just the fields they exercise.
    fn sample_data() -> PlayerBarViewData {
        PlayerBarViewData {
            playback_position: 0,
            playback_duration: 0,
            playback_playing: false,
            playback_paused: false,
            volume: 1.0,
            has_queue: false,
            is_radio: false,
            is_random_mode: false,
            is_repeat_mode: false,
            is_repeat_queue_mode: false,
            is_consume_mode: false,
            eq_enabled: false,
            sound_effects_enabled: false,
            sfx_volume: 1.0,
            crossfade_enabled: false,
            lyrics_enabled: false,
            bit_perfect_mode: nokkvi_data::types::player_settings::BitPerfectMode::Off,
            visualization_mode: VisualizationMode::Off,
            window_width: 1200.0,
            layout: PlayerBarLayout::default(),
            is_light_mode: false,
            track_title: String::new(),
            track_artist: String::new(),
            track_album: String::new(),
            format_suffix: String::new(),
            sample_rate: 0,
            bitrate: 0,
            bit_perfect_status: crate::state::BitPerfectStatus::Off,
            bit_perfect_holder: None,
            radio_name: None,
            artwork_handle: None,
            hamburger_open: false,
            player_modes_open: false,
        }
    }

    /// Capsule end-caps tuck codec + bitrate around the time for strip-mode
    /// parity: `3:40 · FLAC 44.1kHz` (left) and `1411kbps · 8:00` (right). The
    /// bare time is kept separate so it can render brighter than the metadata.
    #[test]
    fn capsule_labels_full_codec_and_bitrate() {
        let (left, right) =
            capsule_scrub_labels("3:40", "8:00", "flac", 44.1, 1411, true, BP_OFF, None);
        assert_eq!(left.full, "3:40 · FLAC 44.1kHz");
        assert_eq!(left.time, "3:40");
        assert_eq!(right.full, "1411kbps · 8:00");
        assert_eq!(right.time, "8:00");
    }

    /// No bitrate → the right cap is time-only (`full == time`); the left cap
    /// still carries the codec + sample rate over a separate bare time.
    #[test]
    fn capsule_labels_no_bitrate_keeps_bare_duration() {
        let (left, right) =
            capsule_scrub_labels("0:05", "5:00", "flac", 96.0, 0, true, BP_OFF, None);
        assert_eq!(left.full, "0:05 · FLAC 96.0kHz");
        assert_eq!(left.time, "0:05");
        assert_eq!(right.full, "5:00");
        assert_eq!(right.time, "5:00");
    }

    /// Radio-style stream (no sample rate, `--:--` duration) — the codec drops
    /// its kHz and the bitrate still tucks against the placeholder duration.
    #[test]
    fn capsule_labels_no_sample_rate_radio_duration() {
        let (left, right) =
            capsule_scrub_labels("0:30", "--:--", "mp3", 0.0, 320, true, BP_OFF, None);
        assert_eq!(left.full, "0:30 · MP3");
        assert_eq!(left.time, "0:30");
        assert_eq!(right.full, "320kbps · --:--");
        assert_eq!(right.time, "--:--");
    }

    /// Empty codec suffix → both caps fall back to time-only (no stray ` · `).
    #[test]
    fn capsule_labels_empty_suffix_falls_back_to_time() {
        let (left, right) =
            capsule_scrub_labels("3:40", "8:00", "", 44.1, 1411, true, BP_OFF, None);
        assert_eq!(left.full, "3:40");
        assert_eq!(left.time, "3:40");
        assert_eq!(right.full, "8:00");
        assert_eq!(right.time, "8:00");
    }

    /// Format display off → time-only caps even when full metadata is present
    /// (honors the shared `strip_show_format_info()` toggle).
    #[test]
    fn capsule_labels_format_off_is_bare_time() {
        let (left, right) =
            capsule_scrub_labels("3:40", "8:00", "flac", 44.1, 1411, false, BP_OFF, None);
        assert_eq!(left.full, "3:40");
        assert_eq!(left.time, "3:40");
        assert_eq!(right.full, "8:00");
        assert_eq!(right.time, "8:00");
    }

    /// Bit-perfect Verified tucks a "· BIT-PERFECT" badge in after the codec on
    /// the left cap (the MiniPlayer's honest indicator); the bare time and the
    /// right cap are untouched. Resampled tucks "· RESAMPLED" the same way.
    #[test]
    fn capsule_labels_tuck_bit_perfect_badge_after_codec() {
        let verified = crate::state::BitPerfectStatus::Verified;
        let (left, right) =
            capsule_scrub_labels("0:05", "5:00", "flac", 96.0, 0, true, verified, None);
        assert_eq!(left.full, "0:05 · FLAC 96.0kHz · BIT-PERFECT");
        assert_eq!(left.time, "0:05");
        assert_eq!(
            right.full, "5:00",
            "the badge only tucks into the left/codec cap"
        );

        // Resampled tucks the device rate AND the holder in inline (track 96k,
        // device latched at 48k by "Zen" → "RESAMPLED→48k · Zen"), so the
        // tooltip-less capsule names the culprit at a glance.
        let resampled = crate::state::BitPerfectStatus::Resampled {
            device_rate: 48_000,
        };
        let (left, _) = capsule_scrub_labels(
            "0:05",
            "5:00",
            "flac",
            96.0,
            0,
            true,
            resampled,
            Some("Zen"),
        );
        assert_eq!(left.full, "0:05 · FLAC 96.0kHz · RESAMPLED→48k · Zen");
    }

    /// Repeat is the most state-rich icon mode: its icon, tooltip, and kebab
    /// label all key off the queue/track/off tri-state. Pins all three plus
    /// the message ctor.
    #[test]
    fn repeat_descriptor_strings_track_queue_track_off_state() {
        // Queue repeat.
        let mut data = sample_data();
        data.is_repeat_queue_mode = true;
        let d = mode_descriptor(ModeId::Repeat, &data);
        assert_eq!(d.icon, Some("assets/icons/repeat-2.svg"));
        assert_eq!(d.tooltip, "Repeat Queue: Restart queue when it ends");
        assert_eq!(d.kebab_label, "Repeat: Queue");
        assert!(matches!(d.message, PlayerBarMessage::ToggleRepeat));

        // Track repeat (queue off).
        let mut data = sample_data();
        data.is_repeat_mode = true;
        let d = mode_descriptor(ModeId::Repeat, &data);
        assert_eq!(d.icon, Some("assets/icons/repeat-1.svg"));
        assert_eq!(d.tooltip, "Repeat Track: Loop the current track");
        assert_eq!(d.kebab_label, "Repeat: Track");

        // Off.
        let d = mode_descriptor(ModeId::Repeat, &sample_data());
        assert_eq!(d.icon, Some("assets/icons/repeat-1.svg"));
        assert_eq!(d.tooltip, "Repeat: Off");
        assert_eq!(d.kebab_label, "Repeat: Off");
    }

    /// Visualizer couples a dynamic icon to a four-way mode. Pins the
    /// icon + dual-label pair for every `VisualizationMode`.
    #[test]
    fn visualizer_descriptor_matches_visualization_mode() {
        let cases = [
            (
                VisualizationMode::Off,
                "assets/icons/audio-lines.svg",
                "Visualizer: Off",
            ),
            (
                VisualizationMode::Lines,
                "assets/icons/audio-waveform.svg",
                "Visualizer: Waveform",
            ),
            (
                VisualizationMode::Bars,
                "assets/icons/audio-lines.svg",
                "Visualizer: Bars",
            ),
            (
                VisualizationMode::Scope,
                "assets/icons/radar.svg",
                "Visualizer: Scope",
            ),
        ];
        for (mode, icon, label) in cases {
            let mut data = sample_data();
            data.visualization_mode = mode;
            let d = mode_descriptor(ModeId::Visualizer, &data);
            assert_eq!(d.icon, Some(icon), "icon for {mode:?}");
            assert_eq!(d.tooltip, label, "tooltip for {mode:?}");
            assert_eq!(d.kebab_label, label, "kebab_label for {mode:?}");
            assert!(matches!(d.message, PlayerBarMessage::CycleVisualization));
        }
    }

    /// EQ and SFX are the two text-toggle modes (icon == None). SFX has an
    /// intentional inline-vs-kebab string asymmetry: a STATIC tooltip
    /// ("...enabled", since the inline button only renders when on) versus a
    /// kebab label that flips On/Off. EQ has its own Active/Disabled vs On/Off
    /// asymmetry. Pinning both stops a future agent from silently unifying them.
    #[test]
    fn text_toggle_modes_have_none_icon_and_distinct_sfx_strings() {
        // EQ off.
        let eq_off = mode_descriptor(ModeId::Eq, &sample_data());
        assert_eq!(eq_off.icon, None);
        assert_eq!(eq_off.tooltip, "Equalizer: Disabled");
        assert_eq!(eq_off.kebab_label, "Equalizer: Off");
        // EQ on.
        let mut data = sample_data();
        data.eq_enabled = true;
        let eq_on = mode_descriptor(ModeId::Eq, &data);
        assert_eq!(eq_on.tooltip, "Equalizer: Active");
        assert_eq!(eq_on.kebab_label, "Equalizer: On");

        // SFX off — tooltip stays the static "enabled" string by design.
        let sfx_off = mode_descriptor(ModeId::Sfx, &sample_data());
        assert_eq!(sfx_off.icon, None);
        assert_eq!(sfx_off.tooltip, "Sound Effects: UI sounds enabled");
        assert_eq!(sfx_off.kebab_label, "UI Sound Effects: Off");
        // SFX on — tooltip unchanged; only the kebab label flips.
        let mut data = sample_data();
        data.sound_effects_enabled = true;
        let sfx_on = mode_descriptor(ModeId::Sfx, &data);
        assert_eq!(sfx_on.tooltip, "Sound Effects: UI sounds enabled");
        assert_eq!(sfx_on.kebab_label, "UI Sound Effects: On");
        assert_eq!(
            sfx_off.tooltip, sfx_on.tooltip,
            "SFX inline tooltip must stay static across the on/off flip",
        );
    }

    /// Guards the message-routing half of the spec against a copy-paste swap:
    /// every `ModeId` must map to its own toggle message ctor.
    #[test]
    fn every_mode_id_yields_its_own_toggle_message() {
        let data = sample_data();
        for mode in CULL_ORDER {
            let d = mode_descriptor(mode, &data);
            let ok = match mode {
                ModeId::Repeat => matches!(d.message, PlayerBarMessage::ToggleRepeat),
                ModeId::Shuffle => matches!(d.message, PlayerBarMessage::ToggleRandom),
                ModeId::Consume => matches!(d.message, PlayerBarMessage::ToggleConsume),
                ModeId::Eq => matches!(d.message, PlayerBarMessage::ToggleEq),
                ModeId::Sfx => matches!(d.message, PlayerBarMessage::ToggleSoundEffects),
                ModeId::Crossfade => matches!(d.message, PlayerBarMessage::ToggleCrossfade),
                ModeId::BitPerfect => matches!(d.message, PlayerBarMessage::ToggleBitPerfect),
                ModeId::Visualizer => matches!(d.message, PlayerBarMessage::CycleVisualization),
                ModeId::Lyrics => matches!(d.message, PlayerBarMessage::ToggleLyrics),
            };
            assert!(ok, "wrong message ctor for {mode:?}: {:?}", d.message);
        }
    }
}
