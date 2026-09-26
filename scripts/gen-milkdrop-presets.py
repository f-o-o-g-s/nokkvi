#!/usr/bin/env python3
"""Generate nokkvi's own MilkDrop presets into assets/milkdrop/.

Usage: python3 scripts/gen-milkdrop-presets.py assets/milkdrop

The JSON files are the shipped artifact (build.rs embeds them); edit this
script and re-run it rather than editing the JSON by hand. Colours are
NOKKVI_* placeholders that nokkvi fills from the active theme at load time
(see src/widgets/visualizer/milkdrop/palette.rs); `sampler_*cover` is the
playing album's cover. Every preset is named "nokkvi - …", which the
`nokkvi` Presets setting draws from.
"""
import json, sys, os
OUT = sys.argv[1]

HEAD = '''
  vec2 s = texsize.xy / min(texsize.x, texsize.y);
'''
def ramp(v, x):
    """Inline 6-stop theme gradient lookup: vec3 `v` from scalar expression `x`."""
    return f'''
  float {v}_x = clamp({x}, 0.0, 1.0) * 5.0;
  vec3 {v} = mix(NOKKVI_RAMP0, NOKKVI_RAMP1, clamp({v}_x, 0.0, 1.0));
  {v} = mix({v}, NOKKVI_RAMP2, clamp({v}_x - 1.0, 0.0, 1.0));
  {v} = mix({v}, NOKKVI_RAMP3, clamp({v}_x - 2.0, 0.0, 1.0));
  {v} = mix({v}, NOKKVI_RAMP4, clamp({v}_x - 3.0, 0.0, 1.0));
  {v} = mix({v}, NOKKVI_RAMP5, clamp({v}_x - 4.0, 0.0, 1.0));
'''
def tone(v, lum):
    """Background for the darkest values, the gradient through the middle,
    the theme's text colour for the brightest."""
    return ramp(v + "_r", lum) + f'''
  vec3 {v} = mix(NOKKVI_BG, {v}_r, smoothstep(0.02, 0.35, {lum}));
  {v} = mix({v}, NOKKVI_TEXT, smoothstep(0.85, 1.0, {lum}) * 0.7);
'''
def tnoise(v, x):
    """Smooth noise from the engine's 256 px noise texture at `x` (in texels
    / 256) into float `v`. A magnified texture shows its bilinear grid; snapping
    to texel centres with a quintic curve (Inigo Quilez's trick) removes it with
    a single read."""
    return f"""
  vec2 {v}_x = ({x}) * 256.0 + 0.5;
  vec2 {v}_i = floor({v}_x);
  vec2 {v}_f = fract({v}_x);
  {v}_f = {v}_f * {v}_f * {v}_f * ({v}_f * ({v}_f * 6.0 - 15.0) + 10.0);
  float {v} = texture(sampler_noise_hq, ({v}_i + {v}_f - 0.5) / 256.0).x;
"""
LUM = "vec3(0.299, 0.587, 0.114)"
# Mirror-tiled cover lookup filling any aspect: `c` in 0..1 around the centre.
def cover(v, c, sampler="sampler_fw_cover"):
    return f'''
  vec2 {v}_m = 1.0 - abs(1.0 - mod({c}, 2.0));
  vec3 {v} = texture({sampler}, vec2({v}_m.x, 1.0 - {v}_m.y)).xyz;
'''

def strip_comments(shader):
    """The engine's shader preprocessor does not understand `//` comments (the
    converted pack has none), so drop them from the generated text."""
    import re
    return "\n".join(l for l in (re.sub(r"\s*//.*$", "", l) for l in shader.split("\n")) if l.strip())

def preset(base, warp, comp, init='', frame='', waves=None):
    warp, comp = strip_comments(warp), strip_comments(comp)
    b = {"gammaadj": 1.0, "decay": 0.98, "echo_zoom": 1.0, "echo_alpha": 0.0,
         "wave_mode": 0, "additivewave": 1, "wave_a": 0.0, "wave_scale": 0.8,
         "wave_smoothing": 0.7, "zoom": 1.0, "rot": 0.0, "warp": 0.0,
         "wave_r": 1.0, "wave_g": 1.0, "wave_b": 1.0,
         "ob_size": 0.0, "ob_a": 0.0, "ib_size": 0.0, "ib_a": 0.0,
         "mv_a": 0.0, "darken_center": 0, "brighten": 0, "darken": 0, "solarize": 0, "invert": 0}
    b.update(base)
    off = {"baseVals": {"enabled": 0}, "init_eqs_eel": "", "frame_eqs_eel": "", "point_eqs_eel": ""}
    offs = {"baseVals": {"enabled": 0}, "init_eqs_eel": "", "frame_eqs_eel": ""}
    ws = list(waves or [])
    return {"version": 2, "baseVals": b, "shapes": [offs]*4, "waves": ws + [off] * (4 - len(ws)),
            "init_eqs_eel": init, "frame_eqs_eel": frame, "pixel_eqs_eel": "",
            "warp": warp, "comp": comp}

def wave_def(base, point, init='', frame=''):
    """A custom wave: the engine draws `samples` points (per-point EEL sets
    x, y in 0..1 around the centre and r, g, b, a) into the feedback texture
    after the warp, so the next frame's warp carries them. `value1` / `value2`
    are the left / right waveform samples, already scaled. With `additive`
    each channel adds `channel * a`, so r, g, b address the warp's own channel
    semantics rather than screen colours."""
    b = {"enabled": 1, "samples": 512, "sep": 0, "spectrum": 0, "usedots": 0, "thick": 1,
         "additive": 1, "scaling": 1.0, "smoothing": 0.5, "r": 1.0, "g": 1.0, "b": 1.0, "a": 1.0}
    b.update(base)
    return {"baseVals": b, "init_eqs_eel": init, "frame_eqs_eel": frame, "point_eqs_eel": point}

WAVE_THEME = "wave_r = NOKKVI_HIGHLIGHT_R;\nwave_g = NOKKVI_HIGHLIGHT_G;\nwave_b = NOKKVI_HIGHLIGHT_B;\n"
# Beat envelope in q3: jumps on a kick, decays over ~0.3 s.
# q3: a kick envelope (bass jumping above its follower), ~0.3 s.
# q5: an instant pop from the raw bass level, ~0.15 s: brightness flashes and
#     punches hit on the beat instead of easing in.
PULSE = ("kick = max(bass - bass_att, 0);\npulse = max(pulse * 0.86, min(kick * 0.9, 1));\nq3 = pulse;\n"
         "pop = max(pop * 0.78, min(max(bass - 1.2, 0) * 0.55, 1.4));\nq5 = pop;\n")

presets = {}

# 1. Tunnel ---------------------------------------------------------------
# Flying down a tunnel papered with the cover. The walls are lit as relief
# (the cover's luminance embossed, a headlight from the near end) and fade
# into the theme's background with depth. The feedback is not the screen
# here: it is the tunnel unwrapped (x = angle, y = depth, far at the top),
# and every frame slides it towards the viewer by the distance flown. On
# each kick a custom wave draws the waveform as a wobbly line across the far
# row, which the comp wraps around the walls: a squiggly hoop that flies
# past, lighting the wall around it. (The previous preset's picture is wiped
# from the buffer on the first frames, since it would read as hoops.)
# Seams: the walls tile the cover MIRRORED in both directions, so no cover
# edge ever meets its opposite edge. atan's jump behind the viewer would
# still make the GPU pick the smallest mip along one ray (a line), so every
# angular lookup is done twice, once with the jump on the left and once on
# the right, and blended by side: each copy's jump sits where its weight is 0.
def tunnel_wall(sfx, ang):
    return f"""
  vec2 t{sfx} = vec2({ang} / 6.2831853 * 2.0 + q2 + 0.06 * dep, dep + q1);
  vec2 m{sfx} = 1.0 - abs(1.0 - mod(t{sfx}, 2.0));
  vec2 ma{sfx} = 1.0 - abs(1.0 - mod(t{sfx} + vec2(0.012, 0.0), 2.0));
  vec2 md{sfx} = 1.0 - abs(1.0 - mod(t{sfx} + vec2(0.0, 0.02), 2.0));
  vec3 w{sfx} = vec3(dot(texture(sampler_fw_cover, vec2(m{sfx}.x, 1.0 - m{sfx}.y)).xyz, {LUM}),
                     dot(texture(sampler_fw_cover, vec2(ma{sfx}.x, 1.0 - ma{sfx}.y)).xyz, {LUM}),
                     dot(texture(sampler_fw_cover, vec2(md{sfx}.x, 1.0 - md{sfx}.y)).xyz, {LUM}));
"""
TUNNEL_V = "2.0"
TUNNEL_WAVE = wave_def(
    {"samples": 400, "scaling": 0.6, "smoothing": 0.5, "r": 1.0, "g": 0.0, "b": 0.0, "a": 0.9},
    "w = min(1, 8 * min(sample, 1 - sample));\n"
    "x = 0.5 + (sample - 0.5) / aspectx;\n"
    "y = 0.5 - (0.42 - value1 * 0.35 * w) / aspecty;\n",
    frame="a = 0.9 * q9;")
presets["nokkvi - cover tunnel"] = preset(
    {"zoom": 1.0, "rot": 0.0, "decay": 1.0, "wave_a": 0.0},
    " shader_body {\n" + HEAD + f"""
  vec2 src = vec2(uv_orig.x, uv_orig.y + q6 / {TUNNEL_V});
  float ink = texture(sampler_main, src).x * 0.995 * step(2.5, frame) * step(src.y, 1.0);
  ret = vec3(ink, 0.0, 0.0);
 }}""",
    "uniform sampler2D sampler_fw_cover;\n shader_body {\n" + HEAD + f"""
  vec2 p = (uv - 0.5) * s;
  p += vec2(0.07 * sin(time * 0.23), 0.06 * cos(time * 0.19));
  float r0 = max(length(p), 0.002);
  float a = atan(p.y, p.x);
  float r = r0 * (1.0 + 0.07 * sin(a * 3.0 + q1 * 1.7) * clamp(mid_att, 0.0, 2.0));
  r *= 1.0 + 0.10 * q3 + 0.08 * q5;
  float dep = 0.26 / r;
  float aR = atan(-p.y, -p.x) + 3.14159265;
  float side = smoothstep(0.03, -0.03, p.x);
""" + tunnel_wall("L", "a") + tunnel_wall("R", "aR") + f"""
  vec3 wall = mix(wL, wR, side);
  float cl = wall.x;
  float ca = wall.y;
  float cd = wall.z;
  vec3 wn = normalize(vec3((cl - ca) * 5.0, (cl - cd) * 5.0, 1.0));
  vec3 wl = normalize(vec3(0.3, -0.7, 0.65));
  float shade = 0.45 + 0.75 * max(dot(wn, wl), 0.0);
  float lum = clamp((cl - 0.5) * 1.6 + 0.5, 0.0, 1.0);
""" + tone("col", "lum") + f"""
  col *= shade;
  float band = pow(abs(sin((dep + q1) * 4.7123889)), 28.0);
  col += mix(NOKKVI_HIGHLIGHT, NOKKVI_TEXT, q3 * 0.5) * band * (0.2 + 0.7 * q3 + 0.6 * q5);
  col += NOKKVI_WARM * band * clamp(treb_att - 1.0, 0.0, 1.0) * 0.5;
  float fog = smoothstep(0.03, 0.30, r0);
  float fogf = fog * fog;
  col = mix(NOKKVI_BG, col, fogf);
  vec2 hu = vec2(a / 6.2831853 + 0.5, dep / {TUNNEL_V});
  vec2 huR = vec2(aR / 6.2831853 - 0.5, hu.y);
  float hoop = mix(texture(sampler_fw_main, hu).x, texture(sampler_fw_main, huR).x, side) * step(hu.y, 1.0);
  col += NOKKVI_ACCENT * GetBlur1(hu).x * step(hu.y, 1.0) * 1.5 * fogf;
  col += mix(NOKKVI_HIGHLIGHT, NOKKVI_TEXT, hoop) * hoop * (2.0 + 0.8 * q3) * fogf;
  col += NOKKVI_ACCENT * smoothstep(0.035, 0.0, abs(r0 - 0.07 - 0.05 * q3 - 0.04 * q5)) * (0.3 + 0.7 * q3 + 0.6 * q5);
  col *= 1.0 + 0.4 * q5;
  col *= 0.85 + 0.15 * smoothstep(0.95, 0.3, r0);
  ret = col;
 }}""",
    init="depth = 0; twist = 0; pulse = 0; pop = 0; cool = 0;",
    frame=PULSE + "dd = 0.005 + 0.012 * min(bass_att, 2.5) + 0.03 * min(kick, 1.5) + 0.02 * pop;\n"
          "depth = depth + dd;\n"
          "twist = twist + 0.0015 + 0.003 * (mid_att - 1);\n"
          "cool = max(cool - 1 / max(fps, 1), 0);\n"
          "stamp = above(kick, 0.2) * below(cool, 0.001);\n"
          "cool = if(stamp, 0.18, cool);\n"
          "q1 = depth;\nq2 = twist;\nq6 = dd;\nq9 = stamp;",
    waves=[TUNNEL_WAVE])

# 2. Halo -----------------------------------------------------------------
# The cover as a glossy card leaning in a dark room: a slight perspective
# wobble (more on kicks), a sheen sliding across it, a soft drop shadow, and
# a bright frame that pulses with the bass. Light streams out from behind the
# card: the warp feeds the card's rim colours into the feedback and zooms it
# outward every frame, with a per-angle gain from streak noise and the bands
# (bass above and below, treble at the sides), so the halo is made of god
# rays that lengthen with the music; dust twinkles where the rays are bright.
halo_box = "float box = 0.30 + 0.02 * clamp(bass_att, 0.0, 2.0) + 0.03 * q3 + 0.035 * q5;"
HALO_TILT = """
  float tx = 0.10 * sin(time * 0.37) + 0.07 * q3;
  float ty = 0.08 * cos(time * 0.29) - 0.05 * q3;
"""
presets["nokkvi - cover halo"] = preset(
    {"zoom": 1.0, "rot": 0.0, "warp": 0.0, "decay": 1.0, "wave_mode": 0, "wave_a": 0.3, "wave_scale": 0.6},
    "uniform sampler2D sampler_fc_cover;\n shader_body {\n" + HEAD + f"""
  {halo_box}
""" + HALO_TILT + """
  vec2 p = (uv_orig - 0.5) * s;
  vec2 q = p / (1.0 + p.x * tx + p.y * ty);
  float d = max(abs(q.x), abs(q.y));
  vec3 cov = texture(sampler_fc_cover, vec2(0.5, 0.5) + vec2(1.0, -1.0) * q / (2.0 * box)).xyz;
  float rim = (1.0 - step(box, d)) * smoothstep(box - 0.05, box, d);
  float ang = atan(p.y, p.x);
  float streak = texture(sampler_noise_hq, vec2(ang / 6.2831853 * 3.0 + time * 0.004, 0.37)).x;
  float band = abs(sin(ang));
  float spec = mix(clamp(treb_att, 0.0, 2.0), clamp(bass_att, 0.0, 2.0), band);
  vec2 src = 0.5 + (uv_orig - 0.5) / (1.025 + 0.02 * q3);
  vec3 fb = texture(sampler_main, src).xyz * (0.935 + 0.035 * streak + 0.02 * clamp(spec - 0.9, 0.0, 1.0)) - 0.002;
  fb += cov * rim * (0.22 + 0.5 * q3 + 0.45 * q5);
  ret = max(fb, vec3(0.0));
 }""",
    "uniform sampler2D sampler_fc_cover;\n shader_body {\n" + HEAD + f"""
  {halo_box}
""" + HALO_TILT + f"""
  vec2 p = (uv - 0.5) * s;
  vec2 q = p / (1.0 + p.x * tx + p.y * ty);
  float d = max(abs(q.x), abs(q.y));
  vec3 fb = texture(sampler_main, uv).xyz + GetBlur1(uv) * 0.8;
  float lum = clamp(dot(fb, {LUM}) * 1.3, 0.0, 1.0);
""" + tone("col", "lum") + """
  col += NOKKVI_WARM * smoothstep(0.75, 1.0, lum) * clamp(treb_att - 0.8, 0.0, 1.0) * 0.35;
  vec2 g = uv * texsize.xy / 7.0;
  vec2 id = floor(g);
  vec2 f = fract(g) - 0.5;
  float h = fract(sin(dot(id, vec2(127.1, 311.7))) * 43758.5453);
  float h2 = fract(h * 91.3);
  float dust = step(0.975, h) * smoothstep(0.3, 0.0, length(f - (vec2(h2, fract(h2 * 7.0)) - 0.5) * 0.4));
  dust *= 0.5 + 0.5 * sin(time * (2.0 + 4.0 * h2) + h * 50.0);
  col += NOKKVI_TEXT * dust * (0.15 + lum) * 0.7;
  col *= 1.0 + 0.4 * q5;
  float dsh = max(abs(q.x - 0.03), abs(q.y + 0.035));
  float shadow = smoothstep(box + 0.07, box - 0.005, dsh) * 0.55;
  col *= 1.0 - shadow;
  vec3 cov = texture(sampler_fc_cover, vec2(0.5, 0.5) + vec2(1.0, -1.0) * q / (2.0 * box)).xyz;
  float inside = 1.0 - smoothstep(box - 0.003, box, d);
  float sheen = pow(clamp(1.0 - abs(q.x * 0.7 + q.y * 0.5 + 0.15 * sin(time * 0.37)) * 3.0, 0.0, 1.0), 3.0) * 0.12;
  col = mix(col, cov + NOKKVI_TEXT * sheen, inside);
  float frame_line = smoothstep(0.010, 0.0, abs(d - box));
  col += NOKKVI_HIGHLIGHT * frame_line * (0.35 + 0.65 * max(q3, clamp(bass_att - 0.8, 0.0, 1.0)) + 0.9 * q5);
  col += NOKKVI_HIGHLIGHT * smoothstep(0.06, 0.0, d - box) * step(box, d) * (0.15 + 0.35 * q3);
  ret = col;
 }""",
    init="pulse = 0; pop = 0;",
    frame=PULSE + WAVE_THEME)

