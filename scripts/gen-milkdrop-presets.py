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
def ease(v, goal, tau):
    """Two cascaded first-order lags (critically damped): no step in velocity."""
    return (f"{v}_m = {v}_m + ({goal} - {v}_m) * (1 - exp(-dt / {tau}));\n"
            f"{v} = {v} + ({v}_m - {v}) * (1 - exp(-dt / {tau}));\n")
# Travel speed from the music, in `spdt` (infinity's flight, julia lace's
# dive): loudness (the bands over their recent average) sets a cruising
# speed of 0.25 to 3.5 that eases over ~1 s, and kicks add a short surge on
# top (decays over ~0.4 s). Runs after PULSE (reads q3, q5) and `dt`.
SPEED_INIT = "spd = 1; spd_m = 1; surge = 0; sg = 0; sg_m = 0; spdt = 1;"
SPEED = ("en = min((1.2 * bass_att + mid_att + 0.8 * treb_att) / 3, 2);\n"
         + ease("spd", "min(0.25 + 1.1 * en * en, 3.5)", "0.8")
         + "surge = max(surge * exp(-dt / 0.4), 1.6 * q5 + 0.9 * q3);\n"
         + ease("sg", "surge", "0.07")
         + "spdt = spd + sg;\n")
# The last three beats (bass above its follower, 0.2 s cooldown) as a ring
# buffer of ages (ba1..ba3, seconds, 9 = long ago) and strengths (bs1..bs3,
# 0.5 to 1.6), newest first, for presets that send something out per beat
# (infinity's inlay waves, julia lace's ripples). Needs `dt`.
BEATS_INIT = "sinceb = 1; ba1 = 9; ba2 = 9; ba3 = 9; bs1 = 0; bs2 = 0; bs3 = 0;"
BEATS = ("bk = bass - bass_att; sinceb = sinceb + dt;\n"
         "trig = above(bk, 0.22) * above(sinceb, 0.2);\n"
         "ba3 = if(trig, ba2, ba3); bs3 = if(trig, bs2, bs3);\n"
         "ba2 = if(trig, ba1, ba2); bs2 = if(trig, bs1, bs2);\n"
         "ba1 = if(trig, 0, ba1); bs1 = if(trig, min(0.5 + bk * 1.4, 1.6), bs1);\n"
         "sinceb = if(trig, 0, sinceb);\n"
         "ba1 = ba1 + dt; ba2 = ba2 + dt; ba3 = ba3 + dt;\n")

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
# An endless dive into a Julia set's spiral vortex, the set drawn as fine
# lace (the distance estimate turns its boundary into hairline ridges with a
# soft glow) lying in a dark, glossy liquid seen in perspective. Its
# parameter c = l/2 - l^2/4 sits just outside the Mandelbrot set's main
# cardioid, where the Julia set is all spirals: l is the multiplier of the
# fixed point alpha = l/2, |l| = 1.03 so alpha repels and the spirals wind
# around it. The Julia set maps onto itself under z -> z^2 + c, which near
# alpha is z -> alpha + l (z - alpha): zooming in by |l|^M while turning by
# M arg(l) shows the same picture again. So the camera looks down at alpha
# and sinks towards it, and after a zoom of |l|^M (about 3x) it jumps back
# up by exactly that map (height times |l|^M, heading plus M arg(l), both
# kept in `grot`), one ring deeper; the last fifth of every cycle crossfades
# into the next cycle's view (the same window mapped by l^M, drawn a second
# time), so the seam never shows even where the map is not quite linear.
# Colour follows the iteration count shifted by M per cycle, so it matches
# across the seam too. Between wraps the dive is a pure zoom: the spirals
# twist by their own geometry, and the camera only drifts (a swing between
# a steep dive and a horizon-grazing glide, a slow sweep around the vortex,
# a gentle bank), aiming a bounded angle off the vortex so it stays framed.
# The shape morphs: arg(l) (in turns) glides (tau 5 s) between JULIA_STOPS,
# a curated walk over the family, from one elegant spiral (0.035) through
# feathery stars (0.09), sunflowers (0.19) and rosettes (0.3) to starbursts
# (0.42); it dwells 18-32 s per stop and turns back 12% of the time. Near a
# fraction p/q with q <= 9 (1/2, 1/3, 1/4, 2/5, 3/7, 1/8, 4/9...) the set
# collapses into q straight arms over a void (c lands in or beside that
# bulb), so every stop keeps |arg - p/q| q^2 above ~0.12 and a glide only
# sweeps through a star for a few seconds. The engine seeds rand from the
# preset text, so the walk would replay one route; its start and its turns
# also hash the music at that moment.
# The liquid: its swells are four octaves of noise in camera space
# (u = the ground point about alpha, turned by -grot, over the height), each
# octave |l|^M apart and weighted by where the cycle is (q17), so every
# octave rides the zoom with the lace and the ladder repeats exactly across
# the wrap; the octaves drift (q19, faster as the dive speeds up). The noise
# is sampler_noise_hq read straight (it is already smooth, a cubic lattice
# every 8 texels) with slopes over 2 texels: the texel-snapping quintic of
# tnoise zeroes the slope at every texel edge and drew the texel grid into
# the glints as straight streaks. The surface refracts the lace (a lookup
# offset by its slope, scaled by the distance to the vortex so it stays in
# proportion to the lace's detail), focuses light into caustics on it (where
# the swells curve, brighter with the bass, q30), absorbs it with the path
# length (grazing rays see less), reflects a sky gradient by Fresnel, and
# glints where it mirrors a low moon that turns with `grot`. Rays above the
# horizon see that sky. The music never touches the shape (a Julia set is
# so sensitive to c that a beat-driven nudge reads as a glitch): loudness
# sets the dive speed and kicks surge it (SPEED; a cycle takes ~17 s at a
# typical speed of 2.4), and every beat (BEATS) sends a ripple ring out
# from the vortex across the liquid (in log radius, so it looks the same at
# any depth), which bends the lace, catches the light, glows faintly on the
# water and lights the lace it passes (both fade in as the ring leaves the
# vortex, so the dense centre never whites out); kicks also brighten the
# lace's glow and the glints. Line widths follow each pixel's footprint on
# the plane (screen derivatives), and the lace, rings and fine swells fade
# where a pixel covers too much of them.
# The scene is drawn in the warp; the comp adds bloom. q map: q1/q2 c, q3
# kick, q4/q10 view heading and pitch, q5 pop, q6 height, q7/q8 camera,
# q9 hue drift, q11 seam hue shift, q12 seam crossfade, q13/q14 l^M,
# q15/q16 alpha, q17 cycle fraction, q18 grot, q19 liquid drift, q20-q25
# beat ages and strengths, q26 light heading, q27 bank, q30 bass glow.
JULIA_ITERS = 240
JULIA_M = 37
JULIA_STOPS = [0.035, 0.05, 0.07, 0.09, 0.12, 0.155, 0.185, 0.225,
               0.265, 0.297, 0.315, 0.36, 0.385, 0.415, 0.452, 0.47]
JULIA_FN = f"""
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
# The liquid shared with the fractal voyage: swell octaves, beat rings, sky.
LIQUID_FNS = f"""float jl_h(vec2 jhv) {{
  vec2 jha = jhv * 0.0156 + vec2(q19 * 0.0011, q19 * 0.0004);
  vec2 jhb = jhv * 0.0273 + vec2(-q19 * 0.0009, q19 * 0.0013);
  return texture(sampler_noise_hq, jha).x * 0.6 + texture(sampler_noise_hq, jhb).x * 0.4;
}}
vec3 jl_swell(vec2 jsu, float jspix) {{
  vec3 jss = vec3(0.0);
  float jslm = length(vec2(q13, q14));
  for (int i = 0; i < 4; i++) {{
    float jsk = float(i) - 2.0 + q17;
    float jsc = pow(jslm, -jsk);
    vec2 jsv = jsu * jsc;
    vec2 jsvx = jsv + vec2(0.3, 0.0);
    vec2 jsvy = jsv + vec2(0.0, 0.3);
    vec2 jsvxm = jsv - vec2(0.3, 0.0);
    vec2 jsvym = jsv - vec2(0.0, 0.3);
    float jh0 = jl_h(jsv);
    float jhx = jl_h(jsvx);
    float jhy = jl_h(jsvy);
    float jhxm = jl_h(jsvxm);
    float jhym = jl_h(jsvym);
    float jsw = 0.5 + 0.5 * cos(1.5707963 * jsk);
    float jsaa = smoothstep(0.4, 0.15, jspix * jsc);
    jss += vec3((jhx - jhxm) * 1.667, (jhy - jhym) * 1.667, (jhx + jhxm + jhy + jhym - 4.0 * jh0) * 11.11) * (jsw * jsaa);
  }}
  return jss;
}}
float jl_ring(float jrr, float jra, float jrs) {{
  float jrf = -2.3 + jra * 2.8;
  float jrx = jrr - jrf;
  return jrs * exp(-jra * 1.2) * sin(jrx * 12.0) * exp(-jrx * jrx * 14.0);
}}
float jl_glow(float jgr, float jga, float jgs) {{
  float jgf = -2.3 + jga * 2.8;
  float jgx = jgr - jgf;
  return jgs * exp(-jga * 1.3) * exp(-jgx * jgx * 30.0) * smoothstep(-2.3, -1.0, jgf);
}}
vec3 jl_env(vec3 jed) {{
  vec3 jel = vec3(0.98 * cos(q26), 0.98 * sin(q26), 0.2);
  float jez = jed.z;
  vec3 jhor = mix(NOKKVI_SURFACE, NOKKVI_ACCENT, 0.2);
  vec3 jsky = mix(jhor * 0.85, NOKKVI_BG * 0.6, smoothstep(0.0, 0.5, jez));
  float jsun = max(dot(jed, jel), 0.0);
  vec3 jlc = mix(NOKKVI_TEXT, NOKKVI_HIGHLIGHT, 0.35);
  jsky += jlc * (pow(jsun, 600.0) * 0.9 + pow(jsun, 12.0) * 0.07);
  jsky += mix(NOKKVI_ACCENT, NOKKVI_WARM, 0.35) * exp(-abs(jez) * 14.0) * (0.12 + 0.08 * q30);
  return jsky;
}}
"""
JULIA_FUNCS = JULIA_FN + LIQUID_FNS
def julia_shade(v, w0, hueshift, light="0.0"):
    """Lace colour `v` for the ground point `w0` (a vec2 expression) over
    `ground`; `light` (a float expression) adds a wave of light."""
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
  {v} += {v}_lace * {v}_glow * (0.3 + 0.35 * q3 + 0.9 * ({light})) * {v}_shade;
  {v} = mix({v}, {v}_lace * (0.9 + 0.3 * q3 + 0.45 * ({light})) * {v}_shade, {v}_line);
  {v} = mix({v}, NOKKVI_TEXT, {v}_line * smoothstep(0.6, 1.0, {v}_glow) * 0.2);
"""
def julia_stop_pick(var, idx):
    e = f"{JULIA_STOPS[-1]}"
    for i in range(len(JULIA_STOPS) - 2, -1, -1):
        e = f"if(equal({idx}, {i}), {JULIA_STOPS[i]}, {e})"
    return f"{var} = {e};\n"
JULIA_WARP = JULIA_FUNCS + " shader_body {\n" + HEAD + """
  vec2 p = (uv_orig - 0.5) * s;
  p = vec2(p.x * cos(q27) - p.y * sin(q27), p.x * sin(q27) + p.y * cos(q27));
  float ha = q4;
  float pt = q10;
  vec3 fw = vec3(cos(pt) * cos(ha), cos(pt) * sin(ha), -sin(pt));
  vec3 rt = vec3(sin(ha), -cos(ha), 0.0);
  vec3 up = cross(rt, fw);
  vec3 rd = normalize(fw + (p.x * rt + p.y * up) * 1.15);
  float down = -rd.z;
  float dn = max(down, 0.004);
  vec2 wa0 = vec2(q7, q8) + rd.xy * (q6 / dn);
  vec2 c = vec2(q1, q2);
  vec2 al = vec2(q15, q16);
  float cg = cos(q18);
  float sn = sin(q18);
  vec2 rel0 = wa0 - al;
  vec2 lu = vec2(cg * rel0.x + sn * rel0.y, cg * rel0.y - sn * rel0.x) / q6;
  float lr = length(lu) + 1e-4;
  float lpix = max(length(dFdx(lu)), length(dFdy(lu))) + 1e-6;
  vec3 lsw = jl_swell(lu, lpix);
  vec2 lsl = lsw.xy * 0.13;
  float caus = pow(max(-lsw.z * 0.12, 0.0), 2.0);
  float lrr = log(lr);
  float rfade = smoothstep(0.1, 0.035, lpix / lr);
  float rsl = jl_ring(lrr, q20, q21) + jl_ring(lrr, q22, q23) + jl_ring(lrr, q24, q25);
  lsl += (lu / lr) * (rsl * 0.16 * rfade);
  float rgl = (jl_glow(lrr, q20, q21) + jl_glow(lrr, q22, q23) + jl_glow(lrr, q24, q25)) * rfade;
  vec2 wsl = vec2(cg * lsl.x - sn * lsl.y, sn * lsl.x + cg * lsl.y);
  vec3 n = normalize(vec3(-wsl.x, -wsl.y, 1.0));
  vec2 wa = wa0 - wsl * (q6 * lr * 0.22);
  vec3 ground = mix(NOKKVI_BG, NOKKVI_ACCENT, 0.07) * 0.7;
""" + julia_shade("ca", "wa", "q11", "rgl") + """
  vec3 lace = ca;
  if (q12 > 0.0) {
    vec2 rel = wa - al;
    vec2 wb = al + vec2(q13 * rel.x - q14 * rel.y, q13 * rel.y + q14 * rel.x);
""" + julia_shade("cb", "wb", f"q11 - {JULIA_M}.0 * 0.015", "rgl") + """
    lace = mix(ca, cb, q12);
  }
  float lfar = smoothstep(0.06, 0.02, lpix / lr);
  lace = mix(ground, lace, lfar);
  lace *= 1.0 + caus * (0.8 + 0.5 * q30) * lfar;
  lace += mix(NOKKVI_ACCENT, NOKKVI_TEXT, 0.35) * caus * 0.09 * lfar;
  float cosv = max(dot(n, -rd), 0.0);
  float fre = 0.02 + 0.98 * pow(1.0 - cosv, 5.0);
  vec3 rfl = reflect(rd, n);
  rfl = vec3(rfl.x, rfl.y, abs(rfl.z));
  vec3 env = jl_env(rfl);
  vec3 lgt = vec3(0.98 * cos(q26), 0.98 * sin(q26), 0.2);
  float sdot = max(dot(rfl, lgt), 0.0);
  float spec = pow(sdot, 2200.0) * 1.1 + pow(sdot, 220.0) * 0.05;
  float trans = exp(-0.15 * (1.0 / dn - 1.0));
  vec3 col = lace * trans * (1.0 - fre) + env * fre + mix(NOKKVI_TEXT, NOKKVI_HIGHLIGHT, 0.35) * spec * (0.8 + 0.8 * q3);
  vec3 sky = jl_env(rd);
  col += mix(NOKKVI_ACCENT, NOKKVI_HIGHLIGHT, 0.3) * rgl * 0.3 * (1.0 - 0.6 * fre);
  col = mix(sky, col, smoothstep(0.0, 0.01, down));
  ret = col;
 }"""
JULIA_COMP = " shader_body {\n" + HEAD + """
  vec2 p = (uv - 0.5) * s;
  vec3 m = texture(sampler_main, uv).xyz;
  vec3 b1 = GetBlur1(uv);
  vec3 b2 = GetBlur2(uv);
  vec3 col = m + max(b1 - 0.4, 0.0) * 0.3 + max(b2 - 0.3, 0.0) * (0.2 + 0.25 * q3);
  col *= 0.82 + 0.18 * smoothstep(1.3, 0.3, length(p));
  ret = col;
 }"""
JULIA_INIT = ("pulse = 0; pop = 0; hue = 0; dv = 0; cyc = 0; grot = 0; flw = 0; core = 1;"
              f" jfc = 0; jidx = 0; jdir = 1; jt = 24; jph = {JULIA_STOPS[0]}; jph_m = jph; "
              + SPEED_INIT + " " + BEATS_INIT)
JULIA_FRAME = PULSE + "dt = min(1 / max(fps, 1), 0.1);\n" + SPEED + BEATS + f"""m = {JULIA_M};
lr = 1.03;
jfc = jfc + 1;
jnew = equal(jfc, 2);
jseed = bass * 5.31 + mid * 2.13 + treb * 1.71 + rand(1);
jseed = jseed - floor(jseed);
jidx = if(jnew, int(jseed * {len(JULIA_STOPS)}), jidx);
jdir = if(jnew, if(below(jseed * 7.3 - floor(jseed * 7.3), 0.5), -1, 1), jdir);
jt = jt - dt;
jchg = below(jt, 0);
jflp = rand(1) + bass * 1.37 + treb * 2.9;
jdir = if(jchg * below(jflp - floor(jflp), 0.12), -jdir, jdir);
jnx = jidx + jdir;
jdir = if(jchg * (below(jnx, 0) + above(jnx, {len(JULIA_STOPS) - 1})), -jdir, jdir);
jidx = if(jchg, jidx + jdir, jidx);
jt = if(jchg, 18 + rand(1000) / 1000 * 14, jt);
""" + julia_stop_pick("jtg", "jidx") + """jph_m = if(jnew, jtg, jph_m);
jph = if(jnew, jtg, jph);
""" + ease("jph", "jtg", "5.0") + """a = (jph + 0.003 * sin(time * 0.05)) * 6.2831853;
q1 = 0.5 * lr * cos(a) - 0.25 * lr * lr * cos(2 * a);
q2 = 0.5 * lr * sin(a) - 0.25 * lr * lr * sin(2 * a);
q15 = 0.5 * lr * cos(a);
q16 = 0.5 * lr * sin(a);
lm = pow(lr, m);
q13 = lm * cos(m * a);
q14 = lm * sin(m * a);
dv = dv + dt * spdt / 40;
f = dv - int(dv);
wrapped = above(int(dv), cyc);
cyc = int(dv);
grot = grot + wrapped * m * a;
grot = grot - 6.2831853 * floor(grot / 6.2831853 + 0.5);
h = grot + 0.8 * sin(time * 0.07) + 0.35 * sin(time * 0.13 + 2);
pch = 0.72 + 0.3 * sin(time * 0.1 + 1) + 0.04 * sin(time * 0.23);
alt = 0.09 * pow(lr, -m * f);
back = alt / tan(pch);
q7 = q15 - back * cos(h);
q8 = q16 - back * sin(h);
q6 = alt;
q4 = h + 0.28 * sin(time * 0.083) + 0.08 * sin(time * 0.21 + 1);
q10 = pch + 0.1 * sin(time * 0.067 + 2);
q27 = 0.16 * sin(time * 0.08) + 0.06 * sin(time * 0.19);
q11 = m * 0.015 * f;
w = min(max((f - 0.8) / 0.2, 0), 1);
q12 = w * w * (3 - 2 * w);
hue = hue + dt * 0.006;
q9 = hue;
q17 = f;
q18 = grot;
flw = flw + dt * (0.5 + 0.35 * spdt);
q19 = flw;
q20 = ba1; q21 = bs1; q22 = ba2; q23 = bs2; q24 = ba3; q25 = bs3;
q26 = grot + 0.3 + 0.35 * sin(time * 0.041);
core = core + (min(bass_att, 2) - core) * (1 - exp(-dt / 0.4));
q30 = core;
"""
presets["nokkvi - julia lace"] = preset({"decay": 0.0, "wave_a": 0.0, "zoom": 1.0}, JULIA_WARP, JULIA_COMP,
                                       init=JULIA_INIT, frame=JULIA_FRAME)

