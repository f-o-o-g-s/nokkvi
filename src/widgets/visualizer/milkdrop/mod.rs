//! MilkDrop mode: plays MilkDrop presets through the `particle-milkdrop` engine.
//!
//! Three threads meet in [`MilkdropShared`]:
//! - the FFT worker feeds a `particle_audio::Analyzer` from the same visualizer
//!   tap the other modes use (`VisualizerState::tick`) and publishes its
//!   latest `Features`;
//! - a blocking task parses + compiles a preset ([`compile_preset`]) and then
//!   builds its renderer on a clone of iced's device ([`build_renderer`]);
//! - the render thread swaps a finished renderer in, advances it at most 60
//!   times a second while playing, and blits its retained composite
//!   ([`program`]).

pub(crate) mod palette;
mod program;
pub(crate) mod shared;

use std::{sync::Arc, time::Duration};

use iced::wgpu;
use particle_milkdrop::{CompiledMilkdropShaderBodies, MilkShaders, MilkdropRenderer};
pub(crate) use program::MilkdropProgram;
pub(crate) use shared::{BuiltPreset, GpuHandles, MilkdropShared};

include!(concat!(env!("OUT_DIR"), "/milkdrop_presets_generated.rs"));

/// The engine's EEL `fps` is 60 and presets decay per frame, so the renderer
/// advances on a 1/60 s schedule whatever the display's refresh rate.
pub(crate) const MILKDROP_FRAME_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 60);

/// Smallest side the renderer is ever built or resized to.
const MIN_RENDER_SIDE: u32 = 64;

const BLIT_WGSL: &str = include_str!("blit.wgsl");

/// Render size for a panel of `physical` px: the shorter side is capped at `cap`
/// (0 = native, never upscaled) and the blit scales the result to the panel.
pub(crate) fn desired_render_size(physical: (u32, u32), cap: u32) -> (u32, u32) {
    let (w, h) = physical;
    let short = w.min(h);
    let scale = if cap == 0 || short == 0 {
        1.0
    } else {
        (cap as f64 / short as f64).min(1.0)
    };
    let side = |v: u32| ((v as f64 * scale).round() as u32).max(MIN_RENDER_SIDE);
    (side(w), side(h))
}

/// Run `f` under Validation + OutOfMemory error scopes. wgpu errors never come
/// back as `Err` from the engine; without a scope they reach the device's
/// default handler, which panics the app. The guards are `!Send`, so push and
/// pop happen here on one thread; on native the pop future is already ready.
pub(crate) fn run_in_error_scopes<T>(
    device: &wgpu::Device,
    f: impl FnOnce() -> T,
) -> Result<T, String> {
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let out = f();
    let oom = futures::executor::block_on(oom.pop());
    let validation = futures::executor::block_on(validation.pop());
    match (validation, oom) {
        (None, None) => Ok(out),
        (Some(e), _) | (None, Some(e)) => Err(e.to_string()),
    }
}

/// Feed one analysis frame to the engine, exactly as the standalone player the
/// owner watched does (a straight field copy).
pub(crate) fn apply_features(renderer: &mut MilkdropRenderer, f: &particle_audio::Features) {
    renderer.set_audio(f.bass_react, f.mid_react, f.treb_react, f.vol_react);
    renderer.set_audio_att(
        f.bass_react_att,
        f.mid_react_att,
        f.treb_react_att,
        f.vol_react_att,
    );
    renderer.set_waveform(&f.waveform_left_full, &f.waveform_right_full);
    renderer.set_freq_spectrum(&f.freq_spectrum);
}

/// A parsed preset with its shaders already translated (the naga work), ready
/// for a device.
pub struct CompiledPreset {
    pub name: String,
    /// The source names theme colours (`NOKKVI_*`); a palette change reloads it.
    pub themed: bool,
    pub shaders: MilkShaders,
    pub bodies: CompiledMilkdropShaderBodies,
}

// Manual: `MilkShaders` derives nothing; print the name only.
impl std::fmt::Debug for CompiledPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledPreset")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// Fill a Butterchurn JSON preset's theme placeholders from `palette`, parse
/// it and translate its shaders. Pure CPU, no device; run it off the UI thread.
pub(crate) fn compile_preset(
    name: String,
    json: &str,
    palette: &palette::PresetPalette,
) -> Result<CompiledPreset, String> {
    let themed = palette::uses_theme(json);
    let json = palette.substitute(json);
    let shaders = particle_milkdrop::load_preset_str(&json, true)?;
    let bodies = particle_milkdrop::compile_milkdrop_shader_bodies(&shaders)?;
    Ok(CompiledPreset {
        name,
        themed,
        shaders,
        bodies,
    })
}