# 3. Kaleido --------------------------------------------------------------
# An endless kaleidoscope dive (FractalDrop's idea): each frame the feedback
# is folded and zoomed into the centre, and a sharpen against its own blur
# keeps growing fine detail, so the pattern recurses into itself forever.
# The folded cover is stirred in faintly as the raw material; kicks punch the
# zoom and spin; a faint Lissajous scribble of the stereo waveform is drawn in
# every frame and leaves a tint that spreads through the recursion. The fold
# count steps through 6, 8, 5 and 4 every twelve kicks. The comp shows it as
# stained glass: the theme's gradient lit by a lamp wandering behind it,
# dark lead lines along the edges and the mirror seams, glints on the lead.
def kfold(v, pexpr, zexpr, folds="q6"):
    return f"""
  vec2 {v}_p = {pexpr};
  float {v}_r = length({v}_p);
  float {v}_seg = 6.2831853 / {folds};
  float {v}_a = mod(atan({v}_p.y, {v}_p.x) + q1, {v}_seg);
  {v}_a = abs({v}_a - {v}_seg * 0.5);
  vec2 {v} = vec2(cos({v}_a), sin({v}_a)) * {v}_r / ({zexpr});
"""
KAL_WAVE = wave_def(
    {"samples": 256, "scaling": 1.0, "smoothing": 0.3, "r": 0.9, "g": 1.0, "b": 0.0, "a": 0.12},
    "x = 0.5 + value1 * 1.3;\n"
    "y = 0.5 + value2 * 1.3;\n",
    frame="a = 0.1 + 0.6 * q9;")
presets["nokkvi - cover kaleido"] = preset(
    {"decay": 1.0, "zoom": 1.0, "wave_a": 0.0},
    "uniform sampler2D sampler_fw_cover;\n shader_body {\n" + HEAD + f"""
  vec2 p = (uv_orig - 0.5) * s;
  float zm = 1.012 + 0.02 * q2 + 0.035 * q5;
  float rt = 0.004 + 0.012 * q3;
  vec2 c = vec2(p.x * cos(rt) - p.y * sin(rt), p.x * sin(rt) + p.y * cos(rt)) / zm;
  vec2 src = c / s + 0.5;
  vec3 m = texture(sampler_main, src).xyz;
  vec3 b1 = GetBlur1(src);
  vec3 b2 = GetBlur2(src);
  float u = m.x + (m.x - b1.x) * 0.5 + (m.x - b2.x) * 0.2;
  u += (texture(sampler_noise_lq, uv_orig * texsize.xy * 0.4 / 256.0 + rand_frame.xy).x - 0.5) * 0.18;
  vec2 cc = p * 0.9 + vec2(0.5, 0.5) + 0.2 * vec2(cos(q4), sin(q4 * 0.7));
""" + cover("cov", "cc") + f"""
  float cl = dot(cov, {LUM});
  float seed = smoothstep(0.22, 0.05, length(p));
  u = mix(u, cl, seed * (0.12 + 0.2 * q5));
  u = clamp(u * 0.93 + 0.035, 0.0, 1.0);
  float tint = max(m.y, b1.y * 0.9) * 0.985;
  ret = vec3(u, tint, 0.0);
 }}""",
    " shader_body {\n" + HEAD + kfold("k", "(uv - 0.5) * s", "1.0") + """
  vec2 ku = k / s + 0.5;
  vec2 px = texsize.zw * 2.0;
  float hx = GetBlur1(ku + vec2(px.x, 0.0)).x - GetBlur1(ku - vec2(px.x, 0.0)).x;
  float hy = GetBlur1(ku + vec2(0.0, px.y)).x - GetBlur1(ku - vec2(0.0, px.y)).x;
  float edge = smoothstep(0.015, 0.09, length(vec2(hx, hy)));
  vec3 n = normalize(vec3(-hx * 4.0, -hy * 4.0, 1.0));
  vec3 L = normalize(vec3(cos(q4), sin(q4), 0.8));
  float spc = pow(max(dot(reflect(-L, n), vec3(0.0, 0.0, 1.0)), 0.0), 24.0);
  vec3 m = texture(sampler_main, ku).xyz;
  float lum = m.x;
  float hue = clamp(lum * 0.55 + k_r * 0.35 + m.y * 0.4, 0.0, 1.0);
""" + ramp("glass", "hue") + """
  vec2 p = (uv - 0.5) * s;
  vec2 lp = 0.35 * vec2(cos(q4 * 0.7), sin(q4 * 0.5));
  float lamp = 0.55 + 0.7 * exp(-dot(p - lp, p - lp) * 2.5);
  float transmit = 0.18 + 0.82 * smoothstep(0.1, 0.9, lum);
  vec3 col = glass * transmit * lamp;
  col += glass * GetBlur2(ku).x * 0.3;
  float seam = smoothstep(0.012, 0.0, min(k_a, k_seg * 0.5 - k_a) * k_r) * step(0.03, k_r);
  float lead = max(edge * 0.85, seam * 0.7);
  col *= 1.0 - lead;
  col += NOKKVI_TEXT * spc * (edge * 0.5 + 0.1) * (0.4 + 0.5 * q5);
  col += NOKKVI_HIGHLIGHT * smoothstep(0.012, 0.0, abs(k_r - 0.08 - 0.3 * q3)) * q3 * 0.6;
  col += NOKKVI_WARM * smoothstep(0.85, 1.0, lum) * clamp(treb_att - 0.9, 0.0, 1.0) * 0.3;
  col *= 0.75 + 0.25 * smoothstep(0.95, 0.15, k_r);
  col *= 1.0 + 0.3 * q5;
  ret = col;
 }""",
    init="phase = 0; orbit = 0; pulse = 0; pop = 0; speed = 0; cool = 0; kicks = 0; q6 = 6;",
    frame=PULSE + "phase = phase + 0.002 + 0.004 * min(bass_att, 2) + 0.01 * q3 + 0.012 * pop;\norbit = orbit + 0.0015 + 0.002 * min(mid_att, 2);\n"
          "speed = speed * 0.9 + 0.1 * (0.2 + 0.4 * min(bass_att, 2) + 1.2 * q3);\n"
          "cool = max(cool - 1 / max(fps, 1), 0);\n"
          "stamp = above(kick, 0.2) * below(cool, 0.001);\n"
          "cool = if(stamp, 0.18, cool);\n"
          "kicks = kicks + stamp;\n"
          "act = floor(kicks / 12) % 4;\n"
          "q6 = if(equal(act, 0), 6, if(equal(act, 1), 8, if(equal(act, 2), 5, 4)));\n"
          "q1 = phase;\nq2 = speed;\nq4 = orbit;\nq9 = stamp;",
    waves=[KAL_WAVE])

# 4. Ripple (new) ---------------------------------------------------------
# Rain: four small, tighter ripples that keep falling at random spots, more
# often when the treble is busy, between the kicks' big ones. The light
# wanders slowly, so the glints travel across the surface.
def rain_slot(i, qx, qy, qa):
    return (f"a{i} = a{i} + dt;\n"
            f"sp{i} = above(a{i}, l{i}) * above(25 + 55 * min(treb_att, 1.5), rand(100));\n"
            f"a{i} = if(sp{i}, 0, a{i});\n"
            f"l{i} = if(sp{i}, 0.9 + rand(100) / 100, l{i});\n"
            f"x{i} = if(sp{i}, (rand(1000) / 1000 - 0.5) * 0.95, x{i});\n"
            f"y{i} = if(sp{i}, (rand(1000) / 1000 - 0.5) * 0.75, y{i});\n"
            f"q{qx} = x{i}; q{qy} = y{i}; q{qa} = a{i};\n")
RAIN = [(4, 14, 15, 16), (5, 17, 18, 19), (6, 20, 21, 22), (7, 23, 24, 25)]
ripple_frame = PULSE + '''dt = 1 / max(fps, 1);
a1 = a1 + dt; a2 = a2 + dt; a3 = a3 + dt;
cool = max(cool - dt, 0);
spawn = above(kick, 0.22) * below(cool, 0.001);
slot = if(spawn, slot + 1 - 3 * above(slot, 1.5), slot);
cool = if(spawn, 0.22, cool);
nx = (rand(1000) / 1000 - 0.5) * 0.9;
ny = (rand(1000) / 1000 - 0.5) * 0.7;
x1 = if(spawn * equal(slot, 0), nx, x1); y1 = if(spawn * equal(slot, 0), ny, y1); a1 = if(spawn * equal(slot, 0), 0, a1);
x2 = if(spawn * equal(slot, 1), nx, x2); y2 = if(spawn * equal(slot, 1), ny, y2); a2 = if(spawn * equal(slot, 1), 0, a2);
x3 = if(spawn * equal(slot, 2), nx, x3); y3 = if(spawn * equal(slot, 2), ny, y3); a3 = if(spawn * equal(slot, 2), 0, a3);
drift = drift + 0.0006 + 0.001 * min(mid_att, 2);
q4 = x1; q5 = y1; q6 = a1;
q7 = x2; q8 = y2; q9 = a2;
q10 = x3; q11 = y3; q12 = a3;
q13 = drift;
lightt = lightt + dt * 0.15;
q26 = lightt;
''' + "".join(rain_slot(i, qx, qy, qa) for i, qx, qy, qa in RAIN)
def wave(i, x, y, a, amp="1.0", freq="38.0"):
    return f'''
  vec2 d{i} = p - vec2({x}, {y});
  float l{i} = max(length(d{i}), 0.0001);
  float f{i} = l{i} - {a} * 0.45;
  float w{i} = sin(f{i} * {freq}) * exp(-abs(f{i}) * 7.0) * exp(-{a} * 1.1) * {amp};
  off += d{i} / l{i} * w{i};
  crest = max(crest, w{i});
'''
presets["nokkvi - cover ripple"] = preset(
    {"decay": 0.9, "zoom": 1.0, "wave_a": 0.0},
    "",
    "uniform sampler2D sampler_fw_cover;\n shader_body {\n" + HEAD + '''
  vec2 p = (uv - 0.5) * s;
  vec2 off = vec2(0.0);
  float crest = 0.0;
''' + wave(1, "q4", "q5", "q6") + wave(2, "q7", "q8", "q9") + wave(3, "q10", "q11", "q12")
    + "".join(wave(i, f"q{qx}", f"q{qy}", f"q{qa}", "0.4", "58.0") for i, qx, qy, qa in RAIN) + '''
  vec3 LUMV = vec3(0.299, 0.587, 0.114);
  vec2 c = p * 0.62 + off * 0.055 * (1.0 + 0.8 * q5) + vec2(0.5 + 0.12 * sin(q13 * 2.0), 0.5 + 0.12 * cos(q13 * 1.3));
''' + cover("cov", "c") + f'''
  float lum = clamp((dot(cov, {LUM}) - 0.5) * 1.5 + 0.5, 0.0, 1.0);
''' + tone("col", "lum") + '''
  vec2 ce = texsize.zw * 1.5;
  float cx = dot(texture(sampler_fw_cover, vec2(1.0 - abs(1.0 - mod(c.x + ce.x, 2.0)), 1.0 - cov_m.y)).xyz, LUMV)
           - dot(texture(sampler_fw_cover, vec2(1.0 - abs(1.0 - mod(c.x - ce.x, 2.0)), 1.0 - cov_m.y)).xyz, LUMV);
  float cy = dot(texture(sampler_fw_cover, vec2(cov_m.x, 1.0 - (1.0 - abs(1.0 - mod(c.y + ce.y, 2.0))))).xyz, LUMV)
           - dot(texture(sampler_fw_cover, vec2(cov_m.x, 1.0 - (1.0 - abs(1.0 - mod(c.y - ce.y, 2.0))))).xyz, LUMV);
  vec3 wn = normalize(vec3(-off * 1.6 - vec2(cx, cy) * 1.6, 1.0));
  vec3 wl = normalize(vec3(-0.45 * cos(q26), 0.3 + 0.35 * sin(q26 * 0.7), 0.75));
  float shade = max(dot(wn, wl), 0.0) / wl.z;
  float glint = pow(max(dot(reflect(-wl, wn), vec3(0.0, 0.0, 1.0)), 0.0), 60.0);
  col *= mix(1.0, shade, 0.7);
  col = mix(col, NOKKVI_HIGHLIGHT, clamp(crest, 0.0, 1.0) * 0.35);
  col += NOKKVI_TEXT * glint * (0.5 + 0.6 * q5);
  col += NOKKVI_TEXT * pow(clamp(crest, 0.0, 1.0), 3.0) * (0.3 + 0.4 * q5);
  col = mix(col, NOKKVI_BG, clamp(-crest, 0.0, 1.0) * 0.35);
  col *= 0.8 + 0.2 * smoothstep(1.0, 0.3, length(p));
  col *= 1.0 + 0.3 * q5;
  ret = col;
 }''',
    init="pulse = 0; pop = 0; a1 = 9; a2 = 9; a3 = 9; slot = 0; cool = 0; drift = 0; x1 = 0; y1 = 0; x2 = 0; y2 = 0; x3 = 0; y3 = 0; "
         "lightt = 0; a4 = 9; a5 = 9; a6 = 9; a7 = 9; l4 = 0; l5 = 0.3; l6 = 0.6; l7 = 0.9; x4 = 0; y4 = 0; x5 = 0; y5 = 0; x6 = 0; y6 = 0; x7 = 0; y7 = 0;",
    frame=ripple_frame)

# Orb: the cover on a spinning, lit sphere resting on a dark glossy floor
# that mirrors it through a ripple. The sphere sheds its colours as paint: a
# thin rim feeds the feedback, which curls away through a noise flow field
# (kept crisp, brushed by fine noise), and the sphere's edge melts into it.
# A ring of the waveform orbits the sphere in the theme's highlight colour
# and is carried off into the paint (brighter on kicks). Kicks shed more
# paint and swell the orb.
orb_r = "float R = 0.26 + 0.03 * q3 + 0.035 * q5 + 0.01 * clamp(bass_att, 0.0, 2.0);"
def orb_sphere(p="p", d2="d2"):
    return f"""
    vec3 n = vec3({p}.x, -{p}.y, sqrt(max(R * R - {d2}, 0.0))) / R;
    float ct = cos(0.35); float st = sin(0.35);
    vec3 nt = vec3(n.x, n.y * ct - n.z * st, n.y * st + n.z * ct);
    float cs = cos(q1); float sn = sin(q1);
    vec3 m = vec3(nt.x * cs + nt.z * sn, nt.y, -nt.x * sn + nt.z * cs);
    float lon = atan(m.x, m.z);
    float lat = asin(clamp(m.y, -1.0, 1.0));
    vec3 cov = texture(sampler_fw_cover, vec2(lon / 3.14159265 + 0.5, 0.5 + lat / 3.14159265)).xyz;
"""
ORB_LIT = """
    vec3 L = normalize(vec3(-0.45, 0.55, 0.75));
    float diff = max(dot(n, L), 0.0);
    float spec = pow(max(dot(reflect(-L, n), vec3(0.0, 0.0, 1.0)), 0.0), 28.0);
    float rim = pow(1.0 - n.z, 3.0);
    vec3 lit = cov * (0.25 + 0.85 * diff) + NOKKVI_TEXT * spec * 0.5;
    lit += NOKKVI_HIGHLIGHT * rim * (0.35 + 0.6 * q3 + 0.8 * q5);
"""
ORB_WAVE = wave_def(
    {"samples": 300, "scaling": 0.7, "smoothing": 0.4, "a": 0.3},
    "w = min(1, 8 * min(sample, 1 - sample));\n"
    "ang = sample * 6.2831853 + q2 * 0.3;\n"
    "rad = (0.26 + 0.03 * q3 + 0.035 * q5 + 0.01 * min(bass_att, 2)) * 1.25 + value1 * 0.5 * w;\n"
    "x = 0.5 + rad * cos(ang);\n"
    "y = 0.5 + rad * 0.35 * sin(ang) + value2 * 0.2 * w;\n"
    "r = NOKKVI_HIGHLIGHT_R; g = NOKKVI_HIGHLIGHT_G; b = NOKKVI_HIGHLIGHT_B;\n",
    frame="a = 0.22 + 0.6 * q9;")
