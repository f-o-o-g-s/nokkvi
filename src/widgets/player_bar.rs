//! Player Bar Component
//!
//! Self-contained player controls bar with message bubbling pattern.
//! Receives pure view data and emits actions for root to process.

use iced::{
    Alignment, Element, Length, Theme,
    font::{Font, Weight},
    mouse::ScrollDelta,
    widget::{column, container, mouse_area, row, text, tooltip},
};

use crate::{
    theme, widgets,
    widgets::{
        hover_overlay::HoverOverlay, three_d_button::three_d_button,
        three_d_icon_button::three_d_icon_button,
    },
};

// Player bar dimensions
const BASE_PLAYER_BAR_HEIGHT: f32 = 56.0;
const BUTTON_SIZE: f32 = 44.0;
const CONTROL_ROW_HEIGHT: f32 = 44.0;
/// Height of the track info strip below the player bar in PlayerBar display mode.
/// Re-uses the canonical constant from `track_info_strip.rs` to avoid drift.
use super::track_info_strip::STRIP_HEIGHT as INFO_STRIP_HEIGHT;

/// Volume change per scroll line (e.g. mouse wheel notch)
const SCROLL_VOLUME_STEP_LINES: f32 = 0.01;
/// Volume change per scroll pixel (e.g. trackpad smooth scrolling)
const SCROLL_VOLUME_STEP_PIXELS: f32 = 0.001;

/// Dynamic player bar height: base 56px, plus info strip when track display is PlayerBar
/// and nav layout is Side (in Top mode the nav bar already shows track/format info).
pub(crate) fn player_bar_height() -> f32 {
    if crate::theme::show_player_bar_strip() {
        BASE_PLAYER_BAR_HEIGHT + INFO_STRIP_HEIGHT
    } else {
        BASE_PLAYER_BAR_HEIGHT
    }
}

// Format-info container is text-only; hide it as a single threshold without
// hysteresis since collapsing text doesn't shift button hit targets.
const BREAKPOINT_HIDE_FORMAT_INFO: f32 = 1000.0;
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
    // Track metadata for progress bar overlay
    pub track_title: String,
    pub track_artist: String,
    pub track_album: String,
    pub format_suffix: String,
    pub sample_rate: u32,
    pub bitrate: u32,
    pub radio_name: Option<String>,
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

/// Helper function to create a player control button with standard sizing,
/// wrapped in a `HoverOverlay` for consistent hover/press feedback.
fn player_control_button(
    icon_path: &'static str,
    message: PlayerBarMessage,
    background: iced::Color,
    icon_color: iced::Color,
    active: bool,
) -> Element<'static, PlayerBarMessage> {
    HoverOverlay::new(
        three_d_icon_button(icon_path)
            .on_press(message)
            .width(BUTTON_SIZE)
            .height(BUTTON_SIZE)
            .background(background)
            .icon_color(icon_color)
            .active(active),
    )
    .into()
}

