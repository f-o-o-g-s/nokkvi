//! Beat-highlight flash field for the Bars visualizer.
//!
//! Each FFT tick feeds in the post-smoothing bar values. A bar that rises by
//! more than `spike_threshold` in one tick is an onset candidate; plateau-aware
//! peak picking plus non-maximum suppression collapse the monstercat spread of
//! one transient into a single [`FlashEvent`] with a sub-bar center. The
//! per-bar flash buffer (`bars.wgsl` blooms each bar toward the peak color by
//! its value) is then re-rendered from that short event list every tick: each
//! event is a smooth bump that ripples outward from its center under an
//! attack/release envelope, and overlapping events combine by soft-max.
//!
//! Rendering from events instead of decaying a stored buffer keeps the shape
//! exact at every age, and a tick whose publish is skipped heals on the next
//! one. Time counts in ticks (one per processed audio chunk), so the field
//! freezes while paused and stays locked to the audio clock.

use std::f32::consts::PI;

/// The look's tunables, in one place so it can be iterated quickly.
#[derive(Debug, Clone, Copy)]
struct FlashTuning {
    /// Rise in one tick (normalized bar units) that counts as an onset.
    spike_threshold: f64,
    /// Rise that maps to a full-strength event.
    spike_full_scale: f64,
    /// Strength boost on a strong beat: `A * (1 + beat_gain * beat_pulse)`.
    beat_gain: f32,
    /// Highlight radius as a share of the bar count, clamped to
    /// `kernel_min..=kernel_max` bars.
    kernel_fraction: f32,
    kernel_min: f32,
    kernel_max: f32,
    /// `false` = raised cosine (reaches zero with zero slope), `true` = a
    /// literal triangle (ends in a visible step at its last bar).
    triangle_kernel: bool,
    /// Attack and release time constants in ticks. 1 / 11 puts the peak near
    /// 41 ms and fades over roughly 300 ms at 60 Hz.
    attack_tau: f32,
    release_tau: f32,
    /// Ticks for the leading edge to ripple out to the full radius
    /// (0 = light the whole bump at once).
    ripple_ticks: f32,
    /// An onset within `merge_distance` bars of an event `merge_ticks` old or
    /// younger folds into it: one transient's rise often spans two or three
    /// ticks. Kept well inside the radius so a separate hit nearby gets its
    /// own event instead of being buried in a weaker one.
    merge_ticks: f32,
    merge_distance: f32,
    /// Onsets picked per tick, strongest first. Caps how much of a broadband
    /// hit (a crash across the whole spectrum) lights up at once.
    max_new_events: usize,
}

const TUNING: FlashTuning = FlashTuning {
    spike_threshold: 0.12,
    spike_full_scale: 0.2,
    beat_gain: 0.35,
    kernel_fraction: 0.045,
    kernel_min: 3.0,
    kernel_max: 7.0,
    triangle_kernel: false,
    attack_tau: 1.0,
    release_tau: 11.0,
    ripple_ticks: 4.0,
    merge_ticks: 3.0,
    merge_distance: 1.5,
    max_new_events: 3,
};

/// Live event cap; a newcomer replaces the faintest event once it is full.
/// Events live about 55 ticks, so this holds ~35 onsets per second before an
/// eviction can cut a still-visible event (a hard one-tick drop).
const MAX_EVENTS: usize = 32;
/// Events whose envelope has fallen below this (after the peak) retire.
const RETIRE_LEVEL: f32 = 0.01;

/// One highlight: a bump centered between bars, fading with age.
#[derive(Debug, Clone, Copy, PartialEq)]
struct FlashEvent {
    /// Sub-bar center, in bar-index units.
    center: f32,
    /// Peak strength `A` in `[0, 1]`.
    strength: f32,
    /// Ticks since the onset.
    age: f32,
}

/// An onset picked from one tick's rises.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Onset {
    center: f32,
    rise: f64,
}

