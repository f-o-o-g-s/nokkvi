//! GPU pipeline initialization for the visualizer
//!
//! Extracted from shader.rs to keep pipeline setup (~400 lines of wgpu boilerplate)
//! separate from the render logic.

use iced::wgpu;

use super::shader::{
    BloomParams, CrtParams, EchoParams, TRAIL_FORMAT, Uniforms, VisualizerPipeline,
};

/// Build one of the four bars/lines × default/MSAA render pipelines.
///
/// The four sites in `VisualizerPipeline::new` differ only along four axes:
/// `topology` (TriangleList everywhere — bars, lines, scope), `msaa`
/// (default vs 4× MSAA), `shader` (bars.wgsl vs lines.wgsl module), and
/// `label` (for debug introspection). Everything else — pipeline layout,
/// vertex/fragment entry points, `ALPHA_BLENDING` blend state, color
/// target format, write mask, depth stencil, multiview mask, cache —
/// is identical, so the helper centralizes the wgpu boilerplate.
///
/// `vs_main` / `fs_main` are the entry points both shaders share.
// Plumbs the few axes that vary across the bars/lines/scope/particle pipelines;
// grouping them into a struct would only move the noise to the eight call sites.
#[allow(clippy::too_many_arguments)]
fn build_visualizer_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    topology: wgpu::PrimitiveTopology,
    msaa: bool,
    label: &'static str,
    format: wgpu::TextureFormat,
    blend: wgpu::BlendState,
) -> wgpu::RenderPipeline {
    let multisample = if msaa {
        wgpu::MultisampleState {
            count: 4,
            mask: !0,
            alpha_to_coverage_enabled: false,
        }
    } else {
        wgpu::MultisampleState::default()
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        primitive: wgpu::PrimitiveState {
            topology,
            ..Default::default()
        },
        depth_stencil: None,
        multisample,
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Build one of the fullscreen-triangle post-process pipelines (blit, trail
/// fade/max, bloom bright/blur/composite, echo feedback, CRT).
///
/// Every post-process pass shares the same descriptor skeleton — a single
/// fullscreen `TriangleList`, no depth/stencil, default (non-MSAA) multisample,
/// no vertex buffers, full `ColorWrites::ALL` mask, no multiview, no pipeline
/// cache — and differs only along `layout`, `shader`, the vertex/fragment
/// `entries`, `blend`, target `format`, and `label`. Centralizing the skeleton
/// here means a future change to it (pipeline caching, a primitive-state tweak,
/// a wgpu API rename) lands once instead of at eight call sites that could
/// silently diverge.
///
/// (`build_visualizer_pipeline` can't be reused for these — it hardcodes
/// `vs_main`/`fs_main` + `ALPHA_BLENDING` and carries the MSAA branch the
/// post-process passes never use.)
fn build_postprocess_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    entries: (&'static str, &'static str),
    blend: wgpu::BlendState,
    format: wgpu::TextureFormat,
    label: &'static str,
) -> wgpu::RenderPipeline {
    let (vs_entry, fs_entry) = entries;
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vs_entry),
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
            module: shader,
            entry_point: Some(fs_entry),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        multiview_mask: None,
        cache: None,
    })
}

impl VisualizerPipeline {
    pub(crate) const MAX_BARS: usize = 2048;
    /// Max particles in the Scope particle field (matches the config cap). Each
    /// is two `vec4<f32>` — (x, y, size, alpha) + (colour_t, _, _, _) — in the
    /// particle storage buffer.
    pub(crate) const MAX_PARTICLES: usize = 2048;

