//! Tunnel (Scope): about the last second of the spectrum spirals down into the
//! cover behind the ring (`shaders/tunnel.wgsl`).
//!
//! A ring is kept every [`ROW_EVERY`] FFT ticks, newest first, [`ROWS`] deep.
//! Each is the spectrum wrapped around the circle: bass at the bottom, treble
//! at the top, mirrored left and right, so a ring closes without a seam. A
//! quiet bin pulls the wall in to [`QUIET`], a loud one pushes it out to the
//! ring's full swing. A beat onset lights the next ring kept, so kicks travel
//! down the tunnel. The live ring itself stays the raw waveform.
//!
//! GPU snapshot (rides the peak buffer, which Scope never reads otherwise):
//! `[rows, samples, phase, _]`, then [`ROWS`] kick strengths (newest first),
//! then the rings flattened, newest first, [`RING_SAMPLES`] values each.

use std::{collections::VecDeque, f32::consts::PI};

use super::onset::BeatTrigger;

/// Rings kept (the oldest is about `ROWS * ROW_EVERY / 60` seconds back).
/// Mirrored by `ROWS_MAX` in `tunnel.wgsl`.
pub(crate) const ROWS: usize = 20;
/// FFT ticks between rings.
pub(crate) const ROW_EVERY: u32 = 3;
/// Values per ring around the circle.
pub(crate) const RING_SAMPLES: usize = 64;
/// Header floats before the kick strengths. Mirrored by `HEADER` in
/// `tunnel.wgsl`.
pub(crate) const HEADER: usize = 4;
/// The whole snapshot at full depth.
pub(crate) const GPU_LEN: usize = HEADER + ROWS + ROWS * RING_SAMPLES;
// The snapshot rides the peak buffer (`MAX_BARS` floats).
const _: () = assert!(GPU_LEN <= super::shader::VisualizerPipeline::MAX_BARS);

/// A silent bin's ring value (-1 = the ring's inner swing, 1 = its outer).
pub(crate) const QUIET: f32 = -0.4;

/// A kick lights its ring at least this bright...
const KICK_BASE: f32 = 0.6;
/// ...plus this much per unit of bass.
const KICK_BASS: f32 = 0.8;

pub(crate) struct TunnelRings {
    rows: VecDeque<([f32; RING_SAMPLES], f32)>,
    since: u32,
    onset: BeatTrigger,
    pending_kick: f32,
    gpu: Vec<f32>,
}

impl TunnelRings {
    pub(crate) fn new() -> Self {
        Self {
            rows: VecDeque::with_capacity(ROWS + 1),
            since: 0,
            onset: BeatTrigger::new(),
            pending_kick: 0.0,
            gpu: Vec::with_capacity(GPU_LEN),
        }
    }

