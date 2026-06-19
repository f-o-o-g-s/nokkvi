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

/// Soft-knee brightness cutoff for the bloom bright-pass (premultiplied luma).
/// Only scene pixels brighter than this bleed a glow; tuned conservative so the
/// dark background and faint AA edges don't haze.
const BLOOM_THRESHOLD: f32 = 0.35;

/// Pixel format for the motion-trail accumulator. It MUST be a float format,
/// not the 8-bit surface format: a multiplicative per-frame fade (`trail *=
/// decay`) on an 8-bit UNORM target has a rounding fixed point — `round(v *
/// 0.92) >= v` for any stored `v <= 6` — so dim pixels get stuck at ~1/255 and
/// never fade, leaving a permanent ghost of wherever the visualizer has been.
/// Rgba16Float has no such floor and is a blendable + filterable render target
/// on Vulkan/Metal/DX12 with no extra wgpu feature. The trail texture and the
/// trail fade/max pipelines must share this format.
pub(super) const TRAIL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Beat-reactive bloom: intensity = bloom_intensity * (dip_base + pump).
/// `dip_base` falls from 1.0 (reactivity 0 = steady glow at the configured
/// strength) toward BLOOM_DIP as reactivity rises, and `pump` surges on bass
/// drops (weighted heavily) plus a touch of any-transient beat. So at full
/// reactivity the glow dips between hits and blooms hard on the bass.
const BLOOM_DIP: f32 = 0.55;
const BLOOM_BEAT_GAIN: f32 = 0.35;
const BLOOM_BASS_GAIN: f32 = 0.85;

/// Echo (Milkdrop feedback) warp tuning. `decay` (persistence) = echo * MAX_DECAY.
/// The warp zooms slowly inward (BASE_ZOOM) and harder on bass (BASS_ZOOM), and
/// spins slowly (BASE_ROT rad/frame) and faster on the beat (BEAT_ROT) — both
/// audio terms scaled by beat reactivity.
const ECHO_MAX_DECAY: f32 = 0.94;
const ECHO_BASE_ZOOM: f32 = 0.012;
const ECHO_BASS_ZOOM: f32 = 0.04;
const ECHO_BASE_ROT: f32 = 0.005;
const ECHO_BEAT_ROT: f32 = 0.02;

/// Spline samples per ring segment for Scope mode. MUST match `SCOPE_SP` in
/// `shaders/scope.wgsl` — the CPU vertex count and the shader's vertex indexing
/// have to agree or the ring drops a segment / overdraws the seam.
const SCOPE_SAMPLES_PER_SEGMENT: u32 = 12;

/// Configuration passed to the GPU shader
#[derive(Debug, Clone, Copy)]
#[repr(C, align(16))]
pub(crate) struct VisualizerConfig {
    pub bar_count: u32,
    pub mode: u32, // 0 = bars, 1 = lines, 2 = scope
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
    pub gradient_mode: u32,      // 0 = static, 2 = wave (1 is intentionally unused)
    pub peak_gradient_mode: u32, // 0=static, 1=cycle, 2=height, 3=match
    pub peak_mode: u32,          // 0=none, 1=fade, 2=fall, 3=fall_accel, 4=fall_fade
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
    pub bars_flash_intensity: f32, // Bars mode: peak-flash bloom strength (0 = off)
    pub scope_radius: f32,       // Scope mode: mean ring radius as fraction of available space
    pub scope_sensitivity: f32,  // Scope mode: waveform swing / gain
    // (scope_radius + scope_sensitivity occupy the 8 bytes that were `_pad`, so
    // flash_data stays 16-byte aligned at offset 144 — size is unchanged.)
    // Flash intensities: one per bar (0.0-1.0), stored as vec4s for GPU efficiency
    // Up to 2048 bars = 512 vec4s
    pub flash_data: [[f32; 4]; 512], // 2048 bars max
}

unsafe impl bytemuck::Pod for VisualizerConfig {}
unsafe impl bytemuck::Zeroable for VisualizerConfig {}