/// Build a renderer for `preset` on `gpu`'s device, plus one warm-up frame, all
/// inside error scopes. Blocking (EEL init, textures, ~20 pipelines); run it on
/// a blocking thread, never the render thread.
pub(crate) fn build_renderer(
    gpu: &GpuHandles,
    size: (u32, u32),
    preset: &CompiledPreset,
) -> Result<MilkdropRenderer, String> {
    let (w, h) = (size.0.max(MIN_RENDER_SIDE), size.1.max(MIN_RENDER_SIDE));
    run_in_error_scopes(&gpu.device, || {
        MilkdropRenderer::new_with_compiled_pipeline_cache(
            Arc::clone(&gpu.device),
            Arc::clone(&gpu.queue),
            w,
            h,
            gpu.format,
            &preset.shaders,
            &preset.bodies,
            None,
        )
        .map(|mut renderer| {
            renderer.render_to_retained_comp();
            renderer
        })
    })?
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn desired_render_size_caps_the_short_side() {
        assert_eq!(desired_render_size((600, 600), 720), (600, 600));
        assert_eq!(desired_render_size((2560, 1440), 720), (1280, 720));
        assert_eq!(desired_render_size((1440, 2560), 720), (720, 1280));
        assert_eq!(
            desired_render_size((2560, 1440), 0),
            (2560, 1440),
            "0 = native"
        );
        assert_eq!(
            desired_render_size((10, 30), 720),
            (64, 64),
            "never below 64"
        );
        assert_eq!(desired_render_size((0, 0), 720), (64, 64));
    }

    #[test]
    fn blit_wgsl_compiles() {
        use iced::wgpu::naga;
        let module = naga::front::wgsl::parse_str(BLIT_WGSL).unwrap_or_else(|e| {
            panic!(
                "blit.wgsl: WGSL parse error\n{}",
                e.emit_to_string_with_path(BLIT_WGSL, "blit.wgsl")
            )
        });
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap_or_else(|e| {
            panic!(
                "blit.wgsl: WGSL validation error\n{}",
                e.emit_to_string_with_path(BLIT_WGSL, "blit.wgsl")
            )
        });
    }

    #[test]
    fn bundled_pack_is_nonempty_unique_and_parses() {
        assert!(BUNDLED_MILKDROP_PRESETS.len() > 100, "the pack ships whole");
        let mut seen = HashSet::new();
        for (name, json) in BUNDLED_MILKDROP_PRESETS {
            assert!(seen.insert(*name), "duplicate stem {name}");
            if let Err(e) = particle_milkdrop::load_preset_str(json, true) {
                panic!("{name}: {e}");
            }
        }
    }

    /// A device with the limits iced's compositor requests
    /// (`iced_wgpu::window::compositor`: `Limits::default()` with
    /// `max_bind_groups: 2`, `max_non_sampler_bindings: 2048`), so a preset
    /// that needs more than iced grants fails here instead of on screen.
    fn iced_like_gpu() -> Option<GpuHandles> {
        let instance = wgpu::Instance::default();
        let adapter = futures::executor::block_on(
            instance.request_adapter(&wgpu::RequestAdapterOptions::default()),
        )
        .ok()?;
        let limits = wgpu::Limits {
            max_bind_groups: 2,
            max_non_sampler_bindings: 2048,
            ..wgpu::Limits::default().using_resolution(adapter.limits())
        };
        let (device, queue) =
            futures::executor::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("nokkvi milkdrop test (iced limits)"),
                required_limits: limits,
                ..Default::default()
            }))
            .ok()?;
        Some(GpuHandles {
            device: Arc::new(device),
            queue: Arc::new(queue),
            format: wgpu::TextureFormat::Bgra8Unorm,
            epoch: 1,
        })
    }

    /// Needs a GPU: `cargo test -p nokkvi -- --ignored bundled_pack_builds`.
    /// Builds every bundled preset's renderer (plus a warm-up frame) on a
    /// device with iced's limits, under the same error scopes the app uses.
    #[test]
    #[ignore = "needs a GPU adapter; slow"]
    fn bundled_pack_builds_on_icedlike_limits() {
        let Some(gpu) = iced_like_gpu() else {
            panic!("no GPU adapter available");
        };
        let failures: Vec<String> = BUNDLED_MILKDROP_PRESETS
            .iter()
            .filter_map(|(name, json)| {
                let preset = match compile_preset(
                    (*name).to_string(),
                    json,
                    &palette::PresetPalette::from_theme(),
                ) {
                    Ok(p) => p,
                    Err(e) => return Some(format!("{name}: compile: {e}")),
                };
                build_renderer(&gpu, (256, 256), &preset)
                    .err()
                    .map(|e| format!("{name}: {}", e.replace('\n', " ")))
            })
            .collect();
        assert!(
            failures.is_empty(),
            "{} presets fail to build:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    /// Slow (naga over every preset): `cargo test -p nokkvi -- --ignored
    /// bundled_pack_compiles`. Lists every preset whose shaders fail to
    /// translate; drop those files from `assets/milkdrop/`.
    #[test]
    #[ignore = "slow; run by hand when the pack changes"]
    fn bundled_pack_compiles() {
        let failures: Vec<String> = BUNDLED_MILKDROP_PRESETS
            .iter()
            .filter_map(|(name, json)| {
                compile_preset(
                    (*name).to_string(),
                    json,
                    &palette::PresetPalette::from_theme(),
                )
                .err()
                .map(|e| format!("{name}: {}", e.lines().next().unwrap_or_default()))
            })
            .collect();
        assert!(
            failures.is_empty(),
            "{} presets fail to compile:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}
