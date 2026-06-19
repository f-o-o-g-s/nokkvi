// Visualizer Scope Shader (circular oscilloscope)
// Maps the time-domain waveform around a ring centered in the widget bounds.
// Reuses the Lines mode's color/glow/AA machinery; only the geometry differs
// (polar instead of linear, closed loop instead of an open polyline).
//
// `bar_data` here holds the SIGNED, normalized waveform (~-1..1), NOT the FFT
// magnitudes — the renderer uploads `get_waveform()` for this mode.
//
// ⚠️  Config struct layout MUST match VisualizerConfig in shader.rs byte-for-byte.
//     If you add/remove/reorder fields, update ALL locations:
//       1. src/widgets/visualizer/shader.rs          (VisualizerConfig)
//       2. src/widgets/visualizer/shaders/bars.wgsl  (Config)
//       3. src/widgets/visualizer/shaders/lines.wgsl (Config)
//       4. src/widgets/visualizer/shaders/scope.wgsl (Config)

struct Uniforms {
    viewport: vec4<f32>,  // x, y, width, height in PIXELS
    gradient_colors: array<vec4<f32>, 8>,
    peak_gradient_colors: array<vec4<f32>, 8>,
    peak_color: vec4<f32>,
    border_color: vec4<f32>,
    config: Config,
    audio: vec4<f32>,  // [beat * reactivity, bass, mid, treble]
}

struct Config {
    bar_count: u32,
    mode: u32,  // 0 = bars, 1 = lines, 2 = scope
    border_width: f32,
    peak_enabled: u32,
    peak_thickness: f32,
    peak_alpha: f32,
    line_thickness: f32,
    bar_width: f32,
    bar_spacing: f32,
    edge_spacing: f32,
    time: f32,
    led_bars: u32,
    led_segment_height: f32,
    led_border_opacity: f32,
    border_opacity: f32,
    gradient_mode: u32,
    peak_gradient_mode: u32,
    peak_mode: u32,
    peak_hold_time: f32,
    peak_fade_time: f32,
    flash_count: u32,
    bar_depth_3d: f32,
    gradient_orientation: u32,
    average_energy: f32,
    global_opacity: f32,
    lines_outline_thickness: f32,
    lines_outline_opacity: f32,
    lines_animation_speed: f32,
    lines_gradient_mode: u32,
    lines_fill_opacity: f32,
    lines_mirror: u32,
    lines_glow_intensity: f32,
    lines_style: u32,
    bars_flash_intensity: f32,
    scope_radius: f32,             // Scope mode: mean ring radius fraction
    scope_sensitivity: f32,        // Scope mode: waveform swing / gain
    flash_data: array<vec4<f32>, 512>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var<storage, read> bar_data: array<f32>;  // Signed waveform (-1..1)
@group(0) @binding(2) var<storage, read> peak_data: array<f32>;  // Unused in scope mode
@group(0) @binding(3) var<storage, read> peak_alpha_data: array<f32>;  // Unused in scope mode

const TAU: f32 = 6.28318530717958647692;

// Smooth spline samples per segment (one segment per waveform point). MUST match
// SCOPE_SAMPLES_PER_SEGMENT in shader.rs (the CPU draw-call vertex count).
const SCOPE_SP: i32 = 12;

// Ring sizing is user-controlled via the uniform:
//   config.scope_radius      mean ring radius as a fraction of the available
//                            space (after reserving stroke + glow margin);
//   config.scope_sensitivity waveform gain (how hard loud audio swings the ring).
// The deviation uses the remaining headroom `(1 - radius)` so a full-scale swing
// reaches the margin but never clips past the panel.
const SCOPE_RADIUS_MIN: f32 = 0.1;
const SCOPE_RADIUS_MAX: f32 = 0.95;

// ---------- Palette segment-count constants (mirrors lines.wgsl) ----------
const LINES_PALETTE_SEGMENTS_STATIC: f32 = 7.0;
const LINES_PALETTE_SEGMENTS_LOOPED: f32 = 8.0;
const LINES_PALETTE_INDEX_TAIL: u32 = 7u;
const LINES_PALETTE_INDEX_MOD: u32 = 8u;

// ---------- Neon glow constants/helpers (mirrors lines.wgsl) ----------
const LINES_GLOW_MIN_RADIUS: f32 = 3.0;
const LINES_GLOW_MAX_RADIUS: f32 = 10.0;
const LINES_GLOW_EXTENT_MULT: f32 = 2.5;
const LINES_BEAT_GLOW: f32 = 0.7;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) distance_to_center: f32,
    @location(2) is_outline: f32,
    @location(3) normalized_x: f32,  // Position around the ring (0..1)
    @location(4) amplitude: f32,     // |deviation| from the silence radius (0..1)
    @location(5) is_fill: f32,       // Always 0 in scope mode (no fill pass)
}

