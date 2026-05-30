//! Side Navigation Bar — vertical nav tabs for Side layout mode
//!
//! Renders view tab buttons in a vertical column on the left edge of the
//! app. Each button uses a canvas widget with rotated text (-90°, reading
//! bottom-to-top). Styling mirrors the horizontal nav bar's flat redesign:
//!
//!   - Flat mode: 32 px wide chrome, edge-to-edge tab cells with full-cell
//!     `accent_bright()` fill when active and 1 px `theme::border()`
//!     horizontal rules between cells.
//!   - Rounded mode: 40 px wide chrome with 4 px outer gutters around 32 px
//!     `ui_radius_md()` tab cards; rules inset 14 px on each side so they
//!     float between the rounded cards.
//!
//! Emits the same `NavBarMessage` variants as the horizontal nav bar.

use std::f32::consts::FRAC_PI_2;

use iced::{
    Background, Border, Color, Element, Length, Point, Rectangle, Vector,
    font::{Font, Weight},
    widget::{Space, canvas, column, container, mouse_area, row},
};
use nokkvi_data::types::player_settings::NavDisplayMode;

use super::nav_bar::{NAV_TABS, NavBarMessage, NavView, colored_icon};
use crate::theme;

/// Side-nav width (px) by chrome mode.
/// - Flat mode: 32 px — edge-to-edge tab cells.
/// - Rounded mode: 40 px — 4 px outer gutters on each side of 32 px
///   `ui_radius_md()` tab cards so the rounded corners breathe.
///
/// Exposed as a function so callers always see the current mode's value.
const SIDE_NAV_WIDTH_FLAT: f32 = 32.0;
const SIDE_NAV_WIDTH_ROUNDED: f32 = 40.0;

#[inline]
pub(crate) fn side_nav_width() -> f32 {
    if theme::is_rounded_mode() {
        SIDE_NAV_WIDTH_ROUNDED
    } else {
        SIDE_NAV_WIDTH_FLAT
    }
}

/// Width of the side-nav right-edge separator (1 px `theme::border()`).
pub(crate) const SIDE_NAV_BORDER: f32 = 1.0;

/// Current side-nav total horizontal footprint (icons + right-edge
/// border) for the active chrome mode. After L6 migration this is the
/// only public symbol — the old `SIDE_NAV_TOTAL_WIDTH` worst-case const
/// was removed once `app_view::content_pane_width()` moved to the live
/// function.
#[inline]
pub(crate) fn side_nav_total_width() -> f32 {
    side_nav_width() + SIDE_NAV_BORDER
}

/// Height of the cluster cells (hamburger and library trigger at the top of
/// the column). Sized to match the icon-only tab profile so the cluster
/// reads as a uniform band with the tabs below, regardless of display mode.
const ICON_TAB_HEIGHT: f32 = 36.0;

/// Height of the icon slot within text+icon tabs (the only Fixed-height
/// component left in `side_nav_tab_content`; the rotated label below it
/// takes `Length::Fill` so the pair tracks the cell's `FillPortion(1)` lane).
const ICON_SLOT_HEIGHT: f32 = 22.0;

/// Icon size in the side nav bar
const ICON_SIZE: f32 = 14.0;

/// Inner gutter (px) around rounded-mode tab cards. Matches the design
/// CSS `margin: 0 4px` on `.nk-side-tab` (rounded).
const SIDE_NAV_CARD_GUTTER: f32 = 4.0;

/// Vertical padding (px) at the top/bottom of the rounded-mode side
/// nav stack. Matches `padding: 8px 4px` on the rounded `.nk-sidenav`.
const SIDE_NAV_TRAY_PAD_V: f32 = 8.0;

/// Width (px) of an individual tab card inside the sidebar. Flat mode
/// fills the whole sidebar; rounded mode leaves a 4 px gutter on each
/// side so the rounded corners aren't clipped against the chrome edge.
/// Happens to be 32 px in both modes today (`40 - 2*4 = 32 = 32`).
#[inline]
fn side_nav_tab_width() -> f32 {
    if theme::is_rounded_mode() {
        side_nav_width() - 2.0 * SIDE_NAV_CARD_GUTTER
    } else {
        side_nav_width()
    }
}