/// Onset events plus the reusable per-bar buffers they render into. Nothing
/// here allocates per tick once the buffers have reached the bar count.
#[derive(Debug, Clone)]
pub(super) struct FlashField {
    tuning: FlashTuning,
    events: Vec<FlashEvent>,
    /// Last tick's bar values, the baseline for `rise`.
    prev: Vec<f64>,
    /// Scratch: this tick's per-bar rise, `max(bar - prev, 0)`.
    rise: Vec<f64>,
    /// Scratch: onset candidates, then the picked onsets.
    onsets: Vec<Onset>,
    /// The rendered field, each value in `[0, 1]`.
    out: Vec<f32>,
    /// False until a tick has set `prev`, so the first tick after a reset or
    /// resize only primes the baseline instead of reading every bar as a rise.
    primed: bool,
}

impl FlashField {
    pub(super) fn new(bar_count: usize) -> Self {
        Self::with_tuning(bar_count, TUNING)
    }

    fn with_tuning(bar_count: usize, tuning: FlashTuning) -> Self {
        let mut field = Self {
            tuning,
            events: Vec::with_capacity(MAX_EVENTS),
            prev: Vec::new(),
            rise: Vec::new(),
            onsets: Vec::new(),
            out: Vec::new(),
            primed: false,
        };
        field.resize(bar_count);
        field
    }

    /// Size every per-bar buffer to `bar_count` and forget all events.
    pub(super) fn resize(&mut self, bar_count: usize) {
        for buf in [&mut self.prev, &mut self.rise] {
            buf.clear();
            buf.resize(bar_count, 0.0);
        }
        self.out.clear();
        self.out.resize(bar_count, 0.0);
        self.onsets.clear();
        self.onsets.reserve(bar_count);
        self.reset();
    }

    /// Forget all events and re-prime the baseline, keeping the bar count.
    pub(super) fn reset(&mut self) {
        self.events.clear();
        self.out.fill(0.0);
        self.primed = false;
    }

    /// Advance one tick on this tick's bar values and return the field.
    ///
    /// `beat_pulse` is the normalized `[0, 1]` beat envelope; it only scales
    /// the strength of onsets found in the bars, never triggers or places one.
    pub(super) fn step(&mut self, bars: &[f64], beat_pulse: f32) -> &[f32] {
        let n = bars.len();
        if n != self.prev.len() {
            self.resize(n);
        }
        self.update_rise(bars);

        let tuning = self.tuning;
        let radius = kernel_radius(n, &tuning);
        let beat = if beat_pulse.is_finite() {
            beat_pulse.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let last = n.saturating_sub(1) as f32;
        let mut onsets = std::mem::take(&mut self.onsets);
        find_onsets(
            &self.rise,
            tuning.spike_threshold,
            radius,
            tuning.max_new_events,
            &mut onsets,
        );
        for onset in &onsets {
            let strength = onset_strength(onset.rise, beat, &tuning);
            self.spawn(onset.center.clamp(0.0, last), strength);
        }
        self.onsets = onsets;

        for event in &mut self.events {
            event.age += 1.0;
        }
        let peak = envelope_peak_time(tuning.attack_tau, tuning.release_tau);
        self.events
            .retain(|e| e.age <= peak || event_level(e, &tuning) >= RETIRE_LEVEL);

        render(&self.events, radius, &tuning, &mut self.out);
        &self.out
    }

    /// `rise = max(bar - prev, 0)` per bar, then `prev = bar`. Non-finite bar
    /// values read as silence so they can never reach the GPU.
    fn update_rise(&mut self, bars: &[f64]) {
        for ((rise, prev), &bar) in self.rise.iter_mut().zip(self.prev.iter_mut()).zip(bars) {
            let bar = if bar.is_finite() { bar } else { 0.0 };
            *rise = if self.primed {
                (bar - *prev).max(0.0)
            } else {
                0.0
            };
            *prev = bar;
        }
        self.primed = true;
    }

    /// Add an event for an onset, folding it into the nearest young event
    /// when it continues the same transient, and replacing the faintest event
    /// when the list is full.
    fn spawn(&mut self, center: f32, strength: f32) {
        let tuning = self.tuning;
        let nearest_young = self
            .events
            .iter_mut()
            .filter(|e| e.age <= tuning.merge_ticks)
            .map(|e| ((e.center - center).abs(), e))
            .filter(|(d, _)| *d <= tuning.merge_distance)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, event)) = nearest_young {
            // Strength-weighted center, so a stronger follow-up pulls the
            // bump toward itself instead of lighting beside it.
            let total = event.strength + strength;
            if total > 0.0 {
                event.center = (event.center * event.strength + center * strength) / total;
            }
            event.strength = event.strength.max(strength);
            return;
        }
        let event = FlashEvent {
            center,
            strength,
            age: 0.0,
        };
        if self.events.len() < MAX_EVENTS {
            self.events.push(event);
            return;
        }
        // Rank by the level an event will still reach: one in its attack
        // (this tick's newcomers sit at age 0, level 0) counts at its peak.
        let peak = envelope_peak_time(tuning.attack_tau, tuning.release_tau);
        let faintest = self
            .events
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let at = FlashEvent {
                    age: e.age.max(peak),
                    ..*e
                };
                (i, event_level(&at, &tuning))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, level)) = faintest
            && level < strength
        {
            self.events[i] = event;
        }
    }
}

