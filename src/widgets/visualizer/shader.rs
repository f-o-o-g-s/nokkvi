//! GPU Shader-based Visualizer
//!
//! Uses Iced's shader widget with wgpu for GPU-accelerated rendering.
//! This offloads rendering from the CPU, allowing smooth animations
//! even during other UI operations like slot list navigation.

// Global start time for animation - lazily initialized
use std::{sync::OnceLock, time::Instant};

use iced::{
    Color, Rectangle, mouse, wgpu,
    widget::shader::{self, Viewport},
};

use super::{VisualizationMode, state::VisualizerState};
static START_TIME: OnceLock<Instant> = OnceLock::new();

fn get_elapsed_time() -> f32 {
    let start = START_TIME.get_or_init(Instant::now);
    start.elapsed().as_secs_f32()
}

/// Configuration passed to the GPU shader
#[derive(Debug, Clone, Copy)]
#[repr(C, align(16))]
pub(crate) struct VisualizerConfig {
    pub bar_count: u32,
    pub mode: u32, // 0 = bars, 1 = lines
    pub border_width: f32,
    pub peak_enabled: u32,
    pub peak_thickness: f32,
    pub peak_alpha: f32,
    pub line_thickness: f32,
    pub bar_width: f32,               // Fixed bar width in pixels
    pub bar_spacing: f32,             // Fixed spacing between bars in pixels
    pub edge_spacing: f32,            // Edge spacing for centering bars in pixels
    pub time: f32,                    // Time in seconds for animation
    pub led_bars: u32,                // 0 = normal bars, 1 = LED segmented bars
    pub led_segment_height: f32,      // Height of each LED segment in pixels
    pub led_border_opacity: f32, // 0.0 = transparent, 1.0 = opaque (border opacity in LED mode)
    pub border_opacity: f32,     // 0.0 = transparent, 1.0 = opaque (border opacity in non-LED mode)
    pub gradient_mode: u32,      // 0 = static, 2 = wave, 3 = shimmer, 4 = energy, 5 = alternate
    pub peak_gradient_mode: u32, // 0=static, 1=cycle, 2=height, 3=match
    pub peak_mode: u32,          // 0=none, 1=fade, 2=fall, 3=fall_accel
    pub peak_hold_time: f32,     // Time in seconds for peak to hold
    pub peak_fade_time: f32,     // Time in seconds for peak to fade (fade mode only)
    pub flash_count: u32,        // Number of bars (for flash data bounds checking)
    pub bar_depth_3d: f32,       // Isometric 3D depth in pixels (0 = flat)
    pub gradient_orientation: u32, // 0 = vertical, 1 = horizontal
    pub average_energy: f32,     // Average bar amplitude (0.0-1.0), computed CPU-side
    pub global_opacity: f32,     // Overall visualizer opacity (0.0-1.0)
    pub lines_outline_thickness: f32, // Lines mode: outline width in pixels (0 = disabled)
    pub lines_outline_opacity: f32, // Lines mode: outline alpha (0.0-1.0)
    pub lines_animation_speed: f32, // Lines mode: color cycling speed (0.05-1.0)
    pub lines_gradient_mode: u32, // Lines mode: 0=breathing, 1=static, 2=position, 3=height
    pub lines_fill_opacity: f32, // Lines mode: fill under curve (0.0 = disabled)
    pub lines_mirror: u32,       // Lines mode: 0=normal, 1=mirrored
    pub lines_glow_intensity: f32, // Lines mode: glow bloom (0.0 = disabled)
    pub lines_style: u32,        // Lines mode: 0=smooth, 1=angular, 2=stepped
    pub _pad: [u32; 3],          // Padding for 16-byte alignment before flash_data
    // Flash intensities: one per bar (0.0-1.0), stored as vec4s for GPU efficiency
    // Up to 2048 bars = 512 vec4s
    pub flash_data: [[f32; 4]; 512], // 2048 bars max
}

unsafe impl bytemuck::Pod for VisualizerConfig {}
unsafe impl bytemuck::Zeroable for VisualizerConfig {}