# Fractal voyage (no cover) -----------------------------------------------
# A voyage through four fractals lying in a dark, glossy liquid, each a
# phase of about 45 s: a Julia set, the Burning Ship, the Mandelbrot set and
# the Heighway dragon curve, round and round. Every phase starts high above
# the whole fractal, dives towards a target, eases to a stop in it (its
# dark "eye" where it has one), then the camera turns and tilts up to the
# moon (33 degrees, so the water leaves the frame) while the scene blurs
# and smears; the fractal swaps out of sight and the camera tilts back down
# onto the next one. The moon's heading carries across the swap (`mo` takes
# up the change of base heading) so the sky in view never jumps, while it
# still turns with `grot` at a Julia wrap. Every beat also swells the moon
# and its reflection on the water (vmp: the last three beats, ~0.25 s each).
#
# The fractals are drawn as fine lace: a distance estimate turns each
# boundary into hairline ridges with a soft glow, coloured by the smooth
# iteration count (the dragon by its position along the curve). The ground
# plane is the complex plane; the camera stands at height q6 over it,
# looking at the target q15/q16 from a pitch that swings between a steep
# dive and a horizon-grazing glide, sweeping its heading around it.
#
# Julia: c = l/2 - l^2/4 makes l the multiplier of the fixed point
# alpha = l/2, and the set maps onto itself near alpha by
# z -> alpha + l (z - alpha), so zooming in by |l|^M while turning by
# M arg(l) shows the same picture again. The phase opens on the whole set,
# zooms down to height 0.09, then dives endlessly: after a zoom of |l|^M
# (about 3x) the camera jumps back up by exactly that map (height times
# |l|^M, heading plus M arg(l), kept in `grot`), one ring deeper, and the
# last fifth of every cycle crossfades into the next cycle's view (the same
# window mapped by l^M, drawn a second time), so the seam never shows.
# After two cycles it stops wrapping and plunges into alpha until the
# points that never escape (the eye, radius ~0.3 |l|^-240) fill the view
# (2x to 500x; the galaxy's and the dendrite's eyes are sub-pixel, so those
# dives just slow to a stop). Each visit takes the
# next set from VOY_JULIAS: the lace family (|l| = 1.03, at a random
# JULIA_STOP) alternates with a galaxy, tendrils, a snowflake and a
# dendrite. Near arg(l) = p/q with q <= 9 the set collapses into q straight
# arms over a void (c lands in or beside that bulb), so every lace stop
# keeps |arg - p/q| q^2 above ~0.12.
# Burning Ship and Mandelbrot: plain zooms (float precision allows about
# 1000x) from the whole set into a mini-copy (VOY_SHIP / VOY_MANDEL: nuclei
# found with Newton on the period-p orbit, 2D with the fold's Jacobian for
# the ship), whose dark body fills the view at the end. The ship's distance
# estimate uses that Jacobian; both loops check for a repeating orbit
# (Brent) so the interior exits early. The ship's flames are dust far below
# a pixel, which a distance estimate lights solid (a white wash), so the ship
# is graded by escape time against the view's median count (about
# 2.4 / sqrt(height), measured along the dive): only pixels escaping several
# times slower than the median light up, thin filaments keep their lines.
# Dragon: the Heighway dragon (the attractor of f1 = (1+i)z/2 and
# f2 = 1 - (1-i)z/2, a curve from 0 to 1). Its distance runs down the IFS
# keeping the 4 best branches per level (by bounding disc, then distance
# to the chord; greedy descent misses 65% of the curve, 4 branches are
# exact to 0.01%), and the last level is a segment that folds into the next
# level's corner as the level's fraction grows, so the curve unfolds
# smoothly as the camera sinks. The level follows each pixel's footprint
# and a cap (q31) that grows through the phase, so the curve first folds
# up from a single line. The camera dives into the start point, where the
# curve is only copies of its first half (the second half never comes
# closer than 1/3), so those levels are skipped straight away.
#
# The liquid: its swells are four octaves of noise in camera space
# (u = the ground point about the target, turned by -q18, over the height),
# each octave LM = |q13, q14| apart and weighted by the depth's fraction
# (q17), so every octave rides the zoom with the fractal and the ladder
# repeats exactly across the Julia wrap; the octaves drift (q19, faster as
# the dive speeds up). The noise is sampler_noise_hq read straight (it is
# already smooth, a cubic lattice every 8 texels) with slopes over 2 texels:
# the texel-snapping quintic of tnoise zeroes the slope at every texel edge
# and drew the texel grid into the glints as straight streaks. The surface
# refracts the fractal (a lookup offset by its slope, scaled by the
# distance to the target so it stays in proportion to the detail), focuses
# light into caustics on it (where the swells curve, brighter with the
# bass, q30), absorbs it with the path length, reflects a sky gradient by
# Fresnel, and glints where it mirrors a low moon. Rays above the horizon
# see that sky.
#
# Music never touches a shape (a Julia set is so sensitive to c that a
# beat-driven nudge reads as a glitch): loudness sets the dive speed and
# kicks surge it (SPEED; the phase clock runs at that speed, clamped so a
# phase takes 22 to 72 s), and every beat (BEATS) sends out a pulse shaped
# by the fractal. Julia and dragon: a ripple ring from the target (a
# vortex, a curve's start) across the liquid, in log radius so it looks the
# same at any depth, which bends the fractal, catches the light, glows
# faintly and lights the lace it passes. Ship and Mandelbrot, whose targets
# are no visual centre (rings read as arbitrary there): no rings, but a
# front of light running outward from the set through its own escape-time
# contours, from 8x the view's typical count down to half of it in 0.6 s
# (vf_pulse; typical = 2.4 / sqrt(height) for the ship, 8 height^-0.4 for
# the Mandelbrot, both measured along the dives: a fixed count range put
# the front where the view had no structure until it had faded). Kicks also
# brighten the glow and the glints. The phase changes on its own clock,
# never on a beat. Everything Julia-only is gated by the phase (isj): the
# seam crossfade keys on the Julia depth, which runs in every phase, and
# ungated it blended a Julia image into the other fractals twice a phase.
#
# The scene is drawn in the warp, which during a transition keeps q29 * 0.8
# of the previous frame (the smear that hides the swap); the comp adds
# bloom and blurs by q29. EEL notes: the engine has no fract (unknown
# functions evaluate to 0), rand(n) is a float, and rand is seeded from
# the preset text, so the per-visit choices also hash the music.
# q map: q1/q2 Julia c, q3 kick, q4/q10 view heading and pitch, q5 pop,
# q6 height, q7/q8 camera, q9 hue, q11 seam hue shift, q12 seam crossfade,
# q13/q14 l^M (Julia) or (3, 0), q15/q16 target, q17 depth fraction, q18 the
# u-space turn (grot or the visit's heading), q19 liquid drift, q20-q25 beat
# ages and strengths, q26 moon heading, q27 bank, q28 fractal (0 Julia,
# 1 ship, 2 Mandelbrot, 3 dragon), q29 transition blur, q30 bass glow,
# q31 dragon level cap, q32 the Julia family's M (the seam's colour shift).
# (|l|, arg(l) in turns, M) for the odd Julia visits: galaxy (c = -0.8 +
# 0.156i), tendrils (0.285 + 0.01i), snowflake (-0.4 + 0.6i), dendrite (i).
VOY_JULIAS = [(1.0659, 0.4773, 17), (1.0197, 0.0604, 56), (1.0171, 0.3824, 65), (1.3864, 0.3213, 3)]
# (target, start height, end height) per visit; the arrival plunges a further 3x.
VOY_SHIP = [((-1.7322877381, -0.0337903588), 1.2, 0.002), ((-1.7577830601, -0.0137961434), 1.2, 0.01)]
VOY_MANDEL = [((-0.745348441, 0.084712189), 1.5, 0.0025), ((-0.742435816, 0.107942324), 1.5, 0.003)]
ESC_ITERS = 400
VOY_FUNCS = JULIA_FN + f"""float mandelf(vec2 mc, out float md, out float mmu) {{
  vec2 mz = vec2(0.0);
  vec2 mdz = vec2(0.0);
  vec2 msv = vec2(1e9);
  float mm2 = 0.0;
  float mit = 0.0;
  int msi = 1;
  for (int i = 0; i < {ESC_ITERS}; i++) {{
    mdz = 2.0 * vec2(mz.x * mdz.x - mz.y * mdz.y, mz.x * mdz.y + mz.y * mdz.x) + vec2(1.0, 0.0);
    mz = vec2(mz.x * mz.x - mz.y * mz.y, 2.0 * mz.x * mz.y) + mc;
    mm2 = dot(mz, mz);
    if (mm2 > 1e5) break;
    vec2 mdd = mz - msv;
    if (dot(mdd, mdd) < 1e-11) {{ mit = {ESC_ITERS}.0; break; }}
    if (i == msi) {{ msv = mz; msi = msi * 2 + 1; }}
    mit += 1.0;
  }}
  float mlz = 0.5 * log(max(mm2, 1.0001));
  md = sqrt(mm2 / max(dot(mdz, mdz), 1e-20)) * mlz;
  mmu = mit + 1.0 - log2(max(mlz, 1e-6));
  return step({ESC_ITERS}.0 - 0.5, mit);
}}
float shipf(vec2 sc, out float sd, out float smu) {{
  vec2 sz = vec2(0.0);
  vec4 sj = vec4(0.0);
  vec2 ssv = vec2(1e9);
  float sm2 = 0.0;
  float sit = 0.0;
  int ssi = 1;
  for (int i = 0; i < {ESC_ITERS}; i++) {{
    vec2 ssg = vec2(2.0 * step(0.0, sz.x) - 1.0, 2.0 * step(0.0, sz.y) - 1.0);
    vec2 sw = abs(sz);
    sj = vec4(2.0 * (sw.x * ssg.x * sj.x - sw.y * ssg.y * sj.z) + 1.0,
              2.0 * (sw.x * ssg.x * sj.y - sw.y * ssg.y * sj.w),
              2.0 * (sw.y * ssg.x * sj.x + sw.x * ssg.y * sj.z),
              2.0 * (sw.y * ssg.x * sj.y + sw.x * ssg.y * sj.w) + 1.0);
    sz = vec2(sw.x * sw.x - sw.y * sw.y, 2.0 * sw.x * sw.y) + sc;
    sm2 = dot(sz, sz);
    if (sm2 > 1e5) break;
    vec2 sdd = sz - ssv;
    if (dot(sdd, sdd) < 1e-11) {{ sit = {ESC_ITERS}.0; break; }}
    if (i == ssi) {{ ssv = sz; ssi = ssi * 2 + 1; }}
    sit += 1.0;
  }}
  float slz = 0.5 * log(max(sm2, 1.0001));
  float sjn = sqrt(0.5 * dot(sj, sj)) + 1e-20;
  sd = sqrt(sm2) * slz / sjn;
  smu = sit + 1.0 - log2(max(slz, 1e-6));
  return step({ESC_ITERS}.0 - 0.5, sit);
}}
float dg_seg(vec2 dsq) {{
  return length(dsq - vec2(clamp(dsq.x, 0.0, 1.0), 0.0));
}}
vec4 dg_c0(vec4 dp0) {{
  vec2 dq0 = vec2(dp0.x + dp0.y, dp0.y - dp0.x);
  vec2 dr0 = dq0 - vec2(0.4167, 0.1667);
  float dk0 = max(length(dr0) - 0.83, 0.0) * 1000.0 + dg_seg(dq0);
  return vec4(dq0, dk0, dp0.w);
}}
vec4 dg_c1(vec4 dp1, float dbl) {{
  vec2 dq1 = vec2(1.0 - dp1.x + dp1.y, 1.0 - dp1.x - dp1.y);
  vec2 dr1 = dq1 - vec2(0.4167, 0.1667);
  float dk1 = max(length(dr1) - 0.83, 0.0) * 1000.0 + dg_seg(dq1);
  float dnf1 = step(3.5, dp1.w);
  float da1 = dp1.w - 4.0 * dnf1 + dbl * (1.0 - 2.0 * dnf1);
  return vec4(dq1, dk1, da1 + 4.0 * (1.0 - dnf1));
}}
vec2 dg_leaf(vec2 dlq, float dlt) {{
  vec2 dm = vec2(0.5, 0.5 * dlt);
  float dh0 = clamp(dot(dlq, dm) / dot(dm, dm), 0.0, 1.0);
  float dd0 = length(dlq - dm * dh0);
  vec2 de1 = vec2(1.0, 0.0) - dm;
  vec2 dlm = dlq - dm;
  float dh1 = clamp(dot(dlm, de1) / dot(de1, de1), 0.0, 1.0);
  float dd1 = length(dlm - de1 * dh1);
  float dsel = step(dd1, dd0);
  return vec2(mix(dd0, dd1, dsel), mix(0.5 * dh0, 0.5 + 0.5 * dh1, dsel));
}}
float dragonf(vec2 dgp, float dgn, float dgt, out float dgd, out float dgmu) {{
  float dgr = length(dgp);
  float dgk = floor(clamp(2.0 * log2(0.25 / max(dgr, 1e-30)), 0.0, dgn));
  float dga = -0.78539816 * dgk;
  vec2 dgq = exp2(0.5 * dgk) * vec2(dgp.x * cos(dga) - dgp.y * sin(dga), dgp.x * sin(dga) + dgp.y * cos(dga));
  vec4 dx0 = vec4(dgq, 0.0, 0.0);
  vec4 dx1 = vec4(1e4, 1e4, 1e9, 0.0);
  vec4 dx2 = vec4(1e4, 1e4, 1e9, 0.0);
  vec4 dx3 = vec4(1e4, 1e4, 1e9, 0.0);
  float dsw = 0.0;
  vec4 dtmp = vec4(0.0);
  for (int k = 0; k < 30; k++) {{
    float dlev = dgk + float(k);
    if (dlev >= dgn) break;
    float dbl = exp2(-dlev);
    vec4 dy0 = dg_c0(dx0);
    vec4 dy1 = dg_c1(dx0, dbl);
    vec4 dy2 = dg_c0(dx1);
    vec4 dy3 = dg_c1(dx1, dbl);
    vec4 dy4 = dg_c0(dx2);
    vec4 dy5 = dg_c1(dx2, dbl);
    vec4 dy6 = dg_c0(dx3);
    vec4 dy7 = dg_c1(dx3, dbl);
""" + "".join(f"""
    dsw = step({b}.z, {a}.z);
    dtmp = mix({a}, {b}, dsw);
    {b} = mix({b}, {a}, dsw);
    {a} = dtmp;""" for a, b in (("dy0", "dy1"), ("dy2", "dy3"), ("dy0", "dy2"), ("dy1", "dy3"), ("dy1", "dy2"),
                                ("dy4", "dy5"), ("dy6", "dy7"), ("dy4", "dy6"), ("dy5", "dy7"), ("dy5", "dy6"))) + f"""
    dx0 = mix(dy0, dy7, step(dy7.z, dy0.z));
    dx1 = mix(dy1, dy6, step(dy6.z, dy1.z));
    dx2 = mix(dy2, dy5, step(dy5.z, dy2.z));
    dx3 = mix(dy3, dy4, step(dy4.z, dy3.z));
  }}
  float dgs = exp2(-0.5 * dgn);
  float dgb = exp2(-dgn);
  vec2 dl0 = dg_leaf(dx0.xy, dgt);
  vec2 dl1 = dg_leaf(dx1.xy, dgt);
  vec2 dl2 = dg_leaf(dx2.xy, dgt);
  vec2 dl3 = dg_leaf(dx3.xy, dgt);
  float dgbest = dl0.x;
  float dgw = dx0.w;
  float dgl = dl0.y;
  float dgsel = step(dl1.x, dgbest);
  dgbest = mix(dgbest, dl1.x, dgsel);
  dgw = mix(dgw, dx1.w, dgsel);
  dgl = mix(dgl, dl1.y, dgsel);
  dgsel = step(dl2.x, dgbest);
  dgbest = mix(dgbest, dl2.x, dgsel);
  dgw = mix(dgw, dx2.w, dgsel);
  dgl = mix(dgl, dl2.y, dgsel);
  dgsel = step(dl3.x, dgbest);
  dgbest = mix(dgbest, dl3.x, dgsel);
  dgw = mix(dgw, dx3.w, dgsel);
  dgl = mix(dgl, dl3.y, dgsel);
  float dgnf = step(3.5, dgw);
  dgd = dgbest * dgs;
  dgmu = ((dgw - 4.0 * dgnf) + dgb * (1.0 - 2.0 * dgnf) * dgl) * 400.0;
  return 0.0;
}}
float vf_front(float vfl, float vfa, float vfs) {{
  float vff = 3.0 - vfa * 6.5;
  float vfx = vfl - vff;
  return vfs * exp(-vfa * 1.8) * exp(-vfx * vfx * 1.6);
}}
float vf_pulse(float vfmu, float vfmed) {{
  float vfl = log2(max(vfmu, 1.0) / vfmed);
  return vf_front(vfl, q20, q21) + vf_front(vfl, q22, q23) + vf_front(vfl, q24, q25);
}}
float fr_eval(vec2 few, float fepix, out float fed, out float femu) {{
  float fein = 0.0;
  if (q28 < 0.5) {{
    fein = julia(few, vec2(q1, q2), fed, femu);
  }} else if (q28 < 1.5) {{
    fein = shipf(few, fed, femu);
    float fesg = smoothstep(0.5, 2.2, log2(max(femu, 1.0) * sqrt(q6) / 2.4));
    fed = max(fed * 3.0, fepix * (0.25 + 30.0 * (1.0 - fesg)));
  }} else if (q28 < 2.5) {{
    fein = mandelf(few, fed, femu);
  }} else {{
    float fel = clamp(min(2.0 * log2(0.12 / max(fepix, 1e-30)), q31), 0.0, 26.0);
    float fen = floor(fel);
    fein = dragonf(few, fen, fel - fen, fed, femu);
  }}
  return fein;
}}
""" + LIQUID_FNS
def lace_shade(v, w0, hueshift, light, evalcall):
    """Lace colour `v` for the ground point `w0` over `ground`: `evalcall`
    (GLSL, may use {v}_w and {v}_pix) sets {v}_d and {v}_mu and returns the
    'never escapes' flag; `light` (a float expression) adds a wave of light."""
    return f"""
  vec2 {v}_w = {w0};
  float {v}_pix = max(length(dFdx({v}_w)), length(dFdy({v}_w))) + 1e-9;
  float {v}_d;
  float {v}_mu;
  float {v}_in = {evalcall.format(v=v)};
  float {v}_line = clamp(1.0 - {v}_d / ({v}_pix * 1.3), 0.0, 1.0) * (1.0 - {v}_in);
  float {v}_glow = exp(-{v}_d / ({v}_pix * 10.0)) * (1.0 - {v}_in);
  float {v}_ridge = exp(-{v}_d / ({v}_pix * 4.0));
  float {v}_shade = clamp(0.75 - dot(vec2(dFdx({v}_ridge), dFdy({v}_ridge)), vec2(0.6, -0.8)) * 2.5, 0.35, 1.4);
  float {v}_hue = fract({v}_mu * 0.015 - ({hueshift}) + q9);
""" + ramp(v + "_lace", f"0.35 + 0.6 * abs({v}_hue * 2.0 - 1.0)") + f"""
  vec3 {v} = mix(ground, NOKKVI_BG * 0.55, {v}_in * 0.8);
  {v} += {v}_lace * {v}_glow * (0.3 + 0.35 * q3 + 0.9 * ({light})) * {v}_shade;
  {v} = mix({v}, {v}_lace * (0.9 + 0.3 * q3 + 0.45 * ({light})) * {v}_shade, {v}_line);
  {v} = mix({v}, NOKKVI_TEXT, {v}_line * smoothstep(0.6, 1.0, {v}_glow) * 0.2);
"""
def voy_pick(var, idx, values):
    e = f"{values[-1]}"
    for i in range(len(values) - 2, -1, -1):
        e = f"if(equal({idx}, {i}), {values[i]}, {e})"
    return f"{var} = {e};\n"
