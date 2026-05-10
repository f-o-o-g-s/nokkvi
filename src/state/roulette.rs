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

/// Time-driven state for an in-progress "Roulette" pick.
///
/// Snapshotted at start so subsequent data churn (page loads, search
/// edits, queue mutations) cannot drift the animation off the chosen
/// target. The final position lands at `target_idx`; intermediate
/// offsets are derived purely from `(start_time, cruise_duration_ms,
/// cruise_steps, decel_keyframes)`, so a tick handler is stateless
/// beyond bookkeeping.
///
/// Two phases:
/// - **Cruise** (continuous): for the first `cruise_duration_ms`, the
///   wheel scrolls at constant velocity through `cruise_steps`
///   positions. Visually a fast blur; SFX fires at the throttled rate.
/// - **Decel** (discrete keyframe walk): the wheel ticks through
///   `decel_keyframes` one at a time, holding at each offset for the
///   keyframe's `duration_ms`. Holds escalate via a cubic curve from
///   ~50 ms (cruise-like rate) to ~950 ms (slot-machine final click),
///   so the click cadence audibly slows from ~20 Hz down to ~1 Hz
///   over the decel phase. The last 0-3 keyframes carry the chosen
///   FakeoutPattern wobble (overshoot, false-settle, etc.) with
///   explicit holds tuned to feel like a "rebound" after the long
///   final natural-walk hold.
///
/// All-decel variant: when `cruise_duration_ms == 0`, the cruise phase
/// is skipped and the wheel starts directly into the decel keyframe
/// walk. The natural-walk keyframes then advance multiple positions
/// each (velocity-weighted) instead of one, so the wheel still
/// traverses several revolutions even without a cruise blur — the
/// "thrown hard" feel.
#[derive(Debug, Clone)]
pub struct RouletteState {
    /// Slot-list view this roulette runs in.
    pub view: crate::View,
    /// Snapshotted item count — frozen so live mutations don't drift the math.
    pub total_items: usize,
    /// Viewport offset captured at start; restored on cancel.
    pub original_offset: usize,
    /// Pre-rolled landing index. The animation always settles here.
    pub target_idx: usize,
    /// Cruise phase duration in milliseconds. Jittered per spin, and
    /// occasionally zero (all-decel variant) so the wheel runs as a
    /// single continuous slowdown from start to settle.
    pub cruise_duration_ms: u64,
    /// Number of positions the cruise phase walks (continuous-velocity
    /// linear interpolation). At end of cruise the wheel is at
    /// `(original_offset + cruise_steps) % total_items` — typically
    /// the position the decel keyframes' first entry starts from.
    /// Zero in the all-decel variant.
    pub cruise_steps: usize,
    /// Pre-rolled decel + fake-out walk. Each entry holds at its
    /// `offset` for `duration_ms`; the terminal entry is at
    /// `target_idx` with `duration_ms = 0`. Holds escalate via a cubic
    /// curve over the natural-walk keyframes, with the last 0-3
    /// entries carrying pattern-specific wobble timing.
    pub decel_keyframes: Vec<DecelKeyframe>,
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

        let elapsed = now.saturating_duration_since(self.start_time);
        let cruise_d = std::time::Duration::from_millis(self.cruise_duration_ms);

        if elapsed < cruise_d {
            // Constant-velocity cruise: linear interpolation through
            // `cruise_steps` positions. With `cruise_duration_ms = 0`
            // this branch is skipped entirely (no cruise — the
            // all-decel variant).
            let cruise_s = cruise_d.as_secs_f32().max(f32::EPSILON);
            let elapsed_s = elapsed.as_secs_f32();
            let progress = (elapsed_s / cruise_s) * (self.cruise_steps as f32);
            let steps = progress as usize;
            return ((self.original_offset + steps) % self.total_items, false);
        }

        // Decel + fake-out keyframe walk. Each non-terminal keyframe
        // holds for its `duration_ms`; the terminal keyframe is
        // entered and immediately reports settled.
        let mut remaining = elapsed - cruise_d;
        let last_idx = self.decel_keyframes.len().saturating_sub(1);
        for (i, kf) in self.decel_keyframes.iter().enumerate() {
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
        (self.target_idx, true)
    }
}