/// An event's current envelope level, `A * envelope(age)`.
fn event_level(event: &FlashEvent, tuning: &FlashTuning) -> f32 {
    event.strength * envelope(event.age, tuning.attack_tau, tuning.release_tau)
}

/// Render `events` into `out`: each event's envelope level times the kernel
/// and the ripple's leading edge, combined across events by soft-max.
fn render(events: &[FlashEvent], radius: f32, tuning: &FlashTuning, out: &mut [f32]) {
    out.fill(0.0);
    let Some(last) = out.len().checked_sub(1) else {
        return;
    };
    let reach = radius + 1.0;
    for event in events {
        let level = event_level(event, tuning);
        if level.is_nan() || level <= 0.0 {
            continue;
        }
        let lo = (event.center - reach).floor().max(0.0) as usize;
        let hi = ((event.center + reach).ceil().max(0.0) as usize).min(last);
        for (i, value) in out.iter_mut().enumerate().take(hi + 1).skip(lo) {
            let d = (i as f32 - event.center).abs();
            let term = level
                * kernel_weight(d, radius, tuning.triangle_kernel)
                * ripple_edge(d, event.age, radius, tuning.ripple_ticks);
            *value = soft_max(*value, term.clamp(0.0, 1.0));
        }
    }
}

/// Highlight radius in bars for a bar count.
fn kernel_radius(bar_count: usize, tuning: &FlashTuning) -> f32 {
    // max/min rather than clamp: clamp panics on an inverted range.
    (bar_count as f32 * tuning.kernel_fraction)
        .round()
        .max(tuning.kernel_min)
        .min(tuning.kernel_max)
}

/// Kernel weight at distance `d` bars: 1 at the center, 0 from `radius + 1`.
fn kernel_weight(d: f32, radius: f32, triangle: bool) -> f32 {
    let x = (d.abs() / (radius.max(0.0) + 1.0)).min(1.0);
    if triangle {
        1.0 - x
    } else {
        0.5 * (1.0 + (PI * x).cos())
    }
}

/// Leading-edge mask of the outward ripple at `age` ticks: bars inside the
/// edge read 1, the bar the edge is crossing reads its fraction, beyond it 0.
fn ripple_edge(d: f32, age: f32, radius: f32, ripple_ticks: f32) -> f32 {
    let edge = if ripple_ticks > 0.0 {
        radius * (age / ripple_ticks).min(1.0)
    } else {
        radius
    };
    (1.0 - (d.abs() - edge)).clamp(0.0, 1.0)
}

/// Ticks from onset to the envelope's peak: `τa · ln((τa + τr) / τa)`.
fn envelope_peak_time(attack_tau: f32, release_tau: f32) -> f32 {
    if attack_tau <= 0.0 {
        return 0.0;
    }
    attack_tau * ((attack_tau + release_tau.max(0.0)) / attack_tau).ln()
}

/// Attack/release envelope at `age` ticks, normalized so its peak is 1:
/// `(1 - e^(-t/τa)) · e^(-t/τr) / norm`.
fn envelope(age: f32, attack_tau: f32, release_tau: f32) -> f32 {
    let age = age.max(0.0);
    let release_tau = release_tau.max(f32::EPSILON);
    let release = |t: f32| (-t / release_tau).exp();
    if attack_tau <= 0.0 {
        return release(age);
    }
    let attack = |t: f32| 1.0 - (-t / attack_tau).exp();
    let peak = envelope_peak_time(attack_tau, release_tau);
    attack(age) * release(age) / (attack(peak) * release(peak))
}

