//! Navigation Bar Component
//!
//! Waybar-style flat navigation bar with three sections:
//! - Left: Navigation tabs (Queue, Albums, etc.) - flat, no 3D effect
//! - Center: Track info text
//! - Right: Audio format info + hamburger menu

use iced::{
    Alignment, Background, Border, Color, Element, Length, Point, Rectangle,
    font::{Font, Weight},
    widget::{Space, button, canvas, column, container, mouse_area, row, text, text::Wrapping},
};
use nokkvi_data::types::player_settings::NavDisplayMode;

use crate::{
    theme,
    widgets::hamburger_menu::{HamburgerMenu, MenuAction},
};

// ============================================================================
// Types & Responsive Breakpoints
// ============================================================================

// Responsive breakpoints — metadata fields collapse progressively (album → artist → title)
const BREAKPOINT_SHOW_ALBUM: f32 = 900.0;
const BREAKPOINT_SHOW_ARTIST: f32 = 750.0;
const BREAKPOINT_SHOW_TITLE: f32 = 600.0;

/// Navigation view options (mirrors app::View)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavView {
    Queue,
    Albums,
    Artists,
    Songs,
    Genres,
    Playlists,
}

/// Pure view data passed from root for nav bar rendering
#[derive(Debug, Clone)]
pub(crate) struct NavBarViewData {
    pub current_view: NavView,
    pub track_title: String,
    pub track_artist: String,
    pub track_album: String,
    /// Whether a track is actively loaded (playing or paused)
    pub is_playing: bool,
    /// Audio format suffix (e.g., "flac", "mp3")
    pub format_suffix: String,
    /// Sample rate in kHz (e.g., 44.1, 48.0, 96.0)
    pub sample_rate_khz: f32,
    /// Bitrate in kbps (e.g., 320, 1411)
    pub bitrate_kbps: u32,
    /// Current window width for responsive breakpoints
    pub window_width: f32,
    /// Current light mode state (for hamburger menu toggle label)
    pub is_light_mode: bool,
    /// Current SFX state (for hamburger menu toggle label)
    pub sound_effects_enabled: bool,
    /// Whether the settings view is currently open (disables nav tab highlighting)
    pub settings_open: bool,
    /// Local music path for "Show in File Manager" (empty = not configured)
    pub local_music_path: String,
    /// Whether the currently playing track is starred
    pub is_current_starred: bool,
}

/// Messages emitted by nav bar interactions
#[derive(Debug, Clone)]
pub enum NavBarMessage {
    SwitchView(NavView),
    ToggleLightMode,
    ToggleSoundEffects,
    OpenSettings,
    /// Track info strip was clicked — dispatch depends on strip_click_action setting
    StripClicked,
    StripContextAction(super::context_menu::StripContextEntry),
    Quit,
}

// ============================================================================
// Navigation Bar Component
// ============================================================================

const NAV_BAR_HEIGHT: f32 = 28.0;

/// Ordered list of navigation tabs — single source of truth shared with `side_nav_bar`.
/// Each entry: (label, icon_path, NavView).
pub(crate) const NAV_TABS: &[(&str, &str, NavView)] = &[
    ("Queue", "assets/icons/list-music.svg", NavView::Queue),
    ("Albums", "assets/icons/disc-3.svg", NavView::Albums),
    ("Artists", "assets/icons/mic.svg", NavView::Artists),
    ("Songs", "assets/icons/music.svg", NavView::Songs),
    ("Genres", "assets/icons/tags.svg", NavView::Genres),
    ("Playlists", "assets/icons/list.svg", NavView::Playlists),
];

/// Flat-mode tab button style (filled accent background when active, bg2 hover).
///
/// Shared between the horizontal nav bar and the vertical side nav bar.
pub(crate) fn flat_tab_style(
    is_active: bool,
) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme: &iced::Theme, status: button::Status| {
        let is_hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: if is_active {
                Some(Background::Color(theme::accent_bright()))
            } else if is_hovered {
                Some(Background::Color(theme::bg2()))
            } else {
                Some(Background::Color(theme::bg0_hard()))
            },
            text_color: if is_active {
                theme::bg0()
            } else {
                theme::fg2()
            },
            border: Border {
                radius: theme::ui_border_radius(),
                ..Default::default()
            },
            ..button::Style::default()
        }
    }
}

/// Build a colored SVG icon widget at the given size.
///
/// Shared helper to eliminate repeated boilerplate across nav bars.
pub(crate) fn colored_icon<'a>(path: &str, size: f32, color: iced::Color) -> iced::widget::Svg<'a> {
    crate::embedded_svg::svg_widget(path)
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_, _| iced::widget::svg::Style { color: Some(color) })
}

