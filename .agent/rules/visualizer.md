---
trigger: glob
globs: src/widgets/visualizer/**,src/visualizer_config.rs
---

# Visualizer

## SpectrumEngine (RustFFT)

Pure-Rust FFT in `data/src/audio/spectrum.rs`. Dual-band FFT (bass 2×, treble 1×). `max_bars_for_sample_rate()` caps treble bins; `interpolate_bars()` fills gaps. Zero allocation (pre-allocated scratch). Engine reinitializes on sample rate change.

Spectrum config: `lower_cutoff_freq`, `higher_cutoff_freq`, `noise_reduction`, `auto_sensitivity`. Smoothing filters (mutually exclusive): `waves` (Catmull-Rom, `waves_smoothing` 2–16) or `monstercat` (exponential; values < `MONSTERCAT_MIN_EFFECTIVE = 0.7` are snapped to 0 / off; default 1.0).

## Shader Pipeline

- `widgets/visualizer/pipeline.rs` — `MAX_BARS = 2048`, GPU buffers, `Pipeline::new`
- `widgets/visualizer/shader.rs` — render dispatch, MSAA texture cache, blit shader
- `widgets/visualizer/shaders/bars.wgsl`, `lines.wgsl` — share a `Config` struct (must stay in sync with `shader.rs`)

**Render path:** non-MSAA fast path by default; switches to **4× MSAA → resolve → blit** when perspective lean is active (`is_msaa_required()` toggles per-frame).

`VisualizationMode` enum: `Bars`, `Lines`. `MIN_BAR_COUNT = 4`; bar width interpolates between `bar_width_min` and `bar_width_max` over a 400→2560px window range.

## Bars Mode

- **Peak modes**: `none`, `fade`, `fall`, `fall_accel`, `fall_fade`. `peak_fall_speed` 1–20.
- **Gradient modes**: `static` (height-based), `wave` (stretch), `shimmer` (per-bar cycling), `energy` (loudness offset), `alternate` (first two colors). `gradient_orientation`: `vertical` | `horizontal`.
- **Peak gradient modes**: `static`, `cycle`, `height`, `match` (separate enum from bar gradients).

## Lines Mode

Catmull-Rom spline (`smooth`) or straight segments (`angular`). 6-instance render: fill / outline / main × normal / mirror. Per-vertex coloring, optional fill / mirror.

Gradient modes: `breathing`, `static`, `position`, `height`, `gradient`. Reuses bar gradient palette.

Key settings: `point_count` (8–512), `line_thickness`, `outline_thickness`, `fill_opacity`, `mirror`, `style`.

## Configuration

- Behavior under `[visualizer]`, `[visualizer.bars]`, `[visualizer.lines]` in `config.toml`
- Colors under `[dark.visualizer]` / `[light.visualizer]` in active theme file: `bar_gradient_colors`, `peak_gradient_colors`, `border_color`, `border_opacity`, `led_border_opacity`

## Runtime

- Hot-reload: config watcher → `VisualizerConfigChanged` → state rebuilt
- Dirty-flag gated redraws: `is_dirty()` / `clear_dirty()` — GPU idle when paused
- `apply_config()` sets `pending_engine_reinit` — FFT thread picks it up, prevents stutter
- Resize debouncing: 100ms for bar count changes
- FFT thread uses `try_lock()` only on the audio engine — main render thread alone may use `lock()`
