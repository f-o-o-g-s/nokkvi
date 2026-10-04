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
    Element, Length, Rectangle, mouse, wgpu,
    widget::shader::{self, Viewport},
};

use crate::widgets::visualizer::milkdrop::palette::PresetPalette;

/// Samples per waterline handed to the shader (4 per `vec4`).
pub(crate) const LINE_SAMPLES: usize = 128;
const LINE_VEC4S: usize = LINE_SAMPLES / 4;
const _: () = assert!(LINE_SAMPLES.is_multiple_of(4));
/// Star slots in the uniform; the constellation must fit.
pub(crate) const MAX_STARS: usize = 64;
/// Glow-point slots (kelp beads, notes, the anchor's glint).
pub(crate) const MAX_GLOWS: usize = 48;
/// Bubble slots (the anchor's stream and the kelp seeps).
pub(crate) const MAX_BUBBLES: usize = 32;

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
}

// SAFETY: `repr(C)`, every field an `f32` array, no padding (all 16-byte
// rows), so any bit pattern is valid and the bytes are fully initialized.
unsafe impl bytemuck::Pod for SceneUniform {}
unsafe impl bytemuck::Zeroable for SceneUniform {}

const _: () = assert!(
    std::mem::size_of::<SceneUniform>()
        == 16 * (8 + 6 + 2 * LINE_VEC4S + MAX_STARS + MAX_GLOWS + MAX_BUBBLES)
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
        palette: PresetPalette::theme_own(),
    })
    .width(Length::Fixed(w))
    .height(Length::Fixed(h))
    .into()
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
                "bg",
                "text",
                "highlight",
                "warm",
                "ramp",
                "front",
                "back",
                "stars",
                "glows",
                "bubbles"
            ],
            "WGSL `Scene` fields drifted from `SceneUniform`"
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