/// GPU-rendered visualizer primitive
/// Contains per-frame data to upload to GPU
#[derive(Debug)]
pub(crate) struct VisualizerPrimitive {
    /// Gradient colors for bars (blue to aqua)
    pub gradient_colors: [[f32; 4]; 8],
    /// Peak breathing colors (warm colors: orange/yellow/red)
    pub peak_gradient_colors: [[f32; 4]; 8],
    /// Peak bar color
    pub peak_color: [f32; 4],
    /// Border color
    pub border_color: [f32; 4],
    /// Configuration
    pub config: VisualizerConfig,
    /// Reference to state for dirty flag checking
    pub state: VisualizerState,
    /// Whether perspective lean is active (triggers MSAA render path)
    pub has_perspective: bool,
}

/// Shader visualizer parameters grouped for cleaner API
/// This reduces the number of function arguments by grouping related configuration
#[derive(Debug, Clone)]
pub(crate) struct ShaderParams {
    /// Gradient colors for bars (bottom to top), 8 colors
    pub gradient_colors: Vec<Color>,
    /// Peak breathing colors (warm colors), 8 colors  
    pub peak_gradient_colors: Vec<Color>,
    /// Border color for bars
    pub border_color: Color,
    /// Border width around each bar in pixels
    pub border_width: f32,
    /// Whether peak bars are enabled
    pub peak_enabled: bool,
    /// Peak bar thickness as ratio of visualizer height
    pub peak_thickness: f32,
    /// Peak bar opacity (0.0-1.0)
    pub peak_alpha: f32,
    /// Peak bar color (for static peak color mode)
    pub peak_color: Color,
    /// Line thickness for lines mode (as ratio of height)
    pub line_thickness: f32,
    /// Fixed bar width in pixels
    pub bar_width: f32,
    /// Spacing between bars in pixels
    pub bar_spacing: f32,
    /// Edge spacing for centering bars in pixels
    pub edge_spacing: f32,
    /// Enable LED-style segmented bars
    pub led_bars: bool,
    /// Height of each LED segment in pixels
    pub led_segment_height: f32,
    /// Border opacity in LED mode (0.0-1.0)
    pub led_border_opacity: f32,
    /// Border opacity in non-LED mode (0.0-1.0)
    pub border_opacity: f32,
    /// Gradient mode: 0=static, 2=wave, 3=shimmer, 4=energy, 5=alternate
    pub gradient_mode: u32,
    /// Peak gradient mode: 0=static, 1=cycle, 2=height, 3=match
    pub peak_gradient_mode: u32,
    /// Peak behavior mode: 0=none, 1=fade, 2=fall, 3=fall_accel
    pub peak_mode: u32,
    /// Time in seconds for peaks to hold before falling/fading
    pub peak_hold_time: f32,
    /// Time in seconds for peaks to fade (fade mode only)
    pub peak_fade_time: f32,
    /// Isometric 3D depth in pixels (0.0 = flat, up to 20.0)
    pub bar_depth_3d: f32,
    /// Gradient orientation: 0=vertical, 1=horizontal
    pub gradient_orientation: u32,
    /// Overall visualizer opacity (0.0 = invisible, 1.0 = fully opaque)
    pub global_opacity: f32,
    /// Lines mode: outline thickness in pixels (0.0 = disabled)
    pub lines_outline_thickness: f32,
    /// Lines mode: outline opacity (0.0 = invisible, 1.0 = fully opaque)
    pub lines_outline_opacity: f32,
    /// Lines mode: color animation cycle speed (0.05 = slow, 1.0 = fast)
    pub lines_animation_speed: f32,
    /// Lines mode: gradient mode (0=breathing, 1=static, 2=position, 3=height)
    pub lines_gradient_mode: u32,
    /// Lines mode: fill opacity under curve (0.0 = disabled, 1.0 = fully opaque)
    pub lines_fill_opacity: f32,
    /// Lines mode: mirror mode (false = normal, true = symmetric oscilloscope)
    pub lines_mirror: bool,
    /// Lines mode: glow bloom intensity (0.0 = disabled, 1.0 = full glow)
    pub lines_glow_intensity: f32,
    /// Lines mode: interpolation style (0=smooth, 1=angular, 2=stepped)
    pub lines_style: u32,
}

