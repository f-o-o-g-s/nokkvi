//! Player Bar Component
//!
//! Self-contained player controls bar with message bubbling pattern.
//! Receives pure view data and emits actions for root to process.

use iced::{
    Alignment, Color, Element, Length, Theme,
    advanced::svg::Handle,
    font::{Font, Weight},
    mouse::ScrollDelta,
    widget::{Svg, button, column, container, mouse_area, row, svg, text, tooltip},
};

use crate::{theme, widgets, widgets::hover_overlay::HoverOverlay};

// Player bar dimensions (flat redesign). 72 px in both modes — the design
// CSS specified 64 in flat mode and 72 in rounded, but the 8 px difference
// makes the 44 px mode buttons feel cramped (10 px gap each side) in flat
// vs. floating (14 px gap each side) in rounded. Using 72 in both modes
// gives the transport + mode buttons the same airy breathing room across
// the two visual languages.
const BASE_PLAYER_BAR_HEIGHT: f32 = 72.0;
const CONTROL_ROW_HEIGHT: f32 = 44.0;
/// Transport button (prev/play/pause/stop/next) — 40×40, borderless flat icon.
const TRANSPORT_SIZE: f32 = 40.0;
/// Mode toggle button (repeat/shuffle/consume/EQ/SFX/crossfade/visualizer).
/// Flat: 38×44; rounded: 40×44.
const MODE_BUTTON_HEIGHT: f32 = 44.0;
/// Height of the track info strip below the player bar in PlayerBar display mode.
/// Re-uses the canonical constant from `track_info_strip.rs` to avoid drift.
use super::track_info_strip::STRIP_HEIGHT as INFO_STRIP_HEIGHT;

/// Compact transport-button size used while in `MiniPlayer` display mode.
/// 28 px (with a 14 px glyph) lets the buttons stack above the progress
/// scrub inside the existing 72 px bar instead of bumping bar height. The
/// icon scales 1:2 with the button so the proportions match the standard
/// 40/20 buttons.
const MINI_PLAYER_TRANSPORT_SIZE: f32 = 28.0;
const MINI_PLAYER_TRANSPORT_ICON_SIZE: f32 = 14.0;

/// Compact progress-row height in MiniPlayer stacked mode: matches the
/// progress widget's intrinsic 24 px so the row introduces no extra
/// vertical padding around the scrub.
const MINI_PLAYER_PROGRESS_ROW_HEIGHT: f32 = 24.0;

/// Vertical gap between the centered transports row and the progress row
/// inside the MiniPlayer stacked column.
const MINI_PLAYER_STACK_SPACING: f32 = 4.0;

/// Vertical padding applied to the main row in `MiniPlayer` rounded mode.
/// Slimmer than the default [10, 12] so 28 (transports) + 4 (gap) + 24
/// (progress) = 56 px of stacked content fits inside the 72 px bar with a
/// few px of slack. Flat mode already runs at zero vertical padding.
const MINI_PLAYER_ROUNDED_PADDING: [u16; 2] = [6, 12];

#[inline]
fn mode_button_width() -> f32 {
    if theme::is_rounded_mode() { 40.0 } else { 38.0 }
}

/// Intra-section button gap (between transport buttons, between mode buttons,
/// between the two vertical volume bars).
const SECTION_BUTTON_GAP: f32 = 4.0;

/// Inter-section gap inside the player bar's main row (between transport and
/// progress, progress and modes, modes and volume).
const MAIN_ROW_INNER_GAP: f32 = 6.0;