presets["nokkvi - cover orb"] = preset(
    {"zoom": 1.0, "rot": 0.0, "decay": 1.0},
    "uniform sampler2D sampler_fw_cover;\n shader_body {\n" + HEAD + f"""
  {orb_r}
  vec2 p = (uv_orig - 0.5) * s;
  float d = length(p);
  vec2 nuv = uv_orig * 0.45 + vec2(q2 * 0.02, -q2 * 0.013);
  float flow_ang = (texture(sampler_noise_hq, nuv).x + 0.5 * texture(sampler_noise_hq, nuv * 2.1 + 0.3).x) * 9.0;
  vec2 flow = vec2(cos(flow_ang), sin(flow_ang)) * (0.0026 + 0.0026 * clamp(mid_att, 0.0, 2.0));
  vec2 swirl = vec2(-p.y, p.x) / max(d, 0.08) * 0.0018;
  vec2 outward = p / max(d, 0.05) * (0.0008 + 0.004 * q3 + 0.006 * q5);
  vec2 src = uv - (flow + swirl + outward) / s;
  vec3 fb = texture(sampler_main, src).xyz;
  fb = mix(fb, GetBlur1(src), 0.05);
  fb *= 0.985 + 0.03 * texture(sampler_noise_lq, uv_orig * texsize.xy / 256.0 * 0.5 + q2 * 0.01).x;
  fb = fb * 0.992 - 0.001;
  float d2 = dot(p, p);
  float band = smoothstep(0.05, 0.0, abs(d - R * 0.96));
  if (d < R) {{
""" + orb_sphere() + f"""
    fb = mix(fb, cov, band * (0.12 + 0.4 * q3 + 0.5 * q5));
  }}
  ret = clamp(fb, 0.0, 1.0);
 }}""",
    "uniform sampler2D sampler_fw_cover;\n shader_body {\n" + HEAD + f"""
  {orb_r}
  vec2 p = (uv - 0.5) * s;
  float d2 = dot(p, p);
  float d = sqrt(d2);
  vec3 paint = texture(sampler_main, uv).xyz;
  vec3 halo = GetBlur2(uv);
  float pl = dot(paint, {LUM});
  vec3 col = mix(NOKKVI_BG, paint, smoothstep(0.0, 0.25, pl + 0.15));
  col += NOKKVI_HIGHLIGHT * dot(halo, {LUM}) * 0.25;
  float floor_y = -0.33;
  if (p.y < floor_y) {{
    float ripple = (texture(sampler_noise_hq, vec2(uv.x * 1.5, uv.y * 10.0 - q2 * 0.02)).x - 0.5) * 0.02;
    vec2 pm = vec2(p.x + ripple, 2.0 * floor_y - p.y);
    vec2 um = pm / s + 0.5;
    vec3 rpaint = texture(sampler_main, um).xyz;
    vec3 rcol = mix(NOKKVI_BG, rpaint, smoothstep(0.0, 0.25, dot(rpaint, {LUM}) + 0.15));
    float d2m = dot(pm, pm);
    if (d2m < R * R) {{
""" + orb_sphere("pm", "d2m") + ORB_LIT + f"""
      rcol = mix(rcol, lit, smoothstep(R, R - 0.02, sqrt(d2m)));
    }}
    float fade = smoothstep(floor_y - 0.3, floor_y, p.y);
    col = mix(NOKKVI_BG * 0.6, rcol * 0.5, fade);
  }}
  float wob = (texture(sampler_noise_hq, p * 0.7 + q2 * 0.006).x - 0.5) * 0.022;
  float Rw = R + wob;
  if (d < Rw) {{
""" + orb_sphere() + ORB_LIT + f"""
    float edge = smoothstep(Rw, Rw - 0.02, d);
    col = mix(col, lit, edge);
  }}
  col += NOKKVI_WARM * smoothstep(0.02, 0.0, abs(d - R)) * clamp(treb_att - 1.0, 0.0, 1.0) * 0.3;
  ret = col;
 }}""",
    init="pulse = 0; pop = 0; spin = 0; drift = 0; cool = 0;",
    frame=PULSE + "spin = spin + 0.005 + 0.006 * min(mid_att, 2) + 0.02 * q3;\n"
          "drift = drift + 0.01 + 0.02 * min(bass_att, 2);\n"
          "cool = max(cool - 1 / max(fps, 1), 0);\n"
          "stamp = above(kick, 0.2) * below(cool, 0.001);\n"
          "cool = if(stamp, 0.18, cool);\n"
          "q1 = spin;\nq2 = drift;\nq9 = stamp;",
    waves=[ORB_WAVE])

# Starfield: 3D star layers flown through with parallax, drawn into the
# feedback so every star leaves a motion trail that zooms outward (longer at
# speed); each star is tied to a frequency and flares with it; kicks surge
# the speed. (A screen-space nebula read as a smudge on the lens; removed.)
STAR_LAYERS = 6
def star_layer(l):
    return f"""
  {{
    float z = fract({l}.0 / {STAR_LAYERS}.0 + q1);
    float scale = mix(26.0, 0.6, z);
    float fade = smoothstep(0.0, 0.25, z) * smoothstep(1.0, 0.85, z);
    float tw = q11 * (1.0 - z) * 2.2;
    vec2 prt = vec2(pr.x * cos(tw) - pr.y * sin(tw), pr.x * sin(tw) + pr.y * cos(tw));
    vec2 g = prt * scale + vec2({l * 37.1:.1f}, {l * 91.7:.1f});
    vec2 id = floor(g);
    vec2 f = fract(g) - 0.5;
    float h = fract(sin(dot(id, vec2(127.1, 311.7))) * 43758.5453);
    float h2 = fract(h * 91.3);
    vec2 d = f - (vec2(h, h2) - 0.5) * 0.5;
    float size = mix(0.012, 0.04, h2) * (0.6 + z);
    float dist = length(d);
    float core = smoothstep(size, 0.0, dist);
    float glow = size * 0.5 / (dist + 0.02) * smoothstep(0.2, 0.0, dist);
    float twinkle = 0.75 + 0.25 * sin(time * (3.0 + h * 5.0) + h * 40.0);
    float spec = get_fft(0.02 + h2 * 0.6);
    float b = (core * 1.6 + glow * 0.5) * fade * twinkle * step(0.55, h) * (0.45 + 2.4 * spec);
""" + ramp(f"sc{l}", "h2") + f"""
    stars += mix(sc{l}, NOKKVI_TEXT, core * 0.7) * b;
  }}
"""
presets["nokkvi - starfield"] = preset(
    {"decay": 1.0, "wave_a": 0.0, "zoom": 1.0},
    " shader_body {\n" + HEAD + """
  vec2 p = (uv_orig - 0.5) * s;
  float cr = cos(q4); float sr = sin(q4);
  vec2 pr = vec2(p.x * cr - p.y * sr, p.x * sr + p.y * cr);
  vec3 stars = vec3(0.0);
""" + "".join(star_layer(l) for l in range(STAR_LAYERS)) + """
  // One frame of flight: spin by `sw` (more at the edge, faster with speed)
  // and zoom in. The second sample sits halfway along that same spiral, so
  // trails curve smoothly instead of beading or fanning.
  vec2 cs = (uv - 0.5) * s;
  float sw = clamp(q10 / 0.02, -1.0, 1.0) * (0.004 + 0.02 * q2) * (0.4 + 1.1 * length(cs));
  float zm = 1.0 + 0.012 + q2 * 0.05;
  vec2 c1 = vec2(cs.x * cos(sw) - cs.y * sin(sw), cs.x * sin(sw) + cs.y * cos(sw));
  vec2 back = c1 / s / zm + 0.5;
  float sh = sw * 0.5;
  vec2 c2 = vec2(cs.x * cos(sh) - cs.y * sin(sh), cs.x * sin(sh) + cs.y * cos(sh));
  vec2 half_back = c2 / s / sqrt(zm) + 0.5;
  float keep = clamp(0.8 + q2 * 0.6, 0.8, 0.95);
  vec3 fb = max(texture(sampler_main, back).xyz, texture(sampler_main, half_back).xyz * 0.92) * keep;
  ret = max(fb, stars);
 }""",
    " shader_body {\n" + HEAD + """
  vec2 p = (uv - 0.5) * s;
  float grain = texture(sampler_noise_lq, uv * texsize.xy / 256.0 + rand_frame.xy).x;
  vec3 col = NOKKVI_BG;
  col += texture(sampler_main, uv).xyz + GetBlur1(uv) * 0.2;
  col += NOKKVI_ACCENT * exp(-length(p) * 6.0) * (0.08 + 0.45 * q3);
  col += NOKKVI_WARM * exp(-length(p) * 14.0) * q3 * 0.35;
  col *= 0.9 + 0.1 * smoothstep(1.1, 0.2, length(p));
  col += (grain - 0.5) * 0.012;
  ret = col;
 }""",
    init="pulse = 0; pop = 0; travel = 0; roll = 0; speed = 0; twist = 0.015; spiral = 0;",
    frame=PULSE + "speed = speed * 0.9 + 0.1 * (0.012 + 0.03 * min(bass_att, 2) + 0.12 * q3 + 0.1 * pop);\n"
          "travel = travel + speed * 0.25;\nroll = roll + 0.0012 * (mid_att - 0.8) + 0.004 * q3 * sign(sin(time * 0.05));\n"
          "q1 = travel;\nq2 = speed * 6;\nq4 = roll;\n"
          "twist = twist * 0.97 + 0.03 * (0.02 * sin(time * 0.11) + 0.012 * (mid_att - 1) + 0.03 * pop * sign(sin(time * 0.11)));\n"
          "spiral = spiral + twist * 60 * speed;\nq10 = twist;\nq11 = spiral;")

# Living ink: a self-organising reaction-diffusion surface (difference of
# blurs, flexi's trick) that creeps along its own gradient and a slow current,
# fed by the music (the live spectrum seeds a ring, each frequency at its own
# angle; kicks send shockwaves), shown as glossy lit enamel through the
# theme's gradient. Every 16 kicks the scene shifts: pattern scale, flow
# direction and light angle. Theme colours only; no cover.
# A faint Lissajous scribble of the stereo waveform, wandering slowly, is
# drawn into the ink every frame (stronger on kicks), so the pattern grows
# from the music's own shape rather than only from the spectrum ring.
INK_WAVE = wave_def(
    {"samples": 256, "scaling": 1.0, "smoothing": 0.3, "r": 1.0, "g": 0.6, "b": 0.0, "a": 0.1},
    "x = 0.5 + value1 * 1.6 + 0.12 * sin(q1 * 0.9);\n"
    "y = 0.5 + value2 * 1.6 + 0.1 * cos(q1 * 0.7);\n",
    frame="a = 0.08 + 0.5 * q10;")
presets["nokkvi - living ink"] = preset(
    {"decay": 1.0, "wave_a": 0.0, "zoom": 1.0},
    " shader_body {\n" + HEAD + """
  vec2 p = (uv_orig - 0.5) * s;
  float r = length(p);
  vec2 px = texsize.zw * 3.0;
  float gx = GetBlur1(uv + vec2(px.x, 0.0)).x - GetBlur1(uv - vec2(px.x, 0.0)).x;
  float gy = GetBlur1(uv + vec2(0.0, px.y)).x - GetBlur1(uv - vec2(0.0, px.y)).x;
  vec2 grad = vec2(gx, gy);
  vec2 current = vec2(sin(p.y * 2.3 + q1), cos(p.x * 2.1 - q1 * 0.8)) * 0.0011 * q7;
  vec2 creep = vec2(-grad.y, grad.x) * 0.006 * q7;
  vec2 shock = (r > 0.0001 ? p / r : vec2(0.0)) * smoothstep(0.06, 0.0, abs(r - q6)) * 0.004 * q3;
  vec2 src = uv - (current + creep + shock) / s;
  vec3 m = texture(sampler_main, src).xyz;
  vec3 b1 = GetBlur1(src);
  vec3 b2 = mix(GetBlur2(src), GetBlur3(src), q9);
  float u = m.x + (b1.x - b2.x) * 1.3 + (m.x - b1.x) * 0.25;
  u += (texture(sampler_noise_lq, uv_orig * texsize.xy / 256.0 + rand_frame.xy).x - 0.5) * 0.06;
  u = u * 0.995 + 0.0015;
  float ang = atan(p.y, p.x) / 6.2831853 + 0.5;
  float spec = get_fft(0.02 + abs(ang * 2.0 - 1.0) * 0.5);
  float ring = smoothstep(0.03, 0.0, abs(r - 0.3 - 0.05 * sin(q1 * 0.7)));
  float feed = ring * spec * (0.6 + 0.8 * q5) + smoothstep(0.02, 0.0, abs(r - q6)) * q3 * 0.5;
  u = mix(u, 1.0, clamp(feed, 0.0, 1.0) * 0.35);
  float heat = max(m.y * 0.93, clamp(feed, 0.0, 1.0));
  ret = vec3(clamp((u - 0.5) * 1.03 + 0.5, 0.0, 1.0), heat, 0.0);
 }""",
    " shader_body {\n" + HEAD + """
  vec2 p = (uv - 0.5) * s;
  vec2 px = texsize.zw * 2.0;
  float hx = GetBlur1(uv + vec2(px.x, 0.0)).x - GetBlur1(uv - vec2(px.x, 0.0)).x;
  float hy = GetBlur1(uv + vec2(0.0, px.y)).x - GetBlur1(uv - vec2(0.0, px.y)).x;
  vec3 n = normalize(vec3(-hx * 6.0, -hy * 6.0, 1.0));
  vec3 L = normalize(vec3(cos(q8), sin(q8), 0.9));
  float diff = max(dot(n, L), 0.0);
  float spc = pow(max(dot(reflect(-L, n), vec3(0.0, 0.0, 1.0)), 0.0), 36.0);
  vec3 m = texture(sampler_main, uv).xyz;
  float u = m.x;
  float ang = atan(p.y, p.x) / 6.2831853;
  float hue = abs(fract(ang + length(p) * 0.6 + m.y * 0.25 + q1 * 0.03) * 2.0 - 1.0);
""" + ramp("body", "0.25 + 0.75 * hue") + """
  vec3 trough = mix(NOKKVI_BG, NOKKVI_SURFACE, 0.6 + 0.4 * sin(length(p) * 9.0 - q1 * 2.0));
  vec3 col = mix(trough, body, smoothstep(0.3, 0.6, u));
  col *= 0.35 + 0.85 * diff;
  col += NOKKVI_TEXT * spc * (0.35 + 0.5 * q5);
  col += NOKKVI_HIGHLIGHT * m.y * 0.55;
  col += NOKKVI_WARM * m.y * smoothstep(0.6, 1.0, m.y) * clamp(treb_att - 0.9, 0.0, 1.0) * 0.6;
  col *= 0.82 + 0.18 * smoothstep(1.05, 0.25, length(p));
  col *= 1.0 + 0.25 * q5;
  col += (texture(sampler_noise_lq, uv * texsize.xy / 256.0 + rand_frame.zw).x - 0.5) * 0.01;
  ret = col;
 }""",
    init="pulse = 0; pop = 0; phase = 0; kicks = 0; scene = 0; shockr = 9; flowd = 1; flowt = 1; light = 0.8; lightt = 0.8; scl = 0.3; sclt = 0.3; cool = 0;",
    frame=PULSE + """dt = 1 / max(fps, 1);
phase = phase + dt * (0.12 + 0.2 * min(mid_att, 2));
cool = max(cool - dt, 0);
hit = above(kick, 0.25) * below(cool, 0.001);
cool = if(hit, 0.2, cool);
kicks = kicks + hit;
shockr = if(hit, 0.02, shockr + dt * 0.55);
newscene = hit * equal(kicks % 16, 0);
scene = scene + newscene;
flowt = if(newscene, -flowt, flowt);
lightt = if(newscene, lightt + 2.1, lightt);
sclt = if(newscene, 1 - sclt, sclt);
flowd = flowd + (flowt - flowd) * 0.02;
light = light + (lightt - light) * 0.02 + dt * 0.08;
scl = scl + (sclt - scl) * 0.02;
q1 = phase;
q6 = shockr;
q7 = flowd;
q8 = light;
q9 = scl;
q10 = hit;
""",
    waves=[INK_WAVE])