/// 2px vertical separator line between tabs.
///
/// In rounded mode separators are hidden by default; pass `force_visible = true`
/// to keep one visible (used for the trailing separator after the last nav tab).
fn tab_separator<'a, M: 'a>(force_visible: bool) -> Element<'a, M> {
    container(Space::new())
        .width(Length::Fixed(2.0))
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: if theme::is_rounded_mode() && !force_visible {
                None
            } else {
                Some(theme::bg1().into())
            },
            ..Default::default()
        })
        .into()
}

/// Height of the rounded underline indicator beneath active/hovered tabs
const UNDERLINE_HEIGHT: f32 = 2.0;

/// Canvas program for the bottom-edge underline indicator in topped nav rounded mode.
/// Expands the cursor detection area upward to cover the button above.
struct UnderlineIndicator {
    /// Active indicator color (always shown)
    indicator_color: Option<Color>,
    /// Hover indicator color (shown on mouse-over when not active)
    hover_indicator_color: Option<Color>,
    /// Expand the cursor detection area upward by this amount
    expand_hover_up: f32,
}

impl<Message> canvas::Program<Message> for UnderlineIndicator {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let show_indicator = self.indicator_color.or_else(|| {
            let hover_area = Rectangle {
                x: bounds.x,
                y: bounds.y - self.expand_hover_up,
                width: bounds.width,
                height: bounds.height + self.expand_hover_up,
            };
            if cursor.position().is_some_and(|p| hover_area.contains(p)) {
                self.hover_indicator_color
            } else {
                None
            }
        });

        let mut frame = canvas::Frame::new(renderer, bounds.size());
        if let Some(accent) = show_indicator {
            // Draw rounded pill shape matching the original container border radius
            let pill = canvas::Path::rectangle(Point::ORIGIN, bounds.size());
            frame.fill(&pill, canvas::Fill::from(accent));
        }
        vec![frame.into_geometry()]
    }
}

/// Build tab content based on display mode (text, icon+text, icon-only).
///
/// Shared between nav tab rendering and the settings indicator.
fn tab_content<'a>(
    label: &'static str,
    icon_path: &'static str,
    display_mode: NavDisplayMode,
    text_color: iced::Color,
) -> Element<'a, NavBarMessage> {
    let icon_size = 14.0;
    match display_mode {
        NavDisplayMode::TextOnly => container(text(label).size(14.0).font(Font {
            weight: Weight::Bold,
            ..theme::ui_font()
        }))
        .width(Length::Shrink)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into(),
        NavDisplayMode::IconsOnly => container(colored_icon(icon_path, icon_size, text_color))
            .width(Length::Shrink)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into(),
        NavDisplayMode::TextAndIcons => container(
            row![
                colored_icon(icon_path, icon_size, text_color),
                text(label).size(14.0).font(Font {
                    weight: Weight::Bold,
                    ..theme::ui_font()
                }),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        )
        .width(Length::Shrink)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into(),
    }
}

