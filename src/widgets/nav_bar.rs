//! Navigation Bar Component
//!
//! Waybar-style flat navigation bar with three sections:
//! - Left: Hamburger menu, library-filter trigger, navigation tabs (Queue, Albums, etc.) - flat, no 3D effect
//! - Center: Track info text
//! - Right: Audio format info

use iced::{
    Alignment, Background, Border, Element, Length,
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
    Radios,
}

impl NavView {
    pub const ALL: &'static [NavView] = &[
        NavView::Queue,
        NavView::Albums,
        NavView::Artists,
        NavView::Songs,
        NavView::Genres,
        NavView::Playlists,
        NavView::Radios,
    ];
}

const _: [(); 7 - NavView::ALL.len()] = [];
const _: [(); NavView::ALL.len() - 7] = [];

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
    /// Whether the settings view is currently open (disables nav tab highlighting)
    pub settings_open: bool,
    /// Local music path for "Show in File Manager" (empty = not configured)
    pub local_music_path: String,
    /// Whether the currently playing track is starred
    pub is_current_starred: bool,
    pub radio_name: Option<String>,
    pub radio_url: Option<String>,
    pub icy_artist: Option<String>,
    pub icy_title: Option<String>,
    /// Whether the hamburger menu is currently open (controlled state).
    pub hamburger_open: bool,
    /// Whether the now-playing strip's right-click context menu is open
    /// (controlled state).
    pub strip_context_open: bool,
    /// Anchor position for the strip context menu when open.
    pub strip_context_position: Option<iced::Point>,
    /// Total libraries known to the client (the popover's row count).
    /// `<= 1` suppresses the trigger entirely — see the design lock
    /// "Hidden at N ≤ 1" in `libraries_imp_plan.md` §2.
    pub library_count: usize,
    /// Subset of `library_count` currently in the active selection.
    /// `0` (the empty-set-as-all default) and `library_count` render
    /// identically (neutral icon); strict subsets show the count badge
    /// and pip.
    pub active_library_count: usize,
    /// Whether the library-filter popover is currently open (controlled
    /// state — mirrors `Nokkvi.open_menu == Some(OpenMenu::LibrarySelector { .. })`).
    pub library_selector_open: bool,
    /// Trigger bounds captured at open time — re-passed each render so
    /// the popover overlay can anchor below the trigger.
    pub library_selector_bounds: Option<iced::Rectangle>,
    /// Library-filter popover rows: `(id, name, song_count, checked)`.
    /// Sorted alphabetically by name. `song_count` is `Some(n)` when the
    /// admin-only `/api/library` endpoint succeeded; `None` when the
    /// Subsonic `getMusicFolders` fallback was used (non-admin session)
    /// — in that case the popover renders the right column blank rather
    /// than spamming N getSongs requests for per-library counts.
    pub library_rows: Vec<(i32, String, Option<u32>, bool)>,
}

/// Messages emitted by nav bar interactions
#[derive(Debug, Clone)]
pub enum NavBarMessage {
    SwitchView(NavView),
    ToggleLightMode,
    OpenSettings,
    /// Track info strip was clicked — dispatch depends on strip_click_action setting
    StripClicked,
    StripContextAction(super::context_menu::StripContextEntry),
    /// Hamburger menu open/close request — bubbled to root `Message::SetOpenMenu`.
    SetOpenMenu(Option<crate::app_message::OpenMenu>),
    /// Library-filter trigger open/close request — bubbled to root as
    /// `Message::Library(LibraryMessage::OpenChange { open, trigger_bounds })`.
    /// Open requests carry the trigger's screen-space bounds so the popover
    /// can anchor below the button; close requests carry `None`.
    LibraryOpenChange {
        open: bool,
        trigger_bounds: Option<iced::Rectangle>,
    },
    /// A row in the library-filter popover was clicked — bubbled to root
    /// as `Message::Library(LibraryMessage::Toggle(i32))`.
    LibraryToggle(i32),
    About,
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
    ("Playlists", "assets/icons/file-music.svg", NavView::Playlists),
    (
        "Radio",
        super::track_info_strip::RADIO_TOWER_ICON_PATH,
        NavView::Radios,
    ),
];

