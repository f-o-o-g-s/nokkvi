// Tunnel (Scope): about the last second of the spectrum (tunnel.rs ROWS x
// ROW_EVERY ticks) spirals down into the cover behind the ring. Each past
// spectrum is a ring (bass at the bottom, treble at the top, mirrored) one
// step further down, smaller in perspective and turned a little further, so
// what the music does twists away into a vortex. The wall between two rings is
// a dark tube (it reads over a bright cover), lit by the ring behind it so the
// tube reads ribbed, and it thins with depth so the cover shows at the far
// end. Ring edges are lit in the bar colours; a ring kept on a beat onset
// glows hot as it falls away. The live ring itself stays the raw waveform.
//
// Front to back: a pixel shows the wall in front of the first ring (from the
// mouth) it lies outside of, so a far ring swinging wider than a nearer one
// stays hidden behind the nearer wall. The newest ring fades in over its slot
// so no ring pops in at the mouth.
//
// Drawn behind the ring, as one fullscreen triangle per frame. The rings come
// from tunnel.rs through the peak buffer (binding 2), which Scope never reads
// otherwise: [rows, samples, phase, _], ROWS_MAX kick strengths, the rings.
//
// ⚠️  Config struct layout MUST match VisualizerConfig in shader.rs byte-for-byte
//     (same block as bars.wgsl / lines.wgsl / scope.wgsl / horizon.wgsl).

struct Uniforms {
    viewport: vec4<f32>,
    gradient_colors: array<vec4<f32>, 8>,
    peak_gradient_colors: array<vec4<f32>, 8>,
    peak_color: vec4<f32>,
    border_color: vec4<f32>,
    config: Config,
    audio: vec4<f32>,  // [beat * reactivity, bass, mid, treble]
}