/// Data passed to the side nav bar for rendering
pub(crate) struct SideNavBarData {
    pub current_view: NavView,
    pub settings_open: bool,
    /// A playlist-edit session is active — render the contextual editor cell.
    pub editor_session_active: bool,
    /// The editor view is the current destination — highlight that cell.
    pub editor_active: bool,
    /// Total libraries known to the client. `<= 1` hides the footer
    /// library trigger entirely (same suppression rule as the top-nav
    /// variant — see `libraries_imp_plan.md` §2).
    pub library_count: usize,
    /// Subset of `library_count` currently in the active selection.
    pub active_library_count: usize,
    /// Whether the library-filter popover is currently open.
    pub library_selector_open: bool,
    /// Trigger bounds captured at click time so the popover overlay
    /// anchors next to the footer trigger.
    pub library_selector_bounds: Option<iced::Rectangle>,
    /// Library-filter popover rows: `(id, name, song_count, checked)`.
    /// Mirrors `NavBarViewData::library_rows`.
    pub library_rows: Vec<(i32, String, Option<u32>, bool)>,
    pub hamburger_open: bool,
    pub is_light_mode: bool,
}

/// Canvas program that draws the side-nav tab's rotated label.
///
/// The redesign moved active-tab signaling onto the card's full-cell
/// `accent_bright()` fill, so this program no longer draws the
/// right-edge accent indicator bar (or its hover variant) — both were
/// dormant before the cleanup. Recover from `git show` if a future
/// design wants an inline indicator strip.
struct RotatedLabel {
    label: &'static str,
    color: Color,
}

impl<Message> canvas::Program<Message> for RotatedLabel {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        // Translate to center, rotate -90°, draw text (reads bottom-to-top)
        let center = frame.center();
        frame.translate(Vector::new(center.x, center.y));
        frame.rotate(-FRAC_PI_2);

        frame.fill_text(canvas::Text {
            content: self.label.to_uppercase(),
            position: Point::new(0.0, 0.0),
            color: self.color,
            size: iced::Pixels(12.0),
            font: Font {
                weight: Weight::Bold,
                ..theme::ui_font()
            },
            align_x: iced::alignment::Horizontal::Center.into(),
            align_y: iced::alignment::Vertical::Center,
            ..Default::default()
        });

        vec![frame.into_geometry()]
    }
}

/// Build side nav tab content based on display mode.
///
/// Returns content whose height is `Length::Fill` so the inner glyph adapts
/// to the outer cell's `FillPortion(1)` lane. Mirrors the top-bar centering
/// fix: a Fixed-height inner overruns the cell at short window heights and
/// bleeds into neighbouring tabs, whereas a Fill inner inherits the cell's
/// vertical extent so the canvas always centers its rotated text against
/// the cell's actual bounds and clips cleanly at the cell boundary.
fn side_nav_tab_content(
    label: &'static str,
    icon_path: &'static str,
    display_mode: NavDisplayMode,
    text_color: Color,
) -> Element<'static, NavBarMessage> {
    let card_width = side_nav_tab_width();
    match display_mode {
        NavDisplayMode::TextOnly => canvas(RotatedLabel {
            label,
            color: text_color,
        })
        .width(Length::Fixed(card_width))
        .height(Length::Fill)
        .into(),
        NavDisplayMode::IconsOnly => {
            // Flat redesign uses a full-cell `accent_bright()` fill for
            // the active state, so the icon centers in the card without
            // an inline indicator strip.
            container(colored_icon(icon_path, ICON_SIZE, text_color))
                .width(Length::Fixed(card_width))
                .height(Length::Fill)
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::Center)
                .into()
        }
        NavDisplayMode::TextAndIcons => {
            // Icon header stays at its `ICON_SLOT_HEIGHT` slot anchored to
            // the bottom of the slot (the design's `align_y(End)` rule
            // tucks the icon flush against the rotated label below). The
            // label canvas takes the remaining cell height via `Fill`, so
            // the pair tracks the cell's `FillPortion(1)` lane instead of
            // overflowing the cell at short windows.
            let icon_widget = container(colored_icon(icon_path, ICON_SIZE, text_color))
                .width(Length::Fixed(card_width))
                .height(Length::Fixed(ICON_SLOT_HEIGHT))
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::End);

            let label_canvas = canvas(RotatedLabel {
                label,
                color: text_color,
            })
            .width(Length::Fixed(card_width))
            .height(Length::Fill);

            column![icon_widget, label_canvas]
                .spacing(0)
                .width(Length::Fixed(card_width))
                .height(Length::Fill)
                .into()
        }
    }
}

