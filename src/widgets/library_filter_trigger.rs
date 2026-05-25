//! Library Filter Trigger
//!
//! Nav-bar button that opens the library-filter popover. Three render
//! states drive the visual:
//!
//! - `library_count <= 1` → returns `Space::new()` so the trigger slot
//!   collapses cleanly. The surrounding `Row` still gets an
//!   `Element`-typed child, so the widget-tree shape stays stable across
//!   re-renders (the iced re-render trap that destroys `text_input`
//!   focus when sibling slot types churn — see
//!   `.agent/rules/gotchas.md` "Widget Tree & Focus" and Plan §14.8).
//! - `active_count == library_count || active_count == 0` → 28 × 28
//!   icon-only button. "All libraries on" and "empty set → treated as
//!   all" are visually identical because they share the same semantics
//!   (no filter is active).
//! - `0 < active_count < library_count` → wider button with `📚 N/M`
//!   text and an 8 px accent-bright pip in the top-right corner. The
//!   pip + text combo is intentionally redundant: the text gives the
//!   exact count, the pip is the at-a-glance "something is filtered"
//!   modifier matching the kebab "modes" trigger.
//!
//! Open/closed is owned by the parent: callers pass `is_open` derived
//! from `Nokkvi.open_menu == Some(OpenMenu::LibrarySelector { .. })`
//! and receive `on_open_change(open, trigger_bounds)`. Bounds are
//! captured at click time so the popover overlay anchors below the
//! trigger without re-reading layout each frame (same contract as
//! [`super::checkbox_dropdown::CheckboxDropdown`]).
//!
//! The trigger only emits the open-change message; the popover panel
//! (built via [`super::checkbox_dropdown::library_selector_popover`])
//! is mounted separately by the nav-bar row layout (see
//! [`super::nav_bar`] `library_trigger_slot`) so the visible trigger and
//! the overlay panel share the same captured trigger bounds.
//!
//! Chassis dimensions match [`super::hamburger_menu`] — 28 px button,
//! 18 px icon — so the trigger reads as a peer of the hamburger when
//! both render side-by-side on the right edge of the nav bar.

use iced::{
    Element, Event, Length, Point, Radians, Rectangle, Size, Theme,
    advanced::{
        Shell,
        layout::{self, Layout},
        renderer,
        svg::{Handle, Svg as SvgData},
        widget::{self, Widget},
    },
    alignment, mouse, touch,
};

use crate::theme;

// ============================================================================
// Constants
// ============================================================================

/// Chassis height — mirrors the hamburger menu so the two triggers
/// line up vertically on the nav bar.
const BUTTON_HEIGHT: f32 = 28.0;

/// Width when only the icon is shown — square chassis matching the
/// hamburger.
const ICON_ONLY_WIDTH: f32 = 28.0;

/// Width when the count label is shown alongside the icon. Sized so the
/// `N/M` text band ends before the pip overlay's horizontal range —
/// see [`draw`] where the text right-edge is clamped to
/// `bounds.right - BADGE_INSET - BADGE_DIAMETER - 3` (gap) so the pip
/// never sits on top of a digit.
const FILTERED_WIDTH: f32 = 56.0;

/// Icon glyph size — matches the hamburger menu's 18 px icon so the two
/// SVGs read at the same visual weight.
const ICON_SIZE: f32 = 18.0;

/// Icon glyph size in filtered mode — slightly smaller to leave room
/// for the count label without overflowing the 28 px chassis height.
const ICON_SIZE_FILTERED: f32 = 14.0;

/// Font size for the `N/M` count badge text.
const COUNT_TEXT_SIZE: f32 = 10.0;

/// Internal horizontal padding for the filtered state.
const FILTERED_HPAD: f32 = 4.0;

/// Height of the compact (side-nav) chassis when the filter is active.
/// Stacks the 18 × 18 icon over a `N/M` count line so the sidebar
/// reads the same information the top-nav strip does, just laid out
/// vertically. Sized for the 28-px sidebar width.
const COMPACT_FILTERED_HEIGHT: f32 = 44.0;

