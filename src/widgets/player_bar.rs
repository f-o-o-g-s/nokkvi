//! Player Bar Component
//!
//! Self-contained player controls bar with message bubbling pattern.
//! Receives pure view data and emits actions for root to process.

use iced::{
    Alignment, Element, Length,
    font::{Font, Weight},
    mouse::ScrollDelta,
    widget::{column, container, mouse_area, row, space, text, tooltip},
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

// Responsive breakpoints for element culling (in pixels)
// These are cumulative - at each narrower breakpoint, more elements are hidden
const BREAKPOINT_HIDE_VISUALIZER: f32 = 920.0; // Hide visualizer button
const BREAKPOINT_HIDE_SFX_SLIDER: f32 = 840.0; // Hide SFX volume slider
const BREAKPOINT_HIDE_CONSUME: f32 = 680.0; // Hide consume button
const BREAKPOINT_HIDE_SHUFFLE: f32 = 600.0; // Hide shuffle button
const BREAKPOINT_HIDE_REPEAT: f32 = 520.0; // Hide repeat button

/// Pure view data passed from root (no direct VM access)
#[derive(Debug, Clone)]
pub(crate) struct PlayerBarViewData {
    pub playback_position: u32,
    pub playback_duration: u32,
    pub playback_playing: bool,
    pub playback_paused: bool, // Distinguish paused from stopped
    pub volume: f32,
    pub show_volume_percentage: bool,
    pub has_queue: bool,
    pub show_sfx_volume_percentage: bool,
    // Mode states
    pub is_random_mode: bool,
    pub is_repeat_mode: bool,
    pub is_repeat_queue_mode: bool,
    pub is_consume_mode: bool,
    pub sound_effects_enabled: bool,
    pub sfx_volume: f32, // 0.0-1.0 for sound effects volume
    pub visualization_mode: nokkvi_data::types::player_settings::VisualizationMode,
    pub window_width: f32,
    pub is_light_mode: bool,
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
    ToggleSoundEffects,
    SfxVolumeChanged(f32),
    CycleVisualization,
    ScrollVolume(f32),
    OpenSettings,
    ToggleLightMode,
    GoToQueue,
    /// Track info strip was clicked — dispatch depends on strip_click_action setting
    StripClicked,
    StripContextAction(super::context_menu::StripContextEntry),
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
    let has_queue = data.has_queue;
    let playback_playing = data.playback_playing;
    let playback_paused = data.playback_paused;

    let player_controls = row![
        // Previous track
        player_control_button(
            "assets/icons/skip-back.svg",
            PlayerBarMessage::PrevTrack,
            theme::bg1(),
            if has_queue {
                theme::fg1()
            } else {
                theme::fg4()
            },
            false
        ),
        // Play
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
            playback_playing
        ),
        // Pause
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
            playback_paused
        ),
        // Stop
        player_control_button(
            "assets/icons/stop.svg",
            PlayerBarMessage::Stop,
            theme::bg1(),
            if has_queue {
                theme::fg1()
            } else {
                theme::fg4()
            },
            false
        ),
        // Next track
        player_control_button(
            "assets/icons/skip-forward.svg",
            PlayerBarMessage::NextTrack,
            theme::bg1(),
            if has_queue {
                theme::fg1()
            } else {
                theme::fg4()
            },
            false
        ),
    ]
    .spacing(4);

    // Progress bar section
    let pos_str = format!(
        "{}:{:02}",
        data.playback_position / 60,
        data.playback_position % 60
    );
    let dur_str = format!(
        "{}:{:02}",
        data.playback_duration / 60,
        data.playback_duration % 60
    );

    // Use custom progress bar widget with exact 3D styling and click/drag seek
    let duration = data.playback_duration as f32;
    let position = data.playback_position as f32;

    let custom_progress_bar =
        widgets::progress_bar::progress_bar(position, duration, PlayerBarMessage::Seek)
            .is_playing(data.playback_playing && !data.playback_paused)
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

    // Responsive visibility flags based on window width
    let show_visualizer = window_width >= BREAKPOINT_HIDE_VISUALIZER;
    let show_sfx_slider = window_width >= BREAKPOINT_HIDE_SFX_SLIDER;
    let show_consume = window_width >= BREAKPOINT_HIDE_CONSUME;
    let show_shuffle = window_width >= BREAKPOINT_HIDE_SHUFFLE;
    let show_repeat = window_width >= BREAKPOINT_HIDE_REPEAT;

    // Build mode toggles row dynamically based on visibility
    let mut mode_toggles_row = iced::widget::Row::new().spacing(4);

    // Repeat button
    if show_repeat {
        let repeat_icon = if is_repeat_queue_mode {
            "assets/icons/repeat-2.svg"
        } else {
            "assets/icons/repeat-1.svg"
        };
        let repeat_label = if is_repeat_queue_mode {
            "Repeat Queue: Restart queue when it ends"
        } else if is_repeat_mode {
            "Repeat Track: Loop the current track"
        } else {
            "Repeat: Off"
        };
        mode_toggles_row = mode_toggles_row.push(mode_toggle_button(
            repeat_icon,
            PlayerBarMessage::ToggleRepeat,
            repeat_active,
            repeat_label,
        ));
    }

    // Shuffle button
    if show_shuffle {
        let shuffle_label = if is_random_mode {
            "Shuffle: Playing in random order"
        } else {
            "Shuffle: Off"
        };
        mode_toggles_row = mode_toggles_row.push(mode_toggle_button(
            "assets/icons/shuffle.svg",
            PlayerBarMessage::ToggleRandom,
            is_random_mode,
            shuffle_label,
        ));
    }

    // Consume button
    if show_consume {
        let consume_label = if is_consume_mode {
            "Consume: Tracks removed from queue after playing"
        } else {
            "Consume: Off"
        };
        mode_toggles_row = mode_toggles_row.push(mode_toggle_button(
            "assets/icons/cookie.svg",
            PlayerBarMessage::ToggleConsume,
            is_consume_mode,
            consume_label,
        ));
    }

    // SFX toggle button
    if sound_effects_enabled {
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

    // Visualizer button
    if show_visualizer {
        let vis_label = match visualization_mode {
            VisualizationMode::Off => "Visualizer: Off",
            VisualizationMode::Lines => "Visualizer: Waveform",
            VisualizationMode::Bars => "Visualizer: Bars",
        };
        mode_toggles_row = mode_toggles_row.push(mode_toggle_button(
            vis_icon,
            PlayerBarMessage::CycleVisualization,
            vis_active,
            vis_label,
        ));
    }

    // Application menu — always visible in side nav mode
    if crate::theme::is_side_nav() {
        use crate::widgets::hamburger_menu::{HamburgerMenu, MenuAction};
        let is_light = data.is_light_mode;
        let sfx_on = sound_effects_enabled;
        mode_toggles_row = mode_toggles_row.push(Element::from(HoverOverlay::new(
            HamburgerMenu::new(
                |action| match action {
                    MenuAction::ToggleLightMode => PlayerBarMessage::ToggleLightMode,
                    MenuAction::ToggleSoundEffects => PlayerBarMessage::ToggleSoundEffects,
                    MenuAction::OpenSettings => PlayerBarMessage::OpenSettings,
                    MenuAction::Quit => PlayerBarMessage::Quit,
                },
                is_light,
                sfx_on,
            )
            .player_bar_style(),
        )));
    }

    let mode_toggles = mode_toggles_row;

    // Volume control - horizontal layout with conditional sfx visibility
    // SFX slider is also hidden at narrow widths (show_sfx_slider flag)
    //
    // Volume percentages are shown in the tooltip label (not as separate widgets)
    // to keep the widget tree stable during slider drags. Adding/removing/resizing
    // percentage text widgets shifts child indices and causes Iced to reset the
    // slider's State (including is_dragging) mid-drag. Iced's Row::push also
    // skips void-sized children, so zero-width text tricks don't work either.
    let volume = data.volume;

    // Tooltip labels: show percentage while adjusting, otherwise show descriptive label
    let vol_label: String = if data.show_volume_percentage {
        format!("{}%", (volume * 100.0) as u32)
    } else {
        "Volume".to_string()
    };
    let sfx_label: String = if data.show_sfx_volume_percentage {
        format!("{}%", (sfx_volume * 100.0) as u32)
    } else {
        "SFX Volume".to_string()
    };

    // Tooltip-wrapped volume slider helpers
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
    let vol_slider = tooltip(
        vol,
        container(text(vol_label).size(11.0).font(theme::ui_font())).padding(4),
        tooltip::Position::Top,
    )
    .gap(4)
    .style(theme::container_tooltip);

    let mut sfx = widgets::volume_slider(sfx_volume, PlayerBarMessage::SfxVolumeChanged)
        .variant(widgets::SliderVariant::Sfx)
        .horizontal(is_horizontal);
    if stacked {
        sfx = sfx.thickness(stacked_thickness);
    }
    let sfx_slider = tooltip(
        sfx,
        container(text(sfx_label).size(11.0).font(theme::ui_font())).padding(4),
        tooltip::Position::Top,
    )
    .gap(4)
    .style(theme::container_tooltip);

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
        let main_row = row![
            player_controls,
            progress_row,
            space().width(Length::Fixed(4.0)),
            mode_toggles,
            volume_control,
        ]
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
        row![
            player_controls,
            progress_row,
            space().width(Length::Fixed(4.0)),
            mode_toggles,
            volume_control,
        ]
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
