---
paths:
  - "src/widgets/visualizer/milkdrop/**"
  - "src/update/milkdrop*.rs"
  - "src/state/milkdrop.rs"
  - "data/src/services/milkdrop_presets.rs"
  - "src/visualizer_config.rs"
  - "data/src/types/visualizer_config.rs"
  - "data/src/services/settings_tables/visualizer.rs"
  - "Cargo.toml"
---

# Gotchas: MilkDrop

- **iced's device takes `wgpu::Limits::default()`** (`iced_wgpu` compositor; 4 bind groups since iced-rs/iced#3457 dropped its old `max_bind_groups: 2` cap). The upstream engine's custom-warp pipeline needed 3, which the fork pin still squeezes into 2. A new engine pin must pass the ignored `bundled_pack_builds_on_icedlike_limits` test on a real GPU before it ships; `iced_like_gpu()` mirrors the compositor's limits, so keep it in step with iced. nokkvi's own blit uses two groups (incoming + outgoing for the preset crossfade).
- **The blit shader and its pipeline layout change in one commit.** naga passes a shader that declares `@group(1)` against a one-group layout; `create_render_pipeline` then panics at first paint. The ignored `blit_pipeline_builds_on_icedlike_limits` test (GPU) catches the mismatch; run it after any `blit.wgsl` or layout edit.
- **`MilkdropRenderer` is `Send` but not `Sync`** (a `RefCell` in its EEL program), so it lives in a `parking_lot::Mutex` inside iced's pipeline storage. Only `prepare` and `trim` touch it.
- **Never hand iced's target to the engine**: `render(view)` clears the whole view it gets. Use `render_to_retained_comp()` + the blit. The retained view is REPLACED on every resize, so rebuild the bind group after `try_resize`.
- **Downgrade hazard**: a pre-MilkDrop binary cannot parse `visualization_mode = "milkdrop"`. It resets the whole `[settings]` section of `config.toml` AND the redb settings blob to defaults. Switch the mode off MilkDrop before running an older build.