VOY_WARP = VOY_FUNCS + " shader_body {\n" + HEAD + """
  vec2 p = (uv_orig - 0.5) * s;
  p = vec2(p.x * cos(q27) - p.y * sin(q27), p.x * sin(q27) + p.y * cos(q27));
  float ha = q4;
  float pt = q10;
  vec3 fw = vec3(cos(pt) * cos(ha), cos(pt) * sin(ha), -sin(pt));
  vec3 rt = vec3(sin(ha), -cos(ha), 0.0);
  vec3 up = cross(rt, fw);
  vec3 rd = normalize(fw + (p.x * rt + p.y * up) * 1.15);
  float down = -rd.z;
  float dn = max(down, 0.004);
  vec2 wa0 = vec2(q7, q8) + rd.xy * (q6 / dn);
  vec2 c = vec2(q1, q2);
  vec2 al = vec2(q15, q16);
  float cg = cos(q18);
  float sn = sin(q18);
  vec2 rel0 = wa0 - al;
  vec2 lu = vec2(cg * rel0.x + sn * rel0.y, cg * rel0.y - sn * rel0.x) / q6;
  float lr = length(lu) + 1e-4;
  float lpix = max(length(dFdx(lu)), length(dFdy(lu))) + 1e-6;
  vec3 lsw = jl_swell(lu, lpix);
  vec2 lsl = lsw.xy * 0.13;
  float caus = pow(max(-lsw.z * 0.12, 0.0), 2.0);
  float lrr = log(lr);
  float rfade = smoothstep(0.1, 0.035, lpix / lr);
  float vfesc = step(0.5, q28) * step(q28, 2.5);
  float vfring = 1.0 - vfesc;
  float vfmed = mix(2.4 / sqrt(q6), 8.0 * pow(q6, -0.4), step(1.5, q28));
  float rsl = jl_ring(lrr, q20, q21) + jl_ring(lrr, q22, q23) + jl_ring(lrr, q24, q25);
  lsl += (lu / lr) * (rsl * 0.16 * rfade * vfring);
  float rgl = (jl_glow(lrr, q20, q21) + jl_glow(lrr, q22, q23) + jl_glow(lrr, q24, q25)) * rfade * vfring;
  vec2 wsl = vec2(cg * lsl.x - sn * lsl.y, sn * lsl.x + cg * lsl.y);
  vec3 n = normalize(vec3(-wsl.x, -wsl.y, 1.0));
  vec2 wa = wa0 - wsl * (q6 * lr * 0.22);
  vec3 ground = mix(NOKKVI_BG, NOKKVI_ACCENT, 0.07) * 0.7;
""" + lace_shade("ca", "wa", "q11", "rgl + vfesc * 1.8 * vf_pulse(ca_mu, vfmed)", "fr_eval({v}_w, {v}_pix, {v}_d, {v}_mu)") + """
  vec3 lace = ca;
  if (q12 > 0.0) {
    vec2 rel = wa - al;
    vec2 wb = al + vec2(q13 * rel.x - q14 * rel.y, q13 * rel.y + q14 * rel.x);
""" + lace_shade("cb", "wb", "q11 - q32 * 0.015", "rgl", "julia({v}_w, c, {v}_d, {v}_mu)") + """
    lace = mix(ca, cb, q12);
  }
  float lfar = smoothstep(0.06, 0.02, lpix / lr);
  lace = mix(ground, lace, lfar);
  lace *= 1.0 + caus * (0.8 + 0.5 * q30) * lfar;
  lace += mix(NOKKVI_ACCENT, NOKKVI_TEXT, 0.35) * caus * 0.09 * lfar;
  float cosv = max(dot(n, -rd), 0.0);
  float fre = 0.02 + 0.98 * pow(1.0 - cosv, 5.0);
  vec3 rfl = reflect(rd, n);
  rfl = vec3(rfl.x, rfl.y, abs(rfl.z));
  vec3 env = jl_env(rfl);
  vec3 lgt = vec3(0.98 * cos(q26), 0.98 * sin(q26), 0.2);
  float vmp = q21 * exp(-q20 * 4.0) + q23 * exp(-q22 * 4.0) + q25 * exp(-q24 * 4.0);
  vec3 vmc = mix(NOKKVI_TEXT, NOKKVI_HIGHLIGHT, 0.35);
  float sdot = max(dot(rfl, lgt), 0.0);
  env += vmc * (pow(sdot, 600.0) * 0.7 + pow(sdot, 12.0) * 0.12) * vmp;
  float spec = (pow(sdot, 2200.0) * 1.1 + pow(sdot, 220.0) * 0.05) * (1.0 + vmp);
  float trans = exp(-0.15 * (1.0 / dn - 1.0));
  vec3 col = lace * trans * (1.0 - fre) + env * fre + mix(NOKKVI_TEXT, NOKKVI_HIGHLIGHT, 0.35) * spec * (0.8 + 0.8 * q3);
  col += mix(NOKKVI_ACCENT, NOKKVI_HIGHLIGHT, 0.3) * rgl * 0.3 * (1.0 - 0.6 * fre);
  vec3 sky = jl_env(rd);
  float vms = max(dot(rd, lgt), 0.0);
  sky += vmc * (pow(vms, 600.0) * 0.7 + pow(vms, 12.0) * 0.12) * vmp;
  col = mix(sky, col, smoothstep(0.0, 0.01, down));
  vec3 prv = texture(sampler_main, uv).xyz;
  ret = mix(col, prv, q29 * 0.8);
 }"""
VOY_COMP = " shader_body {\n" + HEAD + """
  vec2 p = (uv - 0.5) * s;
  vec3 m = texture(sampler_main, uv).xyz;
  vec3 b1 = GetBlur1(uv);
  vec3 b2 = GetBlur2(uv);
  vec3 b3 = GetBlur3(uv);
  vec3 col = m + max(b1 - 0.4, 0.0) * 0.3 + max(b2 - 0.3, 0.0) * (0.2 + 0.25 * q3);
  col = mix(col, b3 * 1.1 + b2 * 0.1, q29 * 0.85);
  col *= 0.82 + 0.18 * smoothstep(1.3, 0.3, length(p));
  ret = col;
 }"""
NFAM = 2 * len(VOY_JULIAS)
VOY_INIT = ("pulse = 0; pop = 0; hue = 0; flw = 0; core = 1; cyc = 0; grot = 0;"
            " ph = 0; pt = 0; pz = 0; pa = 0; pu = 0; pinit = 0; jv = -1; mv = -1; sv = -1; jst = 0; hb = 0; mo = 0; gbp = 0; "
            + SPEED_INIT + " " + BEATS_INIT)
VOY_FRAME = PULSE + "dt = min(1 / max(fps, 1), 0.1);\n" + SPEED + BEATS + f"""pt = pt + dt;
pz = min(pz + dt * min(max(spdt, 1.2), 4) / 86.4, 1);
pa = if(below(pz, 1), 0, min(pa + dt / 3, 1));
pu = if(below(pa, 1), 0, min(pu + dt / 3, 1));
pnext = above(pu, 0.9999);
swp = pnext + below(pinit, 0.5);
pinit = 1;
ph = if(pnext, (ph + 1) % 4, ph);
pt = if(swp, 0, pt);
pz = if(swp, 0, pz);
pa = if(swp, 0, pa);
pu = if(swp, 0, pu);
jv = if(swp * equal(ph, 0), jv + 1, jv);
sv = if(swp * equal(ph, 1), sv + 1, sv);
mv = if(swp * equal(ph, 2), mv + 1, mv);
vsd = rand(1) + bass * 1.37 + treb * 2.9 + mid * 0.71;
vsd = vsd - floor(vsd);
jst = if(swp, int(vsd * {len(JULIA_STOPS)}), jst);
hb = if(swp, vsd * 37.7, hb);
hb = hb - 6.2831853 * floor(hb / 6.2831853);
cyc = if(swp, 0, cyc);
grot = if(swp, hb, grot);
jf = jv % {NFAM};
jlace = equal(jf % 2, 0);
jfo = int(jf / 2);
""" + voy_pick("jlr", "jfo", [f[0] for f in VOY_JULIAS]) + voy_pick("jfph", "jfo", [f[1] for f in VOY_JULIAS]) \
    + voy_pick("jfm", "jfo", [f[2] for f in VOY_JULIAS]) + voy_pick("jstp", "jst", JULIA_STOPS) + f"""lr = if(jlace, 1.03, jlr);
m = if(jlace, {JULIA_M}, jfm);
a = (if(jlace, jstp, jfph) + 0.003 * sin(time * 0.05)) * 6.2831853;
q1 = 0.5 * lr * cos(a) - 0.25 * lr * lr * cos(2 * a);
q2 = 0.5 * lr * sin(a) - 0.25 * lr * lr * sin(2 * a);
jalx = 0.5 * lr * cos(a);
jaly = 0.5 * lr * sin(a);
lm = pow(lr, m);
isj = equal(ph, 0);
pae = 1 - (1 - pa) * (1 - pa);
jzs = log(0.09 / 1.2) / log(lm);
jz = jzs + (2 - jzs) * pz;
jn = floor(max(jz, 0));
wrapped = above(jn, cyc) * isj;
cyc = jn;
grot = grot + wrapped * m * a;
grot = grot - 6.2831853 * floor(grot / 6.2831853 + 0.5);
jfr = jz - floor(jz);
jpl = min(max(0.11 / (0.3 * pow(lr, -240)), 2), 500);
jalt = if(below(jz, 0), 0.09 * pow(lm, -jz), 0.09 * pow(lm, -jfr)) * pow(jpl, -pae);
jlad = jz + pae * log(jpl) / log(lm);
jw = min(max((jfr - 0.8) / 0.2, 0), 1) * above(jz, 0) * below(pz, 1) * isj;
svi = sv % {len(VOY_SHIP)};
mvi = mv % {len(VOY_MANDEL)};
""" + voy_pick("stx", "svi", [t[0][0] for t in VOY_SHIP]) + voy_pick("sty", "svi", [t[0][1] for t in VOY_SHIP]) \
    + voy_pick("sa1", "svi", [t[2] for t in VOY_SHIP]) + voy_pick("mtx", "mvi", [t[0][0] for t in VOY_MANDEL]) \
    + voy_pick("mty", "mvi", [t[0][1] for t in VOY_MANDEL]) + voy_pick("ma1", "mvi", [t[2] for t in VOY_MANDEL]) + f"""pa0 = if(equal(ph, 2), {VOY_MANDEL[0][1]}, 1.2);
pa1 = if(equal(ph, 1), sa1, if(equal(ph, 2), ma1, 0.004));
ppl = if(equal(ph, 3), 2, 3);
palt = pa0 * pow(pa1 / pa0, pz) * pow(ppl, -pae);
plad = log(pa0 / palt) / log(3);
alt = if(isj, jalt, palt);
lad = if(isj, jlad, plad);
q15 = if(isj, jalx, if(equal(ph, 1), stx, if(equal(ph, 2), mtx, 0)));
q16 = if(isj, jaly, if(equal(ph, 1), sty, if(equal(ph, 2), mty, 0)));
gb = if(isj, grot, hb);
h = gb + 0.8 * sin(time * 0.07) + 0.35 * sin(time * 0.13 + 2);
pch = 0.72 + 0.3 * sin(time * 0.1 + 1) + 0.04 * sin(time * 0.23);
back = alt / tan(pch);
q7 = q15 - back * cos(h);
q8 = q16 - back * sin(h);
q6 = alt;
dsc = max(1 - pt / 3, 0);
tilt = max(dsc * dsc * (3 - 2 * dsc), pu * pu * (3 - 2 * pu));
mo = mo + pnext * (gbp - gb);
mo = mo - 6.2831853 * floor(mo / 6.2831853 + 0.5);
gbp = gb;
q26 = gb + mo + 0.3 + 0.35 * sin(time * 0.041);
hv = h + 0.28 * sin(time * 0.083) + 0.08 * sin(time * 0.21 + 1);
dmh = q26 - hv;
dmh = dmh - 6.2831853 * floor(dmh / 6.2831853 + 0.5);
q4 = hv + dmh * tilt;
q10 = (pch + 0.1 * sin(time * 0.067 + 2)) * (1 - tilt) - 0.58 * tilt;
q27 = (0.16 * sin(time * 0.08) + 0.06 * sin(time * 0.19)) * (1 - tilt);
q11 = isj * m * 0.015 * jfr * above(jz, 0);
q12 = jw * jw * (3 - 2 * jw);
q13 = if(isj, lm * cos(m * a), 3);
q14 = if(isj, lm * sin(m * a), 0);
q17 = lad - floor(lad);
q18 = gb;
hue = hue + dt * 0.006;
q9 = hue + ph * 0.23;
flw = flw + dt * (0.5 + 0.35 * spdt);
q19 = flw;
q20 = ba1; q21 = bs1; q22 = ba2; q23 = bs2; q24 = ba3; q25 = bs3;
core = core + (min(bass_att, 2) - core) * (1 - exp(-dt / 0.4));
q30 = core;
q28 = ph;
q29 = pow(tilt, 1.4);
q31 = pz * 27;
q32 = m;
"""
presets["nokkvi - fractal voyage"] = preset({"decay": 0.0, "wave_a": 0.0, "zoom": 1.0}, VOY_WARP, VOY_COMP,
                                          init=VOY_INIT, frame=VOY_FRAME)

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
# A flight down an endless corridor of frames standing half-sunk in a dark,
# glossy liquid. The idea is martin's "infinity (2010 update)" (nested
# polygons that spiral outward while the camera changes pattern); nothing of
# its code is used, and nothing is feedback: the whole scene is raymarched
# sharp every frame, like coral city. Each frame is a bevelled metal beam
# bent into a rounded polygon (mostly triangles, sometimes squares,
# pentagons or hexagons) with a neon inlay line, placed one spacing apart
# along a gently winding path and turned a little more than the one before
# (the twist), so the corridor spirals and the spiral turns as the camera
# flies. The liquid is a plane of slow noise swells and ripples (periodic
# with the camera's wrap, faded with distance so it never aliases) that
# reflects the frames through a second march; frames are lit by a lamp that
# circles ahead of the camera plus a slow key light, and reflect the scene
# with a Fresnel sheen. Fog and a horizon glow close the vanishing point.
# Camera patterns (IFR_ACTS: straight corridor, tight spiral, serpent bends,
# orbit, counter-twisting vortex) are re-drawn every 13-21 s, never on a
# beat; twist, path bends, orbit, sway, spin and lens all ease twice (tau
# ~3 s), so a change glides. Shape and palette changes are frozen per frame:
# frames spawned after a switch index (q26) take the new order and palette,
# so a new shape enters from the far end and nothing morphs on screen.
# Music: loudness sets the cruising speed (0.25 to 3.5 frames per second),
# kicks add a short surge that also widens the lens, and spin and liquid flow
# scale with the speed; each detected beat (bass above its follower, 0.2 s
# cooldown) sends a bright wave down the inlays (the last three beats' ages
# and strengths ride in q1/q2, q4/q6, q7/q8), with dashes running along the
# lit beams and a treble shimmer between kicks. Beat never touches geometry.
# The shader preprocessor collapses a `?:` whose condition uses && or || to
# grey, so combined conditions are built from step/min/max.
# q map: q3 kick pulse, q5 pop, q9 fract(flown), q10 flown cell mod 32, q11/q12
# camera xy, q13-q15 forward, q16 roll sway, q17 lens, q18 spin, q19 twist,
# q20/q21 path amplitudes, q22 lamp phase, q23 hue drift, q24 liquid flow,
# q25 core light, q26 shape switch index, q27/q28 old/new polygon order,
# q29 flown cell mod 256, q30/q31 new/old palette, q32 inlay flow.

