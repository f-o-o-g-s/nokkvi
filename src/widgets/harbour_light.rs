//! Per-pixel night light for the Harbour Trawl scene.
//!
//! The canvas scene in [`super::harbour_sea`] draws in flat fills and stepped
//! gradients, which cannot carry light the way the `nokkvi - aurora` MilkDrop
//! preset does. At night this shader draws the scene's light underneath the
//! canvas: the sky and the aurora curtain, the far swell, the near water
//! (light shafts under the curtain, the surface seen from below, glowing
//! plankton and currents) and the seabed with its moving net of light. The
//! canvas keeps everything that is an object (stars, moon glow, kelp, fish,
//! crate, anchor, rope) and the boat sprite sits on top of both.
//!
//! The shader has no feedback buffer: every term is a function of the pixel
//! and the scene clock (`harbour_light.wgsl`). The two waterlines arrive as
//! sample arrays built by [`super::harbour_sea::sea_light`] from the SAME
//! sampler the boat physics steps against, so the hull rides exactly the
//! surface lit here.

use iced::{
    Element, Length, Rectangle, Widget as _, mouse, wgpu,
    widget::shader::{self, Viewport},
};

use crate::widgets::visualizer::{
    milkdrop::palette::PresetPalette,
    state::{SCENE_BANDS, SceneMusic},
};

/// Samples per waterline handed to the shader (4 per `vec4`).
pub(crate) const LINE_SAMPLES: usize = 128;
const LINE_VEC4S: usize = LINE_SAMPLES / 4;
const _: () = assert!(LINE_SAMPLES.is_multiple_of(4));
const _: () = assert!(SCENE_BANDS.is_multiple_of(4));
/// Star slots in the uniform; the constellation must fit.
pub(crate) const MAX_STARS: usize = 64;
/// Glow-point slots (kelp beads, notes, the anchor's glint).
pub(crate) const MAX_GLOWS: usize = 48;
/// Bubble slots (the anchor's stream and the kelp seeps).
pub(crate) const MAX_BUBBLES: usize = 32;
/// The seabed props' fixed slots (`props` in the uniform). Positions are x
/// across (0..1); sizes and heights in panel heights.
/// - `PROP_ROCKS..+3`: rock mounds (x, half width, height, seed)
/// - `PROP_STARFISH`: (x, arm, rotation, 0)
/// - `PROP_SHIELD`: the sunken shield (x, size, tilt radians, 0)
/// - `PROP_KELP..+KELP_SLOTS`: fronds (root x, height, tip reach, 1 = present)
/// - `PROP_ANCHOR`: the trawled anchor's shadow (x, half width, 0, strength)
pub(crate) const PROP_ROCKS: usize = 0;
pub(crate) const PROP_STARFISH: usize = 3;
pub(crate) const PROP_SHIELD: usize = 4;
pub(crate) const PROP_KELP: usize = 5;
pub(crate) const KELP_SLOTS: usize = 8;
pub(crate) const PROP_ANCHOR: usize = PROP_KELP + KELP_SLOTS;
pub(crate) const MAX_PROPS: usize = 16;
const _: () = assert!(PROP_ANCHOR < MAX_PROPS);

/// What the scene hands the shader each frame.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SeaLight {
    /// Front waterline heights (fraction of the panel height above its
    /// bottom), `LINE_SAMPLES` points evenly across the width.
    pub front: [f32; LINE_SAMPLES],
    /// The far swell's crest heights, same layout.
    pub back: [f32; LINE_SAMPLES],
    /// The scene clock in seconds (continuous across phase wraps).
    pub time: f32,
    /// The boat's `x_ratio` and waterline height, for the glow behind it;
    /// `None` hides the glow (boat out of view).
    pub boat: Option<(f32, f32)>,
    /// The night stars: x (0..1 across), height above the bottom and radius
    /// (both in panel heights; a negative radius marks a sparkle), alpha.
    pub stars: [[f32; 4]; MAX_STARS],
    pub star_count: usize,
    /// The moon's centre (x across, height above bottom), radius (panel
    /// heights) and breath, for its bloom; `None` when the moon is hidden.
    pub moon: Option<[f32; 4]>,
    /// Soft glow points: x, height, radius (panel heights), intensity; a
    /// positive intensity glows starlight, a negative one bioluminescent.
    pub glows: [[f32; 4]; MAX_GLOWS],
    pub glow_count: usize,
    /// Bubbles drawn as glassy spheres: x, height, radius, alpha.
    pub bubbles: [[f32; 4]; MAX_BUBBLES],
    pub bubble_count: usize,
    /// The music the light follows this frame.
    pub music: HarbourMusic,
    /// The day scene (light themes) instead of the night.
    pub day: bool,
    /// The seabed props, in the `PROP_*` slots.
    pub props: [[f32; 4]; MAX_PROPS],
}

