//! Checkbox / Action Dropdown Widget
//!
//! The shared header-anchored dropdown chassis: a left-click trigger button that
//! opens an overlay panel anchored below it (single-active `OpenMenu` mutual
//! exclusion, Escape / outside-press dismissal, click-time trigger-bounds
//! anchoring, shadow-halo). It hosts two row families off one widget so the
//! subtle overlay/dismiss/anchor machinery lives at one site:
//!
//! - **Checkbox rows** (multi-toggle UIs, e.g. per-column visibility): a styled
//!   checkbox glyph (the shared [`super::checkbox_glyph::element`]) + label. The
//!   menu **stays open** on click so the user can flip several toggles in one
//!   open (`close_on_click = false`). See [`checkbox_dropdown`] /
//!   [`library_selector_popover`].
//! - **Action rows** (one-shot fire-and-close, e.g. the Queue server-sync
//!   push/pull menu): a leading action icon + label + optional dim consequence
//!   subtitle, **no** checkbox glyph. The menu **closes** on click
//!   (`close_on_click = true`, publishing `on_open_change(None)` alongside the
//!   row's action). See [`action_dropdown`].
//!
//! Mirrors [`crate::widgets::context_menu`] but with a **left-click trigger**
//! (vs right-click) **anchored below the trigger** (vs at the cursor).
//!
//! Two entry points share the same widget chassis (overlay positioning,
//! escape / click-outside, persisted hover state):
//!
//! 1. [`checkbox_dropdown`] — **single-column rows** with `&'static str` labels,
//!    used by view-header column-visibility menus. Renders its own trigger
//!    button (icon + tooltip chrome).
//! 2. [`library_selector_popover`] — **two-column rows** with owned `String`
//!    name + right-aligned dim metadata label. The trigger is supplied
//!    externally (the nav-bar `library_filter_trigger` widget has its own
//!    icon + count + pip chrome that the standard trigger button can't
//!    represent); this constructor only renders the overlay panel.
//!
//! ```ignore
//! checkbox_dropdown(
//!     "assets/icons/columns-3-cog.svg",
//!     "Show/hide columns",
//!     vec![
//!         (Col::Stars, "Stars", state.stars),
//!         (Col::Album, "Album", state.album),
//!     ],
//!     Message::ToggleColumn,  // bare tuple-variant constructor
//! )
//! ```

use iced::{
    Element, Event, Length, Point, Rectangle, Size, Theme, Vector,
    advanced::{
        Layout, Shell, Widget, layout, overlay, renderer,
        widget::{self, tree},
    },
    mouse,
    widget::{column, container, mouse_area, row, svg, text, tooltip},
};

use crate::{
    theme,
    widgets::{
        menu_constants::{
            MENU_ICON_SIZE, MENU_MIN_WIDTH, MENU_TEXT_SIZE, inflate_for_shadow_around_child,
            visible_menu_layout,
        },
        menu_dismiss,
    },
};

/// Trigger-button glyph size (px). Matches the 18 px icon used by
/// `view_header::header_icon_cell` so the column-dropdown trigger reads at
/// the same visual weight as the sibling sort/refresh/center icons.
const TRIGGER_ICON_SIZE: f32 = 18.0;

/// Max width of the name column in two-column rows (px). Sized for the
/// wider `LIBRARY_SELECTOR_WIDTH` so longer library names ("Longmont
/// Potion Castle") don't ellipsize unnecessarily.
const MAX_NAME_WIDTH: f32 = 220.0;

/// Wider popover width used by [`library_selector_popover`]. The column
/// dropdowns (Albums/Artists/Songs header gear) keep using
/// `MENU_MIN_WIDTH` — only the library selector overrides this.
const LIBRARY_SELECTOR_WIDTH: f32 = 340.0;

/// Width of the [`action_dropdown`] popover (px). Wider than the column
/// dropdowns' `MENU_MIN_WIDTH` so a two-line action row's consequence subtitle
/// ("Replaces the queue saved on the server") fits without ellipsizing; a wider
/// active font wraps rather than truncates inside this fixed width.
const ACTION_MENU_WIDTH: f32 = 300.0;

/// One row in the dropdown menu. Single-column rows carry a static label
/// (used by view-header column dropdowns); two-column rows carry an owned
/// name + dim right-aligned metadata label (constructed by
/// [`library_selector_popover`] for the runtime-data library filter).
enum DropdownItemData<Key> {
    SingleColumn {
        key: Key,
        label: &'static str,
        checked: bool,
    },
    TwoColumn {
        key: Key,
        name: String,
        right_label: String,
        checked: bool,
    },
    /// A one-shot ACTION row (not a toggle): a leading action icon + a label +
    /// an optional dim consequence subtitle. Rendered by [`dropdown_item_action`]
    /// and used only by [`action_dropdown`], whose widget is built with
    /// `close_on_click = true` so selecting a row fires its action and closes
    /// the menu (the checkbox rows stay open across toggles).
    Action {
        key: Key,
        icon: &'static str,
        label: &'static str,
        /// Dim second line naming the action's effect; an empty string renders
        /// no subtitle (keeps the row single-line).
        subtitle: &'static str,
    },
}

