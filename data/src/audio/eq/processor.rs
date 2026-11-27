//! The per-stream filter chain: runs the band list on every sample.
//!
//! Ported from Pawse's `crates/equalizer/src/processor.rs`. Changed here: it
//! runs sample by sample inside `StreamingSource::next` instead of on decoded
//! batches, reads its bands from the lock-free [`EqState`], keeps its two
//! chains inline (nothing is allocated on the audio thread), and crossfades
//! over a fixed time instead of over one batch.
//!
//! MIT License
//!
//! Copyright (c) 2026 Aleksandr Popov
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all
//! copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//! SOFTWARE.

use super::{
    EQ_MAX_BANDS, EqState,
    band::{EqBand, EqBandKind},
    filter::{Coefficients, preamp_db},
};

/// Channels with their own filter state. A stream with more keeps the
/// preamp on the extra channels and leaves them unfiltered.
const MAX_CHANNELS: usize = 8;
/// Frames between two looks at the shared state (~11 ms at 48 kHz).
const CHECK_INTERVAL_FRAMES: u32 = 512;
/// Length of the crossfade from the old chain to the new one, in seconds
/// (smoothstep-shaped). Switching coefficients under a running filter clicks.
const FADE_SECS: f64 = 0.02;
/// Filter state under this is zeroed: decaying state turns denormal in long
/// silence, which is slow on x86.
const SETTLE_BELOW: f64 = 1e-30;

const NO_BAND: EqBand = EqBand::new(EqBandKind::Peak, 1000.0, 0.0, 1.0);

/// One biquad of a chain, in transposed direct form II with f64 state (f32
/// loses low shelves at high sample rates).
#[derive(Debug, Clone, Copy)]
struct Section {
    /// Index of the band in the band list it was built from.
    band: usize,
    kind: EqBandKind,
    coefficients: Coefficients,
    state: [[f64; 2]; MAX_CHANNELS],
}

const NO_SECTION: Section = Section {
    band: 0,
    kind: EqBandKind::Peak,
    coefficients: Coefficients::IDENTITY,
    state: [[0.0; 2]; MAX_CHANNELS],
};

/// The filters of one band list at one sample rate, plus its preamp.
#[derive(Debug, Clone)]
struct Chain {
    /// The automatic preamp, linear.
    gain: f64,
    sections: [Section; EQ_MAX_BANDS],
    len: usize,
}

impl Chain {
    const EMPTY: Self = Self {
        gain: 1.0,
        sections: [NO_SECTION; EQ_MAX_BANDS],
        len: 0,
    };

    /// Rebuild in place for `bands` at `rate` Hz, with silent filter state.
    fn rebuild(&mut self, bands: &[EqBand], rate: f64) {
        self.len = 0;
        for (index, band) in bands.iter().enumerate().take(EQ_MAX_BANDS) {
            if band.is_flat() {
                continue;
            }
            self.sections[self.len] = Section {
                band: index,
                kind: band.kind,
                coefficients: Coefficients::design(band, rate),
                state: [[0.0; 2]; MAX_CHANNELS],
            };
            self.len += 1;
        }
        self.gain = 10f64.powf(preamp_db(bands, rate) / 20.0);
    }

    /// Whether the chain changes the signal at all.
    fn is_active(&self) -> bool {
        self.len > 0
    }

    /// Take the filter state of `previous`'s section built from the same band
    /// (same index and kind), so an untouched narrow bass band keeps ringing
    /// instead of restarting from zero, which would dip its frequency for
    /// longer than the crossfade.
    fn inherit(&mut self, previous: &Self) {
        for section in &mut self.sections[..self.len] {
            if let Some(old) = previous.sections[..previous.len]
                .iter()
                .find(|old| old.band == section.band && old.kind == section.kind)
            {
                section.state = old.state;
            }
        }
    }