/// How the night scene follows the music: the aurora's rays reach with a
/// smoothed coarse spectrum, and each kick launches a surge of light that
/// sweeps along the curtain, alternating sides (the `nokkvi - aurora`
/// preset's own moves). Light only: the sea's sway and the boat stay calm.
/// With nothing playing every input is zero and the scene settles to its
/// quiet look.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HarbourMusic {
    /// Smoothed spectrum, bass first, 0..1.
    pub reach: [f32; SCENE_BANDS],
    /// The surge's strength, 1 at a kick and decaying.
    pub surge: f32,
    /// Where the surge is, across the panel (runs past both edges).
    pub surge_x: f32,
    /// A fast beat pulse (jumps with each kick, falls over ~`PULSE_TAU`):
    /// the stars brighten and swell on it.
    pub pulse: f32,
    surge_dir: f32,
    cooldown: f32,
    /// The kick fell back below the re-arm level since the last surge.
    armed: bool,
}

impl Default for HarbourMusic {
    fn default() -> Self {
        Self {
            reach: [0.0; SCENE_BANDS],
            surge: 0.0,
            surge_x: 0.5,
            pulse: 0.0,
            surge_dir: -1.0,
            cooldown: 0.0,
            armed: true,
        }
    }
}

// TUNE: reach follows the spectrum over ~REACH_TAU; a kick above
// KICK_TRIGGER (after falling under KICK_REARM, at most once per
// SURGE_COOLDOWN) launches a surge crossing at SURGE_SPEED panels/s that
// fades over SURGE_TAU.
const REACH_TAU: f32 = 0.18;
const KICK_TRIGGER: f32 = 0.6;
const KICK_REARM: f32 = 0.35;
const SURGE_COOLDOWN: f32 = 0.25;
const SURGE_SPEED: f32 = 1.6;
const SURGE_TAU: f32 = 0.45;
const PULSE_TAU: f32 = 0.18;

impl HarbourMusic {
    /// Advance by `dt` seconds toward the music `now`.
    pub(crate) fn step(&mut self, now: SceneMusic, dt: f32) {
        let ease = 1.0 - (-dt / REACH_TAU).exp();
        for (r, target) in self.reach.iter_mut().zip(now.spectrum) {
            *r += (target - *r) * ease;
        }
        self.cooldown = (self.cooldown - dt).max(0.0);
        if now.kick < KICK_REARM {
            self.armed = true;
        }
        if self.armed && self.cooldown <= 0.0 && now.kick > KICK_TRIGGER {
            self.surge_dir = -self.surge_dir;
            self.surge_x = if self.surge_dir > 0.0 { -0.1 } else { 1.1 };
            self.surge = 1.0;
            self.cooldown = SURGE_COOLDOWN;
            self.armed = false;
        }
        self.surge_x += SURGE_SPEED * self.surge_dir * dt;
        self.surge *= (-dt / SURGE_TAU).exp();
        self.pulse = (self.pulse * (-dt / PULSE_TAU).exp()).max(now.kick.clamp(0.0, 1.0));
    }

    /// The overall level, 0..1.
    pub(crate) fn level(&self) -> f32 {
        self.reach.iter().sum::<f32>() / SCENE_BANDS as f32
    }
}

/// The night scene's colours for the canvas furniture drawn over the
/// shader, from the same theme-only palette and the same ramp positions
/// `harbour_light.wgsl` lights the scene with (`lc` = ramp 0.85, `bio` =
/// ramp 0.7 for the shader's beads, `starc` = mix(lc, text, 0.6)), so a kelp edge, a fish's back
/// and the water around them are lit by one light.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NightInk {
    /// Underwater silhouettes: the night background, barely lifted by the
    /// light.
    pub silhouette: iced::Color,
    /// The aurora's light catching an edge (`lc`).
    pub rim: iced::Color,
    /// Starlight (`starc`): notes, highlights.
    pub starlight: iced::Color,
}