/// Combine two highlights, `1 - (1 - a)(1 - b)`: bounded by 1, no hard ridge.
fn soft_max(a: f32, b: f32) -> f32 {
    1.0 - (1.0 - a) * (1.0 - b)
}

/// Event strength from a rise, boosted by the normalized beat pulse.
fn onset_strength(rise: f64, beat_pulse: f32, tuning: &FlashTuning) -> f32 {
    let base = (rise / tuning.spike_full_scale).min(1.0) as f32;
    (base * (1.0 + tuning.beat_gain * beat_pulse)).clamp(0.0, 1.0)
}

/// Sub-bar offset of the peak at `i` by parabolic interpolation over its
/// neighbors, clamped to `±0.5`. Zero at the edges and on flat or degenerate
/// neighborhoods (the zero-denominator case).
fn parabolic_offset(rise: &[f64], i: usize) -> f32 {
    if i == 0 || i + 1 >= rise.len() {
        return 0.0;
    }
    let (l, c, r) = (rise[i - 1], rise[i], rise[i + 1]);
    let denom = l - 2.0 * c + r;
    if denom.abs() < 1e-12 {
        return 0.0;
    }
    let delta = (0.5 * (l - r) / denom).clamp(-0.5, 0.5);
    if delta.is_finite() { delta as f32 } else { 0.0 }
}

