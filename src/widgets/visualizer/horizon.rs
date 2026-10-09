//! Horizon (Bars / Lines): about the last second and a half of the spectrum
//! recedes behind the live visualizer toward a horizon (`shaders/horizon.wgsl`).
//!
//! A row is kept every [`ROW_EVERY`] FFT ticks, newest first, [`ROWS`] deep.
//! Bars keeps stepped rows (one height per bar, bars grouped by max past
//! [`MAX_ROW_BARS`]) that the shader draws as receding rows of bars; Lines
//! keeps smooth rows ([`SMOOTH_SAMPLES`] heights along a Catmull-Rom through
//! the points) drawn as misty waves. Either way each row runs [`MARGIN`] of
//! the field past both edges, the spectrum mirrored outward and easing down,
//! so the near rows run off-screen and only the far, narrower ones show ends.
//!
//! GPU snapshot (it rides the particle storage buffer, which only Scope's dust
//! uses otherwise): entry 0 = `(rows, samples, phase, margin, core, group,
//! stepped, lift)`, then the rows flattened, 8 heights to an entry. `margin` is
//! a field fraction for smooth rows and a bar count for stepped ones; `lift`
//! is the canvas headroom above the band (px), filled in by the primitive.

use std::collections::VecDeque;

use super::state::catmull_rom_1d;

/// Canvas above the band the rows may crest into, as a share of the scene
/// height: the far rows' baselines climb to `RIDGE_HORIZON` and their crests
/// stand on top, so the farthest loud crest reaches about 1.14 scenes
/// (`headroom_holds_the_farthest_crest` pins it against horizon.wgsl).
pub(crate) const HEADROOM: f32 = 0.25;
/// Rows kept (the oldest is about `ROWS * ROW_EVERY / 60` seconds back).
pub(crate) const ROWS: usize = 14;
/// FFT ticks between rows.
pub(crate) const ROW_EVERY: u32 = 6;
/// How far a row runs past the field on either side (fraction of its width).
pub(crate) const MARGIN: f32 = 0.35;
/// Heights per smooth (Lines) row.
pub(crate) const SMOOTH_SAMPLES: usize = 128;
/// Bars per stepped row before neighbours are grouped (by max).
pub(crate) const MAX_ROW_BARS: usize = 192;
/// A mirrored stretch eases down to this share by the margin's end, so it
/// reads as the range continuing rather than a copy.
const MIRROR_FLOOR: f32 = 0.6;

/// How a row is shaped: stepped bars or a smooth wave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowShape {
    /// Bars mode: one height per (grouped) bar.
    Stepped,
    /// Lines mode: a smooth wave through the points.
    Smooth,
}

pub(crate) struct HorizonRows {
    rows: VecDeque<Vec<f32>>,
    /// (value count, shape) the rows were built for; a change starts over.
    layout: (usize, RowShape),
    since: u32,
    gpu: Vec<[f32; 8]>,
}

impl HorizonRows {
    pub(crate) fn new() -> Self {
        Self {
            rows: VecDeque::with_capacity(ROWS + 1),
            layout: (0, RowShape::Stepped),
            since: 0,
            gpu: Vec::new(),
        }
    }

    /// Drop the history (a track change): the next tick starts over.
    pub(crate) fn clear(&mut self) {
        self.rows.clear();
        self.since = 0;
        self.gpu.clear();
    }

    /// The GPU snapshot (see the module docs).
    pub(crate) fn gpu_data(&self) -> &[[f32; 8]] {
        &self.gpu
    }

    /// One FFT tick with this tick's values (0..1).
    pub(crate) fn update(&mut self, values: &[f64], shape: RowShape) {
        if self.layout != (values.len(), shape) {
            self.layout = (values.len(), shape);
            self.rows.clear();
        }
        let group = values.len().div_ceil(MAX_ROW_BARS).max(1);
        let core = values.len().div_ceil(group);
        let margin_bars = (core as f32 * MARGIN).round() as usize;
        self.since += 1;
        if self.since >= ROW_EVERY || self.rows.is_empty() {
            self.since = 0;
            let row = match shape {
                RowShape::Stepped => stepped_row(values, group, margin_bars),
                RowShape::Smooth => smooth_row(values),
            };
            self.rows.push_front(row);
            self.rows.truncate(ROWS);
        }
        let phase = self.since as f32 / ROW_EVERY as f32;
        let samples = self.rows.front().map_or(0, Vec::len);
        self.gpu.clear();
        self.gpu.push(match shape {
            RowShape::Stepped => [
                self.rows.len() as f32,
                samples as f32,
                phase,
                margin_bars as f32,
                core as f32,
                group as f32,
                1.0,
                0.0,
            ],
            RowShape::Smooth => [
                self.rows.len() as f32,
                samples as f32,
                phase,
                MARGIN,
                0.0,
                0.0,
                0.0,
                0.0,
            ],
        });
        let mut entry = [0.0; 8];
        let mut filled = 0;
        for &v in self.rows.iter().flatten() {
            entry[filled] = v;
            filled += 1;
            if filled == 8 {
                self.gpu.push(entry);
                entry = [0.0; 8];
                filled = 0;
            }
        }
        if filled > 0 {
            self.gpu.push(entry);
        }
    }
}