IFR_P = 1.0            # frame spacing
IFR_R = 1.8            # frame circumradius
IFR_W = 6.2831853 / 32 # path frequency per cell (path period 32 cells)
IFR_NF = 26             # frames ahead that are drawn
IFR_FH = -0.9           # liquid surface height

IFR_FUNCS = f"""
vec2 ifr_path(float pu) {{
  return vec2(q20 * cos({IFR_W:.9f} * pu) + q21 * cos({3 * IFR_W:.9f} * pu + 1.3),
              0.25 * (q20 * sin({IFR_W:.9f} * pu) + q21 * sin({2 * IFR_W:.9f} * pu + 0.4)));
}}
float ifr_hash(float hu) {{
  return fract(sin(mod(hu, 32.0) * 12.9898 + 4.1) * 43758.5453);
}}
vec3 ifr_ramp(float rx) {{""" + ramp("rr", "rx") + f"""
  return rr;
}}
vec2 ifr_local(vec2 lq, float lj) {{
  float lu = q10 + lj;
  vec2 lq2 = lq - ifr_path(lu);
  float la = q19 * (lj - q9) + q18;
  float lc = cos(la);
  float ls = sin(la);
  return vec2(lc * lq2.x + ls * lq2.y, -ls * lq2.x + lc * lq2.y);
}}
float ifr_poly(vec2 pq, float pn, out float palong) {{
  float pan = 3.14159265 / pn;
  float pa = atan(pq.y, pq.x);
  float ps = mod(pa + pan, 2.0 * pan) - pan;
  float pl = length(pq);
  vec2 pr = pl * vec2(cos(ps), abs(sin(ps)));
  float prad = {IFR_R * 0.93:.4f};
  float pap = prad * cos(pan);
  float ph = prad * sin(pan);
  vec2 pe = pr - vec2(pap, clamp(pr.y, 0.0, ph));
  palong = pr.y / ph;
  return length(pe) * sign(pr.x - pap) - {IFR_R * 0.07:.4f};
}}
float ifr_order(float oj) {{
  return oj >= q26 ? q28 : q27;
}}
float ifr_frame(vec3 fp, float fj, out float falong, out float fface) {{
  vec2 fq = ifr_local(fp.xy, fj);
  float fn = ifr_order(fj);
  float fd = ifr_poly(fq, fn, falong);
  float fz = fp.z - fj * {IFR_P:.3f};
  vec2 fe = vec2(abs(fd - 0.07) - 0.07, abs(fz) - 0.05);
  float fb = length(max(fe, 0.0)) + min(max(fe.x, fe.y), 0.0) - 0.018;
  vec2 fe2 = vec2(abs(fd - 0.2) - 0.02, abs(fz) - 0.022);
  float fb2 = length(max(fe2, 0.0)) + min(max(fe2.x, fe2.y), 0.0) - 0.008;
  float fg = abs(fd - 0.07) - 0.012;
  float fgz = abs(fz) - 0.06;
  fb = max(fb, -max(fg, -fgz + 0.0));
  fface = fd;
  return min(fb, fb2);
}}
float ifr_scene(vec3 sp, out float sj, out float salong, out float sface) {{
  float sj0 = floor(sp.z / {IFR_P:.3f});
  float sa0;
  float sf0;
  float sa1;
  float sf1;
  float sd0 = ifr_frame(sp, sj0, sa0, sf0);
  float sj1 = sj0 + 1.0;
  float sd1 = ifr_frame(sp, sj1, sa1, sf1);
  sj = sd0 < sd1 ? sj0 : sj1;
  salong = sd0 < sd1 ? sa0 : sa1;
  sface = sd0 < sd1 ? sf0 : sf1;
  return min(sd0, sd1);
}}
float ifr_d(vec3 dp) {{
  float da;
  float db;
  float dc;
  return ifr_scene(dp, da, db, dc);
}}
vec3 ifr_lamp() {{
  float lu = q10 + q9 + 5.0;
  vec2 lxy = ifr_path(lu);
  return vec3(lxy + vec2(0.5 * cos(q22), 0.5 * sin(q22)), (q9 + 5.0) * {IFR_P:.3f});
}}
vec3 ifr_inlay(float ij) {{
  float ih = ifr_hash(q10 + ij + 7.0);
  float ipal = ij >= q26 ? q30 : q31;
  vec3 ic = ifr_ramp(0.55 + 0.45 * fract(ih * 0.61 + ipal));
  vec3 iw = mix(NOKKVI_WARM, NOKKVI_HIGHLIGHT, 0.25);
  return mix(ic, iw, step(0.62, fract(ih + ipal * 0.5)));
}}
float ifr_beat(float bdj, float bage, float bstr) {{
  float bfront = bage * 11.0;
  float bx = bdj - bfront;
  return bstr * exp(-bx * bx * 1.3) * exp(-bage * 0.9) * step(0.0, bage);
}}
float ifr_pulse(float pj) {{
  float pdj = pj - q9;
  return ifr_beat(pdj, q1, q2) + ifr_beat(pdj, q4, q6) + ifr_beat(pdj, q7, q8);
}}
vec3 ifr_env(vec3 ed, vec3 efw) {{
  float eg = pow(max(dot(ed, efw), 0.0), 24.0);
  float ey = ed.y;
  vec3 esky = mix(NOKKVI_SURFACE * 0.7, NOKKVI_BG * 0.6, smoothstep(0.0, 0.6, ey));
  esky = mix(esky, NOKKVI_BG * 0.25, smoothstep(0.0, -0.3, ey));
  float ehz = exp(-abs(ey) * 18.0);
  return esky + mix(NOKKVI_ACCENT, NOKKVI_WARM, 0.4) * ehz * (0.18 + 0.1 * q25) + NOKKVI_ACCENT * eg * (0.2 + 0.2 * q25);
}}
float ifr_n(vec2 nx) {{
  vec2 nxx = nx * 256.0 + 0.5;
  vec2 ni = floor(nxx);
  vec2 nf = fract(nxx);
  nf = nf * nf * nf * (nf * (nf * 6.0 - 15.0) + 10.0);
  return texture(sampler_noise_hq, (ni + nf - 0.5) / 256.0).x;
}}
float ifr_h(vec2 hw, float hfade) {{
  vec2 hw1 = hw * 0.0015 + vec2(q24 * 0.0011, q24 * 0.0004);
  vec2 hw2 = hw * 0.0117 + vec2(-q24 * 0.0019, q24 * 0.0013);
  float hwa = ifr_n(hw1);
  float hwb = ifr_n(hw1 * 2.6 + vec2(0.31, 0.57) + hwa * 0.004);
  float hwc = ifr_n(hw2);
  return hwa * 0.5 + hwb * 0.3 + hwc * 0.2 * hfade;
}}
vec3 ifr_liquid(vec2 lq, float lf) {{
  float le = 0.05;
  float lh0 = ifr_h(lq, lf);
  vec2 lqx = lq + vec2(le, 0.0);
  vec2 lqz = lq + vec2(0.0, le);
  float lhx = ifr_h(lqx, lf);
  float lhz = ifr_h(lqz, lf);
  float lamp = 1.4 * (0.35 + 0.65 * lf);
  return vec3((lhx - lh0) / le * lamp, (lhz - lh0) / le * lamp, lh0);
}}
"""

def ifr_frame_colour(v, j, n="n", pos="pos", rd="rd", t="t"):
    """Surface colour of a frame hit: brushed metal tinted by identity."""
    return f"""
  float {v}_h = ifr_hash(q10 + {j});
  float {v}_pal = {j} >= q26 ? q30 : q31;
  vec3 {v}_base = ifr_ramp(0.15 + 0.7 * fract({v}_h * 0.37 + {v}_pal));
  vec3 {v}_lp = ifr_lamp() - {pos};
  float {v}_ld = length({v}_lp);
  vec3 {v}_l = {v}_lp / {v}_ld;
  float {v}_att = (1.6 + 1.5 * q25 + 1.2 * q3) / (1.0 + 0.35 * {v}_ld * {v}_ld);
  vec3 {v}_key = normalize(vec3(cos(q22 * 0.37), 0.7, sin(q22 * 0.37) - 0.6));
  float {v}_dif = max(dot({n}, {v}_l), 0.0) * {v}_att + max(dot({n}, {v}_key), 0.0) * 0.35 + 0.06;
  vec3 {v}_hv = normalize({v}_l - {rd});
  float {v}_spc = pow(max(dot({n}, {v}_hv), 0.0), 60.0) * {v}_att;
  vec3 {v} = {v}_base * {v}_dif + mix(NOKKVI_TEXT, NOKKVI_ACCENT, 0.4) * {v}_spc * 1.4;
"""

IFR_WARP = IFR_FUNCS + " shader_body {\n" + HEAD + f"""
  vec2 p = (uv_orig - 0.5) * s;
  vec3 ro = vec3(q11, q12, q9 * {IFR_P:.3f});
  vec3 fw = normalize(vec3(q13, q14, q15));
  vec3 upr = vec3(sin(q16), cos(q16), 0.0);
  vec3 rt = normalize(cross(fw, upr));
  vec3 up = cross(rt, fw);
  vec3 rd = normalize(fw + (p.x * rt + p.y * up) * q17);
  float pixang = q17 / min(texsize.x, texsize.y);
  float t = 0.02;
  float d = 1.0;
  float hit = 0.0;
  float mind = 1e9;
  float mint = 0.0;
  for (int i = 0; i < 120; i++) {{
    d = ifr_d(ro + rd * t);
    float dr = d / (t * pixang);
    if (dr < mind) {{ mind = dr; mint = t; }}
    if (d < t * pixang * 0.5) {{ hit = 1.0; break; }}
    t += d * 0.9;
    if (t > 34.0) break;
  }}
  float tfl = rd.y < -0.0001 ? ({IFR_FH:.3f} - ro.y) / rd.y : 1e9;
  float onfl = max(step(tfl, t), 1.0 - hit) * step(tfl, 1e8);
  float ts = onfl > 0.5 ? tfl : t;
  vec3 pos = ro + rd * ts;
  float hj;
  float halong;
  float hface;
  ifr_scene(pos, hj, halong, hface);
  float e = max(t * pixang, 0.0008);
  vec3 n = normalize(vec3(
    ifr_d(pos + vec3(e, 0.0, 0.0)) - ifr_d(pos - vec3(e, 0.0, 0.0)),
    ifr_d(pos + vec3(0.0, e, 0.0)) - ifr_d(pos - vec3(0.0, e, 0.0)),
    ifr_d(pos + vec3(0.0, 0.0, e)) - ifr_d(pos - vec3(0.0, 0.0, e))));
  float ao = 0.0;
  for (int k = 1; k <= 3; k++) {{
    float hk = 0.03 * float(k);
    ao += (hk - ifr_d(pos + n * hk)) / hk * (0.5 / float(k));
  }}
  ao = clamp(1.0 - ao, 0.0, 1.0);
  vec2 lw = vec2(pos.x, pos.z + q29);
  float lfade = exp(-ts * 0.12);
  vec3 lg = ifr_liquid(lw, lfade);
  vec3 ln = normalize(vec3(-lg.x, 1.0, -lg.y));
  if (onfl > 0.5) {{ n = ln; ao = 1.0; }}
""" + ifr_frame_colour("sc", "hj") + f"""
  vec3 rfd = reflect(rd, n);
  rfd = onfl > 0.5 ? vec3(rfd.x, abs(rfd.y), rfd.z) : rfd;
  float rt2 = 0.03;
  float rhit = 0.0;
  for (int i = 0; i < 64; i++) {{
    float rdd = ifr_d(pos + rfd * rt2);
    if (rdd < 0.002 * rt2 + 0.001) {{ rhit = 1.0; break; }}
    rt2 += rdd * 0.9;
    if (rt2 > 30.0) break;
  }}
  vec3 rpos = pos + rfd * rt2;
  float rj;
  float ralong;
  float rface;
  ifr_scene(rpos, rj, ralong, rface);
  vec3 rn = normalize(vec3(
    ifr_d(rpos + vec3(0.003, 0.0, 0.0)) - ifr_d(rpos - vec3(0.003, 0.0, 0.0)),
    ifr_d(rpos + vec3(0.0, 0.003, 0.0)) - ifr_d(rpos - vec3(0.0, 0.003, 0.0)),
    ifr_d(rpos + vec3(0.0, 0.0, 0.003)) - ifr_d(rpos - vec3(0.0, 0.0, 0.003))));
""" + ifr_frame_colour("rc", "rj", n="rn", pos="rpos", rd="rfd") + f"""
  float rinl = smoothstep(0.03, 0.0, abs(rface - 0.07));
  float rpl = ifr_pulse(rj);
  vec3 ricol = ifr_inlay(rj);
  rc = rc * 0.6 + mix(ricol, NOKKVI_TEXT, min(rpl * 0.35, 0.6)) * rinl * (0.8 + 0.6 * q3 + 3.0 * rpl);
  vec3 renv = ifr_env(rfd, fw);
  vec3 refl = rhit > 0.5 ? mix(rc, renv, 1.0 - exp(-rt2 * 0.075)) : renv;
  float fre = 0.25 + 0.75 * pow(1.0 - max(dot(n, -rd), 0.0), 3.0);
  float q24t = clamp(treb - treb_att, 0.0, 1.0);
  float inl = smoothstep(0.022, 0.006, abs(hface - 0.07)) * smoothstep(0.03, 0.05, abs(pos.z - hj * {IFR_P:.3f}));
  float wav = 0.5 + 0.5 * sin(q32 - hj * 0.9 + halong * 1.5);
  float hpl = ifr_pulse(hj);
  vec3 hicol = ifr_inlay(hj);
  float dash = 0.5 + 0.5 * sin(halong * 14.0 - q32 * 2.5 + hj * 2.0);
  vec3 emis = mix(hicol, NOKKVI_TEXT, min(hpl * 0.35, 0.6)) * inl
              * (0.6 + 0.5 * wav + 0.6 * q3 * wav + hpl * (2.2 + 1.6 * dash) + 0.5 * q24t * dash);
  vec3 col = (sc * 0.4 + refl * fre * 0.8) * (0.35 + 0.65 * ao) + emis;
  float lcs = max(dot(ln, -rd), 0.0);
  float lfr = 0.1 + 0.9 * pow(1.0 - lcs, 5.0);
  vec3 llp = ifr_lamp() - pos;
  float lld = length(llp);
  float lsp = pow(max(dot(rfd, llp / lld), 0.0), 900.0) * (0.6 + 1.2 * q3) / (1.0 + 0.08 * lld * lld);
  float ldeep = lg.z;
""" + ramp("ltint", "0.1 + 0.3 * ldeep + 0.2 * fract(q23)") + f"""
  vec3 lcol = refl * mix(vec3(1.0), ltint * 1.6, 0.35) * (0.15 + 0.85 * lfr) + ltint * (1.0 - lfr) * (0.05 + 0.08 * ldeep) + mix(NOKKVI_TEXT, NOKKVI_ACCENT, 0.3) * lsp;
  col = onfl > 0.5 ? lcol : col;
  float fog = 1.0 - exp(-ts * (onfl > 0.5 ? 0.06 : 0.075));
  vec3 bg = ifr_env(rd, fw);
  float solid = max(hit, onfl);
  col = solid > 0.5 ? mix(col, bg, fog) : bg;
  float cov = smoothstep(1.0, 0.0, mind);
  vec3 edgec = mix(sc * 0.55 + refl * 0.5, bg, 1.0 - exp(-mint * 0.075));
  float solidf = hit * (1.0 - onfl);
  col = mix(mix(col, edgec, cov * 0.8 * step(mint, ts)), col, solidf);
  col *= 1.0 + 0.12 * q5;
  ret = col;
 }}"""

IFR_COMP = " shader_body {\n" + HEAD + """
  vec2 p = (uv - 0.5) * s;
  vec3 m = texture(sampler_main, uv).xyz;
  vec3 b1 = GetBlur1(uv);
  vec3 b2 = GetBlur2(uv);
  vec3 col = m + max(b1 - 0.3, 0.0) * 0.5 + max(b2 - 0.22, 0.0) * (0.6 + 0.6 * q3);
  col = 1.0 - exp(-col * 1.35);
  col *= 0.8 + 0.2 * smoothstep(1.3, 0.3, length(p));
  ret = col;
 }"""

# Acts: twist per frame, path amps a1/a2, orbit radius, orbit rate, roll rate, spin rate, fov.
IFR_ACTS = [
    (0.04, 0.35, 0.10, 0.00, 0.0, 0.10, 0.10, 1.00),
    (0.22, 0.20, 0.05, 0.10, 0.2, 0.35, 0.25, 1.10),
    (0.08, 1.40, 0.40, 0.10, 0.3, 0.20, 0.05, 0.90),
    (0.12, 0.40, 0.10, 0.45, 0.5, 0.15, -0.20, 1.20),
    (-0.30, 0.30, 0.20, 0.25, -0.6, 0.45, -0.35, 1.30),
]
def ifr_act_pick(var, col):
    e = f"{IFR_ACTS[-1][col]}"
    for i in range(len(IFR_ACTS) - 2, -1, -1):
        e = f"if(equal(act, {i}), {IFR_ACTS[i][col]}, {e})"
    return f"{var} = {e};\n"

IFR_INIT = ("pulse = 0; pop = 0; hue = 0; lite = 0; core = 0; flow = 0; dist = 0; " + SPEED_INIT +
        " act = rand(5); acttm = 12; sgn = 1; tw = 0.05; tw_m = 0.05; a1 = 0.3; a1_m = 0.3; a2 = 0.1; a2_m = 0.1;"
        " orr = 0; orr_m = 0; ora = 0; ora_m = 0; oph = 0; rlr = 0; rlr_m = 0; rlang = 0; spr = 0; spr_m = 0; spin = 0;"
        " fov = 1; fov_m = 1; ks = -100; nold = 3; nnew = 3; pold = rand(1000) / 1000; pnew = pold; lph = 0; lqt = 0; " + BEATS_INIT)
