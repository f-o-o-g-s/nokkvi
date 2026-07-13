---
paths:
  - "src/widgets/visualizer/**"
  - "src/visualizer_config.rs"
  - "data/src/types/visualizer_config.rs"
  - "data/src/services/settings_tables/visualizer.rs"
  - "data/src/audio/spectrum.rs"
---

# Visualizer

## SpectrumEngine (RustFFT)

Pure-Rust FFT in `data/src/audio/spectrum.rs`. Dual-band FFT (bass 2×, treble 1×). `max_bars_for_sample_rate()` caps treble bins; `interpolate_bars()` (`src/widgets/visualizer/state.rs`) fills gaps. Zero allocation (pre-allocated scratch). Engine reinitializes on sample rate change.

Spectrum config: `lower_cutoff_freq`, `higher_cutoff_freq`, `noise_reduction`, `auto_sensitivity` (fields + `MONSTERCAT_MIN_EFFECTIVE` in `data/src/types/visualizer_config.rs`; `src/visualizer_config.rs` re-exports everything). Smoothing filters (mutually exclusive, applied as `waves_filter` / `monstercat_filter` in `src/widgets/visualizer/state.rs`): `waves` (Catmull-Rom, `waves_smoothing` 2–16) or `monstercat` (exponential; values < `MONSTERCAT_MIN_EFFECTIVE = 0.7` are snapped to 0 / off; default 1.0).

## Module Structure

- `widgets/visualizer/mod.rs` — `Visualizer` Iced widget glue (its `view()` wraps a `ShaderVisualizer` in `iced::widget::shader`); `build_shader_params(...)` constructs the 43-field `ShaderParams` from a config snapshot, theme palette, and viewport
- `widgets/visualizer/state.rs` — `VisualizerState` runtime (audio callback, FFT pipeline, peak/effect state, display buffers); also `interpolate_bars`, `waves_filter` / `monstercat_filter`. `VisualizerTiming` is a zero-sized struct holding the per-frame tick constants (`TICK_RATE_HZ = 60`, `TICK_INTERVAL`, and ms/secs variants) all derived from one rate
- `widgets/visualizer/particles.rs` — `ParticleSystem`, the CPU particle simulation for the Scope ring (curl-noise flow field, ambient + crest-spark spawns, audio/beat-scaled launch). Positions are normalized ring-space; snapshotted into `DisplayBuffers.particles` (two `vec4` per particle) for the GPU
- `widgets/visualizer/pipeline.rs` — `MAX_BARS = 2048`, `MAX_PARTICLES`, GPU buffers, per-mode + MSAA + particle/beam render pipelines, and the bloom/echo/crt post-process pipelines; `VisualizerPipeline::new` (the struct itself is declared in `shader.rs`). All pipelines are `TriangleList`
- `widgets/visualizer/shader.rs` — `ShaderParams` (43-field CPU grouping struct), the bytemuck-Pod GPU uniform `VisualizerConfig`, `ShaderVisualizer` (the `shader::Program` impl), render dispatch, MSAA texture cache, bloom/echo/crt post-process wiring, blit shader
- `widgets/visualizer/shaders/bars.wgsl`, `lines.wgsl`, `scope.wgsl` — the three mode shaders; each declares a `Config` struct that must mirror the GPU uniform `VisualizerConfig` (`shader.rs`, NOT `ShaderParams` — that is a CPU-side grouping struct with a different field list) verbatim; a drift is silent memory reinterpretation. Interlocks: const-asserts in `shader.rs` pin alignment (16), size (8336), and key offsets; `wgsl_config_field_names_match_rust_struct` in `mod.rs` pins the WGSL field names against the Rust struct, and `wgsl_config_blocks_declare_identical_fields` pins bars/lines/scope against each other. Update all four (shader.rs + the three mode shaders) together when changing a config field.
- `widgets/visualizer/shaders/bloom.wgsl`, `echo.wgsl`, `crt.wgsl`, `particles.wgsl` — post-process + particle shaders with their own uniform structs (NOT pinned to `VisualizerConfig`). `bloom` = bright-pass → separable Gaussian blur → additive composite; `echo` = Milkdrop zoom/rotate feedback; `crt` = chromatic-aberration/scanline/vignette/grain/beat-punch composite (no screen curvature); `particles` = instanced additive glowing quads for the Scope dust field