// --- Byte-layout interlock: VisualizerConfig (Rust) <-> Config (bars.wgsl / lines.wgsl) ---
// bytemuck::Pod makes a Rust-vs-WGSL field-order/size mismatch SILENT memory
// reinterpretation, not a compile error. naga validates each .wgsl internally but
// cannot see this struct. These const-asserts + the offset_of checks below + the
// wgsl-vs-rust field-name test in mod.rs are the only guard on the GPU upload contract.
const _: () = assert!(
    core::mem::align_of::<VisualizerConfig>() == 16,
    "VisualizerConfig must stay 16-byte aligned for the std140-style WGSL uniform",
);
const _: () = assert!(
    core::mem::size_of::<VisualizerConfig>() == 8336,
    "VisualizerConfig size changed — update bars.wgsl + lines.wgsl Config and this assert",
);
// First and last scalar before the pad, plus the two array members, anchor the layout.
const _: () = assert!(core::mem::offset_of!(VisualizerConfig, bar_count) == 0);
const _: () = assert!(core::mem::offset_of!(VisualizerConfig, time) == 40);
const _: () = assert!(core::mem::offset_of!(VisualizerConfig, lines_style) == 128);
const _: () = assert!(
    core::mem::offset_of!(VisualizerConfig, scope_radius) == 136,
    "scope_radius/scope_sensitivity reuse the old _pad slot (136..144) so flash_data stays 16-byte aligned (144)",
);
const _: () = assert!(
    core::mem::offset_of!(VisualizerConfig, flash_data) == 144,
    "flash_data must be 16-byte aligned (WGSL array<vec4<f32>> stride = 16)",
);

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
    /// Whether bloom post-processing is active (routes through the render()
    /// offscreen path so the scene can be blurred + additively composited)
    pub bloom_enabled: bool,
    /// Bloom glow strength (0.0-1.0), uploaded to the bloom uniform in prepare()
    pub bloom_intensity: f32,
    /// Beat reactivity (0.0-1.0) — scales how hard effects pump on beat/bass
    pub beat_reactivity: f32,
    /// Whether motion trails are active (routes through the offscreen path so
    /// the scene can be accumulated into the persistent trail texture)
    pub trails_enabled: bool,
    /// Per-frame trail persistence/decay (0.0 = no trail). Set as the fade
    /// pass's blend constant each frame.
    pub trails_decay: f32,
    /// Whether echo (Milkdrop feedback) is active — routes through the offscreen
    /// path and takes over the display.
    pub echo_enabled: bool,
    /// Echo amount (0.0-1.0) — drives the EchoParams decay + warp in prepare().
    pub echo: f32,
    /// Whether the CRT/film composite is active — replaces the plain display blit.
    pub crt_enabled: bool,
    /// CRT amount (0.0-1.0) — master retro intensity, into CrtParams.
    pub crt: f32,
    /// Scope particle-field snapshot (two `vec4`s per particle), empty unless
    /// Scope mode + particles are enabled. Uploaded in prepare() and drawn
    /// (instanced, additive) after the ring.
    pub particle_data: Vec<[f32; 8]>,
    /// Number of particles to draw (== `particle_data.len()`, capped at MAX).
    pub particle_count: u32,
    /// Scope mode: render the ring additively for the luminous-beam look.
    pub scope_beam: bool,
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
    /// Gradient mode: 0=static, 2=wave (1 is intentionally unused)
    pub gradient_mode: u32,
    /// Peak gradient mode: 0=static, 1=cycle, 2=height, 3=match
    pub peak_gradient_mode: u32,
    /// Peak behavior mode: 0=none, 1=fade, 2=fall, 3=fall_accel, 4=fall_fade
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
    /// Scope mode: mean ring radius as a fraction of the available space inside
    /// the cover (0.1 = tiny, 0.95 = nearly fills the panel)
    pub scope_radius: f32,
    /// Scope mode: waveform swing / gain (how hard loud audio pushes the ring)
    pub scope_sensitivity: f32,
    /// Scope mode: whether the particle field is enabled (drawn after the ring)
    pub scope_particles: bool,
    /// Scope mode: luminous-beam look — render the ring with additive blending
    pub scope_beam: bool,
    /// Bars mode: peak-flash bloom strength (0.0 = disabled, 1.0 = max)
    pub bars_flash_intensity: f32,
    /// Bloom post-processing enabled (soft additive glow over the whole scene)
    pub bloom_enabled: bool,
    /// Bloom glow strength (0.0 = off, 1.0 = max additive glow)
    pub bloom_intensity: f32,
    /// Beat reactivity (0.0 = static, 1.0 = full pump) — scales the bloom
    /// surge, glow flare, and bar lift together
    pub beat_reactivity: f32,
    /// Motion trails amount (0.0 = off, 1.0 = long after-images)
    pub trails: f32,
    /// Echo (Milkdrop feedback) amount (0.0 = off, 1.0 = strong persistence)
    pub echo: f32,
    /// CRT / film composite amount (0.0 = off, 1.0 = full retro)
    pub crt: f32,
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
                VisualizationMode::Scope => 2,
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
            bars_flash_intensity: params.bars_flash_intensity,
            scope_radius: params.scope_radius,
            scope_sensitivity: params.scope_sensitivity,
            flash_data,
        };

        let has_perspective = params.bar_depth_3d > 0.001;
        let bloom_enabled = params.bloom_enabled && params.bloom_intensity > 0.001;
        // Map the trails amount to a per-frame decay. Any non-zero amount maps
        // into a visible range (short 0.6 → long 0.92); 0 disables entirely.
        let trails_enabled = params.trails > 0.001;
        let echo_enabled = params.echo > 0.001;
        let crt_enabled = params.crt > 0.001;
        let trails_decay = if trails_enabled {
            0.6 + 0.32 * params.trails.clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Scope particle field: pull the latest snapshot only in Scope mode with
        // particles enabled (otherwise empty → nothing drawn). Dim by the global
        // visualizer opacity and cap to the GPU buffer size.
        let particle_data = if mode == VisualizationMode::Scope && params.scope_particles {
            let opacity = params.global_opacity.clamp(0.0, 1.0);
            let mut p = state.get_particles();
            p.truncate(VisualizerPipeline::MAX_PARTICLES);
            for particle in &mut p {
                particle[3] *= opacity; // alpha channel
            }
            p
        } else {
            Vec::new()
        };
        let particle_count = particle_data.len() as u32;

        Self {
            gradient_colors: gradient,
            peak_gradient_colors: peak_gradient,
            peak_color: peak_col,
            border_color: border_col,
            config,
            state: state.clone(),
            has_perspective,
            bloom_enabled,
            bloom_intensity: params.bloom_intensity,
            beat_reactivity: params.beat_reactivity,
            trails_enabled,
            trails_decay,
            echo_enabled,
            echo: params.echo,
            crt_enabled,
            crt: params.crt,
            particle_data,
            particle_count,
            scope_beam: params.scope_beam,
        }
    }
}