# 6. Aurora (no cover) ----------------------------------------------------
# A night over the sea with a folded aurora curtain: a wavy arc (three
# drifting sine folds) with rays reaching up from it, brightest at the arc,
# in patches along it (a slow noise envelope) and where a fold turns edge-on
# (denser), streaked by three layers of vertically stretched noise, and lit
# from the theme's gradient: the light end at the arc, the dark end high up,
# the theme's warm colour at the tops. A fainter, slower curtain hangs higher
# behind it. The spectrum sets the rays' reach along the arc (bass in the
# middle, treble at the sides), smoothed through the feedback's y channel so
# it breathes instead of flickering; the rays' own glow rises through the
# feedback's x channel. A kick launches a surge that sweeps along the curtain
# (alternating sides); treble makes the rays shimmer. Stars twinkle behind
# the curtain, a dark ridge closes the horizon and the sea below mirrors it
# all through a ripple. (v runs upward in this engine's texture space: the
# cover helpers sample 1 - y for that reason.) x = curtain light,
# y = smoothed spectrum reach.
AURORA_ARC = """
  float xx = p.x + q2;
  float base = -0.1 + 0.05 * sin(xx * 2.1 + q1 * 0.7) + 0.03 * sin(xx * 5.3 - q1 * 1.1) + 0.015 * sin(xx * 11.0 + q1 * 1.9);
"""
AURORA_SEA_Y = "-0.36"
presets["nokkvi - aurora"] = preset(
    {"decay": 1.0, "wave_a": 0.0, "zoom": 1.0},
    " shader_body {\n" + HEAD + """
  vec2 p = (uv_orig - 0.5) * s;
  float yup = p.y;
  vec3 prev = texture(sampler_main, uv).xyz;
  float band = abs(uv_orig.x * 2.0 - 1.0);
  float spec = get_fft(0.02 + band * 0.45);
  float hgt = mix(prev.y, clamp(spec * 2.0, 0.0, 1.0), 0.1);
""" + AURORA_ARC + """
  float slope = 0.105 * cos(xx * 2.1 + q1 * 0.7) + 0.159 * cos(xx * 5.3 - q1 * 1.1) + 0.165 * cos(xx * 11.0 + q1 * 1.9);
  float dens = 0.4 + 2.5 * abs(slope);
  float env = 0.3 + 0.7 * texture(sampler_noise_hq, vec2(uv_orig.x * 0.25 + q2 * 0.15, 0.23)).x;
  float above = yup - base;
  float r1 = texture(sampler_noise_hq, vec2(uv_orig.x * 3.0 + q2 * 0.35, uv_orig.y * 0.12 + q1 * 0.015)).x;
  float r1c = texture(sampler_noise_hq, vec2(uv_orig.x * 3.0 + q2 * 0.35, 0.5 + q1 * 0.01)).x;
  float r2 = texture(sampler_noise_hq, vec2(uv_orig.x * 8.0 - q2 * 0.6, uv_orig.y * 0.25 - q1 * 0.04)).x;
  float r3 = texture(sampler_noise_hq, vec2(uv_orig.x * 20.0 + q2 * 0.9, uv_orig.y * 0.06 + q1 * 0.02)).x;
  float len = (0.1 + 0.45 * hgt + 0.1 * q3) * (0.5 + 1.1 * r1c * r1c);
  float body = exp(-max(above, 0.0) / len) * smoothstep(-0.025, 0.005, above);
  float edge = exp(-abs(above) / 0.018) * (0.6 + 0.4 * r2);
  float rays = (0.3 + 0.7 * r1 * r1) * (0.6 + 0.4 * r2) * (0.7 + 0.3 * r3);
  float sweep = exp(-pow((uv_orig.x - q4) * 5.0, 2.0)) * q6;
  float cur = (body * rays + edge * 0.7) * dens * env * (0.5 + 0.5 * hgt + 0.3 * q3) * (1.0 + 1.5 * sweep);
  float xx2 = p.x * 0.8 - q2 * 0.5 + 3.0;
  float base2 = 0.12 + 0.04 * sin(xx2 * 1.7 + q1 * 0.4) + 0.02 * sin(xx2 * 4.1 - q1 * 0.6);
  float above2 = yup - base2;
  float body2 = exp(-max(above2, 0.0) / (0.1 + 0.25 * hgt)) * smoothstep(-0.02, 0.005, above2);
  float env2 = 0.3 + 0.7 * texture(sampler_noise_hq, vec2(uv_orig.x * 0.4 - q2 * 0.1 + 0.5, 0.61)).x;
  cur += body2 * env2 * (0.4 + 0.6 * r1) * (0.6 + 0.4 * r3) * 0.35 * (0.6 + 0.4 * hgt);
  cur *= 1.0 + 0.25 * (r2 - 0.5) * clamp(treb_att - 0.8, 0.0, 1.0);
  float glow = texture(sampler_main, uv - vec2(0.0, texsize.w * 1.2)).x * 0.86;
  ret = vec3(clamp(max(cur, glow), 0.0, 1.0), hgt, 0.0);
 }""",
    " shader_body {\n" + HEAD + """
  vec2 p = (uv - 0.5) * s;
  float yup = p.y;
  vec3 m = texture(sampler_main, uv).xyz;
  float I = m.x;
""" + AURORA_ARC + """
  float hpos = clamp((yup - base) / 0.55, 0.0, 1.0);
""" + ramp("cc", "0.92 - 0.85 * hpos") + """
  vec3 col = NOKKVI_BG * (0.7 + 0.3 * smoothstep(0.5, -0.5, yup));
  vec2 g = uv * texsize.xy / 5.0;
  vec2 id = floor(g);
  vec2 f = fract(g) - 0.5;
  float h = fract(sin(dot(id, vec2(127.1, 311.7))) * 43758.5453);
  float h2 = fract(h * 91.3);
  float star = step(0.985, h) * smoothstep(0.35, 0.0, length(f - (vec2(h2, fract(h2 * 7.0)) - 0.5) * 0.4));
  star *= 0.5 + 0.5 * sin(time * (1.5 + 3.0 * h2) + h * 50.0);
  col += NOKKVI_TEXT * star * 0.55 * smoothstep(-0.32, -0.26, yup) * (1.0 - clamp(I * 2.5, 0.0, 1.0));
  col += cc * I * 1.7;
  col += NOKKVI_WARM * I * smoothstep(0.3, 0.85, hpos) * 0.45;
  col += NOKKVI_TEXT * pow(I, 3.0) * 0.45;
  col += cc * GetBlur2(uv).x * 0.5 + NOKKVI_HIGHLIGHT * GetBlur1(uv).x * 0.12;
  float sea_y = """ + AURORA_SEA_Y + """;
  float ridge = sea_y + 0.015 + 0.03 * texture(sampler_noise_hq, vec2(uv.x * 0.9 + 0.13, 0.37)).x + 0.012 * texture(sampler_noise_hq, vec2(uv.x * 3.1, 0.71)).x;
  float ripple = (texture(sampler_noise_hq, vec2(uv.x * 2.0, uv.y * 12.0 - q1 * 0.3)).x - 0.5) * 0.02;
  float ry = sea_y + (sea_y - yup) * 2.2;
  float Ir = texture(sampler_main, vec2(uv.x + ripple, 0.5 + ry / s.y)).x;
  float rhpos = clamp((ry - base) / 0.55, 0.0, 1.0);
""" + ramp("rc", "0.92 - 0.85 * rhpos") + """
  vec3 seacol = NOKKVI_BG * 0.45 + rc * Ir * 0.55 * (0.8 + 10.0 * ripple);
  float landr = smoothstep(ridge + 0.004, ridge - 0.004, ry);
  seacol = mix(seacol, NOKKVI_BG * 0.4, landr);
  float sea = smoothstep(sea_y + 0.003, sea_y - 0.003, yup);
  col = mix(col, seacol, sea);
  float land = smoothstep(ridge + 0.004, ridge - 0.004, yup) * (1.0 - sea);
  col = mix(col, NOKKVI_BG * 0.5, land);
  col *= 1.0 + 0.25 * q5;
  col *= 0.85 + 0.15 * smoothstep(1.2, 0.3, length(p));
  ret = col;
 }""",
    init="pulse = 0; pop = 0; t = 0; drift = 0; cool = 0; dir = 1; sx = 2; surge = 0;",
    frame=PULSE + """dt = 1 / max(fps, 1);
t = t + dt * (0.35 + 0.25 * min(bass_att, 2));
drift = drift + dt * (0.03 + 0.05 * (mid_att - 0.8));
cool = max(cool - dt, 0);
stamp = above(kick, 0.25) * below(cool, 0.001);
cool = if(stamp, 0.25, cool);
dir = if(stamp, -dir, dir);
sx = if(stamp, 0.5 - 0.6 * dir, sx);
sx = sx + dt * 1.6 * dir;
surge = if(stamp, 1, surge * 0.965);
q1 = t;
q2 = drift;
q4 = sx;
q6 = surge;
""")

# Deep dive: an endless flight into a self-growing relief. The cover is
# planted as a seed at the vanishing point; the feedback zooms into it while a
# sharpen + difference-of-blurs pass (FractalDrop / Geiss's Reaction Diffusion
# trick) keeps growing new fine detail, so every patch that swells into view
# breaks into smaller detail as it arrives. x = height, y = music heat,
# z = age (how far it has flown, which walks it along the theme's gradient).
# The comp shows it as a lava field: the high ground is dark crust lit as
# relief, the low ground glows through the cracks in the theme's gradient
# (the young end is light), heat and kicks flare it, treble tints it warm.
DIVE_FOCUS = "vec2 foc = vec2(0.09 * sin(q1 * 0.31), 0.07 * cos(q1 * 0.23));"
# On each kick the waveform is stamped as a ring just outside the seed, into
# the height (it becomes relief that dives outward with everything else) with
# a flash of heat; centred on the wandering focus.
DIVE_WAVE = wave_def(
    {"samples": 300, "scaling": 0.8, "smoothing": 0.4, "r": 0.7, "g": 0.9, "b": 0.0, "a": 0.8},
    "w = min(1, 8 * min(sample, 1 - sample));\n"
    "ang = sample * 6.2831853 + value2 * 0.6 * w;\n"
    "rad = q8 + value1 * 0.9 * w;\n"
    "x = 0.5 + q10 + rad * cos(ang);\n"
    "y = 0.5 - q11 + rad * sin(ang);\n",
    frame="a = 0.8 * q9;")
presets["nokkvi - cover deep dive"] = preset(
    {"decay": 1.0, "wave_a": 0.0, "zoom": 1.0},
    "uniform sampler2D sampler_fw_cover;\n shader_body {\n" + HEAD + f"""
  {DIVE_FOCUS}
  vec2 p = (uv_orig - 0.5) * s - foc;
  float zm = 1.010 + 0.022 * q2 + 0.03 * q5;
  float rt = 0.0025 + 0.006 * sin(q1 * 0.13) + 0.01 * q3;
  vec2 c = vec2(p.x * cos(rt) - p.y * sin(rt), p.x * sin(rt) + p.y * cos(rt)) / zm;
  vec2 src = (c + foc) / s + 0.5;
  vec3 m = texture(sampler_main, src).xyz;
  vec3 b1 = GetBlur1(src);
  vec3 b2 = GetBlur2(src);
  float u = m.x + (m.x - b1.x) * 0.5 + (m.x - b2.x) * 0.3;
  u += (texture(sampler_noise_lq, uv_orig * texsize.xy * 0.4 / 256.0 + rand_frame.xy).x - 0.5) * 0.3;
  u = clamp(u * 0.92 + 0.04, 0.0, 1.0);
  float age = min(m.z + 0.0045, 1.0);
  float r = length(p);
  float seedr = 0.06 + 0.015 * q3 + 0.02 * q5;
  vec2 cc = p / (seedr * 2.0) + 0.5;
  float cl = dot(texture(sampler_fw_cover, vec2(cc.x, 1.0 - cc.y)).xyz, {LUM});
  float seed = smoothstep(seedr, seedr * 0.55, r);
  u = mix(u, cl, seed * 0.55);
  age = mix(age, 0.0, seed);
  float ang = atan(p.y, p.x) / 6.2831853 + 0.5;
  float band = abs(ang * 2.0 - 1.0);
  float spec = mix(mix(bass, mid, smoothstep(0.0, 0.5, band)), treb, smoothstep(0.5, 1.0, band)) * 0.4;
  float ring = smoothstep(0.02, 0.0, abs(r - seedr * 1.15));
  float feed = ring * spec * (0.5 + 0.9 * q5);
  u = mix(u, 1.0, clamp(feed, 0.0, 1.0) * 0.3);
  float heat = max(m.y * 0.9, clamp(feed, 0.0, 1.0));
  ret = vec3(u, heat, age);
 }}""",
    " shader_body {\n" + HEAD + f"""
  {DIVE_FOCUS}
  vec2 p = (uv - 0.5) * s - foc;
  vec2 px = texsize.zw * 2.0;
  float hx = GetBlur1(uv + vec2(px.x, 0.0)).x - GetBlur1(uv - vec2(px.x, 0.0)).x;
  float hy = GetBlur1(uv + vec2(0.0, px.y)).x - GetBlur1(uv - vec2(0.0, px.y)).x;
  vec3 n = normalize(vec3(-hx * 7.0, -hy * 7.0, 1.0));
  vec3 L = normalize(vec3(cos(q4), sin(q4), 0.85));
  float diff = max(dot(n, L), 0.0);
  float spc = pow(max(dot(reflect(-L, n), vec3(0.0, 0.0, 1.0)), 0.0), 40.0);
  vec3 m = texture(sampler_main, uv).xyz;
""" + ramp("lava", "0.9 - 0.7 * m.z") + """
  float crust = smoothstep(0.35, 0.65, m.x);
  vec3 rock = mix(NOKKVI_BG, NOKKVI_SURFACE, 0.4 + 0.6 * m.z) * (0.35 + 0.9 * diff) + NOKKVI_TEXT * spc * 0.25;
  vec3 glow = lava * (1.0 - crust) * (0.55 + 0.6 * m.y + 0.4 * q3);
  glow += NOKKVI_WARM * m.y * (0.5 + 0.5 * clamp(treb_att - 0.9, 0.0, 1.0));
  vec3 col = rock * crust + glow;
  vec3 b2 = GetBlur2(uv);
  col += lava * (1.0 - b2.x) * (0.3 + 0.3 * q3) + NOKKVI_WARM * b2.y * 0.5;
  col *= 0.8 + 0.2 * smoothstep(1.1, 0.2, length(p));
  col *= 1.0 + 0.25 * q5;
  col += (texture(sampler_noise_lq, uv * texsize.xy / 256.0 + rand_frame.zw).x - 0.5) * 0.01;
  ret = col;
 }""",
    init="pulse = 0; pop = 0; phase = 0; speed = 0; light = 0.8; cool = 0;",
    frame=PULSE + "speed = speed * 0.9 + 0.1 * (0.2 + 0.5 * min(bass_att, 2) + 1.5 * q3);\n"
          "phase = phase + 0.01 + 0.02 * min(mid_att, 2);\nlight = light + 0.003 + 0.01 * q3;\n"
          "cool = max(cool - 1 / max(fps, 1), 0);\n"
          "stamp = above(kick, 0.2) * below(cool, 0.001);\n"
          "cool = if(stamp, 0.18, cool);\n"
          "q1 = phase;\nq2 = speed;\nq4 = light;\n"
          "q8 = 0.11 + 0.03 * q3;\nq9 = stamp;\n"
          "q10 = 0.09 * sin(phase * 0.31);\nq11 = 0.07 * cos(phase * 0.23);",
    waves=[DIVE_WAVE])

# Fractal zoom: the classic endless MilkDrop dive, computed rather than fed
# back. A Kali-set fractal (z = |z| / |z|^2 + c) is drawn at three zoom
# octaves that cross-fade in a loop, so the fall never ends; each octave's c
# differs a little and drifts with the music, so the shapes warp and change as
# you sink into them, and each octave turns as it zooms (the twist). The
# waveform rides along: on every kick a custom wave stamps it as a squiggly
# ring around the vanishing point, into the feedback's spare z channel (the
# ink), and the feedback is zoomed and twisted at exactly the fractal's own
# rate (q6 = the frame's magnification, q7 = its turn), so each beat's ring
# flies outward past the viewer with the dive and the next beat pushes it on.
# A magnified ring also gets thicker, so the ink fades with the zoom as well
# as with time; otherwise the rings pile up into a white haze. (Redrawing the
# ring every frame, sharpened against its blur, saturated the whole field
# into a reaction-diffusion print.) The comp lights the result as relief in
# the theme's gradient and draws the ink as neon, white-hot when fresh and
# cooling along the gradient as it recedes. x = brightness, y = colour
# position, z = ink.
FZ_OCTAVES = 3
def fz_octave(k):
    return f"""
  {{
    float f = fract(q1 + {k}.0 / {FZ_OCTAVES}.0);
    float w = sin(3.14159265 * f);
    float sc = 1.3 / pow(10.0, f);
    float an = q2 + f * 2.2 + {k * 2.1:.1f};
    vec2 z = vec2(p.x * cos(an) - p.y * sin(an), p.x * sin(an) + p.y * cos(an)) * sc + vec2(0.18, 0.11);
    vec2 kc = vec2(-0.58, -0.54) + 0.06 * vec2(cos(q4 + {k * 1.7:.1f}), sin(q4 * 0.8 + {k * 2.3:.1f}));
    float sum = 0.0;
    float orb = 0.0;
    float prev = length(z);
    for (int i = 0; i < 14; i++) {{
      z = abs(z) / max(dot(z, z), 0.0001) + kc;
      float l = length(z);
      sum += abs(l - prev);
      prev = l;
      orb += exp(-l * l * 1.5);
    }}
    float v = pow(clamp((sum / 14.0 - 0.8) / 2.5, 0.0, 1.0), 1.3);
    acc += v * w;
    hue += clamp(v * 0.6 + 0.4 * fract(orb * 0.25 + {k}.0 * 0.13), 0.0, 1.0) * w;
    wsum += w;
  }}
"""
# The squiggle: the left channel bends the ring's radius, the right nudges
# its angle; both ease to zero at the seam so the ring closes. Drawn only on
# the frame of a kick (q9), at radius q8 (bigger for a harder kick); q2 turns
# the seam with the fractal.
FZ_WAVE = wave_def(
    {"samples": 400, "scaling": 0.9, "smoothing": 0.4, "r": 0.6, "g": 0.0, "b": 1.0, "a": 0.9},
    "w = min(1, 8 * min(sample, 1 - sample));\n"
    "ang = sample * 6.2831853 + q2 * 0.5 + value2 * 0.7 * w;\n"
    "rad = q8 + value1 * 1.1 * w;\n"
    "x = 0.5 + rad * cos(ang);\n"
    "y = 0.5 + rad * sin(ang);\n",
    frame="a = 0.9 * q9;")