IFR_FRAME = PULSE + "dt = min(1 / max(fps, 1), 0.1);\n" \
    "loud = min((bass_att + mid_att + treb_att) / 3, 2);\n" \
    "acttm = acttm - dt;\nchg = below(acttm, 0);\n" \
    "act = if(chg, (act + 1 + rand(4)) % 5, act);\n" \
    "sgn = if(chg, if(below(rand(100), 50), -1, 1), sgn);\n" \
    "acttm = if(chg, 13 + rand(1000) / 1000 * 8, acttm);\n" \
    "cabs = int(dist);\n" \
    "shp = chg * above(cabs, ks);\n" \
    "nold = if(shp, nnew, nold); pold = if(shp, pnew, pold);\n" \
    "r = rand(100);\n" \
    "nnew = if(shp, if(below(r, 55), 3, if(below(r, 75), 4, if(below(r, 90), 5, 6))), nnew);\n" \
    "pnew = if(shp, rand(1000) / 1000, pnew);\n" \
    f"ks = if(shp, cabs + {IFR_NF + 2}, ks);\n" \
    + ifr_act_pick("twg", 0) + ifr_act_pick("a1g", 1) + ifr_act_pick("a2g", 2) + ifr_act_pick("org", 3) \
    + ifr_act_pick("orag", 4) + ifr_act_pick("rlg", 5) + ifr_act_pick("spg", 6) + ifr_act_pick("fovg", 7) \
    + ease("tw", "twg * sgn", "3.0") + ease("a1", "a1g", "3.5") + ease("a2", "a2g", "3.5") \
    + ease("orr", "org", "3.0") + ease("ora", "orag * sgn", "3.0") + ease("rlr", "rlg * sgn", "3.0") \
    + ease("spr", "spg * sgn", "3.0") + ease("fov", "fovg", "3.0") \
    + SPEED + \
    "dist = dist + spdt * dt;\n" \
    "cabs = int(dist); fr = dist - cabs;\n" \
    "q9 = fr; q10 = cabs % 32;\n" \
    "q20 = a1; q21 = a2;\n" \
    f"pu = q10 + q9; px0 = a1 * cos({IFR_W} * pu) + a2 * cos({3 * IFR_W} * pu + 1.3);\n" \
    f"py0 = 0.25 * (a1 * sin({IFR_W} * pu) + a2 * sin({2 * IFR_W} * pu + 0.4));\n" \
    f"pu = pu + 3; px3 = a1 * cos({IFR_W} * pu) + a2 * cos({3 * IFR_W} * pu + 1.3);\n" \
    f"py3 = 0.25 * (a1 * sin({IFR_W} * pu) + a2 * sin({2 * IFR_W} * pu + 0.4));\n" \
    "oph = oph + ora * dt;\n" \
    "ox = orr * cos(oph); oy = 0.35 * orr * sin(oph);\n" \
    "q11 = px0 + ox; q12 = py0 + oy;\n" \
    "lwx = 0.08 * sin(time * 0.13) - 0.25 * ox; lwy = 0.06 * sin(time * 0.17 + 1) - 0.25 * oy;\n" \
    f"fx = px3 - px0 + lwx * 3; fy = py3 - py0 + lwy * 3; fz = 3 * {IFR_P};\n" \
    "fl = sqrt(fx * fx + fy * fy + fz * fz);\nq13 = fx / fl; q14 = fy / fl; q15 = fz / fl;\n" \
    "rlang = rlang + dt * 0.21; q16 = rlr * sin(rlang);\n" "lqt = lqt + dt * (0.3 + 0.5 * spdt); q24 = lqt;\n" "q29 = cabs % 256;\n" \
    "q17 = fov * (1 + 0.08 * min(sg, 2) + 0.03 * max(spd - 1, 0));\n" \
    "spin = spin + spr * dt * (0.4 + 0.6 * spdt); spin = spin - 6.2831853 * int(spin / 6.2831853); q18 = spin;\n" \
    "q19 = tw;\n" \
    "lph = lph + dt * (0.4 + 0.5 * loud); q22 = lph;\n" \
    "hue = hue + dt * 0.004; q23 = hue;\n" \
    "core = core + (min(bass_att, 2) - core) * (1 - exp(-dt / 0.4)); q25 = core;\n" \
    "q26 = ks - cabs; q27 = nold; q28 = nnew; q30 = pnew + hue; q31 = pold + hue;\n" \
    "flow = flow + dt * (1.5 + 4 * q3); q32 = flow;\n" \
    + BEATS + \
    "q1 = ba1; q2 = bs1; q4 = ba2; q6 = bs2; q7 = ba3; q8 = bs3;\n"


presets["nokkvi - infinity"] = preset({"decay": 0.0, "wave_a": 0.0, "zoom": 1.0}, IFR_WARP, IFR_COMP,
                                     init=IFR_INIT, frame=IFR_FRAME)

# Fjord (no cover) -----------------------------------------------------------
# A low flight down a winding Norwegian fjord: cliffs rise almost straight out
# of the water and round over into a snowy plateau. The land and its lighting
# hold still (sun and rock tint are fixed per visit); the music lives on the
# water and in the sky. The feedback texture is not a picture here, it is
# data: the warp keeps a band block in the texture's first rows and columns
# and bakes a height cache of the valley beside it, and the comp raymarches the
# landscape from that cache, sharp every frame (the comp never shows the
# feedback).
# Kicks: each kick (bass over its follower, 0.5 s cooldown) launches a light
# under the water just ahead of the camera that races down the fjord in a lane
# beside the centre line (left and right alternate), following the river's
# bends (the track is straight in channel coordinates: u = x - path(z) across,
# z along), and fades into the distance. On the surface it leaves
# a Kelvin-style wake (divergent crests along the arms at the 19.5 degree
# cusp, transverse crests inside the V, a bow swell) and a splash ring where it
# dove in; these bend the water's normal, so reflections of the cliffs and sky
# ripple. The light is seen through that surface: the view ray is refracted
# into the water and the glow is looked up where it reaches the light's depth,
# so the wake distorts the glow, with a sparkling trail of stirred plankton
# behind it and the wake crests catching its light. Beat never touches terrain.
# Bands: FJ_NB log-spaced bands (40 Hz to 14 kHz, from get_fft) across. Row 0
# is a per-band running average (AGC, log encoded, ~10 s), row 1 a follower of
# the level over that average (fast attack, 0.2 s release), row 2 the
# follower's own slow average, row 3 the flow: how far the follower stands
# above its band's own normal, 0..1. At night the aurora brightens with it.
# Rates come from the fps/frame uniforms. Values are 16 bit, split high/low
# over two 8-bit channels; the split is linear, so bilinear taps interpolate
# true values. (The engine's feedback mips pick one texel per block rather
# than averaging, so nothing here reads them.) The block sits where the
# engine's resize keeps the old feedback (low x, low y), so it survives a
# resize; only the first frames are wiped.
# Cache: at most FJ_CWMAX x FJ_CHMAX texels, x = depth, y = u = x - river
# path(z) (uniform). Depth texels are anchored to the world: spacing 256/N (N
# texels per 256 units, so the grid survives the wrap) from an origin snapped
# to that spacing, so a world point is sampled identically every frame (a
# camera-relative grid made small peaks and crests crawl). Each texel is the
# full height: water that swells and narrows along the path with hashed
# headlands; a cliff profile that plunges below the water and rises steeply to
# a convex shoulder, with a hashed bench part-way up; buttresses and bulges from
# ridged noise about as fine up the face as along it (vertical-only relief read
# as organ pipes); a rolling plateau with far peaks; gully erosion only on
# shoulders and slopes (on a cliff its small height steps became flutes).
# The rock is one surface, the cubic B-spline of the cache: the march reads it
# directly within 6 units and beyond that lets the bilinear cache (one read
# per step) only propose hits that the B-spline confirms, backtracking until
# the start of the refinement is above it; shadows and cavity taps near the
# surface read it too, with the unbumped normal. (Mixing the two surfaces left
# hits floating in front of the rock and self-shadowed crests, which popped as
# the camera moved.) The march stops where the cache ends (fog has closed by
# then); fine rock detail goes into the lighting normal only, by triplanar
# projection (horizontal-only detail striped the cliffs vertically). Nothing
# on the rock is screen-locked or view-sampled: no per-pixel jitter, and the
# valley mist is looked up at the hit point and does not drift.
# Scene: matte low-albedo rock with stain streaks and a wet tide band, snow on
# the flat tops, cavity AO from taps along the normal, soft shadows, a liquid plane at y = 0 (ripples streaked along the
# view, the wakes, Fresnel, a reflection march with shadows, depth tint, a foam
# fringe at the shore), sky with drifting clouds, sunlight reddened by airmass,
# height fog and patchy valley mist, filmic tone map.
# Each visit picks a time of day from the audio at frame 3 (the rand stream
# replays per preset): overcast day with a side sun, golden hour with a low sun
# beside the valley's axis, or night with the moon, stars and aurora curtains
# marched through a slab of sky (so they have perspective). The camera eases
# between four altitudes (skim, glide, soar, high; all below the rim) every
# 14-22 s, drifts sideways within the channel and banks into the river's bends
# ahead; loudness sets the speed, kicks a softened surge. The nearest wall foot
# stays at least 1.75 units from the channel's centre line and the drift is at
# most 0.5, so the camera is always over water.
# Everything world-space is periodic in 256 along z (path, width, cliffs,
# spurs, hashes mod 64 at multiples of 0.25 per unit, snow line, sparkle cells,
# clouds via q25, flow mod 640), so the camera's wrap at 256 is seamless.
# Dream (after the mandelbox explorer's soft, echoing look): the comp also reads
# its own previous frame (sampler_prev_comp, with averaged blur levels) and
# blends it in after the tone map. The warp keeps the camera as 24-bit values
# (three channels) in a small block under the bands: row FJ_DH this frame's,
# row FJ_DH + 1 a copy of the last frame's, which the comp reads to reproject:
# each pixel's world point (rock hit; on water the reflection's mirror image
# along the view ray, its inverse depth weighted by Fresnel so a faint sky
# reflection stays near the surface; sky by direction) is projected with last
# frame's camera and the history is sampled there. The rock takes no sharp echo
# (resampling it every frame only softens detail in drifting bands); what moves
# or depends on the view on the water and in the sky (lights, wakes, plankton,
# ripples, aurora) leaves a short echo (q17). The blur taps read one level finer
# than their spread, so a moving glow slides instead of stepping. On top, a
# depth of field focused on the water where the lights run (q3, aperture q24;
# the sky's blur capped at FJ_SKYCOC so stars and aurora survive) pulls each
# pixel toward the blurred history, never less than FJ_HALO: a soft halo
# everywhere and a glowing, softened distance. The history weight is zero for
# the first frames, off screen, behind the old camera and where the history has
# no alpha (the engine clears it on a resize). Every blend is convex, so the loop
# gain stays below 1; a 1/255 dither keeps the 8-bit history from sticking.
# q map: pulse slots (packed origin; age ms + strength * 0.3 in one q) in q1/q2,
# q5/q6, q18/q23, q26/q28, q30/q31; q3 focus distance, q17 echo, q24 aperture;
# q4 camera z (mod 256), q7/q8 camera x/y, q9 night,
# q10-q12 forward, q13 roll, q14 lens, q15/q16 key light (sun, or moon at
# night) azimuth/elevation, q19 liquid flow, q20 loudness, q21 treble shimmer,
# q22 rock tint, q25 cloud drift, q27 aurora strength; q29, q32 free.
FJ_NB = 16
FJ_DW = FJ_NB
FJ_DH = 4
FJ_SMAX = 4.0
FJ_HMIN = -1.0
FJ_HMAX = 10.0
FJ_U = 12.0
FJ_ZB = 3.0
FJ_ZL = 90.0
FJ_CWMAX = 1400
FJ_CHMAX = 800
FJ_W1 = 6.2831853 * 3 / 256
FJ_W2 = 6.2831853 * 8 / 256
FJ_A1, FJ_A2 = 2.2, 0.9
FJ_PV = 9.0
FJ_PLIFE = 2.5
FJ_CAMN = 8
FJ_NEARCOC = 0.5
FJ_DOFW = 0.6
FJ_HALO = 0.2
FJ_HALOR = 0.004
FJ_COCFULL = 0.008
FJ_SKYCOC = 0.35
FJ_ECHO = 0.35
FJ_APER = 0.0035
FJ_FOCUS = 4.0

FJ_COMMON = f"""
float fj_dec(vec2 dc) {{
  return (dc.x * 65280.0 + dc.y * 255.0) / 65535.0;
}}
vec2 fj_enc(float ev) {{
  float ex = floor(clamp(ev, 0.0, 1.0) * 65535.0 + 0.5);
  float eh = floor(ex / 256.0);
  return vec2(eh, ex - eh * 256.0) / 255.0;
}}
float fj_dec24(vec3 dd) {{
  vec3 db = floor(dd * 255.0 + 0.5);
  return (db.x * 65536.0 + db.y * 256.0 + db.z) / 16777215.0;
}}
vec3 fj_enc24(float ev) {{
  float ex = floor(clamp(ev, 0.0, 1.0) * 16777215.0 + 0.5);
  float er = floor(ex / 65536.0);
  float eg = floor((ex - er * 65536.0) / 256.0);
  return vec3(er, eg, ex - er * 65536.0 - eg * 256.0) / 255.0;
}}
float fj_path(float pz) {{
  return {FJ_A1} * sin({FJ_W1:.9f} * pz) + {FJ_A2} * sin({FJ_W2:.9f} * pz + 1.7);
}}
float fj_width(float wz) {{
  return 3.0 + 0.8 * sin({6.2831853 * 5 / 256:.9f} * wz + 0.6) + 0.4 * sin({6.2831853 * 11 / 256:.9f} * wz + 2.1);
}}
float fj_cliff(float cz, float cs) {{
  return 4.3 + 0.9 * sin({6.2831853 * 7 / 256:.9f} * cz + cs * 1.3) + 0.5 * sin({6.2831853 * 17 / 256:.9f} * cz + 0.7 + cs * 2.0);
}}
vec2 fj_duv(float fdi, float fdj) {{
  return (vec2(fdi, fdj) + 0.5) * texsize.zw;
}}
float fj_cw() {{
  return clamp(texsize.x - {FJ_DW + 1}.0, 8.0, {FJ_CWMAX}.0);
}}
float fj_ch() {{
  return min(texsize.y, {FJ_CHMAX}.0);
}}
float fj_dzt() {{
  return 256.0 / max(floor(256.0 * fj_cw() / {FJ_ZL} + 0.5), 1.0);
}}
float fj_z0() {{
  float zd = fj_dzt();
  return (floor(q4 / zd) - ceil({FJ_ZB} / zd)) * zd;
}}
vec3 fj_tex(vec2 tuv) {{
  return textureLod(sampler2D(sampler_fc_main, sampler_fc_main_samp), tuv, 0.0).xyz;
}}
vec3 fj_bspline(vec2 bc, vec2 bbase) {{
  vec2 bi0 = floor(bc);
  vec2 bf = bc - bi0;
  vec2 bf2 = bf * bf;
  vec2 bf3 = bf2 * bf;
  vec2 bw0 = (1.0 - 3.0 * bf + 3.0 * bf2 - bf3) / 6.0;
  vec2 bw1 = (4.0 - 6.0 * bf2 + 3.0 * bf3) / 6.0;
  vec2 bw2 = (1.0 + 3.0 * bf + 3.0 * bf2 - 3.0 * bf3) / 6.0;
  vec2 bw3 = bf3 / 6.0;
  vec2 bg0 = bw0 + bw1;
  vec2 bg1 = bw2 + bw3;
  vec2 bh0 = bbase + bi0 - 0.5 + bw1 / bg0;
  vec2 bh1 = bbase + bi0 + 1.5 + bw3 / bg1;
  vec2 bt00 = bh0 * texsize.zw;
  vec2 bt10 = vec2(bh1.x, bh0.y) * texsize.zw;
  vec2 bt01 = vec2(bh0.x, bh1.y) * texsize.zw;
  vec2 bt11 = bh1 * texsize.zw;
  vec3 bs00 = fj_tex(bt00);
  vec3 bs10 = fj_tex(bt10);
  vec3 bs01 = fj_tex(bt01);
  vec3 bs11 = fj_tex(bt11);
  return (bs00 * bg0.x + bs10 * bg1.x) * bg0.y + (bs01 * bg0.x + bs11 * bg1.x) * bg1.y;
}}
float fj_hash(vec2 hp) {{
  vec2 hq = mod(hp, 64.0);
  vec3 h3 = fract(vec3(hq.xyx) * 0.1031);
  h3 += dot(h3, h3.yzx + 33.33);
  return fract((h3.x + h3.y) * h3.z);
}}
vec3 fj_noised(vec2 nx) {{
  vec2 ni = floor(nx);
  vec2 nf = nx - ni;
  vec2 nu = nf * nf * nf * (nf * (nf * 6.0 - 15.0) + 10.0);
  vec2 ndu = 30.0 * nf * nf * (nf * (nf - 2.0) + 1.0);
  vec2 n10 = ni + vec2(1.0, 0.0);
  vec2 n01 = ni + vec2(0.0, 1.0);
  vec2 n11 = ni + vec2(1.0, 1.0);
  float na = fj_hash(ni);
  float nb = fj_hash(n10);
  float nc = fj_hash(n01);
  float nd = fj_hash(n11);
  float k1 = nb - na;
  float k2 = nc - na;
  float k4 = na - nb - nc + nd;
  return vec3(na + k1 * nu.x + k2 * nu.y + k4 * nu.x * nu.y,
              ndu * vec2(k1 + k4 * nu.y, k2 + k4 * nu.x));
}}
"""

