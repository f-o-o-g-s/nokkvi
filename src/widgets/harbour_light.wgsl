// Harbour Trawl night light: the sky, the aurora, the water and the seabed
// of the Trawl scene, drawn per pixel under the canvas furniture (stars,
// kelp, fish, crate, anchor, boat). A port of the `nokkvi - aurora` MilkDrop
// preset's curtain, without its feedback buffer: every term is a function of
// the pixel and the scene clock, so the panel needs no history and a resize
// or a theme change is clean on the next frame.
//
// Coordinates: `uv` runs 0..1 across the panel with y DOWN (the pass viewport
// is the widget bounds); the scene works in `Y` = height above the panel
// bottom as a fraction of the panel height (the same convention as
// `harbour_sea::sea_bars`) and `X` = `uv.x` scaled by the aspect, so a unit
// in X and Y is one panel height.
//
// The waterlines come from the CPU (`front` / `back`, 128 samples each, the
// same sampler the boat physics and the canvas use), so the hull rides
// exactly the surface this shader lights.

struct Scene {
    // panel width px, panel height px, scene clock s, 1 = decode to sRGB-linear
    frame: vec4<f32>,
    // boat x (0..1 of width), boat centre height (Y), boat height (Y units), shown
    boat: vec4<f32>,
    // moon x (0..1), height (Y), radius (Y units), breath (0 = no moon)
    moon: vec4<f32>,
    // star count, unused x3
    sky: vec4<f32>,
    bg: vec4<f32>,
    text: vec4<f32>,
    highlight: vec4<f32>,
    warm: vec4<f32>,
    ramp: array<vec4<f32>, 6>,
    // waterline heights (Y), 128 samples across the width, 4 per vec4
    front: array<vec4<f32>, 32>,
    back: array<vec4<f32>, 32>,
    // x (0..1), height (Y), radius (Y units; negative = a sparkle), alpha
    stars: array<vec4<f32>, 64>,
};

@group(0) @binding(0) var<uniform> scene: Scene;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VOut {
    // One triangle covering the viewport: uv (0,0) top-left .. (2,2).
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VOut;
    o.pos = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    return o;
}

// ── helpers ─────────────────────────────────────────────────────────────