impl VisualizerPrimitive {
    /// Create primitive from visualizer state and shader parameters
    pub(crate) fn new(
        state: &VisualizerState,
        mode: VisualizationMode,
        params: &ShaderParams,
    ) -> Self {
        // Convert gradient colors to array (pad to 8 colors)
        let mut gradient: [[f32; 4]; 8] = [[0.0; 4]; 8];
        for (i, color) in params.gradient_colors.iter().take(8).enumerate() {
            gradient[i] = [color.r, color.g, color.b, color.a];
        }

        // Convert peak gradient colors to array (pad to 8 colors)
        let mut peak_gradient: [[f32; 4]; 8] = [[0.0; 4]; 8];
        for (i, color) in params.peak_gradient_colors.iter().take(8).enumerate() {
            peak_gradient[i] = [color.r, color.g, color.b, color.a];
        }

        let peak_col = [
            params.peak_color.r,
            params.peak_color.g,
            params.peak_color.b,
            params.peak_alpha,
        ];
        let border_col = [
            params.border_color.r,
            params.border_color.g,
            params.border_color.b,
            params.border_color.a,
        ];

        // Get flash intensities from state
        let flash_intensities = state.get_flash_intensities();
        let flash_count = flash_intensities.len().min(2048) as u32;

        // Pack flash data into vec4s (4 floats per vec4)
        let mut flash_data: [[f32; 4]; 512] = [[0.0; 4]; 512];
        for (i, &intensity) in flash_intensities.iter().take(2048).enumerate() {
            let vec_idx = i / 4;
            let component = i % 4;
            flash_data[vec_idx][component] = intensity;
        }

        // Compute average energy CPU-side (avoids per-vertex loop in the shader)
        let bars = state.get_bars();
        let bar_count_val = state.bar_count();
        let average_energy = if bar_count_val > 0 {
            bars.iter().map(|&v| v as f32).sum::<f32>() / bar_count_val as f32
        } else {
            0.0
        };

        let config = VisualizerConfig {
            bar_count: bar_count_val as u32,
            mode: match mode {
                VisualizationMode::Bars => 0,
                VisualizationMode::Lines => 1,
            },
            border_width: params.border_width,
            peak_enabled: if params.peak_enabled { 1 } else { 0 },
            peak_thickness: params.peak_thickness,
            peak_alpha: params.peak_alpha,
            line_thickness: params.line_thickness,
            bar_width: params.bar_width,
            bar_spacing: params.bar_spacing,
            edge_spacing: params.edge_spacing,
            time: get_elapsed_time(),
            led_bars: if params.led_bars { 1 } else { 0 },
            led_segment_height: params.led_segment_height,
            led_border_opacity: params.led_border_opacity,
            border_opacity: params.border_opacity,
            gradient_mode: params.gradient_mode,
            peak_gradient_mode: params.peak_gradient_mode,
            peak_mode: params.peak_mode,
            peak_hold_time: params.peak_hold_time,
            peak_fade_time: params.peak_fade_time,
            flash_count,
            bar_depth_3d: params.bar_depth_3d,
            gradient_orientation: params.gradient_orientation,
            average_energy,
            global_opacity: params.global_opacity,
            lines_outline_thickness: params.lines_outline_thickness,
            lines_outline_opacity: params.lines_outline_opacity,
            lines_animation_speed: params.lines_animation_speed,
            lines_gradient_mode: params.lines_gradient_mode,
            lines_fill_opacity: params.lines_fill_opacity,
            lines_mirror: u32::from(params.lines_mirror),
            lines_glow_intensity: params.lines_glow_intensity,
            lines_style: params.lines_style,
            _pad: [0; 3],
            flash_data,
        };

        let has_perspective = params.bar_depth_3d > 0.001;

        Self {
            gradient_colors: gradient,
            peak_gradient_colors: peak_gradient,
            peak_color: peak_col,
            border_color: border_col,
            config,
            state: state.clone(),
            has_perspective,
        }
    }
}