impl NightInk {
    pub(crate) fn from_theme() -> Self {
        let p = PresetPalette::theme_own();
        let mix = |a: [f32; 3], b: [f32; 3], t: f32| {
            iced::Color::from_rgb(
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            )
        };
        let lc = p.ramp_at(0.85);
        Self {
            // From the background and the light only: the ramp's ends are
            // theme-ordered (some run light to dark), so no ramp end is
            // assumed dark.
            silhouette: mix(p.bg.map(|c| c * 0.7), lc, 0.07),
            rim: mix(lc, lc, 1.0),
            starlight: mix(lc, p.text, 0.6),
        }
    }
}

/// The GPU uniform. Mirrors `struct Scene` in `harbour_light.wgsl` field for
/// field; `scene_uniform_matches_wgsl_layout` pins the size and the WGSL
/// field names against this declaration.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct SceneUniform {
    /// Panel width px, panel height px, scene clock s, decode-to-linear flag.
    frame: [f32; 4],
    /// Boat x (0..1), boat centre height, boat height (both in panel
    /// heights), shown flag.
    boat: [f32; 4],
    /// Moon x, height, radius (panel heights), breath (0 = no moon).
    moon: [f32; 4],
    /// Star, glow and bubble counts, unused.
    sky: [f32; 4],
    /// Music level, surge strength, surge x (0..1 across), beat pulse.
    music: [f32; 4],
    /// The smoothed spectrum, bass first.
    spectrum: [[f32; 4]; SCENE_BANDS / 4],
    /// 1 = the day scene, unused ×3.
    mode: [f32; 4],
    /// The logo's shield colour (the sunken shield's paint).
    shield: [f32; 4],
    bg: [f32; 4],
    text: [f32; 4],
    highlight: [f32; 4],
    warm: [f32; 4],
    ramp: [[f32; 4]; 6],
    front: [[f32; 4]; LINE_VEC4S],
    back: [[f32; 4]; LINE_VEC4S],
    stars: [[f32; 4]; MAX_STARS],
    glows: [[f32; 4]; MAX_GLOWS],
    bubbles: [[f32; 4]; MAX_BUBBLES],
    props: [[f32; 4]; MAX_PROPS],
}

// SAFETY: `repr(C)`, every field an `f32` array, no padding (all 16-byte
// rows), so any bit pattern is valid and the bytes are fully initialized.
unsafe impl bytemuck::Pod for SceneUniform {}
unsafe impl bytemuck::Zeroable for SceneUniform {}

const _: () = assert!(
    std::mem::size_of::<SceneUniform>()
        == 16
            * (11
                + SCENE_BANDS / 4
                + 6
                + 2 * LINE_VEC4S
                + MAX_STARS
                + MAX_GLOWS
                + MAX_BUBBLES
                + MAX_PROPS)
);

const WGSL: &str = include_str!("harbour_light.wgsl");

fn rgba(c: [f32; 3]) -> [f32; 4] {
    [c[0], c[1], c[2], 1.0]
}

fn pack(line: &[f32; LINE_SAMPLES]) -> [[f32; 4]; LINE_VEC4S] {
    std::array::from_fn(|i| {
        [
            line[4 * i],
            line[4 * i + 1],
            line[4 * i + 2],
            line[4 * i + 3],
        ]
    })
}

/// The night-light backdrop at the scene's size. Event-transparent.
pub(crate) fn light_backdrop<'a, M: 'a>(light: SeaLight, w: f32, h: f32) -> Element<'a, M> {
    iced::widget::shader(LightProgram {
        light,
        palette: if light.day {
            day_palette()
        } else {
            PresetPalette::theme_own()
        },
    })
    .width(Length::Fixed(w))
    .height(Length::Fixed(h))
    .boxed()
}

/// The day scene's palette: the active (light) background and ink, the
/// accent, the logo's gold for the sun, and the theme's own ramp for the
/// water's hues.
fn day_palette() -> PresetPalette {
    use crate::theme;
    let rgb = |c: iced::Color| [c.r, c.g, c.b];
    PresetPalette {
        bg: rgb(theme::bg0_hard()),
        text: rgb(theme::fg0()),
        highlight: rgb(theme::accent()),
        warm: rgb(theme::logo_wood()),
        light: true,
        ..PresetPalette::theme_own()
    }
}

struct LightProgram {
    light: SeaLight,
    palette: PresetPalette,
}

