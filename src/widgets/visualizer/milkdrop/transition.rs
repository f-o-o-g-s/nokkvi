//! The crossfade between two MilkDrop presets, as pure bookkeeping.
//!
//! When a new renderer arrives while one is on screen, the old one is kept
//! alive as the *outgoing* side and both advance in lockstep while the blit
//! dissolves from it into the *incoming* one. Progress counts renderer
//! advances, not wall time, so pausing freezes the mix and resuming continues
//! it. Never more than two renderers live: a third arrival mid-fade keeps
//! whichever side dominates the picture and drops the other.
//!
//! Generic over the slot type so it is tested without a GPU.

use super::MILKDROP_FRAME_INTERVAL;

/// Renderer advances a fade of `secs` lasts, clamped to half the preset
/// interval (as the audio crossfade clamps itself) unless that is 0 (never
/// switch on a timer). 0, negative and NaN give 0: a hard cut.
pub(crate) fn fade_frames(secs: f32, interval_secs: f32) -> u32 {
    if !secs.is_finite() || secs <= 0.0 {
        return 0;
    }
    let len = if interval_secs.is_finite() && interval_secs > 0.0 {
        secs.min(interval_secs / 2.0)
    } else {
        secs
    };
    (len / MILKDROP_FRAME_INTERVAL.as_secs_f32()).round() as u32
}

/// The per-pixel order in which the incoming preset replaces the outgoing
/// one. Picked per switch from the fade's seed, as MilkDrop randomises its
/// blend patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlendPattern {
    /// Everywhere at once (a staggered max, so neither side dips).
    Uniform,
    /// A soft edge sweeping across in a random direction.
    Wipe,
    /// A soft ring growing from the centre, or closing in from the rim.
    Radial,
    /// Blotches of smooth noise.
    Plasma,
}

impl BlendPattern {
    const ALL: [Self; 4] = [Self::Uniform, Self::Wipe, Self::Radial, Self::Plasma];

    pub(crate) fn pick(seed: u32) -> Self {
        Self::ALL[(seed % Self::ALL.len() as u32) as usize]
    }

    /// The `pattern` value `blit.wgsl` switches on.
    pub(crate) fn shader_id(self) -> u32 {
        match self {
            Self::Uniform => 0,
            Self::Wipe => 1,
            Self::Radial => 2,
            Self::Plasma => 3,
        }
    }
}

