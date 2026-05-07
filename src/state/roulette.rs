//! Roulette (slot-machine random pick) animation state and easing.

/// Single keyframe in the post-spin fake-out walk.
///
/// The wheel snaps to `offset` at the start of the keyframe and holds
/// there for `duration_ms`. The last keyframe in the sequence is always
/// at `target_idx` and signals that the spin has settled.
#[derive(Debug, Clone, Copy)]
pub struct FakeoutKeyframe {
    /// Absolute viewport offset to display at this keyframe.
    pub offset: usize,
    /// How long to hold at `offset` before advancing to the next keyframe.
    /// The final keyframe's duration is unused (we settle on entering it).
    pub duration_ms: u64,
}

/// Time-driven state for an in-progress "Roulette" pick.
///
/// Snapshotted at start so subsequent data churn (page loads, search edits,
/// queue mutations) cannot drift the animation off the chosen target. The
/// final position lands at `target_idx`; intermediate offsets are derived
/// purely from `(start_time, main_duration_ms, cruise_duration_ms,
/// main_spin_steps, fakeout_keyframes)` via a constant-velocity cruise
/// followed by an `ease_out_quad` deceleration for the main spin and a
/// keyframe walk for the fake-out, so a tick handler is stateless beyond
/// bookkeeping.
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
    /// Eased main spin duration in milliseconds. Variable per spin —
    /// when the fake-out is short or absent the main spin claims more of
    /// the total roulette budget; longer fake-outs steal time back so the
    /// total feels roughly consistent.
    pub main_duration_ms: u64,
    /// Cruise phase duration in milliseconds. The main spin runs at
    /// constant velocity for this long before transitioning into the
    /// `ease_out_quad` deceleration phase. Velocities are matched at the
    /// handoff so there's no visible kink. Jittered per spin.
    pub cruise_duration_ms: u64,
    /// Cumulative-index distance the eased main spin walks. Lands at the
    /// first fake-out keyframe, not directly at `target_idx`. Inflated by
    /// full-list revolutions so the wheel "spins" several times.
    pub main_spin_steps: usize,
    /// Pre-rolled fake-out walk. First entry is the position the eased
    /// main spin lands on. Subsequent entries wobble around the target —
    /// sometimes overshoot, sometimes undershoot, sometimes zigzag, and
    /// sometimes the wheel just decelerates straight onto target with no
    /// wobble at all. The final entry is always `target_idx` and signals
    /// settle.
    pub fakeout_keyframes: Vec<FakeoutKeyframe>,
    /// Animation start timestamp.
    pub start_time: std::time::Instant,
    /// Last offset actually applied to the slot list. Tick handlers compare
    /// against the freshly-computed offset to decide whether to fire a Tab
    /// SFX / record_scroll().
    pub last_offset: usize,
    /// Last time a Tab SFX was fired. Throttled to one per ~30 ms so the
    /// fastest spin frames sound like a rattle rather than a buzz.
    pub last_sfx_at: Option<std::time::Instant>,
}

impl RouletteState {
    /// Minimum spacing between Tab SFX plays during the spin.
    pub const SFX_MIN_INTERVAL_MS: u64 = 30;

    /// Compute the viewport offset at `now`, plus whether the animation
    /// has fully settled. Pure function of stored state.
    pub fn position_at(&self, now: std::time::Instant) -> (usize, bool) {
        if self.total_items == 0 {
            return (self.original_offset, true);
        }

        let elapsed = now.saturating_duration_since(self.start_time);
        let main_d = std::time::Duration::from_millis(self.main_duration_ms);

        if elapsed < main_d {
            // Two-phase profile: constant-velocity cruise, then ease-out-quad
            // deceleration. Velocities match at the handoff so the wheel
            // transitions smoothly from "spinning fast" into "slowing down"
            // — a single eased curve from t=0 starts decelerating
            // immediately and never feels like a real wheel.
            //
            // For continuous velocity at the cruise→decel handoff with
            // ease_out_quad (whose initial derivative is 2):
            //     v_cruise = S1 / cruise_d = 2 * S2 / decel_d
            //   ⇒ S2 = S * decel_d / (decel_d + 2 * cruise_d)
            let cruise_s =
                ((self.cruise_duration_ms.min(self.main_duration_ms / 2)) as f32) / 1000.0;
            let main_s = main_d.as_secs_f32();
            let decel_s = (main_s - cruise_s).max(f32::EPSILON);
            let total_steps = self.main_spin_steps as f32;
            let s2 = total_steps * decel_s / (decel_s + 2.0 * cruise_s);
            let s1 = total_steps - s2;

            let elapsed_s = elapsed.as_secs_f32();
            let progress = if elapsed_s < cruise_s {
                (elapsed_s / cruise_s) * s1
            } else {
                let u = (elapsed_s - cruise_s) / decel_s;
                s1 + ease_out_quad(u) * s2
            };
            let steps = progress as usize;
            return (self.offset_after_steps(steps), false);
        }

        // Walk the fake-out keyframes. Each keyframe holds for its
        // duration; entering the final keyframe (target) marks settled.
        let mut remaining = elapsed - main_d;
        let last_idx = self.fakeout_keyframes.len().saturating_sub(1);
        for (i, kf) in self.fakeout_keyframes.iter().enumerate() {
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

    fn offset_after_steps(&self, steps: usize) -> usize {
        if self.total_items == 0 {
            return self.original_offset;
        }
        (self.original_offset + steps) % self.total_items
    }
}

/// Ease-out quadratic: linear deceleration from initial velocity 2 to 0.
/// Models a wheel slowing under roughly constant friction — which is what
/// the roulette decel phase is meant to feel like once the cruise ends.
fn ease_out_quad(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let inv = 1.0 - t;
    1.0 - inv * inv
}