/// Build a mode toggle button with tooltip — shared pattern for repeat, shuffle,
/// consume, and visualizer toggles. The inner `player_control_button` already
/// wraps in `HoverOverlay`, so this function only adds the tooltip layer.
fn mode_toggle_button<'a>(
    icon_path: &'static str,
    message: PlayerBarMessage,
    active: bool,
    label: &'a str,
) -> Element<'a, PlayerBarMessage> {
    tooltip(
        player_control_button(
            icon_path,
            message,
            if active {
                theme::accent_bright()
            } else {
                theme::bg1()
            },
            if active {
                theme::bg0_hard()
            } else {
                theme::fg1()
            },
            active,
        ),
        container(text(label).size(11.0).font(theme::ui_font())).padding(4),
        tooltip::Position::Top,
    )
    .gap(4)
    .style(theme::container_tooltip)
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

    let prev_button = player_control_button(
        "assets/icons/skip-back.svg",
        PlayerBarMessage::PrevTrack,
        theme::bg1(),
        if controls_active {
            theme::fg1()
        } else {
            theme::fg4()
        },
        false,
    );
    let next_button = player_control_button(
        "assets/icons/skip-forward.svg",
        PlayerBarMessage::NextTrack,
        theme::bg1(),
        if controls_active {
            theme::fg1()
        } else {
            theme::fg4()
        },
        false,
    );

    let player_controls: Element<'_, PlayerBarMessage> = if data.layout.transports_collapsed {
        // Collapsed transports: prev / play-or-pause toggle / next.
        // BUTTON_SIZE is fixed (44px) so the middle button's hit target stays
        // in place when the glyph swaps between play and pause.
        let middle_active = playback_playing || playback_paused;
        let (middle_icon, middle_message) = if playback_playing {
            ("assets/icons/pause.svg", PlayerBarMessage::Pause)
        } else {
            ("assets/icons/play.svg", PlayerBarMessage::Play)
        };
        row![
            prev_button,
            player_control_button(
                middle_icon,
                middle_message,
                if middle_active {
                    theme::accent_bright()
                } else {
                    theme::bg1()
                },
                if middle_active {
                    theme::bg0_hard()
                } else {
                    theme::fg1()
                },
                middle_active,
            ),
            next_button,
        ]
        .spacing(4)
        .into()
    } else {
        row![
            prev_button,
            player_control_button(
                "assets/icons/play.svg",
                PlayerBarMessage::Play,
                if playback_playing {
                    theme::accent_bright()
                } else {
                    theme::bg1()
                },
                if playback_playing {
                    theme::bg0_hard()
                } else {
                    theme::fg1()
                },
                playback_playing,
            ),
            player_control_button(
                "assets/icons/pause.svg",
                PlayerBarMessage::Pause,
                if playback_paused {
                    theme::accent_bright()
                } else {
                    theme::bg1()
                },
                if playback_paused {
                    theme::bg0_hard()
                } else {
                    theme::fg1()
                },
                playback_paused,
            ),
            player_control_button(
                "assets/icons/stop.svg",
                PlayerBarMessage::Stop,
                theme::bg1(),
                if controls_active {
                    theme::fg1()
                } else {
                    theme::fg4()
                },
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

    // Build metadata for scrolling overlay on the progress bar track
    // Colored segments matching the track info strip: title/artist/album each get their own color
    use super::progress_bar::OverlaySegment;
    let mut meta_segments: Vec<OverlaySegment> = Vec::new();
    let mut format_info_left = String::new();
    let mut format_info_right = String::new();

    if !data.track_title.is_empty()
        && crate::theme::track_info_display()
            == nokkvi_data::types::player_settings::TrackInfoDisplay::ProgressTrack
    {
        let separator = crate::theme::strip_separator().as_join_str();
        let segments = super::track_info_strip::build_now_playing_segments(
            &data.track_title,
            &data.track_artist,
            &data.track_album,
            crate::theme::strip_show_title(),
            crate::theme::strip_show_artist(),
            crate::theme::strip_show_album(),
            crate::theme::strip_show_labels(),
            separator,
        );
        meta_segments.extend(segments.into_iter().map(|s| OverlaySegment {
            text: s.text,
            color: s.color,
        }));

        if let Some(rname) = &data.radio_name {
            if !meta_segments.is_empty() {
                meta_segments.push(OverlaySegment {
                    text: separator.to_string(),
                    color: theme::fg4(),
                });
            }
            meta_segments.push(OverlaySegment {
                text: format!("{rname} (LIVE)"),
                color: theme::accent_bright(),
            });
        }

        if crate::theme::strip_show_format_info()
            && let Some((left, right)) = super::format_info::format_audio_info_split(
                &data.format_suffix,
                data.sample_rate as f32 / 1000.0,
                data.bitrate,
            )
        {
            format_info_left = left;
            if let Some(r) = right {
                format_info_right = r;
            }
        }
    }

    let mut custom_progress_bar =
        widgets::progress_bar::progress_bar(position, duration, PlayerBarMessage::Seek)
            .is_playing(data.playback_playing && !data.playback_paused)
            .hide_handle(data.is_radio)
            .width(Length::Fill)
            .height(24.0);
    if !meta_segments.is_empty() {
        custom_progress_bar = custom_progress_bar.overlay_segments(meta_segments);
    }

    let mut progress_items: Vec<Element<'_, PlayerBarMessage>> = vec![
        text(pos_str.clone())
            .size(11.0)
            .font(theme::ui_font())
            .color(theme::fg4())
            .width(Length::Fixed(40.0))
            .align_x(Alignment::End)
            .align_y(Alignment::Center)
            .into(),
        custom_progress_bar.into(),
        text(dur_str.clone())
            .size(11.0)
            .font(theme::ui_font())
            .color(theme::fg4())
            .width(Length::Fixed(40.0))
            .align_y(Alignment::Center)
            .into(),
    ];
    if !format_info_left.is_empty() && data.window_width >= BREAKPOINT_HIDE_FORMAT_INFO {
        let mut col_items: Vec<Element<'_, PlayerBarMessage>> = vec![
            text(format_info_left)
                .size(8.0)
                .font(theme::ui_font())
                .color(theme::fg4())
                .wrapping(text::Wrapping::None)
                .align_x(Alignment::Center)
                .into(),
        ];
        if !format_info_right.is_empty() {
            col_items.push(
                text(format_info_right)
                    .size(8.0)
                    .font(theme::ui_font())
                    .color(theme::fg4())
                    .wrapping(text::Wrapping::None)
                    .align_x(Alignment::Center)
                    .into(),
            );
        }
        let format_col = container(column(col_items).align_x(Alignment::Center).spacing(0))
            .style(|_: &Theme| {
                let (inset_tl, _) = theme::border_3d_inset();
                container::Style {
                    background: Some(theme::bg1().into()),
                    border: iced::Border {
                        color: inset_tl,
                        width: 1.0,
                        radius: theme::ui_border_radius(),
                    },
                    ..Default::default()
                }
            })
            .padding([0, 6])
            .center_y(BUTTON_SIZE)
            .align_x(Alignment::Center);
        progress_items.push(format_col.into());
    }

    let progress_row = row(progress_items)
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
        // EQ inline button — text-styled 3D button (not icon-based).
        mode_toggles_row = mode_toggles_row.push(Element::from(HoverOverlay::new(
            tooltip(
                three_d_button(
                    container(text("EQ").size(10.0).font(Font {
                        weight: Weight::Bold,
                        ..theme::ui_font()
                    }))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
                )
                .on_press(PlayerBarMessage::ToggleEq)
                .width(Length::Fixed(BUTTON_SIZE))
                .height(BUTTON_SIZE)
                .background(if eq_enabled {
                    theme::accent_bright()
                } else {
                    theme::bg1()
                })
                .active(eq_enabled),
                container(text(eq_tooltip).size(11.0).font(theme::ui_font())).padding(4),
                tooltip::Position::Top,
            )
            .gap(4)
            .style(theme::container_tooltip),
        )));
    }
    if !sfx_in_kebab && sound_effects_enabled {
        // SFX inline button — text-styled 3D button. Only renders when SFX
        // is on AND not yet folded into the kebab.
        mode_toggles_row = mode_toggles_row.push(Element::from(HoverOverlay::new(
            tooltip(
                three_d_button(
                    container(text("SFX").size(10.0).font(Font {
                        weight: Weight::Bold,
                        ..theme::ui_font()
                    }))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
                )
                .on_press(PlayerBarMessage::ToggleSoundEffects)
                .width(Length::Fixed(BUTTON_SIZE))
                .height(BUTTON_SIZE)
                .background(theme::accent_bright())
                .active(true),
                container(
                    text("Sound Effects: UI sounds enabled")
                        .size(11.0)
                        .font(theme::ui_font()),
                )
                .padding(4),
                tooltip::Position::Top,
            )
            .gap(4)
            .style(theme::container_tooltip),
        )));
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

        mode_toggles_row =
            mode_toggles_row.push(Element::from(HoverOverlay::new(PlayerModesMenu::new(
                kebab_rows,
                |open| {
                    PlayerBarMessage::SetOpenMenu(
                        open.then_some(crate::app_message::OpenMenu::PlayerModes),
                    )
                },
                data.player_modes_open,
            ))));
    }

    // Application menu — always visible when there is no top nav bar
    // (i.e. in side and none layouts), since otherwise the user has no way to
    // reach Settings/About/Quit.
    if !crate::theme::is_top_nav() {
        use crate::widgets::hamburger_menu::{HamburgerMenu, MenuAction};
        let is_light = data.is_light_mode;
        let sfx_on = sound_effects_enabled;
        let hamburger_open = data.hamburger_open;
        mode_toggles_row = mode_toggles_row.push(Element::from(HoverOverlay::new(
            HamburgerMenu::new(
                |action| match action {
                    MenuAction::ToggleLightMode => PlayerBarMessage::ToggleLightMode,
                    MenuAction::ToggleSoundEffects => PlayerBarMessage::ToggleSoundEffects,
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
                sfx_on,
            )
            .player_bar_style(),
        )));
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

    let mut vol =
        widgets::volume_slider(volume, PlayerBarMessage::VolumeChanged).horizontal(is_horizontal);
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

    let main_content: Element<'_, PlayerBarMessage> = if let Some(strip) = info_strip {
        // --- TRACK DISPLAY MODE ---
        // Main row: controls + progress bar (same layout as normal mode)
        // Info strip below: pre-built by the caller

        // Main row: controls + progress + toggles + volume
        let main_row = row![player_controls, progress_row, mode_toggles, volume_control,]
            .spacing(4)
            .padding([4, 8])
            .align_y(Alignment::Center);

        column![
            container(main_row)
                .width(Length::Fill)
                .height(Length::Fixed(BASE_PLAYER_BAR_HEIGHT - 2.0))
                .center_y(Length::Fill),
            strip,
        ]
        .into()
    } else {
        // --- NORMAL MODE (unchanged) ---
        row![player_controls, progress_row, mode_toggles, volume_control,]
            .spacing(4)
            .padding([4, 8])
            .into()
    };

    // Top separator: always visible (2px, bg1), matching nav bar / settings separator style.
    // Replaces border_light+border_dark which hide in rounded mode.
    let top_separator: Element<'_, PlayerBarMessage> = theme::horizontal_separator(2.0);

    // Container with BG1 background, top separator, and dynamic height.
    // Wrapped in mouse_area so scrolling anywhere on the player bar adjusts volume.
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
