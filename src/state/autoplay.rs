//! Autoplay's UI bookkeeping: the fill in flight, the retry gate, the
//! "nothing found" toast latch, a resume waiting for a batch after the queue
//! ran out, and the last ran-out count the tick carried. The backend
//! (`AppService::autoplay_top_up` and friends) decides what to add; the 100 ms
//! playback tick decides when to ask (`update/playback.rs`).

use std::time::{Duration, Instant};

use nokkvi_data::types::autoplay::{
    AUTOPLAY_MAX_RETRIES, AUTOPLAY_RETRY_SECS, AutoplayOutcome, autoplay_wants_fill,
};

/// How long the tick waits before it asks again after the backend answered
/// "not needed": the UI's view of the queue and the backend's can disagree
/// for a moment (a reload in flight), and without a wait every tick would
/// start a task.
pub(crate) const AUTOPLAY_NOT_NEEDED_WAIT: Duration = Duration::from_secs(2);

/// How long the tick waits after a fill found nothing or failed.
pub(crate) const AUTOPLAY_RETRY_WAIT: Duration = Duration::from_secs(AUTOPLAY_RETRY_SECS);

/// What one fill (`AppService::autoplay_top_up`) came back with, as the UI
/// carries it in a message (`anyhow::Error` is not `Clone`).
#[derive(Debug, Clone)]
pub enum AutoplayFill {
    /// The backend answered.
    Outcome(AutoplayOutcome),
    /// The server turned the session away: drop to login.
    SessionRejected,
    /// Any other failure (the transport, the queue save), as text for the log.
    Failed(String),
}

impl AutoplayFill {
    /// Sort a backend answer into the three cases the UI tells apart.
    pub(crate) fn from_result(result: anyhow::Result<AutoplayOutcome>) -> Self {
        match result {
            Ok(outcome) => Self::Outcome(outcome),
            // The test `session_expired_message` makes (a 401 anywhere in the
            // chain).
            Err(e) if nokkvi_data::types::error::NokkviError::is_unauthorized(&e) => {
                Self::SessionRejected
            }
            Err(e) => Self::Failed(format!("{e:#}")),
        }
    }
}

/// What the tick knows about the app when it decides whether to ask for a
/// batch (the inputs of [`AutoplayState::ask`] that are not Autoplay's own).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AutoplayGate {
    /// The Autoplay setting.
    pub enabled: bool,
    /// Queue playback (not a radio station).
    pub queue_mode: bool,
    /// Repeat (track and queue) is off.
    pub repeat_off: bool,
    /// Songs after the current one in play order; `None` with no current
    /// row (`QueueManager::runway`).
    pub runway: Option<usize>,
    /// Playing or paused: a stopped, restored queue asks nothing.
    pub transport_active: bool,
    /// A progressive Songs load is still appending pages.
    pub progressive_load: bool,
}

/// Which kind of fill to ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoplayAsk {
    /// The queue runs low under a current row.
    Fill,
    /// The queue ran out and a resume waits for a batch
    /// (`autoplay_top_up(true)`).
    AfterRunOut,
}

impl AutoplayAsk {
    /// The `after_run_out` argument of `AppService::autoplay_top_up`.
    pub(crate) const fn after_run_out(self) -> bool {
        matches!(self, Self::AfterRunOut)
    }
}

/// See the module docs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutoplayState {
    /// A fill is on its way; no other is asked meanwhile.
    pub in_flight: bool,
    /// Fills for the current song that found nothing or failed. Asking stops
    /// once it passes [`AUTOPLAY_MAX_RETRIES`] (the first ask plus that many
    /// asks again), until the song changes.
    pub failures: u8,
    /// No fill is asked before this instant.
    pub wait_until: Option<Instant>,
    /// "Autoplay found nothing new to add" was shown for the current song.
    pub nothing_toasted: bool,
    /// The queue ran out while Autoplay was on: the first song of the next
    /// batch plays when it lands.
    pub resume_pending: bool,
    /// The last `queue_ran_out_epoch` a tick carried. `None` until the first
    /// tick of a session, which only adopts its value: the backend's count
    /// starts at 0 with every login, so a change can only be judged against
    /// a value seen in the same session.
    pub seen_ran_out_epoch: Option<u64>,
}

impl AutoplayState {
    /// Whether the retry gate lets a fill be asked at `now`.
    pub(crate) fn retry_gate_open(&self, now: Instant) -> bool {
        self.failures <= AUTOPLAY_MAX_RETRIES && self.wait_until.is_none_or(|until| now >= until)
    }

    /// The fill predicate: which fill to ask for at `now`, if any. Pure, so
    /// the tick and its tests share it.
    ///
    /// A fill is asked only while Autoplay is on, playback is the queue's,
    /// Repeat is off, no fill is in flight, no progressive Songs load is
    /// still appending and the retry gate is open. Then: with a resume
    /// pending and no current row, a fill after the run-out; otherwise,
    /// while playing or paused, a fill when the runway is short
    /// (`autoplay_wants_fill`).
    pub(crate) fn ask(&self, gate: &AutoplayGate, now: Instant) -> Option<AutoplayAsk> {
        if !gate.enabled
            || !gate.queue_mode
            || !gate.repeat_off
            || self.in_flight
            || gate.progressive_load
            || !self.retry_gate_open(now)
        {
            return None;
        }
        if self.resume_pending && gate.runway.is_none() {
            return Some(AutoplayAsk::AfterRunOut);
        }
        (gate.transport_active && autoplay_wants_fill(true, gate.repeat_off, gate.runway))
            .then_some(AutoplayAsk::Fill)
    }

    /// A fill found nothing or failed: count it and wait
    /// [`AUTOPLAY_RETRY_WAIT`] before the next ask.
    pub(crate) fn count_failure(&mut self, now: Instant) {
        self.failures = self.failures.saturating_add(1);
        self.wait_until = Some(now + AUTOPLAY_RETRY_WAIT);
    }

    /// The backend said no batch was needed: wait briefly, count nothing.
    pub(crate) fn wait_briefly(&mut self, now: Instant) {
        self.wait_until = Some(now + AUTOPLAY_NOT_NEEDED_WAIT);
    }

    /// A batch landed: the retry state starts over.
    pub(crate) fn clear_retries(&mut self) {
        self.failures = 0;
        self.wait_until = None;
    }

    /// The current song changed: the retry count, the wait and the toast
    /// latch start over.
    pub(crate) fn song_changed(&mut self) {
        self.clear_retries();
        self.nothing_toasted = false;
    }

    /// Note the tick's ran-out count. True when the queue ran out since the
    /// last tick (a change from a value seen before); the first value of a
    /// session is only adopted.
    pub(crate) fn observe_ran_out_epoch(&mut self, epoch: u64) -> bool {
        let moved = self.seen_ran_out_epoch.is_some_and(|seen| seen != epoch);
        self.seen_ran_out_epoch = Some(epoch);
        moved
    }

    /// Autoplay went off: everything starts over except the fill still in
    /// flight (its answer clears it) and the ran-out count (still the
    /// session's).
    pub(crate) fn reset_for_mode_off(&mut self) {
        *self = Self {
            in_flight: self.in_flight,
            seen_ran_out_epoch: self.seen_ran_out_epoch,
            ..Self::default()
        };
    }
}
