//! Procedural sea + trawling-longship scene for the Harbour Trawl panel.
//!
//! The Harbour landing view opens centered on the Trawl mix-builder row, whose
//! artwork panel used to show a static anchor glyph. This module replaces it
//! with a living scene: a gently travelling two-layer sea (drawn by
//! [`SeaCanvas`]) with the nokkvi longship sailing across it, perpetually
//! dragging its anchor along the seabed — trawling. The boat itself is the
//! Lines-visualizer surfing boat reused verbatim ([`boat_overlay`] with a
//! `trail` offset); only the wave source is new.
//!
//! Coherence contract: [`sea_bars`] produces ONE array per tick
//! (`update::boat::step_harbour_scene`), which is BOTH fed to
//! `boat_physics::step()` and stored on `Nokkvi.harbour_sea_bars` for
//! [`SeaCanvas`] to draw through the same [`sample_line_height`] sampler the
//! physics used. A phase or sampler mismatch would desync the hull from the
//! drawn water invisibly to tests/clippy — always route both sides through
//! this module.
//!
//! Everything here is silence-proof by construction: the sea is a pure
//! function of a phase the boat tick advances, and the physics' presence
//! cruise is fed a fixed [`HARBOUR_CRUISE_BAR_ENERGY`] instead of live audio,
//! so the scene breathes identically with the player stopped, paused, or
//! playing.

use iced::{Color, Element, Length, Point, Rectangle, widget::canvas};

use crate::widgets::boat::{BoatState, boat_overlay, parse_hex_color, sample_line_height};

/// Samples in the sea height field — enough that the Catmull-Rom resample
/// reads as a smooth swell at panel widths, few enough that building the
/// array per frame is negligible.
pub(crate) const SEA_POINTS: usize = 96;

/// Travelling-phase advance rate in cycles/sec. The front swell's crest
/// speed is `(SWELL_PHASE_K / SWELL_CYCLES) · SEA_DRIFT_HZ` panel-widths
/// per second — 0.05 gives a ~20 s crest crossing, the calm baseline.
// TUNE: raise for a livelier sea, lower for glassier water.
pub(crate) const SEA_DRIFT_HZ: f32 = 0.05;

/// Fixed `MusicSignals::bar_energy` fed to the harbour boat's physics step.
/// This is the scene's calm lever, deliberately NOT the sea's true mean
/// (~0.45): presence cruise = `(0.20 − 0.10) · 1.5 = 0.15` → terminal
/// velocity ≈ 0.017 ratio/sec (a ~60 s crossing) with a 0.006 velocity
/// floor, so the boat always creeps forward but never hurries.
// TUNE: the single strongest calm↔alive dial. 0.30 ≈ 30 s crossings.
pub(crate) const HARBOUR_CRUISE_BAR_ENERGY: f32 = 0.20;

/// How far behind the hull (in `x_ratio` units) the trawled anchor trails at
/// cruise speed. The render eases this by `|x_velocity| / TRAIL_V_REF`, so
/// the anchor slides under the hull as the boat stalls through a tack.
// TUNE: longer reads as a heavier drag; shorter tucks the anchor under the stern.
pub(crate) const TRAIL_OFFSET: f32 = 0.08;

/// Sea shape — one slow swell plus a faster low ripple, both travelling.
/// Every layer's phase multiplier is an INTEGER so the field is exactly
/// periodic in the `[0, 1)` phase (`sea_bars(0) == sea_bars(1)`): the tick
/// wraps the phase with `rem_euclid(1.0)` to dodge long-session f32 sin
/// precision decay, and integer multipliers make that wrap seamless.
/// `SWELL_CYCLES` / `RIPPLE_CYCLES` are integers too, which additionally
/// makes the field periodic in X — the boat's toroidal slope sampling near
/// the wrap seam then reads a REAL gradient instead of a fake edge cliff.
// TUNE: DC sets the waterline height (fraction of the scene); amps set chop.
const SEA_DC: f64 = 0.45;
const SWELL_AMP: f64 = 0.06;
const SWELL_CYCLES: f64 = 2.0;
const SWELL_PHASE_K: f64 = 2.0;
const RIPPLE_AMP: f64 = 0.025;
const RIPPLE_CYCLES: f64 = 5.0;
const RIPPLE_PHASE_K: f64 = 8.0;
/// Fixed phase offset decorrelating the ripple from the swell so their
/// crests don't align every cycle.
const RIPPLE_SHIFT: f64 = 1.3;

/// Back parallax layer — drawn only (the physics never samples it), a dimmer
/// swell riding higher on the panel. Crest speed `(1 / 2) · SEA_DRIFT_HZ` is
/// HALF the front's — that speed difference is the whole parallax read.
// TUNE: BACK_RAISE lifts the horizon; BACK_AMP sets the far swell's chop.
const BACK_RAISE: f64 = 0.12;
const BACK_AMP: f64 = 0.04;
const BACK_CYCLES: f64 = 2.0;
const BACK_PHASE_K: f64 = 1.0;
const BACK_SHIFT: f64 = 0.7;

