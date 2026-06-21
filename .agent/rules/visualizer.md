---
trigger: glob
globs: src/widgets/visualizer/**,src/visualizer_config.rs
---

# Visualizer

## SpectrumEngine (RustFFT)

Pure-Rust FFT in `data/src/audio/spectrum.rs`. Dual-band FFT (bass 2×, treble 1×). `max_bars_for_sample_rate()` caps treble bins; `interpolate_bars()` (`src/widgets/visualizer/state.rs`) fills gaps. Zero allocation (pre-allocated scratch). Engine reinitializes on sample rate change.

Spectrum config: `lower_cutoff_freq`, `higher_cutoff_freq`, `noise_reduction`, `auto_sensitivity` (fields + `MONSTERCAT_MIN_EFFECTIVE` in `src/visualizer_config.rs`). Smoothing filters (mutually exclusive, applied as `waves_filter` / `monstercat_filter` in `src/widgets/visualizer/state.rs`): `waves` (Catmull-Rom, `waves_smoothing` 2–16) or `monstercat` (exponential; values < `MONSTERCAT_MIN_EFFECTIVE = 0.7` are snapped to 0 / off; default 1.0).

## Module Structure

- `widgets/visualizer/mod.rs` — `Visualizer` Iced widget glue (its `view()` wraps a `ShaderVisualizer` in `iced::widget::shader`); `build_shader_params(...)` constructs the 42-field `ShaderParams` from a config snapshot, theme palette, and viewport
- `widgets/visualizer/state.rs` — `VisualizerState` runtime (audio callback, FFT pipeline, peak/effect state, display buffers); `VisualizerTiming` is a zero-sized struct holding the per-frame tick constants (`TICK_RATE_HZ = 60`, `TICK_INTERVAL`, and ms/secs variants) all derived from one rate
- `widgets/visualizer/pipeline.rs` — `MAX_BARS = 2048`, GPU buffers, `VisualizerPipeline::new` (the struct itself is declared in `shader.rs`)
- `widgets/visualizer/shader.rs` — `ShaderParams` struct, `ShaderVisualizer` (the `shader::Program` impl), render dispatch, MSAA texture cache, blit shader
- `widgets/visualizer/shaders/bars.wgsl`, `lines.wgsl`, `scope.wgsl` — each declares a `Config` struct that must mirror the bytemuck-Pod GPU uniform `VisualizerConfig` (`shader.rs`, NOT `ShaderParams` — that is a CPU-side grouping struct with a different field list) verbatim; a drift is silent memory reinterpretation. Interlocks: const-asserts in `shader.rs` pin alignment (16), size (8336), and key offsets; `wgsl_config_field_names_match_rust_struct` in `mod.rs` pins the WGSL field names against the Rust struct, and `wgsl_config_blocks_declare_identical_fields` pins bars/lines/scope against each other. Update all four (shader.rs + the three shaders) together when changing a config field.

**Render path:** non-MSAA fast path by default; switches to **4× MSAA → resolve → blit** when perspective lean is active (the `has_perspective` flag on `VisualizerPrimitive`, set from `bar_depth_3d > 0.001`, gates the path per-frame).

`VisualizationMode` enum (`data/src/types/player_settings/visualizer.rs`): `Off`, `Bars`, `Lines`, `Scope` (cycled by the player-bar toggle).  `MIN_BAR_COUNT = 4`; bar width interpolates between `bar_width_min` and `bar_width_max` over a 400→2560px window range.

**Placement.** Bars and Lines each carry a `VisualizerPlacement` (`OverCover` default / `BottomBand`) in `src/visualizer_config.rs` (per-mode field on `BarsConfig` / `LinesConfig`, follows the same `ALL`/`as_wire_str()`/pin-test enum convention as the others). `BottomBand` draws a band above the player bar (every view); `OverCover` draws over the now-playing cover art in the Queue, while playing — the slot the Scope ring uses (Scope is always over-cover, with no placement of its own). Over the cover, Bars/Lines honor the `Visualizer Height` setting (`height_percent`) as a bottom-anchored fraction of the cover height; Scope fills the panel (its ring sizes off `scope.radius`). `widgets::visualizer::resolve_placement(mode, bars_placement, lines_placement) -> VisualizerSlots { bottom_band, over_art }` is the single source of truth for the render fork (the two slots are mutually exclusive); `app_view` calls it for both the bottom-band overlay and the over-cover (`single_artwork_panel_inner`) render sites. The surfing boat rides the Lines wave in either placement — the bottom band (`app_view`) or over the cover (the Queue artwork panel, via `OverCoverBoat`).

## Bars Mode

Mode enums in `src/visualizer_config.rs` (real Rust enums, not strings — hand-rolled `#[derive(Serialize, Deserialize)]` `#[serde(rename_all = "snake_case")]` `#[repr(u32)]` enums, each with a manual `as_wire_str()`, a pinned `ALL` const slice (declaration order = settings-dropdown display order), and `all_wire_strs()`; deserialization is via the derived `Deserialize`). The settings dropdowns in `src/views/settings/items_visualizer.rs` derive their option lists from `all_wire_strs()` — when adding a variant, extend `ALL` too; pin tests assert each `ALL` carries every variant exactly once in declaration order, and the no-wildcard `as_wire_str()` matches force a compile error until both are updated. Enums:
- `BarsPeakMode`: `None` / `Fade` / `Fall` / `FallAccel` / `FallFade`. `peak_fall_speed` 1–20.
- `BarsGradientMode`: `Static` (0) / `Wave` (2). **Discriminant `1` is intentionally skipped** — `bars.wgsl` has no branch for it; the `bars_gradient_mode_never_emits_dead_1u` test in `src/visualizer_config.rs` pins this against accidental future use. (Shimmer/Energy/Alternate were dropped in b92d311; the glow/bloom/beat effects supersede them.)
- `BarsGradientOrientation`: `Vertical` (within-bar) / `Horizontal` (bass → treble across bars).
- `BarsPeakGradientMode`: `Static` / `Cycle` / `Height` / `Match` (separate enum from bar gradients).

## Lines Mode

- `LinesStyle`: `Smooth` (Catmull-Rom spline) / `Angular` (straight segments).
- `LinesGradientMode`: `Breathing` / `Static` / `Position` / `Height` / `Gradient`. Reuses bar gradient palette.

6-instance render: fill / outline / main × normal / mirror. Per-vertex coloring, optional fill / mirror.

Key settings: `point_count` (8–512), `line_thickness`, `outline_thickness`, `fill_opacity`, `mirror`, `style`, `boat` (toggle the surfing-boat overlay; `widgets/boat.rs`, CPU-only, themed via `embedded_svg::themed_boat_svg()` using `border_color`).

## Configuration

- Behavior under `[visualizer]`, `[visualizer.bars]`, `[visualizer.lines]`, `[visualizer.scope]` in `config.toml`
- Colors under `[dark.visualizer]` / `[light.visualizer]` in active theme file: `bar_gradient_colors`, `peak_gradient_colors`, `border_color`, `border_opacity`, `led_border_opacity`

## Runtime

- Hot-reload: config watcher → `VisualizerConfigChanged` → state rebuilt
- Dirty-flag gated redraws: `is_dirty()` / `clear_dirty()` — GPU idle when paused
- `apply_config()` sets `pending_engine_reinit` — FFT thread picks it up, prevents stutter
- Resize debouncing: 100ms for bar count changes
- FFT thread uses `try_lock()` only on the audio engine — main render thread alone may use `lock()`
