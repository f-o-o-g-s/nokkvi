---
trigger: glob
globs: src/widgets/visualizer/**,src/visualizer_config.rs
---

# Visualizer

## SpectrumEngine (RustFFT)

Pure-Rust FFT in `data/src/audio/spectrum.rs`. Dual-band FFT (bass 2×, treble 1×). `max_bars_for_sample_rate()` caps treble bins; `interpolate_bars()` fills gaps. Zero allocation (pre-allocated scratch). Engine reinitializes on sample rate change.

Config: `lower_cutoff_freq`, `higher_cutoff_freq`, `noise_reduction`, `auto_sensitivity`.

## Shader Pipeline

- **Flat mode**: fast path via iced render pass
- **3D mode (4x MSAA)**: two-pass: render to MSAA texture → resolve → blit
- Two WGSL shaders: `bars.wgsl` (gradient, peaks, LED, 3D), `lines.wgsl` (spline, fill, mirror)
- Both share a `Config` struct (must stay in sync with `shader.rs`)

## Peak Modes (Bars)

`none`, `fade`, `fall`, `fall_accel`, `fall_fade`. `peak_fall_speed` (1–20) scales velocity.

## Gradient Modes (Bars)

`static` (height-based), `wave` (stretch), `shimmer` (flat per-bar cycling), `energy` (loudness offset), `alternate` (first two colors). `gradient_orientation`: `vertical` (default) or `horizontal`.

## Lines Mode

Catmull-Rom spline interpolation, per-vertex coloring, optional fill/mirror. Instance-based rendering (6 instances: fill/outline/main × normal/mirror). Gradient modes: `breathing`, `static`, `position`, `height`, `gradient`. Reuses bar gradient colors palette.

Key settings: `point_count` (8–512), `line_thickness`, `outline_thickness`, `fill_opacity`, `mirror`, `style` (smooth/angular).

## Configuration

Under `[visualizer]` and `[visualizer.bars]` / `[visualizer.lines]` in `config.toml`. Colors in active theme file under `[dark.visualizer]` / `[light.visualizer]`: `bar_gradient_colors`, `peak_gradient_colors`, `border_color`, border opacities.

Smoothing filters (mutually exclusive): `waves` (Catmull-Rom, `waves_smoothing` 2–16) or `monstercat` (exponential, 0.7–1.0).

## Runtime

- Hot-reload: config watcher → `VisualizerConfigChanged` → state rebuilt
- Dirty-flag gated redraws: `is_dirty()` / `clear_dirty()` — GPU idle when paused
- `apply_config()` sets `pending_engine_reinit` — FFT thread picks it up, prevents stutter
- Resize debouncing: 100ms for bar count changes