/// Width of the transport-controls section for the currently-rendered count
/// of buttons. 5-button uncollapsed (prev/play/pause/stop/next) or 3-button
/// collapsed (prev / play-or-pause / next) — the section sizes to fit only
/// what's on screen, so the progress track can claim the rest of the row.
#[inline]
pub(crate) fn transport_section_width(transports_collapsed: bool) -> f32 {
    let n = if transports_collapsed { 3.0 } else { 5.0 };
    n * TRANSPORT_SIZE + (n - 1.0) * SECTION_BUTTON_GAP
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

/// Side length of the artwork thumbnail rendered to the left of the
/// transport controls in `TrackInfoDisplay::MiniPlayer` mode.
pub(crate) const MINI_PLAYER_ARTWORK_SIZE: f32 = 56.0;
/// Width of the title/artist/album text column next to that artwork.
const MINI_PLAYER_TEXT_WIDTH: f32 = 180.0;
/// Gap between the artwork and the text column inside the section.
const MINI_PLAYER_INNER_GAP: f32 = 8.0;
/// Total horizontal extent of the mini-player section. Fed to the
/// `Length::Fixed` wrapper in `main_row` so the progress bar flexes
/// into the remainder.
pub(crate) const MINI_PLAYER_SECTION_WIDTH: f32 =
    MINI_PLAYER_ARTWORK_SIZE + MINI_PLAYER_INNER_GAP + MINI_PLAYER_TEXT_WIDTH;

/// Window-width threshold below which the mini-player section hides so
/// the rest of the bar retains breathing room. Set well below the pre-stack
/// 760 px figure because MiniPlayer mode now lifts the transports out of the
/// main row and stacks them on top of the progress scrub at the smaller
/// 28 px scale — that 156 px no longer competes with the mini-player section
/// for horizontal space. Tuned to leave the progress widget itself ≳100 px
/// wide at the boundary (244 mini + ~160 stacked column min + ~80 modes/vol
/// + gaps + padding ≈ 540).
pub(crate) const MINI_PLAYER_HIDE_BELOW: f32 = 540.0;

/// Whether the mini-player left-of-transport section should render
/// for the given window width AND the active `TrackInfoDisplay`.
#[inline]
pub(crate) fn show_mini_player_section(width: f32) -> bool {
    use nokkvi_data::types::player_settings::TrackInfoDisplay;
    theme::track_info_display() == TrackInfoDisplay::MiniPlayer && width >= MINI_PLAYER_HIDE_BELOW
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

/// Dynamic player bar height: base 64/72 px, plus info strip when track display
/// is PlayerBar and nav layout is Side (in Top mode the nav bar already shows
/// track/format info). MiniPlayer mode keeps the base height — the stacked
/// transports + progress column is sized to fit by shrinking transport
/// buttons and trimming the row's vertical padding.
pub(crate) fn player_bar_height() -> f32 {
    let base = base_player_bar_height();
    if crate::theme::show_player_bar_strip() {
        base + INFO_STRIP_HEIGHT
    } else {
        base
    }
}

// SFX volume slider has its own breakpoint (independent of mode-toggle tier
// because the slider is wider than a button).
const BREAKPOINT_HIDE_SFX_SLIDER: f32 = 840.0;

/// One of the seven mode toggles the player bar exposes. Used to tag a mode
/// for cull-priority and in-kebab queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModeId {
    Visualizer,
    Crossfade,
    Sfx,
    Eq,
    Consume,
    Shuffle,
    Repeat,
}

/// Cull priority — index `i` is the i-th mode to fold into the kebab as the
/// window narrows. Ordered to match the inline row's right-to-left disappear
/// (rightmost-first) so gaps close cleanly from the right edge.
pub(crate) const CULL_ORDER: [ModeId; 7] = [
    ModeId::Visualizer,
    ModeId::Crossfade,
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
pub(crate) const CULL_ENTER_WIDTHS: [f32; 7] = [
    1070.0, // Visualizer
    1010.0, // Crossfade
    950.0,  // SFX
    890.0,  // EQ
    830.0,  // Consume
    750.0,  // Shuffle
    670.0,  // Repeat
];

pub(crate) const CULL_HYSTERESIS_PX: f32 = 40.0;

/// Width below which the transport row collapses from 5 buttons to 3 (prev /
/// play-or-pause toggle / next). Independent of mode culling — tight bars
/// benefit from collapsing transports even with a few modes still inline.
pub(crate) const TRANSPORT_COLLAPSE_ENTER: f32 = 870.0;
pub(crate) const TRANSPORT_COLLAPSE_EXIT: f32 = 910.0;

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
    /// `true` when the bar is narrow enough that transports collapse to 3
    /// (prev / play-or-pause / next).
    pub transports_collapsed: bool,
}

impl PlayerBarLayout {
    /// Whether the given mode is currently folded into the kebab menu.
    pub(crate) fn is_in_kebab(self, mode: ModeId) -> bool {
        let n = self.kebab_mode_count as usize;
        CULL_ORDER.iter().take(n).any(|m| *m == mode)
    }
}

/// Recompute the layout for a new window width given the previous layout
/// (for hysteresis). Each mode has its own enter/exit threshold pair, and
/// the transport collapse has its own threshold pair independent of mode
/// culling.
pub(crate) fn compute_layout(width: f32, prev: PlayerBarLayout) -> PlayerBarLayout {
    PlayerBarLayout {
        kebab_mode_count: update_kebab_count(width, prev.kebab_mode_count),
        transports_collapsed: update_transport_collapse(width, prev.transports_collapsed),
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

fn update_transport_collapse(width: f32, prev: bool) -> bool {
    if prev {
        // Already collapsed — stay collapsed until width clears the exit
        // threshold (hysteresis margin above the enter threshold).
        width < TRANSPORT_COLLAPSE_EXIT
    } else {
        // Expanded — collapse when width drops below the enter threshold.
        width < TRANSPORT_COLLAPSE_ENTER
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
    Stop,
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
    ScrollVolume(f32),
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

/// Style for a flat borderless transport button: no border, no background by
/// default, `bg1()` background on hover, and an optional accent fill when the
/// button is in its active state (play/pause toggled on).
fn transport_button_style(
    active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    move |_theme, status| {
        let radius = if theme::is_rounded_mode() {
            theme::ui_radius_pill()
        } else {
            iced::border::Radius::from(0.0)
        };
        let background = if active {
            Some(theme::accent_bright().into())
        } else {
            match status {
                button::Status::Hovered | button::Status::Pressed => Some(theme::bg1().into()),
                _ => None,
            }
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
/// outline; active = `accent_bright()` fill + `bg0_hard()` text; hover lightens
/// to `bg1()`). Rounded mode applies `ui_radius_sm()`.
fn mode_toggle_style(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    move |_theme, status| {
        let radius = if theme::is_rounded_mode() {
            theme::ui_radius_sm()
        } else {
            iced::border::Radius::from(0.0)
        };
        let (bg, fg, border_color) = if active {
            (
                theme::accent_bright(),
                theme::bg0_hard(),
                theme::accent_bright(),
            )
        } else {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => theme::bg1(),
                _ => theme::bg0(),
            };
            (bg, theme::fg0(), theme::border())
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

/// Active transport-button side length. Shrinks to
/// [`MINI_PLAYER_TRANSPORT_SIZE`] while the MiniPlayer track-info display
/// is selected so the stacked transports + progress column fits the
/// existing 72 px bar without bumping bar height.
#[inline]
pub(crate) fn transport_button_size() -> f32 {
    use nokkvi_data::types::player_settings::TrackInfoDisplay;
    if theme::track_info_display() == TrackInfoDisplay::MiniPlayer {
        MINI_PLAYER_TRANSPORT_SIZE
    } else {
        TRANSPORT_SIZE
    }
}

#[inline]
fn transport_icon_size() -> f32 {
    use nokkvi_data::types::player_settings::TrackInfoDisplay;
    if theme::track_info_display() == TrackInfoDisplay::MiniPlayer {
        MINI_PLAYER_TRANSPORT_ICON_SIZE
    } else {
        20.0
    }
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
        .border_radius(theme::ui_radius_pill())
        .into()
}

/// Build a flat text-labeled mode toggle (used by EQ / SFX inline buttons).
fn mode_text_toggle(
    label: &'static str,
    on_press: PlayerBarMessage,
    active: bool,
    tooltip_text: &str,
) -> Element<'static, PlayerBarMessage> {
    let label_widget = text(label).size(10.0).font(Font {
        weight: Weight::Bold,
        ..theme::ui_font()
    });
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
    .border_radius(theme::ui_radius_sm())
    .into()
}

/// Build a flat icon-based mode toggle (repeat / shuffle / consume / crossfade
/// / visualizer). 38×44 in flat mode, 40×44 in rounded mode.
fn mode_toggle_button<'a>(
    icon_path: &'static str,
    message: PlayerBarMessage,
    active: bool,
    label: &'a str,
) -> Element<'a, PlayerBarMessage> {
    let icon_color = if active {
        theme::bg0_hard()
    } else {
        theme::fg0()
    };
    let icon = svg_icon(icon_path, 18.0, icon_color);
    let inner = fixed_centered(icon.into(), mode_button_width(), MODE_BUTTON_HEIGHT);
    let btn = button(inner)
        .padding(0)
        .style(mode_toggle_style(active))
        .on_press(message);
    HoverOverlay::new(
        tooltip(
            btn,
            container(text(label).size(11.0).font(theme::ui_font())).padding(4),
            tooltip::Position::Top,
        )
        .gap(4)
        .style(theme::container_tooltip),
    )
    .border_radius(theme::ui_radius_sm())
    .into()
}

/// Build the left-of-transport artwork + 3-line metadata column rendered
/// in `TrackInfoDisplay::MiniPlayer` mode.
///
/// Layout: [56 px artwork] [8 px gap] [180 px text column with
/// `title` / `artist` / `album` stacked vertically]. Each text line is a
/// marquee that scrolls when its content overflows the column width.
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
    let radius = theme::ui_border_radius();

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

    let make_line =
        |value: String, color: Color, bold: bool| -> Element<'static, PlayerBarMessage> {
            let weight = if bold { Weight::Bold } else { Weight::Medium };
            super::marquee_text::marquee_text(value)
                .size(12.0)
                .font(Font {
                    weight,
                    ..theme::ui_font()
                })
                .color(color)
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

    let title_line = make_line(line1, theme::now_playing_color(), true);
    let artist_line = make_line(line2, theme::selected_color(), false);
    let album_line = make_line(line3, theme::fg2(), false);

    let text_column = container(
        column![title_line, artist_line, album_line]
            .spacing(2)
            .width(Length::Fixed(MINI_PLAYER_TEXT_WIDTH)),
    )
    .height(Length::Fixed(MINI_PLAYER_ARTWORK_SIZE))
    .align_y(Alignment::Center);

    let inner = row![artwork, text_column]
        .spacing(MINI_PLAYER_INNER_GAP)
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

    let player_controls: Element<'_, PlayerBarMessage> = if data.layout.transports_collapsed {
        // Collapsed transports: prev / play-or-pause toggle / next.
        // The button side length is fixed for the current mode (40 px standard,
        // 28 px MiniPlayer) so the middle button's hit target stays in place
        // when the glyph swaps between play and pause.
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
    } else {
        let play_icon_color = if playback_playing {
            theme::bg0_hard()
        } else {
            theme::fg0()
        };
        let pause_icon_color = if playback_paused {
            theme::bg0_hard()
        } else {
            theme::fg0()
        };
        let stop_icon_color = if controls_active {
            theme::fg0()
        } else {
            theme::fg4()
        };
        row![
            prev_button,
            player_control_button(
                "assets/icons/play.svg",
                PlayerBarMessage::Play,
                play_icon_color,
                playback_playing,
            ),
            player_control_button(
                "assets/icons/pause.svg",
                PlayerBarMessage::Pause,
                pause_icon_color,
                playback_paused,
            ),
            player_control_button(
                "assets/icons/stop.svg",
                PlayerBarMessage::Stop,
                stop_icon_color,
                false,
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

    let custom_progress_bar =
        widgets::progress_bar::progress_bar(position, duration, PlayerBarMessage::Seek)
            .is_playing(data.playback_playing && !data.playback_paused)
            .hide_handle(data.is_radio)
            .width(Length::Fill)
            .height(24.0);

    // MiniPlayer mode stacks the transports above the progress scrub inside a
    // single Length::Fill column. The progress row trims to a compact 24 px
    // height there so 28 (transports) + 4 (gap) + 24 (progress) sits inside
    // the existing 72 px bar with the slimmer rounded-mode padding.
    use nokkvi_data::types::player_settings::TrackInfoDisplay;
    let is_mini_player_mode = theme::track_info_display() == TrackInfoDisplay::MiniPlayer;
    let progress_row_height = if is_mini_player_mode {
        MINI_PLAYER_PROGRESS_ROW_HEIGHT
    } else {
        CONTROL_ROW_HEIGHT
    };

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
    .height(Length::Fixed(progress_row_height))
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
    let vis_icon = if visualization_mode == VisualizationMode::Lines {
        "assets/icons/audio-waveform.svg"
    } else {
        "assets/icons/audio-lines.svg"
    };
    let window_width = data.window_width;

    // SFX volume slider keeps its own width-based gate (independent of the
    // mode-toggle tier — the slider is genuinely wider than a button so it
    // deserves a separate threshold).
    let show_sfx_slider = window_width >= BREAKPOINT_HIDE_SFX_SLIDER;

    // Tooltip strings for inline mode-toggle buttons (Wide / Medium tiers).
    let repeat_icon = if is_repeat_queue_mode {
        "assets/icons/repeat-2.svg"
    } else {
        "assets/icons/repeat-1.svg"
    };
    let repeat_tooltip: &'static str = if is_repeat_queue_mode {
        "Repeat Queue: Restart queue when it ends"
    } else if is_repeat_mode {
        "Repeat Track: Loop the current track"
    } else {
        "Repeat: Off"
    };
    let shuffle_tooltip: &'static str = if is_random_mode {
        "Shuffle: Playing in random order"
    } else {
        "Shuffle: Off"
    };
    let consume_tooltip: &'static str = if is_consume_mode {
        "Consume: Tracks removed from queue after playing"
    } else {
        "Consume: Off"
    };
    let crossfade_tooltip: &'static str = if data.crossfade_enabled {
        "Crossfade: Active"
    } else {
        "Crossfade: Off"
    };
    let visualizer_tooltip: &'static str = match visualization_mode {
        VisualizationMode::Off => "Visualizer: Off",
        VisualizationMode::Lines => "Visualizer: Waveform",
        VisualizationMode::Bars => "Visualizer: Bars",
    };
    let eq_tooltip: &'static str = if eq_enabled {
        "Equalizer: Active"
    } else {
        "Equalizer: Disabled"
    };

    // Shorter labels for kebab-menu rows (Medium / Narrow tiers). Reads
    // tighter inside a 220px-wide menu than the full tooltip strings.
    let shuffle_menu_label = if is_random_mode {
        "Shuffle: On"
    } else {
        "Shuffle: Off"
    };
    let repeat_menu_label = if is_repeat_queue_mode {
        "Repeat: Queue"
    } else if is_repeat_mode {
        "Repeat: Track"
    } else {
        "Repeat: Off"
    };
    let consume_menu_label = if is_consume_mode {
        "Consume: On"
    } else {
        "Consume: Off"
    };
    let eq_menu_label = if eq_enabled {
        "Equalizer: On"
    } else {
        "Equalizer: Off"
    };
    let crossfade_menu_label = if data.crossfade_enabled {
        "Crossfade: On"
    } else {
        "Crossfade: Off"
    };
    let visualizer_menu_label: &'static str = match visualization_mode {
        VisualizationMode::Off => "Visualizer: Off",
        VisualizationMode::Lines => "Visualizer: Waveform",
        VisualizationMode::Bars => "Visualizer: Bars",
    };
    let sfx_menu_label = if sound_effects_enabled {
        "UI Sound Effects: On"
    } else {
        "UI Sound Effects: Off"
    };

    // Per-mode kebab membership — derived once from the layout snapshot so
    // the inline row and kebab construction stay in sync.
    let layout = data.layout;
    let repeat_in_kebab = layout.is_in_kebab(ModeId::Repeat);
    let shuffle_in_kebab = layout.is_in_kebab(ModeId::Shuffle);
    let consume_in_kebab = layout.is_in_kebab(ModeId::Consume);
    let eq_in_kebab = layout.is_in_kebab(ModeId::Eq);
    let sfx_in_kebab = layout.is_in_kebab(ModeId::Sfx);
    let crossfade_in_kebab = layout.is_in_kebab(ModeId::Crossfade);
    let visualizer_in_kebab = layout.is_in_kebab(ModeId::Visualizer);

    let mut mode_toggles_row = iced::widget::Row::new().spacing(4);

    // Inline mode toggles, in the historical visual order. Each mode renders
    // here only when it's NOT in the kebab. SFX has the additional gate of
    // `sound_effects_enabled` (preserves the long-standing "no SFX button
    // when SFX is off" behavior at wide widths).
    if !repeat_in_kebab {
        mode_toggles_row = mode_toggles_row.push(mode_toggle_button(
            repeat_icon,
            PlayerBarMessage::ToggleRepeat,
            repeat_active,
            repeat_tooltip,
        ));
    }
    if !shuffle_in_kebab {
        mode_toggles_row = mode_toggles_row.push(mode_toggle_button(
            "assets/icons/shuffle.svg",
            PlayerBarMessage::ToggleRandom,
            is_random_mode,
            shuffle_tooltip,
        ));
    }
    if !consume_in_kebab {
        mode_toggles_row = mode_toggles_row.push(mode_toggle_button(
            "assets/icons/cookie.svg",
            PlayerBarMessage::ToggleConsume,
            is_consume_mode,
            consume_tooltip,
        ));
    }
    if !eq_in_kebab {
        // EQ inline button — flat text-labeled toggle.
        mode_toggles_row = mode_toggles_row.push(mode_text_toggle(
            "EQ",
            PlayerBarMessage::ToggleEq,
            eq_enabled,
            eq_tooltip,
        ));
    }
    if !sfx_in_kebab && sound_effects_enabled {
        // SFX inline button — flat text-labeled toggle. Only renders when
        // SFX is on AND not yet folded into the kebab.
        mode_toggles_row = mode_toggles_row.push(mode_text_toggle(
            "SFX",
            PlayerBarMessage::ToggleSoundEffects,
            true,
            "Sound Effects: UI sounds enabled",
        ));
    }
    if !crossfade_in_kebab {
        mode_toggles_row = mode_toggles_row.push(mode_toggle_button(
            "assets/icons/blend.svg",
            PlayerBarMessage::ToggleCrossfade,
            data.crossfade_enabled,
            crossfade_tooltip,
        ));
    }
    if !visualizer_in_kebab {
        mode_toggles_row = mode_toggles_row.push(mode_toggle_button(
            vis_icon,
            PlayerBarMessage::CycleVisualization,
            vis_active,
            visualizer_tooltip,
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
        let audio_group_has_items =
            crossfade_in_kebab || eq_in_kebab || visualizer_in_kebab || sfx_in_kebab;

        let mut kebab_rows = Vec::with_capacity(layout.kebab_mode_count as usize + 1);
        if shuffle_in_kebab {
            kebab_rows.push(mode_menu_item(
                shuffle_menu_label,
                is_random_mode,
                PlayerBarMessage::ToggleRandom,
            ));
        }
        if repeat_in_kebab {
            kebab_rows.push(mode_menu_item(
                repeat_menu_label,
                repeat_active,
                PlayerBarMessage::ToggleRepeat,
            ));
        }
        if consume_in_kebab {
            kebab_rows.push(mode_menu_item(
                consume_menu_label,
                is_consume_mode,
                PlayerBarMessage::ToggleConsume,
            ));
        }
        if queue_group_has_items && audio_group_has_items {
            kebab_rows.push(mode_menu_separator());
        }
        if crossfade_in_kebab {
            kebab_rows.push(mode_menu_item(
                crossfade_menu_label,
                data.crossfade_enabled,
                PlayerBarMessage::ToggleCrossfade,
            ));
        }
        if eq_in_kebab {
            kebab_rows.push(mode_menu_item(
                eq_menu_label,
                eq_enabled,
                PlayerBarMessage::ToggleEq,
            ));
        }
        if visualizer_in_kebab {
            kebab_rows.push(mode_menu_item(
                visualizer_menu_label,
                vis_active,
                PlayerBarMessage::CycleVisualization,
            ));
        }
        if sfx_in_kebab {
            kebab_rows.push(mode_menu_item(
                sfx_menu_label,
                sound_effects_enabled,
                PlayerBarMessage::ToggleSoundEffects,
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
            .border_radius(theme::ui_radius_sm()),
        ));
    }

    // Application menu — only visible in NavLayout::None (no nav chrome of
    // any kind). Top has the hamburger in the top nav bar; Side has it in
    // the side nav column.
    if crate::theme::is_none_nav() {
        use crate::widgets::hamburger_menu::{HamburgerMenu, MenuAction};
        let is_light = data.is_light_mode;
        let hamburger_open = data.hamburger_open;
        mode_toggles_row = mode_toggles_row.push(Element::from(
            HoverOverlay::new(
                HamburgerMenu::new(
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
                .player_bar_style(),
            )
            .border_radius(theme::ui_radius_sm()),
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
    let outer_padding = if theme::is_rounded_mode() {
        if is_mini_player_mode {
            MINI_PLAYER_ROUNDED_PADDING
        } else {
            [10, 12]
        }
    } else {
        [0, 6]
    };

    // Wrap each non-progress section in a Length::Fixed container sized for
    // its *currently-visible* widgets, so the progress track flexes into the
    // rest of the row. Sections still shift at cull / SFX-gate boundaries —
    // those breakpoints already carry hysteresis, so the shift is a single
    // user-visible event tied to a window-size change. Transport content
    // sits flush-left (anchors `prev` to the bar's left edge); modes and
    // volume content sit flush-right (anchors the kebab/hamburger and music
    // slider to the bar's right edge).
    let has_hamburger = crate::theme::is_none_nav();
    let mode_toggles = container(mode_toggles)
        .width(Length::Fixed(mode_section_width(
            data.layout,
            has_hamburger,
        )))
        .height(Length::Fill)
        .align_x(Alignment::End)
        .center_y(Length::Fill);
    let volume_control = container(volume_control)
        .width(Length::Fixed(volume_section_width(show_sfx_slider)))
        .height(Length::Fill)
        .align_x(Alignment::End)
        .center_y(Length::Fill);

    // Progress-track section (artwork + 3-line metadata column) — only
    // present when the user has picked `TrackInfoDisplay::MiniPlayer`
    // AND the window is wide enough that adding the section doesn't crush
    // the progress bar.
    let mini_player_visible = show_mini_player_section(data.window_width);
    let mini_player_element = mini_player_visible.then(|| {
        container(mini_player_section(data))
            .width(Length::Fixed(MINI_PLAYER_SECTION_WIDTH))
            .height(Length::Fill)
            .align_x(Alignment::Start)
            .center_y(Length::Fill)
    });

    let mut main_row = iced::widget::Row::new()
        .spacing(MAIN_ROW_INNER_GAP)
        .padding(outer_padding)
        .align_y(Alignment::Center);
    if let Some(section) = mini_player_element {
        main_row = main_row.push(section);
    }
    if is_mini_player_mode {
        // MiniPlayer mode: transports sit centered above the scrub inside a
        // single Length::Fill column, so the transports no longer claim
        // their own fixed-width section of the main row. Combined with the
        // smaller 28 px transport buttons that fit the existing bar height,
        // that lets MINI_PLAYER_HIDE_BELOW drop well under the previous
        // 760 px threshold.
        let transports_centered = container(player_controls)
            .width(Length::Fill)
            .align_x(Alignment::Center);
        let stacked = iced::widget::Column::new()
            .push(transports_centered)
            .push(progress_row)
            .spacing(MINI_PLAYER_STACK_SPACING)
            .width(Length::Fill);
        main_row = main_row.push(stacked);
    } else {
        let transports_section = container(player_controls)
            .width(Length::Fixed(transport_section_width(
                data.layout.transports_collapsed,
            )))
            .height(Length::Fill)
            .align_x(Alignment::Start)
            .center_y(Length::Fill);
        main_row = main_row.push(transports_section).push(progress_row);
    }
    let main_row = main_row.push(mode_toggles).push(volume_control);

    let main_content: Element<'_, PlayerBarMessage> = if let Some(strip) = info_strip {
        // --- TRACK DISPLAY MODE ---
        // Main row on top, info strip below (separator built into the strip).
        column![
            container(main_row)
                .width(Length::Fill)
                .height(Length::Fixed(base_height - 1.0))
                .center_y(Length::Fill),
            strip,
        ]
        .into()
    } else {
        // --- NORMAL MODE ---
        main_row.into()
    };

    // Top separator: flat 1 px `theme::border()` line. Acts as the chrome
    // divider between the page content above and the player bar.
    let top_separator: Element<'_, PlayerBarMessage> = container(iced::widget::Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|_: &Theme| container::Style {
            background: Some(theme::border().into()),
            ..Default::default()
        })
        .into();

    // Container with `bg0_hard()` background, top separator, and dynamic height.
    // Wrapped in mouse_area so scrolling anywhere on the player bar adjusts
    // volume.
    let bar = container(column![
        top_separator,
        container(main_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_y(Length::Fill)
            .style(theme::container_bg0_hard),
    ])
    .height(Length::Fixed(player_bar_height()));

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
mod section_width_tests {
    use super::{
        CULL_ORDER, PlayerBarLayout, SECTION_BUTTON_GAP, TRANSPORT_SIZE, mode_button_width,
        mode_section_width, transport_section_width, volume_section_width,
    };

    fn layout(kebab: u8, transports_collapsed: bool) -> PlayerBarLayout {
        PlayerBarLayout {
            kebab_mode_count: kebab,
            transports_collapsed,
        }
    }

    #[test]
    fn transport_width_tracks_collapsed_state() {
        // 5 buttons × 40 + 4 gaps × 4 = 216 (uncollapsed)
        assert_eq!(
            transport_section_width(false),
            5.0 * TRANSPORT_SIZE + 4.0 * SECTION_BUTTON_GAP
        );
        assert_eq!(transport_section_width(false), 216.0);
        // 3 buttons × 40 + 2 gaps × 4 = 128 (collapsed)
        assert_eq!(
            transport_section_width(true),
            3.0 * TRANSPORT_SIZE + 2.0 * SECTION_BUTTON_GAP
        );
        assert_eq!(transport_section_width(true), 128.0);
    }

    #[test]
    fn mode_width_tracks_inline_count_and_kebab() {
        let mode_btn_w = mode_button_width();
        let chrome_w = crate::widgets::sizes::TOOLBAR_BUTTON_SIZE;
        let total_modes = CULL_ORDER.len() as f32;

        // All inline (kebab_count=0): 7 buttons + 6 gaps, no kebab.
        let all_inline = mode_section_width(layout(0, false), false);
        assert_eq!(
            all_inline,
            total_modes * mode_btn_w + (total_modes - 1.0) * SECTION_BUTTON_GAP
        );

        // Some culled (kebab_count=5): 2 inline + kebab + 2 gaps.
        let some_culled = mode_section_width(layout(5, false), false);
        assert_eq!(
            some_culled,
            2.0 * mode_btn_w + chrome_w + 2.0 * SECTION_BUTTON_GAP
        );

        // Hamburger adds one more button + gap.
        let with_hamburger = mode_section_width(layout(5, false), true);
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
        CULL_ENTER_WIDTHS, CULL_HYSTERESIS_PX, ModeId, PlayerBarLayout, TRANSPORT_COLLAPSE_ENTER,
        TRANSPORT_COLLAPSE_EXIT, compute_layout,
    };

    fn empty() -> PlayerBarLayout {
        PlayerBarLayout::default()
    }

    fn layout(count: u8, transports: bool) -> PlayerBarLayout {
        PlayerBarLayout {
            kebab_mode_count: count,
            transports_collapsed: transports,
        }
    }

    // ---- mode culling ----

    #[test]
    fn wide_width_keeps_all_modes_inline() {
        // Far above any threshold — no culling.
        let result = compute_layout(1200.0, empty());
        assert_eq!(result.kebab_mode_count, 0);
        assert!(!result.transports_collapsed);
    }

    #[test]
    fn at_exact_first_threshold_no_culling() {
        // Visualizer enters when width *strictly* < threshold[0], so a width
        // sitting exactly on the threshold leaves it inline.
        let result = compute_layout(CULL_ENTER_WIDTHS[0], empty());
        assert_eq!(result.kebab_mode_count, 0);
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
    fn extremely_narrow_width_culls_all_seven_modes() {
        let result = compute_layout(100.0, empty());
        assert_eq!(result.kebab_mode_count, CULL_ENTER_WIDTHS.len() as u8);
    }

    // ---- mode hysteresis ----

    #[test]
    fn culled_mode_stays_culled_inside_hysteresis_band() {
        // Visualizer was culled at < threshold[0]; pops out only once width
        // reaches threshold[0] + hysteresis. One pixel inside the band, the
        // count stays at 1.
        let prev = layout(1, false);
        let inside_band = CULL_ENTER_WIDTHS[0] + CULL_HYSTERESIS_PX - 1.0;
        assert_eq!(compute_layout(inside_band, prev).kebab_mode_count, 1);
    }

    #[test]
    fn culled_mode_pops_inline_at_exit_threshold() {
        // Width hits threshold[0] + hysteresis exactly → visualizer pops back
        // inline.
        let prev = layout(1, false);
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
            let prev = layout(count_before, false);
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
        assert!(result.transports_collapsed);
    }

    #[test]
    fn jump_from_narrow_to_wide_pops_all_modes_back_inline() {
        let prev = layout(7, true);
        let result = compute_layout(1200.0, prev);
        assert_eq!(result.kebab_mode_count, 0);
        assert!(!result.transports_collapsed);
    }

    // ---- transport collapse ----

    #[test]
    fn transport_collapses_just_below_enter_threshold() {
        let result = compute_layout(TRANSPORT_COLLAPSE_ENTER - 1.0, empty());
        assert!(result.transports_collapsed);
    }

    #[test]
    fn transport_does_not_collapse_at_exact_enter_threshold() {
        // Strictly less-than is the trigger; exactly-720 leaves transports
        // expanded.
        let result = compute_layout(TRANSPORT_COLLAPSE_ENTER, empty());
        assert!(!result.transports_collapsed);
    }

    #[test]
    fn transport_collapse_holds_inside_hysteresis_band() {
        let prev = layout(0, true);
        let result = compute_layout(TRANSPORT_COLLAPSE_EXIT - 1.0, prev);
        assert!(result.transports_collapsed);
    }

    #[test]
    fn transport_expands_at_exit_threshold() {
        let prev = layout(0, true);
        let result = compute_layout(TRANSPORT_COLLAPSE_EXIT, prev);
        assert!(!result.transports_collapsed);
    }

    #[test]
    fn transport_collapse_independent_of_mode_culling() {
        // Pick a width that's below the transport-collapse enter threshold
        // AND below the EQ threshold (so EQ is in the kebab) but above the
        // Consume threshold (so Consume is still inline). That leaves the
        // first 4 modes (Visualizer/Crossfade/SFX/EQ) folded — proving the
        // two systems run independently of each other.
        let width = (TRANSPORT_COLLAPSE_ENTER - 1.0).min(CULL_ENTER_WIDTHS[3] - 1.0);
        debug_assert!(width >= CULL_ENTER_WIDTHS[4]);
        let result = compute_layout(width, empty());
        assert_eq!(result.kebab_mode_count, 4);
        assert!(result.transports_collapsed);
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
    fn is_in_kebab_first_culled_is_visualizer() {
        let l = layout(1, false);
        assert!(l.is_in_kebab(ModeId::Visualizer));
        assert!(!l.is_in_kebab(ModeId::Crossfade));
        assert!(!l.is_in_kebab(ModeId::Repeat));
    }

    #[test]
    fn is_in_kebab_all_modes_at_full_count() {
        let l = layout(7, true);
        for mode in [
            ModeId::Visualizer,
            ModeId::Crossfade,
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