presets["nokkvi - fractal zoom"] = preset(
    {"decay": 1.0, "wave_a": 0.0, "zoom": 1.0},
    " shader_body {\n" + HEAD + """
  vec2 p = (uv_orig - 0.5) * s;
  float acc = 0.0;
  float hue = 0.0;
  float wsum = 0.0;
""" + "".join(fz_octave(k) for k in range(FZ_OCTAVES)) + """
  acc /= wsum;
  hue /= wsum;
  vec2 cs = (uv - 0.5) * s;
  vec2 c1 = vec2(cs.x * cos(q7) - cs.y * sin(q7), cs.x * sin(q7) + cs.y * cos(q7)) / q6;
  vec2 c1u = c1 / s + 0.5;
  vec3 fb = texture(sampler_main, c1u).xyz;
  float ink = fb.z * 0.996 / sqrt(q6) * step(2.5, frame);
  float b = max(acc, fb.x * (0.72 + 0.12 * q3));
  float h = mix(hue, fb.y, step(acc, fb.x * 0.9) * 0.8);
  ret = vec3(clamp(b, 0.0, 1.0), h, clamp(ink, 0.0, 1.0));
 }""",
    " shader_body {\n" + HEAD + """
  vec2 p = (uv - 0.5) * s;
  vec2 px = texsize.zw * 1.5;
  vec3 bl = GetBlur1(uv - vec2(px.x, 0.0));
  vec3 br = GetBlur1(uv + vec2(px.x, 0.0));
  vec3 bd = GetBlur1(uv - vec2(0.0, px.y));
  vec3 bu = GetBlur1(uv + vec2(0.0, px.y));
  float hx = (br.x - bl.x) + (br.z - bl.z) * 0.6;
  float hy = (bu.x - bd.x) + (bu.z - bd.z) * 0.6;
  vec3 n = normalize(vec3(-hx * 3.0, -hy * 3.0, 1.0));
  vec3 L = normalize(vec3(-0.5, 0.6, 0.8));
  float diff = max(dot(n, L), 0.0);
  float spc = pow(max(dot(reflect(-L, n), vec3(0.0, 0.0, 1.0)), 0.0), 30.0);
  vec3 m = texture(sampler_main, uv).xyz;
  float ink = m.z;
""" + ramp("body", "m.y") + """
  vec3 col = mix(NOKKVI_BG, body, smoothstep(0.08, 0.55, m.x));
  col = mix(col, NOKKVI_TEXT, smoothstep(0.85, 1.0, m.x) * 0.5);
  col *= 0.55 + 0.6 * diff;
""" + ramp("inkc", "0.3 + 0.7 * ink") + """
  inkc = mix(inkc, NOKKVI_TEXT, smoothstep(0.55, 1.0, ink) * 0.85);
  col = mix(col, inkc * (0.7 + 0.6 * diff), clamp(ink * 1.3, 0.0, 1.0) * 0.85);
  vec3 b2 = GetBlur2(uv);
  col += NOKKVI_HIGHLIGHT * b2.x * (0.04 + 0.25 * q3);
  col += NOKKVI_HIGHLIGHT * b2.z * (0.35 + 0.4 * q3);
  col += NOKKVI_TEXT * spc * max(m.x, ink) * (0.3 + 0.5 * q5);
  col += NOKKVI_WARM * smoothstep(0.8, 1.0, m.x) * clamp(treb_att - 0.9, 0.0, 1.0) * 0.3;
  col *= 0.8 + 0.2 * smoothstep(1.1, 0.2, length(p));
  col *= 1.0 + 0.3 * q5;
  ret = col;
 }""",
    init="pulse = 0; pop = 0; dive = 0; turn = 0; morph = 0; cool = 0; q6 = 1; q7 = 0; q8 = 0.2; q9 = 0;",
    frame=PULSE + "dd = (0.0015 + 0.002 * min(bass_att, 2) + 0.006 * q3) * 60 / max(fps, 1);\n"
          "dive = dive + dd;\n"
          "dturn = 0.002 + 0.003 * (mid_att - 1) + 0.01 * q3;\n"
          "turn = turn + dturn;\n"
          "morph = morph + 0.004 + 0.01 * min(mid_att, 2) + 0.02 * pop;\n"
          "q1 = dive;\nq2 = turn;\nq4 = morph;\n"
          "q6 = pow(10, dd);\nq7 = dturn + 2.2 * dd;\n"
          "cool = max(cool - 1 / max(fps, 1), 0);\n"
          "stamp = above(kick, 0.2) * below(cool, 0.001);\n"
          "cool = if(stamp, 0.16, cool);\n"
          "q8 = 0.2 + 0.06 * min(kick / 0.5, 1);\nq9 = stamp;",
    waves=[FZ_WAVE])

# Ports -------------------------------------------------------------------
# "Flexi, martin + geiss - dedicated to the sherwin maxawow" is the preset
# Butterchurn's README loads as its example, the first one most people see,
# and it has many remixes. Its look: three whirlpools (bass, mid, treble)
# stir the picture, the warp smears it along its own colour (painterly
# strokes), and the comp embosses it with glinting ridges; the paint comes
# from an inner border whose colour cycles through a rainbow. The ports keep
# all of that and change only where the paint comes from: the theme's
# gradient (the border walks along it, the wave takes the far end), or the
# playing cover (its colours bleed in from the screen edges while the border
# is off). The border also pulses between the theme's background and the
# gradient, since a theme gradient alone has far less brightness range than
# the original's rainbow and the relief washed out. The ridges glint in the theme's highlight instead of white.
MAXAWOW = "Flexi, martin + geiss - dedicated to the sherwin maxawow"
def ramp_eel(v, t):
    """EEL: r/g/b of the theme gradient at `t` (0..1) into v_r, v_g, v_b,
    as a sum of clamped linear segments."""
    out = f"{v}_x = min(max({t}, 0), 1) * 5;\n"
    for ch in "RGB":
        terms = [f"NOKKVI_RAMP0_{ch}"]
        for k in range(1, 6):
            terms.append(f"(NOKKVI_RAMP{k}_{ch} - NOKKVI_RAMP{k-1}_{ch}) * min(max({v}_x - {k-1}, 0), 1)")
        out += f"{v}_{ch.lower()} = " + " + ".join(terms) + ";\n"
    return out
def maxawow_port(cover):
    src = json.load(open(os.path.join(OUT, MAXAWOW + ".json")))
    p = json.loads(json.dumps(src))
    frame = ("t = 0.5 + 0.3 * sin(time * 2.3) + 0.2 * sin(time * 0.61);\n"
             + ramp_eel("c", "t") + ramp_eel("w", "1 - t")
             + "lit = 0.2 + 0.8 * (0.5 + 0.5 * sin(time * 4));\n"
             + "ib_r = c_r * lit + NOKKVI_BG_R * (1 - lit); ib_g = c_g * lit + NOKKVI_BG_G * (1 - lit); ib_b = c_b * lit + NOKKVI_BG_B * (1 - lit);\n"
             + "wave_r = w_r; wave_g = w_g; wave_b = w_b;\n"
             + "wave_x = 0.5 + sin(time * 3) * 0.3;\nwave_y = 0.5 + cos(time * 2.187) * 0.3;\n")
    if cover:
        frame += "ib_a = 0;\n"
    p["frame_eqs_eel"] = frame
    comp = p["comp"]
    old = "tmpvar_6.xyz = (tmpvar_5 +"
    assert comp.count(old) == 1
    p["comp"] = comp.replace(old, "tmpvar_6.xyz = (NOKKVI_HIGHLIGHT * tmpvar_5 +")
    if cover:
        warp = p["warp"]
        old = "  ret = tmpvar_4.xyz;\n"
        assert warp.count(old) == 1
        p["warp"] = "uniform sampler2D sampler_fw_cover;\n" + warp.replace(old,
            "  vec2 ed = min(uv_orig, 1.0 - uv_orig);\n"
            "  float edge = smoothstep(0.02, 0.0, min(ed.x, ed.y));\n"
            "  vec2 cc = uv_orig * 0.6 + 0.2 + 0.18 * vec2(sin(time * 0.13), cos(time * 0.11));\n"
            "  vec3 cov = texture(sampler_fw_cover, vec2(cc.x, 1.0 - cc.y)).xyz;\n"
            "  tmpvar_4.xyz = mix(tmpvar_4.xyz, cov, edge * 0.5);\n"
            "  ret = tmpvar_4.xyz;\n")
    return p

# "Flexi - black holes [sucking fractals for lunch] 2", one of the most
# downloaded presets in the Internet Archive's MilkDrop collection. It is not
# in the Butterchurn pack (it needs a photo texture, "sunrise"), so its
# source lives in scripts/milkdrop-sources/. Four black holes bounce around
# the screen with gravity and collide with each other; the comp shows the
# feedback through 16 / ((p - h1)(p - h2)(p - h3)(p - h4)) in the complex
# plane, which has a pole at each hole, so the picture spirals endlessly
# into every one of them and the space between them warps as they move. The
# picture is the inverted feedback blended with the photo. The ports put the
# playing cover (or clouds in the theme's gradient) where the photo was,
# colour the inverted feedback from the theme instead of its raw channels,
# mix the two by brightness rather than per channel (which tinted the result
# blue and red whatever the theme), give holes near the floor a hop on each
# kick (the original only swells them) and add a ceiling so a hop never
# launches a hole off the screen for good. The holes' height is flipped
# (this engine's v runs up), so gravity pulls them to the bottom.
SOURCES = os.path.join(os.path.dirname(os.path.abspath(__file__)), "milkdrop-sources")
BLACK_HOLES = "Flexi - black holes [sucking fractals for lunch] 2"
def black_holes_port(cover):
    p = json.load(open(os.path.join(SOURCES, BLACK_HOLES + ".json")))
    hop = ("kick = max(bass - bass_att, 0);\ncool = max(cool - 1 / max(fps, 1), 0);\n"
           "hop = above(kick, 0.25) * below(cool, 0.001);\ncool = if(hop, 0.25, cool);\n")
    for i in range(1, 5):
        hop += (f"low = below(y{i}, 0.35);\n"
                f"vy{i} = vy{i} + hop * low * (0.002 + 0.002 * rand(100) / 100);\n"
                f"vx{i} = vx{i} + hop * low * (rand(100) / 100 - 0.5) * 0.006;\n"
                f"vy{i} = if(above(y{i}, 0.96), -abs(vy{i}) * 0.9, vy{i});\n")
    frame = p["frame_eqs_eel"]
    for i, q in ((1, 2), (2, 4), (3, 6), (4, 8)):
        old_q = f"q{q} = -0.5 + y{i};"
        assert frame.count(old_q) == 1, old_q
        frame = frame.replace(old_q, f"q{q} = 0.5 - y{i};")
    p["frame_eqs_eel"] = hop + frame
    p["init_eqs_eel"] = p["init_eqs_eel"] + "\ncool = 0;\n"
    comp = p["comp"]
    def rep(old, new):
        nonlocal comp
        assert comp.count(old) == 1, old
        comp = comp.replace(old, new)
    if cover:
        rep("uniform sampler2D sampler_sunrise;", "uniform sampler2D sampler_fw_cover;")
        photo = "texture (sampler_fw_cover, vec2(uv_1.x, 1.0 - uv_1.y))"
        sky_code = ""
    else:
        rep("uniform sampler2D sampler_sunrise;\n", "")
        n = ("clamp(texture(sampler_noise_hq, uv_1 * 0.35 + vec2(time * 0.004, 0.0)).x * 0.7"
             " + texture(sampler_noise_hq, uv_1 * 0.9 - vec2(0.0, time * 0.006)).x * 0.3, 0.0, 1.0)")
        sky_code = "  float sky_n = " + n + ";\n" + ramp("sky", "0.15 + 0.7 * sky_n")
        photo = "vec4(sky, 1.0)"
    rep("texture (sampler_sunrise, uv_1)", photo)
    rep("  vec4 tmpvar_19;\n  tmpvar_19.w = 0.0;\n  tmpvar_19.xyz = (1.1 - tmpvar_18.xyz);\n",
        sky_code
        + f"  float inv_l = clamp(1.1 - dot(tmpvar_18.xyz, {LUM}), 0.0, 1.0);\n"
        + ramp("inv_c", "inv_l")
        + "  vec4 tmpvar_19;\n  tmpvar_19.w = 0.0;\n"
          "  tmpvar_19.xyz = mix(NOKKVI_BG, inv_c, smoothstep(0.05, 0.6, inv_l)) + NOKKVI_TEXT * smoothstep(0.85, 1.1, inv_l) * 0.5;\n")
    rep("  tmpvar_20.xyz = ((-(tmpvar_18.xyz) * 1.1) + 1.5);\n",
        f"  tmpvar_20.xyz = vec3(1.5 - 1.1 * dot(tmpvar_18.xyz, {LUM}));\n")
    p["comp"] = comp
    return p
# Chladni (no cover) ------------------------------------------------------
# Sand on a vibrating metal plate. The plate rings in a Chladni mode
# A = cos(n pi x) cos(m pi y) + s cos(m pi x) cos(n pi y) (the plate spans
# -1..1 across the short side), and the sand slides down the gradient of the
# vibration energy A^2 into the nodal lines, where the plate stands still,
# drawing the mode's figure. The music tunes the plate: a brighter mix
# (treble against bass, relative to the song's own average) picks a more
# complex mode, and on a kick the plate retunes; the old and new modes
# crossfade over 1.5 s, so the sand visibly migrates from one figure to the
# next. It retunes anyway after 16 kicks on one mode. Big kicks throw a
# handful of fresh sand in the next colour somewhere on the plate, and the
# figure pulls it in. Louder music shakes the plate harder: the sand moves
# faster and hops off the antinodes; silence freezes the figure.
# The feedback holds the sand, not the picture: x = density (advected with
# a continuity term so it piles up where the flow converges, relaxing slowly
# towards an even layer so bare plate refills), y = the sand's colour tag
# (carried with it, mass-weighted with fresh sand), z = how fast it moves
# (glints). The comp draws a brushed dark plate, grains thresholded against
# a static per-pixel noise, lit and shadowed, coloured from the gradient.
CHLADNI_MODES = [(1, 2, 1), (1, 3, -1), (1, 4, 1), (2, 3, 1), (1, 5, -1), (2, 5, -1), (3, 4, 1), (3, 5, 1)]
def chladni_pick(var, idx, k):
    """Nested EEL ifs: component k (0 = m, 1 = n, 2 = sign) of mode `idx`."""
    e = str(CHLADNI_MODES[-1][k])
    for i in range(len(CHLADNI_MODES) - 2, -1, -1):
        e = f"if(equal({idx}, {i}), {CHLADNI_MODES[i][k]}, {e})"
    return f"{var} = {e};\n"
def chladni_a(x, y):
    """Plate displacement at plate coords (x, y): the old mode (q11-q13)
    crossfaded into the new one (q14-q16) by q17."""
    pi = "3.14159265"
    return (f"mix(cos(q12 * {pi} * ({x})) * cos(q11 * {pi} * ({y})) + q13 * cos(q11 * {pi} * ({x})) * cos(q12 * {pi} * ({y})),"
            f" cos(q15 * {pi} * ({x})) * cos(q14 * {pi} * ({y})) + q16 * cos(q14 * {pi} * ({x})) * cos(q15 * {pi} * ({y})), q17)")
def chladni_e(v, x, y):
    return f"  float {v}_a = {chladni_a(x, y)};\n  float {v} = {v}_a * {v}_a;\n"