// Hermite step that also runs downhill when `a > b`.
fn ss(a: f32, b: f32, x: f32) -> f32 {
    let t = clamp((x - a) / (b - a), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

// Uniform 0..1 from an integer lattice point.
fn hash2(c: vec2<f32>) -> f32 {
    let x = bitcast<u32>(i32(c.x));
    let y = bitcast<u32>(i32(c.y));
    return f32(pcg(x ^ pcg(y + 0x9e3779b9u))) * (1.0 / 4294967295.0);
}

// Smooth value noise, one lattice cell per unit, quintic fade (no grid).
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let c = hash2(i + vec2<f32>(0.0, 1.0));
    let d = hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// The theme's visualizer gradient, dark (0) to light (1).
fn ramp(x: f32) -> vec3<f32> {
    let s = clamp(x, 0.0, 1.0) * 5.0;
    var c = mix(scene.ramp[0].rgb, scene.ramp[1].rgb, clamp(s, 0.0, 1.0));
    c = mix(c, scene.ramp[2].rgb, clamp(s - 1.0, 0.0, 1.0));
    c = mix(c, scene.ramp[3].rgb, clamp(s - 2.0, 0.0, 1.0));
    c = mix(c, scene.ramp[4].rgb, clamp(s - 3.0, 0.0, 1.0));
    c = mix(c, scene.ramp[5].rgb, clamp(s - 4.0, 0.0, 1.0));
    return c;
}

fn front_sample(i: u32) -> f32 {
    return scene.front[i / 4u][i % 4u];
}

fn back_sample(i: u32) -> f32 {
    return scene.back[i / 4u][i % 4u];
}

// Waterline height at `u` (0..1 across), linear between the 128 samples.
fn front_at(u: f32) -> f32 {
    let f = clamp(u, 0.0, 1.0) * 127.0;
    let i0 = u32(floor(f));
    let i1 = min(i0 + 1u, 127u);
    return mix(front_sample(i0), front_sample(i1), fract(f));
}

fn back_at(u: f32) -> f32 {
    let f = clamp(u, 0.0, 1.0) * 127.0;
    let i0 = u32(floor(f));
    let i1 = min(i0 + 1u, 127u);
    return mix(back_sample(i0), back_sample(i1), fract(f));
}

// ── the aurora ──────────────────────────────────────────────────────────
// A folded curtain: a wavy arc (three drifting sine folds) with rays
// reaching up from it, brightest at the arc, in patches along it and where
// a fold turns edge-on, streaked by three layers of vertically stretched
// noise. A fainter, slower curtain hangs higher behind it. `q1` walks the
// folds, `q2` drifts the curtain sideways.

const AURORA_BASE: f32 = 0.67;
const AURORA_BACK_BASE: f32 = 0.80;

fn arc_base(px: f32, q1: f32, q2: f32) -> f32 {
    let xx = px + q2;
    return AURORA_BASE + 0.035 * sin(xx * 2.1 + q1 * 0.7) + 0.02 * sin(xx * 5.3 - q1 * 1.1)
        + 0.01 * sin(xx * 11.0 + q1 * 1.9);
}

// x = the curtain's light at (u, y); y = a soft wide glow standing in for
// the preset's feedback trail and blur.
fn aurora(u: f32, px: f32, y: f32, q1: f32, q2: f32) -> vec2<f32> {
    let xx = px + q2;
    let base = arc_base(px, q1, q2);
    let slope = 0.07 * cos(xx * 2.1 + q1 * 0.7) + 0.106 * cos(xx * 5.3 - q1 * 1.1)
        + 0.11 * cos(xx * 11.0 + q1 * 1.9);
    let dens = 0.45 + 2.2 * abs(slope);
    let env = 0.25 + 0.75 * vnoise(vec2<f32>(u * 8.0 + q2 * 4.8, 7.36));
    let above = y - base;
    let r1 = vnoise(vec2<f32>(u * 96.0 + q2 * 11.2, y * 3.84 + q1 * 0.48));
    let r1c = vnoise(vec2<f32>(u * 96.0 + q2 * 11.2, 16.0 + q1 * 0.32));
    let r2 = vnoise(vec2<f32>(u * 256.0 - q2 * 19.2, y * 8.0 - q1 * 1.28));
    let r3 = vnoise(vec2<f32>(u * 640.0 + q2 * 28.8, y * 1.92 + q1 * 0.64));
    let len = 0.15 * (0.5 + 1.1 * r1c * r1c);
    let body = exp(-max(above, 0.0) / len) * ss(-0.02, 0.004, above);
    let edge = exp(-abs(above) / 0.014) * (0.6 + 0.4 * r2);
    let rays = (0.3 + 0.7 * r1 * r1) * (0.6 + 0.4 * r2) * (0.7 + 0.3 * r3);
    var cur = (body * rays + edge * 0.6) * dens * env * 0.68;

    let xx2 = px * 0.8 - q2 * 0.5 + 3.0;
    let base2 = AURORA_BACK_BASE + 0.025 * sin(xx2 * 1.7 + q1 * 0.4) + 0.012 * sin(xx2 * 4.1 - q1 * 0.6);
    let above2 = y - base2;
    let body2 = exp(-max(above2, 0.0) / 0.12) * ss(-0.07, 0.02, above2);
    let env2 = 0.3 + 0.7 * vnoise(vec2<f32>(u * 12.8 - q2 * 3.2 + 16.0, 19.5));
    cur += body2 * env2 * (0.4 + 0.6 * r1) * (0.6 + 0.4 * r3) * 0.3;

    let soft = (exp(-max(above, 0.0) / (len * 2.5)) * ss(-0.08, 0.0, above)
        + exp(-abs(above) / 0.05) * 0.5) * dens * env * 0.35;
    return vec2<f32>(clamp(cur, 0.0, 1.0), clamp(soft, 0.0, 1.0));
}

// ── glowing plankton ────────────────────────────────────────────────────
// Sparse motes riding a slow current, one per lattice cell at most, each
// with a short fading tail behind it and its own slow blink. Drawn in the
// current's frame (X minus the drift), so a mote is still there and its
// tail runs straight back along -x. x = core + tail, y = a wide halo.
fn plankton(px: f32, y: f32, t: f32, cell: f32, speed: f32, seed: f32, r: f32, tl: f32,
            density: f32) -> vec2<f32> {
    let q = vec2<f32>(px - t * speed, y);
    let id = floor(q / cell);
    var core = 0.0;
    var halo = 0.0;
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            let c = id + vec2<f32>(f32(i), f32(j));
            if (hash2(c + vec2<f32>(seed, seed * 1.7)) > density) {
                continue;
            }
            let ox = hash2(c + vec2<f32>(seed + 11.0, 3.0));
            let oy = hash2(c + vec2<f32>(7.0, seed + 13.0));
            let ph = hash2(c + vec2<f32>(seed + 29.0, 31.0));
            let wob = 0.1 * sin(t * (0.4 + 0.5 * ph) + ph * 6.283);
            let pc = (c + vec2<f32>(ox, 0.1 + 0.8 * oy + wob)) * cell;
            let dv = q - pc;
            let along = clamp(-dv.x / tl, 0.0, 1.0);
            let sd = length(vec2<f32>(dv.x + along * tl, dv.y));
            let blink = pow(0.5 + 0.5 * sin(t * (0.7 + 1.3 * ph) + ph * 40.0), 3.0);
            let b = blink * (0.5 + 0.5 * hash2(c + vec2<f32>(seed + 41.0, 43.0)));
            let d2 = dot(dv, dv);
            core += (exp(-d2 / (r * r)) + exp(-sd * sd / (r * r * 0.5)) * (1.0 - along) * (1.0 - along) * 0.45) * b;
            halo += exp(-d2 / (r * r * 16.0)) * b;
        }
    }
    return vec2<f32>(core, halo);
}