/// Build a checkbox dropdown anchored to a trigger icon button.
///
/// `is_open` and `on_open_change` make this a controlled component — open
/// state lives on the parent (so a single root-level menu coordinator can
/// enforce mutual exclusion with other overlay menus). When opening, the
/// callback receives the trigger's screen-space bounds so the parent can
/// stash them in `OpenMenu::CheckboxDropdown`. The bounds come back via
/// `trigger_bounds` so the overlay can anchor below the trigger.
pub(crate) fn checkbox_dropdown<'a, Key, Message>(
    trigger_icon: &'static str,
    tooltip: &'static str,
    items: Vec<(Key, &'static str, bool)>,
    on_item_toggle: impl Fn(Key) -> Message + 'a,
    on_open_change: impl Fn(Option<Rectangle>) -> Message + 'a,
    is_open: bool,
    trigger_bounds: Option<Rectangle>,
) -> CheckboxDropdown<'a, Key, Message>
where
    Key: Copy + 'a,
    Message: Clone + 'a,
{
    let items = items
        .into_iter()
        .map(|(key, label, checked)| DropdownItemData::SingleColumn {
            key,
            label,
            checked,
        })
        .collect();

    CheckboxDropdown {
        trigger: trigger_button(trigger_icon, tooltip),
        items,
        on_item_toggle: Box::new(on_item_toggle),
        on_open_change: Box::new(on_open_change),
        is_open,
        trigger_bounds,
        header: None,
        menu_width: MENU_MIN_WIDTH,
        menu: None,
        close_on_click: false,
    }
}

/// Build an ACTION dropdown anchored to a header trigger icon: a close-on-click
/// menu of one-shot actions (leading icon + label + optional consequence
/// subtitle), as opposed to the stay-open checkbox rows of [`checkbox_dropdown`].
///
/// Reuses the shared header-anchored-menu chassis (trigger-bounds capture,
/// single-active `OpenMenu` mutual exclusion, Escape / outside-press dismissal,
/// shadow-halo anchoring) for a *fire-and-close* menu: each row publishes
/// `on_select(key)` and the widget additionally publishes `on_open_change(None)`
/// in the same frame so the menu closes. Used by the Queue view's server-sync
/// (push / pull) menu.
///
/// Precedent: group **two or more related, occasional, consequential** actions
/// behind one neutral trigger; keep frequent or standalone actions (Refresh,
/// center-on-playing) as direct header buttons.
///
/// Items are `(key, icon_path, label, subtitle)`; pass `""` for `subtitle` to
/// keep a row single-line.
pub(crate) fn action_dropdown<'a, Key, Message>(
    trigger_icon: &'static str,
    tooltip: &'static str,
    items: Vec<(Key, &'static str, &'static str, &'static str)>,
    on_select: impl Fn(Key) -> Message + 'a,
    on_open_change: impl Fn(Option<Rectangle>) -> Message + 'a,
    is_open: bool,
    trigger_bounds: Option<Rectangle>,
) -> CheckboxDropdown<'a, Key, Message>
where
    Key: Copy + 'a,
    Message: Clone + 'a,
{
    let items = items
        .into_iter()
        .map(|(key, icon, label, subtitle)| DropdownItemData::Action {
            key,
            icon,
            label,
            subtitle,
        })
        .collect();

    CheckboxDropdown {
        trigger: trigger_button(trigger_icon, tooltip),
        items,
        on_item_toggle: Box::new(on_select),
        on_open_change: Box::new(on_open_change),
        is_open,
        trigger_bounds,
        header: None,
        menu_width: ACTION_MENU_WIDTH,
        menu: None,
        close_on_click: true,
    }
}