CHLADNI_GRAIN = """
  vec2 gr_i = floor(uv * texsize.xy / 1.6);
  float grain = texture(sampler_noise_lq, (gr_i + 0.5) / 256.0).x;
  float grain2 = texture(sampler_noise_lq, (gr_i.yx + 17.5) / 256.0).y;
"""
presets["nokkvi - chladni"] = preset(
    {"decay": 1.0, "wave_a": 0.0, "zoom": 1.0},
    " shader_body {\n" + HEAD + """
  vec2 p = (uv_orig - 0.5) * s * 2.0;
  float h = 0.006;
""" + chladni_e("e0", "p.x", "p.y") + chladni_e("ex1", "p.x + h", "p.y") + chladni_e("ex0", "p.x - h", "p.y")
    + chladni_e("ey1", "p.x", "p.y + h") + chladni_e("ey0", "p.x", "p.y - h") + """
  vec2 grad = vec2(ex1 - ex0, ey1 - ey0) / (2.0 * h);
  float lap = (ex1 + ex0 + ey1 + ey0 - 4.0 * e0) / (h * h);
  float k = 0.00006 * q18;
  vec2 vel = -grad * k;
  float vl = length(vel);
  vel *= min(1.0, 0.004 / max(vl, 0.000001));
  vec2 hop = (texture(sampler_noise_lq, uv_orig * texsize.xy / 256.0 * 0.5 + rand_frame.xy).xy - 0.5)
           * min(e0, 2.0) * (0.0015 * q18 + 0.012 * q5);
  vec2 src = uv - (vel + hop) / (s * 2.0);
  vec3 m = texture(sampler_main, src).xyz;
  vec3 b = GetBlur1(src);
  float u = m.x / 0.45 * (1.0 + clamp(k * lap, -0.06, 0.06));
  u = mix(u, b.x / 0.45, clamp(e0 * q18 * 0.08, 0.0, 0.35));
  float local_mean = max(GetBlur3(src).x / 0.45, 0.02);
  u *= pow(0.3 / local_mean, 0.04);
  float tag = m.y;
  vec2 hd = p - vec2(q21 * s.x, q22 * s.y) * 1.6;
  float hand = smoothstep(0.16, 0.0, length(hd)) * q23
             * step(0.55, texture(sampler_noise_hq, uv_orig * texsize.xy / 256.0 + rand_frame.zw).x);
  float fresh = max(0.3 - u, 0.0) * 0.002 + hand * 0.9;
  tag = (tag * max(u, 0.0) + q19 * fresh) / (max(u, 0.0) + fresh + 0.0001);
  u = clamp(u + fresh, 0.0, 2.2);
  float moving = max(m.z * 0.9, clamp(length(vel) * 400.0, 0.0, 1.0) * clamp(u, 0.0, 1.0));
  if (frame < 2.5) {
    u = 0.3;
    tag = 0.5 + 0.35 * sin(atan(p.y, p.x) * 2.0 + length(p) * 3.0);
    moving = 0.0;
  }
  vec2 dith = texture(sampler_noise_hq, uv_orig * texsize.xy / 256.0 + rand_frame.yz).xy - 0.5;
  ret = vec3(u * 0.45 + dith.x / 255.0, tag + dith.y / 255.0, moving);
 }""",
    " shader_body {\n" + HEAD + """
  vec2 p = (uv - 0.5) * s * 2.0;
""" + chladni_e("e0", "p.x", "p.y") + CHLADNI_GRAIN + """
  vec3 m = texture(sampler_main, uv).xyz;
  float u = m.x / 0.45;
  vec2 L = normalize(vec2(-0.6, 0.8));
  vec2 px = texsize.zw * 2.5;
  float sh = texture(sampler_main, uv - L * px).x / 0.45;
  float hx = GetBlur1(uv + vec2(texsize.z * 2.0, 0.0)).x - GetBlur1(uv - vec2(texsize.z * 2.0, 0.0)).x;
  float hy = GetBlur1(uv + vec2(0.0, texsize.w * 2.0)).x - GetBlur1(uv - vec2(0.0, texsize.w * 2.0)).x;
  float lit = clamp(0.75 + dot(vec2(hx, hy), L) * 10.0, 0.4, 1.4);
  float brush = texture(sampler_noise_hq, vec2(uv.x * 0.05, uv.y * 3.0)).x;
  vec3 plate = mix(NOKKVI_BG, NOKKVI_SURFACE, 0.35 + 0.35 * brush);
  plate += NOKKVI_SURFACE * pow(max(1.0 - length(p - vec2(-0.5, 0.6)) * 0.6, 0.0), 3.0) * 0.5;
  plate += NOKKVI_HIGHLIGHT * min(e0, 4.0) * 0.012 * q18 * (0.5 + 0.5 * sin(time * 47.0 + e0 * 3.0));
  plate *= 1.0 - 0.35 * smoothstep(0.3, 1.2, sh);
  float g = step(grain, clamp(u * 0.9 - 0.08, 0.0, 1.0));
""" + ramp("sand", "m.y") + """
  vec3 col_s = sand * (0.55 + 0.45 * grain2) * lit;
  col_s = mix(col_s, NOKKVI_TEXT, smoothstep(1.3, 2.0, u) * 0.25);
  vec3 col = mix(plate, col_s, g);
  float glint = step(0.985, grain2) * g * (0.3 + m.z * 1.5 + q5 * 0.6);
  col += NOKKVI_TEXT * glint * 0.7;
  col *= 0.75 + 0.25 * smoothstep(1.4, 0.3, length(p / s));
  ret = col;
 }""",
    init="pulse = 0; pop = 0; phase = 0; kicks = 0; since = 0; cool = 0; hitcool = 0; tavg = 1; energy = 0;"
         " cur = 0; old = 0; blend = 1; pc = 0.8; hs = 0; hx = 0; hy = 0; handcool = 0;",
    frame=PULSE + """dt = 1 / max(fps, 1);
phase = phase + dt;
loud = min((bass_att + mid_att + treb_att) / 3, 2.5);
energy = energy * 0.95 + 0.05 * loud;
tone = (mid_att + 2 * treb_att) / (bass_att + mid_att + treb_att + 0.01);
tavg = tavg * 0.997 + tone * 0.003;
target = min(max(floor(3.5 + (tone / max(tavg, 0.1) - 1) * 10), 0), 7);
cool = max(cool - dt, 0);
hitcool = max(hitcool - dt, 0);
handcool = max(handcool - dt, 0);
hit = above(kick, 0.25) * below(hitcool, 0.001);
hitcool = if(hit, 0.2, hitcool);
since = since + hit;
retune = hit * below(cool, 0.001) * above(abs(target - cur) + above(since, 15.5) * 2, 0.5);
nxt = if(above(abs(target - cur), 0.5), target, (cur + 3) % 8);
old = if(retune, cur, old);
cur = if(retune, nxt, cur);
blend = if(retune, 0, min(blend + dt / 1.5, 1));
cool = if(retune, 4, cool);
since = if(retune, 0, since);
pc = if(retune, pc + 0.37 - floor(pc + 0.37), pc);
throw = hit * above(pop, 0.35) * below(handcool, 0.001);
handcool = if(throw, 0.6, handcool);
hx = if(throw, rand(1000) / 1000 - 0.5, hx);
hy = if(throw, rand(1000) / 1000 - 0.5, hy);
hs = if(throw, 1, hs * 0.7);
""" + chladni_pick("q11", "old", 0) + chladni_pick("q12", "old", 1) + chladni_pick("q13", "old", 2)
    + chladni_pick("q14", "cur", 0) + chladni_pick("q15", "cur", 1) + chladni_pick("q16", "cur", 2) + """q17 = blend * blend * (3 - 2 * blend);
q18 = min(energy, 2) * above(energy, 0.05);
q19 = pc;
q21 = hx;
q22 = hy;
q23 = hs;
""")

# Julia lace (no cover) ---------------------------------------------------
# An endless dive into a Julia set's spiral vortex. The set is drawn as fine
# lace (the distance estimate turns its boundary into hairline ridges with a
# soft glow) on a ground plane seen in perspective, lit as relief. Its
# parameter c = l/2 - l^2/4 sits just outside the Mandelbrot set's main
# cardioid in the seahorse valley, where the Julia set is all spirals: l is
# the multiplier of the fixed point alpha = l/2, |l| = 1.03 so alpha repels
# and the spirals wind around it. The Julia set maps onto itself under
# z -> z^2 + c, which near alpha is z -> alpha + l (z - alpha): zooming in by
# |l|^M while turning by M arg(l) shows the same picture again. So the camera
# looks steeply down at alpha and sinks towards it, the ground turning under
# it, and after a zoom of |l|^M (about 3x) it is back where it started, one
# ring deeper; the last fifth of every cycle crossfades into the next cycle's
# view (the same window mapped by l^M, drawn a second time), so the seam
# never shows even where the map is not quite linear. Colour follows the
# iteration count shifted by M per cycle, so it matches across the seam too.
# Only M arg(l) modulo a full turn matters at the seam, so the ground turns
# by at most half a turn per cycle (the state `grot` absorbs the rest at the
# wrap). The camera is free: it swings between a steep dive and a shallow,
# horizon-grazing glide, sweeps its heading around the vortex, looks a
# little off centre (an offset that scales with the altitude and turns with
# `grot`, so it stays self-similar across the seam) and banks gently.
# arg(l) drifts slowly, so the spirals wind and unwind as you fall. The music
# never touches the shape (a Julia set is so sensitive to c that a
# beat-driven nudge reads as a glitch): loudness speeds the dive, very
# smoothly, and kicks brighten the lace's glow. Line widths follow each
# pixel's footprint on the plane (screen derivatives). Pure comp.
JULIA_ITERS = 240
JULIA_M = 37
JULIA_FUNCS = f"""
float julia(vec2 z, vec2 c, out float d, out float mu) {{
  vec2 dz = vec2(1.0, 0.0);
  float m2 = 0.0;
  float it = 0.0;
  for (int i = 0; i < {JULIA_ITERS}; i++) {{
    dz = 2.0 * vec2(z.x * dz.x - z.y * dz.y, z.x * dz.y + z.y * dz.x);
    z = vec2(z.x * z.x - z.y * z.y, 2.0 * z.x * z.y) + c;
    m2 = dot(z, z);
    if (m2 > 1e5) break;
    it += 1.0;
  }}
  float lz = 0.5 * log(max(m2, 1.0001));
  d = sqrt(m2 / max(dot(dz, dz), 1e-20)) * lz;
  mu = it + 1.0 - log2(max(lz, 1e-6));
  return step({JULIA_ITERS}.0 - 0.5, it);
}}
"""
def julia_shade(v, w0, hueshift):
    """Lace colour `v` for the ground point `w0` (a vec2 expression)."""
    return f"""
  vec2 {v}_w = {w0};
  float {v}_d;
  float {v}_mu;
  float {v}_in = julia({v}_w, c, {v}_d, {v}_mu);
  float {v}_pix = max(length(dFdx({v}_w)), length(dFdy({v}_w))) + 1e-9;
  float {v}_line = clamp(1.0 - {v}_d / ({v}_pix * 1.3), 0.0, 1.0) * (1.0 - {v}_in);
  float {v}_glow = exp(-{v}_d / ({v}_pix * 10.0)) * (1.0 - {v}_in);
  float {v}_ridge = exp(-{v}_d / ({v}_pix * 4.0));
  float {v}_shade = clamp(0.75 - dot(vec2(dFdx({v}_ridge), dFdy({v}_ridge)), vec2(0.6, -0.8)) * 2.5, 0.35, 1.4);
  float {v}_hue = fract({v}_mu * 0.015 - ({hueshift}) + q9);
""" + ramp(v + "_lace", f"0.35 + 0.6 * abs({v}_hue * 2.0 - 1.0)") + f"""
  vec3 {v} = mix(ground, NOKKVI_BG * 0.55, {v}_in * 0.8);
  {v} += {v}_lace * {v}_glow * (0.3 + 0.35 * q3) * {v}_shade;
  {v} = mix({v}, {v}_lace * (0.9 + 0.3 * q3) * {v}_shade, {v}_line);
  {v} = mix({v}, NOKKVI_TEXT, {v}_line * smoothstep(0.6, 1.0, {v}_glow) * 0.2);
"""
presets["nokkvi - julia lace"] = preset(
    {"decay": 0.0, "wave_a": 0.0, "zoom": 1.0},
    " shader_body {\n  ret = vec3(0.0);\n }",
    JULIA_FUNCS + " shader_body {\n" + HEAD + """
  vec2 p = (uv - 0.5) * s;
  p = vec2(p.x * cos(q27) - p.y * sin(q27), p.x * sin(q27) + p.y * cos(q27));
  float ha = q4;
  float pt = q10;
  vec3 fw = vec3(cos(pt) * cos(ha), cos(pt) * sin(ha), -sin(pt));
  vec3 rt = vec3(sin(ha), -cos(ha), 0.0);
  vec3 up = cross(rt, fw);
  vec3 rd = normalize(fw + (p.x * rt + p.y * up) * 1.15);
  float down = max(-rd.z, 0.02);
  vec2 wa = vec2(q7, q8) + rd.xy * (q6 / down);
  vec2 c = vec2(q1, q2);
  vec2 al = vec2(q15, q16);
  vec3 ground = mix(NOKKVI_BG, NOKKVI_SURFACE, 0.2);
""" + julia_shade("ca", "wa", "q11") + """
  vec3 col = ca;
  if (q12 > 0.0) {
    vec2 rel = wa - al;
    vec2 wb = al + vec2(q13 * rel.x - q14 * rel.y, q13 * rel.y + q14 * rel.x);
""" + julia_shade("cb", "wb", f"q11 - {JULIA_M}.0 * 0.015") + """
    col = mix(ca, cb, q12);
  }
  float fogd = 1.0 - exp(-(1.0 / down) * 0.12);
  vec3 sky = mix(NOKKVI_SURFACE, NOKKVI_BG, 0.5);
  col = mix(col, sky, clamp(fogd, 0.0, 1.0) * 0.6);
  col *= 0.8 + 0.2 * smoothstep(1.2, 0.3, length(p));
  ret = col;
 }""",
    init="pulse = 0; pop = 0; hue = 0; glide = 0.8; dv = 0; fprev = 0; cyc = 0; grot = 0;",
    frame=PULSE + f"""dt = min(1 / max(fps, 1), 0.1);
loud = min((bass_att + mid_att + treb_att) / 3, 2);
glide = glide * 0.995 + 0.005 * (0.6 + 0.5 * loud);
m = {JULIA_M};
lr = 1.03;
phi = 0.43 + 0.035 * sin(time * 0.017) + 0.005 * sin(time * 0.05);
a = phi * 6.2831853;
q1 = 0.5 * lr * cos(a) - 0.25 * lr * lr * cos(2 * a);
q2 = 0.5 * lr * sin(a) - 0.25 * lr * lr * sin(2 * a);
q15 = 0.5 * lr * cos(a);
q16 = 0.5 * lr * sin(a);
lm = pow(lr, m);
q13 = lm * cos(m * a);
q14 = lm * sin(m * a);
dv = dv + dt * glide / 22;
f = dv - int(dv);
wrapped = above(int(dv), cyc);
cyc = int(dv);
ma = m * a;
wma = ma - 6.2831853 * floor(ma / 6.2831853 + 0.5);
grot = grot - wma * (f - fprev + wrapped) + wrapped * ma;
grot = grot - 6.2831853 * floor(grot / 6.2831853 + 0.5);
fprev = f;
h = grot + 0.8 * sin(time * 0.07) + 0.35 * sin(time * 0.13 + 2);
pch = 0.85 + 0.38 * sin(time * 0.1 + 1) + 0.05 * sin(time * 0.23);
alt = 0.09 * pow(lr, -m * f);
back = alt / tan(pch);
lox = alt * 0.9 * cos(grot + time * 0.09);
loy = alt * 0.9 * sin(grot + time * 0.09);
q7 = q15 + lox - back * cos(h);
q8 = q16 + loy - back * sin(h);
q6 = alt;
q4 = h;
q10 = pch;
q27 = 0.16 * sin(time * 0.08) + 0.06 * sin(time * 0.19);
q11 = m * 0.015 * f;
w = min(max((f - 0.8) / 0.2, 0), 1);
q12 = w * w * (3 - 2 * w);
hue = hue + dt * 0.006;
q9 = hue;
""")

# Coral city (no cover) ---------------------------------------------------
# A flight through an endless coral foam: an Apollonian-style fractal (space
# folded into a period-2 lattice, then inverted in the unit sphere, eight
# times over), whose distance estimate keeps a sphere shell of every level,
# so the city is bubbles inside bubbles: domes, arches, pores and tunnels,
# all rounded. It is raymarched in full every frame (surface normals,
# ambient occlusion, a headlamp plus a fixed sun, fog into the theme's
# background, a near-miss glow). Points that land close to the inversion
# centre early are the city's lights; they flicker with the treble. Before
# folding, space is displaced by a smooth sine field that repeats with the
# lattice (period 2, so it stays seamless, also when the camera wraps); the
# music swells it and its phase flows with the loudness, so the reef sways
# and undulates. (Turning the point inside each fold instead tore the
# lattice apart at the cell edges.) The distance is divided by the field's
# Lipschitz bound so the march stays safe.
# The autopilot (martin's mandelbox explorer is the ancestor) runs the same
# distance estimate in EEL: it probes five directions each frame, steers
# towards open space, slows near walls and surges on kicks; its probes and
# turn rates are smoothed and capped, so it glides instead of whipping, and
# a slow wander in yaw, pitch and roll keeps it drifting like a swimmer. The
# scene is rendered in the warp and blended (max) over the previous frame,
# zoomed a touch with the flight speed, so lights and bright edges
# leave motion trails (longer on kicks); the comp adds bloom. Camera
# position q10-q12, forward q13-q15, up q16-q18; inversion strength q19,
# sway phase q20, sway amplitude q21. The position wraps inside one 2-unit cell (space repeats).
CORAL_ITERS = 8
def coral_de_eel(x, y, z, out, iters=CORAL_ITERS):
    """The distance estimate in EEL, mirroring coral_de() in the shader
    exactly (the autopilot must see the walls the viewer sees)."""
    fold = lambda v: f"{v} = -1 + 2 * (0.5 * {v} + 1000.5 - int(0.5 * {v} + 1000.5));"
    return f"""ex = {x}; ey = {y}; ez = {z};
dx = ex + q21 * sin(3.14159265 * ey + q20); dy = ey + q21 * sin(3.14159265 * ez + q20 * 1.3); dz = ez + q21 * sin(3.14159265 * ex + q20 * 0.7); scl = 1;
loop({iters},
  {fold("dx")}
  {fold("dy")}
  {fold("dz")}
  r2 = max(dx * dx + dy * dy + dz * dz, 0.000001);
  k = q19 / r2;
  dx = dx * k; dy = dy * k; dz = dz * k; scl = scl * k;
);
{out} = 0.25 * (sqrt(dx * dx + dy * dy + dz * dz) - 0.35) / scl / (1 + 3.14159265 * abs(q21));
"""
def coral_probe(ox, oy, oz, out):
    """March from the camera along (ox, oy, oz) with the distance estimate;
    `out` = how far is free, capped at 1."""
    return f"""pt = 0.005;
loop(18,
{coral_de_eel(f"q10 + ({ox}) * pt", f"q11 + ({oy}) * pt", f"q12 + ({oz}) * pt", "pd")}pt = min(pt + pd * 0.9, 1);
);
{out} = pt;
"""
def coral_funcs(iters):
    return f"""
float coral_de(vec3 pw, out float trap, out float lights) {{
  vec3 z = pw + q21 * sin(3.14159265 * vec3(pw.y, pw.z, pw.x) + vec3(q20, q20 * 1.3, q20 * 0.7));
  float scl = 1.0;
  trap = 1e9;
  lights = 0.0;
  for (int i = 0; i < {iters}; i++) {{
    z = -1.0 + 2.0 * fract(0.5 * z + 0.5);
    float r2 = max(dot(z, z), 0.000001);
    trap = min(trap, r2);
    if (i > 1 && i < 5 && r2 < 0.06) lights += 1.0;
    float k = q19 / r2;
    z *= k;
    scl *= k;
  }}
  return 0.25 * (length(z) - 0.35) / scl / (1.0 + 3.14159265 * abs(q21));
}}
float coral_d(vec3 pw) {{
  float a;
  float b;
  return coral_de(pw, a, b);
}}
"""
CORAL_FUNCS = coral_funcs(CORAL_ITERS)