    pub(crate) fn new(
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        // Create uniform buffer
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("visualizer uniform buffer"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Create bar data buffer (storage buffer for bar heights)
        let bar_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("visualizer bar buffer"),
            size: (Self::MAX_BARS * std::mem::size_of::<f32>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Create peak data buffer
        let peak_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("visualizer peak buffer"),
            size: (Self::MAX_BARS * std::mem::size_of::<f32>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Create peak alpha buffer (for fade mode)
        let peak_alpha_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("visualizer peak alpha buffer"),
            size: (Self::MAX_BARS * std::mem::size_of::<f32>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Create particle buffer (two vec4 per particle: x,y,size,alpha + colour_t,…)
        let particle_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("visualizer particle buffer"),
            size: (Self::MAX_PARTICLES * 8 * std::mem::size_of::<f32>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Bind group layout
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("visualizer bind group layout"),
            entries: &[
                // Uniforms
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // Bar data
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // Peak data
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // Peak alpha data (for fade mode)
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // Particle data (two vec4 per particle, scope mode only)
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        // Create bind group
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("visualizer bind group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: bar_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: peak_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: peak_alpha_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: particle_buffer.as_entire_binding(),
                },
            ],
        });

        // Pipeline layout
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("visualizer pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        // Load bars shader
        let bars_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("visualizer bars shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "shaders/bars.wgsl"
            ))),
        });

        // Load lines shader
        let lines_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("visualizer lines shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "shaders/lines.wgsl"
            ))),
        });

        // Load scope shader (circular oscilloscope)
        let scope_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("visualizer scope shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "shaders/scope.wgsl"
            ))),
        });

        // Load particle shader (scope particle field)
        let particle_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("visualizer particle shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "shaders/particles.wgsl"
            ))),
        });

        // Create bars render pipeline (no MSAA — fast path for flat mode)
        let bars_pipeline = build_visualizer_pipeline(
            device,
            &layout,
            &bars_shader,
            wgpu::PrimitiveTopology::TriangleList,
            false,
            "visualizer bars pipeline",
            format,
            wgpu::BlendState::ALPHA_BLENDING,
        );

        // Create bars render pipeline with 4x MSAA (for perspective/3D mode)
        let bars_pipeline_msaa = build_visualizer_pipeline(
            device,
            &layout,
            &bars_shader,
            wgpu::PrimitiveTopology::TriangleList,
            true,
            "visualizer bars pipeline (MSAA 4x)",
            format,
            wgpu::BlendState::ALPHA_BLENDING,
        );

        // Create lines render pipeline. TriangleList: the SDF stroke emits one
        // miter-tiled quad (6 verts) per dense spline segment — quads tile on the
        // join bisector so they never overlap (no double-composite seam) and the
        // ribbon can't self-intersect (see lines.wgsl).
        let lines_pipeline = build_visualizer_pipeline(
            device,
            &layout,
            &lines_shader,
            wgpu::PrimitiveTopology::TriangleList,
            false,
            "visualizer lines pipeline",
            format,
            wgpu::BlendState::ALPHA_BLENDING,
        );

        // Create lines render pipeline with 4x MSAA (for perspective/3D mode)
        let lines_pipeline_msaa = build_visualizer_pipeline(
            device,
            &layout,
            &lines_shader,
            wgpu::PrimitiveTopology::TriangleList,
            true,
            "visualizer lines pipeline (MSAA 4x)",
            format,
            wgpu::BlendState::ALPHA_BLENDING,
        );

        // Create scope render pipeline (TriangleList miter-quads, same as lines)
        let scope_pipeline = build_visualizer_pipeline(
            device,
            &layout,
            &scope_shader,
            wgpu::PrimitiveTopology::TriangleList,
            false,
            "visualizer scope pipeline",
            format,
            wgpu::BlendState::ALPHA_BLENDING,
        );

        // Create scope render pipeline with 4x MSAA (effects/offscreen path)
        let scope_pipeline_msaa = build_visualizer_pipeline(
            device,
            &layout,
            &scope_shader,
            wgpu::PrimitiveTopology::TriangleList,
            true,
            "visualizer scope pipeline (MSAA 4x)",
            format,
            wgpu::BlendState::ALPHA_BLENDING,
        );

        // Particle pipelines (instanced glowing quads, additive blend so the
        // dust accumulates into bright spots — the NCS look). TriangleList: 6
        // verts per particle. One non-MSAA + one MSAA, mirroring the scope.
        let additive_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let particle_pipeline = build_visualizer_pipeline(
            device,
            &layout,
            &particle_shader,
            wgpu::PrimitiveTopology::TriangleList,
            false,
            "visualizer particle pipeline",
            format,
            additive_blend,
        );
        let particle_pipeline_msaa = build_visualizer_pipeline(
            device,
            &layout,
            &particle_shader,
            wgpu::PrimitiveTopology::TriangleList,
            true,
            "visualizer particle pipeline (MSAA 4x)",
            format,
            additive_blend,
        );

        // Scope beam pipelines: the scope shader rendered with additive blending
        // (the luminous woscope-style beam). Same TriangleList geometry as the
        // regular scope pipelines — only the blend differs.
        let scope_pipeline_beam = build_visualizer_pipeline(
            device,
            &layout,
            &scope_shader,
            wgpu::PrimitiveTopology::TriangleList,
            false,
            "visualizer scope beam pipeline",
            format,
            additive_blend,
        );
        let scope_pipeline_beam_msaa = build_visualizer_pipeline(
            device,
            &layout,
            &scope_shader,
            wgpu::PrimitiveTopology::TriangleList,
            true,
            "visualizer scope beam pipeline (MSAA 4x)",
            format,
            additive_blend,
        );

        // --- Blit pipeline for compositing MSAA result onto framebuffer ---
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("visualizer blit shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(
                r#"
struct VertexOut {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vs_blit(@builtin(vertex_index) idx: u32) -> VertexOut {
    // Full-viewport triangle (3 vertices cover the entire viewport)
    var positions = array<vec2f, 3>(
        vec2f(-1.0, -1.0),
        vec2f(3.0, -1.0),
        vec2f(-1.0, 3.0),
    );
    var out: VertexOut;
    out.position = vec4f(positions[idx], 0.0, 1.0);
    // Map NDC to UV: [-1,1] -> [0,1], flip Y for texture coordinates
    out.uv = positions[idx] * vec2f(0.5, -0.5) + 0.5;
    return out;
}

@group(0) @binding(0) var t_resolve: texture_2d<f32>;
@group(0) @binding(1) var s_resolve: sampler;

@fragment
fn fs_blit(in: VertexOut) -> @location(0) vec4f {
    return textureSample(t_resolve, s_resolve, in.uv);
}

// Trail fade: output is multiplied by the Zero src-factor, so the value is
// irrelevant — the pass exists only to scale the destination by the blend
// constant (trail *= decay). Returns 0 to keep it explicit.
@fragment
fn fs_fade(in: VertexOut) -> @location(0) vec4f {
    return vec4f(0.0);
}
"#,
            )),
        });

        let blit_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("visualizer blit bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let blit_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("visualizer blit pipeline layout"),
            bind_group_layouts: &[Some(&blit_bind_group_layout)],
            immediate_size: 0,
        });

        // Premultiplied alpha blending: src*1 + dst*(1-src_alpha)
        // This correctly composites the resolved MSAA texture (which has premultiplied alpha
        // from rendering onto a transparent-cleared MSAA target) onto the framebuffer.
        let premultiplied_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };

        let blit_pipeline = build_postprocess_pipeline(
            device,
            &blit_layout,
            &blit_shader,
            ("vs_blit", "fs_blit"),
            premultiplied_blend,
            format,
            "visualizer blit pipeline",
        );

        // --- Motion trail pipelines (reuse the blit shader + layout) ---
        // Fade: out = dst * blend_constant (the per-frame decay) — src is
        // multiplied by Zero so fs_fade's output is irrelevant.
        let trail_fade_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::Constant,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::Constant,
                operation: wgpu::BlendOperation::Add,
            },
        };
        // Max: out = max(scene, faded trail) — bright motion leaves a fading
        // ghost without additive saturation.
        let trail_max_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Max,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Max,
            },
        };
        // Trail pipelines render into the float accumulator, not the 8-bit
        // surface (see TRAIL_FORMAT).
        let trail_fade_pipeline = build_postprocess_pipeline(
            device,
            &blit_layout,
            &blit_shader,
            ("vs_blit", "fs_fade"),
            trail_fade_blend,
            TRAIL_FORMAT,
            "visualizer trail fade pipeline",
        );
        let trail_max_pipeline = build_postprocess_pipeline(
            device,
            &blit_layout,
            &blit_shader,
            ("vs_blit", "fs_blit"),
            trail_max_blend,
            TRAIL_FORMAT,
            "visualizer trail max pipeline",
        );

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("visualizer blit sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // --- Bloom post-processing pipelines ---
        let bloom_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("visualizer bloom shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "shaders/bloom.wgsl"
            ))),
        });

        // texture + sampler + BloomParams uniform
        let bloom_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("visualizer bloom bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        let bloom_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("visualizer bloom uniform buffer"),
            size: std::mem::size_of::<BloomParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bloom_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("visualizer bloom pipeline layout"),
            bind_group_layouts: &[Some(&bloom_bind_group_layout)],
            immediate_size: 0,
        });

        // Blur/threshold passes overwrite their target; the composite adds light.
        let additive_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };

        let bloom_bright_pipeline = build_postprocess_pipeline(
            device,
            &bloom_layout,
            &bloom_shader,
            ("vs_main", "fs_bright_h"),
            wgpu::BlendState::REPLACE,
            format,
            "visualizer bloom bright/H pipeline",
        );
        let bloom_blur_v_pipeline = build_postprocess_pipeline(
            device,
            &bloom_layout,
            &bloom_shader,
            ("vs_main", "fs_blur_v"),
            wgpu::BlendState::REPLACE,
            format,
            "visualizer bloom blur V pipeline",
        );
        // Horizontal blur without the threshold — drives the wide-glow iterations.
        let bloom_blur_h_pipeline = build_postprocess_pipeline(
            device,
            &bloom_layout,
            &bloom_shader,
            ("vs_main", "fs_blur_h"),
            wgpu::BlendState::REPLACE,
            format,
            "visualizer bloom blur H pipeline",
        );
        let bloom_composite_pipeline = build_postprocess_pipeline(
            device,
            &bloom_layout,
            &bloom_shader,
            ("vs_main", "fs_composite"),
            additive_blend,
            format,
            "visualizer bloom composite pipeline",
        );

        // --- Echo (Milkdrop feedback) pipeline ---
        let echo_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("visualizer echo shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "shaders/echo.wgsl"
            ))),
        });
        let texture_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let echo_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("visualizer echo bind group layout"),
                entries: &[
                    texture_entry(0), // scene
                    texture_entry(1), // prev echo (scratch)
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let echo_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("visualizer echo uniform buffer"),
            size: std::mem::size_of::<EchoParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let echo_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("visualizer echo pipeline layout"),
            bind_group_layouts: &[Some(&echo_bind_group_layout)],
            immediate_size: 0,
        });
        let echo_feedback_pipeline = build_postprocess_pipeline(
            device,
            &echo_layout,
            &echo_shader,
            ("vs_echo", "fs_echo"),
            wgpu::BlendState::REPLACE,
            TRAIL_FORMAT,
            "visualizer echo feedback pipeline",
        );

        // --- CRT / film composite pipeline ---
        let crt_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("visualizer crt shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "shaders/crt.wgsl"
            ))),
        });
        let crt_uniform_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("visualizer crt uniform bind group layout"),
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
        let crt_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("visualizer crt uniform buffer"),
            size: std::mem::size_of::<CrtParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let crt_uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("visualizer crt uniform bind group"),
            layout: &crt_uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: crt_uniform_buffer.as_entire_binding(),
            }],
        });
        // Group 0 reuses the blit layout (display texture + sampler); group 1 is
        // the CrtParams uniform.
        let crt_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("visualizer crt pipeline layout"),
            bind_group_layouts: &[Some(&blit_bind_group_layout), Some(&crt_uniform_layout)],
            immediate_size: 0,
        });
        let crt_pipeline = build_postprocess_pipeline(
            device,
            &crt_layout,
            &crt_shader,
            ("vs_crt", "fs_crt"),
            premultiplied_blend,
            format,
            "visualizer crt pipeline",
        );

        Self {
            bars_pipeline,
            bars_pipeline_msaa,
            lines_pipeline,
            lines_pipeline_msaa,
            scope_pipeline,
            scope_pipeline_msaa,
            particle_pipeline,
            particle_pipeline_msaa,
            scope_pipeline_beam,
            scope_pipeline_beam_msaa,
            uniform_buffer,
            bar_buffer,
            particle_buffer,
            peak_buffer,
            peak_alpha_buffer,
            bind_group,
            max_bars: Self::MAX_BARS,
            msaa_texture: None,
            resolve_texture: None,
            ring_only_texture: None,
            blit_bind_group: None,
            blit_pipeline,
            blit_bind_group_layout,
            sampler,
            msaa_size: (0, 0),
            format,
            bloom_bright_pipeline,
            bloom_blur_v_pipeline,
            bloom_blur_h_pipeline,
            bloom_composite_pipeline,
            bloom_bind_group_layout,
            bloom_uniform_buffer,
            bloom_texture_a: None,
            bloom_texture_b: None,
            bloom_bg_scene: None,
            bloom_bg_a: None,
            bloom_bg_b: None,
            trail_fade_pipeline,
            trail_max_pipeline,
            trail_texture: None,
            blit_bg_trail: None,
            trails_were_active: false,
            trail_needs_clear: false,
            echo_feedback_pipeline,
            echo_bind_group_layout,
            echo_uniform_buffer,
            echo_texture: None,
            echo_temp: None,
            echo_feedback_bg: None,
            blit_bg_echo: None,
            echo_were_active: false,
            echo_needs_clear: false,
            crt_pipeline,
            crt_uniform_buffer,
            crt_uniform_bind_group,
        }
    }
}
