//! GPU pipeline initialization for the visualizer
//!
//! Extracted from shader.rs to keep pipeline setup (~400 lines of wgpu boilerplate)
//! separate from the render logic.

use iced::wgpu;

use super::shader::{Uniforms, VisualizerPipeline};

/// Build one of the four bars/lines × default/MSAA render pipelines.
///
/// The four sites in `VisualizerPipeline::new` differ only along four axes:
/// `topology` (TriangleList for bars, TriangleStrip for lines), `msaa`
/// (default vs 4× MSAA), `shader` (bars.wgsl vs lines.wgsl module), and
/// `label` (for debug introspection). Everything else — pipeline layout,
/// vertex/fragment entry points, `ALPHA_BLENDING` blend state, color
/// target format, write mask, depth stencil, multiview mask, cache —
/// is identical, so the helper centralizes the wgpu boilerplate.
///
/// `vs_main` / `fs_main` are the entry points both shaders share.
fn build_visualizer_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    topology: wgpu::PrimitiveTopology,
    msaa: bool,
    label: &'static str,
    format: wgpu::TextureFormat,
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
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
            ],
        });

        // Pipeline layout
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("visualizer pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
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

        // Create bars render pipeline (no MSAA — fast path for flat mode)
        let bars_pipeline = build_visualizer_pipeline(
            device,
            &layout,
            &bars_shader,
            wgpu::PrimitiveTopology::TriangleList,
            false,
            "visualizer bars pipeline",
            format,
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
        );

        // Create lines render pipeline (uses TriangleStrip for thick lines)
        let lines_pipeline = build_visualizer_pipeline(
            device,
            &layout,
            &lines_shader,
            wgpu::PrimitiveTopology::TriangleStrip,
            false,
            "visualizer lines pipeline",
            format,
        );

        // Create lines render pipeline with 4x MSAA (for perspective/3D mode)
        let lines_pipeline_msaa = build_visualizer_pipeline(
            device,
            &layout,
            &lines_shader,
            wgpu::PrimitiveTopology::TriangleStrip,
            true,
            "visualizer lines pipeline (MSAA 4x)",
            format,
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
            bind_group_layouts: &[&blit_bind_group_layout],
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

        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("visualizer blit pipeline"),
            layout: Some(&blit_layout),
            vertex: wgpu::VertexState {
                module: &blit_shader,
                entry_point: Some("vs_blit"),
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
                module: &blit_shader,
                entry_point: Some("fs_blit"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(premultiplied_blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("visualizer blit sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Self {
            bars_pipeline,
            bars_pipeline_msaa,
            lines_pipeline,
            lines_pipeline_msaa,
            uniform_buffer,
            bar_buffer,
            peak_buffer,
            peak_alpha_buffer,
            bind_group,
            max_bars: Self::MAX_BARS,
            msaa_texture: None,
            resolve_texture: None,
            blit_bind_group: None,
            blit_pipeline,
            blit_bind_group_layout,
            sampler,
            msaa_size: (0, 0),
            format,
        }
    }
}