FJ_WARP_FUNCS = FJ_COMMON + f"""
float fj_eroded(vec2 ep) {{
  vec2 eq = ep * 0.25;
  float ea = 0.0;
  float eb = 1.0;
  vec2 ed = vec2(0.0);
  for (int k = 0; k < 4; k++) {{
    vec3 en = fj_noised(eq);
    ed += en.yz;
    ea += eb * en.x / (1.0 + dot(ed, ed));
    eb *= 0.45;
    eq = vec2(2.0 * eq.x - eq.y, eq.x + 2.0 * eq.y) + vec2(0.37, 0.71);
  }}
  return ea;
}}
float fj_spur(float sz, float sper, float ssalt, float swid, out float sside) {{
  float scell = floor(sz / sper);
  float sbest = 0.0;
  sside = 1.0;
  for (int k = -1; k <= 1; k++) {{
    float sc = scell + float(k);
    float scm = mod(sc, {256.0} / sper);
    vec2 sh1 = vec2(scm, ssalt);
    vec2 sh2 = vec2(scm, ssalt + 13.0);
    vec2 sh3 = vec2(scm, ssalt + 29.0);
    float szs = (sc + 0.2 + 0.6 * fj_hash(sh1)) * sper;
    float sdz = (sz - szs) / swid;
    float samp = (0.5 + 0.5 * fj_hash(sh3)) * exp(-sdz * sdz);
    float sgn = fj_hash(sh2) < 0.5 ? -1.0 : 1.0;
    sside = samp > sbest ? sgn : sside;
    sbest = max(sbest, samp);
  }}
  return sbest;
}}
float fj_base(float gu, float gzw) {{
  float gsd0 = gu < 0.0 ? -1.0 : 1.0;
  float gau = abs(gu);
  float gsd;
  float gsp = fj_spur(gzw, 25.6, 3.0, 3.0, gsd);
  float gon = step(0.0, gu * gsd);
  float gwf = max(fj_width(gzw) - gsp * gon * 1.3, 2.0);
  vec2 gp = vec2(gu, gzw);
  float gn = fj_eroded(gp);
  float gcl = fj_cliff(gzw, gsd0);
  float grr = gcl / 3.0;
  float gx0 = clamp((gau - gwf) / grr, 0.0, 1.0);
  float gyf = gx0 * gcl;
  vec2 gfq = vec2(gzw * 0.75 + gyf * 0.3, gyf * 0.75 + gsd0 * 5.0);
  vec3 gfn = fj_noised(gfq);
  vec2 gfq2 = vec2(gzw * 2.25 - gyf * 0.9 + 0.3, gyf * 1.9 + gzw * 0.5 + 1.7);
  vec3 gfn2 = fj_noised(gfq2);
  float gbut = (1.0 - abs(2.0 * gfn.x - 1.0)) * 0.4 + (1.0 - abs(2.0 * gfn2.x - 1.0)) * 0.13;
  float gaa = gau - gwf - gbut + 0.6 * (gn - 0.35);
  float gx = clamp(gaa / grr, -0.4, 1.0);
  vec2 gbq = vec2(gzw * 0.25, gsd0 * 7.0);
  vec3 gbn = fj_noised(gbq);
  float gbz = 0.35 + 0.25 * gbn.x;
  float gbw = 0.1 + 0.12 * gbn.x;
  float gxlo = gx * (gbz + 0.4 * gbw) / gbz;
  float gxmid = gbz + 0.4 * gbw + (gx - gbz) * 0.6;
  float gxb = mix(gxlo, mix(gxmid, gx, step(gbz + gbw, gx)), step(gbz, gx));
  float gh0 = max(gcl * (1.0 - pow(1.0 - gxb, 2.4)), -0.9);
  float gpl = max(gaa - grr, 0.0);
  float gpw = smoothstep(0.0, 2.5, gpl);
  gh0 += gpw * ((gn - 0.35) * 1.2 + 0.06 * gpl);
  gh0 += smoothstep(2.0, 7.0, gpl) * max(gn - 0.3, 0.0) * 2.5;
  return gh0;
}}
vec3 fj_gully(vec2 lp, vec2 ldir) {{
  vec2 li = floor(lp);
  vec2 lf = lp - li;
  vec3 lva = vec3(0.0);
  float lwt = 0.0;
  for (int i = -1; i <= 1; i++) {{
    for (int j = -1; j <= 1; j++) {{
      vec2 lo = vec2(float(i), float(j));
      vec2 lc = li + lo;
      vec2 lc2 = lc + vec2(17.0, 31.0);
      vec2 lc3 = lc + vec2(5.0, 11.0);
      vec2 lh = vec2(fj_hash(lc), fj_hash(lc2)) * 0.5;
      float lk = 0.6 + 0.8 * fj_hash(lc3);
      vec2 lpp = lf - lo - lh;
      float ld = dot(lpp, lpp);
      float lw = exp(-ld * 2.0);
      lwt += lw;
      float lmag = dot(lpp, ldir) * 6.2831853 * lk;
      float lrid = pow(1.0 - abs(cos(lmag)), 1.5);
      lva += vec3(lrid, -sin(lmag) * ldir * lk) * lw;
    }}
  }}
  return lva / lwt;
}}
float fj_height(float cu, float czw, out float cmat) {{
  float ce = 0.06;
  float ch0 = fj_base(cu, czw);
  float chu = fj_base(cu + ce, czw);
  float chz = fj_base(cu, czw + ce);
  vec2 cg = vec2(chu - ch0, chz - ch0) / ce;
  float cslope = length(cg);
  vec2 cp = vec2(cu, czw) * 1.5;
  vec2 cdir = vec2(cg.y, -cg.x) / max(cslope, 0.001) * 1.1;
  vec3 cacc = vec3(0.0);
  float cam = 0.5;
  float cf = 1.0;
  for (int k = 0; k < 3; k++) {{
    vec2 cpk = cp * cf;
    vec2 cdk = cdir + cacc.zy * vec2(1.0, -1.0) * 0.35;
    vec3 cgl = fj_gully(cpk, cdk);
    cacc += cgl * cam * vec3(1.0, cf, cf);
    cam *= 0.42;
    cf *= 2.0;
  }}
  vec2 cmq = vec2(cu, czw) * 0.25 + vec2(0.43, 0.17);
  vec3 cmn = fj_noised(cmq);
  float cgate = smoothstep(0.32, 0.62, cmn.x);
  float cstr = smoothstep(0.35, 1.2, cslope) * smoothstep(2.6, 1.4, cslope) * (0.14 + 0.12 * smoothstep(2.0, 6.0, abs(cu))) * cgate;
  float cmw = smoothstep(0.4, 1.1, cslope) * smoothstep(2.6, 1.4, cslope) * cgate;
  cmat = mix(0.55, clamp(0.5 + (cacc.x - 0.3) * 1.8, 0.0, 1.0), cmw);
  return ch0 + (cacc.x - 0.3) * cstr;
}}
"""

FJ_WARP = FJ_WARP_FUNCS + f"""
float fj_camv(float cvi) {{
  float cvv = q4 / 256.0;
  if (cvi > 0.5) cvv = (q7 + 8.0) / 16.0;
  if (cvi > 1.5) cvv = (q8 + 1.0) / 8.0;
  if (cvi > 2.5) cvv = (q10 + 1.0) * 0.5;
  if (cvi > 3.5) cvv = (q11 + 1.0) * 0.5;
  if (cvi > 4.5) cvv = (q12 + 1.0) * 0.5;
  if (cvi > 5.5) cvv = (q13 + 2.0) * 0.25;
  if (cvi > 6.5) cvv = q14 * 0.25;
  return cvv;
}}
""" + " shader_body {\n" + f"""
  vec2 tx = uv_orig * texsize.xy;
  vec3 outc = vec3(0.0);
  float dj = floor(tx.y);
  if (tx.x < {FJ_DW}.0 && dj < {FJ_DH}.0) {{
    float di = floor(tx.x);
    float clr = step(frame, 2.5);
    float fdt = min(1.0 / max(fps, 1.0), 0.1);
    float fagt = min(10.0, 0.3 + frame / 60.0 * 0.8);
    float fslow = 1.0 - exp(-fdt / fagt);
    float fatk = 1.0 - exp(-fdt / 0.03);
    float frel = 1.0 - exp(-fdt / 0.2);
    vec2 duv0 = fj_duv(di, dj);
    float fcur = 0.0;
    for (int k = 0; k < 6; k++) {{
      float fq = 40.0 * pow(350.0, (di + (float(k) + 0.5) / 6.0) / {FJ_NB}.0);
      fcur += get_fft_hz(fq);
    }}
    fcur /= 6.0;
    vec2 agcuv = fj_duv(di, 0.0);
    vec3 agc = fj_tex(agcuv);
    float favg = exp2(fj_dec(agc.xy) * 18.0 - 14.0);
    float fval = fcur / max(favg, 0.001);
    if (dj < 0.5) {{
      float fnew = mix(favg, fcur, fslow);
      fnew = mix(fnew, 0.05, clr);
      outc = vec3(fj_enc((log2(max(fnew, 0.0001)) + 14.0) / 18.0), 0.0);
    }} else {{
      vec2 penduv = fj_duv(di, 1.0);
      vec3 pend = fj_tex(penduv);
      float ffol = fj_dec(pend.xy) * {FJ_SMAX};
      vec2 meanuv = fj_duv(di, 2.0);
      vec3 mrow = fj_tex(meanuv);
      float fmean = max(fj_dec(mrow.xy) * {FJ_SMAX}, 0.2);
      vec3 wiped = vec3(0.0);
      if (dj < 1.5) {{
        float fgo = min(fval, {FJ_SMAX});
        float frate = fgo > ffol ? fatk : frel;
        float fnew = ffol + (fgo - ffol) * frate;
        outc = vec3(fj_enc(fnew / {FJ_SMAX}), 0.0);
      }} else if (dj < 2.5) {{
        float fmn = fmean + (ffol - fmean) * fslow;
        outc = vec3(fj_enc(fmn / {FJ_SMAX}), 0.0);
        wiped = vec3(fj_enc(1.6 / {FJ_SMAX}), 0.0);
      }} else {{
        float flw = clamp((ffol - 0.95 * fmean) / (0.65 * fmean), 0.0, 1.0);
        outc = vec3(fj_enc(flw), 0.0);
      }}
      outc = mix(outc, wiped, clr);
    }}
  }} else if (tx.x < {FJ_CAMN}.0 && dj < {FJ_DH + 2}.0) {{
    float ci = floor(tx.x);
    vec3 ccur = fj_enc24(fj_camv(ci));
    vec2 clastuv = fj_duv(ci, {FJ_DH}.0);
    vec3 clast = fj_tex(clastuv);
    outc = mix(clast, ccur, step(dj, {FJ_DH}.5));
  }} else {{
    float cw = fj_cw();
    float chh = fj_ch();
    float cxl = tx.x - {FJ_DW + 1}.0;
    if (cxl >= 0.0 && cxl < cw && tx.y < chh) {{
      float ca = tx.y / chh;
      float cu = {FJ_U} * (2.0 * ca - 1.0);
      float czd = fj_dzt();
      float czn = floor(256.0 / czd + 0.5);
      float czi = floor(q4 / czd) - ceil({FJ_ZB} / czd) + floor(cxl);
      float czw = mod(czi, czn) * czd;
      float cmat;
      float ch = fj_height(cu, czw, cmat);
      outc = vec3(fj_enc((ch - {FJ_HMIN}) / {FJ_HMAX - FJ_HMIN}), cmat);
    }}
  }}
  ret = outc;
 }}"""

