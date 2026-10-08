// Horizon (Bars / Lines): about the last second and a half of the spectrum
// (horizon.rs ROWS x ROW_EVERY ticks) recedes behind the live visualizer
// toward a horizon. Each past frame is a row
// further back: its baseline climbs toward the horizon, it shrinks in height
// and width with distance (the near rows run off-screen, the far ones show
// their ends), its body is dark (so it reads over a bright cover) and lifts
// into mist with distance, and its crest is lit in the bar colours. Rows are
// composited front to back, so what a near row leaves uncovered shows the
// rows behind it.
//
// Bars rows are stepped: each a row of bars standing on its baseline, with
// real gaps and (in LED mode) LED segments, all shrinking with distance.
// Lines rows are smooth waves. A quiet stretch never drops out, or wherever
// the music is low the horizon goes empty: it stays low ground, a dim contour
// over a dark lip (Lines) or a row of stubs (Bars), thin enough that the cover
// shows through instead of a slab. The newest row sits on the live bars / line.
//
// Drawn behind the bars / line, as one fullscreen triangle per frame. The rows
// come from horizon.rs through the particle storage buffer (binding 4):
// vec4 0 = (rows, samples, phase, margin), vec4 1 = (core bars, group,
// stepped, _), the heights from vec4 2 on, newest row first.
//
// ⚠️  Config struct layout MUST match VisualizerConfig in shader.rs byte-for-byte
//     (same block as bars.wgsl / lines.wgsl / scope.wgsl).

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
@group(0) @binding(4) var<storage, read> rows_buf: array<vec4<f32>>;

// ── bar layout (the same arithmetic as bars.wgsl vs_main) ───────────────

fn led_segment_gap() -> f32 {
    let border_width = uniforms.config.border_width;
    return uniforms.config.bar_spacing + select(0.0, border_width, border_width > 0.0);
}

fn top_margin() -> f32 {
    return uniforms.config.border_width + uniforms.config.bar_depth_3d;
}

fn usable_height() -> f32 {
    return max(uniforms.viewport.w - top_margin(), 1.0);
}

