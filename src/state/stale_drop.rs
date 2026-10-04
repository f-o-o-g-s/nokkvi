//! Stale-drop generation counters.

/// A stale-drop generation. Async work captures [`current`](Self::current)
/// when it is dispatched, and its result is applied only while the counter
/// still [`accepts`](Self::accepts) the captured value; every new dispatch
/// [`bump`](Self::bump)s, superseding whatever is still in flight.
///
/// The counter only ever counts up, across logout too. iced Tasks are not
/// cancelled by a session reset, so work dispatched before logout can land
/// after the next login; a counter restarted at 0 would re-mint the value
/// that work captured and accept its stale result. A session reset keeps
/// the counter [`carried_forward`](Self::carried_forward). There is no
/// setter, but `Default` still builds a zeroed one: a group holding a
/// counter must reset through its `reset_for_session()`, never by being
/// replaced with `Default::default()`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StaleDropGen(u64);

impl StaleDropGen {
    /// The value to hand work dispatched now.
    pub fn current(self) -> u64 {
        self.0
    }

    /// Supersede everything in flight. Returns the value to hand the work
    /// that replaces it.
    pub fn bump(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(1);
        self.0
    }

    /// Whether work dispatched under `captured` is still the latest.
    pub fn accepts(self, captured: u64) -> bool {
        self.0 == captured
    }

    /// The counter a session reset keeps: bumped, so work dispatched before
    /// the reset is still dropped after it.
    #[must_use]
    pub fn carried_forward(self) -> Self {
        Self(self.0.wrapping_add(1))
    }

    /// A counter standing at `value`, for tests that seed one.
    #[cfg(test)]
    pub fn at(value: u64) -> Self {
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_supersedes_the_captured_value() {
        let mut generation = StaleDropGen::default();
        let first = generation.bump();
        assert!(generation.accepts(first));
        let second = generation.bump();
        assert!(
            !generation.accepts(first),
            "a newer dispatch drops the older"
        );
        assert!(generation.accepts(second));
    }

    #[test]
    fn carried_forward_drops_work_captured_before_the_reset() {
        let mut generation = StaleDropGen::default();
        let before_logout = generation.bump();
        let after_reset = generation.carried_forward();
        assert!(!after_reset.accepts(before_logout));
        assert_eq!(after_reset.current(), before_logout.wrapping_add(1));
    }

    #[test]
    fn carried_forward_never_re_mints_a_value_the_next_session_reaches() {
        // The bug a zeroing reset had: work captured at 1 before logout
        // matched the next session's first bump.
        let mut generation = StaleDropGen::default();
        let before_logout = generation.bump();
        let mut next_session = generation.carried_forward();
        for _ in 0..3 {
            assert!(!next_session.accepts(before_logout));
            next_session.bump();
        }
    }

    #[test]
    fn wraps_instead_of_overflowing() {
        let mut generation = StaleDropGen::at(u64::MAX);
        assert_eq!(generation.bump(), 0);
        assert_eq!(StaleDropGen::at(u64::MAX).carried_forward().current(), 0);
    }
}