/// Overlay-only constructor for the library-filter popover. Renders the
/// two-column row variant with owned `String` name + dim right-aligned
/// metadata label; the trigger lives outside this widget (the nav-bar
/// `library_filter_trigger` has its own icon + count + pip chrome that
/// the standard internal trigger button cannot represent).
///
/// The widget itself renders a zero-size `Space` as its "trigger" — it
/// takes no layout space and intercepts no clicks. The overlay still
/// anchors to the externally-captured `trigger_bounds`, so the popover
/// appears below the parent's visible trigger button.
///
/// The parent's trigger button is responsible for emitting the
/// open / close `on_open_change` messages on left-click; this widget
/// only handles row clicks, click-outside-to-close, and Escape.
///
/// Each item is `(id, name, right_label, checked)`:
/// - `name` is the primary label (ellipsized at [`MAX_NAME_WIDTH`]).
/// - `right_label` is the dim right-aligned metadata (e.g. a song count).
///   Pass an empty string when no metadata is available.
/// - `checked` drives the leading filled-or-outlined checkbox glyph.
///
/// `active_count` / `total_count` populate the header row's right-side
/// counter ("Active Libraries — 3 / 5").
pub(crate) fn library_selector_popover<'a, Message>(
    items: Vec<(i32, String, String, bool)>,
    active_count: usize,
    total_count: usize,
    on_item_toggle: impl Fn(i32) -> Message + 'a,
    on_open_change: impl Fn(Option<Rectangle>) -> Message + 'a,
    is_open: bool,
    trigger_bounds: Option<Rectangle>,
) -> CheckboxDropdown<'a, i32, Message>
where
    Message: Clone + 'a,
{
    let items = items
        .into_iter()
        .map(
            |(key, name, right_label, checked)| DropdownItemData::TwoColumn {
                key,
                name,
                right_label,
                checked,
            },
        )
        .collect();

    CheckboxDropdown {
        trigger: iced::widget::Space::new().into(),
        items,
        on_item_toggle: Box::new(on_item_toggle),
        on_open_change: Box::new(on_open_change),
        is_open,
        trigger_bounds,
        header: Some(DropdownHeader {
            label: "Active Libraries".to_string(),
            counter: format!("{active_count} / {total_count}"),
        }),
        menu_width: LIBRARY_SELECTOR_WIDTH,
        menu: None,
        close_on_click: false,
    }
}

/// Drop-in wrapper around [`checkbox_dropdown`] that pre-builds the
/// `OpenMenu::CheckboxDropdown { view, trigger_bounds }` open-change message
/// so each view only supplies its column items, toggle-message constructor,
/// and `SetOpenMenu` handler.
pub(crate) fn view_columns_dropdown<'a, Key, Message>(
    view: crate::View,
    items: Vec<(Key, &'static str, bool)>,
    on_toggle: impl Fn(Key) -> Message + 'a,
    on_set_open_menu: impl Fn(Option<crate::app_message::OpenMenu>) -> Message + 'a,
    is_open: bool,
    trigger_bounds: Option<iced::Rectangle>,
) -> CheckboxDropdown<'a, Key, Message>
where
    Key: Copy + 'a,
    Message: Clone + 'a,
{
    checkbox_dropdown(
        "assets/icons/columns-3-cog.svg",
        "Show/hide columns",
        items,
        on_toggle,
        move |rect| match rect {
            Some(b) => on_set_open_menu(Some(crate::app_message::OpenMenu::CheckboxDropdown {
                view,
                trigger_bounds: b,
            })),
            None => on_set_open_menu(None),
        },
        is_open,
        trigger_bounds,
    )
}

/// Like [`view_columns_dropdown`] for the Similar panel, which uses
/// `OpenMenu::CheckboxDropdownSimilar` because `View` has no `Similar` variant.
pub(crate) fn similar_columns_dropdown<'a, Key, Message>(
    items: Vec<(Key, &'static str, bool)>,
    on_toggle: impl Fn(Key) -> Message + 'a,
    on_set_open_menu: impl Fn(Option<crate::app_message::OpenMenu>) -> Message + 'a,
    is_open: bool,
    trigger_bounds: Option<iced::Rectangle>,
) -> CheckboxDropdown<'a, Key, Message>
where
    Key: Copy + 'a,
    Message: Clone + 'a,
{
    checkbox_dropdown(
        "assets/icons/columns-3-cog.svg",
        "Show/hide columns",
        items,
        on_toggle,
        move |rect| match rect {
            Some(b) => on_set_open_menu(Some(
                crate::app_message::OpenMenu::CheckboxDropdownSimilar { trigger_bounds: b },
            )),
            None => on_set_open_menu(None),
        },
        is_open,
        trigger_bounds,
    )
}

/// Like [`view_columns_dropdown`] for the smart-playlist rules editor's
/// preview/results pane, which uses `OpenMenu::CheckboxDropdownPreview`
/// because that surface has no matching `View` variant.
pub(crate) fn preview_columns_dropdown<'a, Key, Message>(
    items: Vec<(Key, &'static str, bool)>,
    on_toggle: impl Fn(Key) -> Message + 'a,
    on_set_open_menu: impl Fn(Option<crate::app_message::OpenMenu>) -> Message + 'a,
    is_open: bool,
    trigger_bounds: Option<iced::Rectangle>,
) -> CheckboxDropdown<'a, Key, Message>
where
    Key: Copy + 'a,
    Message: Clone + 'a,
{
    checkbox_dropdown(
        "assets/icons/columns-3-cog.svg",
        "Show/hide columns",
        items,
        on_toggle,
        move |rect| match rect {
            Some(b) => on_set_open_menu(Some(
                crate::app_message::OpenMenu::CheckboxDropdownPreview { trigger_bounds: b },
            )),
            None => on_set_open_menu(None),
        },
        is_open,
        trigger_bounds,
    )
}

