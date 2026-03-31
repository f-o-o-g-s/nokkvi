---
trigger: glob
globs: src/widgets/visualizer/**,src/visualizer_config.rs
---

# Visualizer

## SpectrumEngine (RustFFT)

- Pure-Rust FFT engine in `data/src/audio/spectrum.rs`
- Config: `lower_cutoff_freq`, `higher_cutoff_freq`, `noise_reduction`, `auto_sensitivity`
- Engine reinitialized when sample rate changes
- **FFT bin limit**: treble buffer bins capped by sample rate; `max_bars_for_sample_rate()`
- **FFT interpolation**: when visual bar count exceeds FFT bin limit, `interpolate_bars()` linearly fills gaps
- **Dual-band FFT**: bass at 2× resolution, treble at 1×
- **Thread-safe**: RustFFT plans are `Send + Sync`
- **Zero allocation**: scratch buffers pre-allocated in `SpectrumEngine::new()`

## Pre-Volume Sample Feed

The visualizer receives **pre-volume samples** from `StreamingSource` — the raw decoded sample is fed to the viz buffer before volume multiplication, scaled to S16 range. FFT input is volume-independent, matching the old PipeWire behavior.

## Shader Pipeline

Two render paths:
- **Flat mode (no MSAA):** fast path via iced's render pass
- **3D/Perspective mode (4x MSAA):** two-pass: render to MSAA texture → resolve → blit via fullscreen quad

Isometric 3D bars use true parallelogram geometry (top + side faces) in WGSL.

Two shader files share a common `Config` struct (must stay in sync with `shader.rs`):
- `shaders/bars.wgsl` — bar rendering with gradient, peaks, LED, 3D
- `shaders/lines.wgsl` — line rendering with spline interpolation, fill, mirror

## Peak Modes (Bars Only)

| Mode | Behavior |
|------|----------|
| `none` | No peak indicators |
| `fade` | Peak fades in place after hold time |
| `fall` | Peak falls at configurable constant velocity |
| `fall_accel` | Peak falls with configurable acceleration (gravity) |
| `fall_fade` | Peak falls at constant speed while simultaneously fading out |

`peak_fall_speed` (1–20, default 5): scales fall/fall_accel velocity.

## Bar Gradient Modes

| Mode | Behavior |
|------|----------|
| `static` | Static height-based gradient (bottom to top) |
| `wave` | Gradient stretching (taller bars show more bottom colors) |
| `shimmer` | Bars cycle through all gradient colors as flat per-bar colors |
| `energy` | Energy-scaled gradient offset (shifts based on overall loudness) |
| `alternate` | Bars alternate between first two gradient colors |

`gradient_orientation`: `"vertical"` (default) maps colors bottom-to-top within each bar; `"horizontal"` maps left-to-right across bars. Works with all gradient modes except `alternate`.

## Lines Mode

Line rendering with Catmull-Rom spline interpolation, per-vertex coloring, and optional fill/mirror.

### Lines Configuration (`[visualizer.lines]`)

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `point_count` | int | 24 | Data points (8–512), more = finer detail |
| `line_thickness` | float | 0.05 | Fraction of visualizer height (0.01–0.10) |
| `outline_thickness` | float | 2.0 | Border behind line in pixels (0 = disabled) |
| `outline_opacity` | float | 1.0 | Outline transparency (0.0–1.0) |
| `animation_speed` | float | 0.25 | Color cycling speed (0.05–1.0) |
| `gradient_mode` | enum | breathing | See gradient modes below |
| `fill_opacity` | float | 0.0 | Fill under curve (0 = disabled, 1 = opaque) |
| `mirror` | bool | false | Symmetric oscilloscope from center |
| `style` | enum | smooth | `smooth` (Catmull-Rom) or `angular` (straight segments) |

### Lines Gradient Modes

| Mode | Shader Value | Behavior |
|------|-------------|----------|
| `breathing` | 0 | Time-based cycling through gradient palette |
| `static` | 1 | Uses first gradient color only |
| `position` | 2 | Color by horizontal position (bass → treble rainbow, all 8 colors) |
| `height` | 3 | Color by amplitude (quiet → loud, all 8 colors) |
| `gradient` | 4 | Position + amplitude blend — peaks shift palette further |

Lines mode reuses the bar gradient colors palette (`dark.visualizer.bar_gradient_colors` / `light.visualizer.bar_gradient_colors`).

### Lines Shader Architecture

- Instance-based rendering: 6 instances (fill/outline/main × normal/mirror)
- 16 spline samples per segment for smooth curves
- Per-vertex gradient color computed in vertex shader
- Fragment shader applies smoothstep antialiasing for line/outline passes
- Fill pass: triangle strip from curve to baseline (no AA needed)
- Mirror pass: Y coordinates flipped around canvas center

## Configuration (TOML)

Under `[visualizer]` and `[visualizer.bars]` in `config.toml`:
- Dynamic bar sizing: `bar_width_min` / `bar_width_max`
- `max_bars`: 16–2048, default 2048
- `auto_sensitivity`: bool toggle (hot-reloadable)
- LED mode: `led_bars`, `led_segment_height`
- Border opacities (per-theme in `[dark.visualizer]` / `[light.visualizer]`):
  - `led_border_opacity` (0.0–1.0): LED mode segment gap opacity
  - `border_opacity` (0.0–1.0): regular bar border opacity
  - Dark defaults: 1.0 (visible); Light defaults: 0.0 (hidden)
- 3D depth: `bar_depth_3d` (0 = flat)
- Pixel-based settings use **integer steps** ("1 px", not "1.00 px")
- `opacity` (float, 0.0–1.0): overall visualizer transparency via `global_opacity` uniform

### Smoothing Filters (mutually exclusive)

- `waves` (bool): Catmull-Rom spline interpolation across bars. `waves_smoothing` (2–16) controls subsampling step.
- `monstercat` (float, 0.0 or 0.7–1.0): exponential smoothing. Values below 0.7 clamped to 0.0.
- Only one filter active at a time — enabling one disables the other in GUI.

### Catmull-Rom Post-Smoothing

When `monstercat` is active, Catmull-Rom spline pass applied after frequency-domain smoothing.

## Hot-Reload

Config watcher detects changes → `VisualizerConfigChanged` → state rebuilt. No restart needed.

## Config Writer

`config_writer.rs` uses `toml_edit` for atomic updates preserving comments. Auto-injects description comments from `SettingMeta.subtitle`.

## Dirty-Flag Gated Redraws

- `is_dirty()` / `clear_dirty()` — atomic flag set by FFT thread after processing
- `ShaderVisualizer::update()` only returns `request_redraw()` when dirty
- When music is paused/stopped, GPU usage drops to near-zero

## Engine Reinitialization

- `apply_config()` sets `pending_engine_reinit` — FFT thread picks it up and reinitializes
- Prevents stutter during config hot-reload or window resize