/// Vertical gap (in pixels) between the icon and the stacked count
/// line in the compact-filtered chassis.
const COMPACT_STACK_GAP: f32 = 2.0;

// ============================================================================
// Public API
// ============================================================================

/// Build the library-filter nav-bar trigger.
///
/// Returns `Space::new()` when `library_count <= 1` (suppressed
/// state); otherwise returns the trigger widget.
///
/// `compact` switches to a side-nav-friendly chassis: icon-only width
/// (28 px) in every state, with a pip overlay in the filtered case (no
/// count text — the 28-px sidebar can't fit `N/M` text alongside the
/// icon, and the popover header already surfaces the exact "{active} /
/// {total}" once open).
///
/// `on_open_change(open, trigger_bounds)`:
/// - `(true, Some(bounds))` — user clicked to open. `bounds` is the
///   trigger's screen-space layout rectangle so the popover overlay
///   can anchor below it.
/// - `(false, None)` — user clicked the open trigger to close.
pub(crate) fn library_filter_trigger<'a, Message>(
    library_count: usize,
    active_count: usize,
    is_open: bool,
    compact: bool,
    trigger_bounds: Option<iced::Rectangle>,
    on_open_change: impl Fn(bool, Option<iced::Rectangle>) -> Message + 'a,
) -> iced::Element<'a, Message>
where
    Message: Clone + 'a,
{
    // Suppression gate — `library_count == 0` is treated identically to
    // `library_count == 1` because AppService starts at 0 until the
    // first `refresh_libraries` lands; we don't want a brief flicker of
    // the trigger before the count arrives.
    if library_count <= 1 {
        return iced::widget::Space::new().into();
    }

    // Clamp active_count to the inclusive bound. A `> library_count`
    // value can transiently appear if the active set carries IDs for
    // libraries that have since been deleted server-side, before the
    // refresh-prune pass runs. Render as if all selected, not >100 %.
    let active_clamped = active_count.min(library_count);
    let mode = if active_clamped == 0 || active_clamped == library_count {
        RenderMode::Neutral
    } else {
        RenderMode::Filtered {
            active: active_clamped,
            total: library_count,
        }
    };

    Element::new(LibraryFilterTrigger {
        mode,
        is_open,
        compact,
        _trigger_bounds: trigger_bounds,
        icon_handle: Handle::from_memory(
            crate::embedded_svg::get_svg("assets/icons/library.svg").as_bytes(),
        ),
        on_open_change: Box::new(on_open_change),
    })
}

// ============================================================================
// Widget
// ============================================================================

#[derive(Debug, Clone, Copy)]
enum RenderMode {
    /// All libraries active (or none — treated identically as "no
    /// filter"). Icon-only chassis, no count, no pip.
    Neutral,
    /// Strict subset of libraries active. Icon + `active/total` label
    /// + pip in top-right corner.
    Filtered { active: usize, total: usize },
}

struct LibraryFilterTrigger<'a, Message> {
    mode: RenderMode,
    is_open: bool,
    /// Side-nav variant: icon-only width even when filtered (no `N/M`
    /// text), pip overlay still drawn in the filtered case so the
    /// "something is on" signal carries through into the narrow
    /// sidebar chassis.
    compact: bool,
    /// Plumbed in for completeness with the controlled-component
    /// contract; the trigger itself doesn't need to read it (the parent
    /// derives open-state from `OpenMenu::LibrarySelector` and passes
    /// `is_open`). Held with a leading underscore — the bounds are
    /// consumed by the popover overlay sibling in the nav-bar row layout
    /// rather than by this widget.
    _trigger_bounds: Option<Rectangle>,
    icon_handle: Handle,
    on_open_change: Box<dyn Fn(bool, Option<Rectangle>) -> Message + 'a>,
}

impl<Message> LibraryFilterTrigger<'_, Message> {
    fn button_width(&self) -> f32 {
        if self.compact {
            return ICON_ONLY_WIDTH;
        }
        match self.mode {
            RenderMode::Neutral => ICON_ONLY_WIDTH,
            RenderMode::Filtered { .. } => FILTERED_WIDTH,
        }
    }

    fn button_height(&self) -> f32 {
        if self.compact && matches!(self.mode, RenderMode::Filtered { .. }) {
            COMPACT_FILTERED_HEIGHT
        } else {
            BUTTON_HEIGHT
        }
    }
}