    /// Drop the history (a track change, or the Tunnel switched off): the
    /// next tick starts over.
    pub(crate) fn clear(&mut self) {
        self.rows.clear();
        self.since = 0;
        self.onset.reset();
        self.pending_kick = 0.0;
        self.gpu.clear();
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The GPU snapshot (see the module docs).
    pub(crate) fn gpu_data(&self) -> &[f32] {
        &self.gpu
    }

    /// One FFT tick with this tick's spectrum (0..1), the beat pulse (0..1)
    /// and the bass level (a heavier kick lights its ring brighter).
    pub(crate) fn update(&mut self, spectrum: &[f64], beat: f32, bass: f32) {
        if self.onset.fire(beat) {
            self.pending_kick = self
                .pending_kick
                .max((KICK_BASE + KICK_BASS * bass).min(1.0));
        }
        if spectrum.is_empty() {
            return;
        }
        self.since += 1;
        if self.since >= ROW_EVERY || self.rows.is_empty() {
            self.since = 0;
            self.rows.push_front((ring(spectrum), self.pending_kick));
            self.pending_kick = 0.0;
            self.rows.truncate(ROWS);
        }
        let phase = self.since as f32 / ROW_EVERY as f32;
        self.gpu.clear();
        self.gpu
            .extend_from_slice(&[self.rows.len() as f32, RING_SAMPLES as f32, phase, 0.0]);
        self.gpu
            .extend((0..ROWS).map(|i| self.rows.get(i).map_or(0.0, |r| r.1)));
        for (values, _) in &self.rows {
            self.gpu.extend_from_slice(values);
        }
    }
}

/// The spectrum wrapped around the circle (sample `j` at angle
/// `j / RING_SAMPLES` of a turn, clockwise from the right as the ring is
/// drawn, y down): bass at the bottom, treble at the top, mirrored.
fn ring(spectrum: &[f64]) -> [f32; RING_SAMPLES] {
    let last = spectrum.len().saturating_sub(1);
    let mut out = [QUIET; RING_SAMPLES];
    for (j, v) in out.iter_mut().enumerate() {
        let angle = j as f32 / RING_SAMPLES as f32 * 2.0 * PI;
        // Angular distance from the bottom (pi / 2 in y-down screen space).
        let mut from_bottom = (angle - PI / 2.0).abs();
        if from_bottom > PI {
            from_bottom = 2.0 * PI - from_bottom;
        }
        let pos = from_bottom / PI * last as f32;
        let i0 = (pos.floor() as usize).min(last);
        let i1 = (i0 + 1).min(last);
        let t = pos - pos.floor();
        let mag = spectrum[i0] as f32 * (1.0 - t) + spectrum[i1] as f32 * t;
        *v = QUIET + mag.clamp(0.0, 1.0) * (1.0 - QUIET);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTTOM: usize = RING_SAMPLES / 4;
    const TOP: usize = RING_SAMPLES * 3 / 4;
    const RIGHT: usize = 0;
    const LEFT: usize = RING_SAMPLES / 2;

    fn bass_only(n: usize) -> Vec<f64> {
        let mut s = vec![0.0; n];
        s[0] = 1.0;
        s
    }

    #[test]
    fn a_ring_puts_bass_at_the_bottom_and_treble_at_the_top() {
        let r = ring(&bass_only(16));
        assert!((r[BOTTOM] - 1.0).abs() < 1e-6, "bass pushes the bottom out");
        assert!((r[TOP] - QUIET).abs() < 1e-6, "silent treble at the top");

        let mut treble = vec![0.0; 16];
        treble[15] = 1.0;
        let r = ring(&treble);
        // The top lands a hair under the last bin in f32 (14.99999 of 15).
        assert!((r[TOP] - 1.0).abs() < 1e-4, "treble pushes the top out");
        assert!((r[BOTTOM] - QUIET).abs() < 1e-6);
    }

    #[test]
    fn a_ring_is_mirrored_left_to_right_so_it_closes_without_a_seam() {
        let spectrum: Vec<f64> = (0..16).map(|i| (i as f64 * 0.37).sin().abs()).collect();
        let r = ring(&spectrum);
        for j in 0..RING_SAMPLES {
            let mirror = (RING_SAMPLES + RING_SAMPLES / 2 - j) % RING_SAMPLES;
            assert!(
                (r[j] - r[mirror]).abs() < 1e-5,
                "sample {j} vs its mirror {mirror}"
            );
        }
        // The sides sit halfway up the spectrum.
        assert!((r[RIGHT] - r[LEFT]).abs() < 1e-5);
    }

    #[test]
    fn a_ring_spans_quiet_to_full_swing() {
        assert!(ring(&[0.0; 8]).iter().all(|&v| (v - QUIET).abs() < 1e-6));
        assert!(ring(&[1.0; 8]).iter().all(|&v| (v - 1.0).abs() < 1e-6));
        assert!(
            ring(&[3.0; 8]).iter().all(|&v| (v - 1.0).abs() < 1e-6),
            "an overshooting bin clamps to the full swing"
        );
        assert!(
            ring(&[0.5])
                .iter()
                .all(|&v| (v - (QUIET + 0.5 * (1.0 - QUIET))).abs() < 1e-6),
            "a one-bin spectrum is a circle"
        );
    }

    #[test]
    fn rings_are_kept_every_few_ticks_newest_first_and_capped() {
        let mut t = TunnelRings::new();
        t.update(&bass_only(8), 0.0, 0.0);
        assert_eq!(t.gpu_data()[0], 1.0, "the first tick keeps a ring at once");
        for _ in 1..ROW_EVERY {
            t.update(&[0.0; 8], 0.0, 0.0);
        }
        assert_eq!(t.gpu_data()[0], 1.0, "no new ring until ROW_EVERY ticks");
        t.update(&[0.0; 8], 0.0, 0.0);
        assert_eq!(t.gpu_data()[0], 2.0);
        let ring0 = HEADER + ROWS;
        assert!(
            (t.gpu_data()[ring0 + BOTTOM] - QUIET).abs() < 1e-6,
            "the newest (quiet) ring comes first"
        );
        assert!(
            (t.gpu_data()[ring0 + RING_SAMPLES + BOTTOM] - 1.0).abs() < 1e-6,
            "the older bass ring behind it"
        );
        for _ in 0..ROWS as u32 * ROW_EVERY * 2 {
            t.update(&[0.2; 8], 0.0, 0.0);
        }
        assert_eq!(t.gpu_data()[0], ROWS as f32);
        assert_eq!(t.gpu_data().len(), GPU_LEN);
    }

    #[test]
    fn the_header_carries_rows_samples_and_the_scroll_phase() {
        let mut t = TunnelRings::new();
        t.update(&[0.1; 4], 0.0, 0.0);
        assert_eq!(
            &t.gpu_data()[..HEADER],
            &[1.0, RING_SAMPLES as f32, 0.0, 0.0]
        );
        t.update(&[0.1; 4], 0.0, 0.0);
        let phase = t.gpu_data()[2];
        assert!(
            (phase - 1.0 / ROW_EVERY as f32).abs() < 1e-6,
            "phase {phase}"
        );
    }

    #[test]
    fn a_beat_onset_lights_the_next_ring_kept_once() {
        let mut t = TunnelRings::new();
        t.update(&[0.1; 4], 0.0, 0.0);
        // The kick lands between rings; the next ring kept carries it.
        t.update(&[0.1; 4], 0.9, 0.5);
        t.update(&[0.1; 4], 0.8, 0.5);
        t.update(&[0.1; 4], 0.8, 0.5);
        let kick0 = t.gpu_data()[HEADER];
        assert!(
            (kick0 - 1.0).abs() < 1e-6,
            "0.6 + 0.8 * 0.5 = 1.0, got {kick0}"
        );
        assert_eq!(
            t.gpu_data()[HEADER + 1],
            0.0,
            "the ring before the kick is dark"
        );
        // The pulse stays high: the same kick lights nothing more.
        for _ in 0..ROW_EVERY {
            t.update(&[0.1; 4], 0.8, 0.5);
        }
        assert_eq!(t.gpu_data()[HEADER], 0.0);
        assert!(t.gpu_data()[HEADER + 1] > 0.0, "the lit ring falls back");
    }

    /// tunnel.wgsl indexes the snapshot with its own copies of the layout.
    #[test]
    fn the_shader_reads_the_same_snapshot_layout() {
        const WGSL: &str = include_str!("shaders/tunnel.wgsl");
        assert!(WGSL.contains(&format!("const ROWS_MAX: u32 = {ROWS}u;")));
        assert!(WGSL.contains(&format!("const HEADER: u32 = {HEADER}u;")));
    }

    #[test]
    fn clear_and_an_empty_spectrum() {
        let mut t = TunnelRings::new();
        t.update(&[], 0.0, 0.0);
        assert!(t.is_empty(), "no spectrum, no ring");
        assert!(t.gpu_data().is_empty());
        t.update(&[0.3; 4], 0.0, 0.0);
        assert!(!t.is_empty());
        t.clear();
        assert!(t.is_empty());
        assert!(t.gpu_data().is_empty());
    }
}
