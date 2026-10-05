//! Reflection (Bars / Lines): the visualizer stands on a waterline and is
//! mirrored in a band of dark water below it.
//!
//! The visualizer draws into the band above [`WATER_LINE`]; `reflection.wgsl`
//! (a post-process over the displayed scene, after the scene blit) fills the
//! band below with the scene flipped about the waterline. [`KickRipples`] is
//! the CPU half: on each beat onset it drops a ripple under the bar that
//! jumped hardest, which the shader spreads sideways across the water by age.

/// Where the waterline sits, as a fraction of the visualizer height from the
/// top: the bars or the line stand above it, the reflection fills the band
/// below. The boat reads it too, to ride the line instead of the water.
pub(crate) const WATER_LINE: f32 = 0.68;

/// How much the reflection is squashed: water `d` px below the line mirrors
/// the scene `d * REFLECTION_STRETCH` px above it, so the band holds most of
/// the skyline.
pub(crate) const REFLECTION_STRETCH: f32 = 1.35;

/// Ripple slots the shader reads (`ReflectionParams::ripples`).
pub(crate) const RIPPLE_SLOTS: usize = 4;

/// A beat pulse above this drops a ripple...
const TRIGGER: f32 = 0.6;
/// ...and the next one waits for the pulse to fall below this.
const REARM: f32 = 0.35;
/// Seconds a ripple lives (the shader has faded it out well before).
const LIFE_SECS: f32 = 3.0;
/// FFT ticks per second (`VisualizerTiming::TICK_RATE_HZ`).
const TICK_SECS: f32 = 1.0 / 60.0;

/// The water's kick ripples: `(x 0..1 across, age s, strength, _)` per slot,
/// strength 0 = an empty slot.
pub(crate) struct KickRipples {
    events: [[f32; 4]; RIPPLE_SLOTS],
    prev: Vec<f32>,
    armed: bool,
}

impl KickRipples {
    pub(crate) fn new() -> Self {
        Self {
            events: [[0.0; 4]; RIPPLE_SLOTS],
            prev: Vec::new(),
            armed: true,
        }
    }

    /// Drop every ripple (a track change).
    pub(crate) fn clear(&mut self) {
        self.events = [[0.0; 4]; RIPPLE_SLOTS];
        self.prev.clear();
        self.armed = true;
    }

    pub(crate) fn events(&self) -> [[f32; 4]; RIPPLE_SLOTS] {
        self.events
    }

    /// One FFT tick: age the ripples, then drop a new one on a beat onset.
    /// `bars` are this tick's values (0..1), `beat` the normalized beat pulse,
    /// `bass` the bass band level (a heavier kick throws a stronger ripple).
    pub(crate) fn update(&mut self, bars: &[f64], beat: f32, bass: f32) {
        for e in &mut self.events {
            if e[2] > 0.0 {
                e[1] += TICK_SECS;
                if e[1] > LIFE_SECS {
                    *e = [0.0; 4];
                }
            }
        }
        let n = bars.len();
        if self.prev.len() != n {
            self.prev = bars.iter().map(|&v| v as f32).collect();
        }
        // The bar that jumped hardest since the last tick.
        let mut best = (0usize, 0.0f32);
        for (i, (&v, p)) in bars.iter().zip(self.prev.iter_mut()).enumerate() {
            let rise = v as f32 - *p;
            *p = v as f32;
            if rise > best.1 {
                best = (i, rise);
            }
        }
        if beat < REARM {
            self.armed = true;
        }
        if self.armed && beat > TRIGGER && n > 0 {
            self.armed = false;
            let x = (best.0 as f32 + 0.5) / n as f32;
            let strength = (0.45 + 1.1 * bass + 2.0 * best.1).min(1.5);
            // An empty slot, else the oldest ripple.
            let slot = self
                .events
                .iter()
                .enumerate()
                .max_by(|a, b| {
                    let age = |e: &[f32; 4]| if e[2] > 0.0 { e[1] } else { f32::MAX };
                    age(a.1).total_cmp(&age(b.1))
                })
                .map_or(0, |(i, _)| i);
            self.events[slot] = [x, 0.0, strength, 0.0];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live(r: &KickRipples) -> Vec<[f32; 4]> {
        r.events().into_iter().filter(|e| e[2] > 0.0).collect()
    }

    #[test]
    fn a_beat_onset_drops_one_ripple_under_the_bar_that_jumped() {
        let mut r = KickRipples::new();
        let quiet = vec![0.1; 10];
        r.update(&quiet, 0.0, 0.0);
        let mut kick = quiet.clone();
        kick[7] = 0.9;
        r.update(&kick, 0.9, 0.5);
        let ripples = live(&r);
        assert_eq!(ripples.len(), 1);
        assert!((ripples[0][0] - 0.75).abs() < 1e-6, "x of bar 7 of 10");
        // Still above the re-arm level: no second ripple for the same kick.
        r.update(&kick, 0.8, 0.5);
        assert_eq!(live(&r).len(), 1);
        // The pulse falls, re-arms, and the next kick drops another.
        r.update(&quiet, 0.1, 0.0);
        r.update(&kick, 0.9, 0.5);
        assert_eq!(live(&r).len(), 2);
    }

    #[test]
    fn ripples_age_out_and_a_full_set_replaces_the_oldest() {
        let mut r = KickRipples::new();
        let bars = vec![0.5; 4];
        for _ in 0..RIPPLE_SLOTS + 1 {
            r.update(&bars, 0.9, 0.2);
            r.update(&bars, 0.0, 0.0);
        }
        assert_eq!(
            live(&r).len(),
            RIPPLE_SLOTS,
            "the fifth replaced the oldest"
        );
        for _ in 0..(LIFE_SECS / TICK_SECS) as usize + 2 {
            r.update(&bars, 0.0, 0.0);
        }
        assert!(live(&r).is_empty(), "every ripple expires");
    }
}
