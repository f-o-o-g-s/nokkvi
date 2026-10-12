//! A track's loudness envelope, drawn by the Waveform seek bar style.
//!
//! Navidrome serves no waveform data, so nokkvi works it out from the audio
//! once per song: [`WaveformBuilder`] folds decoded samples into short time
//! windows as they stream past (the whole PCM is never held), and
//! [`WaveformBuilder::finish`] resamples those windows into
//! [`WAVEFORM_BINS`] evenly spaced columns. Working in time windows and
//! resampling at the end keeps a column in the right place even when the
//! song's duration metadata is slightly off.

/// Columns a stored waveform keeps, start to end. The seek bar resamples them
/// to however many bars fit its width.
pub const WAVEFORM_BINS: usize = 1024;

/// Analysis windows per second of audio. 50 Hz (20 ms) is far finer than any
/// bar the seek bar draws, and keeps a three-hour mix to ~4 MB of windows.
const WINDOWS_PER_SEC: u32 = 50;

/// Quietest column height, as a fraction of the bar, so near-silence still
/// draws a visible sliver instead of a gap.
const MIN_COLUMN: f32 = 0.06;

/// A track's envelope: per-column peak and RMS amplitude, each quantized to
/// `0..=255` of full scale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waveform {
    peaks: Vec<u8>,
    rms: Vec<u8>,
}

impl Waveform {
    /// A waveform from its two columns. `None` when they are empty or differ
    /// in length (a corrupt cache file, say).
    pub fn new(peaks: Vec<u8>, rms: Vec<u8>) -> Option<Self> {
        (!peaks.is_empty() && peaks.len() == rms.len()).then_some(Self { peaks, rms })
    }

    /// Per-column peak amplitude, start to end.
    pub fn peaks(&self) -> &[u8] {
        &self.peaks
    }

    /// Per-column RMS amplitude, start to end.
    pub fn rms(&self) -> &[u8] {
        &self.rms
    }

    /// Bar heights in `MIN_COLUMN..=1.0` for `n` evenly spaced bars, scaled to
    /// the loudest bar. Drawn from RMS rather than peaks: a modern master
    /// peaks near full scale almost everywhere, so a peak envelope reads as a
    /// flat brick, while RMS keeps verse / chorus / breakdown apart.
    pub fn columns(&self, n: usize) -> Vec<f32> {
        if n == 0 {
            return Vec::new();
        }
        let bins = self.rms.len();
        let raw: Vec<u8> = (0..n)
            .map(|i| {
                let start = i * bins / n;
                let end = ((i + 1) * bins / n).max(start + 1).min(bins);
                self.rms[start..end].iter().copied().max().unwrap_or(0)
            })
            .collect();
        let loudest = raw.iter().copied().max().unwrap_or(0);
        if loudest == 0 {
            return vec![MIN_COLUMN; n];
        }
        raw.into_iter()
            .map(|v| (f32::from(v) / f32::from(loudest)).max(MIN_COLUMN))
            .collect()
    }
}

/// Folds interleaved samples into the per-window envelope as they decode.
#[derive(Debug)]
pub struct WaveformBuilder {
    channels: usize,
    window_frames: u64,
    /// Frames folded into the open window so far.
    frames_in_window: u64,
    window_peak: f32,
    window_sum_sq: f64,
    /// Closed windows: `(peak, mean square)`.
    windows: Vec<(f32, f32)>,
    frames: u64,
    sample_rate: u32,
}

impl WaveformBuilder {
    /// A builder for audio at `sample_rate` with `channels` interleaved
    /// channels. `None` for a zero rate or channel count.
    pub fn new(sample_rate: u32, channels: usize) -> Option<Self> {
        if sample_rate == 0 || channels == 0 {
            return None;
        }
        Some(Self {
            channels,
            window_frames: u64::from((sample_rate / WINDOWS_PER_SEC).max(1)),
            frames_in_window: 0,
            window_peak: 0.0,
            window_sum_sq: 0.0,
            windows: Vec::new(),
            frames: 0,
            sample_rate,
        })
    }

    /// Fold a buffer of interleaved samples in `-1.0..=1.0`. A trailing
    /// partial frame is ignored.
    pub fn push_interleaved(&mut self, samples: &[f32]) {
        for frame in samples.chunks_exact(self.channels) {
            let mut sum_sq = 0.0_f64;
            for &s in frame {
                let a = s.abs();
                if a > self.window_peak {
                    self.window_peak = a;
                }
                sum_sq += f64::from(s) * f64::from(s);
            }
            self.window_sum_sq += sum_sq / self.channels as f64;
            self.frames_in_window += 1;
            self.frames += 1;
            if self.frames_in_window == self.window_frames {
                self.close_window();
            }
        }
    }