/// The ease applied `past` (in margin units, 0..1) beyond the field's edge.
fn mirror_ease(past: f32) -> f32 {
    1.0 - (1.0 - MIRROR_FLOOR) * past.clamp(0.0, 1.0)
}

/// One stepped row: the bars (grouped by max), plus `margin` bars either side
/// mirrored outward and easing down.
fn stepped_row(values: &[f64], group: usize, margin: usize) -> Vec<f32> {
    let core: Vec<f32> = values
        .chunks(group)
        .map(|c| c.iter().fold(0.0f32, |m, &v| m.max(v as f32)))
        .collect();
    let len = core.len();
    if len == 0 {
        return Vec::new();
    }
    (0..len + 2 * margin)
        .map(|j| {
            let i = j as isize - margin as isize;
            let (src, past) = if i < 0 {
                ((-i - 1) as usize, -i as f32)
            } else if i as usize >= len {
                let over = i as usize - len;
                (len.saturating_sub(1 + over), (over + 1) as f32)
            } else {
                (i as usize, 0.0)
            };
            core[src.min(len - 1)] * mirror_ease(past / margin.max(1) as f32)
        })
        .collect()
}

/// One smooth row: a Catmull-Rom through the points (point `i` at field
/// position `i / (n - 1)`, as Lines draws them), over the field plus `MARGIN`
/// either side mirrored outward, lightly softened so the mirror seams stay
/// smooth.
fn smooth_row(values: &[f64]) -> Vec<f32> {
    let n = values.len();
    if n < 2 {
        return vec![0.0; SMOOTH_SAMPLES];
    }
    let at = |i: isize| values[i.clamp(0, n as isize - 1) as usize];
    let point = |t: f32| -> f32 {
        let f = t.clamp(0.0, 1.0) * (n - 1) as f32;
        let i = f.floor() as isize;
        let v = catmull_rom_1d(
            at(i - 1),
            at(i),
            at(i + 1),
            at(i + 2),
            f64::from(f - i as f32),
        );
        (v as f32).clamp(0.0, 1.0)
    };
    let span = 1.0 + 2.0 * MARGIN;
    let raw: Vec<f32> = (0..SMOOTH_SAMPLES)
        .map(|s| {
            let t = -MARGIN + span * s as f32 / (SMOOTH_SAMPLES - 1) as f32;
            let (mirrored, past) = if t < 0.0 {
                (-t, -t)
            } else if t > 1.0 {
                (2.0 - t, t - 1.0)
            } else {
                (t, 0.0)
            };
            point(mirrored) * mirror_ease(past / MARGIN)
        })
        .collect();
    (0..SMOOTH_SAMPLES)
        .map(|s| {
            let (mut sum, mut wsum) = (0.0, 0.0);
            for k in -3i32..=3 {
                let i = (s as i32 + k).clamp(0, SMOOTH_SAMPLES as i32 - 1) as usize;
                let w = (-(k * k) as f32 / 3.0).exp();
                sum += raw[i] * w;
                wsum += w;
            }
            sum / wsum
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `const <name>: f32 = <value>;` from horizon.wgsl.
    fn wgsl_f32(name: &str) -> f32 {
        let src = include_str!("shaders/horizon.wgsl");
        let decl = format!("const {name}: f32 =");
        let start = src.find(&decl).unwrap_or_else(|| panic!("missing {decl}")) + decl.len();
        let end = start
            + src[start..]
                .find(';')
                .unwrap_or_else(|| panic!("{name} has no `;`"));
        src[start..end]
            .trim()
            .parse()
            .unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    /// The farthest full-height crest stays under the guard fade at the top
    /// of the canvas, so no row is ever cut flat by the canvas edge. Mirrors
    /// the shader's row geometry: baseline `RIDGE_HORIZON * (1 - 1/d) /
    /// (1 - 1/dmax)`, crest `RIDGE_AMP * h / d` above it, `d = 1 + z * DEPTH`.
    #[test]
    fn headroom_holds_the_farthest_crest() {
        let (horizon, depth, amp, fade) = (
            wgsl_f32("RIDGE_HORIZON"),
            wgsl_f32("RIDGE_DEPTH"),
            wgsl_f32("RIDGE_AMP"),
            wgsl_f32("TOP_FADE"),
        );
        let dmax = 1.0 + ROWS as f32 * depth;
        let highest = (0..=ROWS * 10)
            .map(|i| {
                let d = 1.0 + i as f32 / 10.0 * depth;
                horizon * (1.0 - 1.0 / d) / (1.0 - 1.0 / dmax) + amp / d
            })
            .fold(0.0f32, f32::max);
        assert!(
            highest > 1.0,
            "the far crests rise above the band, which is why the headroom exists"
        );
        assert!(
            highest <= 1.0 + HEADROOM - fade,
            "a crest at {highest} scenes reaches the top fade (starts at {})",
            1.0 + HEADROOM - fade
        );
    }

    #[test]
    fn stepped_rows_mirror_past_both_edges_and_ease_down() {
        let row = stepped_row(&[0.2, 0.4, 0.6, 1.0], 1, 2);
        assert_eq!(row.len(), 8);
        assert_eq!(
            &row[2..6],
            &[0.2, 0.4, 0.6, 1.0],
            "the field itself is untouched"
        );
        // Left: mirrored outward from bar 0 (0.2 then 0.4), easing toward the floor.
        assert!((row[1] - 0.2 * mirror_ease(0.5)).abs() < 1e-6);
        assert!((row[0] - 0.4 * MIRROR_FLOOR).abs() < 1e-6);
        // Right: mirrored outward from the last bar.
        assert!((row[6] - 1.0 * mirror_ease(0.5)).abs() < 1e-6);
        assert!((row[7] - 0.6 * MIRROR_FLOOR).abs() < 1e-6);
    }

    #[test]
    fn wide_fields_group_bars_by_their_loudest() {
        let mut values = vec![0.1; MAX_ROW_BARS * 2];
        values[5] = 0.9;
        let mut h = HorizonRows::new();
        h.update(&values, RowShape::Stepped);
        let head = h.gpu_data()[0];
        assert_eq!(head[4] as usize, MAX_ROW_BARS, "core bars after grouping");
        assert_eq!(head[5] as usize, 2, "two bars per row bar");
        let margin = head[3] as usize;
        let first_row = &h.gpu_data()[1..].concat()[..head[1] as usize];
        assert!(
            (first_row[margin + 2] - 0.9).abs() < 1e-6,
            "bar 5 lands in row bar 2"
        );
    }

    #[test]
    fn rows_arrive_every_few_ticks_newest_first_and_cap() {
        let mut h = HorizonRows::new();
        for tick in 0..(ROWS as u32 + 3) * ROW_EVERY {
            h.update(&[f64::from(tick) / 1000.0; 8], RowShape::Stepped);
            let head = h.gpu_data()[0];
            assert!((0.0..1.0).contains(&head[2]), "phase stays in [0, 1)");
        }
        let head = h.gpu_data()[0];
        assert_eq!(head[0] as usize, ROWS, "capped at ROWS");
        let samples = head[1] as usize;
        let margin = head[3] as usize;
        let flat = h.gpu_data()[1..].concat();
        assert!(
            flat[margin] > flat[samples + margin],
            "the newest row (loudest here) comes first"
        );
    }

    #[test]
    fn a_layout_change_starts_the_history_over() {
        let mut h = HorizonRows::new();
        for _ in 0..ROW_EVERY * 3 {
            h.update(&[0.5; 8], RowShape::Stepped);
        }
        assert!(h.gpu_data()[0][0] > 1.0);
        h.update(&[0.5; 9], RowShape::Stepped);
        assert_eq!(h.gpu_data()[0][0], 1.0, "a new bar count starts over");
        h.update(&[0.5; 9], RowShape::Smooth);
        assert_eq!(h.gpu_data()[0][0], 1.0, "so does a new shape");
        assert_eq!(h.gpu_data()[0][1] as usize, SMOOTH_SAMPLES);
        assert_eq!(h.gpu_data()[0][6], 0.0, "smooth rows are flagged as such");
    }

    #[test]
    fn smooth_rows_follow_the_points_and_mirror_symmetrically() {
        let row = smooth_row(&[0.0, 1.0, 0.0]);
        assert_eq!(row.len(), SMOOTH_SAMPLES);
        let mid = SMOOTH_SAMPLES / 2;
        assert!(row[mid] > 0.7, "the peak point stands up: {}", row[mid]);
        // A symmetric input gives a symmetric row, margins included.
        for s in 0..SMOOTH_SAMPLES {
            assert!((row[s] - row[SMOOTH_SAMPLES - 1 - s]).abs() < 1e-4);
        }
    }
}
