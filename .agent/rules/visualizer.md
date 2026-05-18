---
trigger: glob
globs: src/widgets/visualizer/**,src/visualizer_config.rs
---

# Visualizer

## SpectrumEngine (RustFFT)

Pure-Rust FFT in `data/src/audio/spectrum.rs`. Dual-band FFT (bass 2×, treble 1×). `max_bars_for_sample_rate()` caps treble bins; `interpolate_bars()` fills gaps. Zero allocation (pre-allocated scratch). Engine reinitializes on sample rate change.

Spectrum config: `lower_cutoff_freq`, `higher_cutoff_freq`, `noise_reduction`, `auto_sensitivity`. Smoothing filters (mutually exclusive): `waves` (Catmull-Rom, `waves_smoothing` 2–16) or `monstercat` (exponential; values < `MONSTERCAT_MIN_EFFECTIVE = 0.7` are snapped to 0 / off; default 1.0).

## Module Structure

- `widgets/visualizer/mod.rs` — `ShaderVisualizer` Iced widget glue; `build_shader_params(...)` constructs the 31-field `ShaderParams` from a config snapshot, theme palette, and viewport
- `widgets/visualizer/state.rs` — `VisualizerState` runtime (audio callback, FFT pipeline, peak/effect state, display buffers); `VisualizerTiming` is a unit-conversion newtype (ms → seconds, percent → unit)
- `widgets/visualizer/pipeline.rs` — `MAX_BARS = 2048`, GPU buffers, `Pipeline::new`
- `widgets/visualizer/shader.rs` — `ShaderParams` struct, render dispatch, MSAA texture cache, blit shader
- `widgets/visualizer/shaders/bars.wgsl`, `lines.wgsl` — share a `Config` struct (must stay in sync with `ShaderParams` field order)

**Render path:** non-MSAA fast path by default; switches to **4× MSAA → resolve → blit** when perspective lean is active (`is_msaa_required()` toggles per-frame).

`VisualizationMode` enum (`data/src/types/player_settings/visualizer.rs`): `Off`, `Bars`, `Lines` (cycled by the player-bar toggle). `MIN_BAR_COUNT = 4`; bar width interpolates between `bar_width_min` and `bar_width_max` over a 400→2560px window range.

## Bars Mode

Mode enums in `src/visualizer_config.rs` (real Rust enums, not strings — Group G `define_labeled_enum!` migration; each has `as_wire_str()` + `from_wire_str()` matching `#[serde(rename_all = "snake_case")]`):
- `BarsPeakMode`: `None` / `Fade` / `Fall` / `FallAccel` / `FallFade`. `peak_fall_speed` 1–20.
- `BarsGradientMode`: `Static` (0) / `Wave` (2) / `Shimmer` (3) / `Energy` (4) / `Alternate` (5). **Discriminant `1` is intentionally skipped** — `bars.wgsl` has no branch for it; the `bars_gradient_mode_never_emits_dead_1u` test in `src/visualizer_config.rs` pins this against accidental future use.
- `BarsGradientOrientation`: `Vertical` (within-bar) / `Horizontal` (bass → treble across bars).
- `BarsPeakGradientMode`: `Static` / `Cycle` / `Height` / `Match` (separate enum from bar gradients).

## Lines Mode

- `LinesStyle`: `Smooth` (Catmull-Rom spline) / `Angular` (straight segments).
- `LinesGradientMode`: `Breathing` / `Static` / `Position` / `Height` / `Gradient`. Reuses bar gradient palette.

6-instance render: fill / outline / main × normal / mirror. Per-vertex coloring, optional fill / mirror.

Key settings: `point_count` (8–512), `line_thickness`, `outline_thickness`, `fill_opacity`, `mirror`, `style`, `boat` (toggle the surfing-boat overlay; `widgets/boat.rs`, CPU-only, themed via `embedded_svg::themed_boat_svg()` using `border_color`).

## Configuration

- Behavior under `[visualizer]`, `[visualizer.bars]`, `[visualizer.lines]` in `config.toml`
- Colors under `[dark.visualizer]` / `[light.visualizer]` in active theme file: `bar_gradient_colors`, `peak_gradient_colors`, `border_color`, `border_opacity`, `led_border_opacity`

## Runtime

- Hot-reload: config watcher → `VisualizerConfigChanged` → state rebuilt
- Dirty-flag gated redraws: `is_dirty()` / `clear_dirty()` — GPU idle when paused
- `apply_config()` sets `pending_engine_reinit` — FFT thread picks it up, prevents stutter
- Resize debouncing: 100ms for bar count changes
- FFT thread uses `try_lock()` only on the audio engine — main render thread alone may use `lock()`
