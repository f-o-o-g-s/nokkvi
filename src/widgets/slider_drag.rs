//! Shared slider drag state machine + axis projection math
//!
//! Backs the three drag-to-set sliders (`settings_slider`, `volume_slider`,
//! `eq_slider`). Deliberately Shell-free: each widget keeps its own
//! `publish` / `capture_event` / `request_redraw` calls because their
//! release semantics differ — `settings_slider` captures + redraws on
//! release while `volume_slider` / `eq_slider` do not. Centralizing only
//! the math and the dragging/throttle bookkeeping preserves that asymmetry
//! by construction.
//!
//! Comparison-operator fidelity matters here: the move gate publishes on
//! `>= threshold` (so a `0.0` threshold publishes every move), while the
//! trailing release gate publishes on strictly `> trailing_threshold`.
//! Flipping either changes publish cadence (volume writes are throttled to
//! audio + disk; EQ publishes in 0.1 dB steps).
//!
//! Drawing-side geometry (handle position math) intentionally stays in each
//! widget's `draw()` — visuals are design intent.

use iced::{Rectangle, mouse};

/// Drag axis for [`project_fraction`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    Horizontal,
    Vertical,
}

/// Project a cursor coordinate onto a `[0.0, 1.0]` fraction along `axis`
/// (left / top = 0.0), insetting half of `handle` at each end so the handle
/// center tracks the cursor. Pass `handle = 0.0` for full-length mapping
/// (the vertical volume bar is a level meter with no handle). Returns
/// `None` when the track is degenerate (`length - handle <= 0.0`) — each
/// caller supplies its own fallback via `unwrap_or`.
pub(crate) fn project_fraction(
    coord: f32,
    bounds: Rectangle,
    handle: f32,
    axis: Axis,
) -> Option<f32> {
    let (origin, length) = match axis {
        Axis::Horizontal => (bounds.x, bounds.width),
        Axis::Vertical => (bounds.y, bounds.height),
    };
    let effective = length - handle;
    if effective <= 0.0 {
        return None;
    }
    let relative = coord - origin - handle / 2.0;
    Some((relative / effective).clamp(0.0, 1.0))
}

/// Shared press → drag → release bookkeeping for the slider widgets. Lives
/// inside each widget's own `State` type (the per-widget `State` types stay
/// distinct so `widget::tree::Tag` identity keeps drag state from being
/// adopted across different widget kinds in a diffed tree).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SliderDragState {
    is_dragging: bool,
    /// Visual drag position — what [`Self::display_value`] returns while
    /// dragging.
    drag_value: f32,
    /// Last value actually published through `on_change`.
    last_published: f32,
}

impl SliderDragState {
    /// Begin a drag at `value`. Returns the value to publish immediately
    /// (a press always publishes for real-time feedback).
    pub(crate) fn press(&mut self, value: f32) -> f32 {
        self.is_dragging = true;
        self.drag_value = value;
        self.last_published = value;
        value
    }

    /// Record a cursor move to `value`. Always updates the visual drag
    /// position; returns `Some(value)` to publish when the change since the
    /// last published value is `>= threshold` — deliberately `>=`, so a
    /// `0.0` threshold publishes every move. Withheld moves accumulate
    /// against the last *published* value, not the last drag position.
    pub(crate) fn drag(&mut self, value: f32, threshold: f32) -> Option<f32> {
        self.drag_value = value;
        if (value - self.last_published).abs() >= threshold {
            self.last_published = value;
            Some(value)
        } else {
            None
        }
    }

    /// End the drag. Returns `Some(final value)` to publish when the visual
    /// position drifted strictly `> trailing_threshold` past the last
    /// published value (matches the sliders' historical trailing gates).
    pub(crate) fn release(&mut self, trailing_threshold: f32) -> Option<f32> {
        self.is_dragging = false;
        if (self.drag_value - self.last_published).abs() > trailing_threshold {
            self.last_published = self.drag_value;
            Some(self.drag_value)
        } else {
            None
        }
    }

    pub(crate) fn is_dragging(&self) -> bool {
        self.is_dragging
    }

    /// Value to draw: the in-flight drag position while dragging, otherwise
    /// the widget's authoritative `fallback`.
    pub(crate) fn display_value(&self, fallback: f32) -> f32 {
        if self.is_dragging {
            self.drag_value
        } else {
            fallback
        }
    }

    /// The latest drag value regardless of dragging state — used by
    /// `volume_slider`'s dedicated `on_release` publish, which fires after
    /// [`Self::release`] has already cleared the dragging flag.
    pub(crate) fn current(&self) -> f32 {
        self.drag_value
    }
}

/// Grab cursor for the slider family: `Grabbing` while dragging, `Grab`
/// when hovering, default otherwise.
pub(crate) fn grab_interaction(is_dragging: bool, cursor_over: bool) -> mouse::Interaction {
    if is_dragging {
        mouse::Interaction::Grabbing
    } else if cursor_over {
        mouse::Interaction::Grab
    } else {
        mouse::Interaction::default()
    }
}

#[cfg(test)]
mod tests {
    use iced::{Point, Size};

    use super::*;

    /// 114 px track with a 14 px handle → 100 px effective length, so the
    /// fractions below are exactly representable.
    fn track() -> Rectangle {
        Rectangle::new(Point::new(0.0, 0.0), Size::new(114.0, 114.0))
    }

