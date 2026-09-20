//! Seek arbitration: one seek in flight, everything else merged behind it.
//!
//! A seek is expensive — the controller holds the engine lock across a 20 ms
//! settle, a blocking decoder seek and a prebuffer, and the 10 Hz tick needs
//! that same lock. A held Seek Forward key repeats ~25 times a second, so
//! letting every press dispatch its own task would pile up decoder seeks
//! against a clock that cannot move between them.
//!
//! [`SeekState`] keeps exactly one request outstanding and folds the rest into
//! a single `queued` request, which is dispatched when the outstanding one
//! lands. Relative requests add up, so nothing a user pressed is lost.

/// One seek request, in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SeekRequest {
    /// Seek to this position, measured from the start of the track.
    Absolute(f32),
    /// Move this far from wherever the engine currently is. Negative rewinds.
    Relative(f32),
}

impl SeekRequest {
    /// Whether dispatching this request would move nothing. A zero relative
    /// offset still costs a full decoder seek and stream rebuild in the
    /// engine, so it is dropped rather than sent.
    fn is_noop(self) -> bool {
        matches!(self, SeekRequest::Relative(delta) if delta == 0.0)
    }
}

/// Seek arbitration state, plus the live Seek Step setting the hotkeys read.
#[derive(Debug)]
pub struct SeekState {
    /// A seek has been dispatched and no `SeekApplied` has come back yet.
    in_flight: bool,
    /// Everything requested while `in_flight`, merged into one request.
    queued: Option<SeekRequest>,
    /// Bumped whenever a seek is dispatched and again when one lands. A
    /// `PlaybackStateUpdate` carries the epoch its tick read the engine under,
    /// so an update built before (or during) a seek is recognisable as stale
    /// and dropped instead of dragging the clock back.
    pub epoch: u64,
    /// Seconds the Seek Backward / Seek Forward keys jump. Mirrored from
    /// `general.seek_step`; 5 until the settings load.
    pub step_secs: u32,
}

/// The Seek Step setting's default, used until persisted settings land.
pub(crate) const DEFAULT_SEEK_STEP_SECS: u32 = 5;

impl Default for SeekState {
    fn default() -> Self {
        Self {
            in_flight: false,
            queued: None,
            epoch: 0,
            step_secs: DEFAULT_SEEK_STEP_SECS,
        }
    }
}

impl SeekState {
    /// Whether a dispatched seek is still outstanding.
    pub fn in_flight(&self) -> bool {
        self.in_flight
    }

    /// Whether a request is waiting behind the outstanding one.
    #[cfg(test)]
    pub fn queued(&self) -> Option<SeekRequest> {
        self.queued
    }

    /// Register a new request. Returns the request to dispatch NOW, or `None`
    /// when it was merged into `queued` (or would move nothing).
    ///
    /// Returning `Some` marks a seek outstanding, so the caller must actually
    /// dispatch it — otherwise `in_flight` sticks and every later seek queues
    /// behind a task that will never answer.
    pub fn request(&mut self, req: SeekRequest) -> Option<SeekRequest> {
        if self.in_flight {
            self.queued = merge(self.queued, req);
            return None;
        }
        if req.is_noop() {
            return None;
        }
        self.in_flight = true;
        Some(req)
    }

    /// Record that the outstanding seek landed. Returns the queued request,
    /// which the caller must dispatch (it is already marked outstanding).
    pub fn applied(&mut self) -> Option<SeekRequest> {
        self.in_flight = false;
        let next = self.queued.take()?;
        self.in_flight = true;
        Some(next)
    }

    /// Drop everything outstanding on logout / session expiry. The dispatched
    /// task is not cancelled, so the epoch bump is what keeps its late
    /// `SeekApplied` from writing a dead session's position. `step_secs` is a
    /// user preference and survives.
    pub fn reset_for_session(&mut self) {
        self.in_flight = false;
        self.queued = None;
        self.epoch = self.epoch.wrapping_add(1);
    }
}