impl<Message> shader::Program<Message> for LightProgram {
    type State = ();
    type Primitive = LightPrimitive;

    fn draw(&self, _state: &(), _cursor: mouse::Cursor, bounds: Rectangle) -> LightPrimitive {
        let (w, h) = (bounds.width.max(1.0), bounds.height.max(1.0));
        let boat = match self.light.boat {
            Some((x, waterline)) => {
                // Same sizing as `boat_overlay` (basis = the shorter side):
                // the sprite's centre sits above the waterline by the part
                // of the hull that floats.
                let (_, boat_h) = crate::widgets::boat::boat_pixel_size(w.min(h));
                let rel = boat_h / h;
                [
                    x,
                    waterline + (0.5 - crate::widgets::boat::BOAT_SINK_FRACTION) * rel,
                    rel,
                    1.0,
                ]
            }
            None => [0.0; 4],
        };
        let p = &self.palette;
        LightPrimitive {
            uniform: SceneUniform {
                frame: [w, h, self.light.time, 0.0],
                boat,
                moon: self.light.moon.unwrap_or([0.0; 4]),
                sky: [
                    self.light.star_count.min(MAX_STARS) as f32,
                    self.light.glow_count.min(MAX_GLOWS) as f32,
                    self.light.bubble_count.min(MAX_BUBBLES) as f32,
                    0.0,
                ],
                music: [
                    self.light.music.level(),
                    self.light.music.surge,
                    self.light.music.surge_x,
                    self.light.music.pulse,
                ],
                spectrum: std::array::from_fn(|i| {
                    std::array::from_fn(|j| self.light.music.reach[4 * i + j])
                }),
                mode: [if self.light.day { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
                shield: {
                    let c = crate::theme::logo_shields();
                    [c.r, c.g, c.b, 1.0]
                },
                bg: rgba(p.bg),
                text: rgba(p.text),
                highlight: rgba(p.highlight),
                warm: rgba(p.warm),
                ramp: p.ramp.map(rgba),
                front: pack(&self.light.front),
                back: pack(&self.light.back),
                stars: self.light.stars,
                glows: self.light.glows,
                bubbles: self.light.bubbles,
                props: self.light.props,
            },
        }
    }
}

#[derive(Debug)]
pub(crate) struct LightPrimitive {
    uniform: SceneUniform,
}

impl shader::Primitive for LightPrimitive {
    type Pipeline = LightPipeline;

    fn prepare(
        &self,
        pipeline: &mut LightPipeline,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &Viewport,
    ) {
        // One Trawl scene is on screen at a time, so one buffer serves.
        let mut uniform = self.uniform;
        uniform.frame[3] = if pipeline.decode_srgb { 1.0 } else { 0.0 };
        queue.write_buffer(&pipeline.uniform, 0, bytemuck::bytes_of(&uniform));
    }

    fn draw(&self, pipeline: &LightPipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        render_pass.set_pipeline(&pipeline.pipeline);
        render_pass.set_bind_group(0, &pipeline.bind_group, &[]);
        render_pass.draw(0..3, 0..1);
        true
    }
}

pub(crate) struct LightPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    /// iced's surface is normally non-sRGB (display-space values pass
    /// straight through, as the MilkDrop blit relies on); an sRGB target
    /// would brighten them, so the shader decodes first.
    decode_srgb: bool,
}

impl shader::Pipeline for LightPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("harbour light shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(WGSL)),
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("harbour light uniform"),
            size: std::mem::size_of::<SceneUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("harbour light bind group layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("harbour light bind group"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("harbour light pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("harbour light pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            pipeline,
            bind_group,
            uniform,
            decode_srgb: format.is_srgb(),
        }
    }
}

#[cfg(test)]
mod tests {
    use iced::wgpu::naga;

    use super::*;

    /// wgpu validates the WGSL at first paint and panics on an error; this
    /// runs the same naga parse + validation so a shader typo fails tests.
    #[test]
    fn harbour_light_wgsl_compiles() {
        let module = naga::front::wgsl::parse_str(WGSL).unwrap_or_else(|e| {
            panic!("{}", e.emit_to_string_with_path(WGSL, "harbour_light.wgsl"))
        });
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{}", e.emit_to_string_with_path(WGSL, "harbour_light.wgsl")));
    }