presets["nokkvi - coral city"] = preset(
    {"decay": 0.0, "wave_a": 0.0, "zoom": 1.0},
    CORAL_FUNCS + " shader_body {\n" + HEAD + """
  vec2 p = (uv_orig - 0.5) * s;
  vec3 ro = vec3(q10, q11, q12);
  vec3 fw = normalize(vec3(q13, q14, q15));
  vec3 rt = normalize(cross(fw, vec3(q16, q17, q18)));
  vec3 up = cross(rt, fw);
  vec3 rd = normalize(fw + (p.x * rt + p.y * up) * 1.1);
  float pixang = 1.1 / min(texsize.x, texsize.y);
  float t = 0.002;
  float d = 1.0;
  float steps = 0.0;
  float hit = 0.0;
  for (int i = 0; i < 110; i++) {
    d = coral_d(ro + rd * t);
    if (d < t * pixang * 0.7) { hit = 1.0; break; }
    t += d * 0.95;
    steps += 1.0;
    if (t > 4.0) break;
  }
  vec3 pos = ro + rd * t;
  float trap;
  float lights;
  coral_de(pos, trap, lights);
  float e = max(t * pixang, 0.0005);
  vec3 n = normalize(vec3(
    coral_d(pos + vec3(e, 0.0, 0.0)) - coral_d(pos - vec3(e, 0.0, 0.0)),
    coral_d(pos + vec3(0.0, e, 0.0)) - coral_d(pos - vec3(0.0, e, 0.0)),
    coral_d(pos + vec3(0.0, 0.0, e)) - coral_d(pos - vec3(0.0, 0.0, e))));
  float ao = 0.0;
  for (int k = 1; k <= 4; k++) {
    float h = 0.012 * float(k) * float(k);
    ao += (h - coral_d(pos + n * h)) / h * (0.5 / float(k));
  }
  ao = clamp(1.0 - ao, 0.0, 1.0);
  vec3 sun = normalize(vec3(0.5, 0.8, -0.3));
  float dif = max(dot(n, sun), 0.0) * 0.9 + max(dot(n, -rd), 0.0) * 0.6 + 0.15 * (0.5 + 0.5 * n.y);
  float spc = pow(max(dot(reflect(rd, n), sun), 0.0), 24.0);
  float tone_x = clamp(sqrt(trap) * 0.9, 0.0, 1.0);
""" + ramp("surf", "0.15 + 0.75 * fract(tone_x + q23)") + """
  vec3 col = surf * (0.15 + dif * 1.05) * (0.3 + 0.7 * ao);
  col += NOKKVI_TEXT * spc * 0.35 * ao;
  float lit = clamp(lights, 0.0, 1.0) * smoothstep(0.07, 0.004, trap);
  col += mix(NOKKVI_WARM, NOKKVI_HIGHLIGHT, 0.5 + 0.5 * sin(pos.x * 3.0 + pos.z * 2.0))
         * lit * (0.7 + 1.0 * q24);
  float fog = 1.0 - exp(-t * (0.6 + 0.25 * q5));
  vec3 bg = mix(NOKKVI_BG, NOKKVI_SURFACE, 0.4 + 0.3 * p.y);
  col = mix(col, bg, hit > 0.5 ? fog : 1.0);
  col += NOKKVI_HIGHLIGHT * pow(steps / 110.0, 2.0) * (0.3 + 0.6 * q3);
  col *= 1.0 + 0.15 * q5;
  vec2 ez = 0.5 + (uv_orig - 0.5) * (1.0 - q25);
  vec3 prev = texture(sampler_main, ez).xyz * step(2.5, frame);
  ret = max(col, prev * q26 - 0.03);
 }""",
    " shader_body {\n" + HEAD + """
  vec2 p = (uv - 0.5) * s;
  vec3 m = texture(sampler_main, uv).xyz;
  vec3 b1 = GetBlur1(uv);
  vec3 b2 = GetBlur2(uv);
  vec3 col = m + max(b1 - 0.28, 0.0) * 0.6 + max(b2 - 0.22, 0.0) * (0.9 + 0.8 * q3);
  col *= 0.85 + 0.15 * smoothstep(1.2, 0.3, length(p));
  ret = col;
 }""",
    init="pulse = 0; pop = 0; q19 = 1.2; q20 = 0; q21 = 0.05; amp = 0.05; flow = 0; spd = 0.1; surge = 0;"
         " yawv = 0; pitchv = 0; rollv = 0; hue = 0; lite = 0; sf = 1; sr = 1; sl = 1; su = 1; sd = 1;"
         " fx = 0; fy = 0; fz = 1; ux = 0; uy = 1; uz = 0; "
         " tries = 0; q10 = 1; q11 = 1; q12 = 1; free = 0;\n"
         "while (exec2(\n"
         "  q10 = rand(200) / 100 - 1; q11 = rand(200) / 100 - 1; q12 = rand(200) / 100 - 1;\n"
         + coral_de_eel("q10", "q11", "q12", "free") +
         "  tries = tries + 1;\n"
         ", below(free, 0.03) * below(tries, 300)));\n",
    frame=PULSE + """dt = min(1 / max(fps, 1), 0.1);
loud = min((bass_att + mid_att + treb_att) / 3, 2);
amp = amp * 0.97 + 0.03 * (0.05 + 0.04 * min(mid_att, 2) + 0.07 * q3);
q21 = amp;
flow = flow + dt * (0.25 + 0.35 * loud);
q20 = flow;
q19 = 1.2 + 0.06 * sin(time * 0.05);
rx = fy * uz - fz * uy; ry = fz * ux - fx * uz; rz = fx * uy - fy * ux;
""" + coral_probe("fx", "fy", "fz", "pf")
    + coral_probe("fx + 0.45 * rx", "fy + 0.45 * ry", "fz + 0.45 * rz", "pr")
    + coral_probe("fx - 0.45 * rx", "fy - 0.45 * ry", "fz - 0.45 * rz", "pl")
    + coral_probe("fx + 0.45 * ux", "fy + 0.45 * uy", "fz + 0.45 * uz", "pu")
    + coral_probe("fx - 0.45 * ux", "fy - 0.45 * uy", "fz - 0.45 * uz", "pdn")
    + coral_de_eel("q10", "q11", "q12", "here") + """sf = sf * 0.9 + 0.1 * pf; sr = sr * 0.9 + 0.1 * pr; sl = sl * 0.9 + 0.1 * pl;
su = su * 0.9 + 0.1 * pu; sd = sd * 0.9 + 0.1 * pdn;
avg = (sr + sl + su + sd) / 4 + 0.001;
turn = 0.5 + 1.2 * min(max((0.45 - sf) / 0.35, 0), 1);
yt = min(max(turn * (sr - sl) / avg, -1.2), 1.2);
ptt = min(max(turn * (su - sd) / avg, -1.2), 1.2);
yawv = yawv * 0.97 + 0.03 * (yt + 0.3 * sin(time * 0.13) + 0.15 * sin(time * 0.31));
pitchv = pitchv * 0.97 + 0.03 * (ptt + 0.22 * sin(time * 0.11 + 1));
rollv = rollv * 0.98 + 0.02 * (0.3 * sin(time * 0.07) + 0.12 * sin(time * 0.17));
ya = min(max(yawv, -0.7), 0.7) * dt; pa = min(max(pitchv, -0.7), 0.7) * dt; ra = rollv * dt;
fx = fx + rx * ya + ux * pa; fy = fy + ry * ya + uy * pa; fz = fz + rz * ya + uz * pa;
fl = sqrt(fx * fx + fy * fy + fz * fz); fx = fx / fl; fy = fy / fl; fz = fz / fl;
ux = ux + rx * ra; uy = uy + ry * ra; uz = uz + rz * ra;
dd = ux * fx + uy * fy + uz * fz; ux = ux - dd * fx; uy = uy - dd * fy; uz = uz - dd * fz;
ul = sqrt(ux * ux + uy * uy + uz * uz); ux = ux / ul; uy = uy / ul; uz = uz / ul;
surge = max(surge * 0.97, q5 * 0.6);
spd = spd * 0.97 + 0.03 * min(sf * 0.6, here * 12) * (0.7 + 0.4 * loud + surge);
q10 = q10 + fx * spd * dt; q11 = q11 + fy * spd * dt; q12 = q12 + fz * spd * dt;
q10 = q10 - 2 * int(q10 / 2 + 30.5) + 60; q11 = q11 - 2 * int(q11 / 2 + 30.5) + 60; q12 = q12 - 2 * int(q12 / 2 + 30.5) + 60;
q13 = fx; q14 = fy; q15 = fz; q16 = ux; q17 = uy; q18 = uz;
q25 = min(spd * 0.12, 0.025) + 0.003;
q26 = min(0.72 + 0.1 * q3, 0.85);
hue = hue + dt * 0.006;
q23 = hue;
lite = max(lite * 0.85, min(treb - treb_att + 0.3, 1.5));
q24 = lite;
""")

# Coral dive (no cover) ---------------------------------------------------
# An endless dive into coral city's reef that keeps finding the same reef.
# The reef is the set left fixed by its own step F (fold into the period-2
# lattice, then invert: z -> s z / |z|^2, s = 1.2). F has a fixed point on
# the x axis, x* = (1 + sqrt(1 + s), 0, 0), congruent to (sqrt(1 + s) - 1,
# 0, 0) in the central cell, and there DF is s / |z|^2 times a reflection,
# so F twice is a pure magnification by L = (s / (sqrt(1 + s) - 1)^2)^2,
# about 26, with no turn: shrinking the camera's distance to x* by L shows
# the same reef again. The camera sinks towards x* along a direction the
# init search picks for open space at three scales (a golden-angle spiral
# over the sphere, turned by a random phase each load), drifting and banking a
# little (orientation is free: the seam only rescales); the last fifth of
# every cycle crossfades into the view from L times further out, which is
# where the next cycle starts. Every length in the render (march cutoff,
# AO steps, fog) scales with the camera's distance, and the view reaches
# only about six distances out. The dive runs at a
# distance of 0.006 or less, where F is close to linear over the whole view,
# so a cycle's two ends really match; the distance estimate runs 14
# iterations (coral city: 8) to resolve the reef at that scale. No sway here: it would break the self-similarity. Lights flicker
# with the treble, kicks brighten the near-miss glow, loudness sets the
# dive speed (smoothly), and the scene leaves max-blend trails with bloom.
DIVE_ITERS = 14
DIVE_VIEW = """
vec3 dive_view(vec3 cam, vec3 cf, vec3 cr, vec3 cu, vec2 sp, float zs) {
  vec3 dir = normalize(cf + (sp.x * cr + sp.y * cu) * 1.1);
  float pxa = 1.1 / min(texsize.x, texsize.y);
  float tt = 0.002 * zs;
  float dd = 1.0;
  float st = 0.0;
  float hh = 0.0;
  for (int i = 0; i < 120; i++) {
    dd = coral_d(cam + dir * tt);
    if (dd < tt * pxa * 0.7) { hh = 1.0; break; }
    tt += dd * 0.95;
    st += 1.0;
    if (tt > 4.0 * zs) break;
  }
  vec3 hp = cam + dir * tt;
  float trp;
  float lts;
  coral_de(hp, trp, lts);
  float ee = max(tt * pxa, 0.0005 * zs);
  vec3 nn = normalize(vec3(
    coral_d(hp + vec3(ee, 0.0, 0.0)) - coral_d(hp - vec3(ee, 0.0, 0.0)),
    coral_d(hp + vec3(0.0, ee, 0.0)) - coral_d(hp - vec3(0.0, ee, 0.0)),
    coral_d(hp + vec3(0.0, 0.0, ee)) - coral_d(hp - vec3(0.0, 0.0, ee))));
  float occ = 0.0;
  for (int k = 1; k <= 4; k++) {
    float hk = 0.012 * float(k) * float(k) * zs;
    occ += (hk - coral_d(hp + nn * hk)) / hk * (0.5 / float(k));
  }
  occ = clamp(1.0 - occ, 0.0, 1.0);
  vec3 sun = normalize(vec3(0.5, 0.8, -0.3));
  float dif = max(dot(nn, sun), 0.0) * 0.9 + max(dot(nn, -dir), 0.0) * 0.6 + 0.15 * (0.5 + 0.5 * nn.y);
  float spc = pow(max(dot(reflect(dir, nn), sun), 0.0), 24.0);
  float tx = clamp(sqrt(trp) * 0.9, 0.0, 1.0);
""" + ramp("surf", "0.15 + 0.75 * fract(tx + q23)") + """
  vec3 cc = surf * (0.15 + dif * 1.05) * (0.3 + 0.7 * occ);
  cc += NOKKVI_TEXT * spc * 0.35 * occ;
  float lit = clamp(lts, 0.0, 1.0) * smoothstep(0.07, 0.004, trp);
  cc += mix(NOKKVI_WARM, NOKKVI_HIGHLIGHT, 0.5 + 0.5 * sin(tx * 9.0)) * lit * (0.7 + 1.0 * q24);
  float fg = 1.0 - exp(-(tt / zs) * (0.6 + 0.25 * q5));
  vec3 bgc = mix(NOKKVI_BG, NOKKVI_SURFACE, 0.4 + 0.3 * sp.y);
  cc = mix(cc, bgc, hh > 0.5 ? fg : 1.0);
  cc += NOKKVI_HIGHLIGHT * pow(st / 120.0, 2.0) * (0.3 + 0.6 * q3);
  return cc;
}
"""
DIVE_S = 1.2
presets["nokkvi - coral dive"] = preset(
    {"decay": 0.0, "wave_a": 0.0, "zoom": 1.0},
    coral_funcs(DIVE_ITERS) + DIVE_VIEW + " shader_body {\n" + HEAD + """
  vec2 p = (uv_orig - 0.5) * s;
  p = vec2(p.x * cos(q27) - p.y * sin(q27), p.x * sin(q27) + p.y * cos(q27));
  vec3 ro = vec3(q10, q11, q12);
  vec3 fw = normalize(vec3(q13, q14, q15));
  vec3 rt = normalize(cross(fw, vec3(q16, q17, q18)));
  vec3 up = cross(rt, fw);
  vec3 xs = vec3(q22, 0.0, 0.0);
  vec3 col = dive_view(ro, fw, rt, up, p, q6);
  if (q28 > 0.0) {
    vec3 colb = dive_view(xs + (ro - xs) * q31, fw, rt, up, p, q6 * q31);
    col = mix(col, colb, q28);
  }
  col *= 1.0 + 0.15 * q5;
  vec2 ez = 0.5 + (uv_orig - 0.5) * (1.0 - q25);
  vec3 prev = texture(sampler_main, ez).xyz * step(2.5, frame);
  ret = max(col, prev * q26 - 0.03);
 }""",
    " shader_body {\n" + HEAD + """
  vec2 p = (uv - 0.5) * s;
  vec3 m = texture(sampler_main, uv).xyz;
  vec3 b1 = GetBlur1(uv);
  vec3 b2 = GetBlur2(uv);
  vec3 col = m + max(b1 - 0.28, 0.0) * 0.6 + max(b2 - 0.22, 0.0) * (0.9 + 0.8 * q3);
  col *= 0.85 + 0.15 * smoothstep(1.2, 0.3, length(p));
  ret = col;
 }""",
    init=f"""pulse = 0; pop = 0; hue = 0; lite = 0; glide = 0.8; dv = 0;
q19 = {DIVE_S}; q20 = 0; q21 = 0;
xsx = sqrt(1 + {DIVE_S}) - 1;
lam = {DIVE_S} / (xsx * xsx);
lam2 = lam * lam;
d0 = 0.006;
best = -1; bx = 0; by = 0; bz = 1; n = 0;
sph = rand(1000) / 1000 * 6.2831853;
loop(400,
  cz = 1 - (n + 0.5) / 200; cr = sqrt(max(1 - cz * cz, 0));
  cx = cr * cos(n * 2.3999632 + sph); cy = cr * sin(n * 2.3999632 + sph);
  n = n + 1;
""" + coral_de_eel("xsx + cx * d0", "cy * d0", "cz * d0", "s1", DIVE_ITERS)
    + coral_de_eel("xsx + cx * d0 / 5", "cy * d0 / 5", "cz * d0 / 5", "s2", DIVE_ITERS)
    + coral_de_eel("xsx + cx * d0 / lam2", "cy * d0 / lam2", "cz * d0 / lam2", "s3", DIVE_ITERS) + """
  sc = min(min(s1 / d0, s2 * 5 / d0), s3 * lam2 / d0);
  better = above(sc, best);
  best = if(better, sc, best);
  bx = if(better, cx, bx); by = if(better, cy, by); bz = if(better, cz, bz);
);
rfx = if(above(abs(bz), 0.8), 1, 0); rfz = 1 - rfx;
""",
    frame=PULSE + """dt = min(1 / max(fps, 1), 0.1);
loud = min((bass_att + mid_att + treb_att) / 3, 2);
glide = glide * 0.995 + 0.005 * (0.6 + 0.5 * loud);
dv = dv + dt * glide / 32;
f = dv - int(dv);
dist = d0 * pow(lam2, -f);
wa = 0.18 * sin(time * 0.05); wb = 0.14 * sin(time * 0.037 + 1);
ux0 = by * rfz - bz * 0; uy0 = bz * rfx - bx * rfz; uz0 = bx * 0 - by * rfx;
ul = sqrt(ux0 * ux0 + uy0 * uy0 + uz0 * uz0) + 0.0001; ux0 = ux0 / ul; uy0 = uy0 / ul; uz0 = uz0 / ul;
vx0 = by * uz0 - bz * uy0; vy0 = bz * ux0 - bx * uz0; vz0 = bx * uy0 - by * ux0;
ddx = bx + ux0 * wa + vx0 * wb; ddy = by + uy0 * wa + vy0 * wb; ddz = bz + uz0 * wa + vz0 * wb;
dl = sqrt(ddx * ddx + ddy * ddy + ddz * ddz); ddx = ddx / dl; ddy = ddy / dl; ddz = ddz / dl;
q10 = xsx + ddx * dist; q11 = ddy * dist; q12 = ddz * dist;
la = 0.12 * sin(time * 0.071); lb = 0.1 * sin(time * 0.053 + 2);
fx = -ddx + ux0 * la + vx0 * lb; fy = -ddy + uy0 * la + vy0 * lb; fz = -ddz + uz0 * la + vz0 * lb;
fl = sqrt(fx * fx + fy * fy + fz * fz); fx = fx / fl; fy = fy / fl; fz = fz / fl;
q13 = fx; q14 = fy; q15 = fz;
q16 = vx0; q17 = vy0; q18 = vz0;
q27 = 0.15 * sin(time * 0.06) + 0.05 * sin(time * 0.17);
q6 = dist * 1.5;
q22 = xsx;
q31 = lam2;
w = min(max((f - 0.8) / 0.2, 0), 1);
q28 = w * w * (3 - 2 * w);
q25 = 0.004 + 0.004 * glide;
q26 = min(0.72 + 0.1 * q3, 0.85);
hue = hue + dt * 0.006;
q23 = hue;
lite = max(lite * 0.85, min(treb - treb_att + 0.3, 1.5));
q24 = lite;
""")