/// A fade seed from a load generation (splitmix64), so the pattern differs per
/// switch and a log line's generation reproduces it.
pub(crate) fn fade_seed(generation: u64) -> u32 {
    let mut z = generation.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (z ^ (z >> 31)) as u32
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Fade {
    frames: u32,
    total: u32,
    pub pattern: BlendPattern,
    pub seed: u32,
}

impl Fade {
    pub(crate) fn new(total: u32, seed: u32) -> Self {
        Self {
            frames: 0,
            total,
            pattern: BlendPattern::pick(seed),
            seed,
        }
    }

    /// How far the incoming side has taken over, eased: 0 at the start, 1 at
    /// the end.
    pub(crate) fn progress(&self) -> f32 {
        if self.total == 0 {
            return 1.0;
        }
        let t = (self.frames as f32 / self.total as f32).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// Count one advance; true once the fade is complete.
    fn advance(&mut self) -> bool {
        self.frames = self.frames.saturating_add(1).min(self.total);
        self.frames >= self.total
    }
}

/// What the blit draws this frame.
#[derive(Debug)]
pub(crate) enum BlitPlan<'a, S> {
    Nothing,
    Solo(&'a S),
    Mix {
        incoming: &'a S,
        outgoing: &'a S,
        progress: f32,
        fade: Fade,
    },
}

/// The renderer on screen (`current`, the incoming side of a fade) and, while
/// a fade runs, the one it replaces.
#[derive(Debug)]
pub(crate) struct SlotPair<S> {
    current: Option<S>,
    fade: Option<(S, Fade)>,
}

impl<S> Default for SlotPair<S> {
    fn default() -> Self {
        Self {
            current: None,
            fade: None,
        }
    }
}

impl<S> SlotPair<S> {
    pub(crate) fn current(&self) -> Option<&S> {
        self.current.as_ref()
    }

    pub(crate) fn is_fading(&self) -> bool {
        self.fade.is_some()
    }

    /// Mutable access to both sides at once: (current, outgoing).
    pub(crate) fn both_mut(&mut self) -> (Option<&mut S>, Option<&mut S>) {
        (
            self.current.as_mut(),
            self.fade.as_mut().map(|(outgoing, _)| outgoing),
        )
    }

    /// The side a new arrival seeds its picture from: the one that dominates
    /// what is on screen (the outgoing until the fade is half done).
    pub(crate) fn seed_source(&self) -> Option<&S> {
        match (&self.current, &self.fade) {
            (_, Some((outgoing, fade))) if fade.progress() < 0.5 => Some(outgoing),
            (Some(current), _) => Some(current),
            (None, Some((outgoing, _))) => Some(outgoing),
            (None, None) => None,
        }
    }

    /// Put `incoming` on screen. The dominant old side becomes the outgoing of
    /// a fade of `total` advances, if it has drawn a frame (`drawn`) and
    /// `total > 0`; everything else is returned for the caller to drop.
    pub(crate) fn arrive(
        &mut self,
        incoming: S,
        total: u32,
        seed: u32,
        drawn: impl Fn(&S) -> bool,
    ) -> Vec<S> {
        let mut dropped = Vec::new();
        let dominant = match (self.current.take(), self.fade.take()) {
            (Some(current), Some((outgoing, fade))) => {
                if fade.progress() < 0.5 {
                    dropped.push(current);
                    Some(outgoing)
                } else {
                    dropped.push(outgoing);
                    Some(current)
                }
            }
            (Some(current), None) => Some(current),
            (None, Some((outgoing, _))) => Some(outgoing),
            (None, None) => None,
        };
        match dominant {
            Some(old) if total > 0 && drawn(&old) => {
                self.fade = Some((old, Fade::new(total, seed)));
            }
            Some(old) => dropped.push(old),
            None => {}
        }
        self.current = Some(incoming);
        dropped
    }

    /// Count one lockstep advance of both sides; returns the outgoing once the
    /// fade has finished.
    pub(crate) fn advance_fade(&mut self) -> Option<S> {
        let (_, fade) = self.fade.as_mut()?;
        if fade.advance() {
            self.fade.take().map(|(outgoing, _)| outgoing)
        } else {
            None
        }
    }

    /// Stop the fade at once (the outgoing was lost, or nobody is watching);
    /// returns the outgoing.
    pub(crate) fn end_fade(&mut self) -> Option<S> {
        self.fade.take().map(|(outgoing, _)| outgoing)
    }

    /// Drop everything (the incoming was lost, or the device changed).
    pub(crate) fn take_all(&mut self) -> Vec<S> {
        let mut out: Vec<S> = self.current.take().into_iter().collect();
        out.extend(self.end_fade());
        out
    }

    /// Drop every side whose generation is below `below`. A fade whose
    /// incoming goes ends with it.
    pub(crate) fn release_below(&mut self, below: u64, generation: impl Fn(&S) -> u64) -> Vec<S> {
        let mut out = Vec::new();
        if self
            .fade
            .as_ref()
            .is_some_and(|(o, _)| generation(o) < below)
        {
            out.extend(self.end_fade());
        }
        if self.current.as_ref().is_some_and(|c| generation(c) < below) {
            out.extend(self.take_all());
        }
        out
    }

    /// What to draw, given each side's rendered frame count. A side with no
    /// frame yet is never drawn.
    pub(crate) fn blit_plan(&self, frames: impl Fn(&S) -> u64) -> BlitPlan<'_, S> {
        let current = self.current.as_ref().filter(|c| frames(c) > 0);
        let outgoing = self.fade.as_ref().filter(|(o, _)| frames(o) > 0);
        match (current, outgoing) {
            (Some(incoming), Some((outgoing, fade))) => BlitPlan::Mix {
                incoming,
                outgoing,
                progress: fade.progress(),
                fade: *fade,
            },
            (Some(only), None) | (None, Some((only, _))) => BlitPlan::Solo(only),
            (None, None) => BlitPlan::Nothing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Slots are (generation, frames).
    type T = (u64, u64);
    fn drawn(s: &T) -> bool {
        s.1 > 0
    }
    fn generation(s: &T) -> u64 {
        s.0
    }
    fn frames(s: &T) -> u64 {
        s.1
    }

    #[test]
    fn fade_frames_counts_advances_and_clamps_to_half_the_interval() {
        assert_eq!(fade_frames(2.0, 30.0), 120);
        assert_eq!(fade_frames(10.0, 5.0), 150, "clamped to 2.5 s");
        assert_eq!(fade_frames(10.0, 0.0), 600, "interval 0 never clamps");
        assert_eq!(fade_frames(0.0, 30.0), 0);
        assert_eq!(fade_frames(-1.0, 30.0), 0);
        assert_eq!(fade_frames(f32::NAN, 30.0), 0);
        assert_eq!(fade_frames(2.0, f32::NAN), 120);
    }

    #[test]
    fn progress_is_eased_monotone_from_zero_to_one() {
        let mut fade = Fade::new(10, 0);
        assert_eq!(fade.progress(), 0.0);
        let mut last = 0.0;
        for step in 1..=10 {
            let done = fade.advance();
            assert!(fade.progress() >= last);
            last = fade.progress();
            assert_eq!(done, step == 10);
        }
        assert_eq!(fade.progress(), 1.0);
        assert!(fade.advance(), "stays finished");
        assert_eq!(Fade::new(0, 0).progress(), 1.0);
    }

    #[test]
    fn pick_covers_every_pattern() {
        let seen: std::collections::HashSet<_> = (0..64)
            .map(|s| BlendPattern::pick(fade_seed(s)))
            .map(BlendPattern::shader_id)
            .collect();
        assert_eq!(seen.len(), BlendPattern::ALL.len());
    }

    #[test]
    fn first_arrival_fades_nothing() {
        let mut pair = SlotPair::default();
        assert!(pair.arrive((1, 0), 120, 7, drawn).is_empty());
        assert!(!pair.is_fading());
        assert_eq!(pair.current(), Some(&(1, 0)));
    }

    #[test]
    fn arrival_over_an_undrawn_slot_cuts() {
        let mut pair = SlotPair::default();
        pair.arrive((1, 0), 120, 7, drawn);
        assert_eq!(pair.arrive((2, 0), 120, 7, drawn), vec![(1, 0)]);
        assert!(!pair.is_fading());
    }

    #[test]
    fn arrival_over_a_drawn_slot_fades_from_it() {
        let mut pair = SlotPair::default();
        pair.arrive((1, 5), 120, 7, drawn);
        assert!(pair.arrive((2, 0), 120, 7, drawn).is_empty());
        assert!(pair.is_fading());
        assert_eq!(
            pair.seed_source(),
            Some(&(1, 5)),
            "the outgoing dominates at 0"
        );
    }

    #[test]
    fn zero_frame_fade_cuts_also_mid_fade() {
        let mut pair = SlotPair::default();
        pair.arrive((1, 5), 0, 7, drawn);
        assert_eq!(pair.arrive((2, 5), 0, 7, drawn), vec![(1, 5)]);
        assert!(!pair.is_fading());

        let mut pair = SlotPair::default();
        pair.arrive((1, 5), 120, 7, drawn);
        pair.arrive((2, 5), 120, 7, drawn);
        let mut dropped = pair.arrive((3, 0), 0, 7, drawn);
        dropped.sort_unstable();
        assert_eq!(dropped, vec![(1, 5), (2, 5)], "both old slots go");
        assert!(!pair.is_fading());
        assert_eq!(pair.current(), Some(&(3, 0)));
    }

    #[test]
    fn arrival_mid_fade_keeps_the_dominant_side() {
        // Before halfway: the outgoing dominates and stays.
        let mut pair = SlotPair::default();
        pair.arrive((1, 5), 10, 7, drawn);
        pair.arrive((2, 5), 10, 7, drawn);
        pair.advance_fade();
        assert_eq!(pair.seed_source(), Some(&(1, 5)));
        assert_eq!(pair.arrive((3, 0), 10, 7, drawn), vec![(2, 5)]);
        assert_eq!(pair.both_mut(), (Some(&mut (3, 0)), Some(&mut (1, 5))));

        // From halfway: the incoming dominates and becomes the outgoing.
        let mut pair = SlotPair::default();
        pair.arrive((1, 5), 10, 7, drawn);
        pair.arrive((2, 5), 10, 7, drawn);
        for _ in 0..5 {
            pair.advance_fade();
        }
        assert_eq!(pair.seed_source(), Some(&(2, 5)));
        assert_eq!(pair.arrive((3, 0), 10, 7, drawn), vec![(1, 5)]);
        assert_eq!(pair.both_mut(), (Some(&mut (3, 0)), Some(&mut (2, 5))));
    }

    #[test]
    fn exactly_total_advances_finish_and_return_the_outgoing() {
        let mut pair = SlotPair::default();
        pair.arrive((1, 5), 3, 7, drawn);
        pair.arrive((2, 5), 3, 7, drawn);
        assert_eq!(pair.advance_fade(), None);
        assert_eq!(pair.advance_fade(), None);
        assert_eq!(pair.advance_fade(), Some((1, 5)));
        assert!(!pair.is_fading());
        assert_eq!(pair.advance_fade(), None);
    }

    #[test]
    fn release_below_drops_both() {
        let mut pair = SlotPair::default();
        pair.arrive((1, 5), 10, 7, drawn);
        pair.arrive((2, 5), 10, 7, drawn);
        assert_eq!(
            pair.release_below(2, generation),
            vec![(1, 5)],
            "only the older"
        );
        assert_eq!(pair.current(), Some(&(2, 5)));

        pair.arrive((3, 5), 10, 7, drawn);
        let mut dropped = pair.release_below(4, generation);
        dropped.sort_unstable();
        assert_eq!(dropped, vec![(2, 5), (3, 5)]);
        assert!(pair.current().is_none() && !pair.is_fading());
    }

    #[test]
    fn losing_a_side() {
        let mut pair = SlotPair::default();
        pair.arrive((1, 5), 10, 7, drawn);
        pair.arrive((2, 5), 10, 7, drawn);
        assert_eq!(pair.end_fade(), Some((1, 5)), "a lost outgoing");
        assert_eq!(pair.current(), Some(&(2, 5)));

        pair.arrive((3, 5), 10, 7, drawn);
        let mut dropped = pair.take_all();
        dropped.sort_unstable();
        assert_eq!(dropped, vec![(2, 5), (3, 5)], "a lost incoming takes both");
    }

    #[test]
    fn blit_plan_never_draws_a_side_without_frames() {
        let mut pair = SlotPair::default();
        assert!(matches!(pair.blit_plan(frames), BlitPlan::Nothing));
        pair.arrive((1, 0), 10, 7, drawn);
        assert!(matches!(pair.blit_plan(frames), BlitPlan::Nothing));

        let mut pair = SlotPair::default();
        pair.arrive((1, 5), 10, 7, drawn);
        pair.arrive((2, 0), 10, 7, drawn);
        assert!(matches!(pair.blit_plan(frames), BlitPlan::Solo(&(1, 5))));
        if let (Some(incoming), _) = pair.both_mut() {
            incoming.1 = 1;
        }
        let BlitPlan::Mix {
            incoming,
            outgoing,
            progress,
            fade,
        } = pair.blit_plan(frames)
        else {
            panic!("both sides have frames: a mix");
        };
        assert_eq!((incoming, outgoing), (&(2, 1), &(1, 5)));
        assert_eq!(progress, 0.0);
        assert_eq!(fade.seed, 7);
    }
}