    /// `bytemuck::Pod` turns a Rust/WGSL drift into silent reinterpretation:
    /// pin the WGSL struct's size (as naga lays it out) and its field order.
    #[test]
    fn scene_uniform_matches_wgsl_layout() {
        let module = naga::front::wgsl::parse_str(WGSL).unwrap_or_else(|e| panic!("{e}"));
        let mut layouter = naga::proc::Layouter::default();
        layouter
            .update(module.to_ctx())
            .unwrap_or_else(|e| panic!("{e}"));
        let (handle, ty) = module
            .types
            .iter()
            .find(|(_, ty)| ty.name.as_deref() == Some("Scene"))
            .unwrap_or_else(|| panic!("harbour_light.wgsl lost `struct Scene`"));
        assert_eq!(
            layouter[handle].size as usize,
            std::mem::size_of::<SceneUniform>(),
            "WGSL `Scene` and Rust `SceneUniform` sizes differ"
        );
        let naga::TypeInner::Struct { members, .. } = &ty.inner else {
            panic!("`Scene` is not a struct");
        };
        let names: Vec<_> = members.iter().filter_map(|m| m.name.as_deref()).collect();
        assert_eq!(
            names,
            [
                "frame",
                "boat",
                "moon",
                "sky",
                "music",
                "spectrum",
                "mode",
                "shield",
                "bg",
                "text",
                "highlight",
                "warm",
                "ramp",
                "front",
                "back",
                "stars",
                "glows",
                "bubbles",
                "props"
            ],
            "WGSL `Scene` fields drifted from `SceneUniform`"
        );
    }

    fn music(level: f32, kick: f32) -> SceneMusic {
        SceneMusic {
            spectrum: [level; SCENE_BANDS],
            kick,
        }
    }

    #[test]
    fn music_reach_follows_the_spectrum_and_settles_in_silence() {
        let mut m = HarbourMusic::default();
        for _ in 0..120 {
            m.step(music(0.8, 0.0), 1.0 / 60.0);
        }
        assert!((m.level() - 0.8).abs() < 0.01, "reach eases to the music");
        for _ in 0..120 {
            m.step(SceneMusic::default(), 1.0 / 60.0);
        }
        assert!(m.level() < 0.01, "silence settles the curtain");
    }

    #[test]
    fn a_kick_launches_one_surge_and_sides_alternate() {
        let mut m = HarbourMusic::default();
        m.step(music(0.5, 0.9), 1.0 / 60.0);
        assert!(m.surge > 0.9, "a kick launches a surge");
        let first_dir_right = m.surge_x < 0.5;
        // Held high, the kick does not retrigger.
        for _ in 0..30 {
            m.step(music(0.5, 0.9), 1.0 / 60.0);
        }
        assert!(m.surge < 0.6, "a held kick only decays the surge");
        // Released and struck again: the next surge runs the other way.
        m.step(music(0.5, 0.0), 1.0 / 60.0);
        m.step(music(0.5, 0.9), 1.0 / 60.0);
        assert!(m.surge > 0.9);
        assert_ne!(m.surge_x < 0.5, first_dir_right, "surges alternate sides");
    }

    #[test]
    fn the_beat_pulse_jumps_with_a_kick_and_falls_away() {
        let mut m = HarbourMusic::default();
        m.step(music(0.5, 0.9), 1.0 / 60.0);
        assert!(m.pulse > 0.85, "a kick lights the pulse");
        for _ in 0..60 {
            m.step(music(0.5, 0.0), 1.0 / 60.0);
        }
        assert!(m.pulse < 0.01, "the pulse falls away between kicks");
    }

    #[test]
    fn surges_respect_the_cooldown() {
        let mut m = HarbourMusic::default();
        m.step(music(0.5, 0.9), 0.01);
        let x = m.surge_x;
        m.step(music(0.5, 0.0), 0.01);
        m.step(music(0.5, 0.9), 0.01);
        assert!(
            (m.surge_x - x).abs() < 0.1,
            "a kick inside the cooldown does not relaunch"
        );
    }

    #[test]
    fn pack_keeps_sample_order() {
        let line: [f32; LINE_SAMPLES] = std::array::from_fn(|i| i as f32);
        let packed = pack(&line);
        assert_eq!(packed[0], [0.0, 1.0, 2.0, 3.0]);
        assert_eq!(packed[LINE_VEC4S - 1][3], (LINE_SAMPLES - 1) as f32);
    }
}