/// Pick up to `max_onsets` onsets from `rise`: plateau-aware local maxima above
/// `threshold`, strongest first, each more than `radius` bars from the others.
/// A flat run of equal maxima is one peak at the run's midpoint.
fn find_onsets(rise: &[f64], threshold: f64, radius: f32, max_onsets: usize, out: &mut Vec<Onset>) {
    out.clear();
    let n = rise.len();
    let mut i = 0;
    while i < n {
        let value = rise[i];
        if value.is_nan() || value <= threshold {
            i += 1;
            continue;
        }
        let mut j = i;
        while j + 1 < n && rise[j + 1] == value {
            j += 1;
        }
        let left_lower = i == 0 || rise[i - 1] < value;
        let right_lower = j + 1 == n || rise[j + 1] < value;
        if left_lower && right_lower {
            let center = if i == j {
                i as f32 + parabolic_offset(rise, i)
            } else {
                (i + j) as f32 * 0.5
            };
            out.push(Onset {
                center,
                rise: value,
            });
        }
        i = j + 1;
    }

    // Greedy non-maximum suppression, strongest first (position breaks ties
    // so the pick is deterministic).
    out.sort_unstable_by(|a, b| {
        b.rise
            .total_cmp(&a.rise)
            .then(a.center.total_cmp(&b.center))
    });
    let mut kept = 0;
    for k in 0..out.len() {
        if kept == max_onsets {
            break;
        }
        let candidate = out[k];
        if out[..kept]
            .iter()
            .all(|picked| (picked.center - candidate.center).abs() > radius)
        {
            out[kept] = candidate;
            kept += 1;
        }
    }
    out.truncate(kept);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::visualizer::state::{interpolate_bars, monstercat_filter};

    const BASE: f64 = 0.1;

    /// A field primed on a flat `BASE` spectrum.
    fn primed(n: usize, tuning: FlashTuning) -> FlashField {
        let mut field = FlashField::with_tuning(n, tuning);
        field.step(&vec![BASE; n], 0.0);
        field
    }

    /// `BASE` everywhere except the given `(index, value)` hits.
    fn spectrum(n: usize, hits: &[(usize, f64)]) -> Vec<f64> {
        let mut bars = vec![BASE; n];
        for &(i, v) in hits {
            bars[i] = v;
        }
        bars
    }

    #[test]
    fn kernel_is_symmetric_monotone_and_spans_the_radius() {
        for triangle in [false, true] {
            for radius in [3.0_f32, 4.0, 5.0, 6.0, 7.0] {
                assert!((kernel_weight(0.0, radius, triangle) - 1.0).abs() < 1e-6);
                assert!(kernel_weight(radius + 1.0, radius, triangle).abs() < 1e-6);
                assert!(kernel_weight(radius + 9.0, radius, triangle).abs() < 1e-6);
                let mut prev = 1.0_f32;
                for step in 0..=400 {
                    let d = step as f32 * (radius + 2.0) / 400.0;
                    let w = kernel_weight(d, radius, triangle);
                    assert!((w - kernel_weight(-d, radius, triangle)).abs() < 1e-6);
                    assert!(w <= prev + 1e-6, "not monotone at d={d} (r={radius})");
                    assert!((0.0..=1.0).contains(&w));
                    prev = w;
                }
            }
        }
    }

    #[test]
    fn kernel_radius_follows_bar_count_within_its_clamp() {
        assert_eq!(kernel_radius(4, &TUNING), 3.0);
        assert_eq!(kernel_radius(46, &TUNING), 3.0);
        assert_eq!(kernel_radius(143, &TUNING), 6.0);
        assert_eq!(kernel_radius(2048, &TUNING), 7.0);
    }

    #[test]
    fn soft_max_never_exceeds_one_or_drops_below_either_input() {
        for a in 0..=20 {
            for b in 0..=20 {
                let (a, b) = (a as f32 / 20.0, b as f32 / 20.0);
                let c = soft_max(a, b);
                assert!(c <= 1.0 && c >= a.max(b) - 1e-6, "soft_max({a}, {b}) = {c}");
            }
        }
    }

    #[test]
    fn envelope_peaks_at_exactly_one_about_41_ms_in() {
        let (attack, release) = (TUNING.attack_tau, TUNING.release_tau);
        let peak = envelope_peak_time(attack, release);
        assert!((envelope(peak, attack, release) - 1.0).abs() < 1e-5);
        let sampled_max = (0..20_000)
            .map(|k| envelope(k as f32 * 0.01, attack, release))
            .fold(0.0_f32, f32::max);
        assert!((sampled_max - 1.0).abs() < 1e-4 && sampled_max <= 1.0 + 1e-5);
        let peak_ms = peak * 1000.0 / 60.0;
        assert!((peak_ms - 41.4).abs() < 1.0, "peak at {peak_ms} ms");
        assert_eq!(envelope(0.0, attack, release), 0.0);
    }

    #[test]
    fn a_single_event_peaks_at_its_strength() {
        let n = 48;
        let mut field = primed(n, TUNING);
        // Symmetric rises of 0.8 / 0.4 / 0.4 around bar 20: full strength.
        let hit = spectrum(n, &[(19, 0.5), (20, 0.9), (21, 0.5)]);
        let mut core_max = 0.0_f32;
        for _ in 0..20 {
            core_max = core_max.max(field.step(&hit, 0.0)[20]);
        }
        // Ticks land on integer ages, so the sampled max sits a hair below the
        // continuous peak (0.99 at age 2 or 3 against 1.0 at age 2.48).
        assert!(
            core_max > 0.98 && core_max <= 1.0,
            "core peaked at {core_max}"
        );
    }

    #[test]
    fn first_tick_after_construction_only_primes_the_baseline() {
        let mut field = FlashField::new(32);
        let out = field.step(&[0.9; 32], 1.0).to_vec();
        assert!(field.events.is_empty());
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn a_monstercat_spread_kick_yields_one_event() {
        // FFT-resolution kick over three bass bins, spread by monstercat and
        // interpolated up to the visual bar count, exactly as `tick()` does.
        let (fft_bins, visual) = (40, 120);
        let mut before = vec![0.05; fft_bins];
        let mut after = before.clone();
        after[2] = 0.7;
        after[3] = 0.9;
        after[4] = 0.75;
        monstercat_filter(&mut before, 1.0);
        monstercat_filter(&mut after, 1.0);
        let before = interpolate_bars(&before, visual);
        let after = interpolate_bars(&after, visual);
        let risen = before
            .iter()
            .zip(&after)
            .filter(|(b, a)| *a - *b > TUNING.spike_threshold)
            .count();
        assert!(
            risen > 6,
            "the kick should lift a wide plateau, lifted {risen}"
        );

        let mut field = FlashField::new(visual);
        field.step(&before, 0.0);
        field.step(&after, 0.0);
        assert_eq!(field.events.len(), 1, "events: {:?}", field.events);
    }

    #[test]
    fn a_flat_plateau_yields_one_centered_event() {
        let mut onsets = Vec::new();
        let mut rise = vec![0.0; 30];
        rise[10..=20].fill(0.5);
        find_onsets(&rise, 0.12, 3.0, TUNING.max_new_events, &mut onsets);
        assert_eq!(onsets.len(), 1);
        assert!((onsets[0].center - 15.0).abs() < 1e-6);

        // Plateaus touching either edge still pick their midpoint.
        let mut rise = vec![0.0; 30];
        rise[0..=5].fill(0.5);
        rise[25..].fill(0.4);
        find_onsets(&rise, 0.12, 3.0, TUNING.max_new_events, &mut onsets);
        let centers: Vec<f32> = onsets.iter().map(|o| o.center).collect();
        assert_eq!(centers, vec![2.5, 27.0]);
    }

    #[test]
    fn non_maximum_suppression_keeps_the_strongest_within_the_radius() {
        let mut onsets = Vec::new();
        let mut rise = vec![0.0; 40];
        rise[10] = 0.3;
        rise[12] = 0.5; // within 3 of bar 10: suppresses it
        rise[30] = 0.2; // far away: kept
        find_onsets(&rise, 0.12, 3.0, TUNING.max_new_events, &mut onsets);
        let centers: Vec<f32> = onsets.iter().map(|o| o.center).collect();
        assert_eq!(centers, vec![12.0, 30.0]);

        // Never more than the per-tick cap, strongest first.
        let rise: Vec<f64> = (0..60)
            .map(|i| {
                if i % 10 == 0 {
                    0.2 + i as f64 / 100.0
                } else {
                    0.0
                }
            })
            .collect();
        find_onsets(&rise, 0.12, 3.0, TUNING.max_new_events, &mut onsets);
        let centers: Vec<f32> = onsets.iter().map(|o| o.center).collect();
        assert_eq!(centers, vec![50.0, 40.0, 30.0]);
    }

    #[test]
    fn sub_bar_offset_is_bounded_finite_and_points_at_the_heavier_side() {
        let steps: Vec<f64> = (0..=20).map(|k| k as f64 * 0.05).collect();
        for &l in &steps {
            for &r in &steps {
                for bump in [0.0, 1e-15, 1e-9, 0.01, 0.3] {
                    let c = l.max(r) + bump;
                    let off = parabolic_offset(&[l, c, r], 1);
                    assert!(
                        off.is_finite() && off.abs() <= 0.5,
                        "[{l}, {c}, {r}] -> {off}"
                    );
                }
            }
        }
        assert_eq!(parabolic_offset(&[0.3, 0.3, 0.3], 1), 0.0);
        assert_eq!(parabolic_offset(&[0.2, 0.5, 0.2], 1), 0.0);
        assert!(parabolic_offset(&[0.2, 0.5, 0.4], 1) > 0.0);
        assert!(parabolic_offset(&[0.4, 0.5, 0.2], 1) < 0.0);
        // Edges have one neighbor: no interpolation.
        assert_eq!(parabolic_offset(&[0.5, 0.2], 0), 0.0);
        assert_eq!(parabolic_offset(&[0.2, 0.5], 1), 0.0);
    }

    #[test]
    fn onsets_at_the_first_and_last_bar_stay_in_bounds() {
        let n = 16;
        let mut field = primed(n, TUNING);
        let hit = spectrum(n, &[(0, 0.9), (n - 1, 0.9)]);
        let out = field.step(&hit, 1.0).to_vec();
        assert_eq!(field.events.len(), 2);
        for event in &field.events {
            assert!((0.0..=(n - 1) as f32).contains(&event.center), "{event:?}");
        }
        assert!(out[0] > 0.0 && out[n - 1] > 0.0);
        for _ in 0..120 {
            let out = field.step(&hit, 1.0);
            assert_eq!(out.len(), n);
            assert!(out.iter().all(|v| (0.0..=1.0).contains(v)));
        }
    }

    #[test]
    fn shoulder_to_core_ratio_holds_through_the_release() {
        let n = 48;
        let mut field = primed(n, TUNING);
        let hit = spectrum(n, &[(19, 0.5), (20, 0.9), (21, 0.5)]);
        let mut expected: Option<(f32, f32)> = None;
        let mut checked = 0;
        for tick in 1..=60 {
            let out = field.step(&hit, 0.0);
            if (tick as f32) < TUNING.ripple_ticks + 1.0 || out[20] < 0.02 {
                continue;
            }
            let ratios = (out[21] / out[20], out[22] / out[20]);
            let want = *expected.get_or_insert(ratios);
            assert!((ratios.0 - want.0).abs() < 1e-4 && (ratios.1 - want.1).abs() < 1e-4);
            checked += 1;
        }
        assert!(checked > 20, "only {checked} release ticks checked");
        let (near, far) = expected.unwrap_or_default();
        assert!(near > far && far > 0.0, "shoulders {near} / {far}");
    }

    #[test]
    fn the_ripple_reaches_the_rim_after_its_ticks() {
        let n = 48;
        let mut field = primed(n, TUNING);
        let hit = spectrum(n, &[(19, 0.5), (20, 0.9), (21, 0.5)]);
        let first = field.step(&hit, 0.0).to_vec();
        assert!(
            first[20] > 0.0 && first[23] == 0.0,
            "rim lit on the first tick"
        );
        for _ in 1..(TUNING.ripple_ticks as usize) {
            field.step(&hit, 0.0);
        }
        assert!(
            field.step(&hit, 0.0)[23] > 0.0,
            "rim still dark after the ripple"
        );

        // With the ripple off the whole bump lights at once.
        let mut field = primed(
            n,
            FlashTuning {
                ripple_ticks: 0.0,
                ..TUNING
            },
        );
        assert!(field.step(&hit, 0.0)[23] > 0.0);
    }

    #[test]
    fn a_follow_up_rise_folds_into_the_young_event() {
        let n = 48;
        let mut field = primed(n, TUNING);
        field.step(&spectrum(n, &[(20, 0.4)]), 0.0);
        field.step(&spectrum(n, &[(20, 0.6)]), 0.0);
        assert_eq!(field.events.len(), 1, "events: {:?}", field.events);
    }

    #[test]
    fn a_stronger_follow_up_pulls_the_merged_center_toward_itself() {
        let n = 48;
        let mut field = primed(n, TUNING);
        field.step(&spectrum(n, &[(20, 0.23)]), 0.0); // weak: rise 0.13
        field.step(&spectrum(n, &[(20, 0.23), (21, 0.9)]), 0.0); // strong, one bar over
        assert_eq!(field.events.len(), 1, "events: {:?}", field.events);
        let event = field.events[0];
        assert!(event.center > 20.5 && event.center < 21.0, "{event:?}");
        assert_eq!(event.strength, 1.0);
    }

    #[test]
    fn a_strong_onset_is_not_buried_in_a_weak_neighbor() {
        let n = 48;
        let mut field = primed(n, TUNING);
        field.step(&spectrum(n, &[(20, 0.23)]), 0.0); // weak event at bar 20
        let strong = spectrum(n, &[(20, 0.23), (23, 0.9)]); // full-strength hit 3 bars over
        let mut peak_at_hit = 0.0_f32;
        for _ in 0..20 {
            peak_at_hit = peak_at_hit.max(field.step(&strong, 0.0)[23]);
        }
        assert!(peak_at_hit > 0.9, "bar 23 peaked at {peak_at_hit}");
    }

    #[test]
    fn a_steady_stream_of_onsets_never_pops() {
        // Dense music: three well-separated onsets every 4 ticks (~45 per
        // second). The list must never evict a still-visible event (a hard
        // one-tick drop), only let events release.
        let n = 143;
        let mut field = primed(n, TUNING);
        let flat = vec![BASE; n];
        let mut prev = vec![0.0_f32; n];
        let mut worst_drop = 0.0_f32;
        for tick in 0..600 {
            let bars = if tick % 4 == 0 {
                let hits: Vec<(usize, f64)> = (0..3)
                    .map(|j| ((7 + 13 * (3 * (tick / 4) + j)) % 140, 0.26))
                    .collect();
                spectrum(n, &hits)
            } else {
                flat.clone()
            };
            let out = field.step(&bars, 0.0);
            for (now, before) in out.iter().zip(&prev) {
                worst_drop = worst_drop.max(before - now);
            }
            prev.copy_from_slice(out);
        }
        assert!(worst_drop < 0.15, "a bar dropped {worst_drop} in one tick");
    }

    #[test]
    fn beat_pulse_boosts_weak_onsets_but_never_past_one() {
        let weak = onset_strength(0.13, 0.0, &TUNING);
        let boosted = onset_strength(0.13, 1.0, &TUNING);
        assert!(weak > 0.0 && boosted > weak && boosted <= 1.0);
        assert_eq!(onset_strength(0.5, 1.0, &TUNING), 1.0);
    }

    #[test]
    fn the_event_cap_holds_under_dense_onsets() {
        let n = 120;
        let mut field = primed(n, TUNING);
        let flat = vec![BASE; n];
        for round in 0..40 {
            let hits: Vec<(usize, f64)> = (0..6).map(|j| ((round * 7 + j * 20) % n, 0.9)).collect();
            let before = field.events.len();
            field.step(&spectrum(n, &hits), 0.5);
            let spawned = field.events.iter().filter(|e| e.age <= 1.0).count();
            assert!(
                spawned <= TUNING.max_new_events,
                "{spawned} spawned (had {before})"
            );
            assert!(field.events.len() <= MAX_EVENTS);
            field.step(&flat, 0.0);
            assert!(field.events.len() <= MAX_EVENTS);
        }
    }

    #[test]
    fn a_full_list_evicts_faded_events_not_this_ticks_newcomers() {
        let n = 400;
        let mut field = primed(n, TUNING);
        let faded = FlashEvent {
            center: 0.0,
            strength: 1.0,
            age: 30.0,
        };
        field.events = (0..MAX_EVENTS)
            .map(|k| FlashEvent {
                center: 60.0 + k as f32 * 7.0,
                ..faded
            })
            .collect();
        field.step(&spectrum(n, &[(5, 0.9), (20, 0.8), (35, 0.7)]), 0.0);
        assert_eq!(field.events.len(), MAX_EVENTS);
        let fresh: Vec<f32> = field
            .events
            .iter()
            .filter(|e| e.age <= 1.0)
            .map(|e| e.center)
            .collect();
        assert_eq!(fresh.len(), 3, "fresh events at {fresh:?}");
    }

    #[test]
    fn resize_and_reset_leave_no_stale_events() {
        let mut field = primed(100, TUNING);
        field.step(&spectrum(100, &[(90, 0.9)]), 0.0);
        assert_eq!(field.events.len(), 1);

        // Shrinking below the event's center drops it and re-primes.
        let out = field.step(&[0.9; 20], 0.0).to_vec();
        assert!(field.events.is_empty());
        assert_eq!(out, vec![0.0; 20]);
        // Growing back only primes, even though every bar "rose".
        field.step(&[0.9; 100], 0.0);
        assert!(field.events.is_empty());

        field.step(&spectrum(100, &[(50, 0.1)]), 0.0);
        field.step(&[0.9; 100], 0.0);
        assert!(!field.events.is_empty());
        field.reset();
        assert!(field.events.is_empty());
        let out = field.step(&[0.95; 100], 0.0);
        assert!(
            out.iter().all(|&v| v == 0.0),
            "reset must re-prime the baseline"
        );
    }

    #[test]
    fn non_finite_input_never_reaches_the_output() {
        let n = 24;
        let mut field = primed(n, TUNING);
        let mut bad = spectrum(n, &[(5, 0.9), (12, 0.9)]);
        bad[3] = f64::NAN;
        bad[8] = f64::INFINITY;
        bad[9] = f64::NEG_INFINITY;
        for beat in [f32::NAN, f32::INFINITY, 2.0, -1.0] {
            for _ in 0..8 {
                let out = field.step(&bad, beat);
                assert!(out.iter().all(|v| (0.0..=1.0).contains(v)), "{out:?}");
            }
        }
        assert!(
            field
                .events
                .iter()
                .all(|e| e.center.is_finite() && e.strength <= 1.0)
        );
    }
}