/// GPU pipeline for visualizer rendering
pub(crate) struct VisualizerPipeline {
    pub(super) bars_pipeline: wgpu::RenderPipeline,
    pub(super) bars_pipeline_msaa: wgpu::RenderPipeline,
    pub(super) lines_pipeline: wgpu::RenderPipeline,
    pub(super) lines_pipeline_msaa: wgpu::RenderPipeline,
    pub(super) scope_pipeline: wgpu::RenderPipeline,
    pub(super) scope_pipeline_msaa: wgpu::RenderPipeline,
    pub(super) particle_pipeline: wgpu::RenderPipeline,
    pub(super) particle_pipeline_msaa: wgpu::RenderPipeline,
    pub(super) scope_pipeline_beam: wgpu::RenderPipeline,
    pub(super) scope_pipeline_beam_msaa: wgpu::RenderPipeline,
    pub(super) uniform_buffer: wgpu::Buffer,
    pub(super) bar_buffer: wgpu::Buffer,
    pub(super) particle_buffer: wgpu::Buffer,
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
    // --- Bloom post-processing ---
    /// resolve_texture -> half-res bloom (horizontal blur + soft-knee threshold)
    pub(super) bloom_bright_pipeline: wgpu::RenderPipeline,
    /// half-res bloom -> half-res bloom (vertical blur)
    pub(super) bloom_blur_v_pipeline: wgpu::RenderPipeline,
    /// half-res bloom -> framebuffer (additive composite, One/One blend)
    pub(super) bloom_composite_pipeline: wgpu::RenderPipeline,
    /// Bind group layout for bloom passes (texture + sampler + BloomParams uniform)
    pub(super) bloom_bind_group_layout: wgpu::BindGroupLayout,
    /// BloomParams uniform (intensity + threshold), refreshed each frame in prepare()
    pub(super) bloom_uniform_buffer: wgpu::Buffer,
    /// Half-res ping-pong bloom targets (created alongside the resolve texture)
    pub(super) bloom_texture_a: Option<(wgpu::Texture, wgpu::TextureView)>,
    pub(super) bloom_texture_b: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// Bloom bind groups, recreated with the textures: sample resolve / bloom_a / bloom_b
    pub(super) bloom_bg_scene: Option<wgpu::BindGroup>,
    pub(super) bloom_bg_a: Option<wgpu::BindGroup>,
    pub(super) bloom_bg_b: Option<wgpu::BindGroup>,
    // --- Motion trails ---
    /// In-place fade pass (trail *= decay via a Zero/Constant blend; the
    /// decay is the per-frame blend constant). Reuses the blit bind group.
    pub(super) trail_fade_pipeline: wgpu::RenderPipeline,
    /// In-place max-blend pass (trail = max(scene, faded trail)) so bright
    /// motion leaves a fading ghost without saturating. Samples resolve.
    pub(super) trail_max_pipeline: wgpu::RenderPipeline,
    /// Persistent full-res trail accumulator (NOT cleared between frames).
    pub(super) trail_texture: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// Blit bind group that samples the trail texture (for the final display).
    pub(super) blit_bg_trail: Option<wgpu::BindGroup>,
    /// Whether trails rendered last frame. The accumulator is only reallocated
    /// on resize, so prepare() uses this to detect the off->on transition.
    pub(super) trails_were_active: bool,
    /// Set by prepare() on the off->on transition: the trail fade pass clears
    /// instead of loads, so a stale trail from a prior session can't ghost back
    /// in. (A freshly resized accumulator is already zero, so it's a no-op then.)
    pub(super) trail_needs_clear: bool,
    // --- Echo (Milkdrop zoom/rotate feedback) ---
    /// Feedback pass: `max(scene, decay * echo_prev(warp(uv)))` -> echo_texture.
    pub(super) echo_feedback_pipeline: wgpu::RenderPipeline,
    /// Bind group layout: scene tex + prev-echo (scratch) tex + sampler + EchoParams.
    pub(super) echo_bind_group_layout: wgpu::BindGroupLayout,
    /// EchoParams uniform (decay + warp), refreshed each frame in prepare().
    pub(super) echo_uniform_buffer: wgpu::Buffer,
    /// Persistent echo accumulator (written by the feedback pass, then displayed).
    pub(super) echo_texture: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// Per-frame copy of the accumulator that the feedback pass samples (warped),
    /// so the warped read never aliases the write — no ping-pong parity needed.
    pub(super) echo_temp: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// Feedback bind group (resolve scene + echo_temp + sampler + uniform).
    pub(super) echo_feedback_bg: Option<wgpu::BindGroup>,
    /// Blit bind group that samples echo_texture for the final display.
    pub(super) blit_bg_echo: Option<wgpu::BindGroup>,
    /// Off->on transition tracking (clear stale echo on re-enable, like trails).
    pub(super) echo_were_active: bool,
    pub(super) echo_needs_clear: bool,
    // --- CRT / film composite ---
    /// Post-process pipeline: samples the display source (group 0 = the blit
    /// bind-group layout) + CrtParams (group 1), applies the retro stack, and
    /// writes the framebuffer in place of the plain blit.
    pub(super) crt_pipeline: wgpu::RenderPipeline,
    /// CrtParams uniform (amount + beat + time), refreshed each frame.
    pub(super) crt_uniform_buffer: wgpu::Buffer,
    /// Bind group for the CrtParams uniform (group 1; reused every frame).
    pub(super) crt_uniform_bind_group: wgpu::BindGroup,
}