/// Layer alphas over the panel background. The sea must stay quiet —
/// Harbour opens centered on this panel, and the landing view must not
/// shout. All three multiply onto the theme's water/crest colors.
// TUNE: raise for a bolder sea, lower to sink it into the background.
const SEA_BACK_ALPHA: f32 = 0.10;
const SEA_FILL_ALPHA: f32 = 0.16;
const SEA_CREST_ALPHA: f32 = 0.38;

/// Horizontal pixel step between sampled points when drawing the water
/// polylines. 3 px keeps the Catmull-Rom curve smooth without building
/// long paths on wide panels.
const SEA_DRAW_STEP_PX: f32 = 3.0;

/// Build the front sea height field for `phase ∈ [0, 1)` — heights in
/// `[0, 1]` of panel height, `SEA_POINTS` samples. This is the ONE array
/// the physics steps against and the canvas draws; see the module docs'
/// coherence contract.
pub(crate) fn sea_bars(phase: f32) -> Vec<f64> {
    use std::f64::consts::TAU;
    let ph = phase as f64;
    (0..SEA_POINTS)
        .map(|i| {
            let x = i as f64 / (SEA_POINTS - 1) as f64;
            let swell = SWELL_AMP * (TAU * (x * SWELL_CYCLES - SWELL_PHASE_K * ph)).sin();
            let ripple =
                RIPPLE_AMP * (TAU * (x * RIPPLE_CYCLES - RIPPLE_PHASE_K * ph) + RIPPLE_SHIFT).sin();
            (SEA_DC + swell + ripple).clamp(0.0, 1.0)
        })
        .collect()
}

/// Height of the decorative back swell at `x ∈ [0, 1]` — drawn behind the
/// front waterline at half its crest speed for the parallax depth read.
/// Analytic (no array) because only the canvas consumes it.
fn back_swell_height(x: f64, phase: f32) -> f64 {
    use std::f64::consts::TAU;
    let ph = phase as f64;
    (SEA_DC
        + BACK_RAISE
        + BACK_AMP * (TAU * (x * BACK_CYCLES - BACK_PHASE_K * ph) + BACK_SHIFT).sin())
    .clamp(0.0, 1.0)
}

/// The Harbour Trawl panel: a quiet two-layer sea with the longship trawling
/// across it, docked above the banded TRAWL pill.
///
/// A COLUMN (not the overlay stack every art-backed panel uses): the pill
/// reserves its own height, so the sea's canvas bottom — the seabed the
/// anchor drags along — lands exactly on the pill's top rail instead of
/// hiding behind the opaque `bg0_hard` band. The inner `responsive` gives
/// the boat and sea the real pixels of the region ABOVE the pill, keeping
/// the sprite sized to the visible water (and dodging the Fill-in-Shrink
/// flex-compression gotcha by carrying bounded sizes itself).
pub(crate) fn trawl_scene<'a, M: 'a>(
    boat: &'a BoatState,
    sea_bars: &'a [f64],
    sea_phase: f32,
    pill: Element<'a, M>,
) -> Element<'a, M> {
    use iced::widget::{column, container, stack};

    let scene = iced::widget::responsive(move |size| {
        let w = size.width.max(1.0);
        let h = size.height.max(1.0);

        // Sky + sea backdrop on the shared artwork background so the panel
        // reads as a sibling of every other artwork column state.
        let backdrop = container(iced::widget::Space::new())
            .width(Length::Fixed(w))
            .height(Length::Fixed(h))
            .style(|_theme| container::Style {
                background: Some(crate::widgets::base_slot_list_layout::artwork_outer_bg().into()),
                ..Default::default()
            });

        let sea = canvas::Canvas::new(SeaCanvas {
            bars: sea_bars,
            phase: sea_phase,
        })
        .width(Length::Fixed(w))
        .height(Length::Fixed(h));

        // The longship, trawling: full opacity (it's the panel's content,
        // not an overlay dimmed against art), mirror off (the harbour sea
        // has no lower reflection), anchor trailed on the seabed.
        let boat_el = boat_overlay::<M>(boat, w, h, w.min(h), 1.0, false, Some(TRAIL_OFFSET));

        let layers: Element<'_, M> = stack![backdrop, sea, boat_el].into();
        layers
    });

    column![
        container(scene).width(Length::Fill).height(Length::Fill),
        crate::widgets::base_slot_list_layout::banded_pill(pill),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Canvas program drawing the two water layers. Inert and event-transparent
/// (a structural sibling of the boat's `RopeCanvas` — no `Cache`, geometry
/// rebuilt per frame, which is correct for a field that changes every tick).
///
/// The FRONT layer is sampled from the SAME bars array the boat physics
/// stepped against, through the SAME [`sample_line_height`] Catmull-Rom
/// sampler — that is what keeps the hull visually sitting ON the water. The
/// BACK layer is decorative parallax, computed analytically from the phase.
struct SeaCanvas<'a> {
    bars: &'a [f64],
    phase: f32,
}

