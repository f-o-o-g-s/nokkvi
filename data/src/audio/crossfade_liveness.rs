//! Network-read liveness for the crossfade incoming decode loop (M9 Part B).
//!
//! The bounded stall watchdog's discriminator: the renderer's completion gate
//! must distinguish an incoming producer that is **blocked on the socket**
//! (a network hiccup pushed a few KB then wedged — promoting that stream
//! yields stuttering/silence with no way to finish) from one that is
//! **sleeping on backpressure** (ring full — perfectly healthy) or already
//! done (EOF). A raw `crossfade_buffer_count` cannot make that call — the
//! count grows and shrinks for healthy reasons — so the decode loop reports
//! its own read state through this handle instead.
//!
//! One instance is created per crossfade decode loop
//! (`CustomAudioEngine::start_crossfade_decode_loop`) and installed on the
//! renderer for the lifetime of that fade; the per-fade instance means a
//! superseded loop's late writes can never pollute a newer fade's verdict.
//! The loop brackets ONLY the blocking decode/network call
//! (`decode_one_chunk`) with [`IncomingLiveness::mark_read_start`] /
//! [`IncomingLiveness::mark_read_end`]; the backpressure sleep, the EOF
//! exit, and lock acquisitions are deliberately outside the bracket, so they
//! always read as live.

use std::{
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Instant,
};

/// How long a SINGLE decode/network read must stay continuously in flight
/// before the incoming producer counts as stalled. Healthy reads return in
/// milliseconds (packet decode) to a few hundred ms (a 256KB range-chunk
/// fetch); a read that cannot complete in 3s implies an effective throughput
/// below real-time FLAC — a link that already cannot sustain playback.
/// Deliberately conservative: a false "stalled" verdict cancels a blend that
/// was audibly playing and reloads the incoming from the top, while a false
/// "live" verdict merely falls back to the pre-M9 behavior (promote, then the
/// issue-9 rebuffer machinery copes). Fades shorter than this threshold fall
/// back to the empty-ring completion gate alone (a read cannot have been
/// blocked longer than the fade has run).
pub(crate) const CROSSFADE_STALL_READ_MS: u64 = 3_000;

/// Shared read-liveness state between one crossfade decode loop (writer) and
/// the renderer's crossfade completion gate (reader). Lock-free: the reader
/// runs on the 20ms render tick.
pub(crate) struct IncomingLiveness {
    /// Millisecond zero-point for `read_started_ms` (the instance's creation
    /// time — strictly before any read it can describe).
    epoch: Instant,
    /// True while the decode loop is inside its blocking decode/network call.
    in_network_read: AtomicBool,
    /// When the in-flight read began, in ms since `epoch`. Only meaningful
    /// while `in_network_read` is true.
    read_started_ms: AtomicU64,
}

impl IncomingLiveness {
    pub(crate) fn new() -> Self {
        Self {
            epoch: Instant::now(),
            in_network_read: AtomicBool::new(false),
            read_started_ms: AtomicU64::new(0),
        }
    }

    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    /// Called by the decode loop immediately before its blocking
    /// decode/network call. Timestamp is stored before the flag flips so a
    /// reader that observes `true` never pairs it with a stale start time.
    pub(crate) fn mark_read_start(&self) {
        self.read_started_ms.store(self.now_ms(), Ordering::Relaxed);
        self.in_network_read.store(true, Ordering::Release);
    }

    /// Called by the decode loop immediately after the blocking call returns
    /// (success, error, or EOF alike — a returned call is a live socket).
    pub(crate) fn mark_read_end(&self) {
        self.in_network_read.store(false, Ordering::Release);
    }

    /// How long the current read has been continuously in flight, in ms.
    /// 0 when the loop is not inside a read (sleeping on backpressure,
    /// decoding between reads, exited at EOF, or not yet started).
    pub(crate) fn blocked_read_ms(&self) -> u64 {
        if !self.in_network_read.load(Ordering::Acquire) {
            return 0;
        }
        // saturating: the writer may store a fresher start between our two
        // loads — reading that race as 0 is the fail-safe (live) direction.
        self.now_ms()
            .saturating_sub(self.read_started_ms.load(Ordering::Relaxed))
    }

    /// The Part B verdict: the producer counts as stalled only when one
    /// blocking read has been in flight for at least
    /// [`CROSSFADE_STALL_READ_MS`].
    pub(crate) fn is_stalled(&self) -> bool {
        self.blocked_read_ms() >= CROSSFADE_STALL_READ_MS
    }

    /// Test-only: an instance whose in-flight read started `blocked_ms` ago,
    /// for injecting a stalled/near-stalled producer into `tick_crossfade`.
    #[cfg(test)]
    pub(crate) fn new_blocked_for_test(blocked_ms: u64) -> Self {
        let liveness = Self {
            epoch: Instant::now()
                .checked_sub(std::time::Duration::from_millis(blocked_ms))
                .expect("test epoch back-date within process uptime"),
            in_network_read: AtomicBool::new(false),
            read_started_ms: AtomicU64::new(0),
        };
        // read_started_ms = 0 == `blocked_ms` before now.
        liveness.in_network_read.store(true, Ordering::Release);
        liveness
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh instance (loop not yet inside any read) is live: the watchdog
    /// must never fire before the producer has even had a chance to run.
    #[test]
    fn fresh_liveness_is_live() {
        let liveness = IncomingLiveness::new();
        assert_eq!(liveness.blocked_read_ms(), 0);
        assert!(!liveness.is_stalled());
    }

    /// A read in flight for less than the threshold is live — healthy chunk
    /// fetches routinely sit in flight for hundreds of ms and must never
    /// trip the watchdog.
    #[test]
    fn in_flight_read_below_threshold_is_live() {
        let liveness = IncomingLiveness::new_blocked_for_test(CROSSFADE_STALL_READ_MS / 2);
        assert!(
            !liveness.is_stalled(),
            "a read blocked for {}ms (below the {}ms threshold) must be live",
            CROSSFADE_STALL_READ_MS / 2,
            CROSSFADE_STALL_READ_MS
        );
    }

    /// A single read continuously in flight past the threshold is stalled.
    #[test]
    fn read_blocked_past_threshold_is_stalled() {
        let liveness = IncomingLiveness::new_blocked_for_test(CROSSFADE_STALL_READ_MS + 500);
        assert!(
            liveness.blocked_read_ms() >= CROSSFADE_STALL_READ_MS,
            "blocked duration must reflect the in-flight read"
        );
        assert!(liveness.is_stalled());
    }

    /// A returned read clears the stall — sleeping on backpressure (or any
    /// state between reads) is healthy no matter how long it lasts.
    #[test]
    fn mark_read_end_clears_stall() {
        let liveness = IncomingLiveness::new_blocked_for_test(CROSSFADE_STALL_READ_MS + 500);
        liveness.mark_read_end();
        assert_eq!(liveness.blocked_read_ms(), 0);
        assert!(!liveness.is_stalled());
    }

    /// `mark_read_start` begins a fresh measurement window: an old completed
    /// read's timestamp never bleeds into the next read's blocked duration.
    #[test]
    fn mark_read_start_begins_fresh_window() {
        let liveness = IncomingLiveness::new_blocked_for_test(CROSSFADE_STALL_READ_MS + 500);
        liveness.mark_read_end();
        liveness.mark_read_start();
        assert!(
            liveness.blocked_read_ms() < CROSSFADE_STALL_READ_MS,
            "a just-started read must not inherit the previous read's age"
        );
        assert!(!liveness.is_stalled());
    }
}
