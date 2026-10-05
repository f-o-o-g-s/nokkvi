// Reflection (Bars / Lines): the visualizer stands on a waterline and is
// mirrored in a band of dark water below it. One post-process pass over the displayed scene (the
// resolve / trail / echo texture at @group(0), the blit layout) drawn after the
// scene blit: the water body is a translucent tint so it reads over a bright
// cover, the reflection is the scene flipped about the waterline (compressed so
// the whole skyline fits the band), wobbled by wavelets that grow with depth
// and on the bass, smeared downward like lights on water, broken into ripple
// lines, and pushed sideways by the kick ripples the CPU spawns.

struct ReflectionParams {
    // waterline (uv y, 0 = top), clock s, beat pulse, bass level
    surface: vec4<f32>,
    // widget width px, height px, mirror compression, unused
    size: vec4<f32>,
    // water body colour
    tint: vec4<f32>,
    // surface highlight colour
    glint: vec4<f32>,
    // kick ripples: x (0..1 across), age s, strength (0 = none), unused
    ripples: array<vec4<f32>, 4>,
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(1) @binding(0) var<uniform> water: ReflectionParams;

struct VOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_reflection(@builtin(vertex_index) idx: u32) -> VOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var out: VOut;
    out.position = vec4<f32>(positions[idx], 0.0, 1.0);
    out.uv = positions[idx] * vec2<f32>(0.5, -0.5) + 0.5;
    return out;
}

fn hash2(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let c = hash2(i + vec2<f32>(0.0, 1.0));
    let d = hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Wavelet + streak tuning (px unless noted).
const SWELL_BASE: f32 = 1.2;       // wobble at rest
const SWELL_BASS: f32 = 4.0;       // extra wobble on the bass
const STREAK_TOP: f32 = 0.004;     // downward smear at the surface (uv)
const STREAK_DEEP: f32 = 0.05;     // ... at the bottom of the band (uv)
const KICK_SPEED: f32 = 0.30;      // ripple front speed (uv / s)
const KICK_WIDTH: f32 = 0.05;      // ripple packet half-width (uv)
const KICK_PUSH: f32 = 7.0;        // ripple sideways push (px)
const KICK_DECAY: f32 = 1.3;       // ripple fade per second

@fragment
fn fs_reflection(in: VOut) -> @location(0) vec4<f32> {
    let line = water.surface.x;
    let t = water.surface.y;
    let beat = water.surface.z;
    let bass = clamp(water.surface.w * 1.6, 0.0, 1.0);
    let size = max(water.size.xy, vec2<f32>(1.0));
    let stretch = water.size.z;
    let uv = in.uv;
    if (uv.y < line) {
        discard;
    }
    let band = max(1.0 - line, 1e-3);
    let d = (uv.y - line) / band;          // 0 at the surface, 1 at the bottom
    let ypx = (uv.y - line) * size.y;      // px below the surface
    let px = 1.0 / size.x;

    // Perspective depth: rows near the waterline are far water (tight, fine
    // ripples), rows lower down are near water (wide, slow swells). `w` runs
    // evenly in that perspective, so ripples are spaced like real water
    // instead of a fixed pixel period (which chopped solid bars into blocks).
    let w = log(1.0 + ypx / 5.0) * 9.0;
    let n1 = vnoise(vec2<f32>(uv.x * 5.0 + t * 0.21, w * 0.55 - t * 0.9));
    let n2 = vnoise(vec2<f32>(uv.x * 13.0 - t * 0.35, w * 1.3 - t * 1.6));
    let amp = (SWELL_BASE + SWELL_BASS * bass) * (0.3 + 0.7 * d);
    var dx = amp * px * ((n1 - 0.5) * 2.2 + (n2 - 0.5) * 0.9
        + 0.35 * sin(w * 1.7 - t * 2.1 + n1 * 3.0));

    // Kick ripples: a packet running sideways both ways from where it landed.
    var lift = 0.0;
    for (var i = 0u; i < 4u; i++) {
        let r = water.ripples[i];
        if (r.z <= 0.0) {
            continue;
        }
        let dist = abs(uv.x - r.x) - r.y * KICK_SPEED;
        let env = exp(-dist * dist / (KICK_WIDTH * KICK_WIDTH)) * r.z * exp(-r.y * KICK_DECAY);
        dx += env * px * KICK_PUSH * sin(dist * 140.0 - ypx * 0.35) * (0.4 + 0.6 * d);
        lift += env;
    }

    // Mirror about the waterline (compressed), smeared toward the depths.
    let src_y = line - (uv.y - line) * stretch;
    let streak = mix(STREAK_TOP, STREAK_DEEP, d);
    var refl = vec4<f32>(0.0);
    for (var k = 0; k < 6; k++) {
        let o = f32(k) / 5.0;
        refl += textureSampleLevel(tex, samp, vec2<f32>(uv.x + dx, src_y + o * streak), 0.0);
    }
    refl = refl / 6.0;

    // Ripple lines: soft, broken dark seams between swells, irregular in
    // both directions so they never read as a stack of blocks.
    let seam = sin(w * 2.3 - t * 1.4 + n2 * 4.0);
    let broken = smoothstep(0.35, 0.75, vnoise(vec2<f32>(uv.x * 9.0 + n1 * 2.0, w * 0.8)));
    let slivers = 1.0 - 0.28 * smoothstep(0.55, 0.95, seam) * broken;
    let fade = mix(1.0, 0.45, d) * slivers;

    // Water body (premultiplied), deepening toward the bottom. The theme's
    // dark stroke colour, decoded to linear (the target is sRGB) and sunk a
    // little further so the band reads as deep water over a bright cover.
    let deep = pow(max(water.tint.rgb, vec3<f32>(0.0)), vec3<f32>(2.2)) * 0.8;
    let body_a = mix(0.80, 0.93, d);
    var col = deep * body_a;
    var a = body_a;
    let ra = clamp(refl.a * fade, 0.0, 1.0);
    col = refl.rgb * fade + col * (1.0 - ra);
    a = ra + a * (1.0 - ra);

    // The surface: a thin shimmering rim, brightest under the bright bars.
    let rim = exp(-(ypx * ypx) / 2.2) * (0.35 + 0.65 * vnoise(vec2<f32>(uv.x * 90.0 + t * 2.0, t * 0.7)));
    let under = textureSampleLevel(tex, samp, vec2<f32>(uv.x, line - 0.01), 0.0).a;
    col += water.glint.rgb * rim * (0.6 + 1.2 * under + 0.6 * beat);
    a = max(a, clamp(rim, 0.0, 1.0));

    // Glints riding the wavelets under bright reflections; kicks flash them.
    let g = vnoise(vec2<f32>((uv.x + dx) * size.x * 0.22, w * 3.0 - t * 1.8));
    let spark = smoothstep(0.88, 0.99, g) * (refl.a * 0.6 + 0.6 * lift) * (1.0 - 0.7 * d);
    col += water.glint.rgb * spark * 0.6;
    col += water.glint.rgb * lift * 0.08 * (1.0 - d);
    return vec4<f32>(col, clamp(a, 0.0, 1.0));
}