    #[inline]
    fn tick(&mut self, x: f64, channel: usize) -> f64 {
        let mut y = x;
        if channel < MAX_CHANNELS {
            for section in &mut self.sections[..self.len] {
                let c = &section.coefficients;
                let z = &mut section.state[channel];
                let out = c.b0 * y + z[0];
                z[0] = c.b1 * y - c.a1 * out + z[1];
                z[1] = c.b2 * y - c.a2 * out;
                y = out;
            }
        }
        y * self.gain
    }

    /// Zero state that went denormal or not finite: a NaN would otherwise
    /// never leave the filter.
    fn settle(&mut self) {
        for z in self.sections[..self.len]
            .iter_mut()
            .flat_map(|section| section.state.iter_mut().flatten())
        {
            if !z.is_finite() || z.abs() < SETTLE_BELOW {
                *z = 0.0;
            }
        }
    }
}

/// Per-stream equalizer: the playing layout's band list as a biquad chain.
///
/// A flat or empty band list, or a disabled equalizer, means no chain:
/// samples pass through untouched. A change of bands (or of the master
/// switch) crossfades from the old chain, or the dry signal, to the new one
/// over [`FADE_SECS`]; a change that arrives during a crossfade waits for it
/// to end. Never blocks and never allocates.
#[derive(Debug)]
pub struct EqProcessor {
    state: EqState,
    sample_rate: f64,
    channels: usize,
    /// Channel of the next sample within its frame.
    channel: usize,
    /// The chain that plays (`chains[current]`) and the one it replaced,
    /// which a running crossfade still reads.
    chains: [Chain; 2],
    current: usize,
    /// The band list `chains[current]` was built from.
    bands: [EqBand; EQ_MAX_BANDS],
    bands_len: usize,
    /// The shared state's generation at the last look.
    generation: u64,
    /// Whether the first look at the shared state has happened. A stream's
    /// first chain starts without a fade: nothing was playing to fade from.
    primed: bool,
    frames_to_check: u32,
    fade_frames: u32,
    /// Frames left of the running crossfade (0 = none).
    fade_left: u32,
    /// Weight of the new chain in the frame being processed (1 = no fade).
    mix: f64,
}

impl EqProcessor {
    pub fn new(state: EqState, sample_rate: u32, channels: u16) -> Self {
        let sample_rate = f64::from(sample_rate.max(1));
        Self {
            state,
            sample_rate,
            channels: usize::from(channels.max(1)),
            channel: 0,
            chains: [Chain::EMPTY, Chain::EMPTY],
            current: 0,
            bands: [NO_BAND; EQ_MAX_BANDS],
            bands_len: 0,
            generation: 0,
            primed: false,
            frames_to_check: 0,
            fade_frames: ((sample_rate * FADE_SECS).round() as u32).max(1),
            fade_left: 0,
            mix: 1.0,
        }
    }

    /// Process one interleaved sample.
    #[inline]
    pub fn process_sample(&mut self, sample: f32) -> f32 {
        let channel = self.channel;
        if channel == 0 {
            self.begin_frame();
        }
        self.channel = (channel + 1) % self.channels;

        let fading = self.mix < 1.0;
        let (first, second) = self.chains.split_at_mut(1);
        let (now, before) = if self.current == 0 {
            (&mut first[0], &mut second[0])
        } else {
            (&mut second[0], &mut first[0])
        };
        if !fading && !now.is_active() {
            // Nothing to do: the sample stays bit-identical.
            return sample;
        }
        let x = f64::from(sample);
        let wet = if now.is_active() {
            now.tick(x, channel)
        } else {
            x
        };
        if !fading {
            return wet as f32;
        }
        let from = if before.is_active() {
            before.tick(x, channel)
        } else {
            x
        };
        (from + (wet - from) * self.mix) as f32
    }