/// GPU pipeline for visualizer rendering
pub(crate) struct VisualizerPipeline {
    pub(super) bars_pipeline: wgpu::RenderPipeline,
    pub(super) bars_pipeline_msaa: wgpu::RenderPipeline,
    pub(super) lines_pipeline: wgpu::RenderPipeline,
    pub(super) lines_pipeline_msaa: wgpu::RenderPipeline,
    pub(super) uniform_buffer: wgpu::Buffer,
    pub(super) bar_buffer: wgpu::Buffer,
    pub(super) peak_buffer: wgpu::Buffer,
    pub(super) peak_alpha_buffer: wgpu::Buffer,
    pub(super) bind_group: wgpu::BindGroup,
    pub(super) max_bars: usize,
    /// Cached 4x MSAA texture for antialiased perspective rendering
    pub(super) msaa_texture: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// Widget-sized resolve target (MSAA resolves here, then blitted to framebuffer)
    pub(super) resolve_texture: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// Bind group for the blit pass (references resolve_texture + sampler)
    pub(super) blit_bind_group: Option<wgpu::BindGroup>,
    /// Pipeline for blitting the resolve texture onto the framebuffer
    pub(super) blit_pipeline: wgpu::RenderPipeline,
    pub(super) blit_bind_group_layout: wgpu::BindGroupLayout,
    pub(super) sampler: wgpu::Sampler,
    pub(super) msaa_size: (u32, u32),
    pub(super) format: wgpu::TextureFormat,
}

/// Uniforms passed to the shader
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub(super) struct Uniforms {
    /// Viewport dimensions
    pub(super) viewport: [f32; 4], // x, y, width, height
    /// Gradient colors for bars (blue to aqua)
    pub(super) gradient_colors: [[f32; 4]; 8],
    /// Peak breathing colors (warm colors)
    pub(super) peak_gradient_colors: [[f32; 4]; 8],
    /// Peak color
    pub(super) peak_color: [f32; 4],
    /// Border color
    pub(super) border_color: [f32; 4],
    /// Config
    pub(super) config: VisualizerConfig,
}

unsafe impl bytemuck::Pod for Uniforms {}
unsafe impl bytemuck::Zeroable for Uniforms {}

impl VisualizerPrimitive {
    /// Shared draw logic for both the non-MSAA and MSAA render paths.
    /// Accepts individual pipeline references to allow the caller to choose
    /// between the standard and MSAA pipeline variants.
    fn draw_bars_and_lines(
        config: &VisualizerConfig,
        bind_group: &wgpu::BindGroup,
        bars_pipeline: &wgpu::RenderPipeline,
        lines_pipeline: &wgpu::RenderPipeline,
        render_pass: &mut wgpu::RenderPass<'_>,
    ) {
        let bar_count = config.bar_count;

        render_pass.set_bind_group(0, bind_group, &[]);

        match config.mode {
            0 => {
                // Bars mode
                render_pass.set_pipeline(bars_pipeline);

                let vertices_per_bar = 6;
                let quads_per_bar = 6u32;
                let peak_multiplier = if config.peak_enabled != 0 {
                    quads_per_bar
                } else {
                    0
                };
                let total_quads = bar_count * quads_per_bar + bar_count * peak_multiplier;

                render_pass.draw(0..(total_quads * vertices_per_bar), 0..1);
            }
            1 => {
                // Lines mode
                render_pass.set_pipeline(lines_pipeline);

                let samples_per_segment = 16u32;
                let num_segments = bar_count.saturating_sub(1);
                let total_spline_points = num_segments * samples_per_segment + 1;
                let vertices_per_spline_point = 2u32;
                let vertices_per_pass = total_spline_points * vertices_per_spline_point;

                // Mirror mode doubles the instances (3 top + 3 bottom reflection)
                let instance_count = if config.lines_mirror != 0 { 6 } else { 3 };
                render_pass.draw(0..vertices_per_pass, 0..instance_count);
            }
            _ => {}
        }
    }

    /// Non-MSAA render fallback (when MSAA texture isn't ready yet)
    fn render_without_msaa(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
        pipeline: &VisualizerPipeline,
    ) {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("visualizer non-MSAA render pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        render_pass.set_viewport(
            clip_bounds.x as f32,
            clip_bounds.y as f32,
            clip_bounds.width as f32,
            clip_bounds.height as f32,
            0.0,
            1.0,
        );

        render_pass.set_scissor_rect(
            clip_bounds.x,
            clip_bounds.y,
            clip_bounds.width,
            clip_bounds.height,
        );

        Self::draw_bars_and_lines(
            &self.config,
            &pipeline.bind_group,
            &pipeline.bars_pipeline,
            &pipeline.lines_pipeline,
            &mut render_pass,
        );
    }
}

