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
    peak_mode: u32,           // 0=none, 1=fade, 2=fall, 3=fall_accel, 4=fall_fade (not used in lines mode)
    peak_hold_time: f32,      // Time in seconds for peak to hold (not used in lines mode)
    peak_fade_time: f32,      // Time in seconds for peak to fade (not used in lines mode)
    flash_count: u32,         // Number of bars (not used in lines mode)
    bar_depth_3d: f32,        // Isometric 3D depth in pixels (not used in lines mode)
    gradient_orientation: u32, // 0 = vertical, 1 = horizontal (not used in lines mode)
    average_energy: f32,       // Average bar amplitude (not used in lines mode)
    global_opacity: f32,       // Overall visualizer opacity (0.0-1.0)
    lines_outline_thickness: f32,  // Outline width in pixels (0.0 = disabled)
    lines_outline_opacity: f32,    // Outline alpha (0.0-1.0)
    lines_animation_speed: f32,    // Color cycling speed (0.05-1.0)
    lines_gradient_mode: u32,      // 0=breathing, 1=static, 2=position, 3=height, 4=gradient
    lines_fill_opacity: f32,       // Fill under curve (0.0 = disabled)
    lines_mirror: u32,             // 0=normal, 1=mirrored
    lines_glow_intensity: f32,     // Glow bloom (0.0 = disabled)
    lines_style: u32,              // 0=smooth, 1=angular, 2=stepped
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
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
    @location(3) normalized_x: f32,  // Horizontal position (0.0 = left, 1.0 = right)
    @location(4) amplitude: f32,  // Point amplitude (0.0 = silent, 1.0 = max)
    @location(5) is_fill: f32,  // 1.0 = fill pass, 0.0 = line/outline pass
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
    // Cycle speed from config (higher = faster cycling through gradient colors)
    let cycle_speed = uniforms.config.lines_animation_speed;
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

// Static gradient: always use first gradient color
fn get_gradient_color_static() -> vec4<f32> {
    return uniforms.gradient_colors[0];
}

// Position-based gradient: color by horizontal position along the line
// Left = first gradient color, right = last gradient color (bass → treble rainbow)
fn get_gradient_color_by_position(normalized_x: f32) -> vec4<f32> {
    let segments = 7.0;
    let pos = clamp(normalized_x, 0.0, 1.0) * segments;
    let idx = u32(floor(pos));
    let frac = pos - floor(pos);
    
    if (idx >= 7u) {
        return uniforms.gradient_colors[7];
    }
    
    let c1 = uniforms.gradient_colors[idx];
    let c2 = uniforms.gradient_colors[idx + 1u];
    
    return mix(c1, c2, frac);
}

// Height-based gradient: color by amplitude (quiet = bottom colors, loud = top colors)
fn get_gradient_color_by_height(amplitude: f32) -> vec4<f32> {
    let segments = 7.0;
    let pos = clamp(amplitude, 0.0, 1.0) * segments;
    let idx = u32(floor(pos));
    let frac = pos - floor(pos);
    
    if (idx >= 7u) {
        return uniforms.gradient_colors[7];
    }
    
    let c1 = uniforms.gradient_colors[idx];
    let c2 = uniforms.gradient_colors[idx + 1u];
    
    return mix(c1, c2, frac);
}

// Wave gradient: blends horizontal position and amplitude for a 2D color field.
// Peaks shift colors further along the palette, creating a music-reactive rainbow.
fn get_gradient_color_wave(normalized_x: f32, amplitude: f32) -> vec4<f32> {
    // Base color from position (0.0-0.5 of palette range)
    // Amplitude pushes further along the palette (0.0-0.5 extra)
    let blended = clamp(normalized_x * 0.5 + amplitude * 0.5, 0.0, 1.0);
    let segments = 7.0;
    let pos = blended * segments;
    let idx = u32(floor(pos));
    let frac = pos - floor(pos);
    
    if (idx >= 7u) {
        return uniforms.gradient_colors[7];
    }
    
    let c1 = uniforms.gradient_colors[idx];
    let c2 = uniforms.gradient_colors[idx + 1u];
    
    return mix(c1, c2, frac);
}