    /// Advance the crossfade by one frame, or look at the shared state when
    /// the check interval is up.
    fn begin_frame(&mut self) {
        if self.fade_left > 0 {
            self.fade_left -= 1;
            // Smoothstep: the weight leaves 0 and arrives at 1 with no slope,
            // so the fade's ends put no kink into the waveform.
            let t = 1.0 - f64::from(self.fade_left) / f64::from(self.fade_frames);
            self.mix = t * t * (3.0 - 2.0 * t);
            return;
        }
        self.mix = 1.0;
        if self.frames_to_check > 0 {
            self.frames_to_check -= 1;
            return;
        }
        self.frames_to_check = CHECK_INTERVAL_FRAMES - 1;
        self.chains[self.current].settle();
        self.refresh();
    }

    /// Rebuild the chain when the shared state moved to a different band
    /// list, and start the crossfade to it.
    fn refresh(&mut self) {
        let generation = self.state.generation();
        if self.primed && generation == self.generation {
            return;
        }
        // Read the generation before the bands: a write that lands in
        // between moves it again, and the next look rebuilds once more.
        self.generation = generation;
        let mut bands = [NO_BAND; EQ_MAX_BANDS];
        let len = self.state.snapshot_into(&mut bands);
        let first_look = !self.primed;
        self.primed = true;
        if bands[..len] == self.bands[..self.bands_len] {
            return;
        }
        self.bands = bands;
        self.bands_len = len;

        let (first, second) = self.chains.split_at_mut(1);
        let (previous, next) = if self.current == 0 {
            (&first[0], &mut second[0])
        } else {
            (&second[0], &mut first[0])
        };
        next.rebuild(&bands[..len], self.sample_rate);
        next.inherit(previous);
        let audible = previous.is_active() || next.is_active();
        self.current = 1 - self.current;
        if audible && !first_look {
            self.fade_left = self.fade_frames;
            // This frame is the fade's first: still all of the old chain.
            self.mix = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        super::{BUILTIN_PRESETS, EQ_BAND_COUNT, EqMode, Response},
        *,
    };

    const RATE: u32 = 48_000;

    fn sine(freq_hz: f64, frame: usize, rate: u32) -> f32 {
        (std::f64::consts::TAU * freq_hz * frame as f64 / f64::from(rate)).sin() as f32
    }

    /// Run `frames` of a full-scale stereo sine and return the left channel.
    fn run_sine(eq: &mut EqProcessor, freq_hz: f64, rate: u32, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|n| {
                let x = sine(freq_hz, n, rate);
                let left = eq.process_sample(x);
                eq.process_sample(x);
                left
            })
            .collect()
    }