    fn close_window(&mut self) {
        if self.frames_in_window == 0 {
            return;
        }
        let mean_sq = (self.window_sum_sq / self.frames_in_window as f64) as f32;
        self.windows.push((self.window_peak.min(1.0), mean_sq));
        self.frames_in_window = 0;
        self.window_peak = 0.0;
        self.window_sum_sq = 0.0;
    }

    /// Seconds of audio folded so far.
    pub fn decoded_secs(&self) -> f64 {
        self.frames as f64 / f64::from(self.sample_rate)
    }

    /// Resample the windows into [`WAVEFORM_BINS`] columns. `None` when no
    /// audio was folded.
    pub fn finish(mut self) -> Option<Waveform> {
        self.close_window();
        let windows = &self.windows;
        if windows.is_empty() {
            return None;
        }
        let w = windows.len();
        let quantize = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let (peaks, rms) = (0..WAVEFORM_BINS)
            .map(|b| {
                let start = b * w / WAVEFORM_BINS;
                let end = ((b + 1) * w / WAVEFORM_BINS).max(start + 1).min(w);
                let span = &windows[start..end];
                let peak = span.iter().map(|&(p, _)| p).fold(0.0_f32, f32::max);
                let mean_sq = span.iter().map(|&(_, m)| m).sum::<f32>() / span.len() as f32;
                (quantize(peak), quantize(mean_sq.sqrt()))
            })
            .unzip();
        Waveform::new(peaks, rms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One second of a constant-amplitude stereo signal at `amp`.
    fn second(rate: u32, amp: f32) -> Vec<f32> {
        (0..rate)
            .flat_map(|i| {
                let s = if i % 2 == 0 { amp } else { -amp };
                [s, s]
            })
            .collect()
    }

    #[test]
    fn builder_places_a_loud_half_in_the_second_half_of_the_bins() {
        let rate = 1000;
        let mut b = WaveformBuilder::new(rate, 2).expect("builder");
        b.push_interleaved(&second(rate, 0.1));
        b.push_interleaved(&second(rate, 0.8));
        assert!((b.decoded_secs() - 2.0).abs() < 1e-9);
        let wf = b.finish().expect("waveform");
        assert_eq!(wf.peaks().len(), WAVEFORM_BINS);
        let half = WAVEFORM_BINS / 2;
        assert!(wf.peaks()[..half].iter().all(|&p| p == quantize_test(0.1)));
        assert!(wf.peaks()[half..].iter().all(|&p| p == quantize_test(0.8)));
        // A constant-amplitude square wave's RMS equals its amplitude.
        assert_eq!(wf.rms()[half + 1], quantize_test(0.8));
    }

    fn quantize_test(v: f32) -> u8 {
        (v * 255.0).round() as u8
    }

    #[test]
    fn builder_spreads_a_short_clip_over_every_bin() {
        // 0.1 s at 50 windows/s is five windows; each bin still gets one.
        let mut b = WaveformBuilder::new(1000, 1).expect("builder");
        b.push_interleaved(&vec![0.5; 100]);
        let wf = b.finish().expect("waveform");
        assert!(wf.peaks().iter().all(|&p| p == quantize_test(0.5)));
    }

    #[test]
    fn builder_without_audio_yields_nothing() {
        assert!(
            WaveformBuilder::new(44_100, 2)
                .expect("builder")
                .finish()
                .is_none()
        );
        assert!(WaveformBuilder::new(0, 2).is_none());
        assert!(WaveformBuilder::new(44_100, 0).is_none());
    }

    #[test]
    fn columns_scale_to_the_loudest_bar_and_keep_a_floor() {
        let wf = Waveform::new(vec![255, 255, 255, 255], vec![0, 64, 128, 255]).expect("wf");
        let cols = wf.columns(4);
        assert_eq!(cols[0], MIN_COLUMN);
        assert!((cols[2] - 128.0 / 255.0).abs() < 1e-6);
        assert_eq!(cols[3], 1.0);
        // Fewer bars than bins take each span's loudest bin.
        assert_eq!(wf.columns(2), vec![64.0 / 255.0, 1.0]);
        // More bars than bins repeat bins rather than read past the end.
        assert_eq!(wf.columns(8).len(), 8);
        assert!(wf.columns(0).is_empty());
    }

    #[test]
    fn silent_waveform_draws_the_floor() {
        let wf = Waveform::new(vec![0; 3], vec![0; 3]).expect("wf");
        assert_eq!(wf.columns(3), vec![MIN_COLUMN; 3]);
    }

    #[test]
    fn mismatched_or_empty_columns_are_rejected() {
        assert!(Waveform::new(vec![], vec![]).is_none());
        assert!(Waveform::new(vec![1, 2], vec![1]).is_none());
    }
}