/// Build the vertical side navigation bar
pub(crate) fn side_nav_bar(data: SideNavBarData) -> Element<'static, NavBarMessage> {
    let settings_open = data.settings_open;
    let editor_active = data.editor_active;
    let current = data.current_view;
    let is_rounded = theme::is_rounded_mode();

    let nav_tab = |label: &'static str,
                   icon_path: &'static str,
                   is_active: bool,
                   on_press: NavBarMessage|
     -> Element<'_, NavBarMessage> {
        let display_mode = theme::nav_display_mode();

        // Active = filled `accent_bright()` + dark text, idle = `bg0_hard()`
        // (matches the chrome) + `fg0()`. Rounded mode rounds to a pill so
        // the active fill reads as a capsule (matches the top-nav tab
        // treatment); flat mode keeps square corners since cells are
        // 1-px-`border()`-separated.
        let text_color = if is_active {
            theme::bg0_hard()
        } else {
            theme::fg0()
        };

        let content = side_nav_tab_content(label, icon_path, display_mode, text_color);

        let card_width = side_nav_tab_width();
        // `ui_radius_pill()` returns `0.0.into()` in flat mode.
        let card_radius = theme::ui_radius_pill();

        // Each tab claims `FillPortion(1)` of the column's remaining vertical
        // space so the stack distributes evenly down the full sidebar instead
        // of leaving a trailing dead zone below the last cell. `align_y` keeps
        // the centered glyph (icon / rotated label / stacked pair) anchored in
        // the middle of the now-taller cell.
        mouse_area(
            super::hover_overlay::HoverOverlay::new(
                container(content)
                    .padding(0)
                    .width(Length::Fixed(card_width))
                    .height(Length::FillPortion(1))
                    .align_y(iced::Alignment::Center)
                    .style(move |_: &iced::Theme| container::Style {
                        background: if is_active {
                            Some(Background::Color(theme::accent_bright()))
                        } else {
                            Some(Background::Color(theme::bg0_hard()))
                        },
                        text_color: Some(if is_active {
                            theme::bg0_hard()
                        } else {
                            theme::fg0()
                        }),
                        border: Border {
                            radius: card_radius,
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
            )
            .border_radius(card_radius)
            .on_accent_surface(is_active),
        )
        .on_press(on_press)
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
    };

    // Separator line between tabs (horizontal rule in the vertical layout).
    // Flat mode: 1 px `theme::border()` rule running the full sidebar
    // width, mirroring the design's `border-bottom: 1px solid #1a2024`
    // on `.nk-side-tab`. Rounded mode: same rule but inset 14 px on
    // each side so it floats inside the gap between the rounded cards
    // (matches the design's `margin: 6px 14px` on `.nk-side-divider`).
    let side_inset = if is_rounded { 14.0_f32 } else { 0.0 };
    let separator = move || -> Element<'_, NavBarMessage> {
        let rule = container(iced::widget::Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(1.0))
            .style(|_| container::Style {
                background: Some(theme::border().into()),
                ..Default::default()
            });
        if side_inset > 0.0 {
            row![
                iced::widget::Space::new().width(Length::Fixed(side_inset)),
                rule,
                iced::widget::Space::new().width(Length::Fixed(side_inset)),
            ]
            .height(Length::Fixed(1.0))
            .into()
        } else {
            rule.into()
        }
    };

    // Settings indicator when settings are open (non-interactive).
    // Renders with the same active-state visuals the other tabs use
    // (filled `accent_bright()` card, `bg0_hard()` text, pill outline in
    // rounded mode) so the user sees "Settings" highlighted in the same
    // vocabulary regardless of chrome mode.
    let settings_indicator: Option<Element<'_, NavBarMessage>> = if settings_open {
        let display_mode = theme::nav_display_mode();
        let text_color = theme::bg0_hard();
        // `ui_radius_pill()` returns `0.0.into()` in flat mode.
        let card_radius = theme::ui_radius_pill();

        let settings_content = side_nav_tab_content(
            "Settings",
            "assets/icons/settings.svg",
            display_mode,
            text_color,
        );

        Some(
            container(settings_content)
                .width(Length::Fixed(side_nav_tab_width()))
                .height(Length::FillPortion(1))
                .align_y(iced::Alignment::Center)
                .style(move |_: &iced::Theme| container::Style {
                    background: Some(Background::Color(theme::accent_bright())),
                    border: Border {
                        radius: card_radius,
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .into(),
        )
    } else {
        None
    };

    // -------------------------------------------------------------------------
    // Library-filter trigger + popover (rendered ABOVE the Queue tab)
    // -------------------------------------------------------------------------
    //
    // The trigger renders compact: icon-only 28 × 28 in the neutral
    // state, icon + pip + `N/M` text stacked vertically in 28 × 44
    // when filtered. Mounting at the top of the column (rather than
    // the footer) keeps the button visible on short windows and
    // matches the top-nav layout that puts the trigger before the
    // Queue tab.
    //
    // The popover is a zero-size `library_selector_popover` sibling
    // whose only render output is the iced overlay anchored to
    // `library_selector_bounds`. From the top of the sidebar the
    // overlay opens downward; the overlay positioning logic falls
    // back to anchoring above when there's no room below (cheap
    // safety for very short windows).
    //
    // Trigger self-suppresses to `Space::new()` when
    // `library_count <= 1`, so the slot collapses naturally on
    // single-library servers.
    let library_count = data.library_count;
    let active_library_count = data.active_library_count;
    let library_selector_open = data.library_selector_open;
    let library_selector_bounds = data.library_selector_bounds;
    let library_rows = data.library_rows.clone();

    // Chassis matches the icon-only side-nav tab cell (56 wide × 36
    // tall) so the hamburger, library trigger, and nav tabs share the
    // same column band — uniform width with the tab cards above them,
    // pill-shaped in rounded mode to match the tabs' active-state pill.
    let nav_width = side_nav_width();
    let cluster_cell_width = side_nav_tab_width();
    let cluster_cell_height = ICON_TAB_HEIGHT;
    // `ui_radius_pill()` returns `0.0.into()` in flat mode.
    let cluster_radius = theme::ui_radius_pill();

    let hamburger_inner: Element<'static, NavBarMessage> = super::hover_overlay::HoverOverlay::new(
        crate::widgets::hamburger_menu::HamburgerMenu::new(
            |action| match action {
                crate::widgets::hamburger_menu::MenuAction::ToggleLightMode => {
                    NavBarMessage::ToggleLightMode
                }
                crate::widgets::hamburger_menu::MenuAction::OpenSettings => {
                    NavBarMessage::OpenSettings
                }
                crate::widgets::hamburger_menu::MenuAction::About => NavBarMessage::About,
                crate::widgets::hamburger_menu::MenuAction::Quit => NavBarMessage::Quit,
            },
            |open| {
                NavBarMessage::SetOpenMenu(open.then_some(crate::app_message::OpenMenu::Hamburger))
            },
            data.hamburger_open,
            data.is_light_mode,
        )
        .chassis(cluster_cell_width, cluster_cell_height),
    )
    .border_radius(cluster_radius)
    .into();
    let hamburger: Element<'static, NavBarMessage> = container(hamburger_inner)
        .width(Length::Fixed(nav_width))
        .center_x(Length::Fixed(nav_width))
        .into();

    let library_chassis = iced::Size::new(cluster_cell_width, cluster_cell_height);
    let library_trigger_inner = super::hover_overlay::HoverOverlay::new(
        container(super::library_filter_trigger::library_filter_trigger(
            library_count,
            active_library_count,
            library_selector_open,
            library_chassis,
            library_chassis,
            |open, trigger_bounds| NavBarMessage::LibraryOpenChange {
                open,
                trigger_bounds,
            },
        ))
        .align_x(iced::Alignment::Center),
    )
    .border_radius(cluster_radius);
    let library_trigger: Element<'_, NavBarMessage> = container(library_trigger_inner)
        .width(Length::Fixed(nav_width))
        .center_x(Length::Fixed(nav_width))
        .into();

    let popover_items: Vec<(i32, String, String, bool)> = library_rows
        .into_iter()
        .map(|(id, name, song_count, checked)| {
            let right_label = song_count
                .map(super::format_count_with_commas)
                .unwrap_or_default();
            (id, name, right_label, checked)
        })
        .collect();
    let library_popover = super::checkbox_dropdown::library_selector_popover(
        popover_items,
        active_library_count,
        library_count,
        NavBarMessage::LibraryToggle,
        |bounds| NavBarMessage::LibraryOpenChange {
            open: bounds.is_some(),
            trigger_bounds: bounds,
        },
        library_selector_open,
        library_selector_bounds,
    );

    // Wrap each tab card in a 4-px-gutter row in rounded mode so the
    // cards float inside the 64-px sidebar with even left/right
    // margins; flat mode renders the card edge-to-edge (no wrap).
    // `height(Length::Fill)` is required so the wrapping container
    // doesn't compress its `FillPortion(1)` child to its content's
    // intrinsic height — the wrap inherits the column's leftover
    // vertical space and forwards it intact to the tab cell inside.
    fn wrap_in_gutter<'a>(
        elem: Element<'a, NavBarMessage>,
        is_rounded: bool,
        nav_width: f32,
    ) -> Element<'a, NavBarMessage> {
        if is_rounded {
            container(elem)
                .width(Length::Fixed(nav_width))
                .height(Length::Fill)
                .center_x(Length::Fixed(nav_width))
                .into()
        } else {
            elem
        }
    }

    // Inter-tab spacing in rounded mode (so the cards don't touch top
    // to bottom).
    let stack_spacing: f32 = if is_rounded {
        SIDE_NAV_CARD_GUTTER
    } else {
        0.0
    };

    // Build vertical column: hamburger + library cluster on top, then
    // a divider, then the NAV_TABS. Mirrors the top-nav layout but
    // rotated 90°.
    //
    // `height(Length::Fill)` is required so the column's `FillPortion(1)`
    // tab children get pass-3 layout (split remaining space) instead of
    // being compressed to their intrinsic height. The parent
    // `container(tabs).height(Length::Fill)` already gives `main_compress
    // = false`; the column must declare Fill height itself for that to
    // propagate to its own children.
    let mut tabs = column![hamburger, library_trigger, library_popover, separator()]
        .spacing(stack_spacing)
        .width(Length::Fixed(nav_width))
        .height(Length::Fill);
    for &(label, icon_path, view) in NAV_TABS {
        // No regular tab is active while editing — the editor cell carries the
        // active state (current_view falls back to Queue for the editor view).
        let is_active = !settings_open && !editor_active && current == view;
        tabs = tabs
            .push(wrap_in_gutter(
                nav_tab(label, icon_path, is_active, NavBarMessage::SwitchView(view)),
                is_rounded,
                nav_width,
            ))
            .push(separator());
    }

    if let Some(indicator) = settings_indicator {
        tabs = tabs
            .push(wrap_in_gutter(indicator, is_rounded, nav_width))
            .push(separator());
    }

    // Contextual editor cell — present only while an edit session is active.
    if data.editor_session_active {
        tabs = tabs
            .push(wrap_in_gutter(
                nav_tab(
                    "Editing",
                    "assets/icons/pencil.svg",
                    editor_active,
                    NavBarMessage::SwitchToEditor,
                ),
                is_rounded,
                nav_width,
            ))
            .push(separator());
    }

    // Apply top/bottom tray padding in rounded mode (matches the design's
    // `padding: 8px 4px` on the rounded `.nk-sidenav`).
    let tray_padding = if is_rounded {
        iced::Padding {
            top: SIDE_NAV_TRAY_PAD_V,
            bottom: SIDE_NAV_TRAY_PAD_V,
            left: 0.0,
            right: 0.0,
        }
    } else {
        iced::Padding::ZERO
    };

    // No trailing Fill spacer — each tab cell already declares
    // `FillPortion(1)` height, so the tab stack itself consumes any
    // leftover vertical space. A trailing `Space::Fill` here would
    // claim a 1/(N+1) share of the lane and shave a slice off every
    // tab cell.

    // Right edge separator (1 px `theme::border()` vertical rule).
    let right_edge: Element<'_, NavBarMessage> = container(Space::new())
        .width(Length::Fixed(SIDE_NAV_BORDER))
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(theme::border().into()),
            ..Default::default()
        })
        .into();

    container(
        row![
            container(tabs)
                .width(Length::Fixed(nav_width))
                .height(Length::Fill)
                .padding(tray_padding)
                .style(theme::container_bg0_hard),
            right_edge,
        ]
        .spacing(0)
        .height(Length::Fill),
    )
    .width(Length::Fixed(side_nav_total_width()))
    .height(Length::Fill)
    .into()
}