impl<'a, Message: Clone + 'a> Widget<Message, Theme, iced::Renderer>
    for LibraryFilterTrigger<'a, Message>
{
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(self.button_width()),
            height: Length::Fixed(self.button_height()),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &iced::Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::new(self.button_width(), self.button_height()))
    }

    fn update(
        &mut self,
        _tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. })
                if cursor.is_over(bounds) =>
            {
                // Open requests carry the bounds; close requests carry
                // `None` so the parent doesn't need to remember
                // anything stale.
                let next_open = !self.is_open;
                let next_bounds = if next_open { Some(bounds) } else { None };
                shell.publish((self.on_open_change)(next_open, next_bounds));
                shell.capture_event();
                shell.request_redraw();
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        _tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::{
            Renderer,
            svg::Renderer as SvgRenderer,
            text::{Renderer as TextRenderer, Text},
        };

        let bounds = layout.bounds();

        // Background quad — accent-bright when open (matches hamburger
        // "open" affordance), bg0_hard idle. Hover feedback is supplied
        // by the `HoverOverlay` wrapper at the call site. Pill radius
        // (`ui_radius_pill()`) in rounded mode mirrors the `.nk-nav-btn`
        // pill chrome from the design CSS; 0 in flat mode keeps the
        // trigger flush with the surrounding nav cells.
        let bg_color = if self.is_open {
            theme::accent_bright()
        } else {
            theme::bg0_hard()
        };
        let fg_color = if self.is_open {
            theme::bg0_hard()
        } else {
            theme::fg0()
        };

        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: iced::Border {
                    radius: theme::ui_radius_pill(),
                    ..Default::default()
                },
                ..Default::default()
            },
            bg_color,
        );

        // Compact (side-nav) variant. Two sub-modes:
        // - Neutral: centered icon in a 28 × 28 chassis (matches the
        //   sidebar tab cells).
        // - Filtered: icon at top + pip in the icon's top-right corner
        //   + `N/M` text centered below the icon in a 28 × 44 chassis.
        //   Stacked vertically because the 28-px sidebar can't fit
        //   text alongside the icon horizontally.
        if self.compact {
            match self.mode {
                RenderMode::Neutral => {
                    let icon_bounds = Rectangle {
                        x: bounds.center_x() - ICON_SIZE / 2.0,
                        y: bounds.center_y() - ICON_SIZE / 2.0,
                        width: ICON_SIZE,
                        height: ICON_SIZE,
                    };
                    renderer.draw_svg(
                        SvgData {
                            handle: self.icon_handle.clone(),
                            color: Some(fg_color),
                            rotation: Radians(0.0),
                            opacity: 1.0,
                        },
                        icon_bounds,
                        icon_bounds,
                    );
                }
                RenderMode::Filtered { active, total } => {
                    // Icon sits in the upper half so the pip can ride
                    // its top-right corner without spilling above the
                    // chassis.
                    let icon_top = bounds.y + 5.0;
                    let icon_bounds = Rectangle {
                        x: bounds.center_x() - ICON_SIZE / 2.0,
                        y: icon_top,
                        width: ICON_SIZE,
                        height: ICON_SIZE,
                    };
                    renderer.draw_svg(
                        SvgData {
                            handle: self.icon_handle.clone(),
                            color: Some(fg_color),
                            rotation: Radians(0.0),
                            opacity: 1.0,
                        },
                        icon_bounds,
                        icon_bounds,
                    );

                    // Count text centered horizontally, sitting under
                    // the icon with `COMPACT_STACK_GAP` of breathing
                    // space.
                    let label = format!("{active}/{total}");
                    let text_y = icon_bounds.y + icon_bounds.height + COMPACT_STACK_GAP;
                    let text_bounds = Rectangle {
                        x: bounds.x,
                        y: text_y,
                        width: bounds.width,
                        height: (bounds.y + bounds.height - text_y).max(0.0),
                    };
                    renderer.fill_text(
                        Text {
                            content: label,
                            bounds: Size::new(text_bounds.width, text_bounds.height),
                            size: COUNT_TEXT_SIZE.into(),
                            line_height: iced::advanced::text::LineHeight::default(),
                            font: iced::font::Font {
                                weight: iced::font::Weight::Bold,
                                ..theme::ui_font()
                            },
                            align_x: alignment::Horizontal::Center.into(),
                            align_y: alignment::Vertical::Top,
                            shaping: iced::advanced::text::Shaping::default(),
                            wrapping: iced::advanced::text::Wrapping::None,
                            ellipsis: iced::advanced::text::Ellipsis::default(),
                            hint_factor: Some(1.0),
                        },
                        Point::new(text_bounds.center_x(), text_bounds.y),
                        fg_color,
                        text_bounds,
                    );

                    // Pip in the icon's top-right corner. Anchor a
                    // synthetic rect at the icon position so the pip
                    // hugs the icon rather than the wider chassis.
                    super::badge_pip::draw_badge_pip(
                        renderer,
                        Rectangle {
                            x: icon_bounds.x,
                            y: icon_bounds.y - 3.0,
                            width: icon_bounds.width + 4.0,
                            height: icon_bounds.height,
                        },
                    );
                }
            }
            return;
        }

        match self.mode {
            RenderMode::Neutral => {
                // Centered 18×18 icon, no text, no pip.
                let icon_bounds = Rectangle {
                    x: bounds.center_x() - ICON_SIZE / 2.0,
                    y: bounds.center_y() - ICON_SIZE / 2.0,
                    width: ICON_SIZE,
                    height: ICON_SIZE,
                };
                renderer.draw_svg(
                    SvgData {
                        handle: self.icon_handle.clone(),
                        color: Some(fg_color),
                        rotation: Radians(0.0),
                        opacity: 1.0,
                    },
                    icon_bounds,
                    icon_bounds,
                );
            }
            RenderMode::Filtered { active, total } => {
                // Layout: [hpad | icon | gap | text | hpad]
                let icon_bounds = Rectangle {
                    x: bounds.x + FILTERED_HPAD,
                    y: bounds.center_y() - ICON_SIZE_FILTERED / 2.0,
                    width: ICON_SIZE_FILTERED,
                    height: ICON_SIZE_FILTERED,
                };
                renderer.draw_svg(
                    SvgData {
                        handle: self.icon_handle.clone(),
                        color: Some(fg_color),
                        rotation: Radians(0.0),
                        opacity: 1.0,
                    },
                    icon_bounds,
                    icon_bounds,
                );

                let label = format!("{active}/{total}");
                let text_x = icon_bounds.x + icon_bounds.width + 3.0;
                // Pip occupies [bounds.right - BADGE_INSET - BADGE_DIAMETER,
                // bounds.right - BADGE_INSET]. Reserve a 3 px gap before
                // the pip so the rightmost digit can't kiss the dot.
                let pip_left_edge = bounds.x + bounds.width
                    - super::badge_pip::BADGE_INSET
                    - super::badge_pip::BADGE_DIAMETER;
                let text_right_max = (pip_left_edge - 3.0).max(text_x);
                let text_bounds = Rectangle {
                    x: text_x,
                    y: bounds.y,
                    width: (text_right_max - text_x).max(0.0),
                    height: bounds.height,
                };
                renderer.fill_text(
                    Text {
                        content: label,
                        bounds: Size::new(text_bounds.width, text_bounds.height),
                        size: COUNT_TEXT_SIZE.into(),
                        line_height: iced::advanced::text::LineHeight::default(),
                        font: iced::font::Font {
                            weight: iced::font::Weight::Bold,
                            ..theme::ui_font()
                        },
                        align_x: alignment::Horizontal::Left.into(),
                        align_y: alignment::Vertical::Center,
                        shaping: iced::advanced::text::Shaping::default(),
                        wrapping: iced::advanced::text::Wrapping::None,
                        ellipsis: iced::advanced::text::Ellipsis::default(),
                        hint_factor: Some(1.0),
                    },
                    Point::new(text_x, text_bounds.center_y()),
                    fg_color,
                    text_bounds,
                );

                // Pip overlay — only drawn in filtered state (in neutral
                // state the lack of a count is the visual signal, no pip
                // needed). Drawn AFTER the icon/text so it sits on top
                // of any overlap at the top-right corner.
                super::badge_pip::draw_badge_pip(renderer, bounds);
            }
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, Message: Clone + 'a> From<LibraryFilterTrigger<'a, Message>> for Element<'a, Message> {
    fn from(trigger: LibraryFilterTrigger<'a, Message>) -> Self {
        Element::new(trigger)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Stand-in message type — the trigger never publishes on construction,
    /// so a plain unit type is enough to exercise the builder.
    type TestMessage = ();

    fn dummy_callback() -> impl Fn(bool, Option<iced::Rectangle>) -> TestMessage {
        |_open, _bounds| ()
    }

    #[test]
    fn library_count_zero_returns_space_element() {
        // AppService starts at 0 libraries until `refresh_libraries`
        // lands; this gate is what prevents a flicker of the trigger
        // before the count is known.
        let _el: Element<'_, TestMessage> =
            library_filter_trigger(0, 0, false, false, None, dummy_callback());
    }

    #[test]
    fn library_count_one_is_suppressed() {
        // Single-library servers never show the filter — there's
        // nothing to toggle.
        let _el: Element<'_, TestMessage> =
            library_filter_trigger(1, 1, false, false, None, dummy_callback());
    }

    #[test]
    fn library_count_five_renders_trigger() {
        // Sanity check that the multi-library case constructs without
        // panic.
        let _el: Element<'_, TestMessage> =
            library_filter_trigger(5, 5, false, false, None, dummy_callback());
    }

    #[test]
    fn five_libraries_zero_active_is_neutral() {
        // Empty set means "all libraries on" (the empty-set-as-all
        // rule). Render the neutral chassis, not a "filtered" badge.
        let _el: Element<'_, TestMessage> =
            library_filter_trigger(5, 0, false, false, None, dummy_callback());
    }

    #[test]
    fn five_libraries_two_active_is_filtered() {
        // Strict subset → filtered render: icon + "2/5" label + pip.
        let _el: Element<'_, TestMessage> =
            library_filter_trigger(5, 2, false, false, None, dummy_callback());
    }

    #[test]
    fn five_libraries_all_active_is_neutral() {
        // active == total is semantically "no filter" — same render as
        // active == 0.
        let _el: Element<'_, TestMessage> =
            library_filter_trigger(5, 5, false, false, None, dummy_callback());
    }

    #[test]
    fn open_state_constructs() {
        // When the popover is open, the parent passes `is_open=true`
        // plus the bounds it stashed on first click. Both render paths
        // must accept that state without panic.
        let bounds = iced::Rectangle {
            x: 10.0,
            y: 20.0,
            width: 28.0,
            height: 28.0,
        };
        let _neutral: Element<'_, TestMessage> =
            library_filter_trigger(5, 5, true, false, Some(bounds), dummy_callback());
        let _filtered: Element<'_, TestMessage> =
            library_filter_trigger(5, 3, true, false, Some(bounds), dummy_callback());
    }

    #[test]
    fn active_count_overshoot_clamps_to_neutral() {
        // Active set carries IDs for libraries that have since been
        // deleted server-side — `active > total` until the prune pass
        // runs. Render as if `active == total` (neutral) instead of
        // producing nonsensical "7/5" output.
        let _el: Element<'_, TestMessage> =
            library_filter_trigger(5, 7, false, false, None, dummy_callback());
    }

    #[test]
    fn compact_mode_renders_for_side_nav() {
        // Side-nav variant: icon-only width even when filtered, pip
        // still drawn. Construct both neutral and filtered states to
        // exercise the compact branch in `draw`.
        let _neutral: Element<'_, TestMessage> =
            library_filter_trigger(5, 5, false, true, None, dummy_callback());
        let _filtered: Element<'_, TestMessage> =
            library_filter_trigger(5, 2, false, true, None, dummy_callback());
    }
}