FJ_COMP_FUNCS = FJ_COMMON + f"""
vec2 fj_cuv(float qu, float qzr) {{
  float qcw = fj_cw();
  float qch = fj_ch();
  float qca = 0.5 + 0.5 * clamp(qu / {FJ_U}, -1.0, 1.0);
  float qci = (q4 + qzr - fj_z0()) / fj_dzt();
  return vec2(({FJ_DW + 1}.0 + clamp(qci, 0.0, qcw - 1.0) + 0.5) * texsize.z, clamp(qca * qch, 0.5, qch - 0.5) * texsize.w);
}}
float fj_h(vec3 hq3) {{
  float hu = hq3.x - fj_path(hq3.z);
  float hzr = hq3.z - q4;
  vec2 huv = fj_cuv(hu, hzr);
  vec3 hc = fj_tex(huv);
  return {FJ_HMIN} + {FJ_HMAX - FJ_HMIN} * fj_dec(hc.xy);
}}
vec3 fj_hs(vec3 sp) {{
  float su = sp.x - fj_path(sp.z);
  float szr = sp.z - q4;
  vec2 suv = fj_cuv(su, szr);
  vec2 sc = suv * texsize.xy - 0.5;
  sc = clamp(sc, vec2({FJ_DW + 2}.0, 1.0), vec2({FJ_DW + 1}.0 + fj_cw() - 3.0, fj_ch() - 3.0));
  vec2 sbase = vec2(0.0);
  vec3 sv = fj_bspline(sc, sbase);
  return vec3({FJ_HMIN} + {FJ_HMAX - FJ_HMIN} * fj_dec(sv.xy), sv.z, su);
}}
vec3 fj_nrm(vec3 np, float ne, out float nmat) {{
  vec3 nh0 = fj_hs(np);
  nmat = nh0.y;
  vec3 npx = np + vec3(ne, 0.0, 0.0);
  vec3 npz = np + vec3(0.0, 0.0, ne);
  vec3 nhx = fj_hs(npx);
  vec3 nhz = fj_hs(npz);
  return normalize(vec3(-(nhx.x - nh0.x) / ne, 1.0, -(nhz.x - nh0.x) / ne));
}}
vec2 fj_detail(vec2 dq) {{
  vec2 dx = dq * 1.5;
  vec2 dg = vec2(0.0);
  float dam = 1.5;
  mat2 djm = mat2(1.5, 0.0, 0.0, 1.5);
  for (int k = 0; k < 4; k++) {{
    vec3 dn = fj_noised(dx);
    dg += dam * (dn.yz * djm);
    dam *= 0.42;
    dx = vec2(2.0 * dx.x - dx.y, dx.x + 2.0 * dx.y) + vec2(0.37, 0.71);
    djm = mat2(2.0, 1.0, -1.0, 2.0) * djm;
  }}
  return dg;
}}
vec2 fj_ptrack(int pk, out float page, out float pstr) {{
  float po = pk == 0 ? q1 : (pk == 1 ? q5 : (pk == 2 ? q18 : (pk == 3 ? q26 : q30)));
  float pq = pk == 0 ? q2 : (pk == 1 ? q6 : (pk == 2 ? q23 : (pk == 3 ? q28 : q31)));
  page = floor(pq) / 1000.0;
  pstr = fract(pq) / 0.3;
  float psd = po < 0.0 ? -1.0 : 1.0;
  float pz0 = abs(po) - 1.0;
  return vec2(psd * (0.55 + 0.45 * fract(pz0 * 0.618)), pz0);
}}
float fj_cam(float cri) {{
  vec2 cruv = fj_duv(cri, {FJ_DH + 1}.0);
  return fj_dec24(fj_tex(cruv));
}}
float fj_penv(float eage) {{
  return smoothstep(0.0, 0.15, eage) * smoothstep({FJ_PLIFE}, {FJ_PLIFE - 0.45}, eage);
}}
float fj_wakeh(vec2 wkr, vec2 wkd, float wktrav, float wkage) {{
  vec2 wkc = wkr - wkd * wktrav;
  float wka = -dot(wkc, wkd);
  float wkb = abs(wkd.x * wkc.y - wkd.y * wkc.x);
  float wkh = exp(-dot(wkc, wkc) * 5.0) * 0.8;
  float wkedge = max(wka, 0.0) * 0.3536;
  float wkwid = 0.05 + 0.08 * max(wka, 0.0);
  float wkbd = (wkb - wkedge) / wkwid;
  float wkarm = exp(-wkbd * wkbd);
  float wkdiv = sin(14.0 * (0.35 * wka + 0.94 * wkb)) * wkarm;
  float wkin = smoothstep(wkedge + 0.05, wkedge * 0.4, wkb);
  float wktr = sin(9.0 * wka) * wkin * 0.45;
  float wklive = step(0.0, wka) * smoothstep(wktrav + 0.4, wktrav - 0.4, wka) * exp(-wka * 0.16);
  wkh += (wkdiv + wktr) * wklive;
  float wkrr = length(wkr);
  float wkrx = wkrr - 0.2 - 3.2 * wkage;
  wkh += sin(wkrx * 11.0) * exp(-wkrx * wkrx * 3.0) * exp(-wkage * 1.4) * 0.7;
  return wkh;
}}
vec3 fj_glowcol() {{
  return mix(NOKKVI_ACCENT, NOKKVI_HIGHLIGHT, 0.35) * 1.8;
}}
vec3 fj_sun() {{
  return normalize(vec3(sin(q15) * cos(q16), sin(q16), cos(q15) * cos(q16)));
}}
float fj_gold() {{
  return smoothstep(0.32, 0.04, q16) * (1.0 - q9);
}}
vec3 fj_airmass() {{
  float am = 1.0 / max(sin(max(q16, 0.0)) + 0.02, 0.03);
  vec3 at = exp(-vec3(0.018, 0.05, 0.11) * am);
  return at / max(at.r, 0.05) * mix(1.0, at.r, 0.5);
}}
vec3 fj_suncol() {{
  float kgw = fj_gold();
  vec3 kbase = mix(NOKKVI_TEXT, NOKKVI_HIGHLIGHT, 0.25);
  kbase = mix(kbase, mix(NOKKVI_WARM, NOKKVI_TEXT, 0.4), kgw * 0.5);
  vec3 kday = kbase * fj_airmass() * mix(2.5, 3.6, kgw);
  vec3 kmc = mix(NOKKVI_TEXT, NOKKVI_ACCENT, 0.35) * 0.75;
  return mix(kday, kmc, q9);
}}
vec3 fj_ambient() {{
  vec3 aday = (NOKKVI_ACCENT * 0.35 + NOKKVI_SURFACE * 0.5 + vec3(0.05)) * 1.3;
  vec3 agold = NOKKVI_ACCENT * 0.3 + NOKKVI_WARM * 0.2 + NOKKVI_SURFACE * 0.4;
  vec3 anight = NOKKVI_BG * 0.9 + NOKKVI_ACCENT * 0.12;
  float agw = fj_gold();
  return mix(mix(aday, agold, agw), anight, q9);
}}
vec3 fj_skybase(vec3 kd, vec3 ksun) {{
  float ky = max(kd.y, 0.0);
  float kgw = fj_gold();
  vec3 kzen = mix(NOKKVI_BG * 0.9 + NOKKVI_ACCENT * 0.12, NOKKVI_BG * 0.7 + NOKKVI_ACCENT * 0.1 + NOKKVI_WARM * 0.05, kgw);
  vec3 khor = mix(NOKKVI_WARM, NOKKVI_ACCENT, 0.45) * 0.55 + NOKKVI_SURFACE * 0.45 + vec3(0.03);
  khor = mix(khor, NOKKVI_WARM * 0.9 + NOKKVI_HIGHLIGHT * 0.25, kgw * 0.8);
  vec3 kc = mix(khor, kzen, pow(ky, 0.5));
  vec3 knight = mix(NOKKVI_BG * 0.35 + NOKKVI_ACCENT * 0.04, NOKKVI_BG * 0.18, pow(ky, 0.4));
  kc = mix(kc, knight, q9);
  float kdot = max(dot(kd, ksun), 0.0);
  vec3 kglow = mix(NOKKVI_HIGHLIGHT, NOKKVI_WARM, 0.4 + 0.4 * kgw) * fj_airmass();
  kc += kglow * (pow(kdot, 6.0) * (0.3 + 0.5 * kgw) + pow(kdot, 48.0) * 0.5) * (1.0 - q9);
  kc += mix(NOKKVI_TEXT, NOKKVI_ACCENT, 0.3) * pow(kdot, 30.0) * 0.12 * q9;
  return kc;
}}
float fj_hash2(vec2 hp) {{
  vec3 h3 = fract(vec3(hp.xyx) * 0.1031);
  h3 += dot(h3, h3.yzx + 33.33);
  return fract((h3.x + h3.y) * h3.z);
}}
vec3 fj_aurora(vec3 ad) {{
  vec3 aacc = NOKKVI_ACCENT * 0.035 * exp(-max(ad.y, 0.0) * 7.0) * (0.5 + 0.5 * q27);
  if (ad.y < 0.012) return aacc;
  vec3 alow = NOKKVI_ACCENT * 1.1 + NOKKVI_HIGHLIGHT * 0.3;
  vec3 ahigh = mix(NOKKVI_WARM, NOKKVI_ACCENT, 0.35) * 0.7;
  vec2 ajq = floor(ad.xy * 3000.0);
  float ajit = fj_hash2(ajq);
  for (int i = 0; i < 16; i++) {{
    float afi = float(i) + ajit;
    float ahg = 20.0 + afi * 1.7;
    float att = (ahg - q8) / ad.y;
    vec2 axz = vec2(q7 + ad.x * att, ad.z * att);
    float acz = 120.0 + 28.0 * sin(axz.x * 0.012 + (time * 0.25) * 0.035) + 9.0 * sin(axz.x * 0.043 - (time * 0.25) * 0.06 + 1.3)
              + 3.0 * sin(axz.x * 0.11 + (time * 0.25) * 0.2);
    float add = axz.y - acz;
    float acv = exp(-add * add * 0.03);
    float acz2 = 175.0 + 30.0 * sin(axz.x * 0.009 - (time * 0.25) * 0.03 + 2.0) + 7.0 * sin(axz.x * 0.05 + (time * 0.25) * 0.05);
    float add2 = axz.y - acz2;
    acv += 0.7 * exp(-add2 * add2 * 0.02);
    vec2 arq = vec2(axz.x * 0.5, (time * 0.25) * 0.35 + afi * 0.02);
    vec3 arn = fj_noised(arq);
    vec2 arq2 = vec2(axz.x * 1.5 + 3.1, (time * 0.25) * 0.9);
    vec3 arn2 = fj_noised(arq2);
    float arays = 0.25 + 0.75 * arn.x * (0.45 + 0.55 * arn2.x);
    float ahp = afi / 15.0;
    float aprof = exp(-ahp * 2.6) * smoothstep(0.0, 0.1, ahp + 0.03);
    float abi = clamp(0.5 + axz.x / 220.0, 0.0, 1.0) * {FJ_NB - 1}.0;
    vec2 abuv = fj_duv(abi, 3.0);
    vec3 abt = fj_tex(abuv);
    float aspec = 1.0 + 2.5 * fj_dec(abt.xy);
    vec3 acol = mix(alow, ahigh, smoothstep(0.1, 0.8, ahp));
    aacc += acol * acv * arays * aprof * (0.12 + 0.5 * aspec * aspec) * 0.2;
  }}
  return aacc;
}}
vec3 fj_sky(vec3 kd, vec3 ksun, float kcl) {{
  vec3 kc = fj_skybase(kd, ksun);
  float kdot = max(dot(kd, ksun), 0.0);
  kc += NOKKVI_TEXT * fj_airmass() * smoothstep(0.99985, 0.99992, kdot) * 5.0 * (1.0 - q9);
  kc += mix(NOKKVI_HIGHLIGHT, NOKKVI_TEXT, 0.5) * fj_airmass() * pow(kdot, 900.0) * 0.8 * (1.0 - q9);
  float kmoon = smoothstep(0.99982, 0.99990, kdot);
  vec2 kmq = vec2(kd.x * 900.0, kd.y * 900.0);
  vec3 kmn = fj_noised(kmq);
  kc = mix(kc, mix(NOKKVI_TEXT, NOKKVI_ACCENT, 0.15) * (0.8 + 0.3 * kmn.x), kmoon * q9);
  float kcy = max(kd.y, 0.01);
  if (q9 > 0.5) {{
    vec2 ksg = vec2(atan(kd.x, kd.z), kd.y) * 170.0;
    vec2 ksc = floor(ksg);
    float ksh = fj_hash2(ksc + vec2(301.0, 173.0));
    vec2 ksj = vec2(fj_hash2(ksc + vec2(11.0, 7.0)), fj_hash2(ksc + vec2(5.0, 19.0)));
    float ksd = length(ksg - ksc - 0.25 - 0.5 * ksj);
    float kst = step(0.985, ksh) * smoothstep(0.35, 0.0, ksd) * (0.5 + 0.5 * sin((time * 0.25) * 7.0 + ksh * 90.0));
    kc += NOKKVI_TEXT * kst * 0.7 * smoothstep(0.02, 0.2, kd.y) * (1.0 - kmoon);
    kc += fj_aurora(kd);
  }}
  if (kcl > 0.5) {{
    float kt = (22.0 - q8) / kcy;
    vec2 kq = vec2(q7 + kd.x * kt, kd.z * kt) * 0.03125 + vec2(0.0, q25);
    float kn = 0.0;
    float kna = 0.5;
    for (int k = 0; k < 4; k++) {{
      vec3 knn = fj_noised(kq);
      kn += kna * knn.x;
      kna *= 0.5;
      kq = vec2(2.0 * kq.x - kq.y, kq.x + 2.0 * kq.y) + vec2(0.13, 0.29);
    }}
    float kcov = smoothstep(0.42 + 0.12 * q9, 0.78, kn) * smoothstep(0.02, 0.25, kd.y);
    vec3 kcc = mix(NOKKVI_SURFACE * 0.8 + NOKKVI_BG * 0.4, fj_suncol() * 0.4, 0.25 + 0.5 * pow(kdot, 3.0));
    kcc = mix(kcc, mix(NOKKVI_WARM, NOKKVI_HIGHLIGHT, 0.4) * fj_airmass() * 1.1, fj_gold() * (0.3 + 0.5 * pow(kdot, 2.0)));
    kcc = mix(kcc, NOKKVI_BG * 0.3 + fj_suncol() * 0.12, q9);
    kc = mix(kc, kcc, kcov * (0.85 - 0.35 * q9));
  }}
  return kc;
}}
float fj_shadow(vec3 so, vec3 sd) {{
  float sres = 1.0;
  float st = 0.03;
  for (int k = 0; k < 40; k++) {{
    vec3 wp = so + sd * st;
    float shh = 0.0;
    if (st < 0.8) {{
      vec3 shs = fj_hs(wp);
      shh = shs.x;
    }} else {{
      shh = fj_h(wp);
    }}
    float sdh = wp.y - shh;
    sres = min(sres, 8.0 * sdh / st);
    if (sres < 0.0) break;
    if (wp.y > {FJ_HMAX}) break;
    st += clamp(sdh * 0.5, 0.025, 0.9);
  }}
  return smoothstep(0.0, 1.0, clamp(sres, 0.0, 1.0));
}}
vec3 fj_rock(vec3 rp, vec3 rn, float rpu, float rmat) {{
  vec2 rv = vec2(rpu, rp.z) * 0.25;
  vec3 rvn = fj_noised(rv);
  float rq = 1.0 - abs(1.0 - 2.0 * fract((rp.y * 0.42 + 0.3 * rvn.x + q22) * 0.5));""" + ramp("rr", "rq") + f"""
  float rl = dot(rr, vec3(0.3, 0.5, 0.2));
  vec3 rs = mix(vec3(rl), rr, 0.5);
  vec3 ra = mix(NOKKVI_SURFACE + vec3(0.05), rs, 0.6);
  ra *= 0.22 / max(dot(ra, vec3(0.3, 0.5, 0.2)), 0.02);
  float rsteep = smoothstep(0.85, 0.45, rn.y);
  float rbl = smoothstep(-0.15, 0.15, abs(rn.x) - abs(rn.z));
  vec2 rsq = vec2(rp.z * 3.0, rp.y * 0.5);
  vec3 rsn1 = fj_noised(rsq);
  vec2 rsq2 = vec2(rp.z * 9.0, rp.y * 1.5 + 0.3);
  vec3 rsn2 = fj_noised(rsq2);
  vec2 rsq3 = vec2(rpu * 3.0 + 0.5, rp.y * 0.5);
  vec3 rsn3 = fj_noised(rsq3);
  vec2 rsq4 = vec2(rpu * 9.0 + 0.5, rp.y * 1.5 + 0.3);
  vec3 rsn4 = fj_noised(rsq4);
  float rstreak = mix(rsn3.x * 0.6 + rsn4.x * 0.4, rsn1.x * 0.6 + rsn2.x * 0.4, rbl);
  ra *= mix(1.0, 0.82 + 0.36 * rstreak, rsteep);
  ra *= 0.8 + 0.4 * rmat;
  float rflat = smoothstep(0.72, 0.9, rn.y) * smoothstep(0.05, 0.3, rp.y);
  vec3 rveg = mix(NOKKVI_ACCENT, NOKKVI_SURFACE, 0.55);
  rveg *= 0.13 / max(dot(rveg, vec3(0.3, 0.5, 0.2)), 0.02);
  ra = mix(ra, rveg, rflat * 0.55);
  float rsn = smoothstep(4.3, 5.1, rp.y + 0.35 * sin(rp.x * 2.1 + rp.z * 1.693504)) * smoothstep(0.72, 0.88, rn.y);
  ra = mix(ra, NOKKVI_TEXT * 0.75, rsn);
  ra *= mix(0.4, 1.0, smoothstep(0.0, 0.15, rp.y));
  return ra;
}}
vec3 fj_lit(vec3 lp, vec3 ln, float lpu, float lmat, vec3 lsun, float lsh, float lao, vec3 lrd) {{
  vec3 la = fj_rock(lp, ln, lpu, lmat);
  float ldif = max(dot(ln, lsun), 0.0);
  vec3 lsky = fj_ambient();
  vec3 lbd = normalize(vec3(-lsun.x, 0.0, -lsun.z));
  float lbn = max(dot(ln, lbd), 0.0);
  vec3 lsc = fj_suncol();
  vec3 lcol = la * (lsc * ldif * lsh + lsky * (0.55 + 0.45 * ln.y) * lao + NOKKVI_WARM * lbn * 0.15 * lao);
  float lwet = 1.0 - smoothstep(0.02, 0.16, lp.y);
  vec3 lhv = normalize(lsun - lrd);
  float lspc = pow(max(dot(ln, lhv), 0.0), 60.0) * lsh * lwet * 0.6;
  return lcol + lsc * lspc;
}}
vec3 fj_fog(vec3 fc, vec3 fd, float ft, float fy, vec3 fsun) {{
  vec3 fdh = normalize(vec3(fd.x, 0.03, fd.z));
  vec3 fh = fj_skybase(fdh, fsun);
  float fdot = max(dot(fd, fsun), 0.0);
  fh += mix(NOKKVI_HIGHLIGHT, NOKKVI_WARM, 0.5 + 0.3 * fj_gold()) * fj_airmass() * pow(fdot, 5.0) * (0.35 + 0.4 * fj_gold()) * (1.0 - q9);
  float fyav = max(0.5 * (q8 + fy), 0.0);
  float fa = max(1.0 - exp(-ft * 0.03 * exp(-fyav * 0.22)), smoothstep({FJ_ZL - FJ_ZB - 32.0}, {FJ_ZL - FJ_ZB - 3.0}, ft));
  float fmt = min(ft, 40.0);
  vec2 fmq = vec2(q7 + fd.x * fmt, q4 + fd.z * fmt) * 0.25;
  vec3 fmn = fj_noised(fmq);
  float fm = exp(-max(fy, 0.0) * 2.2) * (1.0 - exp(-ft * 0.06)) * (0.5 - 0.15 * fj_gold()) * (0.45 + 1.1 * fmn.x);
  return mix(fc, fh, clamp(fa + fm, 0.0, 1.0));
}}
vec2 fj_ripple(vec2 wq) {{
  vec2 wq1 = wq * 1.75 + vec2(0.0, q19 * 0.7);
  vec2 wq2 = vec2(4.0 * wq.x - wq.y, wq.x + 4.0 * wq.y) + vec2(q19 * 1.1, -q19 * 0.4);
  vec3 wn1 = fj_noised(wq1);
  vec3 wn2 = fj_noised(wq2);
  vec2 wg2 = vec2(4.0 * wn2.y + wn2.z, -wn2.y + 4.0 * wn2.z);
  return wn1.yz * 1.75 * 0.6 + wg2 * 0.12;
}}
"""