    fn peak_of(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0, |m, s| m.max(s.abs()))
    }

    /// Level of a settled tone in dB re full scale, from its RMS. Exact over
    /// a whole number of half cycles (a sampled peak misses the crest at
    /// frequencies that divide the sample rate).
    fn level_db(samples: &[f32]) -> f64 {
        let power: f64 = samples.iter().map(|&s| f64::from(s).powi(2)).sum();
        10.0 * (2.0 * power / samples.len() as f64).log10()
    }

    /// Longest run of bit-identical consecutive samples above half scale: a
    /// sine never repeats a value three times, a clipped one sits on the rail.
    fn longest_flat_run(samples: &[f32]) -> usize {
        let mut longest = 1;
        let mut run = 1;
        for pair in samples.windows(2) {
            if pair[0] == pair[1] && pair[0].abs() > 0.5 {
                run += 1;
                longest = longest.max(run);
            } else {
                run = 1;
            }
        }
        longest
    }

    /// Largest jump between two neighbouring samples.
    fn largest_step(samples: &[f32]) -> f32 {
        samples
            .windows(2)
            .fold(0.0, |m, pair| m.max((pair[1] - pair[0]).abs()))
    }

    fn graphic_state(gains: &[f32; EQ_BAND_COUNT]) -> EqState {
        let state = EqState::new();
        state.set_all_gains(gains);
        state.set_enabled(true);
        state
    }

    /// An AutoEq-style correction: a low shelf, peaks, a high shelf.
    fn autoeq_profile() -> Vec<EqBand> {
        vec![
            EqBand::new(EqBandKind::LowShelf, 105.0, 5.5, 0.7),
            EqBand::new(EqBandKind::Peak, 150.0, -3.2, 0.52),
            EqBand::new(EqBandKind::Peak, 1300.0, -1.8, 1.4),
            EqBand::new(EqBandKind::Peak, 3200.0, 4.1, 2.2),
            EqBand::new(EqBandKind::Peak, 5800.0, -5.6, 4.0),
            EqBand::new(EqBandKind::HighShelf, 10_000.0, -2.5, 0.7),
        ]
    }

    fn parametric_state(bands: &[EqBand]) -> EqState {
        let state = EqState::new();
        state.set_parametric_bands(bands);
        state.set_mode(EqMode::Parametric);
        state.set_enabled(true);
        state
    }

    /// A boosting preset must leave a full-scale bass tone unclipped: the
    /// preamp takes the response peak off, so the loudest tone comes out at
    /// full scale at most, and nothing is flattened against a clamp.
    #[test]
    fn boosting_presets_do_not_clip_a_full_scale_bass_sine() {
        for name in ["Hip-Hop", "Bass Boost"] {
            let preset = BUILTIN_PRESETS
                .iter()
                .find(|p| p.name == name)
                .expect("built-in preset");
            for rate in [44_100, 48_000, 96_000] {
                let mut eq = EqProcessor::new(graphic_state(&preset.gains), rate, 2);
                let frames = rate as usize * 2;
                let out = run_sine(&mut eq, 50.0, rate, frames).split_off(frames / 4);
                let peak = peak_of(&out);
                assert!(
                    peak <= 1.0,
                    "{name} at {rate} Hz: a full-scale 50 Hz sine peaks at {peak}"
                );
                assert!(peak > 0.5, "{name} at {rate} Hz: the tone is still there");
                let flat = longest_flat_run(&out);
                assert!(
                    flat < 3,
                    "{name} at {rate} Hz: {flat} identical samples in a row (flat-topped)"
                );
            }
        }
    }

    /// A flat or empty band list processes nothing: every sample comes out
    /// bit-identical, whether the equalizer is off, on with flat sliders, on
    /// with an empty band list, or on with every band switched off.
    #[test]
    fn a_flat_or_empty_list_leaves_samples_untouched() {
        let off = EqState::new();
        off.set_all_gains(&[6.0; EQ_BAND_COUNT]);
        let flat_sliders = graphic_state(&[0.0; EQ_BAND_COUNT]);
        let empty = parametric_state(&[]);
        let all_off: Vec<EqBand> = autoeq_profile()
            .into_iter()
            .map(|band| EqBand {
                enabled: false,
                ..band
            })
            .collect();
        for state in [off, flat_sliders, empty, parametric_state(&all_off)] {
            let mut eq = EqProcessor::new(state, RATE, 2);
            for n in 0..4096 {
                let x = sine(997.0, n, RATE) * 0.999_999;
                assert_eq!(eq.process_sample(x).to_bits(), x.to_bits());
                assert_eq!(eq.process_sample(-x).to_bits(), (-x).to_bits());
            }
        }
    }

    /// The chain plays the curve the response function draws: a tone at any
    /// frequency settles at the designed gain plus the preamp.
    #[test]
    fn the_measured_response_matches_the_designed_curve() {
        let bands = autoeq_profile();
        for rate in [44_100, 96_000] {
            let response = Response::new(&bands, f64::from(rate));
            let preamp = preamp_db(&bands, f64::from(rate));
            assert!(preamp < -1.0, "the profile boosts, so the preamp cuts");
            for freq in [30.0, 105.0, 150.0, 1000.0, 3200.0, 5800.0, 12_000.0] {
                let mut eq = EqProcessor::new(parametric_state(&bands), rate, 2);
                let frames = rate as usize;
                let out = run_sine(&mut eq, freq, rate, frames).split_off(frames / 2);
                let measured = level_db(&out);
                let designed = response.db(freq) + preamp;
                assert!(
                    (measured - designed).abs() < 0.01,
                    "{freq} Hz at {rate}: measured {measured:.3} dB, designed {designed:.3} dB"
                );
            }
        }
    }

    /// A band change during playback crossfades: the output never jumps by
    /// more than a tone of that level moves between two samples anyway.
    #[test]
    fn a_band_change_during_playback_makes_no_step() {
        let state = graphic_state(&[0.0; EQ_BAND_COUNT]);
        let mut eq = EqProcessor::new(state.clone(), RATE, 2);
        let mut out = run_sine(&mut eq, 100.0, RATE, 4800);
        // Every kind of change in turn: flat to a boost, a preset, the layout,
        // the master switch off and on again.
        let changes: [&dyn Fn(); 6] = [
            &|| state.set_band_gain(1, 12.0),
            &|| state.set_all_gains(&[-12.0; EQ_BAND_COUNT]),
            &|| {
                state.set_parametric_bands(&autoeq_profile());
                state.set_mode(EqMode::Parametric);
            },
            &|| state.set_enabled(false),
            &|| state.set_enabled(true),
            &|| state.set_mode(EqMode::Graphic),
        ];
        let mut frame = 4800;
        for change in changes {
            change();
            for _ in 0..4800 {
                let x = sine(100.0, frame, RATE);
                out.push(eq.process_sample(x));
                eq.process_sample(x);
                frame += 1;
            }
        }
        // A full-scale 100 Hz sine moves 2*pi*100/48000 = 0.0131 per sample.
        // A hard switch between these curves steps by up to ~0.7.
        let step = largest_step(&out);
        assert!(step < 0.03, "largest step {step}");
        // No kink either: the curvature stays near a 100 Hz tone's own
        // (1.7e-4 at full scale). A linear fade's ends reach ~2e-3.
        let kink = out
            .windows(3)
            .fold(0.0f32, |m, w| m.max((w[2] - 2.0 * w[1] + w[0]).abs()));
        assert!(kink < 6e-4, "largest second difference {kink}");
        assert!(peak_of(&out) <= 1.0 + 1e-3);
    }

    /// A change that lands while a crossfade runs is taken once it ends, so
    /// a drag (a change every few milliseconds) ends on the last value.
    #[test]
    fn rapid_changes_end_on_the_last_one() {
        let state = graphic_state(&[0.0; EQ_BAND_COUNT]);
        let mut eq = EqProcessor::new(state.clone(), RATE, 2);
        let mut out = Vec::new();
        let mut frame = 0;
        for step in 0..=48 {
            state.set_band_gain(5, -(step as f32) * 0.25);
            for _ in 0..100 {
                let x = sine(1000.0, frame, RATE);
                out.push(eq.process_sample(x));
                eq.process_sample(x);
                frame += 1;
            }
        }
        // 1 kHz moves 0.131 per sample at full scale; no jump beyond that.
        assert!(largest_step(&out) < 0.14, "{}", largest_step(&out));
        let tail = run_sine(&mut eq, 1000.0, RATE, 9600).split_off(4800);
        let measured = level_db(&tail);
        assert!(
            (measured + 12.0).abs() < 0.01,
            "ends at -12 dB, got {measured}"
        );
    }

    /// A band that did not change keeps its filter state across a change of
    /// another band; a changed or new band starts from silence.
    #[test]
    fn an_unchanged_band_keeps_its_state_across_a_rebuild() {
        let mut bands = autoeq_profile();
        let mut old = Chain::EMPTY;
        old.rebuild(&bands, 48_000.0);
        for n in 0..256 {
            old.tick(f64::from(sine(60.0, n, RATE)), 0);
        }
        assert!(old.sections[0].state[0][0].abs() > 1e-6);

        bands[3].gain = -9.0;
        bands[1].kind = EqBandKind::HighPass;
        let mut new = Chain::EMPTY;
        new.rebuild(&bands, 48_000.0);
        new.inherit(&old);
        assert_eq!(new.sections[0].state, old.sections[0].state, "untouched");
        assert_eq!(
            new.sections[3].state, old.sections[3].state,
            "same band and kind"
        );
        assert_eq!(
            new.sections[1].state, [[0.0; 2]; MAX_CHANNELS],
            "a new kind"
        );
    }

    /// Channels are filtered apart: a tone on one never leaks into another,
    /// on a stereo stream and on a 5.1 one.
    #[test]
    fn channels_do_not_leak_into_each_other() {
        let preset = &BUILTIN_PRESETS[0].gains;
        for (channels, loud) in [(2u16, 1usize), (6, 5)] {
            let mut eq = EqProcessor::new(graphic_state(preset), RATE, channels);
            for n in 0..4800 {
                for channel in 0..usize::from(channels) {
                    let x = if channel == loud {
                        sine(60.0, n, RATE)
                    } else {
                        0.0
                    };
                    let y = eq.process_sample(x);
                    if channel != loud {
                        assert_eq!(y, 0.0, "channel {channel} of {channels}");
                    }
                }
            }
        }
    }

    /// A stream with more channels than the chain has state for keeps
    /// playing: the extra channels take the preamp only.
    #[test]
    fn channels_past_the_state_limit_take_the_preamp_only() {
        let preset = &BUILTIN_PRESETS[0].gains;
        let gain = 10f64
            .powf(preamp_db(&super::super::graphic_bands(preset), f64::from(RATE)) / 20.0)
            as f32;
        let mut eq = EqProcessor::new(graphic_state(preset), RATE, 10);
        for _ in 0..64 {
            for channel in 0..10 {
                let y = eq.process_sample(0.5);
                if channel >= MAX_CHANNELS {
                    assert!((y - 0.5 * gain).abs() < 1e-6);
                }
            }
        }
    }

    /// A NaN in the input leaves the filters again, and long silence leaves
    /// exact zeros behind instead of denormals.
    #[test]
    fn nan_and_denormal_state_is_flushed() {
        let mut eq = EqProcessor::new(graphic_state(&BUILTIN_PRESETS[0].gains), RATE, 1);
        run_sine_mono(&mut eq, 2048);
        eq.process_sample(f32::NAN);
        for _ in 0..(CHECK_INTERVAL_FRAMES * 2) {
            eq.process_sample(0.0);
        }
        let out = run_sine_mono(&mut eq, 4096);
        assert!(out.iter().all(|s| s.is_finite()), "the NaN left the chain");

        for _ in 0..(RATE * 20) {
            eq.process_sample(0.0);
        }
        let chain = &eq.chains[eq.current];
        assert!(
            chain.sections[..chain.len]
                .iter()
                .all(|section| section.state[0] == [0.0, 0.0]),
            "silence settles to exact zero"
        );
    }

    fn run_sine_mono(eq: &mut EqProcessor, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|n| eq.process_sample(sine(200.0, n, RATE)))
            .collect()
    }

    /// The Graphic layout is a layout of the same engine: the sliders play
    /// exactly what the same ten peaks play as a Parametric list.
    #[test]
    fn the_graphic_layout_plays_like_the_same_bands_as_a_list() {
        let gains = BUILTIN_PRESETS[2].gains;
        let mut graphic = EqProcessor::new(graphic_state(&gains), RATE, 2);
        let list = super::super::graphic_bands(&gains);
        let mut parametric = EqProcessor::new(parametric_state(&list), RATE, 2);
        let a = run_sine(&mut graphic, 440.0, RATE, 4800);
        let b = run_sine(&mut parametric, 440.0, RATE, 4800);
        assert_eq!(a, b);
    }

    #[test]
    fn processors_build_at_every_common_rate() {
        for rate in [
            8_000, 22_050, 32_000, 44_100, 48_000, 96_000, 192_000, 384_000,
        ] {
            let mut eq = EqProcessor::new(graphic_state(&[6.0; EQ_BAND_COUNT]), rate, 2);
            let out = run_sine(&mut eq, 1000.0, rate, 4096);
            assert!(out.iter().all(|s| s.is_finite()), "{rate}");
        }
    }
}
