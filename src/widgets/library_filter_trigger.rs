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

/// Width when the count label is shown alongside the icon. Tuned to fit
/// `📚 N/M` at the small badge font without crowding the pip overlay.
const FILTERED_WIDTH: f32 = 44.0;

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

// ============================================================================
// Public API
// ============================================================================

/// Build the library-filter nav-bar trigger.
///
/// Returns `Space::new()` when `library_count <= 1` (suppressed
/// state); otherwise returns the trigger widget.
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
        match self.mode {
            RenderMode::Neutral => ICON_ONLY_WIDTH,
            RenderMode::Filtered { .. } => FILTERED_WIDTH,
        }
    }
}

impl<'a, Message: Clone + 'a> Widget<Message, Theme, iced::Renderer>
    for LibraryFilterTrigger<'a, Message>
{
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(self.button_width()),
            height: Length::Fixed(BUTTON_HEIGHT),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &iced::Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::new(self.button_width(), BUTTON_HEIGHT))
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
        // by the `HoverOverlay` wrapper at the call site.
        let bg_color = if self.is_open {
            theme::accent_bright()
        } else {
            theme::bg0_hard()
        };
        let fg_color = if self.is_open {
            theme::bg0()
        } else {
            theme::fg1()
        };

        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: iced::Border {
                    radius: theme::ui_border_radius(),
                    ..Default::default()
                },
                ..Default::default()
            },
            bg_color,
        );

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
                let text_bounds = Rectangle {
                    x: text_x,
                    y: bounds.y,
                    width: (bounds.x + bounds.width - FILTERED_HPAD - text_x).max(0.0),
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
            library_filter_trigger(0, 0, false, None, dummy_callback());
    }

    #[test]
    fn library_count_one_is_suppressed() {
        // Single-library servers never show the filter — there's
        // nothing to toggle.
        let _el: Element<'_, TestMessage> =
            library_filter_trigger(1, 1, false, None, dummy_callback());
    }

    #[test]
    fn library_count_five_renders_trigger() {
        // Sanity check that the multi-library case constructs without
        // panic.
        let _el: Element<'_, TestMessage> =
            library_filter_trigger(5, 5, false, None, dummy_callback());
    }

    #[test]
    fn five_libraries_zero_active_is_neutral() {
        // Empty set means "all libraries on" (the empty-set-as-all
        // rule). Render the neutral chassis, not a "filtered" badge.
        let _el: Element<'_, TestMessage> =
            library_filter_trigger(5, 0, false, None, dummy_callback());
    }

    #[test]
    fn five_libraries_two_active_is_filtered() {
        // Strict subset → filtered render: icon + "2/5" label + pip.
        let _el: Element<'_, TestMessage> =
            library_filter_trigger(5, 2, false, None, dummy_callback());
    }

    #[test]
    fn five_libraries_all_active_is_neutral() {
        // active == total is semantically "no filter" — same render as
        // active == 0.
        let _el: Element<'_, TestMessage> =
            library_filter_trigger(5, 5, false, None, dummy_callback());
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
            library_filter_trigger(5, 5, true, Some(bounds), dummy_callback());
        let _filtered: Element<'_, TestMessage> =
            library_filter_trigger(5, 3, true, Some(bounds), dummy_callback());
    }

    #[test]
    fn active_count_overshoot_clamps_to_neutral() {
        // Active set carries IDs for libraries that have since been
        // deleted server-side — `active > total` until the prune pass
        // runs. Render as if `active == total` (neutral) instead of
        // producing nonsensical "7/5" output.
        let _el: Element<'_, TestMessage> =
            library_filter_trigger(5, 7, false, None, dummy_callback());
    }
}
