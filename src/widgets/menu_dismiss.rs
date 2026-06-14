//! Shared overlay-menu dismissal handling
//!
//! The four overlay menus (`hamburger_menu`, `player_modes_menu`,
//! `context_menu`, `checkbox_dropdown` — the library-selector popover reuses
//! the latter) all dismiss the same way: Escape closes with the event
//! captured, and a press outside the menu closes WITHOUT capturing. This
//! module centralizes those two actions; each overlay supplies its own
//! outside-press predicate because the historical predicates differ (event
//! set, cursor-unavailable semantics, trigger-rect exemption).
//!
//! The no-capture rule on outside presses is load-bearing: a different
//! menu's trigger may be under the cursor, and iced dispatches overlays
//! before the widget tree, so the trigger's open emit arrives after this
//! close and wins (the root `SetOpenMenu` handler simply replaces the
//! value), achieving the "click another menu's trigger to switch" UX. For a
//! click in genuinely empty space, only the close emits and the menu
//! disappears next frame. Capturing here would silently break the switch
//! behavior.

use iced::{Event, advanced::Shell, keyboard, mouse, touch};

/// Handle the two shared dismissal gestures for an overlay menu.
///
/// - **Escape**: publishes `close()`, captures the event, and requests a
///   redraw — consumed so it can't leak into hosted children or ancestors.
/// - **Outside press** (`outside_press()` returns `true`): publishes
///   `close()` and requests a redraw, deliberately WITHOUT capturing (see
///   the module docs for why capturing would break trigger switching).
///
/// Returns `true` when the event was handled; callers `return` immediately.
pub(crate) fn handle_dismiss<Message>(
    event: &Event,
    shell: &mut Shell<'_, Message>,
    outside_press: impl FnOnce() -> bool,
    close: impl FnOnce() -> Message,
) -> bool {
    if matches!(
        event,
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        })
    ) {
        shell.publish(close());
        shell.capture_event();
        shell.request_redraw();
        true
    } else if outside_press() {
        shell.publish(close());
        shell.request_redraw();
        true
    } else {
        false
    }
}

/// `true` when the event begins a press — any mouse button or a touch
/// press. Used by the outside-press predicates of the three overlays that
/// honor touch (`checkbox_dropdown` historically matches mouse presses
/// only and keeps its own narrower check).
pub(crate) fn press_began(event: &Event) -> bool {
    matches!(
        event,
        Event::Mouse(mouse::Event::ButtonPressed(_))
            | Event::Touch(touch::Event::FingerPressed { .. })
    )
}

#[cfg(test)]
mod tests {
    use iced::{Point, keyboard::key};

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Close;

    fn escape_event() -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(key::Named::Escape),
            modified_key: keyboard::Key::Named(key::Named::Escape),
            physical_key: key::Physical::Code(key::Code::Escape),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::default(),
            text: None,
            repeat: false,
        })
    }

    fn left_press() -> Event {
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
    }

    /// Builds a throwaway `Shell` for exercising `handle_dismiss`. iced's
    /// `Shell::new` now takes a window handle + waker; a headless window and a
    /// no-op waker are inert here (the tests only assert on capture/messages).
    fn test_shell(messages: &mut Vec<Close>) -> Shell<'_, Close> {
        static WINDOW: iced::window::Headless = iced::window::Headless;
        Shell::new(
            &WINDOW,
            iced::advanced::graphics::core::shell::Waker::noop(),
            messages,
        )
    }

    #[test]
    fn press_began_matches_mouse_and_touch_presses() {
        assert!(press_began(&left_press()));
        assert!(press_began(&Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Right
        ))));
        assert!(press_began(&Event::Touch(touch::Event::FingerPressed {
            id: touch::Finger(0),
            position: Point::ORIGIN,
        })));
    }

    #[test]
    fn press_began_ignores_non_press_events() {
        assert!(!press_began(&Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Left
        ))));
        assert!(!press_began(&Event::Mouse(mouse::Event::CursorMoved {
            position: Point::ORIGIN,
        })));
        assert!(!press_began(&Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
        })));
    }

    /// Pins the load-bearing invariant: an outside press publishes exactly
    /// one close and does NOT capture, so the press can still reach another
    /// menu's trigger in the widget tree (click-to-switch UX).
    #[test]
    fn outside_press_closes_without_capturing() {
        let mut messages: Vec<Close> = Vec::new();
        let mut shell = test_shell(&mut messages);
        let event = left_press();

        let handled = handle_dismiss(&event, &mut shell, || true, || Close);

        assert!(handled);
        assert!(!shell.is_event_captured());
        assert_eq!(messages, vec![Close]);
    }

    #[test]
    fn inside_press_is_not_handled() {
        let mut messages: Vec<Close> = Vec::new();
        let mut shell = test_shell(&mut messages);
        let event = left_press();

        let handled = handle_dismiss(&event, &mut shell, || false, || Close);

        assert!(!handled);
        assert!(!shell.is_event_captured());
        assert!(messages.is_empty());
    }

    #[test]
    fn escape_closes_and_captures() {
        let mut messages: Vec<Close> = Vec::new();
        let mut shell = test_shell(&mut messages);
        let event = escape_event();

        let handled = handle_dismiss(&event, &mut shell, || false, || Close);

        assert!(handled);
        assert!(shell.is_event_captured());
        assert_eq!(messages, vec![Close]);
    }

    #[test]
    fn unrelated_event_with_false_predicate_is_ignored() {
        let mut messages: Vec<Close> = Vec::new();
        let mut shell = test_shell(&mut messages);
        let event = Event::Mouse(mouse::Event::CursorMoved {
            position: Point::ORIGIN,
        });

        let handled = handle_dismiss(&event, &mut shell, || false, || Close);

        assert!(!handled);
        assert!(!shell.is_event_captured());
        assert!(messages.is_empty());
    }
}