fn bar_pitch() -> f32 {
    return uniforms.config.bar_width + led_segment_gap();
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

// Hermite step that also runs downhill when `a > b`.
fn ss(a: f32, b: f32, x: f32) -> f32 {
    let t = clamp((x - a) / (b - a), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
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

// ── rows ────────────────────────────────────────────────────────────────

const RIDGE_HORIZON: f32 = 0.96;   // the farthest baseline (fraction of the band)
const RIDGE_DEPTH: f32 = 0.30;     // perspective per row
const RIDGE_AMP: f32 = 0.95;       // nearest row's height vs the live visualizer
const RIDGE_NARROW: f32 = 0.07;    // how much narrower each row back
const FLAT_CREST: f32 = 0.45;      // a quiet stretch's contour vs a ridge's crest
const FLAT_LIP_ALPHA: f32 = 0.7;   // the dark lip right under a quiet contour
const FLAT_LIP: f32 = 0.03;        // its depth (fraction of the band)
const FLAT_HAZE: f32 = 0.05;       // the faint ground below the lip
const FLAT_BAR_BODY: f32 = 0.6;    // a quiet row bar's stub vs a loud bar's body
const BAR_STUB_PX: f32 = 3.0;      // a row bar's least height (px) without LEDs

fn ridge_value(r: u32, samples: u32, i: u32) -> f32 {
    let f = r * samples + i;
    return rows_buf[2u + f / 4u][f % 4u];
}

// Smooth: height of row `r` at field position `u` (0..1); the row covers
// `margin` past the field either side, tapering at its very ends. x = height,
// y = how much of the row is there (0 past its ends).
fn ridge_at(r: u32, samples: u32, u: f32, margin: f32) -> vec2<f32> {
    let t = (u + margin) / (1.0 + 2.0 * margin);
    if (t <= 0.0 || t >= 1.0) {
        return vec2<f32>(0.0);
    }
    let s = t * f32(samples - 1u);
    let i0 = u32(floor(s));
    let i1 = min(i0 + 1u, samples - 1u);
    let edge = ss(0.0, 0.06, t) * ss(1.0, 0.94, t);
    let h = mix(ridge_value(r, samples, i0), ridge_value(r, samples, i1), s - floor(s));
    return vec2<f32>(h * edge, edge);
}

// Stepped: row `r`'s bar under `b` (that row's own bar units, 0 = the field's
// first bar) and how much of the pixel it covers (0 in the gaps, anti-aliased
// at the sides). x = height, y = coverage.
fn ridge_step(r: u32, samples: u32, margin: f32, b: f32, frac: f32, aa: f32) -> vec2<f32> {
    let k = floor(b);
    let idx = k + margin;
    if (idx < 0.0 || idx >= f32(samples)) {
        return vec2<f32>(0.0);
    }
    let f = b - k;
    let cover = ss(-aa, aa, f) * ss(frac + aa, frac - aa, f);
    return vec2<f32>(ridge_value(r, samples, u32(idx)), cover);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let head = rows_buf[0];
    let rows = u32(head.x);
    let samples = u32(head.y);
    let phase = head.z;
    let margin = head.w;
    let head2 = rows_buf[1];
    let core = head2.x;
    let group = max(head2.y, 1.0);
    let stepped = head2.z > 0.5;
    if (rows < 2u || samples < 2u) {
        return vec4<f32>(0.0);
    }
    let vp = uniforms.viewport;
    let px = in.uv * vp.zw;
    let uh = usable_height();
    let y = (top_margin() + uh - px.y) / uh;
    // Field position: Bars span the bar field, Lines the full width (point i
    // of n at i / (n - 1), as lines.wgsl lays them out).
    var u = px.x / max(vp.z, 1.0);
    if (stepped) {
        let field = max(f32(uniforms.config.bar_count) * bar_pitch(), 1.0);
        u = (px.x - uniforms.config.edge_spacing) / field;
    }
    let dmax = 1.0 + f32(rows) * RIDGE_DEPTH;
    let deep = pow(max(uniforms.border_color.rgb, vec3<f32>(0.0)), vec3<f32>(2.2));
    let mist = ramp(0.15) * 0.42;
    var acc = vec3<f32>(0.0);
    var acc_a = 0.0;
    let pitch = bar_pitch();
    // A grouped row bar spans `group` bars: its fill share grows to match.
    let gfrac = 1.0 - (1.0 - uniforms.config.bar_width / pitch) / group;
    let bfield = u * f32(uniforms.config.bar_count) / group;
    let led = uniforms.config.led_bars != 0u;

    for (var r = 0u; r < rows; r++) {
        let z = f32(r) + phase;
        let d = 1.0 + z * RIDGE_DEPTH;
        let base = RIDGE_HORIZON * (1.0 - 1.0 / d) / (1.0 - 1.0 / dmax);
        let sc = 1.0 + z * RIDGE_NARROW;
        var h = 0.0;
        var cov = 1.0;
        var span = 1.0;
        if (stepped) {
            let br = core * 0.5 + (bfield - core * 0.5) * sc;
            let aa = 0.6 * sc / (pitch * group);
            let hs = ridge_step(r, samples, margin, br, gfrac, aa);
            // Every row bar keeps a stub (one LED in LED mode), so a quiet
            // stretch still shows its row.
            let stub = select(BAR_STUB_PX, uniforms.config.led_segment_height + 0.5 * led_segment_gap(), led);
            h = max(hs.x, stub / (uh * RIDGE_AMP));
            cov = hs.y;
            // Bars stand on their row's baseline, LED-cut in LED mode.
            cov *= ss(-1.0, 0.5, (y - base) * uh);
            if (led) {
                let period = (uniforms.config.led_segment_height + led_segment_gap()) / d;
                let seg = uniforms.config.led_segment_height / d;
                let pos = max(y - base, 0.0) * uh;
                let fp = pos - floor(pos / period) * period;
                cov *= ss(seg + 0.5, seg - 0.5, fp);
            }
        } else {
            let ur = 0.5 + (u - 0.5) * sc;
            let hs = ridge_at(r, samples, ur, margin);
            h = hs.x;
            span = hs.y;
        }
        let top = base + RIDGE_AMP * h / d;
        let dpx = (y - top) * uh;
        if (dpx < 1.6 && cov > 0.001) {
            let fog = clamp(z / f32(rows), 0.0, 1.0);
            // Far rows dissolve into the mist; the last few fade out.
            let vanish = ss(1.0, 0.75, fog);
            let line = exp(-dpx * dpx / 1.1) + exp(-abs(dpx) / 3.0) * 0.3;
            let fill = ss(0.6, -1.2, dpx);
            // Aerial perspective: near ridges dark, far ones lifted into the
            // mist, and mist pooling at each ridge's foot so layers separate.
            // Full crests only where the ridge has shape: a quiet stretch is calm
            // mist with a dimmer contour.
            let relief = select(ss(0.07, 0.22, h), ss(0.01, 0.06, h), stepped);
            let depth_below = clamp((top - y) / max(top - base + 0.08, 0.05), 0.0, 1.0);
            let haze = clamp(fog * 0.85 + depth_below * 0.45 * relief + (1.0 - relief) * 0.35, 0.0, 1.0);
            var body = mix(deep, mist, haze);
            if (stepped) {
                // Real bars, receding: the bar gradient up the bar, dimmed and
                // lost to the mist with distance.
                let up = clamp((y - base) * d / RIDGE_AMP, 0.0, 1.0);
                body = mix(ramp(up) * 0.62, mist, clamp(fog * 0.9, 0.0, 1.0));
            }
            var crest = ramp(clamp(0.45 + h * 0.8, 0.0, 1.0));
            crest = mix(crest, mist * 1.4, fog * 0.5);
            // Dense under the crest, thinning below it so the cover ghosts
            // through the range instead of a solid slab.
            let under = max(top - y, 0.0);
            let thin = select(mix(0.16, 1.0, exp(-under / 0.08)), 1.0, stepped);
            // Flat (quiet) stretches are low ground: no slab, the cover shows.
            let ground = select(ss(0.02, 0.14, h), ss(0.0, 0.05, h), stepped);
            // But a row never vanishes there, or wherever the music is low the
            // horizon goes empty: a smooth row keeps a dimmer contour over a
            // dark lip, a stepped row its stubs.
            let lip = mix(FLAT_HAZE, FLAT_LIP_ALPHA, exp(-under / FLAT_LIP));
            let floor_a = select(lip, FLAT_BAR_BODY, stepped) * span;
            let crest_gate = max(relief, FLAT_CREST * span);
            let fill_a = fill * mix(0.92, 0.55, fog) * vanish * max(thin * ground, floor_a) * cov;
            let line_a = clamp(line, 0.0, 1.0) * vanish * (1.0 - 0.55 * fog) * crest_gate * cov;
            let top_fade = ss(1.0, 0.82, y) * uniforms.config.global_opacity;
            let a = clamp(fill_a + line_a * (1.0 - fill_a), 0.0, 1.0) * top_fade;
            let col = (body * fill_a * (1.0 - line_a) + crest * line_a * 1.3) * top_fade;
            // Front to back: what this row leaves uncovered shows the next.
            acc = acc + col * (1.0 - acc_a);
            acc_a = acc_a + a * (1.0 - acc_a);
            if (acc_a > 0.995) {
                break;
            }
        }
    }
    return vec4<f32>(acc, acc_a);
}
