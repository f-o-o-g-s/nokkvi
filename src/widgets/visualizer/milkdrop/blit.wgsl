// MilkDrop blit: samples the engine's retained composite texture into iced's
// pass. The pass viewport is the widget's bounds and the scissor its clip
// bounds, so one fullscreen triangle fills exactly the panel.
//
// The retained texture is Rgba8Unorm holding display-space values. iced keeps
// its `web-colors` feature, so its surface is non-sRGB and a raw copy shows the
// values as the standalone player did. If the target were ever sRGB, its encode
// would brighten them, so `decode_srgb` undoes that first.

struct BlitParams {
    decode_srgb: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

struct VertexOut {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
}

@group(0) @binding(0) var t_comp: texture_2d<f32>;
@group(0) @binding(1) var s_comp: sampler;
@group(0) @binding(2) var<uniform> params: BlitParams;

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

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    let c = textureSample(t_comp, s_comp, in.uv);
    var rgb = c.rgb;
    if (params.decode_srgb != 0u) {
        rgb = srgb_to_linear(rgb);
    }
    // Opaque: the preset replaces the cover underneath.
    return vec4f(rgb, 1.0);
}