**Render path:** non-MSAA fast path by default; switches to **4× MSAA → resolve → blit** when perspective lean is active (the `has_perspective` flag on `VisualizerPrimitive`, set from `bar_depth_3d > 0.001`, gates the path per-frame).

`VisualizationMode` enum (`data/src/types/player_settings/visualizer.rs`): `Off`, `Bars`, `Lines`, `Scope` (cycled by the player-bar toggle).  `MIN_BAR_COUNT = 4`; bar width interpolates between `bar_width_min` and `bar_width_max` over a 400→2560px window range.

**Placement.** Bars and Lines each carry a `VisualizerPlacement` (`OverCover` default / `BottomBand`) — a per-mode field on `BarsConfig` / `LinesConfig`, same `wire_enum!` convention as the others. `BottomBand` draws a band above the player bar (every view); `OverCover` draws over the now-playing cover art in the Queue, while playing — the slot the Scope ring uses (Scope is always over-cover, with no placement of its own). Over the cover, Bars/Lines honor the `Visualizer Height` setting (`height_percent`) as a bottom-anchored fraction of the cover height; Scope fills the panel (its ring sizes off `scope.radius`). `widgets::visualizer::resolve_placement(mode, bars_placement, lines_placement) -> VisualizerSlots { bottom_band, over_art }` is the single source of truth for the render fork (the two slots are mutually exclusive); `app_view` calls it for both the bottom-band overlay and the over-cover (`single_artwork_panel_inner`) render sites. The surfing boat rides the Lines wave in either placement — the bottom band (`app_view`) or over the cover (the Queue artwork panel, via `OverCoverBoat`).

## Bars Mode

Mode enums live in `data/src/types/visualizer_config.rs` as `wire_enum!` invocations (explicit per-variant wire literals tied to per-variant `#[serde(rename)]`, explicit `#[repr(u32)]` discriminants; the macro generates `ALL` / `as_wire_str` / `all_wire_strs` / `from_wire_str` (tolerant Default fallback) / `as_u32`; declaration order = settings-dropdown display order). The settings dropdowns derive their option lists from `all_wire_strs()` via each entry's `ui_meta.options`; enum dispatch parses with `from_wire_str` (visualizer dropdowns key on WIRE strings, unlike the `from_label` tabs). Pin tests assert each `ALL` carries every variant exactly once in declaration order. Enums:
- `BarsPeakMode`: `None` / `Fade` / `Fall` / `FallAccel` / `FallFade`. `peak_fall_speed` 1–20.
- `BarsGradientMode`: `Static` (0) / `Wave` (2). **Discriminant `1` is intentionally skipped** — `bars.wgsl` has no branch for it; the `bars_gradient_mode_never_emits_dead_1u` test in `data/src/types/visualizer_config.rs` pins this against accidental future use. (Shimmer/Energy/Alternate were dropped in b92d311; the glow/bloom/beat effects supersede them.)
- `BarsGradientOrientation`: `Vertical` (within-bar) / `Horizontal` (bass → treble across bars).
- `BarsPeakGradientMode`: `Static` / `Cycle` / `Height` / `Match` (separate enum from bar gradients).

## Lines Mode

- `LinesStyle`: `Smooth` (Catmull-Rom spline) / `Angular` (straight segments).
- `LinesGradientMode`: `Breathing` / `Static` / `Position` / `Height` / `Gradient`. Reuses bar gradient palette.

6-instance render (`instance_index` in `lines.wgsl`): fill / outline / main × normal / mirror. Per-vertex coloring, optional fill / mirror.

**Stroke + glow (a7ab6efe).** The stroke is an analytic **SDF over miter-tiled per-segment quads** (`TriangleList`): adjacent quads meet on the join bisector so they tile with no overlap or gap, and coverage is `sd_segment(frag_pos, seg_a, seg_b)` — a true distance, so round joins/caps are free and the strip can never fold (the old fixed-width triangle-strip ribbon spiked at acute bends). The neon halo is NOT an in-shader analytic halo (those spike/facet at sharp tips) — the shader draws only the thin crisp core and the glow is the separable-blur **bloom post-process** (`bloom.wgsl`) re-purposed for the stroke (wider blur, near-zero brightness cutoff so the whole thin line glows evenly), driven by `glow_intensity` independently of the user's global Bloom toggle. Scope ports the identical stroke to its closed ring (wrapping via `ring_point`, raw asymmetric waveform preserved).