FJ_COMP = FJ_COMP_FUNCS + " shader_body {\n" + HEAD + f"""
  vec2 p = (uv - 0.5) * s;
  vec3 ro = vec3(q7, q8, q4);
  vec3 fw = normalize(vec3(q10, q11, q12));
  vec3 upr = vec3(sin(q13), cos(q13), 0.0);
  vec3 rt = normalize(cross(upr, fw));
  vec3 up = cross(fw, rt);
  vec3 rd = normalize(fw + (p.x * rt + p.y * up) * q14);
  vec3 sun = fj_sun();
  float tw = rd.y < -0.0001 ? -ro.y / rd.y : 1e9;
  float tmax = min(tw, 90.0);
  float t = 0.03;
  float tp = t;
  float hit = 0.0;
  float mdh = 1.0;
  for (int i = 0; i < 220; i++) {{
    vec3 mp = ro + rd * t;
    float mhn = 0.0;
    if (t < 6.0) {{
      vec3 mhs = fj_hs(mp);
      mhn = mhs.x;
    }} else {{
      mhn = fj_h(mp);
    }}
    mdh = mp.y - mhn;
    if (mdh < 0.0) {{
      if (t < 6.0) {{ hit = 1.0; break; }}
      vec3 mhv = fj_hs(mp);
      if (mp.y - mhv.x < 0.0) {{ hit = 1.0; break; }}
    }}
    tp = t;
    t += clamp(mdh * 0.3, 0.004 + 0.004 * t, 0.25 + 0.03 * t);
    if (t > tmax) break;
    if (mp.z - q4 > {FJ_ZL - FJ_ZB - 2.0}) break;
  }}
  vec3 tend = ro + rd * t;
  hit = max(hit, step(t, tmax) * step(mdh, 0.05 + 0.01 * t) * step(tend.z - q4, {FJ_ZL - FJ_ZB - 2.0}));
  if (hit > 0.5) {{
    float ta = tp;
    float tb = t;
    for (int k = 0; k < 10; k++) {{
      vec3 bap = ro + rd * ta;
      vec3 bah = fj_hs(bap);
      if (bap.y - bah.x > 0.0) break;
      tb = ta;
      ta = max(ta - (0.08 + 0.02 * ta), 0.03);
    }}
    for (int k = 0; k < 8; k++) {{
      float tm = 0.5 * (ta + tb);
      vec3 bp = ro + rd * tm;
      vec3 bhs = fj_hs(bp);
      float bdh = bp.y - bhs.x;
      tb = bdh < 0.0 ? tm : tb;
      ta = bdh < 0.0 ? ta : tm;
    }}
    t = 0.5 * (ta + tb);
  }}
  vec3 col = fj_sky(rd, sun, 1.0);
  float dwv = tw;
  vec3 wq0 = ro + rd * tw;
  vec3 wqs = fj_hs(wq0);
  float shore = (1.0 - hit) * step(tw, 1e8) * step(0.0, wqs.x);
  t = shore > 0.5 ? tw : t;
  hit = max(hit, shore);
  if (hit > 0.5) {{
    vec3 pos = ro + rd * t;
    float e = 0.025 + 0.004 * t;
    float pmat;
    vec3 n = fj_nrm(pos, e, pmat);
    float pu = pos.x - fj_path(pos.z);
    vec3 nb = n;
    vec3 tpw = pow(abs(n), vec3(4.0));
    tpw /= tpw.x + tpw.y + tpw.z;
    vec2 dqh = vec2(pu, pos.z);
    vec2 dgh = fj_detail(dqh);
    vec2 dqx = vec2(pos.z, pos.y);
    vec2 dgx = fj_detail(dqx);
    vec2 dqz = vec2(pos.x + 17.0, pos.y);
    vec2 dgz = fj_detail(dqz);
    vec3 dbump = vec3(-dgh.x, 0.0, -dgh.y) * tpw.y + vec3(0.0, -dgx.y, -dgx.x) * tpw.x + vec3(-dgz.x, -dgz.y, 0.0) * tpw.z;
    float damp = (0.03 + 0.07 * smoothstep(0.95, 0.5, n.y)) * exp(-t * 0.07);
    n = normalize(n + dbump * damp);
    float aor = 0.0;
    for (int k = 1; k <= 4; k++) {{
      float aod = 0.12 * float(k * k);
      vec3 aoq = pos + nb * aod;
      vec3 aos = fj_hs(aoq);
      aor += max(aos.x - aoq.y, 0.0) / aod * (1.0 / float(k));
    }}
    float ao = clamp(1.0 - aor * 0.45, 0.3, 1.0);
    vec3 spos = pos + nb * 0.03;
    float sh = fj_shadow(spos, sun);
    col = fj_lit(pos, n, pu, pmat, sun, sh, ao, rd);
    col = fj_fog(col, rd, t, pos.y, sun);
  }} else if (tw < 1e8) {{
    vec3 wp = ro + rd * tw;
    float wfade = exp(-tw * 0.05);
    vec2 wg0 = fj_ripple(wp.xz);
    vec2 wf2 = normalize(rd.xz + vec2(0.00001, 0.0));
    vec2 wl2 = vec2(-wf2.y, wf2.x);
    vec2 wg = wf2 * dot(wg0, wf2) + wl2 * dot(wg0, wl2) * 0.35;
    float wa = (0.012 + 0.03 * wfade) / (1.0 + 0.15 * tw);
    vec2 wkg = vec2(0.0);
    float wkamp = 0.01 * smoothstep(45.0, 12.0, tw);
    float pwu = wp.x - fj_path(wp.z);
    vec2 pdir = vec2(0.0, 1.0);
    for (int k = 0; k < 5; k++) {{
      float pag;
      float pst;
      vec2 ptr = fj_ptrack(k, pag, pst);
      float pen = fj_penv(pag) * pst;
      float ptrav = {FJ_PV} * pag;
      vec2 prel = vec2(pwu - ptr.x, mod(wp.z - ptr.y + 128.0, 256.0) - 128.0);
      vec2 prx = prel + vec2(0.02, 0.0);
      vec2 prz = prel + vec2(0.0, 0.02);
      float ph0 = fj_wakeh(prel, pdir, ptrav, pag);
      float phx = fj_wakeh(prx, pdir, ptrav, pag);
      float phz = fj_wakeh(prz, pdir, ptrav, pag);
      wkg += vec2(phx - ph0, phz - ph0) / 0.02 * pen;
    }}
    vec2 wks = wkg * wkamp;
    wks *= min(1.0, 0.22 / max(length(wks), 0.0001));
    vec3 wn = normalize(vec3(-wg.x * wa - wks.x, 1.0, -wg.y * wa - wks.y));
    vec3 rfd = reflect(rd, wn);
    rfd.y = abs(rfd.y);
    float rt2 = 0.03;
    float rtp = rt2;
    float rhit = 0.0;
    float rdh = 1.0;
    for (int i = 0; i < 90; i++) {{
      vec3 rp = wp + rfd * rt2;
      rdh = rp.y - fj_h(rp);
      if (rdh < 0.0) {{ rhit = 1.0; break; }}
      rtp = rt2;
      rt2 += clamp(rdh * 0.35, 0.01 + 0.006 * rt2, 0.4 + 0.03 * rt2);
      if (rt2 > 70.0) break;
      if (rp.y > {FJ_HMAX}) break;
    }}
    vec3 refl = fj_sky(rfd, sun, 1.0);
    if (rhit > 0.5) {{
      float ra2 = rtp;
      float rb2 = rt2;
      for (int k = 0; k < 6; k++) {{
        float rm2 = 0.5 * (ra2 + rb2);
        vec3 rbp = wp + rfd * rm2;
        float rbd = rbp.y - fj_h(rbp);
        rb2 = rbd < 0.0 ? rm2 : rb2;
        ra2 = rbd < 0.0 ? ra2 : rm2;
      }}
      rt2 = 0.5 * (ra2 + rb2);
      vec3 rpos = wp + rfd * rt2;
      float rmat;
      vec3 rn = fj_nrm(rpos, 0.05, rmat);
      float rpu = rpos.x - fj_path(rpos.z);
      vec3 rsp = rpos + rn * 0.03;
      float rsh = fj_shadow(rsp, sun);
      refl = fj_lit(rpos, rn, rpu, rmat, sun, rsh, 0.8, rfd);
      refl = fj_fog(refl, rfd, rt2 + tw, rpos.y, sun);
    }}
    float wc = max(dot(-rd, wn), 0.0);
    float fre = 0.02 + 0.98 * pow(1.0 - wc, 5.0);
    float rvd = rhit > 0.5 ? rt2 : 1e4;
    dwv = 1.0 / mix(1.0 / tw, 1.0 / (tw + rvd), fre);
    float bed = fj_h(wp);
    float wdep = max(-bed, 0.0);
    vec3 deep = NOKKVI_BG * 0.3 + NOKKVI_ACCENT * 0.07;
    vec3 wsh3 = wp + vec3(0.0, 0.01, 0.0);
    float wsh = fj_shadow(wsh3, sun);
    vec3 wup = vec3(0.0, 1.0, 0.0);
    float wpu = wp.x - fj_path(wp.z);
    vec3 shallow = fj_rock(wp, wup, wpu, 0.5) * (fj_suncol() * wsh * 0.35 + fj_ambient() * 0.6);
    vec3 under = mix(shallow, deep, 1.0 - exp(-wdep * 5.0));
    col = mix(under, refl, fre);
    float wdp = max(-wqs.x, 0.0);
    vec2 wfq = vec2(wp.x * 4.0, wp.z * 4.0 + q19 * 0.4);
    vec3 wfn = fj_noised(wfq);
    float foam = smoothstep(0.04, 0.0, wdp) * (0.35 + 0.65 * wfn.x) * exp(-tw * 0.04);
    col = mix(col, fj_suncol() * wsh * 0.16 + fj_ambient() * 0.3, foam * 0.6);
    vec3 wrf = refract(rd, wn, 0.75);
    float wdd = 0.3 / max(-wrf.y, 0.08);
    vec2 wpd = wp.xz + wrf.xz * wdd;
    vec2 gspg = wpd * 13.0;
    vec2 gspc = mod(floor(gspg), vec2(384.0, 3328.0));
    vec2 gspc2 = gspc + vec2(19.0, 7.0);
    vec2 gspc3 = gspc + vec2(3.0, 41.0);
    vec2 gspo = vec2(fj_hash2(gspc2), fj_hash2(gspc3)) * 0.6 + 0.2;
    float gspd = length(fract(gspg) - gspo);
    float gsph = fj_hash2(gspc);
    float gspdot = smoothstep(0.08 + 0.12 * gsph, 0.0, gspd) * step(0.7, gsph) * (1.0 + 3.0 * (gsph - 0.7));
    float gspk = mix(0.5 + gspdot * 3.0, 1.0, smoothstep(3.0, 10.0, tw));
    float plit = 0.0;
    float lwu = wpd.x - fj_path(wpd.y);
    for (int k = 0; k < 5; k++) {{
      float lag;
      float lst;
      vec2 ltr = fj_ptrack(k, lag, lst);
      float len2 = fj_penv(lag) * lst;
      float ltrav = {FJ_PV} * lag;
      vec2 lts = vec2(lwu - ltr.x, mod(wpd.y - ltr.y + 128.0, 256.0) - 128.0);
      vec2 ldv = lts - vec2(0.0, ltrav);
      float ld2 = dot(ldv, ldv);
      float lglo = exp(-ld2 * 3.0) * 1.2 + 0.12 / (1.0 + ld2 * 3.0);
      float lsg = clamp(lts.y, 0.0, ltrav);
      vec2 lto = lts - vec2(0.0, lsg);
      float ltrl = exp(-dot(lto, lto) * 6.0) * exp(-(ltrav - lsg) * 0.4) * 0.55;
      plit += len2 * (lglo + ltrl * gspk);
    }}
    float wkcr = min(length(wks) * 5.0, 1.0);
    col += fj_glowcol() * plit * (0.3 + 0.7 * (1.0 - fre) + 0.8 * wkcr);
    float gl = pow(max(dot(rfd, sun), 0.0), 350.0) * (1.5 + 2.0 * q21);
    col += fj_suncol() * gl * wsh * (0.92 + 0.16 * q20);
    col = fj_fog(col, rd, tw, 0.0, sun);
  }}
  col *= 1.05 + 0.45 * q9;
  col = clamp((col * (2.51 * col + 0.03)) / (col * (2.43 * col + 0.59) + 0.14), 0.0, 1.0);
  float dd = mix(mix(1e4, dwv, step(tw, 1e8)), t, step(0.5, hit));
  vec3 dpos = ro + rd * dd;
  vec3 cro = vec3(fj_cam(1.0) * 16.0 - 8.0, fj_cam(2.0) * 8.0 - 1.0, fj_cam(0.0) * 256.0);
  cro.z += 256.0 * floor((q4 - cro.z) / 256.0 + 0.5);
  vec3 cfw0 = vec3(fj_cam(3.0), fj_cam(4.0), fj_cam(5.0)) * 2.0 - 1.0;
  vec3 cfw = cfw0 / max(length(cfw0), 0.0001);
  float crl = fj_cam(6.0) * 4.0 - 2.0;
  float cln = max(fj_cam(7.0) * 4.0, 0.05);
  vec3 cupr = vec3(sin(crl), cos(crl), 0.0);
  vec3 crx = cross(cupr, cfw);
  vec3 crt = crx / max(length(crx), 0.0001);
  vec3 cup = cross(cfw, crt);
  vec3 cv = dpos - cro;
  float cvz = dot(cv, cfw);
  vec2 hp = vec2(dot(cv, crt), dot(cv, cup)) / (max(cvz, 0.001) * cln);
  vec2 huv = hp / s + 0.5;
  float hok = step(0.001, cvz) * step(abs(huv.x - 0.5), 0.5) * step(abs(huv.y - 0.5), 0.5) * step(3.5, frame);
  float fdist = max(q3, 0.5);
  float cocd = dd > fdist ? q24 * (1.0 - fdist / dd) : {FJ_NEARCOC} * q24 * (fdist / dd - 1.0);
  cocd *= mix(1.0, {FJ_SKYCOC}, smoothstep(60.0, 300.0, dd));
  float cpx = min({FJ_HALOR} + cocd, 0.05) * min(texsize.x, texsize.y);
  float hlod = clamp(log2(max(2.0 * cpx, 1.0)), 0.0, 6.0);
  vec2 hoff = exp2(hlod) * 0.5 * texsize.zw;
  float hlod1 = max(hlod - 1.0, 0.0);
  vec2 hoff2 = vec2(hoff.x, -hoff.y);
  vec2 huv1 = huv + hoff;
  vec2 huv2 = huv - hoff;
  vec2 huv3 = huv + hoff2;
  vec2 huv4 = huv - hoff2;
  vec4 hs = textureLod(sampler2D(sampler_prev_comp, sampler_prev_comp_samp), huv, 0.0);
  vec4 hb = textureLod(sampler2D(sampler_prev_comp, sampler_prev_comp_samp), huv, hlod1) * 0.4;
  hb += textureLod(sampler2D(sampler_prev_comp, sampler_prev_comp_samp), huv1, hlod1) * 0.15;
  hb += textureLod(sampler2D(sampler_prev_comp, sampler_prev_comp_samp), huv2, hlod1) * 0.15;
  hb += textureLod(sampler2D(sampler_prev_comp, sampler_prev_comp_samp), huv3, hlod1) * 0.15;
  hb += textureLod(sampler2D(sampler_prev_comp, sampler_prev_comp_samp), huv4, hlod1) * 0.15;
  float hvig = 0.8 + 0.2 * smoothstep(1.45, 0.35, length(hp));
  float hwb = {FJ_HALO} + ({FJ_DOFW} - {FJ_HALO}) * smoothstep(0.0, {FJ_COCFULL}, cocd);
  float hval = hok * hs.a;
  col = mix(col, hs.rgb / hvig, q17 * (1.0 - step(0.5, hit)) * hval);
  col = mix(col, hb.rgb / hvig, hwb * hval);
  vec2 dthq = floor(uv * texsize.xy) + vec2(mod(frame, 64.0) * 7.0, mod(frame, 61.0) * 3.0);
  col += (fj_hash2(dthq) - 0.5) / 255.0;
  col *= 0.8 + 0.2 * smoothstep(1.45, 0.35, length(p));
  ret = col;
 }}"""

FJ_ACTS = [
    (0.14, 4.0, 0.05, 0.10, 1.00),
    (0.55, 5.0, 0.30, 0.35, 0.95),
    (1.60, 7.0, 0.70, 0.50, 0.90),
    (3.00, 9.0, 1.40, 0.30, 0.85),
]
def fj_act_pick(var, col):
    e = f"{FJ_ACTS[-1][col]}"
    for i in range(len(FJ_ACTS) - 2, -1, -1):
        e = f"if(equal(act, {i}), {FJ_ACTS[i][col]}, {e})"
    return f"{var} = {e};\n"
def fj_path_eel(z, out):
    return f"{out} = {FJ_A1} * sin({FJ_W1:.9f} * ({z})) + {FJ_A2} * sin({FJ_W2:.9f} * ({z}) + 1.7);\n"

# Light pulses: a kick (bass over its follower, 0.5 s cooldown) launches a
# light under the water 2.5 units ahead of the camera that races down the
# fjord at FJ_PV units/s along a lane beside the centre line (left and right
# alternate; the lane follows the river path, so the light takes every bend)
# and fades by FJ_PLIFE. Five slots, newest first, each packed as
# side * (origin z + 1) and whole milliseconds of age (capped at 60 s) plus
# strength * 0.3 as the fraction (freeing q's for the dream pass); the cooldown
# times the slot count equals the lifetime, so a slot is recycled only once its
# pulse has faded.
FJ_PULSE_EEL = ("kk = bass - bass_att; ksince = ksince + dt;\n"
    "ktr = above(kk, 0.26) * above(ksince, 0.5);\n"
    "p5o = if(ktr, p4o, p5o); p5a = if(ktr, p4a, p5a); p5s = if(ktr, p4s, p5s);\n"
    "p4o = if(ktr, p3o, p4o); p4a = if(ktr, p3a, p4a); p4s = if(ktr, p3s, p4s);\n"
    "p3o = if(ktr, p2o, p3o); p3a = if(ktr, p2a, p3a); p3s = if(ktr, p2s, p3s);\n"
    "p2o = if(ktr, p1o, p2o); p2a = if(ktr, p1a, p2a); p2s = if(ktr, p1s, p2s);\n"
    "kside = if(ktr, -kside, kside);\n"
    "kz = zc + 2.5; kz = kz - 256 * int(kz / 256);\n"
    "p1o = if(ktr, kside * (kz + 1), p1o); p1a = if(ktr, 0, p1a); p1s = if(ktr, min(0.6 + kk * 1.2, 1.6), p1s);\n"
    "ksince = if(ktr, 0, ksince);\n"
    "p1a = p1a + dt; p2a = p2a + dt; p3a = p3a + dt; p4a = p4a + dt; p5a = p5a + dt;\n"
    "q1 = p1o; q2 = int(min(p1a, 60) * 1000) + p1s * 0.3;\n"
    "q5 = p2o; q6 = int(min(p2a, 60) * 1000) + p2s * 0.3;\n"
    "q18 = p3o; q23 = int(min(p3a, 60) * 1000) + p3s * 0.3;\n"
    "q26 = p4o; q28 = int(min(p4a, 60) * 1000) + p4s * 0.3;\n"
    "q30 = p5o; q31 = int(min(p5a, 60) * 1000) + p5s * 0.3;\n")

FJ_INIT = ("pulse = 0; pop = 0; dist = 0; " + SPEED_INIT +
           " act = int(rand(4)); acttm = 12; alt = 0.5; alt_m = 0.5; lk = 5; lk_m = 5; lkh = 0.3; lkh_m = 0.3;"
           " lat = 0.1; lat_m = 0.1; fov = 1; fov_m = 1; bank = 0; bank_m = 0; lsm = 1; flow = 0; hue = 0; vis = -1;"
           " aur = 1; fsp = 1; fsp_m = 1; gside = 1; saz = 0.8;"
           " ksince = 1; kside = 1; p1o = 1; p2o = 1; p3o = 1; p4o = 1; p5o = 1;"
           " p1a = 9; p2a = 9; p3a = 9; p4a = 9; p5a = 9; p1s = 0; p2s = 0; p3s = 0; p4s = 0; p5s = 0;")
FJ_FRAME = (PULSE + "dt = min(1 / max(fps, 1), 0.1);\n" + SPEED
    + ease("fsp", "spd + 0.55 * sg", "0.12")
    + "dstep = min(fsp * 1.1 * dt, 0.4);\n"
    "dist = dist + dstep;\n"
    "zc = dist - 256 * int(dist / 256); q4 = zc;\n"
    "cld = dist * 0.03125 + time * 0.02; q25 = cld - 64 * int(cld / 64);\n"
    "acttm = acttm - dt; chg = below(acttm, 0);\n"
    "act = if(chg, (act + 1 + int(rand(3))) % 4, act);\n"
    "acttm = if(chg, 14 + rand(1000) / 1000 * 8, acttm);\n"
    + fj_act_pick("altg", 0) + fj_act_pick("lkg", 1) + fj_act_pick("lkhg", 2) + fj_act_pick("latg", 3)
    + fj_act_pick("fovg", 4)
    + ease("alt", "altg", "3.0") + ease("lk", "lkg", "3.0") + ease("lkh", "lkhg", "3.0")
    + ease("lat", "latg", "3.0") + ease("fov", "fovg", "3.0")
    + fj_path_eel("zc", "cxp")
    + "q7 = cxp + lat * sin(time * 0.13); q8 = alt + 0.03 * sin(time * 0.31);\n"
    + fj_path_eel("zc + lk", "cxt")
    + "fx = cxt - q7; fy = lkh - q8; fz = lk;\n"
      "fl = sqrt(fx * fx + fy * fy + fz * fz); q10 = fx / fl; q11 = fy / fl; q12 = fz / fl;\n"
    "bz = zc + 0.5 * lk + 1.1 * spd * 3.0;\n"
    f"curv = -({FJ_A1 * FJ_W1 ** 2:.9f} * sin({FJ_W1:.9f} * bz) + {FJ_A2 * FJ_W2 ** 2:.9f} * sin({FJ_W2:.9f} * bz + 1.7));\n"
    + ease("bank", "curv * 5", "1.5")
    + "q13 = bank;\n"
      "q14 = fov * (1 + 0.04 * min(fsp - spd, 2));\n"
      "vpick = below(vis, 0) * above(frame, 2);\n"
      "vh = int(bass * 977 + mid * 631 + treb * 401 + rand(1000));\n"
      "vis = if(vpick, if(below(vh % 20, 6), 0, if(below(vh % 20, 13), 1, 2)), vis);\n"
      "vv = max(vis, 0); isgold = equal(vv, 1); q9 = equal(vv, 2);\n"
      "gside = if(vpick, if(below(vh % 2, 1), -1, 1), gside);\n"
      "saz = if(vpick, gside * (0.45 + (vh % 7) / 10), saz);\n"
      "q15 = if(isgold, gside * 0.6, if(q9, gside * 0.8, saz));\n"
      "q16 = if(isgold, 0.09, if(q9, 0.42, 0.38));\n"
      "hue = if(vpick, (vh % 97) / 97, hue); q22 = hue;\n"
      "aur = aur + (min(mid_att, 2) - aur) * (1 - exp(-dt / 1.5)); q27 = aur * q9;\n"
      "loud = min((bass_att + mid_att + treb_att) / 3, 2);\n"
      "lsm = lsm + (loud - lsm) * (1 - exp(-dt / 0.6)); q20 = lsm;\n"
      "flow = flow + dt * (0.3 + 0.4 * fsp); flow = flow - 640 * int(flow / 640); q19 = flow;\n"
      "q21 = min(max(treb - treb_att, 0), 1);\n"
      f"q3 = {FJ_FOCUS}; q17 = {FJ_ECHO}; q24 = {FJ_APER};\n"
    + FJ_PULSE_EEL)

presets["nokkvi - fjord"] = preset({"decay": 0.0, "wave_a": 0.0, "zoom": 1.0}, FJ_WARP, FJ_COMP,
                                  init=FJ_INIT, frame=FJ_FRAME)

presets["nokkvi - black holes"] = black_holes_port(False)
presets["nokkvi - cover black holes"] = black_holes_port(True)
presets["nokkvi - maxawow"] = maxawow_port(False)
presets["nokkvi - cover maxawow"] = maxawow_port(True)

for name, p in presets.items():
    json.dump(p, open(os.path.join(OUT, name + ".json"), "w"), indent=1)
print(len(presets))