/// Fold `incoming` into whatever is already queued.
///
/// - An **absolute** request replaces whatever was queued: it names a
///   position, so anything queued ahead of it is moot.
/// - A **relative** onto a relative adds, which is what makes a held key
///   accumulate instead of landing as one jump.
/// - A **relative** onto a queued absolute shifts that target, floored at 0.
///
/// A merge that cancels out (relative sum of zero) clears the queue rather
/// than dispatching a seek that moves nothing.
fn merge(queued: Option<SeekRequest>, incoming: SeekRequest) -> Option<SeekRequest> {
    let merged = match (queued, incoming) {
        (_, SeekRequest::Absolute(pos)) => SeekRequest::Absolute(pos),
        (None, req) => req,
        (Some(SeekRequest::Relative(queued)), SeekRequest::Relative(delta)) => {
            SeekRequest::Relative(queued + delta)
        }
        (Some(SeekRequest::Absolute(pos)), SeekRequest::Relative(delta)) => {
            SeekRequest::Absolute((pos + delta).max(0.0))
        }
    };
    (!merged.is_noop()).then_some(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Put a seek in flight so the next `request` has something to merge into.
    fn in_flight() -> SeekState {
        let mut state = SeekState::default();
        assert_eq!(
            state.request(SeekRequest::Relative(5.0)),
            Some(SeekRequest::Relative(5.0))
        );
        assert!(state.in_flight());
        state
    }

    #[test]
    fn first_request_dispatches_immediately() {
        let mut state = SeekState::default();
        assert_eq!(
            state.request(SeekRequest::Absolute(42.0)),
            Some(SeekRequest::Absolute(42.0))
        );
        assert!(state.in_flight());
        assert_eq!(state.queued(), None);
    }

    #[test]
    fn a_relative_onto_a_relative_adds() {
        let mut state = in_flight();
        assert_eq!(state.request(SeekRequest::Relative(5.0)), None);
        assert_eq!(state.request(SeekRequest::Relative(5.0)), None);
        assert_eq!(state.queued(), Some(SeekRequest::Relative(10.0)));
    }

    #[test]
    fn an_absolute_replaces_whatever_is_queued() {
        let mut state = in_flight();
        assert_eq!(state.request(SeekRequest::Relative(5.0)), None);
        assert_eq!(state.request(SeekRequest::Absolute(90.0)), None);
        assert_eq!(state.queued(), Some(SeekRequest::Absolute(90.0)));
    }

    #[test]
    fn a_relative_onto_a_queued_absolute_shifts_it() {
        let mut state = in_flight();
        assert_eq!(state.request(SeekRequest::Absolute(90.0)), None);
        assert_eq!(state.request(SeekRequest::Relative(-5.0)), None);
        assert_eq!(state.queued(), Some(SeekRequest::Absolute(85.0)));
    }

    #[test]
    fn a_relative_can_only_shift_a_queued_absolute_down_to_zero() {
        let mut state = in_flight();
        assert_eq!(state.request(SeekRequest::Absolute(2.0)), None);
        assert_eq!(state.request(SeekRequest::Relative(-30.0)), None);
        assert_eq!(state.queued(), Some(SeekRequest::Absolute(0.0)));
    }

    #[test]
    fn a_relative_sum_of_zero_still_clears() {
        let mut state = in_flight();
        assert_eq!(state.request(SeekRequest::Relative(5.0)), None);
        assert_eq!(state.request(SeekRequest::Relative(-5.0)), None);
        assert_eq!(state.queued(), None, "the queued pair cancelled out");
        assert_eq!(state.applied(), None);
        assert!(!state.in_flight(), "nothing may stay outstanding");
    }

    #[test]
    fn applied_hands_back_the_queued_request_still_outstanding() {
        let mut state = in_flight();
        assert_eq!(state.request(SeekRequest::Relative(7.0)), None);
        assert_eq!(state.applied(), Some(SeekRequest::Relative(7.0)));
        assert!(
            state.in_flight(),
            "the handed-back request is dispatched by the caller, so it counts as outstanding"
        );
        assert_eq!(state.queued(), None);
    }

    #[test]
    fn applied_with_nothing_queued_clears_in_flight() {
        let mut state = in_flight();
        assert_eq!(state.applied(), None);
        assert!(!state.in_flight());
    }

    #[test]
    fn a_zero_relative_request_dispatches_nothing() {
        let mut state = SeekState::default();
        assert_eq!(state.request(SeekRequest::Relative(0.0)), None);
        assert!(
            !state.in_flight(),
            "a request that moves nothing must not mark a seek outstanding"
        );
    }

    #[test]
    fn reset_for_session_clears_the_cluster_and_bumps_the_epoch() {
        let mut state = in_flight();
        assert_eq!(state.request(SeekRequest::Relative(5.0)), None);
        let epoch = state.epoch;
        state.step_secs = 12;
        state.reset_for_session();
        assert!(!state.in_flight());
        assert_eq!(state.queued(), None);
        assert_eq!(state.epoch, epoch.wrapping_add(1));
        assert_eq!(state.step_secs, 12, "the Seek Step setting is a preference");
    }
}