# Infinity (no cover) ------------------------------------------------------
# An endless corridor of luminous hoops, flown through by a camera that
# glides, rolls and breathes, receding into a slowly turning haze. The idea
# is martin's "infinity (2010 update)" (feedback copies on planes at depth
# t = fract(n/10 - q9), screen scale 1/t, pan and roll); nothing of its code
# is used. Here every plane is an analytic shape drawn with a pixel-wide
# anti-aliased edge and a bounded glow (no textures, no 1/r kernels),
# composited nearest first with "over" and fogged into the theme's background
# with depth, so near hoops hide far ones where they overlap.
# Depth: the camera has flown D plane spacings (a user variable, wrapped
# modulo INF_P). Slot i of INF_N sits at depth z = (i + 1 - fract(D)) / N and
# holds the plane whose identity is u = floor(D) + i + 1, constant while it
# flies, so its colour, lobes and turn never change on the way in; every
# function of u is periodic in INF_P, so nothing jumps when D wraps. A plane
# fades in at the far end and out before it reaches the camera.
# The corridor curves: plane u is centred on a smooth path C(u) (harmonics
# periodic in INF_P) and the camera rides the path looking along its tangent,
# so the near hoops stay centred while the far ones swing aside. The twist
# of plane u is psi + tw * N * z, with psi += tw * dD: constant for a flying
# plane, eased when tw changes. Camera speed, roll rate, pan, focal length,
# twist and the path's bend are all eased twice (critically damped), toward
# goals re-drawn on the clock, never on a beat. Beats drive brightness and a
# speed surge only.
# The vanishing point: faint filaments of domain-warped fbm (ridged, so they
# read as wisps rather than cloud) in three layers that each zoom in by 8x
# over their life and fade in and out (sin weights), phased by the flown
# distance, so the haze streams toward the viewer without an angle seam; a
# layer changes its pattern only while its weight is 0. The scene is
# rendered in the warp and kept as max-blend trails zoomed toward the
# vanishing point; the comp adds bloom and short, soft god rays from the core.
# q map: q3 kick pulse, q5 pop, q7/q8 cos/sin roll, q9 fract(D), q10
# floor(D), q11/q12 path at the camera, q13/q14 path tangent at the camera
# (per plane spacing), q15/q16 pan, q17 focal length, q18 psi, q19 tw, q20/q21 path amplitudes,
# q22 hoop breathing phase, q23 hue drift, q24 haze scroll, q25 core light,
# q26/q27 vanishing point (camera plane units), q28 trail zoom, q29 trail
# keep. Texture coordinates are the same in warp and comp, so q26/q27 map to
# one uv in both (the comp's uv.y flip is internal to its preamble).
INF_N = 16
INF_P = 64
INF_W = f"{6.2831853 / INF_P:.9f}"
INF_MOTIF = "hoop"
def inf_path_eel(u, ox, oy):
    return (f"{ox} = q20 * cos({INF_W} * ({u})) + q21 * cos(3 * {INF_W} * ({u}) + 1.3);\n"
            f"{oy} = q20 * sin({INF_W} * ({u})) + q21 * sin(2 * {INF_W} * ({u}) + 0.4);\n")
def ease(v, goal, tau):
    """Two cascaded first-order lags (critically damped): no step in velocity."""
    return (f"{v}_m = {v}_m + ({goal} - {v}_m) * (1 - exp(-dt / {tau}));\n"
            f"{v} = {v} + ({v}_m - {v}) * (1 - exp(-dt / {tau}));\n")
def retarget(timer, lo, span, *sets):
    """Every lo..lo+span seconds, re-draw goals (`sets` = (var, expr) pairs)."""
    out = f"{timer} = {timer} - dt;\n{timer}_n = below({timer}, 0);\n"
    for var, expr in sets:
        out += f"{var} = if({timer}_n, {expr}, {var});\n"
    return out + f"{timer} = if({timer}_n, {lo} + rand(1000) / 1000 * {span}, {timer});\n"
INF_FUNCS = f"""
vec2 inf_path(float pu) {{
  return vec2(q20 * cos({INF_W} * pu) + q21 * cos(3.0 * {INF_W} * pu + 1.3),
              q20 * sin({INF_W} * pu) + q21 * sin(2.0 * {INF_W} * pu + 0.4));
}}
float inf_hash(float hu) {{
  return fract(sin(hu * 12.9898 + 4.1) * 43758.5453);
}}
vec3 inf_ramp(float rx) {{""" + ramp("rr", "rx") + """
  return rr;
}
float inf_hoop(vec2 hc, float hh) {
  float hr = length(hc);
  float ha = atan(hc.y, hc.x);
  float lobes = 3.0 + floor(hh * 3.0);
  float amp = 0.028 * (0.55 + 0.45 * sin(q22 + hh * 6.2831853));
  float rad0 = 0.30 + 0.05 * (hh - 0.5);
  return abs(hr - rad0 - amp * sin(lobes * ha + hh * 6.2831853)) / (1.0 + lobes * amp / rad0);
}
float inf_frame(vec2 fc, float fh) {
  vec2 fb = vec2(0.30, 0.22 + 0.08 * fh);
  vec2 fq = abs(fc) - fb + 0.06;
  float fd = length(max(fq, 0.0)) + min(max(fq.x, fq.y), 0.0) - 0.06;
  return abs(fd);
}
float inf_fbm(vec2 nc) {
  float fs = 0.0;
  float fa = 0.5;
  vec2 fx = nc;
  for (int o = 0; o < 4; o++) {
    fs += fa * texture(sampler_noise_hq, fx).x;
    fx = fx * 2.0 + vec2(0.37, 0.61);
    fa *= 0.5;
  }
  return fs;
}
float inf_haze(vec2 tc) {
  float hs = 0.0;
  for (int l = 0; l < 3; l++) {
    float lp = q24 + float(l) / 3.0;
    float ph = fract(lp);
    float lid = mod(floor(lp), 4.0) * 3.0 + float(l);
    float lsc = 0.05 * exp2(3.0 * (1.0 - ph));
    float la = lid * 2.39996;
    vec2 lc = vec2(tc.x * cos(la) - tc.y * sin(la), tc.x * sin(la) + tc.y * cos(la)) * lsc
              + vec2(0.31, 0.17) * lid;
    float wn = inf_fbm(lc * 0.6 + vec2(0.5, 0.2));
    vec2 wc = lc + vec2(wn, -wn) * 0.35;
    hs += inf_fbm(wc) * sin(3.14159265 * ph);
  }
  return hs / 1.5;
}
"""
def infinity():
    shape = {"hoop": "inf_hoop", "frame": "inf_frame"}[INF_MOTIF]
    warp = INF_FUNCS + " shader_body {\n" + HEAD + f"""
  vec2 sp = (uv_orig - 0.5) * s + vec2(q15, q16);
  vec2 p = vec2(sp.x * q7 + sp.y * q8, -sp.x * q8 + sp.y * q7);
  float pix = 1.0 / min(texsize.x, texsize.y);
  vec3 acc = vec3(0.0);
  float cov = 0.0;
  for (int i = 0; i < {INF_N}; i++) {{
    float z = (float(i) + 1.0 - q9) / {INF_N}.0;
    float w = smoothstep(0.0, 0.12, z) * smoothstep(1.0, 0.72, z);
    float u = q10 + float(i) + 1.0;
    vec2 ctr = inf_path(u) - vec2(q11, q12) - (u - q10 - q9) * vec2(q13, q14);
    vec2 c = p * (q17 * z) - ctr;
    float th = q18 + q19 * {INF_N}.0 * z;
    c = vec2(c.x * cos(th) - c.y * sin(th), c.x * sin(th) + c.y * cos(th));
    float h = inf_hash(mod(u, {INF_P}.0));
    float pw = q17 * z * pix;
    float d = {shape}(c, h) - 0.0035;
    float line = smoothstep(pw, -pw, d);
    float gs = 0.008 + pw;
    float glow = gs * gs / (d * d + gs * gs);
    vec3 pcol = inf_ramp(0.25 + 0.7 * fract(h * 0.73 + q23));
    float fog = 1.0 - exp(-2.2 * z);
    pcol = mix(pcol, vec3(dot(pcol, {LUM})), 0.45 * z);
    vec3 lcol = mix(pcol * (1.0 + 0.35 * q5 + 0.2 * q3), NOKKVI_BG, fog * 0.8);
    float a = w * line;
    acc += (1.0 - cov) * (a * lcol + w * glow * 0.12 * (1.0 - line) * lcol);
    cov += (1.0 - cov) * a;
  }}
  vec2 hp = p - vec2(q26, q27);
  float hr = length(hp) + 0.0001;
  float hz = inf_haze(hp);
  float hzf = smoothstep(0.42, 0.06, hr);
  float fil = pow(1.0 - abs(2.0 * clamp(hz, 0.0, 1.0) - 1.0), 5.0);
""" + ramp("hzc", "0.15 + 0.75 * smoothstep(0.2, 0.8, hz)") + f"""
  vec3 back = mix(NOKKVI_BG, NOKKVI_SURFACE, 0.35 * hzf * smoothstep(0.2, 0.7, hz));
  back += hzc * fil * hzf * (0.35 + 0.25 * q25);
  float cg = 0.02 / (0.02 + hr * hr * 40.0);
  back += mix(NOKKVI_ACCENT, NOKKVI_TEXT, 0.3) * cg * (0.12 + 0.18 * q25);
  vec3 col = acc + (1.0 - cov) * back;
  col = 1.0 - exp(-1.6 * col);
  vec2 vp = vec2(q26 * q7 - q27 * q8, q26 * q8 + q27 * q7) - vec2(q15, q16);
  vec2 vuv = vp / s + 0.5;
  vec2 ez = vuv + (uv_orig - vuv) * (1.0 - q28);
  ez = clamp(ez, 0.0, 1.0);
  vec3 prev = texture(sampler_main, ez).xyz * step(2.5, frame);
  ret = max(col, prev * q29 - 0.03);
 }}"""
    comp = " shader_body {\n" + HEAD + """
  vec2 p = (uv - 0.5) * s;
  vec3 m = texture(sampler_main, uv).xyz;
  vec3 b1 = GetBlur1(uv);
  vec3 b2 = GetBlur2(uv);
  vec3 col = m + max(b1 - 0.25, 0.0) * 0.5 + max(b2 - 0.2, 0.0) * (0.7 + 0.6 * q3);
  vec2 vp = vec2(q26 * q7 - q27 * q8, q26 * q8 + q27 * q7) - vec2(q15, q16);
  vec2 vuv = vp / s + 0.5;
  vec3 rays = vec3(0.0);
  for (int j = 1; j <= 10; j++) {
    float rf = float(j) / 10.0;
    rays += GetBlur2(uv + (vuv - uv) * rf * 0.35) * (1.0 - rf);
  }
  float rl = dot(rays, vec3(0.299, 0.587, 0.114)) / 5.5;
  col += NOKKVI_ACCENT * max(rl - 0.12, 0.0) * (0.35 + 0.3 * q25);
  col *= 0.85 + 0.15 * smoothstep(1.2, 0.3, length(p));
  ret = col;
 }"""
    init = ("pulse = 0; pop = 0; dd = rand(1000) / 1000 * " + str(INF_P) + "; psi = 0; tw = 0; tw_m = 0; twg = 0.06;"
            " spd = 1.2; spd_m = 1.2; roll = rand(1000) / 1000 * 6.2831853; rw = 0; rw_m = 0; rwg = 0.1;"
            " px = 0; px_m = 0; py = 0; py_m = 0; pgx = 0; pgy = 0; foc = 1.3; foc_m = 1.3; focg = 1.3;"
            " a1 = 0.25; a1_m = 0.25; a1g = 0.25; a2 = 0.05; a2_m = 0.05; a2g = 0.05;"
            " ttw = 0; trw = 0; tpan = 0; tfoc = 0; tbend = 0; breath = 0; hue = rand(1000) / 1000; core = 0;")
    frame = PULSE + "dt = min(1 / max(fps, 1), 0.1);\n" \
        "loud = min((bass_att + mid_att + treb_att) / 3, 2);\n" \
        + ease("spd", "0.9 + 0.9 * loud", "1.0") + \
        "fly = max(spd * (1 + 0.45 * q3), 0.25);\n" \
        f"dd = dd + fly * dt;\ndd = dd - {INF_P} * floor(dd / {INF_P});\n" \
        + retarget("ttw", 8, 5, ("twg", "(rand(1000) / 1000 * 2 - 1) * 0.12")) \
        + ease("tw", "twg", "2.5") + \
        "psi = psi + tw * fly * dt;\npsi = psi - 6.2831853 * floor(psi / 6.2831853);\n" \
        + retarget("trw", 6, 4, ("rwg", "(rand(1000) / 1000 * 2 - 1) * 0.25")) \
        + ease("rw", "rwg", "1.5") + \
        "roll = roll + rw * dt;\nroll = roll - 6.2831853 * floor(roll / 6.2831853);\n" \
        + retarget("tpan", 5, 3, ("pgx", "(rand(1000) / 1000 * 2 - 1) * 0.06"),
                   ("pgy", "(rand(1000) / 1000 * 2 - 1) * 0.05")) \
        + ease("px", "pgx", "1.4") + ease("py", "pgy", "1.4") \
        + retarget("tfoc", 9, 6, ("focg", "1.05 + rand(1000) / 1000 * 0.5")) \
        + ease("foc", "focg", "3") \
        + retarget("tbend", 12, 6, ("a1g", "0.08 + rand(1000) / 1000 * 0.2"),
                   ("a2g", "rand(1000) / 1000 * 0.04")) \
        + ease("a1", "a1g", "3.5") + ease("a2", "a2g", "3.5") + \
        "q20 = a1; q21 = a2;\n" \
        + inf_path_eel("dd", "gx0", "gy0") \
        + f"tx = -q20 * {INF_W} * sin({INF_W} * dd) - q21 * 3 * {INF_W} * sin(3 * {INF_W} * dd + 1.3);\n" \
        f"ty = q20 * {INF_W} * cos({INF_W} * dd) + q21 * 2 * {INF_W} * cos(2 * {INF_W} * dd + 0.4);\n" \
        + inf_path_eel(f"dd + {INF_N}", "gxf", "gyf") + \
        f"q26 = (gxf - gx0 - {INF_N} * tx) / foc;\nq27 = (gyf - gy0 - {INF_N} * ty) / foc;\n" \
        "q7 = cos(roll); q8 = sin(roll);\n" \
        "q9 = dd - floor(dd); q10 = floor(dd);\n" \
        "q11 = gx0; q12 = gy0; q13 = tx; q14 = ty;\n" \
        "q15 = px; q16 = py; q17 = foc; q18 = psi; q19 = tw;\n" \
        "breath = breath + dt * 0.21;\nbreath = breath - 6.2831853 * floor(breath / 6.2831853);\nq22 = breath;\n" \
        "hue = hue + dt * 0.005;\nhue = hue - floor(hue);\nq23 = hue;\n" \
        f"q24 = dd / {INF_N};\n" \
        "core = core + (min(bass_att, 2) - core) * (1 - exp(-dt / 0.4));\nq25 = core;\n" \
        "q28 = min(0.0012 + 0.0022 * fly, 0.012);\n" \
        "q29 = min(0.66 + 0.1 * q3, 0.8);\n"
    return preset({"decay": 0.0, "wave_a": 0.0, "zoom": 1.0}, warp, comp, init=init, frame=frame)

presets["nokkvi - infinity"] = infinity()

presets["nokkvi - black holes"] = black_holes_port(False)
presets["nokkvi - cover black holes"] = black_holes_port(True)
presets["nokkvi - maxawow"] = maxawow_port(False)
presets["nokkvi - cover maxawow"] = maxawow_port(True)

for name, p in presets.items():
    json.dump(p, open(os.path.join(OUT, name + ".json"), "w"), indent=1)
print(len(presets))
