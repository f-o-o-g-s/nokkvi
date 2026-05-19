//! Roulette (slot-machine random pick) animation state.

/// One step in the decel + fake-out keyframe walk. The wheel snaps to
/// `offset` at the start of the keyframe and holds there for
/// `duration_ms` before advancing to the next entry. The terminal
/// (last) keyframe is always at `target_idx` and signals that the
/// spin has settled the moment it's entered.
#[derive(Debug, Clone, Copy)]
pub struct DecelKeyframe {
    /// Absolute viewport offset to display at this keyframe.
    pub offset: usize,
    /// How long to hold at `offset` before advancing to the next
    /// keyframe. The terminal keyframe's duration is unused.
    pub duration_ms: u64,
}

/// Decel-phase parameters, populated when the user presses Enter to stop
/// the spin. While `RouletteState.decel` is `None` the wheel is in the
/// indefinite cruise phase (constant velocity, waiting for the user).
/// Once armed, the decel walk is committed and the animation rides it to
/// settle on `target_idx`.
#[derive(Debug, Clone)]
pub struct DecelArmed {
    /// Walltime when Stop fired. The decel keyframe walk is anchored to
    /// this — `position_at(now)` walks from `(now - stop_time)` through
    /// `decel_keyframes` in order.
    pub stop_time: std::time::Instant,
    /// Pre-rolled landing index. The animation always settles here.
    pub target_idx: usize,
    /// Pre-rolled decel + fake-out walk. Each entry holds at its `offset`
    /// for `duration_ms`; the terminal entry sits at `target_idx` with
    /// `duration_ms = 0`. Holds escalate via a cubic curve over the
    /// natural-walk keyframes, with the last 0–2 entries carrying
    /// pattern-specific wobble timing.
    pub decel_keyframes: Vec<DecelKeyframe>,
}

/// Time-driven state for an in-progress "Roulette" pick.
///
/// Snapshotted at start so subsequent data churn (page loads, search
/// edits, queue mutations) cannot drift the animation off the chosen
/// target. Intermediate offsets are derived purely from `(start_time,
/// cruise_pos_per_sec, decel)`, so a tick handler is stateless beyond
/// bookkeeping.
///
/// Two phases:
/// - **Cruise** (`decel = None`): the wheel scrolls at a constant
///   velocity through the slot list, cycling indefinitely. SFX fires at
///   the throttled rate. Continues until the user presses Enter, which
///   dispatches `RouletteMessage::Stop` and arms `decel`.
/// - **Decel** (`decel = Some(arm)`): the wheel ticks through
///   `arm.decel_keyframes` one at a time from `arm.stop_time`, holding
///   at each offset for the keyframe's `duration_ms`. Holds escalate
///   via a cubic curve from ~50 ms (cruise-like rate) to ~1190 ms
///   (slot-machine final click), so the click cadence audibly slows
///   from ~20 Hz down to ~1 Hz over the decel phase. The last 0–2
///   keyframes carry the chosen FakeoutPattern wobble (overshoot,
///   false-settle, etc.).
///
/// `target_idx` and the keyframe walk are rolled when Stop fires, not at
/// Start — the cruise phase has no committed landing until the user
/// decides to halt it. This is what makes the feature feel like the user
/// is *controlling* the spin rather than watching a pre-baked animation.
#[derive(Debug, Clone)]
pub struct RouletteState {
    /// Slot-list view this roulette runs in.
    pub view: crate::View,
    /// Snapshotted item count — frozen so live mutations don't drift the math.
    pub total_items: usize,
    /// Viewport offset captured at start; restored on cancel.
    pub original_offset: usize,
    /// Indefinite cruise rate, positions per second. Constant for the
    /// lifetime of the spin. Picked at start to match the old cruise feel
    /// (≈ `revolutions × total_items / 1.5 s`), so visual velocity stays
    /// consistent with library size — small lists cycle slowly enough to
    /// read, large lists blur the way a real wheel would.
    pub cruise_pos_per_sec: u32,
    /// Decel phase: `None` while cruising, `Some(arm)` once Stop has
    /// fired. Once armed the spin is committed — further Stop messages
    /// are no-ops.
    pub decel: Option<DecelArmed>,
    /// Animation start timestamp.
    pub start_time: std::time::Instant,
    /// Last offset actually applied to the slot list. Tick handlers
    /// compare against the freshly-computed offset to decide whether
    /// to fire an SFX / record_scroll().
    pub last_offset: usize,
    /// Last time an SFX was fired. Throttled to one per ~30 ms so the
    /// fastest cruise frames sound like a rattle rather than a buzz.
    /// Decel keyframe holds are already ≥ 50 ms, so the throttle is
    /// inactive during decel — every click fires its SFX.
    pub last_sfx_at: Option<std::time::Instant>,
    /// Last time a viewport-artwork prefetch was dispatched. Throttled
    /// so the spin doesn't hammer the artwork API at 60 Hz — without
    /// this, the roulette would queue duplicate fetches faster than
    /// they can return and the LRU snapshot lags behind the spinning
    /// viewport, leaving thumbnails as gray boxes until settle.
    pub last_prefetch_at: Option<std::time::Instant>,
}

impl RouletteState {
    /// Minimum spacing between SFX plays during the spin.
    pub const SFX_MIN_INTERVAL_MS: u64 = 30;

    /// Compute the viewport offset at `now`, plus whether the
    /// animation has fully settled. Pure function of stored state.
    pub fn position_at(&self, now: std::time::Instant) -> (usize, bool) {
        if self.total_items == 0 {
            return (self.original_offset, true);
        }

        match &self.decel {
            None => {
                // Indefinite cruise: integer-math linear interpolation at
                // constant rate. `position_at` is called every frame so
                // accumulated rounding from integer division is invisible —
                // visual rate is steps/sec exactly. Wraps via modulo.
                let elapsed = now.saturating_duration_since(self.start_time);
                let elapsed_ms = elapsed.as_millis() as u64;
                let steps = elapsed_ms.saturating_mul(self.cruise_pos_per_sec as u64) / 1000;
                let offset = (self.original_offset + steps as usize) % self.total_items;
                (offset, false)
            }
            Some(arm) => {
                // Decel + fake-out keyframe walk, anchored at stop_time.
                // Each non-terminal keyframe holds for its `duration_ms`;
                // the terminal keyframe is entered and immediately reports
                // settled.
                let mut remaining = now.saturating_duration_since(arm.stop_time);
                let last_idx = arm.decel_keyframes.len().saturating_sub(1);
                for (i, kf) in arm.decel_keyframes.iter().enumerate() {
                    if i == last_idx {
                        return (kf.offset, true);
                    }
                    let kf_d = std::time::Duration::from_millis(kf.duration_ms);
                    if remaining < kf_d {
                        return (kf.offset, false);
                    }
                    remaining -= kf_d;
                }
                // Empty keyframe list — degenerate but safe: settle on target.
                (arm.target_idx, true)
            }
        }
    }
}