/// Bloom pass parameters — a small standalone uniform, deliberately NOT part of
/// the 8336-byte `VisualizerConfig` interlock.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub(super) struct BloomParams {
    pub(super) intensity: f32,
    pub(super) threshold: f32,
    pub(super) _pad: [f32; 2],
}

unsafe impl bytemuck::Pod for BloomParams {}
unsafe impl bytemuck::Zeroable for BloomParams {}

/// Echo (Milkdrop zoom/rotate feedback) params — a small standalone uniform.
/// The feedback warps the previous frame by `zoom` + a rotation given as
/// precomputed `sin_a`/`cos_a`, then fades it by `decay` and maxes the scene on
/// top. `decay = 0` clears the accumulator (used on the off->on transition).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub(super) struct EchoParams {
    pub(super) decay: f32,
    pub(super) zoom: f32,
    pub(super) sin_a: f32,
    pub(super) cos_a: f32,
}

unsafe impl bytemuck::Pod for EchoParams {}
unsafe impl bytemuck::Zeroable for EchoParams {}

/// CRT / film composite params — master `amount` plus the beat pulse (zoom
/// punch) and time (grain + scanline scroll). A small standalone uniform.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub(super) struct CrtParams {
    pub(super) amount: f32,
    pub(super) beat: f32,
    pub(super) time: f32,
    pub(super) _pad: f32,
}

