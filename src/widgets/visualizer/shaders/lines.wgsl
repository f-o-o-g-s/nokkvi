// Visualizer Lines Shader
// GPU-accelerated smooth antialiased line rendering with Catmull-Rom spline interpolation
// Antialiasing technique from: https://www.shadertoy.com/view/4dcfW8
// Uses instanced rendering: instance 0 = outline, instance 1 = main line
//
// ⚠️  Config struct layout MUST match VisualizerConfig in shader.rs byte-for-byte.
//     If you add/remove/reorder fields, update ALL THREE locations:
//       1. src/widgets/visualizer/shader.rs          (VisualizerConfig)
//       2. src/widgets/visualizer/shaders/bars.wgsl  (Config)
//       3. src/widgets/visualizer/shaders/lines.wgsl (Config)

struct Uniforms {
    viewport: vec4<f32>,  // x, y, width, height in PIXELS
    gradient_colors: array<vec4<f32>, 8>,  // Expanded to 8 colors for more variety
    peak_gradient_colors: array<vec4<f32>, 8>,  // Peak breathing colors (matches bars.wgsl layout)
    peak_color: vec4<f32>,
    border_color: vec4<f32>,
    config: Config,
}

struct Config {
    bar_count: u32,
    mode: u32,  // 0 = bars, 1 = lines
    border_width: f32,
    peak_enabled: u32,
    peak_thickness: f32,
    peak_alpha: f32,
    line_thickness: f32,  // Line thickness in pixels
    bar_width: f32,
    bar_spacing: f32,
    edge_spacing: f32,
    time: f32,  // Time in seconds for animation
    led_bars: u32,       // 0 = normal bars, 1 = LED segmented bars (not used in lines mode)
    led_segment_height: f32,  // Height of each LED segment in pixels (not used in lines mode)
    led_border_opacity: f32,  // Border opacity in LED mode (not used in lines mode)
    border_opacity: f32,      // Border opacity in non-LED mode (not used in lines mode)
    gradient_mode: u32,       // 0 = static gradient, 1 = breathing animation
    peak_gradient_mode: u32,  // 0=static, 1=cycle, 2=height, 3=match (not used in lines mode)
    peak_mode: u32,           // 0=none, 1=fade, 2=fall, 3=fall_accel (not used in lines mode)
    peak_hold_time: f32,      // Time in seconds for peak to hold (not used in lines mode)
    peak_fade_time: f32,      // Time in seconds for peak to fade (not used in lines mode)
    flash_count: u32,         // Number of bars (not used in lines mode)
    bar_depth_3d: f32,        // Isometric 3D depth in pixels (not used in lines mode)
    gradient_orientation: u32, // 0 = vertical, 1 = horizontal (not used in lines mode)
    average_energy: f32,       // Average bar amplitude (not used in lines mode)
    global_opacity: f32,       // Overall visualizer opacity (0.0-1.0)
    _pad: u32,                 // Padding for 16-byte alignment before flash_data
    // Flash intensities: one per bar (0.0-1.0), stored as vec4s
    // Up to 2048 bars = 512 vec4s (not used in lines mode)
    flash_data: array<vec4<f32>, 512>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var<storage, read> bar_data: array<f32>;  // Point heights (0.0 - 1.0)
@group(0) @binding(2) var<storage, read> peak_data: array<f32>;  // Not used in lines mode
@group(0) @binding(3) var<storage, read> peak_alpha_data: array<f32>;  // Not used in lines mode

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) distance_to_center: f32,  // Distance from center line for antialiasing
    @location(2) is_outline: f32,  // 1.0 = outline, 0.0 = main line
}

// Convert pixel coordinates to NDC (-1 to 1)
fn pixel_to_ndc(pixel_x: f32, pixel_y: f32) -> vec2<f32> {
    let viewport = uniforms.viewport;
    let ndc_x = (pixel_x / viewport.z) * 2.0 - 1.0;
    let ndc_y = 1.0 - (pixel_y / viewport.w) * 2.0;
    return vec2<f32>(ndc_x, ndc_y);
}