Key settings: `point_count` (8–512), `line_thickness`, `outline_thickness`, `fill_opacity`, `glow_intensity`, `mirror`, `style`, `boat` (toggle the surfing-boat overlay; `widgets/boat.rs`, CPU-only, themed via `embedded_svg::themed_boat_svg()` using `border_color`).

## Scope Mode

Circular oscilloscope: a time-domain waveform plotted as a closed ring over the now-playing cover (Queue, while playing — no placement of its own). `ScopeConfig` reuses `LinesGradientMode` / `LinesStyle` and the same SDF stroke + bloom-glow path, plus ring-only geometry. `tick()` snapshots the raw PCM chunk (asymmetric, untouched — design intent is waveform purity) instead of the FFT.

Settings (`point_count` 16–512, default 16; `radius`, `sensitivity`, `line_thickness`, `fill_opacity`, `glow_intensity`, `outline_thickness`/`outline_opacity`, `gradient_mode`, `animation_speed`, `style`, `particles` + `particle_count` (0–2048) + `particle_speed`, `beam`, `trails`, `echo`). The `gradient_mode` default is `LinesGradientMode::Height` (052c19ea, reads better over cover art); `beam` (additive woscope-style ring) and `particles` default on, `echo` defaults to `1.0`.

## Configuration

- Behavior under `[visualizer]`, `[visualizer.bars]`, `[visualizer.lines]`, `[visualizer.scope]` in `config.toml`
- Colors under `[dark.visualizer]` / `[light.visualizer]` in active theme file: `bar_gradient_colors`, `peak_gradient_colors`, `border_color`, `border_opacity`, `led_border_opacity`

## Configuration pipeline (M3 unified)

- Pure types (`VisualizerConfig` + Bars/Lines/Scope + the 7 `wire_enum!` enums + `validate()` + the `keys` module incl. `keys::ALL_KEYS`) live in `data/src/types/visualizer_config.rs`; the UI residue (`ThemeBarColors`, `SharedVisualizerConfig` + `apply`/`snapshot`, `ConfigWatcher`) stays in `src/visualizer_config.rs` with a glob re-export.
- Persistence is config.toml `[visualizer]` ONLY — never redb (`persisted_player_settings_json_has_no_visualizer_key` pins it). `SettingsManager` holds an in-memory `visualizer` field (startup phase 1 + `reload_from_toml` via `read_toml_visualizer`), mirrored wholesale onto `LivePlayerSettings.visualizer`.
- Dispatch + UI rows come from the `Tab::Visualizer` `define_settings!` table (`data/src/services/settings_tables/visualizer.rs`): setters mutate `mgr.with_visualizer(...)` (validate rides along); each entry's `ui_meta` builds its row (`build_visualizer_tab_settings_items` consumed by `items_visualizer.rs` via `MacroRows`). Several f32 config fields surface as INT pixel rows — `every_visualizer_macro_row_dispatches` pins row↔dispatch type agreement.
- Writes: `handle_settings_write_config` does the surgical per-key config.toml write (color sub-tables survive — never whole-section on this path), then routes through `dispatch_visualizer_tab_setting` → `PlayerSettingsLoaded` (which pushes the shared render config, change-gated). monstercat↔waves exclusivity lives in the data-crate setters AND writes BOTH keys to config.toml (`visualizer_exclusivity_companion`) — config.toml wins on reload.

## Runtime

- Hot-reload: config watcher → `SettingsConfigReloaded` → `reload_from_toml` (re-reads `[visualizer]`) → `PlayerSettingsLoaded` → shared config apply (the standalone `VisualizerConfigChanged` message is gone)
- Dirty-flag gated redraws: `is_dirty()` / `clear_dirty()` — GPU idle when paused
- `apply_config()` sets `pending_engine_reinit` — FFT thread picks it up, prevents stutter
- Resize debouncing: 100ms for bar count changes
- FFT thread uses `try_lock()` only on the audio engine — main render thread alone may use `lock()`
