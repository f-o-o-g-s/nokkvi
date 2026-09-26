// MilkDrop blit: samples the engine's retained composite texture into iced's
// pass. The pass viewport is the widget's bounds and the scissor its clip
// bounds, so one fullscreen triangle fills exactly the panel.
//
// The retained texture is Rgba8Unorm holding display-space values. iced keeps
// its `web-colors` feature, so its surface is non-sRGB and a raw copy shows the
// values as the standalone player did. If the target were ever sRGB, its encode
// would brighten them, so `decode_srgb` undoes that first.
//
// Crossfade: group 0 is the incoming preset, group 1 the outgoing one (both use
// the same layout; binding 2 of group 1 is the same params buffer, unused
// here). A solo draw binds one group twice with progress 1. As in MilkDrop, a
// per-pixel pattern decides where the incoming preset takes over first, with a
// soft band, so most pixels show one preset whole at any moment: a plain mix of
// two additive-on-black presets would dim both at the halfway point.

struct BlitParams {
    decode_srgb: u32,
    // 0 uniform, 1 wipe, 2 radial, 3 plasma (`BlendPattern::shader_id`).
    pattern: u32,
    seed: u32,
    _pad0: u32,
    // Eased 0..1; 1 = the incoming preset alone.
    progress: f32,
    softness: f32,
    // Panel width / height, so wipes and rings are not squashed.
    aspect: f32,
    _pad1: f32,
}

struct VertexOut {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
}

@group(0) @binding(0) var t_in: texture_2d<f32>;
@group(0) @binding(1) var s_in: sampler;
@group(0) @binding(2) var<uniform> params: BlitParams;
@group(1) @binding(0) var t_out: texture_2d<f32>;
@group(1) @binding(1) var s_out: sampler;

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOut {
    var positions = array<vec2f, 3>(
        vec2f(-1.0, -1.0),
        vec2f(3.0, -1.0),
        vec2f(-1.0, 3.0),
    );
    var out: VertexOut;
    out.position = vec4f(positions[idx], 0.0, 1.0);
    out.uv = positions[idx] * vec2f(0.5, -0.5) + 0.5;
    return out;
}

fn srgb_to_linear(c: vec3f) -> vec3f {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3f(2.4));
    return select(high, low, c <= vec3f(0.04045));
}

fn linear_to_srgb(c: vec3f) -> vec3f {
    let low = c * 12.92;
    let high = 1.055 * pow(max(c, vec3f(0.0)), vec3f(1.0 / 2.4)) - 0.055;
    return select(high, low, c <= vec3f(0.0031308));
}

fn hash21(p: vec2f, s: f32) -> f32 {
    return fract(sin(dot(p, vec2f(127.1, 311.7)) + s * 17.13) * 43758.5453);
}

fn value_noise(p: vec2f, s: f32) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i, s);
    let b = hash21(i + vec2f(1.0, 0.0), s);
    let c = hash21(i + vec2f(0.0, 1.0), s);
    let d = hash21(i + vec2f(1.0, 1.0), s);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// When each pixel changes over, 0 (first) .. 1 (last).
fn pattern_at(uv: vec2f) -> f32 {
    let rnd = f32(params.seed & 0xffffu) / 65535.0;
    let p = vec2f((uv.x - 0.5) * params.aspect, uv.y - 0.5);
    switch params.pattern {
        case 1u: {
            let ang = rnd * 6.2831853;
            let dir = vec2f(cos(ang), sin(ang));
            let extent = 0.5 * (abs(dir.x) * params.aspect + abs(dir.y));
            return clamp(dot(p, dir) / (2.0 * extent) + 0.5, 0.0, 1.0);
        }
        case 2u: {
            let r = clamp(length(p) / (0.5 * length(vec2f(params.aspect, 1.0))), 0.0, 1.0);
            return select(r, 1.0 - r, (params.seed & 0x10000u) != 0u);
        }
        case 3u: {
            let s = rnd * 100.0;
            let q = p * 3.0;
            let n = 0.57 * value_noise(q, s) + 0.29 * value_noise(q * 2.0 + 5.2, s)
                + 0.14 * value_noise(q * 4.0 + 1.7, s);
            return clamp((n - 0.5) * 1.8 + 0.5, 0.0, 1.0);
        }
        default: {
            return 0.5;
        }
    }
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    // Both samples first: textureSample needs uniform control flow.
    let b = textureSample(t_in, s_in, in.uv).rgb;
    let a = textureSample(t_out, s_out, in.uv).rgb;
    let pr = params.progress;
    var shown: vec3f;
    if (pr >= 1.0) {
        shown = b;
    } else if (params.pattern == 0u) {
        // Staggered weights, max-combined: neither side drops below full
        // strength for half the fade, and nothing overbrights.
        let wa = min(1.0, 2.0 * (1.0 - pr));
        let wb = min(1.0, 2.0 * pr);
        shown = linear_to_srgb(max(srgb_to_linear(a) * wa, srgb_to_linear(b) * wb));
    } else {
        let s = max(params.softness, 0.001);
        let t = clamp((pr * (1.0 + s) - pattern_at(in.uv)) / s, 0.0, 1.0);
        if (t <= 0.0) {
            shown = a;
        } else if (t >= 1.0) {
            shown = b;
        } else {
            // Mixed in linear light, back to the texture's display space.
            shown = linear_to_srgb(mix(srgb_to_linear(a), srgb_to_linear(b), t));
        }
    }
    var rgb = shown;
    if (params.decode_srgb != 0u) {
        rgb = srgb_to_linear(rgb);
    }
    // Opaque: the preset replaces the cover underneath.
    return vec4f(rgb, 1.0);
}