// Dispatch to the correct gradient function based on lines_gradient_mode
// mode: 0=breathing, 1=static, 2=position, 3=height, 4=gradient
fn get_lines_gradient_color(time: f32, normalized_x: f32, amplitude: f32) -> vec4<f32> {
    let mode = uniforms.config.lines_gradient_mode;
    if (mode == 1u) {
        return get_gradient_color_static();
    } else if (mode == 2u) {
        return get_gradient_color_by_position(normalized_x);
    } else if (mode == 3u) {
        return get_gradient_color_by_height(amplitude);
    } else if (mode == 4u) {
        return get_gradient_color_wave(normalized_x, amplitude);
    }
    // Default: breathing (mode 0)
    return get_gradient_color_animated(time);
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
    
    // Y position depends on mirror mode
    let line_thickness = max(uniforms.config.line_thickness, 2.0);
    let outline_extra = uniforms.config.lines_outline_thickness;
    let aa_padding = 1.5;
    let max_expansion = ((line_thickness * 0.5) + outline_extra + aa_padding) * 4.0;
    
    var y: f32;
    if (uniforms.config.lines_mirror == 1u) {
        // Mirror mode: line extends from center, value controls displacement
        // Center of canvas, with expansion margin
        let center_y = canvas_height * 0.5;
        let drawable_half = (canvas_height - max_expansion * 2.0) * 0.5;
        // Value of 0 = center, value > 0 = extends upward from center
        y = center_y - (clamped_value * drawable_half);
    } else {
        // Normal mode: value maps to height (0 = bottom, 1 = top)
        let drawable_height = canvas_height - max_expansion;
        y = canvas_height - (clamped_value * drawable_height);
    }
    
    return vec2<f32>(x, y);
}