struct Config {
    bar_count: u32,
    mode: u32,
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
    scope_radius: f32,
    scope_sensitivity: f32,
    flash_data: array<vec4<f32>, 512>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(2) var<storage, read> tunnel: array<f32>;  // tunnel.rs snapshot

const TAU: f32 = 6.28318530717958647692;
const ROWS_MAX: u32 = 20u;  // tunnel.rs ROWS (the kick slots in the snapshot)
const HEADER: u32 = 4u;     // tunnel.rs HEADER

// ── ring geometry (copied verbatim from scope.wgsl; a test pins them) ───

const SCOPE_RADIUS_MIN: f32 = 0.1;
const SCOPE_RADIUS_MAX: f32 = 0.95;
const LINES_GLOW_MIN_RADIUS: f32 = 3.0;
const LINES_GLOW_MAX_RADIUS: f32 = 10.0;
const LINES_GLOW_EXTENT_MULT: f32 = 2.5;

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

// The bar gradient, bottom colour (0) to top colour (1), 6 stops.
fn ramp(t: f32) -> vec3<f32> {
    let s = clamp(t, 0.0, 1.0) * 5.0;
    let i = u32(floor(s));
    if (i >= 5u) {
        return uniforms.gradient_colors[5].rgb;
    }
    return mix(uniforms.gradient_colors[i].rgb, uniforms.gradient_colors[i + 1u].rgb, s - floor(s));
}

fn ss(a: f32, b: f32, x: f32) -> f32 {
    let t = clamp((x - a) / (b - a), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn catmull_rom_1d(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    return 0.5 * ((2.0 * p1) + (-p0 + p2) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3);
}

struct VOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// One triangle covering the viewport: uv (0,0) top-left .. (2,2).
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VOut;
    o.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    return o;
}

// ── the tunnel ──────────────────────────────────────────────────────────

const TUNNEL_DEPTH: f32 = 0.16;     // perspective per ring (scale 1 / (1 + z * depth))
const TWIST: f32 = 0.035;           // turns per ring of depth (a ring spins as it falls)
const WALL_ALPHA: f32 = 0.9;        // the nearest wall's opacity
const WALL_FADE: f32 = 1.3;         // how fast the wall thins with depth
const LINE_PX: f32 = 1.4;           // a ring edge's half width at the mouth (px)
const SPILL_FALLOFF: f32 = 6.0;     // how fast a ring's light dies along the wall in front
const SPILL_GAIN: f32 = 0.26;       // a ring's light on the wall in front of it
const SPILL_KICK: f32 = 0.9;        // extra light from a kick ring

fn ring_scale(z: f32) -> f32 {
    return 1.0 / (1.0 + z * TUNNEL_DEPTH);
}

fn ring_raw(r: u32, samples: u32, i: i32) -> f32 {
    let n = i32(samples);
    let w = u32(((i % n) + n) % n);
    return clamp(tunnel[HEADER + ROWS_MAX + r * samples + w], -1.0, 1.0);
}

// Ring `r`'s value at angle fraction `s0` (0..1 of a turn), turned by its
// depth `z`. Smooth (Catmull-Rom) or Angular (straight), like the ring.
fn ring_value(r: u32, samples: u32, s0: f32, z: f32) -> f32 {
    let s = fract(s0 + z * TWIST);
    let f = s * f32(samples);
    let i = i32(floor(f));
    let t = f - f32(i);
    let b = ring_raw(r, samples, i);
    let c = ring_raw(r, samples, i + 1);
    if (uniforms.config.lines_style == 1u) {
        return mix(b, c, t);
    }
    return catmull_rom_1d(ring_raw(r, samples, i - 1), b, c, ring_raw(r, samples, i + 2), t);
}

fn kick(r: u32) -> f32 {
    return tunnel[HEADER + r];
}

// The tunnel seen from ring `first` back (premultiplied).
fn tunnel_from(first: u32, rows: u32, samples: u32, phase: f32, r: f32, s: f32, base_r: f32, dev_r: f32) -> vec4<f32> {
    let zend = f32(rows) + 1.0;
    let z0 = f32(first) + phase;
    var near_v = ring_value(first, samples, s, z0);
    var near_r = ring_scale(z0) * (base_r + near_v * dev_r);
    if (r > near_r) {
        return vec4<f32>(0.0);
    }
    var near_z = z0;
    var near_kick = kick(first);

    var hit = false;
    var far_r = 0.0;
    var far_z = zend;
    var far_v = 0.0;
    var far_kick = 0.0;
    for (var k = first + 1u; k < rows; k++) {
        let z = f32(k) + phase;
        let v = ring_value(k, samples, s, z);
        let rr = ring_scale(z) * (base_r + v * dev_r);
        if (r >= rr) {
            hit = true;
            far_r = rr;
            far_z = z;
            far_v = v;
            far_kick = kick(k);
            break;
        }
        // A far ring swinging wider than the nearer wall stays hidden behind it.
        if (rr < near_r) {
            near_r = rr;
            near_v = v;
            near_kick = kick(k);
        }
        near_z = z;
    }

    // Where along this wall the pixel sits: 0 at the near ring, 1 at the far.
    let u = clamp((near_r - r) / max(near_r - far_r, 1e-3), 0.0, 1.0);
    let fog = clamp(mix(near_z, far_z, u) / zend, 0.0, 1.0);
    let fog_n = clamp(near_z / zend, 0.0, 1.0);
    let fog_f = clamp(far_z / zend, 0.0, 1.0);

    // Ring edges at both sides of this wall, thinner with depth, lit in the
    // bar colours (louder = higher up the ramp); a kick ring glows hot.
    let hot = uniforms.peak_gradient_colors[0].rgb;
    let col_n = mix(ramp(0.45 + abs(near_v) * 0.8), hot, near_kick);
    let col_f = mix(ramp(0.45 + abs(far_v) * 0.8), hot, far_kick);
    let dn = near_r - r;
    let df = select(1e4, r - far_r, hit);
    let wn = LINE_PX * max(ring_scale(near_z), 0.3);
    let wf = LINE_PX * max(ring_scale(far_z), 0.3);
    let edge_n = clamp(exp(-dn * dn / (wn * wn)) * (1.0 + near_kick * 1.5), 0.0, 1.0) * (1.0 - 0.75 * fog_n);
    let edge_f = clamp(exp(-df * df / (wf * wf)) * (1.0 + far_kick * 1.5), 0.0, 1.0) * (1.0 - 0.75 * fog_f);
    let line_a = clamp(edge_n + edge_f, 0.0, 1.0);
    let line_c = (col_n * edge_n + col_f * edge_f) / max(edge_n + edge_f, 1e-4);

    // The wall: a dark tube, each segment lit by the ring behind it (its light
    // spills forward onto the wall and dies toward the next ring), thinning
    // with depth so the cover shows at the far end.
    let deep = pow(max(uniforms.border_color.rgb, vec3<f32>(0.0)), vec3<f32>(2.2));
    let spill = exp(-(1.0 - u) * SPILL_FALLOFF) * select(0.0, 1.0, hit);
    let body = deep * (1.0 - 0.35 * u)
        + col_f * spill * (SPILL_GAIN + far_kick * SPILL_KICK) * (1.0 - 0.6 * fog_f);
    let wall_a = WALL_ALPHA * pow(1.0 - fog, WALL_FADE);

    let a = clamp(wall_a + line_a * (1.0 - wall_a), 0.0, 1.0);
    let col = body * wall_a * (1.0 - line_a) + line_c * line_a * 1.3;
    return vec4<f32>(col, a);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let rows = min(u32(tunnel[0]), ROWS_MAX);
    let samples = u32(tunnel[1]);
    let phase = tunnel[2];
    if (rows < 3u || samples < 2u) {
        return vec4<f32>(0.0);
    }
    // The ring's centre and (aspect-stretched) circle, as scope.wgsl draws it.
    let vp = uniforms.viewport;
    let aspect = vp.zw / min(vp.z, vp.w);
    let q = (in.uv * vp.zw - vp.zw * 0.5) / aspect;
    let r = length(q);
    var ang = atan2(q.y, q.x);
    if (ang < 0.0) {
        ang = ang + TAU;
    }
    let s = ang / TAU;

    let avail = ring_available_radius();
    let rf = clamp(uniforms.config.scope_radius, SCOPE_RADIUS_MIN, SCOPE_RADIUS_MAX);
    let base_r = avail * rf;
    let dev_r = avail * (1.0 - rf);

    // The newest ring fades in over its slot, so no ring pops in at the
    // mouth: the tunnel from ring 1 back, blended toward the tunnel from
    // ring 0 back as ring 0 settles in.
    let settled = tunnel_from(1u, rows, samples, phase, r, s, base_r, dev_r);
    let fresh = tunnel_from(0u, rows, samples, phase, r, s, base_r, dev_r);
    return mix(settled, fresh, ss(0.0, 1.0, phase)) * uniforms.config.global_opacity;
}