unsafe impl bytemuck::Pod for CrtParams {}
unsafe impl bytemuck::Zeroable for CrtParams {}

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
    /// Audio signals for beat-reactive effects: `[beat * reactivity, bass, mid, treble]`.
    /// Appended after `config` (lands 16-aligned), so it sits OUTSIDE the
    /// pinned 8336-byte Config layout — the WGSL `Uniforms` mirrors mirror it
    /// with a trailing `audio: vec4<f32>` in bars.wgsl + lines.wgsl.
    pub(super) audio: [f32; 4],
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
        scope_pipeline: &wgpu::RenderPipeline,
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
            2 => {
                // Scope mode (circular oscilloscope)
                render_pass.set_pipeline(scope_pipeline);

                // Closed loop: one segment per waveform point, +1 closing sample
                // to seal the triangle strip. MUST match SCOPE_SP in scope.wgsl.
                let samples_per_segment = SCOPE_SAMPLES_PER_SEGMENT;
                let total_samples = bar_count * samples_per_segment;
                let ring_points = total_samples + 1;
                let vertices_per_spline_point = 2u32;
                let vertices_per_pass = ring_points * vertices_per_spline_point;

                // Instance 0 = fill, 1 = outline (under), 2 = main line (on top).
                render_pass.draw(0..vertices_per_pass, 0..3);
            }
            _ => {}
        }
    }

    /// Draw the Scope particle field (instanced glowing quads, additive). No-op
    /// when there are no particles (every non-scope mode + particles-off).
    /// Shares the bind group with the bars/lines/scope draw; the additive blend
    /// lives on the pipeline.
    fn draw_particles(
        particle_pipeline: &wgpu::RenderPipeline,
        bind_group: &wgpu::BindGroup,
        particle_count: u32,
        render_pass: &mut wgpu::RenderPass<'_>,
    ) {
        if particle_count == 0 {
            return;
        }
        render_pass.set_pipeline(particle_pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        // 6 vertices per particle (two triangles), one instance per particle.
        render_pass.draw(0..6, 0..particle_count);
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

        let scope_pipe = if self.scope_beam {
            &pipeline.scope_pipeline_beam
        } else {
            &pipeline.scope_pipeline
        };
        Self::draw_bars_and_lines(
            &self.config,
            &pipeline.bind_group,
            &pipeline.bars_pipeline,
            &pipeline.lines_pipeline,
            scope_pipe,
            &mut render_pass,
        );
        Self::draw_particles(
            &pipeline.particle_pipeline,
            &pipeline.bind_group,
            self.particle_count,
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

        // Convert to f32 for GPU. Scope mode uploads the signed time-domain
        // waveform into the same storage buffer the bars/lines shaders read as
        // `bar_data` — scope.wgsl interprets the entries as signed (-1..1).
        let bar_data: Vec<f32> = if self.config.mode == 2 {
            self.state.get_waveform()
        } else {
            fresh_bars.iter().map(|&v| v as f32).collect()
        };
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
            audio: {
                // [beat (scaled by reactivity, drives the glow flare + bar
                // pump), bass, mid, treble]. Bands are raw for future
                // band-driven shader effects (motion trails, warp).
                let (bass, mid, treble) = self.state.current_bands();
                let beat = self.state.current_beat_pulse() * self.beat_reactivity;
                [beat, bass, mid, treble]
            },
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

        // Update particle data (Scope mode; only the first particle_count entries
        // are read by the draw, so no padding is needed).
        if !self.particle_data.is_empty() {
            queue.write_buffer(
                &pipeline.particle_buffer,
                0,
                bytemuck::cast_slice(&self.particle_data),
            );
        }

        // Create/resize offscreen targets if perspective/3D, bloom, trails, OR
        // echo are active (all need the resolve texture; bloom adds the half-res
        // targets, trails the accumulator, echo the accumulator + scratch).
        if self.has_perspective
            || self.bloom_enabled
            || self.trails_enabled
            || self.echo_enabled
            || self.crt_enabled
        {
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

                // --- Half-res ping-pong bloom targets + their bind groups ---
                let bw = (w / 2).max(1);
                let bh = (h / 2).max(1);
                let bloom_size = wgpu::Extent3d {
                    width: bw,
                    height: bh,
                    depth_or_array_layers: 1,
                };
                let bloom_desc = wgpu::TextureDescriptor {
                    label: Some("visualizer bloom texture"),
                    size: bloom_size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: pipeline.format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                };
                let bloom_a = device.create_texture(&bloom_desc);
                let bloom_a_view = bloom_a.create_view(&wgpu::TextureViewDescriptor::default());
                let bloom_b = device.create_texture(&bloom_desc);
                let bloom_b_view = bloom_b.create_view(&wgpu::TextureViewDescriptor::default());

                // Bind groups: bloom_bg_scene samples resolve, bg_a samples
                // bloom_a, bg_b samples bloom_b — all share the bloom uniform.
                let bloom_bg_scene = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("visualizer bloom bg scene"),
                    layout: &pipeline.bloom_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&resolve_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&pipeline.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: pipeline.bloom_uniform_buffer.as_entire_binding(),
                        },
                    ],
                });
                let bloom_bg_a = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("visualizer bloom bg a"),
                    layout: &pipeline.bloom_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&bloom_a_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&pipeline.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: pipeline.bloom_uniform_buffer.as_entire_binding(),
                        },
                    ],
                });
                let bloom_bg_b = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("visualizer bloom bg b"),
                    layout: &pipeline.bloom_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&bloom_b_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&pipeline.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: pipeline.bloom_uniform_buffer.as_entire_binding(),
                        },
                    ],
                });

                // --- Persistent full-res trail accumulator + its blit bind group ---
                let trail_tex = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("visualizer trail texture"),
                    size: wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    // Float, NOT the 8-bit surface format — see TRAIL_FORMAT.
                    format: TRAIL_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                let trail_view = trail_tex.create_view(&wgpu::TextureViewDescriptor::default());
                let blit_bg_trail = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("visualizer trail blit bind group"),
                    layout: &pipeline.blit_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&trail_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&pipeline.sampler),
                        },
                    ],
                });

                // --- Echo (Milkdrop feedback): accumulator + scratch + bgs ---
                let echo_size = wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                };
                let echo_tex = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("visualizer echo texture"),
                    size: echo_size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: TRAIL_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                });
                let echo_tex_view = echo_tex.create_view(&wgpu::TextureViewDescriptor::default());
                let echo_temp_tex = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("visualizer echo scratch"),
                    size: echo_size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: TRAIL_FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let echo_temp_view =
                    echo_temp_tex.create_view(&wgpu::TextureViewDescriptor::default());
                let echo_feedback_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("visualizer echo feedback bind group"),
                    layout: &pipeline.echo_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&resolve_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&echo_temp_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&pipeline.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: pipeline.echo_uniform_buffer.as_entire_binding(),
                        },
                    ],
                });
                let blit_bg_echo = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("visualizer echo blit bind group"),
                    layout: &pipeline.blit_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&echo_tex_view),
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
                pipeline.bloom_texture_a = Some((bloom_a, bloom_a_view));
                pipeline.bloom_texture_b = Some((bloom_b, bloom_b_view));
                pipeline.bloom_bg_scene = Some(bloom_bg_scene);
                pipeline.bloom_bg_a = Some(bloom_bg_a);
                pipeline.bloom_bg_b = Some(bloom_bg_b);
                pipeline.trail_texture = Some((trail_tex, trail_view));
                pipeline.blit_bg_trail = Some(blit_bg_trail);
                pipeline.echo_texture = Some((echo_tex, echo_tex_view));
                pipeline.echo_temp = Some((echo_temp_tex, echo_temp_view));
                pipeline.echo_feedback_bg = Some(echo_feedback_bg);
                pipeline.blit_bg_echo = Some(blit_bg_echo);
                pipeline.msaa_size = (w, h);
            }
        }

        // Refresh the bloom uniform every frame (intensity tracks config without
        // necessarily triggering a texture resize).
        if self.bloom_enabled {
            // Surge on bass drops specifically (bass weighted heavily) plus a
            // little any-transient beat, scaled by the user's beat reactivity.
            // At reactivity 0 the glow holds steady at the configured strength.
            let beat = self.state.current_beat_pulse();
            let (bass, _, _) = self.state.current_bands();
            let dip_base = 1.0 - self.beat_reactivity * (1.0 - BLOOM_DIP);
            let pump = (BLOOM_BEAT_GAIN * beat + BLOOM_BASS_GAIN * bass) * self.beat_reactivity;
            let intensity = self.bloom_intensity * (dip_base + pump);
            let bloom_params = BloomParams {
                intensity,
                threshold: BLOOM_THRESHOLD,
                _pad: [0.0; 2],
            };
            queue.write_buffer(
                &pipeline.bloom_uniform_buffer,
                0,
                bytemuck::bytes_of(&bloom_params),
            );
        }

        // Trails just turned on: the accumulator still holds whatever it had
        // when trails were last disabled (it's only reallocated on resize), so
        // tell the render trail-fade pass to clear it instead of loading —
        // otherwise the stale trail fades back in over ~1s on re-enable.
        pipeline.trail_needs_clear = self.trails_enabled && !pipeline.trails_were_active;
        pipeline.trails_were_active = self.trails_enabled;

        // Echo just turned on: clear the stale accumulator on the first frame
        // (decay = 0 below makes the feedback ignore the prev), same idea as
        // trails. Then refresh the echo uniform (warp pulses with bass/beat).
        pipeline.echo_needs_clear = self.echo_enabled && !pipeline.echo_were_active;
        pipeline.echo_were_active = self.echo_enabled;
        if self.echo_enabled {
            let beat = self.state.current_beat_pulse();
            let (bass, _, _) = self.state.current_bands();
            let react = self.beat_reactivity;
            let decay = if pipeline.echo_needs_clear {
                0.0
            } else {
                self.echo * ECHO_MAX_DECAY
            };
            let zoom = 1.0 + ECHO_BASE_ZOOM + bass * ECHO_BASS_ZOOM * react;
            let angle = ECHO_BASE_ROT + beat * ECHO_BEAT_ROT * react;
            let (sin_a, cos_a) = angle.sin_cos();
            let echo_params = EchoParams {
                decay,
                zoom,
                sin_a,
                cos_a,
            };
            queue.write_buffer(
                &pipeline.echo_uniform_buffer,
                0,
                bytemuck::bytes_of(&echo_params),
            );
        }

        // Refresh the CRT uniform (master amount + beat zoom-punch + time).
        if self.crt_enabled {
            let crt_params = CrtParams {
                amount: self.crt,
                beat: self.state.current_beat_pulse(),
                time: get_elapsed_time(),
                _pad: 0.0,
            };
            queue.write_buffer(
                &pipeline.crt_uniform_buffer,
                0,
                bytemuck::bytes_of(&crt_params),
            );
        }
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        let bar_count = self.config.bar_count;
        if bar_count == 0 {
            return true;
        }

        // Perspective/3D (MSAA), bloom, trails, and echo all need the offscreen
        // render() path (to sample/accumulate the scene).
        if self.has_perspective
            || self.bloom_enabled
            || self.trails_enabled
            || self.echo_enabled
            || self.crt_enabled
        {
            return false;
        }

        let scope_pipe = if self.scope_beam {
            &pipeline.scope_pipeline_beam
        } else {
            &pipeline.scope_pipeline
        };
        Self::draw_bars_and_lines(
            &self.config,
            &pipeline.bind_group,
            &pipeline.bars_pipeline,
            &pipeline.lines_pipeline,
            scope_pipe,
            render_pass,
        );
        Self::draw_particles(
            &pipeline.particle_pipeline,
            &pipeline.bind_group,
            self.particle_count,
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

            let scope_pipe = if self.scope_beam {
                &pipeline.scope_pipeline_beam_msaa
            } else {
                &pipeline.scope_pipeline_msaa
            };
            Self::draw_bars_and_lines(
                &self.config,
                &pipeline.bind_group,
                &pipeline.bars_pipeline_msaa,
                &pipeline.lines_pipeline_msaa,
                scope_pipe,
                &mut render_pass,
            );
            Self::draw_particles(
                &pipeline.particle_pipeline_msaa,
                &pipeline.bind_group,
                self.particle_count,
                &mut render_pass,
            );
        }

        // Bloom blur passes (BP1: resolve -> bloom_a with bright/H, BP2: bloom_a
        // -> bloom_b with V). The match result is Copy (all refs), so the same
        // handles drive the additive composite after the scene blit below.
        let bloom_views = match (
            self.bloom_enabled,
            &pipeline.bloom_texture_a,
            &pipeline.bloom_texture_b,
            &pipeline.bloom_bg_scene,
            &pipeline.bloom_bg_a,
            &pipeline.bloom_bg_b,
        ) {
            (true, Some((_, av)), Some((_, bv)), Some(bgs), Some(bga), Some(bgb)) => {
                Some((av, bv, bgs, bga, bgb))
            }
            _ => None,
        };

        if let Some((bloom_a_view, bloom_b_view, bg_scene, bg_a, _)) = bloom_views {
            let bw = (pipeline.msaa_size.0 / 2).max(1);
            let bh = (pipeline.msaa_size.1 / 2).max(1);

            for (label, view, blur_pipeline, bind_group) in [
                (
                    "visualizer bloom bright/H pass",
                    bloom_a_view,
                    &pipeline.bloom_bright_pipeline,
                    bg_scene,
                ),
                (
                    "visualizer bloom blur V pass",
                    bloom_b_view,
                    &pipeline.bloom_blur_v_pipeline,
                    bg_a,
                ),
            ] {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(label),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_viewport(0.0, 0.0, bw as f32, bh as f32, 0.0, 1.0);
                pass.set_scissor_rect(0, 0, bw, bh);
                pass.set_pipeline(blur_pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
        }

        // Motion trails: fade the persistent accumulator (trail *= decay), then
        // max-blend the current scene onto it (trail = max(scene, faded trail)).
        // The accumulator (scene + fading history) then becomes what we display.
        // Refs are Copy, so the same handle drives the blit-source swap below.
        let trail_handles = match (
            self.trails_enabled,
            &pipeline.trail_texture,
            &pipeline.blit_bg_trail,
        ) {
            (true, Some((_, tv)), Some(bg)) => Some((tv, bg)),
            _ => None,
        };

        if let Some((trail_view, _)) = trail_handles {
            let (tex_w, tex_h) = pipeline.msaa_size;
            let decay = self.trails_decay as f64;

            // Fade pass: scale the accumulator by the decay blend constant. On
            // the frame trails turn back on, clear it instead of loading so a
            // stale trail from a prior session can't ghost back in (see
            // prepare(): trail_needs_clear).
            let fade_load = if pipeline.trail_needs_clear {
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
            } else {
                wgpu::LoadOp::Load
            };
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("visualizer trail fade pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: trail_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: fade_load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_viewport(0.0, 0.0, tex_w as f32, tex_h as f32, 0.0, 1.0);
                pass.set_scissor_rect(0, 0, tex_w, tex_h);
                pass.set_blend_constant(wgpu::Color {
                    r: decay,
                    g: decay,
                    b: decay,
                    a: decay,
                });
                pass.set_pipeline(&pipeline.trail_fade_pipeline);
                pass.set_bind_group(0, blit_bg, &[]); // ignored by fs_fade
                pass.draw(0..3, 0..1);
            }

            // Max pass: composite the current scene onto the faded trail.
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("visualizer trail max pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: trail_view,
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
                pass.set_viewport(0.0, 0.0, tex_w as f32, tex_h as f32, 0.0, 1.0);
                pass.set_scissor_rect(0, 0, tex_w, tex_h);
                pass.set_pipeline(&pipeline.trail_max_pipeline);
                pass.set_bind_group(0, blit_bg, &[]); // samples resolve (the scene)
                pass.draw(0..3, 0..1);
            }
        }

        // Echo (Milkdrop feedback): copy the accumulator into the scratch (so the
        // warped read can't alias the write), then run one feedback pass that
        // warps + fades the scratch and maxes the scene on top. Echo, when on,
        // takes over the display.
        let echo_handles = match (
            self.echo_enabled,
            &pipeline.echo_texture,
            &pipeline.echo_temp,
            &pipeline.echo_feedback_bg,
            &pipeline.blit_bg_echo,
        ) {
            (true, Some((etex, ev)), Some((ttex, _)), Some(fbg), Some(dbg)) => {
                Some((etex, ev, ttex, fbg, dbg))
            }
            _ => None,
        };

        if let Some((echo_tex, echo_view, echo_temp_tex, feedback_bg, _)) = echo_handles {
            let (tex_w, tex_h) = pipeline.msaa_size;
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: echo_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: echo_temp_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: tex_w,
                    height: tex_h,
                    depth_or_array_layers: 1,
                },
            );

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("visualizer echo feedback pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: echo_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // REPLACE blend covers every texel, so the prior contents
                        // are never read — clear (matches the bloom blur passes).
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(0.0, 0.0, tex_w as f32, tex_h as f32, 0.0, 1.0);
            pass.set_scissor_rect(0, 0, tex_w, tex_h);
            pass.set_pipeline(&pipeline.echo_feedback_pipeline);
            pass.set_bind_group(0, feedback_bg, &[]);
            pass.draw(0..3, 0..1);
        }

        // The displayed scene is the echo accumulator when echo is on, else the
        // trail accumulator when trails are on, else the raw resolve texture.
        let display_bg = match echo_handles {
            Some((.., dbg)) => dbg,
            None => match trail_handles {
                Some((_, bg)) => bg,
                None => blit_bg,
            },
        };

        // Pass 2: Blit the displayed scene onto the framebuffer with premultiplied alpha blending
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

            // When CRT is on, the display blit runs through the CRT post-process
            // pipeline instead (same display texture at group 0, CrtParams at
            // group 1). Bloom still composites on top afterward.
            if self.crt_enabled {
                blit_pass.set_pipeline(&pipeline.crt_pipeline);
                blit_pass.set_bind_group(0, display_bg, &[]);
                blit_pass.set_bind_group(1, &pipeline.crt_uniform_bind_group, &[]);
            } else {
                blit_pass.set_pipeline(&pipeline.blit_pipeline);
                blit_pass.set_bind_group(0, display_bg, &[]);
            }
            blit_pass.draw(0..3, 0..1); // Single fullscreen triangle
        }

        // Pass 3: additively composite the blurred bloom over the scene.
        if let Some((.., bg_bloom)) = bloom_views {
            let mut bloom_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("visualizer bloom composite pass"),
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

            let (tex_w, tex_h) = pipeline.msaa_size;
            bloom_pass.set_viewport(
                clip_bounds.x as f32,
                clip_bounds.y as f32,
                tex_w as f32,
                tex_h as f32,
                0.0,
                1.0,
            );
            bloom_pass.set_scissor_rect(clip_bounds.x, clip_bounds.y, width, height);

            bloom_pass.set_pipeline(&pipeline.bloom_composite_pipeline);
            bloom_pass.set_bind_group(0, bg_bloom, &[]);
            bloom_pass.draw(0..3, 0..1);
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
        // Request a redraw when the FFT thread has produced new data, OR (with
        // motion trails on) while the trail is still draining after audio
        // stopped — otherwise a paused trail freezes on screen instead of
        // fading out. Both conditions converge to false when nothing is
        // animating, so a paused/stopped visualizer still drops to near-zero
        // GPU once the trail has faded.
        let feedback_draining =
            (self.params.trails > 0.001 || self.params.echo > 0.001) && self.state.trail_draining();
        if self.state.is_dirty() || feedback_draining {
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

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn config_size_and_alignment_are_pinned() {
        assert_eq!(core::mem::size_of::<VisualizerConfig>(), 8336);
        assert_eq!(core::mem::align_of::<VisualizerConfig>(), 16);
        assert_eq!(core::mem::size_of::<VisualizerConfig>() % 16, 0);
    }

    #[test]
    fn flash_data_is_sixteen_byte_aligned() {
        assert_eq!(core::mem::offset_of!(VisualizerConfig, scope_radius), 136);
        assert_eq!(core::mem::offset_of!(VisualizerConfig, flash_data) % 16, 0);
    }
}