/// Get the Y coordinate of the fill baseline (bottom for normal, center for mirror)
fn get_fill_baseline(canvas_height: f32) -> f32 {
    if (uniforms.config.lines_mirror == 1u) {
        return canvas_height * 0.5;
    }
    return canvas_height;
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
    
    // Instance layout:
    // 0=fill(top), 1=outline(top), 2=main(top)
    // 3=fill(mirror), 4=outline(mirror), 5=main(mirror)
    let is_mirror_pass = instance_index >= 3u;
    let base_instance = select(instance_index, instance_index - 3u, is_mirror_pass);
    let is_fill_pass = base_instance == 0u;
    let is_outline_pass = base_instance == 1u;
    // base_instance 2 = main line pass
    
    if (point_count < 2) {
        output.position = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
        output.color = vec4<f32>(0.0);
        output.distance_to_center = 0.0;
        output.is_outline = 0.0;
        output.normalized_x = 0.0;
        output.amplitude = 0.0;
        output.is_fill = 0.0;
        return output;
    }
    
    // If fill is disabled, discard fill pass vertices
    if (is_fill_pass && uniforms.config.lines_fill_opacity < 0.001) {
        output.position = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
        output.color = vec4<f32>(0.0);
        output.distance_to_center = 0.0;
        output.is_outline = 0.0;
        output.normalized_x = 0.0;
        output.amplitude = 0.0;
        output.is_fill = 0.0;
        return output;
    }
    
    // Spline interpolation: 16 samples per segment for smoother curves
    let samples_per_segment = 16;
    let num_segments = point_count - 1;
    let total_spline_points = num_segments * samples_per_segment + 1;
    
    // Each spline point needs 2 vertices (left/right for line, curve/bottom for fill)
    let vertices_per_spline = 2;
    let vertices_per_pass = u32(total_spline_points) * u32(vertices_per_spline);
    
    if (vertex_index >= vertices_per_pass) {
        output.position = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
        output.color = vec4<f32>(0.0);
        output.distance_to_center = 0.0;
        output.is_outline = 0.0;
        output.normalized_x = 0.0;
        output.amplitude = 0.0;
        output.is_fill = 0.0;
        return output;
    }
    
    // Which spline point is this vertex for?
    let spline_point_index = vertex_index / 2u;
    let is_even_vertex = (vertex_index % 2u) == 0u;
    
    // Calculate which segment and t value within segment
    let segment_index = i32(spline_point_index) / samples_per_segment;
    let sample_in_segment = i32(spline_point_index) % samples_per_segment;
    let t = f32(sample_in_segment) / f32(samples_per_segment);
    
    // Handle the final point
    var current_point: vec2<f32>;
    var next_point: vec2<f32>;
    var prev_point: vec2<f32>;
    
    if (i32(spline_point_index) >= total_spline_points - 1) {
        current_point = get_point(point_count - 1, point_count, canvas_width, canvas_height);
        prev_point = get_point(point_count - 2, point_count, canvas_width, canvas_height);
        next_point = current_point;
    } else if (segment_index >= num_segments) {
        current_point = get_point(point_count - 1, point_count, canvas_width, canvas_height);
        prev_point = get_point(point_count - 2, point_count, canvas_width, canvas_height);
        next_point = current_point;
    } else {
        let p0 = get_point(segment_index - 1, point_count, canvas_width, canvas_height);
        let p1 = get_point(segment_index, point_count, canvas_width, canvas_height);
        let p2 = get_point(segment_index + 1, point_count, canvas_width, canvas_height);
        let p3 = get_point(segment_index + 2, point_count, canvas_width, canvas_height);
        
        let line_style = uniforms.config.lines_style;
        if (line_style == 1u) {
            // Angular: straight line segments between data points
            current_point = mix(p1, p2, t);
            let t_prev_a = max(t - 0.01, 0.0);
            let t_next_a = min(t + 0.01, 1.0);
            prev_point = mix(p1, p2, t_prev_a);
            next_point = mix(p1, p2, t_next_a);
        } else {
            // Smooth (default): Catmull-Rom spline
            current_point = catmull_rom(p0, p1, p2, p3, t);
            let t_prev_s = max(t - 0.01, 0.0);
            let t_next_s = min(t + 0.01, 1.0);
            prev_point = catmull_rom(p0, p1, p2, p3, t_prev_s);
            next_point = catmull_rom(p0, p1, p2, p3, t_next_s);
        }
    }
    
    // Clamp Y coordinates after spline interpolation
    let outline_extra = uniforms.config.lines_outline_thickness;
    let aa_padding = 1.5;
    let max_expansion = half_thickness + outline_extra + aa_padding;
    if (uniforms.config.lines_mirror == 1u) {
        let center_y = canvas_height * 0.5;
        current_point.y = clamp(current_point.y, max_expansion, center_y);
        prev_point.y = clamp(prev_point.y, max_expansion, center_y);
        next_point.y = clamp(next_point.y, max_expansion, center_y);
    } else {
        current_point.y = clamp(current_point.y, max_expansion, canvas_height);
        prev_point.y = clamp(prev_point.y, max_expansion, canvas_height);
        next_point.y = clamp(next_point.y, max_expansion, canvas_height);
    }
    
    // Mirror pass: flip Y coordinates around center line
    if (is_mirror_pass) {
        let center_y = canvas_height * 0.5;
        current_point.y = 2.0 * center_y - current_point.y;
        prev_point.y = 2.0 * center_y - prev_point.y;
        next_point.y = 2.0 * center_y - next_point.y;
    }
    
    // Calculate normalized x position (0.0 = left, 1.0 = right)
    let normalized_x = clamp(current_point.x / canvas_width, 0.0, 1.0);
    
    // Calculate amplitude from the current point's Y position
    let outline_extra_val = uniforms.config.lines_outline_thickness;
    let aa_pad_val = 1.5;
    let max_exp_val = ((line_thickness * 0.5) + outline_extra_val + aa_pad_val) * 4.0;
    let draw_h = canvas_height - max_exp_val;
    var amplitude: f32;
    if (uniforms.config.lines_mirror == 1u) {
        let center_y = canvas_height * 0.5;
        // For mirror pass, amplitude is distance from center (works for both halves)
        amplitude = clamp(abs(center_y - current_point.y) / (center_y - max_exp_val * 0.5), 0.0, 1.0);
    } else {
        amplitude = clamp((canvas_height - current_point.y) / draw_h, 0.0, 1.0);
    }
    
    // Get gradient color for this vertex
    let gradient_color = get_lines_gradient_color(uniforms.config.time, normalized_x, amplitude);
    
    // === FILL PASS: triangle strip from curve to baseline ===
    if (is_fill_pass) {
        let fill_baseline = get_fill_baseline(canvas_height);
        var fill_point: vec2<f32>;
        
        if (is_even_vertex) {
            // Even vertex = on the curve
            fill_point = current_point;
        } else {
            // Odd vertex = at the baseline (bottom or center)
            fill_point = vec2<f32>(current_point.x, fill_baseline);
        }
        
        let ndc = pixel_to_ndc(fill_point.x, fill_point.y);
        
        var fill_color = gradient_color;
        fill_color.a *= uniforms.config.lines_fill_opacity;
        
        output.position = vec4<f32>(ndc.x, ndc.y, 0.0, 1.0);
        output.color = fill_color;
        output.distance_to_center = 0.0;
        output.is_outline = 0.0;
        output.normalized_x = normalized_x;
        output.amplitude = amplitude;
        output.is_fill = 1.0;
        return output;
    }
    
    // === LINE / OUTLINE PASS ===
    // Calculate perpendicular direction for line thickness
    var tangent = next_point - prev_point;
    let tangent_len = length(tangent);
    if (tangent_len > 0.001) {
        tangent = tangent / tangent_len;
    } else {
        tangent = vec2<f32>(1.0, 0.0);
    }
    
    let perp = vec2<f32>(-tangent.y, tangent.x);
    
    // Determine thickness based on outline vs main line
    let actual_half_thickness = select(half_thickness, half_thickness + outline_extra, is_outline_pass);
    
    let extended_half_thickness = actual_half_thickness + aa_padding;
    
    // Offset vertex to left or right of center line
    var offset_point: vec2<f32>;
    var dist_to_center: f32;
    if (is_even_vertex) {
        offset_point = current_point + perp * extended_half_thickness;
        dist_to_center = extended_half_thickness;
    } else {
        offset_point = current_point - perp * extended_half_thickness;
        dist_to_center = -extended_half_thickness;
    }
    
    // Clamp offset point to stay within canvas bounds
    offset_point.y = clamp(offset_point.y, 0.0, canvas_height);
    
    // Calculate color
    var color: vec4<f32>;
    if (is_outline_pass) {
        var outline_color = uniforms.border_color;
        outline_color.a *= uniforms.config.lines_outline_opacity;
        color = outline_color;
    } else {
        color = gradient_color;
    }
    
    // Convert to NDC
    let ndc = pixel_to_ndc(offset_point.x, offset_point.y);
    
    output.position = vec4<f32>(ndc.x, ndc.y, 0.0, 1.0);
    output.color = color;
    output.distance_to_center = dist_to_center;
    output.is_outline = select(0.0, 1.0, is_outline_pass);
    output.normalized_x = normalized_x;
    output.amplitude = amplitude;
    output.is_fill = 0.0;
    
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    var color = input.color;
    
    // Fill pass: no antialiasing needed, just use the fill color directly
    if (input.is_fill > 0.5) {
        color.a *= uniforms.config.global_opacity;
        return color;
    }
    
    // Line/outline pass: antialiased rendering
    let line_thickness = max(uniforms.config.line_thickness, 2.0);
    let half_thickness = line_thickness * 0.5;
    
    let outline_extra = uniforms.config.lines_outline_thickness;
    let actual_half_thickness = select(half_thickness, half_thickness + outline_extra, input.is_outline > 0.5);
    
    let dist = abs(input.distance_to_center);
    let alpha = smoothstep(actual_half_thickness + 0.75, actual_half_thickness - 0.75, dist);
    
    color.a *= alpha;
    color.a *= uniforms.config.global_opacity;
    
    return color;
}