/// Flat-mode tab button style (filled accent background when active, bg0_hard idle).
///
/// Hover feedback is handled by `HoverOverlay` at the call site — this style
/// only distinguishes active (accent) vs idle (bg0_hard).
///
/// Shared between the horizontal nav bar and the vertical side nav bar.
pub(crate) fn flat_tab_container_style(
    is_active: bool,
) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme: &iced::Theme| container::Style {
        background: if is_active {
            Some(Background::Color(theme::accent_bright()))
        } else {
            Some(Background::Color(theme::bg0_hard()))
        },
        text_color: Some(if is_active {
            theme::bg0()
        } else {
            theme::fg2()
        }),
        border: Border {
            radius: theme::ui_border_radius(),
            ..Default::default()
        },
        ..Default::default()
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
    theme::nav_separator(theme::NavSeparatorAxis::Vertical, force_visible)
}

/// 2px vertical separator for the metadata info row — always visible
/// (callers want a hard visual divide between fields even in rounded mode).
fn info_separator<'a, M: 'a>() -> Element<'a, M> {
    theme::nav_separator(theme::NavSeparatorAxis::Vertical, true)
}

/// Height of the rounded underline indicator beneath active/hovered tabs
const UNDERLINE_HEIGHT: f32 = 2.0;

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
/// - Left: Hamburger menu, library-filter trigger, flat navigation tabs with active highlight
/// - Center: Track info text
/// - Right: Audio format info
pub(crate) fn nav_bar(data: NavBarViewData) -> Element<'static, NavBarMessage> {
    // -------------------------------------------------------------------------
    // Left Section: Hamburger + Library Filter + Flat Navigation Tabs
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
                canvas(super::hover_indicator::HoverIndicator {
                    indicator_color: underline_active,
                    hover_indicator_color: underline_hover,
                    expand: super::hover_indicator::HoverExpand::up(NAV_BAR_HEIGHT),
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
            // Flat mode: filled background container wrapped in mouse_area so the
            // HoverOverlay's press-scale fires (native button captures ButtonPressed first).
            let tab_style = flat_tab_container_style(is_active);
            let text_color = if is_active {
                theme::bg0()
            } else {
                theme::fg2()
            };

            mouse_area(
                super::hover_overlay::HoverOverlay::new(
                    container(tab_content(label, icon_path, display_mode, text_color))
                        .padding(tab_padding)
                        .height(Length::Fill)
                        .style(tab_style),
                )
                .border_radius(theme::ui_border_radius()),
            )
            .on_press(NavBarMessage::SwitchView(view))
            .interaction(iced::mouse::Interaction::Pointer)
            .into()
        }
    };

    let current = data.current_view;
    let is_side_nav = theme::is_side_nav();

    // -------------------------------------------------------------------------
    // Hamburger Menu (head of left_section, beside library filter)
    // -------------------------------------------------------------------------
    let hamburger: Element<'static, NavBarMessage> =
        super::hover_overlay::HoverOverlay::new(HamburgerMenu::new(
            |action| match action {
                MenuAction::ToggleLightMode => NavBarMessage::ToggleLightMode,
                MenuAction::OpenSettings => NavBarMessage::OpenSettings,
                MenuAction::About => NavBarMessage::About,
                MenuAction::Quit => NavBarMessage::Quit,
            },
            |open| {
                NavBarMessage::SetOpenMenu(open.then_some(crate::app_message::OpenMenu::Hamburger))
            },
            data.hamburger_open,
            data.is_light_mode,
        ))
        .border_radius(theme::ui_border_radius())
        .into();

    // -------------------------------------------------------------------------
    // Library-filter trigger + popover (icon button + dropdown panel)
    // -------------------------------------------------------------------------
    //
    // Sits at the LEFT edge of the top nav, BEFORE the Queue tab —
    // ensures it's always visible regardless of nav-bar contents on
    // narrow windows and matches the side-nav placement above the
    // Queue tab. Side-nav mode renders `Space::new()` here because the
    // sidebar's own footer handles the trigger.
    //
    // Slot is a Row of two widgets:
    //  1. The visible trigger (`library_filter_trigger`) — icon + N/M
    //     label + filtered-state pip. Emits `LibraryOpenChange` on
    //     left-click with the captured screen-space bounds.
    //  2. A zero-size `library_selector_popover` whose only render
    //     output is an iced overlay when `library_selector_open == true`.
    //     Anchors to `library_selector_bounds` (the trigger bounds
    //     captured at click time, stashed in
    //     `OpenMenu::LibrarySelector { trigger_bounds }`).
    //
    // Slot type stays `Element` in every branch so the surrounding row's
    // widget-tree shape never churns across renders (the iced re-render
    // trap that destroys `text_input` focus — see
    // `.agent/rules/gotchas.md` "Widget Tree & Focus").
    let library_trigger_slot: Element<'static, NavBarMessage> = if is_side_nav {
        Space::new().into()
    } else {
        let library_count = data.library_count;
        let active_library_count = data.active_library_count;
        let library_selector_open = data.library_selector_open;
        let library_selector_bounds = data.library_selector_bounds;
        let trigger = super::hover_overlay::HoverOverlay::new(
            super::library_filter_trigger::library_filter_trigger(
                library_count,
                active_library_count,
                library_selector_open,
                false, // not compact — top nav has room for the N/M text
                library_selector_bounds,
                |open, trigger_bounds| NavBarMessage::LibraryOpenChange {
                    open,
                    trigger_bounds,
                },
            ),
        )
        .border_radius(theme::ui_border_radius());

        // Popover rows: format song_count into a comma-grouped string
        // for the dim right column; `None` → empty string (Subsonic
        // fallback case for non-admin sessions).
        let popover_items: Vec<(i32, String, String, bool)> = data
            .library_rows
            .clone()
            .into_iter()
            .map(|(id, name, song_count, checked)| {
                let right_label = song_count
                    .map(super::format_count_with_commas)
                    .unwrap_or_default();
                (id, name, right_label, checked)
            })
            .collect();

        let total_count = data.library_count;
        let active_count = data.active_library_count;
        let popover = super::checkbox_dropdown::library_selector_popover(
            popover_items,
            active_count,
            total_count,
            NavBarMessage::LibraryToggle,
            |bounds| NavBarMessage::LibraryOpenChange {
                open: bounds.is_some(),
                trigger_bounds: bounds,
            },
            library_selector_open,
            library_selector_bounds,
        );

        iced::widget::row![trigger, popover]
            .align_y(Alignment::Center)
            .into()
    };

    // In Side mode, nav tabs move to the vertical sidebar — hide them here
    let mut left_section: iced::widget::Row<'static, NavBarMessage> = if is_side_nav {
        row![]
            .spacing(0)
            .height(Length::Fill)
            .align_y(Alignment::Center)
    } else {
        let mut tabs = row![
            hamburger,
            tab_separator(false),
            library_trigger_slot,
            tab_separator(false)
        ]
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

    // Only show nav bar metadata when the display mode targets the top/nav bar.
    // Off, PlayerBar, and ProgressTrack modes shouldn't show metadata here.
    let show_nav_metadata = {
        use nokkvi_data::types::player_settings::TrackInfoDisplay;
        let mode = theme::track_info_display();
        mode == TrackInfoDisplay::TopBar
    };

    // In merged mode the marquee scrolls any-length text, so breakpoints are
    // irrelevant — all user-enabled fields stay visible regardless of window
    // width.  In non-merged mode fields are separate labeled elements that each
    // need their own lane, so the responsive breakpoints still apply.
    let merged_mode_active = theme::strip_merged_mode();

    let show_title = show_nav_metadata
        && (merged_mode_active || data.window_width >= BREAKPOINT_SHOW_TITLE)
        && theme::strip_show_title();
    let show_artist = show_nav_metadata
        && (merged_mode_active || data.window_width >= BREAKPOINT_SHOW_ARTIST)
        && theme::strip_show_artist();
    let show_album = show_nav_metadata
        && (merged_mode_active || data.window_width >= BREAKPOINT_SHOW_ALBUM)
        && theme::strip_show_album();
    let show_labels = theme::strip_show_labels();
    let title_label = if show_labels { "title:" } else { "" };
    let artist_label = if show_labels { "artist:" } else { "" };
    let album_label = if show_labels { "album:" } else { "" };

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
                    .size(13.0)
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

            let info_sep = info_separator;

            let mut info_row = iced::widget::Row::new()
                .spacing(6)
                .align_y(Alignment::Center)
                .height(Length::Fill);

            // Merged mode wants the marquee to span the full lane (matching
            // `track_info_strip::build_merged_centered_strip`); flanking Fill
            // spacers would compete with it and shrink the scroll lane.
            let is_merged_layout = merged_mode_active;

            if !is_merged_layout {
                // Fill spacer → center the metadata fields
                info_row = info_row.push(Space::new().width(Length::Fill));
            }

            // Progressive metadata: each field independently toggleable
            let mut has_prev_field = false;

            if let Some(radio_name) = &data.radio_name {
                let icy_title = data.icy_title.as_deref().unwrap_or("");
                let icy_artist = data.icy_artist.as_deref().unwrap_or("");
                let radio_url = data.radio_url.as_deref().unwrap_or("");

                let radio_icon = || {
                    colored_icon(
                        super::track_info_strip::RADIO_TOWER_ICON_PATH,
                        12.0,
                        theme::fg4(),
                    )
                };

                if merged_mode_active {
                    // Merged radio: single marquee containing station + ICY fields,
                    // bracketed by separators with the radio-tower icon prepended.
                    info_row = info_row.push(info_sep());
                    info_row = info_row.push(radio_icon());

                    let merged = super::track_info_strip::merged_radio_strip_string(
                        radio_name,
                        icy_title,
                        icy_artist,
                        radio_url,
                        show_labels,
                        theme::strip_separator().as_join_str(),
                    );
                    if !merged.is_empty() {
                        info_row = info_row.push(
                            iced::widget::row![
                                super::marquee_text::marquee_text(merged)
                                    .size(10.0)
                                    .font(theme::ui_font())
                                    .color(theme::selected_color())
                                    .align_x(iced::alignment::Horizontal::Center),
                            ]
                            .align_y(Alignment::Center)
                            .width(Length::Fill),
                        );
                    }
                    info_row = info_row.push(info_sep());
                } else {
                    // Columnar radio: shared builder so the nav-bar path and
                    // the track_info_strip path stay in lockstep on field
                    // labels, ordering, and URL fallback (audit Tier 2 #2.11).
                    info_row = super::track_info_strip::columnar_radio_strip(
                        info_row,
                        radio_name,
                        icy_title,
                        icy_artist,
                        data.radio_url.as_deref(),
                        info_sep,
                    );
                }
            } else if merged_mode_active {
                let merged = super::track_info_strip::merged_strip_string(
                    show_title,
                    show_artist,
                    show_album,
                    show_labels,
                    theme::strip_separator().as_join_str(),
                    &title,
                    &artist,
                    &album,
                );
                if !merged.is_empty() {
                    info_row = info_row.push(
                        iced::widget::row![
                            super::marquee_text::marquee_text(merged)
                                .size(10.0)
                                .font(theme::ui_font())
                                .color(theme::selected_color())
                                .align_x(iced::alignment::Horizontal::Center),
                        ]
                        .align_y(Alignment::Center)
                        .width(Length::Fill),
                    );
                }
            } else {
                if show_title {
                    info_row = info_row.push(info_sep());
                    info_row =
                        info_row.push(info_field(title_label, title, theme::now_playing_color()));
                    has_prev_field = true;
                }

                if show_artist {
                    info_row = info_row.push(info_sep());
                    info_row =
                        info_row.push(info_field(artist_label, artist, theme::selected_color()));
                    has_prev_field = true;
                }

                if show_album {
                    info_row = info_row.push(info_sep());
                    info_row = info_row.push(info_field(album_label, album, theme::fg2()));
                    has_prev_field = true;
                }

                if has_prev_field {
                    info_row = info_row.push(info_sep());
                }
            }

            if !is_merged_layout {
                // Fill spacer → push format info away
                info_row = info_row.push(Space::new().width(Length::Fill));
            }

            let clickable = container(mouse_area(info_row).on_press(NavBarMessage::StripClicked))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_y(Length::Fill);

            let wrapped: Element<'static, NavBarMessage> = if data.radio_name.is_some() {
                clickable.into()
            } else {
                let has_local_path = !data.local_music_path.is_empty();
                let is_starred = data.is_current_starred;
                let strip_context_open = data.strip_context_open;
                let strip_context_position = data.strip_context_position;
                super::context_menu::context_menu(
                    clickable,
                    super::context_menu::strip_entries(has_local_path),
                    move |entry, length| {
                        super::context_menu::strip_entry_view(
                            entry,
                            length,
                            is_starred,
                            NavBarMessage::StripContextAction,
                        )
                    },
                    strip_context_open,
                    strip_context_position,
                    |position| match position {
                        Some(p) => NavBarMessage::SetOpenMenu(Some(
                            crate::app_message::OpenMenu::Context {
                                id: crate::app_message::ContextMenuId::Strip,
                                position: p,
                            },
                        )),
                        None => NavBarMessage::SetOpenMenu(None),
                    },
                )
                .into()
            };
            wrapped
        };

    // -------------------------------------------------------------------------
    // Format Info (independent of metadata — stays visible at narrow widths)
    // -------------------------------------------------------------------------
    let format_section: Element<'static, NavBarMessage> =
        if is_playing && show_nav_metadata && theme::strip_show_format_info() {
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
                let fmt_sep = info_separator;
                row![
                    fmt_sep(),
                    text(combined)
                        .size(10.0)
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
    // Assemble Layout: Hamburger + LibraryFilter + Tabs | Track Info | Format Info
    // -------------------------------------------------------------------------
    let nav_content = container(
        row![
            // Left: Hamburger + Library filter trigger + navigation tabs
            left_section,
            // Center: Track info (collapses at narrow widths)
            center_section,
            // Format info (stays visible independently)
            format_section,
        ]
        .align_y(Alignment::Center)
        .padding(0)
        .width(Length::Fill)
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

#[cfg(test)]
mod layout_invariants {
    //! Verifies the layout pattern used by `nav_content`'s outer row. The
    //! merged-mode marquee depends on `center_section` being given a max width
    //! equal to the *visible* center area (window − hamburger − tabs − format).
    //!
    //! iced's flex layout caches a `compression.width` flag in `Limits`. A row
    //! with default `Length::Shrink` width forces `compression.width = true`
    //! through `limits.width(Shrink)`, which makes pass 1 lay out *all*
    //! children (Fill or otherwise) sequentially with `available` shrinking
    //! after each. Children declared earlier claim space first; later children
    //! get whatever's left, possibly squeezed to 0.
    //!
    //! Setting the outer row to `Length::Fill` keeps `compression.width = false`
    //! (inherited from the parent `nav_content` Container, which sits inside a
    //! Vertical column that hardcodes `compression.width = false` on its
    //! children — see flex.rs `axis.pack(main_compress, false)`). That flips
    //! `main_compress` to false and routes Fill children through pass 3, which
    //! gives them `remaining = max - sum_of_non_fill_children` as their max.
    //!
    //! These tests pin that contract by running iced's actual layout against
    //! the null renderer (`()`) on a structure that mirrors the nav bar.
    use iced::{
        Element, Length, Size,
        advanced::{
            layout::{Limits, Node},
            widget::{Tree, Widget},
        },
        widget::{Container, Row, Space},
    };

    type NullRenderer = ();
    type TestMessage = ();

    /// Build a 3-child row [Fixed(left_w), Fill, Fixed(right_w)] mimicking the
    /// nav bar's [hamburger+library+tabs, center_section, format_section]
    /// layout. `outer_width` is the row's own `.width(...)` setting — the
    /// variable under test.
    fn build_three_child_row(
        outer_width: Length,
        left_w: f32,
        right_w: f32,
    ) -> Row<'static, TestMessage, iced::Theme, NullRenderer> {
        let left: Element<'static, TestMessage, iced::Theme, NullRenderer> = Space::new()
            .width(Length::Fixed(left_w))
            .height(Length::Fill)
            .into();
        let center: Element<'static, TestMessage, iced::Theme, NullRenderer> =
            Container::new(Space::new().width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        let right: Element<'static, TestMessage, iced::Theme, NullRenderer> = Space::new()
            .width(Length::Fixed(right_w))
            .height(Length::Fill)
            .into();
        Row::with_children([left, center, right])
            .spacing(0)
            .width(outer_width)
            .height(Length::Fill)
    }

    /// Run `Widget::layout` against the null renderer and return the node.
    fn layout_row(row: Row<'static, TestMessage, iced::Theme, NullRenderer>, max_w: f32) -> Node {
        let mut tree = Tree::new(&row as &dyn Widget<TestMessage, iced::Theme, NullRenderer>);
        let renderer: NullRenderer = ();
        let limits = Limits::new(Size::ZERO, Size::new(max_w, 100.0));
        let mut row_owned = row;
        row_owned.layout(&mut tree, &renderer, &limits)
    }

    #[test]
    fn shrink_outer_row_collapses_fill_center_to_zero() {
        // With the row at default Shrink width, compression.width cascades
        // true. The Fill center container resolves to its content's intrinsic,
        // which for a Space::Fill is 0 — exactly the bug the fix addresses.
        let row = build_three_child_row(Length::Shrink, 100.0, 50.0);
        let node = layout_row(row, 1000.0);
        let center = &node.children()[1];
        assert_eq!(
            center.bounds().width,
            0.0,
            "Shrink outer row leaves Fill center at 0 (intrinsic of Space::Fill)"
        );
    }

    #[test]
    fn fill_outer_row_gives_center_the_remaining_lane() {
        // With the row explicitly Length::Fill, compression.width stays false
        // (inherited). main_compress=false routes the Fill center through
        // pass 3, which awards it remaining = max - non_fill_widths.
        let row = build_three_child_row(Length::Fill, 100.0, 50.0);
        let node = layout_row(row, 1000.0);
        let center = &node.children()[1];
        assert!(
            (center.bounds().width - 850.0).abs() < 0.5,
            "Fill outer row gives Fill center the visible lane (1000 - 100 - 50 = 850), got {}",
            center.bounds().width
        );
    }

    #[test]
    fn fill_outer_row_lane_tracks_window_resize() {
        // Verifies the lane width follows the window width — the resize behavior
        // the user observed as broken in Top Bar mode. With the fix the lane
        // recomputes correctly at every width.
        for window_w in [600.0_f32, 800.0, 1200.0, 1600.0] {
            let row = build_three_child_row(Length::Fill, 100.0, 50.0);
            let node = layout_row(row, window_w);
            let center = &node.children()[1];
            let expected = window_w - 100.0 - 50.0;
            assert!(
                (center.bounds().width - expected).abs() < 0.5,
                "window={window_w} expected lane={expected}, got {}",
                center.bounds().width,
            );
        }
    }
}