impl shader::Primitive for VisualizerPrimitive {
    type Pipeline = VisualizerPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        _viewport: &Viewport,
    ) {
        // NOTE: tick() runs on a background FFT thread, decoupled from the render path.
        // The shader widget self-drives redraws via Action::request_redraw() in update().

        // IMPORTANT: Read FRESH bar data from state, not the stale snapshot from draw().
        //
        // The previous approach captured bar_data at draw() time, but the dirty flag
        // could change between draw() and prepare(), causing stale data to be uploaded
        // and the dirty flag to be incorrectly cleared.
        //
        // By reading fresh data here, we guarantee we always upload the current state.
        let fresh_bars = self.state.get_bars();
        let fresh_peaks = self.state.get_peak_bars();
        let fresh_peak_alphas = self.state.get_peak_alphas();

        // Convert to f32 for GPU
        let bar_data: Vec<f32> = fresh_bars.iter().map(|&v| v as f32).collect();
        let peak_data: Vec<f32> = fresh_peaks.iter().map(|&v| v as f32).collect();
        let peak_alpha_data: Vec<f32> = fresh_peak_alphas.iter().map(|&v| v as f32).collect();

        // Clear dirty flag since we've read the current state
        self.state.clear_dirty();

        // Update uniforms
        let uniforms = Uniforms {
            viewport: [bounds.x, bounds.y, bounds.width, bounds.height],
            gradient_colors: self.gradient_colors,
            peak_gradient_colors: self.peak_gradient_colors,
            peak_color: self.peak_color,
            border_color: self.border_color,
            config: self.config,
        };
        queue.write_buffer(&pipeline.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        // Update bar data (pad to max size)
        let mut bar_data_padded = bar_data;
        bar_data_padded.resize(pipeline.max_bars, 0.0);
        queue.write_buffer(
            &pipeline.bar_buffer,
            0,
            bytemuck::cast_slice(&bar_data_padded),
        );

        // Update peak data
        let mut peak_data_padded = peak_data;
        peak_data_padded.resize(pipeline.max_bars, 0.0);
        queue.write_buffer(
            &pipeline.peak_buffer,
            0,
            bytemuck::cast_slice(&peak_data_padded),
        );

        // Update peak alpha data
        let mut peak_alpha_padded = peak_alpha_data;
        peak_alpha_padded.resize(pipeline.max_bars, 1.0);
        queue.write_buffer(
            &pipeline.peak_alpha_buffer,
            0,
            bytemuck::cast_slice(&peak_alpha_padded),
        );

        // Create/resize MSAA + resolve textures if perspective/3D is active
        if self.has_perspective {
            let scale = _viewport.scale_factor();
            let w = (bounds.width * scale).ceil() as u32;
            let h = (bounds.height * scale).ceil() as u32;

            if w > 0 && h > 0 && pipeline.msaa_size != (w, h) {
                // 4x MSAA texture (widget-sized)
                let msaa_tex = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("visualizer MSAA texture"),
                    size: wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 4,
                    dimension: wgpu::TextureDimension::D2,
                    format: pipeline.format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                });
                let msaa_view = msaa_tex.create_view(&wgpu::TextureViewDescriptor::default());

                // 1x resolve texture (widget-sized, also TEXTURE_BINDING for blit sampling)
                let resolve_tex = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("visualizer resolve texture"),
                    size: wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: pipeline.format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                let resolve_view = resolve_tex.create_view(&wgpu::TextureViewDescriptor::default());

                // Bind group for blit pass (references resolve texture + sampler)
                let blit_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("visualizer blit bind group"),
                    layout: &pipeline.blit_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&resolve_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&pipeline.sampler),
                        },
                    ],
                });

                pipeline.msaa_texture = Some((msaa_tex, msaa_view));
                pipeline.resolve_texture = Some((resolve_tex, resolve_view));
                pipeline.blit_bind_group = Some(blit_bind_group);
                pipeline.msaa_size = (w, h);
            }
        }
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        let bar_count = self.config.bar_count;
        if bar_count == 0 {
            return true;
        }

        // When perspective/3D is active, fall through to render() for MSAA
        if self.has_perspective {
            return false;
        }

        Self::draw_bars_and_lines(
            &self.config,
            &pipeline.bind_group,
            &pipeline.bars_pipeline,
            &pipeline.lines_pipeline,
            render_pass,
        );
        true
    }

    fn render(
        &self,
        pipeline: &Self::Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        let bar_count = self.config.bar_count;
        if bar_count == 0 {
            return;
        }

        let width = clip_bounds.width;
        let height = clip_bounds.height;

        if width == 0 || height == 0 {
            return;
        }

        // Need all three: MSAA texture, resolve texture, and blit bind group
        let (msaa_view, resolve_view, blit_bg) = match (
            &pipeline.msaa_texture,
            &pipeline.resolve_texture,
            &pipeline.blit_bind_group,
        ) {
            (Some((_, mv)), Some((_, rv)), Some(bg)) => (mv, rv, bg),
            _ => return self.render_without_msaa(encoder, target, clip_bounds, pipeline),
        };

        // Pass 1: Render bars into MSAA texture, resolve into widget-sized resolve texture
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("visualizer MSAA render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: msaa_view,
                    depth_slice: None,
                    resolve_target: Some(resolve_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            // Viewport covers the full widget-sized texture (0,0 origin)
            let (tex_w, tex_h) = pipeline.msaa_size;
            render_pass.set_viewport(0.0, 0.0, tex_w as f32, tex_h as f32, 0.0, 1.0);
            render_pass.set_scissor_rect(0, 0, tex_w, tex_h);

            Self::draw_bars_and_lines(
                &self.config,
                &pipeline.bind_group,
                &pipeline.bars_pipeline_msaa,
                &pipeline.lines_pipeline_msaa,
                &mut render_pass,
            );
        }

        // Pass 2: Blit the resolve texture onto the framebuffer with premultiplied alpha blending
        {
            let mut blit_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("visualizer blit pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load, // Preserve existing framebuffer content
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            // Viewport positions the blit at the widget's location on the framebuffer
            let (tex_w, tex_h) = pipeline.msaa_size;
            blit_pass.set_viewport(
                clip_bounds.x as f32,
                clip_bounds.y as f32,
                tex_w as f32,
                tex_h as f32,
                0.0,
                1.0,
            );
            blit_pass.set_scissor_rect(clip_bounds.x, clip_bounds.y, width, height);

            blit_pass.set_pipeline(&pipeline.blit_pipeline);
            blit_pass.set_bind_group(0, blit_bg, &[]);
            blit_pass.draw(0..3, 0..1); // Single fullscreen triangle
        }
    }
}

impl shader::Pipeline for VisualizerPipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self::new(device, queue, format)
    }
}