// Get gradient color based on time (breathing animation cycling through all colors)
fn get_gradient_color_animated(time: f32) -> vec4<f32> {
    // Cycle speed: complete cycle through all colors in ~4 seconds
    let cycle_speed = 0.25;  // Lower = slower
    let t = fract(time * cycle_speed);
    
    // Interpolate through 8 gradient colors (0-7)
    let segments = 8.0;
    let pos = t * segments;
    let idx = u32(floor(pos)) % 8u;
    let next_idx = (idx + 1u) % 8u;
    let frac = pos - floor(pos);
    
    let c1 = uniforms.gradient_colors[idx];
    let c2 = uniforms.gradient_colors[next_idx];
    
    return mix(c1, c2, frac);
}

// Catmull-Rom spline interpolation
fn catmull_rom(p0: vec2<f32>, p1: vec2<f32>, p2: vec2<f32>, p3: vec2<f32>, t: f32) -> vec2<f32> {
    let t2 = t * t;
    let t3 = t2 * t;
    
    return 0.5 * (
        (2.0 * p1) +
        (-p0 + p2) * t +
        (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2 +
        (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3
    );
}

// Get point at index, clamping to valid range
fn get_point(idx: i32, point_count: i32, canvas_width: f32, canvas_height: f32) -> vec2<f32> {
    let clamped_idx = clamp(idx, 0, point_count - 1);
    let value = bar_data[u32(clamped_idx)];
    
    // Clamp value to 0-1 range (CAVA auto-sensitivity can produce values > 1.0)
    let clamped_value = clamp(value, 0.0, 1.0);
    
    // X position: evenly distributed across width
    let x = f32(clamped_idx) / f32(point_count - 1) * canvas_width;
    
    // Y position: value maps to height (0 = bottom, 1 = top)
    // Reserve space at top for maximum line expansion to prevent clipping
    // Double the margin to account for worst-case perpendicular offset at steep angles
    let line_thickness = max(uniforms.config.line_thickness, 2.0);
    let outline_extra = 2.0;
    let aa_padding = 1.5;
    let max_expansion = ((line_thickness * 0.5) + outline_extra + aa_padding) * 4.0;
    let drawable_height = canvas_height - max_expansion;
    
    let y = canvas_height - (clamped_value * drawable_height);
    
    return vec2<f32>(x, y);
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32
) -> VertexOutput {
    var output: VertexOutput;
    
    let point_count = i32(uniforms.config.bar_count);
    let line_thickness = max(uniforms.config.line_thickness, 2.0);
    let half_thickness = line_thickness * 0.5;
    let viewport = uniforms.viewport;
    let canvas_width = viewport.z;
    let canvas_height = viewport.w;
    
    // Determine if this is outline (instance 0) or main line (instance 1)
    let is_outline_pass = instance_index == 0u;
    
    if (point_count < 2) {
        output.position = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
        output.color = vec4<f32>(0.0);
        output.distance_to_center = 0.0;
        output.is_outline = 0.0;
        return output;
    }
    
    // Spline interpolation: 16 samples per segment for smoother curves
    let samples_per_segment = 16;
    let num_segments = point_count - 1;
    let total_spline_points = num_segments * samples_per_segment + 1;
    
    // Each spline point needs 2 vertices (left and right of line)
    let vertices_per_spline = 2;
    let vertices_per_pass = u32(total_spline_points) * u32(vertices_per_spline);
    
    if (vertex_index >= vertices_per_pass) {
        output.position = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
        output.color = vec4<f32>(0.0);
        output.distance_to_center = 0.0;
        output.is_outline = 0.0;
        return output;
    }
    
    // Which spline point is this vertex for?
    let spline_point_index = vertex_index / 2u;
    let is_left_side = (vertex_index % 2u) == 0u;
    
    // Calculate which segment and t value within segment
    let segment_index = i32(spline_point_index) / samples_per_segment;
    let sample_in_segment = i32(spline_point_index) % samples_per_segment;
    let t = f32(sample_in_segment) / f32(samples_per_segment);
    
    // Handle the final point
    var current_point: vec2<f32>;
    var next_point: vec2<f32>;
    var prev_point: vec2<f32>;
    
    if (i32(spline_point_index) >= total_spline_points - 1) {
        // Last point - use tangent from prev to current, don't extrapolate
        current_point = get_point(point_count - 1, point_count, canvas_width, canvas_height);
        prev_point = get_point(point_count - 2, point_count, canvas_width, canvas_height);
        // Use current_point as next_point to avoid extrapolation that causes artifacts
        next_point = current_point;
    } else if (segment_index >= num_segments) {
        // Safety: clamp to last segment
        current_point = get_point(point_count - 1, point_count, canvas_width, canvas_height);
        prev_point = get_point(point_count - 2, point_count, canvas_width, canvas_height);
        next_point = current_point;
    } else {
        // Get control points for Catmull-Rom spline
        let p0 = get_point(segment_index - 1, point_count, canvas_width, canvas_height);
        let p1 = get_point(segment_index, point_count, canvas_width, canvas_height);
        let p2 = get_point(segment_index + 1, point_count, canvas_width, canvas_height);
        let p3 = get_point(segment_index + 2, point_count, canvas_width, canvas_height);
        
        // Interpolate
        current_point = catmull_rom(p0, p1, p2, p3, t);
        
        // Get tangent for perpendicular calculation
        let t_prev = max(t - 0.01, 0.0);
        let t_next = min(t + 0.01, 1.0);
        prev_point = catmull_rom(p0, p1, p2, p3, t_prev);
        next_point = catmull_rom(p0, p1, p2, p3, t_next);
    }
    
    // Clamp Y coordinates after spline interpolation
    // Catmull-Rom splines can overshoot at sharp peaks (low-HIGH-low pattern)
    // The minimum safe Y is max_expansion (to leave room for line thickness at top)
    let outline_extra = 2.0;
    let aa_padding = 1.5;
    let max_expansion = half_thickness + outline_extra + aa_padding;
    current_point.y = clamp(current_point.y, max_expansion, canvas_height);
    prev_point.y = clamp(prev_point.y, max_expansion, canvas_height);
    next_point.y = clamp(next_point.y, max_expansion, canvas_height);
    
    // Calculate perpendicular direction for line thickness
    var tangent = next_point - prev_point;
    let tangent_len = length(tangent);
    if (tangent_len > 0.001) {
        tangent = tangent / tangent_len;
    } else {
        tangent = vec2<f32>(1.0, 0.0);
    }
    
    // Perpendicular is 90 degrees rotated
    let perp = vec2<f32>(-tangent.y, tangent.x);
    
    // Determine thickness based on outline vs main line
    // Outline is 2px thicker on each side (outline_extra already defined above)
    let actual_half_thickness = select(half_thickness, half_thickness + outline_extra, is_outline_pass);
    
    // Extend line width slightly for antialiasing (aa_padding already defined above)
    let extended_half_thickness = actual_half_thickness + aa_padding;
    
    // Offset vertex to left or right of center line
    var offset_point: vec2<f32>;
    var dist_to_center: f32;
    if (is_left_side) {
        offset_point = current_point + perp * extended_half_thickness;
        dist_to_center = extended_half_thickness;
    } else {
        offset_point = current_point - perp * extended_half_thickness;
        dist_to_center = -extended_half_thickness;
    }
    
    // Clamp offset point to stay within canvas bounds
    // Line thickness can push vertices outside the visible area
    offset_point.y = clamp(offset_point.y, 0.0, canvas_height);
    
    // Calculate color - use time-based breathing animation
    var color: vec4<f32>;
    if (is_outline_pass) {
        color = uniforms.border_color;  // Dark outline
    } else {
        color = get_gradient_color_animated(uniforms.config.time);  // Animated gradient
    }
    
    // Convert to NDC
    let ndc = pixel_to_ndc(offset_point.x, offset_point.y);
    
    output.position = vec4<f32>(ndc.x, ndc.y, 0.0, 1.0);
    output.color = color;
    output.distance_to_center = dist_to_center;
    output.is_outline = select(0.0, 1.0, is_outline_pass);
    
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Antialiasing using smoothstep based on distance from center line
    // Technique from: https://www.shadertoy.com/view/4dcfW8
    let line_thickness = max(uniforms.config.line_thickness, 2.0);
    let half_thickness = line_thickness * 0.5;
    
    // Add outline thickness if this is the outline pass
    let outline_extra = 2.0;
    let actual_half_thickness = select(half_thickness, half_thickness + outline_extra, input.is_outline > 0.5);
    
    // Distance from center line (in pixels)
    let dist = abs(input.distance_to_center);
    
    // Smoothstep for antialiasing: fade out at the edges
    // The 0.75 range creates a smooth transition
    let alpha = smoothstep(actual_half_thickness + 0.75, actual_half_thickness - 0.75, dist);
    
    var color = input.color;
    color.a *= alpha;
    
    // Apply global opacity
    color.a *= uniforms.config.global_opacity;
    
    return color;
}
