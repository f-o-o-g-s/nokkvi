//! When the app counts as idle, so the display-rate frame loop
//! (`Nokkvi::frames_wanted`) stands down. Nothing here animates on its own:
//! the frame step and the 10 Hz tick stamp `transport_seen` while the
//! transport runs, and the ambient-input listener stamps `last_input`.

use std::time::{Duration, Instant};

/// How long a visualizer keeps its frames after the transport stops: long
/// enough for the bars and their peaks to fall and a Cover Beat swell to
/// settle, after which the picture holds still until playback resumes.
pub(crate) const VISUALIZER_SETTLE: Duration = Duration::from_secs(4);

/// How long the Harbour scene keeps breathing with nothing playing after the
/// last mouse move, wheel, click or key press in the focused window.
pub(crate) const AMBIENT_IDLE_AFTER: Duration = Duration::from_secs(60);

/// Input older than this re-arms the ambient-input listener. While input is
/// fresher the listener stays off, so a mouse move costs no message while the
/// scene already runs; the scene therefore holds between half and the full
/// [`AMBIENT_IDLE_AFTER`] after the last input.
pub(crate) const AMBIENT_INPUT_REARM: Duration = Duration::from_secs(30);

/// The 10 Hz tick's period while anything moves or plays.
pub(crate) const TICK_LIVELY: Duration = Duration::from_millis(100);

/// The tick's period while the app idles: nothing plays, nothing animates,
/// no toast shows. Every poll the tick runs (the engine snapshot, toast
/// expiry, the loading watchdog) still runs, only less often.
pub(crate) const TICK_IDLE: Duration = Duration::from_millis(500);

/// How long the tick stays at [`TICK_LIVELY`] after the transport stops or
/// after any message that is not the idle poll itself: past every
/// tick-counted hold (the cover accent's `ACCENT_HOLD_TICKS`, 1.5 s), and
/// long enough that a play started from anywhere (a click, a hotkey, MPRIS,
/// IPC) is picked up by a fast tick.
pub(crate) const TICK_LIVELY_HOLD: Duration = Duration::from_secs(5);

/// Root-owned idle bookkeeping (`Nokkvi.idle`).
#[derive(Debug, Clone, Copy)]
pub struct IdleState {
    /// Last frame or tick that saw the transport running.
    pub transport_seen: Option<Instant>,
    /// Last message that was not the idle poll itself
    /// (`Message::is_idle_poll`).
    pub last_event: Option<Instant>,
    /// Last user input the ambient-input listener heard. Starts at launch so
    /// the scene breathes before the first input.
    pub last_input: Option<Instant>,
}

impl Default for IdleState {
    fn default() -> Self {
        Self {
            transport_seen: None,
            last_event: None,
            last_input: Some(Instant::now()),
        }
    }
}

impl IdleState {
    /// The transport ran within `window` of `now`.
    pub(crate) fn transport_within(&self, now: Instant, window: Duration) -> bool {
        self.transport_seen
            .is_some_and(|at| now.saturating_duration_since(at) < window)
    }

    /// A message other than the idle poll within `window` of `now`.
    pub(crate) fn event_within(&self, now: Instant, window: Duration) -> bool {
        self.last_event
            .is_some_and(|at| now.saturating_duration_since(at) < window)
    }

    /// User input within `window` of `now`.
    pub(crate) fn input_within(&self, now: Instant, window: Duration) -> bool {
        self.last_input
            .is_some_and(|at| now.saturating_duration_since(at) < window)
    }
}