// A luminous current: a winding thread with a soft sheath, patchy along its
// length and wisped by stretched noise drifting with the water.
fn current(px: f32, y: f32, mid: f32, env: f32, wisp: f32) -> vec2<f32> {
    let d = abs(y - mid);
    return vec2<f32>(exp(-d / 0.006) * env * wisp, exp(-d / 0.025) * env);
}

// ── the scene ───────────────────────────────────────────────────────────

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let w = scene.frame.x;
    let h = max(scene.frame.y, 1.0);
    let t = scene.frame.z;
    let aspect = w / h;
    let u = in.uv.x;
    let y = 1.0 - in.uv.y;
    let px = (u - 0.5) * aspect;
    let q1 = t * 0.30;
    let q2 = t * 0.03;

    let bg = scene.bg.rgb;
    let text = scene.text.rgb;
    let hl = scene.highlight.rgb;
    let warm = scene.warm.rgb;
    let air = ramp(0.25);
    let lc = ramp(0.85);

    let surf = front_at(u);
    let backs = back_at(u);

    // Sky: the theme's darkest background, lifting toward the horizon, with
    // a faint airglow gathering at the waterline.
    var col = bg * (0.62 + 0.45 * ss(1.0, 0.5, y));
    col += air * 0.10 * exp(-max(y - surf, 0.0) / 0.12);

    // The moon, behind the aurora: a luminous disc, its limb a touch
    // darker and its face faintly mottled, in a soft bloom that breathes.
    // (During the moon's dream the face's marks arrive as a sprite on top.)
    let starc = mix(lc, text, 0.6);
    if (scene.moon.w > 0.0) {
        let mr = scene.moon.z;
        let mv = vec2<f32>((u - scene.moon.x) * aspect, y - scene.moon.y);
        let dm = length(mv);
        let bloom = exp(-max(dm - mr, 0.0) / (mr * 0.45)) * 0.16 + exp(-dm / (mr * 2.8)) * 0.07;
        col += starc * bloom * scene.moon.w;
        let limb = sqrt(max(1.0 - (dm / mr) * (dm / mr), 0.0));
        let maria = vnoise(mv / mr * 2.2 + 5.0) * 0.6 + vnoise(mv / mr * 5.0 + 9.0) * 0.4;
        let face = starc * (0.62 + 0.2 * limb) * (1.0 - 0.16 * maria);
        col = mix(col, face, ss(mr, mr - 1.5 / h, dm));
    }

    let a = aurora(u, px, y, q1, q2);
    let base = arc_base(px, q1, q2);
    let hpos = clamp((y - base) / 0.4, 0.0, 1.0);
    let cc = ramp(0.92 - 0.85 * hpos);
    col += cc * a.x * 1.45;
    col += warm * a.x * ss(0.35, 0.9, hpos) * 0.35;
    col += text * pow(a.x, 3.0) * 0.4;
    col += cc * a.y * 0.45 + hl * a.y * 0.10;

    // The stars (positions, twinkle and the black hole's pull come from the
    // CPU): a tight core in a soft glow, the sparkles with fine cross
    // spikes, dimmed where the curtain hangs in front of them.
    if (y > backs) {
        var starl = 0.0;
        let n = min(u32(scene.sky.x), 64u);
        for (var i = 0u; i < n; i++) {
            let st = scene.stars[i];
            let r = abs(st.z);
            let dv = vec2<f32>((u - st.x) * aspect, y - st.y);
            if (max(abs(dv.x), abs(dv.y)) > r * 8.0) {
                continue;
            }
            let d2 = dot(dv, dv);
            var v = exp(-d2 / (r * r * 0.3)) + exp(-sqrt(d2) / (r * 1.4)) * 0.22;
            if (st.z < 0.0) {
                let ax = abs(dv.x);
                let ay = abs(dv.y);
                v += (exp(-ax / (r * 0.1)) * exp(-ay / (r * 3.2))
                    + exp(-ay / (r * 0.1)) * exp(-ax / (r * 2.0))) * 0.55;
            }
            starl += v * st.w;
        }
        col += starc * starl * (1.0 - clamp(a.x * 2.0, 0.0, 1.0) * 0.6);
    }

    // Stretched ripple noise shared by the far swell and the surface.
    let hx = vnoise(vec2<f32>(u * 38.4 + t * 0.1, y * h * 0.25 - t * 0.6)) - 0.5;
    
    // The far swell: dark distant water with a broken sheen of the curtain
    // mirrored in it and a faint lit crest.
    let inback = ss(backs + 0.002, backs - 0.002, y);
    if (inback > 0.0) {
        let db = max(backs - y, 0.0);
        let ru = u + hx * 0.02;
        let ra = aurora(ru, (ru - 0.5) * aspect, backs + db * 2.5, q1, q2);
        let rpos = clamp((backs + db * 2.5 - arc_base((ru - 0.5) * aspect, q1, q2)) / 0.4, 0.0, 1.0);
        var backcol = bg * 0.85 + air * 0.09 * (1.0 - ss(0.0, 0.08, db));
        backcol += ramp(0.92 - 0.85 * rpos) * (ra.x + ra.y * 0.5) * 0.3 * (0.55 + 0.9 * hx);
        backcol += mix(hl, text, 0.3) * exp(-db / 0.002) * 0.10;
        col = mix(col, backcol, inback);
    }

    // The near water: the aurora's own grammar turned upside down. The
    // surface is a bright arc, brighter where a fold turns edge-on, with
    // streaked rays of light hanging from it into the water, kept a step
    // dimmer than the sky's curtain.
    let d0 = surf - y;
    let inw = ss(-0.002, 0.002, d0);
    // Slope of the waterline (per panel height): steep stretches are folds
    // seen edge-on, where the light gathers, as on the curtain.
    let du = 1.0 / 127.0;
    let slope = (front_at(u + du) - front_at(u - du)) / (2.0 * du * aspect);
    let fold = 0.5 + 2.5 * abs(slope) / 0.12;
    let senv = 0.35 + 0.65 * vnoise(vec2<f32>(u * 6.0 - q2 * 3.0, 41.0));
    if (inw > 0.0) {
        let d = max(d0, 0.0);
        let wtop = ramp(0.4);
        let wdeep = ramp(0.1);
        let bio = ramp(0.7);
        var water = mix(wtop * 0.26, wdeep * 0.18, ss(0.0, 0.2, d));
        water = mix(water, bg * 0.28, ss(0.12, 0.42, d));

        // The curtain's light over this column: where the aurora flares,
        // the water under it brightens.
        let sbase = arc_base(px, q1, q2);
        let a1 = aurora(u, px, sbase + 0.03, q1, q2);
        let lit = clamp(a1.x + a1.y, 0.0, 1.0);

        // Hanging rays: the sky curtain's three streak layers, reaching
        // down instead of up and drifting with the water.
        let r1 = vnoise(vec2<f32>(u * 96.0 - q2 * 9.0, d * 3.84 + q1 * 0.3));
        let r1c = vnoise(vec2<f32>(u * 96.0 - q2 * 9.0, 31.0 + q1 * 0.25));
        let r2 = vnoise(vec2<f32>(u * 256.0 + q2 * 15.0, d * 8.0 - q1 * 0.9));
        let r3 = vnoise(vec2<f32>(u * 640.0 - q2 * 22.0, d * 1.92 + q1 * 0.5));
        let len = 0.11 * (0.5 + 1.1 * r1c * r1c);
        let rays = (0.3 + 0.7 * r1 * r1) * (0.6 + 0.4 * r2) * (0.7 + 0.3 * r3);
        let hang = exp(-d / len) * rays * fold * senv * (0.55 + 0.6 * lit);
        let hpos_w = clamp(d / 0.3, 0.0, 1.0);
        let wc = ramp(0.85 - 0.6 * hpos_w);
        water += wc * clamp(hang, 0.0, 1.0) * 0.36;
        water += wc * exp(-d / 0.08) * fold * senv * 0.06;

        // Glowing plankton (fine motes + a few nearer ones) and two
        // luminous currents winding through mid-water.
        let deep_in = ss(0.01, 0.04, d);
        let p1 = plankton(px, y, t, 0.045, 0.012, 3.0, 0.0024, 0.028, 0.42);
        let p2 = plankton(px, y, t, 0.09, 0.02, 17.0, 0.0038, 0.04, 0.3);
        let rb = 0.33 + 0.05 * sin(px * 2.3 + t * 0.21) + 0.02 * sin(px * 6.1 - t * 0.33);
        let rb2 = 0.20 + 0.04 * sin(px * 1.7 - t * 0.17 + 2.0) + 0.015 * sin(px * 5.3 + t * 0.29);
        let renv = ss(0.3, 0.85, vnoise(vec2<f32>(u * 9.0 + t * 0.1, 23.4)));
        let renv2 = ss(0.35, 0.9, vnoise(vec2<f32>(u * 7.5 - t * 0.08, 6.1)));
        let wisp = 0.55 + 0.45 * vnoise(vec2<f32>((px - t * 0.05) * 24.0, y * h * 0.12));
        let c1 = current(px, y, rb, renv, wisp);
        let c2 = current(px, y, rb2, renv2, wisp);
        let glow_core = (p1.x + p2.x * 1.3 + (c1.x + c2.x * 0.8) * 0.18) * deep_in;
        let glow_soft = (p1.y * 0.12 + p2.y * 0.2 + (c1.y + c2.y * 0.8) * 0.22) * deep_in;
        water += bio * (glow_core * 0.32 + glow_soft * 0.35);
        water += text * ss(0.1, 0.8, glow_core) * 0.18;

        // The seabed: a floor seen at a slight angle, sand ripples and a
        // dancing net of light across it, dim under the deep water.
        let bt = 0.17 + 0.018 * vnoise(vec2<f32>(u * 19.2, 3.5)) + 0.01 * sin(px * 4.0 + 1.0);
        let onbed = ss(-0.003, 0.025, bt - y);
        if (onbed > 0.0) {
            let fz = max(bt - y, 0.0);
            let per = 0.03 + fz * 3.0;
            let fq = vec2<f32>(px / per, 1.0 / per) * 0.05;
            let fa = vnoise(vec2<f32>((fq.x * 1.6 + t * 0.005) * 32.0, (fq.y * 0.9 + t * 0.004) * 32.0));
            let fb = vnoise(vec2<f32>((fq.x * 1.5 - t * 0.004) * 32.0, (fq.y - t * 0.005) * 32.0) + 9.6);
            let fnz = vnoise(fq * 19.2 + 22.4);
            let fc = pow(clamp(1.0 - abs(fa - fb) * 6.0, 0.0, 1.0), 5.0);
            let rip = 0.5 + 0.5 * sin(fq.y * 260.0 + fnz * 9.0 + fq.x * 6.0);
            let near = ss(0.0, 0.16, fz);
            var sand = mix(wdeep * 0.25 + bg * 0.22, bg * 0.36 + air * 0.05, near);
            sand *= 0.85 + 0.25 * rip * ss(0.04, 0.14, fz);
            sand += mix(lc, text, 0.35) * fc * (0.15 + 0.6 * lit) * (0.5 + 0.5 * near) * 0.38;
            sand += bio * glow_soft * 0.3;
            let rim = exp(-fz / 0.004);
            sand += lc * rim * (0.1 + 0.3 * lit) * 0.25;
            sand = mix(sand, water, (1.0 - near) * 0.5);
            water = mix(water, sand, onbed);
        }

        col = mix(col, water, inw);
    }
    // The surface arc, as the curtain's edge: a fine bright line in a soft
    // glow, patchy along its length and brightest on edge-on folds.
    let r2s = vnoise(vec2<f32>(u * 256.0 + q2 * 15.0, 7.0 - q1 * 0.9));
    let arc = (exp(-abs(d0) / 0.0025) * 0.8 + exp(-abs(d0) / 0.012) * 0.25) * (0.6 + 0.4 * r2s);
    col += mix(lc, text, 0.2) * clamp(arc * fold * senv, 0.0, 1.0) * 0.38;

    // A soft glow behind the hull so the dark boat stands out against the
    // night (the boat sprite draws over it).
    if (scene.boat.w > 0.5) {
        let bd = vec2<f32>((u - scene.boat.x) * aspect, (y - scene.boat.y) * 1.3);
        col += mix(lc, text, 0.3) * exp(-length(bd) / (scene.boat.z * 0.45)) * 0.16;
    }

    col *= 0.86 + 0.14 * (1.0 - ss(0.3, 1.1, length(vec2<f32>(px, y - 0.5))));
    col = max(col, vec3<f32>(0.0));
    if (scene.frame.w > 0.5) {
        col = pow(col, vec3<f32>(2.2));
    }
    return vec4<f32>(col, 1.0);
}