fn dead_output() -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
    output.color = vec4<f32>(0.0);
    output.distance_to_center = 0.0;
    output.is_outline = 0.0;
    output.normalized_x = 0.0;
    output.amplitude = 0.0;
    output.is_fill = 0.0;
    return output;
}

fn pixel_to_ndc(pixel_x: f32, pixel_y: f32) -> vec2<f32> {
    let viewport = uniforms.viewport;
    let ndc_x = (pixel_x / viewport.z) * 2.0 - 1.0;
    let ndc_y = 1.0 - (pixel_y / viewport.w) * 2.0;
    return vec2<f32>(ndc_x, ndc_y);
}

fn lines_glow_radius() -> f32 {
    let s = clamp(uniforms.config.lines_glow_intensity, 0.0, 1.0);
    return mix(LINES_GLOW_MIN_RADIUS, LINES_GLOW_MAX_RADIUS, s);
}

fn lines_glow_extent() -> f32 {
    if (uniforms.config.lines_glow_intensity <= 0.001) {
        return 0.0;
    }
    return lines_glow_radius() * LINES_GLOW_EXTENT_MULT;
}

// ---------- Gradient functions (verbatim from lines.wgsl) ----------
fn get_gradient_color_animated(time: f32) -> vec4<f32> {
    let cycle_speed = uniforms.config.lines_animation_speed;
    let t = fract(time * cycle_speed);
    let segments = LINES_PALETTE_SEGMENTS_LOOPED;
    let pos = t * segments;
    let idx = u32(floor(pos)) % LINES_PALETTE_INDEX_MOD;
    let next_idx = (idx + 1u) % LINES_PALETTE_INDEX_MOD;
    let frac = pos - floor(pos);
    let c1 = uniforms.gradient_colors[idx];
    let c2 = uniforms.gradient_colors[next_idx];
    return mix(c1, c2, frac);
}

fn get_gradient_color_static() -> vec4<f32> {
    return uniforms.gradient_colors[0];
}

fn get_gradient_color_by_position(normalized_x: f32) -> vec4<f32> {
    let segments = LINES_PALETTE_SEGMENTS_STATIC;
    let pos = clamp(normalized_x, 0.0, 1.0) * segments;
    let idx = u32(floor(pos));
    let frac = pos - floor(pos);
    if (idx >= LINES_PALETTE_INDEX_TAIL) {
        return uniforms.gradient_colors[LINES_PALETTE_INDEX_TAIL];
    }
    let c1 = uniforms.gradient_colors[idx];
    let c2 = uniforms.gradient_colors[idx + 1u];
    return mix(c1, c2, frac);
}

fn get_gradient_color_by_height(amplitude: f32) -> vec4<f32> {
    let segments = LINES_PALETTE_SEGMENTS_STATIC;
    let pos = clamp(amplitude, 0.0, 1.0) * segments;
    let idx = u32(floor(pos));
    let frac = pos - floor(pos);
    if (idx >= LINES_PALETTE_INDEX_TAIL) {
        return uniforms.gradient_colors[LINES_PALETTE_INDEX_TAIL];
    }
    let c1 = uniforms.gradient_colors[idx];
    let c2 = uniforms.gradient_colors[idx + 1u];
    return mix(c1, c2, frac);
}

fn get_gradient_color_wave(normalized_x: f32, amplitude: f32) -> vec4<f32> {
    let blended = clamp(normalized_x * 0.5 + amplitude * 0.5, 0.0, 1.0);
    let segments = LINES_PALETTE_SEGMENTS_STATIC;
    let pos = blended * segments;
    let idx = u32(floor(pos));
    let frac = pos - floor(pos);
    if (idx >= LINES_PALETTE_INDEX_TAIL) {
        return uniforms.gradient_colors[LINES_PALETTE_INDEX_TAIL];
    }
    let c1 = uniforms.gradient_colors[idx];
    let c2 = uniforms.gradient_colors[idx + 1u];
    return mix(c1, c2, frac);
}

// mode: 0=breathing, 1=static, 2=position, 3=height, 4=gradient(wave)
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
    return get_gradient_color_animated(time);
}

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

// ---------- Ring geometry ----------
// Center of the widget in pixels.
fn ring_center() -> vec2<f32> {
    return vec2<f32>(uniforms.viewport.z * 0.5, uniforms.viewport.w * 0.5);
}

// Radius (px) the ring is allowed to reach after reserving stroke + glow margin.
fn ring_available_radius() -> f32 {
    let min_dim = min(uniforms.viewport.z, uniforms.viewport.w);
    let line_thickness = max(uniforms.config.line_thickness, 2.0);
    let margin = line_thickness * 0.5
        + uniforms.config.lines_outline_thickness
        + lines_glow_extent()
        + 2.0;
    let avail = min_dim * 0.5 - margin;
    // Never collapse on a tiny panel (or when glow margin exceeds half the panel).
    return max(avail, min_dim * 0.15);
}