/// Build the Waybar-style navigation bar
///
/// Three-section layout:
/// - Left: Flat navigation tabs with active highlight
/// - Center: Track info text
/// - Right: Audio format info
pub(crate) fn nav_bar(data: NavBarViewData) -> Element<'static, NavBarMessage> {
    // -------------------------------------------------------------------------
    // Left Section: Flat Navigation Tabs
    // -------------------------------------------------------------------------
    let settings_open = data.settings_open;
    // Shared tab builder — used for both regular nav tabs AND the settings indicator.
    // `force_active` overrides the active state (used for settings tab, always active).
    let nav_tab = |label: &'static str,
                   icon_path: &'static str,
                   view: NavView,
                   current: NavView,
                   force_active: bool| {
        let is_active = force_active || (!settings_open && current == view);
        let is_rounded = theme::is_rounded_mode();
        let display_mode = theme::nav_display_mode();

        let tab_padding: [u16; 2] = if matches!(display_mode, NavDisplayMode::IconsOnly) {
            [2, 6]
        } else {
            [2, 10]
        };

        if is_rounded {
            // Rounded mode: flat button + static underline indicator below
            let rounded_accent = theme::active_accent();
            let text_color = if is_active {
                rounded_accent
            } else {
                theme::fg2()
            };
            let tab_style = move |_theme: &iced::Theme, status: button::Status| {
                let is_hovered =
                    matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: Some(Background::Color(theme::bg0_hard())),
                    text_color: if is_active {
                        rounded_accent
                    } else if is_hovered {
                        theme::fg1()
                    } else {
                        theme::fg2()
                    },
                    ..button::Style::default()
                }
            };

            // Underline: accent colored for active tab, accent on hover for idle
            let underline_active = if is_active {
                Some(rounded_accent)
            } else {
                None
            };
            let underline_hover = if !is_active {
                Some(rounded_accent)
            } else {
                None
            };

            let elem: Element<'_, NavBarMessage> = column![
                button(tab_content(label, icon_path, display_mode, text_color))
                    .on_press(NavBarMessage::SwitchView(view))
                    .padding(tab_padding)
                    .height(Length::Fill)
                    .style(tab_style),
                canvas(UnderlineIndicator {
                    indicator_color: underline_active,
                    hover_indicator_color: underline_hover,
                    expand_hover_up: 100.0, // Generous expansion to cover the button above
                })
                .width(Length::Fill)
                .height(Length::Fixed(UNDERLINE_HEIGHT)),
            ]
            .spacing(0)
            .width(Length::Shrink)
            .height(Length::Fill)
            .into();
            elem
        } else {
            // Flat mode: filled background button (shared style)
            let tab_style = flat_tab_style(is_active);
            let text_color = if is_active {
                theme::bg0()
            } else {
                theme::fg2()
            };

            button(tab_content(label, icon_path, display_mode, text_color))
                .on_press(NavBarMessage::SwitchView(view))
                .padding(tab_padding)
                .height(Length::Fill)
                .style(tab_style)
                .into()
        }
    };

    let current = data.current_view;
    let is_side_nav = theme::is_side_nav();

    // In Side mode, nav tabs move to the vertical sidebar — hide them here
    let mut left_section: iced::widget::Row<'static, NavBarMessage> = if is_side_nav {
        row![]
            .spacing(0)
            .height(Length::Fill)
            .align_y(Alignment::Center)
    } else {
        let mut tabs = row![tab_separator(false)]
            .spacing(0)
            .height(Length::Fill)
            .align_y(Alignment::Center);
        let tab_count = NAV_TABS.len();
        for (i, &(label, icon_path, view)) in NAV_TABS.iter().enumerate() {
            let is_last = i == tab_count - 1;
            tabs = tabs
                .push(nav_tab(label, icon_path, view, current, false))
                .push(tab_separator(is_last && !settings_open));
        }
        tabs
    };

    // Settings indicator: reuses the same nav_tab builder with force_active=true
    if settings_open && !is_side_nav {
        // Use Queue as a dummy NavView — the on_press emits CloseSettings
        // which restores the pre-settings view instead of navigating to Queue.
        left_section = left_section.push(nav_tab(
            "Settings",
            "assets/icons/settings.svg",
            NavView::Queue,
            current,
            true,
        ));
        left_section = left_section.push(tab_separator(true));
    }

    // Responsive visibility flags — fields collapse progressively.
    // AND with the user's settings toggles so fields are hidden either by
    // narrow window OR by user preference.
    let show_title = data.window_width >= BREAKPOINT_SHOW_TITLE && theme::strip_show_title();
    let show_artist = data.window_width >= BREAKPOINT_SHOW_ARTIST && theme::strip_show_artist();
    let show_album = data.window_width >= BREAKPOINT_SHOW_ALBUM && theme::strip_show_album();

    // Helper: labeled field (dimmed label: + scrolling value) — delegates to shared helper
    let info_field = |label: &'static str,
                      value: String,
                      value_color: iced::Color|
     -> Element<'static, NavBarMessage> {
        super::track_info_strip::info_field_widget(label, value, value_color)
    };

    // -------------------------------------------------------------------------
    // Center Section: Track Info (hidden below breakpoint)
    // -------------------------------------------------------------------------
    // Layout: │ title: xxx │ artist: xxx │ album: xxx │ [fill] │ FLAC 44.1kHz · 1411kbps │
    let is_playing = data.is_playing;

    let center_section: Element<'static, NavBarMessage> =
        if !show_title && !show_artist && !show_album {
            // All metadata hidden (narrow window OR all user toggles off)
            Space::new().width(Length::Fill).into()
        } else if !is_playing {
            // Stopped state - no track loaded
            container(
                text("No track loaded")
                    .size(12.0)
                    .font(Font {
                        weight: Weight::Semibold,
                        ..theme::ui_font()
                    })
                    .color(theme::fg4())
                    .wrapping(Wrapping::None),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
        } else {
            // Playing or paused - build nav-bar-specific track info
            let title = data.track_title.clone();
            let artist = data.track_artist.clone();
            let album = data.track_album.clone();

            let info_sep = || -> Element<'static, NavBarMessage> {
                container(Space::new())
                    .width(Length::Fixed(2.0))
                    .height(Length::Fill)
                    .style(move |_| container::Style {
                        background: Some(theme::bg1().into()),
                        ..Default::default()
                    })
                    .into()
            };

            let mut info_row = iced::widget::Row::new()
                .spacing(6)
                .align_y(Alignment::Center)
                .height(Length::Fill);

            // Fill spacer → center the metadata fields
            info_row = info_row.push(Space::new().width(Length::Fill));

            // Progressive metadata: each field independently toggleable
            let mut has_prev_field = false;

            if show_title {
                info_row = info_row.push(info_sep());
                info_row = info_row.push(info_field("title:", title, theme::now_playing_color()));
                has_prev_field = true;
            }

            if show_artist {
                info_row = info_row.push(info_sep());
                info_row = info_row.push(info_field("artist:", artist, theme::selected_color()));
                has_prev_field = true;
            }

            if show_album {
                info_row = info_row.push(info_sep());
                info_row = info_row.push(info_field("album:", album, theme::fg2()));
                has_prev_field = true;
            }

            if has_prev_field {
                info_row = info_row.push(info_sep());
            }

            // Fill spacer → push format info away
            info_row = info_row.push(Space::new().width(Length::Fill));

            // Wrap in mouse_area so clicking metadata navigates to queue
            let clickable = mouse_area(info_row).on_press(NavBarMessage::StripClicked);
            let has_local_path = !data.local_music_path.is_empty();
            let is_starred = data.is_current_starred;
            let wrapped = super::context_menu::context_menu(
                container(clickable)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_y(Length::Fill),
                super::context_menu::strip_entries(has_local_path),
                move |entry, length| {
                    super::context_menu::strip_entry_view(
                        entry,
                        length,
                        is_starred,
                        NavBarMessage::StripContextAction,
                    )
                },
            );
            wrapped.into()
        };

    // -------------------------------------------------------------------------
    // Format Info (independent of metadata — stays visible at narrow widths)
    // -------------------------------------------------------------------------
    let format_section: Element<'static, NavBarMessage> =
        if is_playing && theme::strip_show_format_info() {
            let format_split = super::format_info::format_audio_info_split(
                &data.format_suffix,
                data.sample_rate_khz,
                data.bitrate_kbps,
            );
            if let Some((left, right)) = format_split {
                let combined = match right {
                    Some(r) => format!("{left} · {r}"),
                    None => left,
                };
                let fmt_sep = || -> Element<'static, NavBarMessage> {
                    container(Space::new())
                        .width(Length::Fixed(2.0))
                        .height(Length::Fill)
                        .style(move |_| container::Style {
                            background: Some(theme::bg1().into()),
                            ..Default::default()
                        })
                        .into()
                };
                row![
                    fmt_sep(),
                    text(combined)
                        .size(9.0)
                        .font(Font {
                            weight: Weight::Medium,
                            ..theme::ui_font()
                        })
                        .color(theme::fg3())
                        .wrapping(Wrapping::None),
                ]
                .spacing(6)
                .align_y(Alignment::Center)
                .height(Length::Fill)
                .padding(iced::Padding {
                    top: 0.0,
                    right: 6.0,
                    bottom: 0.0,
                    left: 0.0,
                })
                .into()
            } else {
                Space::new().width(Length::Shrink).into()
            }
        } else {
            Space::new().width(Length::Shrink).into()
        };

    // -------------------------------------------------------------------------
    // Hamburger Menu (far right)
    // -------------------------------------------------------------------------
    let hamburger: Element<'static, NavBarMessage> = HamburgerMenu::new(
        |action| match action {
            MenuAction::ToggleLightMode => NavBarMessage::ToggleLightMode,
            MenuAction::ToggleSoundEffects => NavBarMessage::ToggleSoundEffects,
            MenuAction::OpenSettings => NavBarMessage::OpenSettings,
            MenuAction::Quit => NavBarMessage::Quit,
        },
        data.is_light_mode,
        data.sound_effects_enabled,
    )
    .into();

    // -------------------------------------------------------------------------
    // Assemble Layout: Tabs | Track Info | Format Info | Hamburger
    // -------------------------------------------------------------------------
    let nav_content = container(
        row![
            // Left: Navigation tabs
            left_section,
            // Center: Track info (collapses at narrow widths)
            center_section,
            // Format info (stays visible independently)
            format_section,
            // Hamburger menu
            tab_separator(false),
            hamburger,
            tab_separator(false),
        ]
        .align_y(Alignment::Center)
        .padding(0)
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::container_bg0_hard);

    // Bottom separator: always visible (2px, bg1), matching settings separator style.
    // Unlike shared border_light/border_dark which hide in rounded mode.
    let bottom_separator: Element<'static, NavBarMessage> = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(2.0))
        .style(move |_| container::Style {
            background: Some(theme::bg1().into()),
            ..Default::default()
        })
        .into();

    container(column![
        crate::widgets::border_light(),
        crate::widgets::border_dark(),
        nav_content,
        bottom_separator,
    ])
    .height(Length::Fixed(NAV_BAR_HEIGHT + 4.0))
    .into()
}