/// Build the trigger element — a transparent 44×50 icon cell matching the
/// surrounding `view_header` icon buttons, with a `HoverOverlay` for the
/// hover/press feedback. Square hover corners regardless of the global
/// rounded-mode toggle: the view header itself stays flat in both modes,
/// so its embedded trigger must too. No `on_press` here —
/// `CheckboxDropdown`'s widget impl intercepts the left-click itself.
fn trigger_button<'a, Message: 'a>(
    icon_path: &'static str,
    tooltip_text: &'static str,
) -> Element<'a, Message> {
    let icon = crate::embedded_svg::svg_widget(icon_path)
        .width(Length::Fixed(TRIGGER_ICON_SIZE))
        .height(Length::Fixed(TRIGGER_ICON_SIZE))
        .style(|_theme, _status| svg::Style {
            color: Some(theme::fg0()),
        });

    let chassis = container(icon)
        .width(Length::Fixed(44.0))
        .height(Length::Fixed(50.0))
        .align_x(iced::Alignment::Center)
        .align_y(iced::Alignment::Center);

    let with_hover = super::hover_overlay::HoverOverlay::new(chassis).border_radius(0.0.into());

    tooltip(
        with_hover,
        container(text(tooltip_text).size(11.0).font(theme::ui_font())).padding(4),
        tooltip::Position::Top,
    )
    .gap(4)
    .style(theme::container_tooltip)
    .into()
}