    #[test]
    fn project_fraction_horizontal_midpoint_with_handle_inset() {
        // coord 57 → relative 57 - 7 = 50 → 50 / 100 = 0.5
        assert_eq!(
            project_fraction(57.0, track(), 14.0, Axis::Horizontal),
            Some(0.5)
        );
    }

    #[test]
    fn project_fraction_clamps_at_both_endpoints() {
        assert_eq!(
            project_fraction(0.0, track(), 14.0, Axis::Horizontal),
            Some(0.0)
        );
        assert_eq!(
            project_fraction(114.0, track(), 14.0, Axis::Horizontal),
            Some(1.0)
        );
    }

    #[test]
    fn project_fraction_vertical_top_is_zero() {
        assert_eq!(
            project_fraction(7.0, track(), 14.0, Axis::Vertical),
            Some(0.0)
        );
        // coord 57 → relative 50 → 0.5 down the track.
        assert_eq!(
            project_fraction(57.0, track(), 14.0, Axis::Vertical),
            Some(0.5)
        );
    }

    #[test]
    fn project_fraction_degenerate_track_returns_none() {
        let tiny = Rectangle::new(Point::new(0.0, 0.0), Size::new(14.0, 14.0));
        assert_eq!(project_fraction(7.0, tiny, 14.0, Axis::Horizontal), None);
        let zero = Rectangle::new(Point::new(0.0, 0.0), Size::new(0.0, 0.0));
        assert_eq!(project_fraction(0.0, zero, 0.0, Axis::Vertical), None);
    }

    #[test]
    fn project_fraction_zero_handle_maps_full_length() {
        // The vertical volume bar case: no inset, full-height mapping.
        let bar = Rectangle::new(Point::new(0.0, 0.0), Size::new(8.0, 44.0));
        assert_eq!(project_fraction(0.0, bar, 0.0, Axis::Vertical), Some(0.0));
        assert_eq!(project_fraction(11.0, bar, 0.0, Axis::Vertical), Some(0.25));
        assert_eq!(project_fraction(44.0, bar, 0.0, Axis::Vertical), Some(1.0));
    }

    #[test]
    fn press_returns_value_and_starts_dragging() {
        let mut state = SliderDragState::default();
        assert_eq!(state.press(0.75), 0.75);
        assert!(state.is_dragging());
        assert_eq!(state.current(), 0.75);
    }

    #[test]
    fn drag_with_zero_threshold_publishes_every_move() {
        // settings_slider semantics: 0.0 threshold → publish every move,
        // including an unchanged value (|0 - 0| >= 0).
        let mut state = SliderDragState::default();
        state.press(0.5);
        assert_eq!(state.drag(0.5, 0.0), Some(0.5));
        assert_eq!(state.drag(0.625, 0.0), Some(0.625));
    }

    #[test]
    fn drag_publishes_at_exact_threshold_and_withholds_below() {
        let mut state = SliderDragState::default();
        state.press(0.0);
        // Just below the threshold → withheld.
        assert_eq!(state.drag(0.125, 0.25), None);
        // Exactly at the threshold (>= semantics) → published.
        state.press(0.0);
        assert_eq!(state.drag(0.25, 0.25), Some(0.25));
    }

    #[test]
    fn withheld_drags_accumulate_against_last_published() {
        let mut state = SliderDragState::default();
        state.press(0.0);
        assert_eq!(state.drag(0.125, 0.25), None);
        // Delta vs the last *published* value (0.0) is now 0.25 even though
        // the step from the previous drag position was only 0.125.
        assert_eq!(state.drag(0.25, 0.25), Some(0.25));
    }

    #[test]
    fn release_at_exact_trailing_threshold_returns_none() {
        // Strict > semantics: drift exactly equal to the gate is withheld.
        let mut state = SliderDragState::default();
        state.press(0.0);
        assert_eq!(state.drag(0.125, 0.25), None);
        assert_eq!(state.release(0.125), None);
        assert!(!state.is_dragging());
    }

    #[test]
    fn release_publishes_trailing_drift_and_updates_last_published() {
        let mut state = SliderDragState::default();
        state.press(0.0);
        assert_eq!(state.drag(0.5, 1.0), None);
        assert_eq!(state.release(0.25), Some(0.5));
        // last_published advanced to the trailing value: a fresh drag to the
        // same value at threshold 0.25 is now withheld.
        state.press(0.5);
        assert_eq!(state.drag(0.625, 0.25), None);
    }

    #[test]
    fn release_clears_dragging_but_current_keeps_final_value() {
        // volume_slider's on_release publish reads current() after release().
        let mut state = SliderDragState::default();
        state.press(0.25);
        assert_eq!(state.drag(0.75, 0.02), Some(0.75));
        assert_eq!(state.release(0.001), None);
        assert!(!state.is_dragging());
        assert_eq!(state.current(), 0.75);
    }

    #[test]
    fn display_value_uses_drag_value_only_while_dragging() {
        let mut state = SliderDragState::default();
        assert_eq!(state.display_value(0.25), 0.25);
        state.press(0.75);
        assert_eq!(state.display_value(0.25), 0.75);
        let _ = state.release(0.0);
        assert_eq!(state.display_value(0.25), 0.25);
    }

    #[test]
    fn grab_interaction_priority() {
        assert_eq!(grab_interaction(true, false), mouse::Interaction::Grabbing);
        assert_eq!(grab_interaction(true, true), mouse::Interaction::Grabbing);
        assert_eq!(grab_interaction(false, true), mouse::Interaction::Grab);
        assert_eq!(
            grab_interaction(false, false),
            mouse::Interaction::default()
        );
    }
}