/// Shader-based visualizer widget program
#[derive(Clone)]
pub(crate) struct ShaderVisualizer {
    state: VisualizerState,
    mode: VisualizationMode,
    params: ShaderParams,
}

impl ShaderVisualizer {
    /// Create a new shader visualizer with the given state, mode, and parameters
    pub(crate) fn new(
        state: VisualizerState,
        mode: VisualizationMode,
        params: ShaderParams,
    ) -> Self {
        Self {
            state,
            mode,
            params,
        }
    }
}

impl<Message> shader::Program<Message> for ShaderVisualizer {
    type State = ();
    type Primitive = VisualizerPrimitive;

    fn update(
        &self,
        _state: &mut Self::State,
        _event: &iced::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<shader::Action<Message>> {
        // Only request a redraw when the FFT thread has produced new data.
        // This prevents the GPU from being perpetually busy with redundant
        // redraws during mouse moves, key presses, and other Iced events.
        // When music is paused/stopped, GPU usage drops to near-zero.
        if self.state.is_dirty() {
            Some(shader::Action::request_redraw())
        } else {
            None
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        bounds: Rectangle,
    ) -> Self::Primitive {
        // Convert thickness ratios to pixels and create adjusted params
        let mut adjusted_params = self.params.clone();
        adjusted_params.line_thickness = self.params.line_thickness * bounds.height;
        // peak_thickness is a ratio (e.g., 0.66 = 66% of bar_width).
        // The WGSL shader multiplies by bar_width to get pixel height,
        // so we pass the ratio through directly (no bounds.height scaling).
        adjusted_params.peak_thickness = self.params.peak_thickness;

        VisualizerPrimitive::new(&self.state, self.mode, &adjusted_params)
    }
}