/// Render a single dropdown item: styled checkbox glyph + label. The glyph is
/// the shared [`super::checkbox_glyph::element`] — identical to the two-column
/// (library-filter) rows and the kebab `player_modes_menu`, so the three menu
/// families stay visually in lockstep.
fn dropdown_item<'a, Message: Clone + 'a>(
    label: &str,
    checked: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let check_element = super::checkbox_glyph::element::<Message>(checked);

    let row_content = row![
        check_element,
        text(label.to_string())
            .size(MENU_TEXT_SIZE)
            .font(theme::ui_font())
            .color(theme::fg0()),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    // `HoverOverlay(container)` so the hover tint resolves cleanly
    // across themes (see `.claude/rules/gotchas.md` "HoverOverlay
    // canonical pattern"). `ui_radius_xs()` matches the
    // panel's `ui_radius_md()` outline at concentric scale.
    mouse_area(
        super::hover_overlay::HoverOverlay::new(
            container(row_content)
                .width(Length::Fill)
                .padding(iced::Padding {
                    left: 8.0,
                    right: 16.0,
                    top: 4.0,
                    bottom: 4.0,
                })
                .style(|_theme| container::Style {
                    background: None,
                    border: iced::Border {
                        radius: theme::ui_radius_xs(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
        )
        .border_radius(theme::ui_radius_xs()),
    )
    .on_press(on_press)
    .interaction(iced::mouse::Interaction::Pointer)
    .into()
}

/// Render a two-column dropdown item: styled checkbox glyph + name
/// (ellipsized at [`MAX_NAME_WIDTH`]) + flexible spacer + dim
/// right-aligned metadata label.
///
/// Used by the library selector. The checkbox glyph is a filled
/// `accent_bright` rounded square with a centered `check.svg` (checked)
/// or an outlined rounded square (unchecked) — see
/// [`super::checkbox_glyph::element`]. Padding is roomier than
/// [`dropdown_item`] to give the library names breathing space and match
/// the airier design intent.
fn dropdown_item_two_column<'a, Message: Clone + 'a>(
    name: &str,
    right_label: &str,
    checked: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let check_element = super::checkbox_glyph::element::<Message>(checked);

    let name_text = text(name.to_string())
        .size(MENU_TEXT_SIZE)
        .font(theme::ui_font())
        .color(theme::fg0())
        .width(Length::Fixed(MAX_NAME_WIDTH))
        .wrapping(iced::widget::text::Wrapping::None)
        .ellipsis(iced::widget::text::Ellipsis::End);

    let right_text = text(right_label.to_string())
        .size(MENU_TEXT_SIZE)
        .font(theme::ui_font())
        .color(theme::fg2());

    let row_content = row![
        check_element,
        name_text,
        iced::widget::Space::new().width(Length::Fill),
        right_text,
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center);

    // Same hover-overlay pattern as `dropdown_item` — see comment there.
    // Padding is roomier here (12 / 8 vs 8 / 4) because library names
    // need more breathing space than column-toggle labels.
    mouse_area(
        super::hover_overlay::HoverOverlay::new(
            container(row_content)
                .width(Length::Fill)
                .padding(iced::Padding {
                    left: 12.0,
                    right: 16.0,
                    top: 8.0,
                    bottom: 8.0,
                })
                .style(|_theme| container::Style {
                    background: None,
                    border: iced::Border {
                        radius: theme::ui_radius_xs(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
        )
        .border_radius(theme::ui_radius_xs()),
    )
    .on_press(on_press)
    .interaction(iced::mouse::Interaction::Pointer)
    .into()
}

/// Render a one-shot ACTION row: leading action icon + label, with an optional
/// dim consequence subtitle stacked under the label. No checkbox glyph (nothing
/// is toggled). Same `HoverOverlay(container)` hover chassis as [`dropdown_item`]
/// so the tint resolves cleanly across themes; the whole row publishes
/// `on_press` and the widget closes the menu (`close_on_click = true`).
fn dropdown_item_action<'a, Message: Clone + 'a>(
    icon: &'static str,
    label: &str,
    subtitle: &str,
    on_press: Message,
) -> Element<'a, Message> {
    let icon_svg = crate::embedded_svg::svg_widget(icon)
        .width(Length::Fixed(MENU_ICON_SIZE))
        .height(Length::Fixed(MENU_ICON_SIZE))
        .style(|_theme, _status| svg::Style {
            color: Some(theme::fg0()),
        });

    let mut text_col = column![
        text(label.to_string())
            .size(MENU_TEXT_SIZE)
            .font(theme::weighted_ui_font(iced::font::Weight::Medium))
            .color(theme::fg0()),
    ]
    .spacing(2);
    if !subtitle.is_empty() {
        text_col = text_col.push(
            text(subtitle.to_string())
                .size(MENU_TEXT_SIZE - 2.0)
                .font(theme::ui_font())
                .color(theme::fg2()),
        );
    }

    let row_content = row![
        container(icon_svg).align_y(iced::Alignment::Center),
        text_col,
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center);

    mouse_area(
        super::hover_overlay::HoverOverlay::new(
            container(row_content)
                .width(Length::Fill)
                .padding(iced::Padding {
                    left: 10.0,
                    right: 16.0,
                    top: 6.0,
                    bottom: 6.0,
                })
                .style(|_theme| container::Style {
                    background: None,
                    border: iced::Border {
                        radius: theme::ui_radius_xs(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
        )
        .border_radius(theme::ui_radius_xs()),
    )
    .on_press(on_press)
    .interaction(iced::mouse::Interaction::Pointer)
    .into()
}

/// Build the menu element that floats below the trigger when open. Dispatches
/// each item to the matching row renderer based on its variant. When `header`
/// is set, prepends the header row + a 1 px separator above the items.
fn build_menu_element<'a, Key, Message>(
    items: &[DropdownItemData<Key>],
    header: Option<&DropdownHeader>,
    menu_width: f32,
    on_item_toggle: &dyn Fn(Key) -> Message,
) -> Element<'a, Message>
where
    Key: Copy,
    Message: Clone + 'a,
{
    let mut rows: Vec<Element<'a, Message>> = Vec::with_capacity(items.len() + 2);
    if let Some(h) = header {
        rows.push(dropdown_header_row(&h.label, &h.counter));
        // 1 px separator under the header so the title row reads as
        // its own band. Color matches `theme::border()` (the panel
        // outline) for visual coherence with the new chrome.
        rows.push(
            container(iced::widget::Space::new())
                .width(Length::Fill)
                .height(Length::Fixed(1.0))
                .style(|_| container::Style {
                    background: Some(theme::border().into()),
                    ..Default::default()
                })
                .into(),
        );
    }
    for item in items {
        let row = match item {
            DropdownItemData::SingleColumn {
                key,
                label,
                checked,
            } => dropdown_item(label, *checked, on_item_toggle(*key)),
            DropdownItemData::TwoColumn {
                key,
                name,
                right_label,
                checked,
            } => dropdown_item_two_column(name, right_label, *checked, on_item_toggle(*key)),
            DropdownItemData::Action {
                key,
                icon,
                label,
                subtitle,
            } => dropdown_item_action(icon, label, subtitle, on_item_toggle(*key)),
        };
        rows.push(row);
    }

    // Shared menu-panel chrome — see `widgets::menu_chrome`.
    container(column(rows).spacing(0))
        .width(Length::Fixed(menu_width))
        .padding(4)
        .style(super::menu_chrome::container_style)
        .into()
}

/// Non-clickable title row at the top of a header-equipped popover.
/// Renders `[ label (bold fg0) ........ counter (dim fg2) ]` at the same
/// padding profile as the data rows so the two read as one stack.
fn dropdown_header_row<'a, Message: 'a>(label: &str, counter: &str) -> Element<'a, Message> {
    let label_text = text(label.to_string())
        .size(MENU_TEXT_SIZE)
        .font(theme::weighted_ui_font(iced::font::Weight::Bold))
        .color(theme::fg0());

    let counter_text = text(counter.to_string())
        .size(MENU_TEXT_SIZE)
        .font(theme::ui_font())
        .color(theme::fg2());

    let row_content = row![
        label_text,
        iced::widget::Space::new().width(Length::Fill),
        counter_text,
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    container(row_content)
        .width(Length::Fill)
        .padding(iced::Padding {
            left: 12.0,
            right: 16.0,
            top: 8.0,
            bottom: 8.0,
        })
        .into()
}

// ============================================================================
// Widget
// ============================================================================

/// Optional menu header — a single non-clickable row at the top of the
/// dropdown panel that names the menu and shows a "{active} / {total}"
/// counter. Used by [`library_selector_popover`] to surface the popover
/// title + active-libraries count without burning one of the toggle
/// rows for it.
#[derive(Clone)]
struct DropdownHeader {
    label: String,
    counter: String,
}

pub struct CheckboxDropdown<'a, Key, Message> {
    trigger: Element<'a, Message>,
    items: Vec<DropdownItemData<Key>>,
    on_item_toggle: Box<dyn Fn(Key) -> Message + 'a>,
    /// Emitted with `Some(trigger_bounds)` to request open at those bounds, or
    /// `None` to request close.
    on_open_change: Box<dyn Fn(Option<Rectangle>) -> Message + 'a>,
    /// Whether the dropdown should currently render. Mirrors the parent's
    /// `Nokkvi.open_menu == Some(OpenMenu::CheckboxDropdown { .. })` for this
    /// widget's view.
    is_open: bool,
    /// Trigger bounds captured by the parent at open time (lives in
    /// `OpenMenu::CheckboxDropdown { trigger_bounds }`). The overlay anchors
    /// below this rectangle.
    trigger_bounds: Option<Rectangle>,
    /// Optional non-clickable header row at the top of the menu. `None`
    /// (the default) preserves the original headless layout used by the
    /// view-column dropdowns.
    header: Option<DropdownHeader>,
    /// Fixed menu width in pixels. Column dropdowns use
    /// [`MENU_MIN_WIDTH`]; the library selector overrides this with
    /// [`LIBRARY_SELECTOR_WIDTH`] for the wider two-column layout.
    menu_width: f32,
    /// Cached menu element, rebuilt each frame the dropdown is open.
    menu: Option<Element<'a, Message>>,
    /// When `true` (action dropdowns), selecting a row publishes
    /// `on_open_change(None)` in the same frame the row fires its action, so the
    /// menu closes on click. Checkbox dropdowns pass `false` and stay open.
    close_on_click: bool,
}

/// Tree-state. We still keep `menu_tree` because the overlay's button widgets
/// need their hover/press state to persist across frames; only `Status` is
/// lifted out (now controlled by the parent via `is_open`).
#[derive(Debug)]
struct State {
    menu_tree: widget::Tree,
}

impl State {
    fn new() -> Self {
        Self {
            menu_tree: widget::Tree::empty(),
        }
    }
}

impl<'a, Key, Message> Widget<Message, Theme, iced::Renderer> for CheckboxDropdown<'a, Key, Message>
where
    Key: Copy + 'a,
    Message: Clone + 'a,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::new())
    }

    fn diff(&mut self, tree: &mut widget::Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.trigger));
    }

    fn size(&self) -> Size<Length> {
        self.trigger.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.trigger
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.trigger.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        // Intercept left-click on the trigger bounds to toggle open/closed.
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && cursor.position_over(layout.bounds()).is_some()
        {
            let intent = if self.is_open {
                None
            } else {
                Some(layout.bounds())
            };
            shell.publish((self.on_open_change)(intent));
            shell.capture_event();
            shell.request_redraw();
            return;
        }

        // Forward all other events to the trigger child (e.g. for cursor
        // interaction tracking — the trigger is a plain container, so nothing
        // load-bearing happens here, but pass through for completeness).
        self.trigger.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if cursor.position_over(layout.bounds()).is_some() {
            return mouse::Interaction::Pointer;
        }
        self.trigger.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        let trigger_state = tree.children.first_mut()?;
        let trigger_overlay = self.trigger.as_widget_mut().overlay(
            trigger_state,
            layout,
            renderer,
            viewport,
            translation,
        );

        let state = tree.state.downcast_mut::<State>();
        let our_overlay = if self.is_open {
            build_overlay(
                state,
                &mut self.menu,
                &self.items,
                self.header.as_ref(),
                self.menu_width,
                &*self.on_item_toggle,
                &*self.on_open_change,
                self.trigger_bounds,
                translation,
                self.close_on_click,
            )
        } else {
            // Drop any cached menu element + reset the persisted tree so the
            // next open starts fresh.
            self.menu = None;
            state.menu_tree = widget::Tree::empty();
            None
        };

        if trigger_overlay.is_none() && our_overlay.is_none() {
            None
        } else {
            Some(
                overlay::Group::with_children(
                    trigger_overlay.into_iter().chain(our_overlay).collect(),
                )
                .overlay(),
            )
        }
    }
}

impl<'a, Key, Message> From<CheckboxDropdown<'a, Key, Message>> for Element<'a, Message>
where
    Key: Copy + 'a,
    Message: Clone + 'a,
{
    fn from(dropdown: CheckboxDropdown<'a, Key, Message>) -> Self {
        Element::new(dropdown)
    }
}

// ============================================================================
// Overlay Builder
// ============================================================================

#[allow(clippy::too_many_arguments)]
fn build_overlay<'a, 'b, Key, Message>(
    state: &'b mut State,
    menu: &'b mut Option<Element<'a, Message>>,
    items: &[DropdownItemData<Key>],
    header: Option<&DropdownHeader>,
    menu_width: f32,
    on_item_toggle: &dyn Fn(Key) -> Message,
    on_open_change: &'b dyn Fn(Option<Rectangle>) -> Message,
    trigger_bounds: Option<Rectangle>,
    translation: Vector,
    close_on_click: bool,
) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>>
where
    Key: Copy,
    Message: Clone + 'a,
{
    if items.is_empty() {
        return None;
    }
    // Without trigger bounds we can't anchor the overlay; bail out (this is a
    // transient state — the parent will dispatch the bounds in the same frame).
    let trigger_bounds = trigger_bounds?;

    let m =
        menu.get_or_insert_with(|| build_menu_element(items, header, menu_width, on_item_toggle));
    // diff unconditionally: iced's `Tree::new` no longer eagerly populates children,
    // so the old `is_empty()` guard would leave the overlay rendering against an empty
    // child tree. diff allocates+populates a fresh tree and reconciles a populated one,
    // preserving the menu buttons' state across the per-frame view rebuild.
    state.menu_tree.diff(&mut *m as &mut Element<'a, Message>);

    menu.as_mut().map(|m| {
        overlay::Element::new(Box::new(MenuOverlay {
            menu: m,
            state,
            on_open_change,
            trigger_bounds: trigger_bounds + translation,
            close_on_click,
        }))
    })
}

// ============================================================================
// Menu Overlay
// ============================================================================

struct MenuOverlay<'a, 'b, Message> {
    menu: &'b mut Element<'a, Message>,
    state: &'b mut State,
    on_open_change: &'b dyn Fn(Option<Rectangle>) -> Message,
    trigger_bounds: Rectangle,
    /// Action dropdowns (`close_on_click = true`) dismiss on row selection;
    /// checkbox dropdowns stay open. See [`action_dropdown`].
    close_on_click: bool,
}

impl<Message> overlay::Overlay<Message, Theme, iced::Renderer> for MenuOverlay<'_, '_, Message> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let limits = layout::Limits::new(Size::ZERO, bounds)
            .width(Length::Shrink)
            .height(Length::Shrink);

        let menu_node =
            self.menu
                .as_widget_mut()
                .layout(&mut self.state.menu_tree, renderer, &limits);

        // Anchor below the trigger, right-aligned to its right edge so the
        // menu doesn't visually pull away from the icon.
        let menu_size = menu_node.size();
        let mut x = self.trigger_bounds.x + self.trigger_bounds.width - menu_size.width;
        let mut y = self.trigger_bounds.y + self.trigger_bounds.height + 4.0;

        // Clamp inside the viewport (with a small inset).
        let padding = 5.0;
        let max_x = bounds.width - padding - menu_size.width;
        let max_y = bounds.height - padding - menu_size.height;
        if x < padding {
            x = padding;
        } else if x > max_x {
            x = max_x.max(padding);
        }
        if y < padding {
            y = padding;
        } else if y > max_y {
            // Fall back to anchoring above the trigger if there's no room
            // below.
            y = (self.trigger_bounds.y - menu_size.height - 4.0).max(padding);
        }

        inflate_for_shadow_around_child(menu_node, Point::new(x, y))
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
    ) {
        // Escape / outside-press dismissal — see `widgets::menu_dismiss` for
        // the capture semantics (outside presses deliberately stay
        // uncaptured). Mouse presses only — historical; the trigger rect
        // counts as inside because the trigger's own Widget::update toggles
        // when clicked, so we leave that case alone.
        if menu_dismiss::handle_dismiss(
            event,
            shell,
            || {
                matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_)))
                    && cursor
                        .position_over(visible_menu_layout(layout).bounds())
                        .is_none()
                    && cursor.position_over(self.trigger_bounds).is_none()
            },
            || (self.on_open_change)(None),
        ) {
            return;
        }

        let menu_layout = visible_menu_layout(layout);
        let menu_bounds = menu_layout.bounds();

        // Forward to menu content so item mouse_areas fire on_press. Checkbox
        // rows stay open on click (the user can flip several toggles in one
        // open); action rows are closed just below via `close_on_click`.
        self.menu.as_widget_mut().update(
            &mut self.state.menu_tree,
            event,
            menu_layout,
            cursor,
            renderer,
            shell,
            &menu_bounds,
        );

        // Action dropdowns dismiss on selection: the child `mouse_area` above
        // already published the row's action into `shell` for this press (and
        // captured the event); publish the close in the same frame so the menu
        // goes away, then capture so a press that landed on the menu's own
        // padding (not on a row, so the child never captured) can't also click
        // through to the view behind. Outside presses were handled by
        // `handle_dismiss` (deliberately uncaptured so the click can switch to
        // another menu's trigger), and a press on the trigger never reaches this
        // overlay — the trigger's own `Widget::update` toggles it. Mirrors the
        // "publish action then SetOpenMenu(None)" contract of the context-menu /
        // hamburger action menus. Left-mouse only, matching this dropdown
        // family's mouse-only trigger-open (`Widget::update`) and outside-dismiss
        // predicate — no touch backend is wired for any dropdown here, and
        // Escape dismisses regardless.
        if self.close_on_click
            && matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            )
            && cursor.position_over(menu_bounds).is_some()
        {
            shell.publish((self.on_open_change)(None));
            shell.capture_event();
        }
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let menu_layout = visible_menu_layout(layout);
        self.menu.as_widget().draw(
            &self.state.menu_tree,
            renderer,
            theme,
            style,
            menu_layout,
            cursor,
            &menu_layout.bounds(),
        );
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let menu_layout = visible_menu_layout(layout);
        self.menu.as_widget().mouse_interaction(
            &self.state.menu_tree,
            menu_layout,
            cursor,
            &menu_layout.bounds(),
            renderer,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Placeholder message used by the smoke tests below. Mirrors the shape a
    /// real call site (e.g. the library-filter popover) would use: one Toggle
    /// arm for item presses and one OpenChange arm for trigger / outside-click
    /// / Escape events. The inner payloads are never read — these tests only
    /// exercise constructor / `Into<Element>` plumbing.
    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    enum TestMessage {
        Toggle(i32),
        OpenChange(Option<iced::Rectangle>),
    }

    #[test]
    fn checkbox_dropdown_compiles_with_typical_inputs() {
        // Smoke test: the public API accepts the expected shape and produces
        // a valid Element. Uses usize as the key type for compactness.
        let items: Vec<(usize, &'static str, bool)> = vec![
            (0, "Stars", true),
            (1, "Album", false),
            (2, "Duration", true),
        ];
        let _el: Element<'_, String> = checkbox_dropdown(
            "assets/icons/columns-3-cog.svg",
            "Show/hide columns",
            items,
            |key: usize| format!("toggle-{key}"),
            |bounds| match bounds {
                Some(_) => "open".to_string(),
                None => "close".to_string(),
            },
            false,
            None,
        )
        .into();
    }

    #[test]
    fn empty_items_still_produces_element() {
        // Edge case: empty items vector is a valid input (overlay just won't
        // render anything when opened).
        let _el: Element<'_, String> = checkbox_dropdown(
            "assets/icons/columns-3-cog.svg",
            "Show/hide columns",
            Vec::<(usize, &'static str, bool)>::new(),
            |key: usize| format!("toggle-{key}"),
            |_| "noop".to_string(),
            false,
            None,
        )
        .into();
    }

    #[test]
    fn action_dropdown_compiles_with_typical_inputs() {
        // Two close-on-click action rows with leading icon + label + consequence
        // subtitle — the shape the Queue server-sync (push / pull) menu produces.
        // The icon-path string literals here are also swept by the
        // `all_svg_paths_in_source_are_registered` guard, so they double as a
        // registration check for the sync glyphs.
        let items: Vec<(usize, &'static str, &'static str, &'static str)> = vec![
            (
                0,
                "assets/icons/arrow-up-to-line.svg",
                "Push to server",
                "Replaces the queue saved on the server",
            ),
            (
                1,
                "assets/icons/arrow-down-to-line.svg",
                "Pull from server",
                "Replaces your local queue",
            ),
        ];
        let _el: Element<'_, String> = action_dropdown(
            "assets/icons/arrow-down-up.svg",
            "Server queue (push / pull)",
            items,
            |key: usize| format!("action-{key}"),
            |bounds| match bounds {
                Some(_) => "open".to_string(),
                None => "close".to_string(),
            },
            true,
            Some(iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: 44.0,
                height: 50.0,
            }),
        )
        .into();
    }

    #[test]
    fn library_selector_popover_compiles_with_zero_items() {
        // Degenerate-but-valid input: empty popover (pre-login, cold cache).
        // Must not panic during construction or conversion to Element; the
        // overlay path short-circuits at `items.is_empty()` if it ever opens.
        let items: Vec<(i32, String, String, bool)> = Vec::new();
        let _el: Element<'_, TestMessage> = library_selector_popover(
            items,
            0, // active_count
            0, // total_count
            TestMessage::Toggle,
            TestMessage::OpenChange,
            false,
            None,
        )
        .into();
    }

    #[test]
    fn library_selector_popover_compiles_with_three_items() {
        // Typical input: three rows with owned name + right-label strings —
        // the shape the nav-bar library-filter popover produces. Mixed
        // checked / unchecked covers both row-icon code paths.
        let items: Vec<(i32, String, String, bool)> = vec![
            (1, "Music Library".to_string(), "13,627".to_string(), true),
            (
                2,
                "Longmont Potion Castle".to_string(),
                "412".to_string(),
                false,
            ),
            (3, "Field Recordings".to_string(), "58".to_string(), true),
        ];
        let _el: Element<'_, TestMessage> = library_selector_popover(
            items,
            2, // active_count: two of three checked above
            3, // total_count
            TestMessage::Toggle,
            TestMessage::OpenChange,
            true,
            Some(iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 40.0,
            }),
        )
        .into();
    }
}