// Cartesian position of control point `i` on the ring. The ANGLE advances with
// the unwrapped index `i` (so the curve sweeps monotonically and the seam joins
// cleanly), while the waveform VALUE is looked up modulo `point_count` (closed
// loop). Returns the position plus the signed deviation (for amplitude/color).
fn ring_point(i: i32, point_count: i32, center: vec2<f32>, base_r: f32, dev_r: f32) -> vec3<f32> {
    let n = point_count;
    let wrapped = ((i % n) + n) % n;
    let raw = clamp(bar_data[u32(wrapped)], -1.0, 1.0);
    let value = clamp(raw * uniforms.config.scope_sensitivity, -1.0, 1.0);

    let angle = (f32(i) / f32(n)) * TAU;
    let radius = base_r + value * dev_r;
    let pos = center + radius * vec2<f32>(cos(angle), sin(angle));
    return vec3<f32>(pos.x, pos.y, value);
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32
) -> VertexOutput {
    var output: VertexOutput;

    let point_count = i32(uniforms.config.bar_count);
    if (point_count < 2) {
        return dead_output();
    }

    // Instance 0 = fill, 1 = outline (under), 2 = main line (on top).
    let is_fill_pass = instance_index == 0u;
    let is_outline_pass = instance_index == 1u;

    // Skip the fill pass entirely when fill is disabled.
    if (is_fill_pass && uniforms.config.lines_fill_opacity < 0.001) {
        return dead_output();
    }

    let line_thickness = max(uniforms.config.line_thickness, 2.0);
    let half_thickness = line_thickness * 0.5;
    let outline_extra = uniforms.config.lines_outline_thickness;
    let aa_padding = 1.5;

    // Closed loop: one segment per point, +1 closing sample to seal the strip.
    let total_samples = point_count * SCOPE_SP;
    let ring_points = total_samples + 1;
    let vertices_per_pass = u32(ring_points) * 2u;
    if (vertex_index >= vertices_per_pass) {
        return dead_output();
    }

    let ring_idx = i32(vertex_index / 2u);
    let is_even_vertex = (vertex_index % 2u) == 0u;

    let segment_index = ring_idx / SCOPE_SP;
    let sample_in_segment = ring_idx % SCOPE_SP;
    let t = f32(sample_in_segment) / f32(SCOPE_SP);

    let center = ring_center();
    let avail = ring_available_radius();
    let radius_frac = clamp(uniforms.config.scope_radius, SCOPE_RADIUS_MIN, SCOPE_RADIUS_MAX);
    let base_r = avail * radius_frac;
    let dev_r = avail * (1.0 - radius_frac);

    // Control points around the ring (indices wrap for the closed loop).
    let p0v = ring_point(segment_index - 1, point_count, center, base_r, dev_r);
    let p1v = ring_point(segment_index, point_count, center, base_r, dev_r);
    let p2v = ring_point(segment_index + 1, point_count, center, base_r, dev_r);
    let p3v = ring_point(segment_index + 2, point_count, center, base_r, dev_r);
    let p0 = p0v.xy;
    let p1 = p1v.xy;
    let p2 = p2v.xy;
    let p3 = p3v.xy;

    var current_point: vec2<f32>;
    var prev_point: vec2<f32>;
    var next_point: vec2<f32>;

    let line_style = uniforms.config.lines_style;
    if (line_style == 1u) {
        // Angular: straight segments between points.
        current_point = mix(p1, p2, t);
        let t_prev_a = max(t - 0.01, 0.0);
        let t_next_a = min(t + 0.01, 1.0);
        prev_point = mix(p1, p2, t_prev_a);
        next_point = mix(p1, p2, t_next_a);
    } else {
        // Smooth (default): Catmull-Rom spline.
        current_point = catmull_rom(p0, p1, p2, p3, t);
        let t_prev_s = max(t - 0.01, 0.0);
        let t_next_s = min(t + 0.01, 1.0);
        prev_point = catmull_rom(p0, p1, p2, p3, t_prev_s);
        next_point = catmull_rom(p0, p1, p2, p3, t_next_s);
    }

    // Position around the ring (0..1) for the position/wave gradient sweep.
    let normalized_x = clamp(f32(ring_idx) / f32(total_samples), 0.0, 1.0);

    // Amplitude = |deviation| from the silence radius, recovered from the
    // current radius so the height/wave gradients track loudness.
    let cur_radius = length(current_point - center);
    let amplitude = clamp(abs(cur_radius - base_r) / max(dev_r, 0.0001), 0.0, 1.0);

    let gradient_color = get_lines_gradient_color(uniforms.config.time, normalized_x, amplitude);

    // Stretch the ring to fill a non-square panel: map the min(w, h) circle out
    // to the full width/height so it follows the cover's aspect ratio instead of
    // sitting in a centered square. Square panels have scale == (1, 1) and are
    // byte-identical to before. Amplitude/color above were recovered from the
    // isotropic radius, so only the geometry from here on is stretched; thickness
    // is applied afterward in this stretched space, so the stroke stays a uniform
    // pixel width all the way around the ellipse.
    let aspect_min_dim = min(uniforms.viewport.z, uniforms.viewport.w);
    let aspect_scale = vec2<f32>(
        uniforms.viewport.z / aspect_min_dim,
        uniforms.viewport.w / aspect_min_dim,
    );
    current_point = center + (current_point - center) * aspect_scale;
    prev_point = center + (prev_point - center) * aspect_scale;
    next_point = center + (next_point - center) * aspect_scale;

    // Fill pass: triangle strip from the ring (rim) to the center, with a radial
    // alpha fade — colored at the rim, transparent at the center — so the ring
    // reads as a filled gradient "wave" (the circular analog of Lines' fill).
    if (is_fill_pass) {
        var fill_point: vec2<f32>;
        var fill_color = gradient_color;
        if (is_even_vertex) {
            fill_point = current_point;
            fill_color.a *= uniforms.config.lines_fill_opacity;
        } else {
            fill_point = center;
            fill_color.a = 0.0;
        }
        let ndc_fill = pixel_to_ndc(fill_point.x, fill_point.y);
        output.position = vec4<f32>(ndc_fill.x, ndc_fill.y, 0.0, 1.0);
        output.color = fill_color;
        output.distance_to_center = 0.0;
        output.is_outline = 0.0;
        output.normalized_x = normalized_x;
        output.amplitude = amplitude;
        output.is_fill = 1.0;
        return output;
    }

    // Perpendicular thickening (same ribbon math as lines.wgsl).
    var tangent = next_point - prev_point;
    let tangent_len = length(tangent);
    if (tangent_len > 0.001) {
        tangent = tangent / tangent_len;
    } else {
        tangent = vec2<f32>(1.0, 0.0);
    }
    let perp = vec2<f32>(-tangent.y, tangent.x);

    let actual_half_thickness = select(half_thickness, half_thickness + outline_extra, is_outline_pass);
    // Widen only the main pass for the glow halo; the outline stays tight.
    let pass_glow_extent = select(lines_glow_extent(), 0.0, is_outline_pass);
    let extended_half_thickness = actual_half_thickness + aa_padding + pass_glow_extent;

    var offset_point: vec2<f32>;
    var dist_to_center: f32;
    if (is_even_vertex) {
        offset_point = current_point + perp * extended_half_thickness;
        dist_to_center = extended_half_thickness;
    } else {
        offset_point = current_point - perp * extended_half_thickness;
        dist_to_center = -extended_half_thickness;
    }

    var color: vec4<f32>;
    if (is_outline_pass) {
        var outline_color = uniforms.border_color;
        outline_color.a *= uniforms.config.lines_outline_opacity;
        color = outline_color;
    } else {
        color = gradient_color;
    }

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

    // Fill pass: flat (radial-faded) color, no AA halo.
    if (input.is_fill > 0.5) {
        color.a *= uniforms.config.global_opacity;
        return color;
    }

    let line_thickness = max(uniforms.config.line_thickness, 2.0);
    let half_thickness = line_thickness * 0.5;

    let outline_extra = uniforms.config.lines_outline_thickness;
    let is_outline = input.is_outline > 0.5;
    let actual_half_thickness = select(half_thickness, half_thickness + outline_extra, is_outline);

    let dist = abs(input.distance_to_center);
    let core = smoothstep(actual_half_thickness + 0.75, actual_half_thickness - 0.75, dist);

    var coverage = core;

    // Neon glow halo (main pass only), flaring with loudness + beat — identical
    // to lines.wgsl, expressed in distance-from-centerline so it ports directly.
    let glow_strength = uniforms.config.lines_glow_intensity;
    if (glow_strength > 0.001 && !is_outline) {
        let beyond = max(dist - actual_half_thickness, 0.0);
        let halo = exp(-beyond / lines_glow_radius());
        let energy = clamp(uniforms.config.average_energy, 0.0, 1.0);
        let beat_flare = 1.0 + uniforms.audio.x * LINES_BEAT_GLOW;
        let halo_coverage = halo * glow_strength * (0.45 + 0.55 * energy) * beat_flare;
        coverage = max(core, halo_coverage);
    }

    color.a *= coverage;
    color.a *= uniforms.config.global_opacity;

    return color;
}