impl<Message> canvas::Program<Message> for SeaCanvas<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let size = bounds.size();
        let (w, h) = (size.width, size.height);
        if w <= 0.0 || h <= 0.0 || self.bars.is_empty() {
            return Vec::new();
        }

        let mut frame = canvas::Frame::new(renderer, size);

        // Water palette from the dark-variant visualizer colors — the same
        // mode-stable family the boat outline, rope, and anchor are themed
        // with, so the whole doodad reads as one system in light AND dark.
        let viz = crate::theme::get_visualizer_colors_dark();
        let water = viz
            .bar_gradient_colors
            .first()
            .and_then(|c| parse_hex_color(c))
            .unwrap_or(Color::from_rgb(0.35, 0.5, 0.6));
        let crest = parse_hex_color(&viz.border_color).unwrap_or(Color::from_rgb(0.5, 0.5, 0.5));

        let steps = ((w / SEA_DRAW_STEP_PX).ceil() as usize).max(2);

        // A closed fill path under a height function: crest polyline, then
        // down the right edge, along the bottom, and back up to the start.
        let fill_under = |height_at: &dyn Fn(f32) -> f32| {
            canvas::Path::new(|b| {
                b.move_to(Point::new(0.0, height_at(0.0)));
                for i in 1..=steps {
                    let x = w * (i as f32 / steps as f32);
                    b.line_to(Point::new(x, height_at(x)));
                }
                b.line_to(Point::new(w, h));
                b.line_to(Point::new(0.0, h));
                b.close();
            })
        };

        // Back layer: dimmer, higher, half-speed — pure depth cue.
        let phase = self.phase;
        let back_y = move |x: f32| h - (back_swell_height((x / w) as f64, phase) as f32) * h;
        frame.fill(
            &fill_under(&back_y),
            Color {
                a: SEA_BACK_ALPHA,
                ..water
            },
        );

        // Front waterline: the surface the boat rides. Fill below, then
        // stroke the crest so the surface reads as a line, not just a wash.
        let bars = self.bars;
        let front_y = move |x: f32| h - sample_line_height(bars, x / w, false) * h;
        frame.fill(
            &fill_under(&front_y),
            Color {
                a: SEA_FILL_ALPHA,
                ..water
            },
        );

        let crest_path = canvas::Path::new(|b| {
            b.move_to(Point::new(0.0, front_y(0.0)));
            for i in 1..=steps {
                let x = w * (i as f32 / steps as f32);
                b.line_to(Point::new(x, front_y(x)));
            }
        });
        frame.stroke(
            &crest_path,
            canvas::Stroke::default()
                .with_color(Color {
                    a: SEA_CREST_ALPHA * viz.border_opacity,
                    ..crest
                })
                .with_width(1.5)
                .with_line_cap(canvas::LineCap::Round),
        );

        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sea_bars_shape_and_range() {
        let bars = sea_bars(0.37);
        assert_eq!(bars.len(), SEA_POINTS);
        assert!(
            bars.iter().all(|&v| (0.0..=1.0).contains(&v)),
            "every sample must stay in [0, 1]"
        );
        // The field must be a real wave, not a flat line.
        let min = bars.iter().copied().fold(f64::INFINITY, f64::min);
        let max = bars.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(
            max - min > 0.01,
            "the sea must undulate (span {})",
            max - min
        );
    }

    #[test]
    fn sea_bars_deterministic() {
        assert_eq!(sea_bars(0.5), sea_bars(0.5));
    }

    #[test]
    fn sea_bars_periodic_across_phase_wrap() {
        // The tick wraps phase with rem_euclid(1.0); integer phase
        // multipliers make sea_bars(1.0) ≡ sea_bars(0.0), so the wrap
        // frame can't visibly jump.
        let a = sea_bars(0.0);
        let b = sea_bars(1.0);
        let max_diff = a
            .iter()
            .zip(&b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f64::max);
        assert!(
            max_diff < 1e-9,
            "phase 0 and phase 1 fields must match (max diff {max_diff})"
        );
    }

    #[test]
    fn sea_bars_travel_with_phase() {
        assert_ne!(
            sea_bars(0.0),
            sea_bars(0.25),
            "advancing the phase must move the wave"
        );
    }

    #[test]
    fn back_swell_periodic_and_bounded() {
        for i in 0..=20 {
            let x = i as f64 / 20.0;
            let v = back_swell_height(x, 0.7);
            assert!((0.0..=1.0).contains(&v));
        }
        assert!((back_swell_height(0.3, 0.0) - back_swell_height(0.3, 1.0)).abs() < 1e-9);
    }
}
