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

# Starfield: 3D star layers flown through with parallax, drawn into the
# feedback so every star leaves a motion trail that zooms outward (longer at
# speed); each star is tied to a frequency and flares with it; kicks surge
# the speed. (A screen-space nebula read as a smudge on the lens; removed.)
STAR_LAYERS = 6
def star_layer(l, twist="q11 * (1.0 - z) * 2.2", girth=""):
    return f"""
  {{
    float z = fract({l}.0 / {STAR_LAYERS}.0 + q1);
    float scale = mix(26.0, 0.6, z);
    float fade = smoothstep(0.0, 0.25, z) * smoothstep(1.0, 0.85, z);
    float tw = {twist};
    vec2 prt = vec2(pr.x * cos(tw) - pr.y * sin(tw), pr.x * sin(tw) + pr.y * cos(tw));
    vec2 g = prt * scale + vec2({l * 37.1:.1f}, {l * 91.7:.1f});
    vec2 id = floor(g);
    vec2 f = fract(g) - 0.5;
    float h = fract(sin(dot(id, vec2(127.1, 311.7))) * 43758.5453);
    float h2 = fract(h * 91.3);
    vec2 d = f - (vec2(h, h2) - 0.5) * 0.5;
    float size = mix(0.012, 0.04, h2) * (0.6 + z){girth};
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


# Starfield nebula: the starfield flown through a living nebula, the reaction
# from "The NG + Flexi + BDRV - Ultramix, Aderrasi + Flexi - Predator Prey
# Spirals" (three species in the feedback's RGB, each chasing the next,
# advected along its own blurred gradient; its per-pixel zoom/rotation
# equations kept, its moving seed disc and border drawn in the warp). The
# star trails live in the feedback's ALPHA (the engine's `ret_alpha`), so the
# two layers share one buffer and touch each other:
# - a trail feeds the predator of the species it crosses (wakes that grow
#   into new fronts), and the reaction's gradient bends the trails;
# - the gas's rotation field is the stars' too: trails curl by it per radius,
#   star positions turn by its running sum (tracked at six radii, leaking over
#   ~2 s so the grid never winds up), and the flight speed spins both;
# - the comp shades the reaction on the theme gradient (species balance picks
#   the stop, relief shades up from the background, fronts catch a
#   highlight), embosses the trails like the gas, tints their tails with the
#   gas they cross, and a drifting gas-density field trades the two layers'
#   opacity by region: thick gas hides the stars, thin gas opens onto space.
# Soft gas clouds (big dim blobs on the same flight) ride in the comp.
# Camera acts: on every beat the whole scene stops, the view is dragged to a
# new camera (the picture's plane tilted in perspective, rolled and zoomed,
# smeared by the COMP's echo of its last frame), and it starts again: it
# flows between beats and snaps to a new perspective on them.
# While stopped, the warp copies the feedback through unchanged and the
# equations hold the flight, roll and swirl sums.
NEB_RADII = [0.0, 0.25, 0.5, 0.75, 1.0, 1.25]
NEB_GASROT = "0.08 * abs(0.746 - {r}) * sin(2.2 * (0.5 - {r}) + 5.7 * sin(0.1 * time))"
def neb_swirl(v, pos):
    """GLSL float `v`: the stars' running swirl angle at texture point `pos`,
    lerped between the radii the equations track (q25..q30). The radius is the
    engine's per-vertex `rad` (x spans -1..1, y aspect-scaled)."""
    out = (f"\n  float {v}_f = clamp(2.0 * length(({pos} - 0.5) * s) / max(s.x, s.y) / 0.25, 0.0, 5.0);\n"
           f"  float {v} = mix(q25, q26, clamp({v}_f, 0.0, 1.0));\n")
    for i in range(2, 6):
        out += f"  {v} = mix({v}, q{25 + i}, clamp({v}_f - {i - 1}.0, 0.0, 1.0));\n"
    return out
# Camera acts (see above). Each beat (bass over its follower, 0.22 s apart)
# starts one unless the last is still running; quiet music gets one after
# 4 s without. An act (`inact`, clock `aa`) stops the scene (`frz` eases to 1
# in ~0.05 s), drags the camera from its old pose (f*) to a new one (t*) over
# ~45% of the smoothed beat interval (`ibi`, 0.12-0.4 s), and lets go, so the
# picture flows between beats and snaps to a new perspective on them. A
# harder kick shifts it further; each pose pulls halfway back to the centre
# so the tilt stays bounded, and every 16th act goes back to flat. The new
# pose mixes the rand stream with the audio, so visits differ; its zoom
# covers about half the tilt and roll (past that the picture folds back as a
# mirror at the edges, which keeps it sharp where a full cover would blur). Each act also twists (`ttw`, random
# direction, bigger on harder kicks). q6 = stop, q7..q10 = pitch, yaw, roll,
# zoom, q11 = drag speed (the COMP's echo), q18 = focal length (a wide lens
# exaggerates the tilt), q12..q15 = this frame's step of
# yaw, pitch, roll, zoom and q16 of the twist (the warp melts the held
# picture by them), q17 = the on-screen twist (in and back out over the drag).
NEB_CAMERA = """dt = 1 / max(fps, 1);
at = at + dt * (1 - inact);
sinceb = sinceb + dt;
beat = above(bass - bass_att, 0.18) * above(sinceb, 0.22);
ibi = if(beat, ibi * 0.8 + min(sinceb, 1.5) * 0.2, ibi);
sinceb = if(beat, 0, sinceb);
go = below(inact, 0.5) * max(beat, above(at, 4));
inact = max(inact, go);
aa = if(go, 0, aa + dt * inact);
nacts = nacts + go;
u1 = rand(1000) / 1000 + bass_att * 3.7; u1 = u1 - int(u1);
u2 = rand(1000) / 1000 + mid_att * 5.3; u2 = u2 - int(u2);
u3 = rand(1000) / 1000 + treb_att * 7.1; u3 = u3 - int(u3);
flat = equal(nacts % 16, 0);
amp = min(0.3 + max(bass - bass_att, 0) * 0.6, 0.7);
fpit = if(go, cpit, fpit); fyaw = if(go, cyaw, fyaw); frol = if(go, crol, frol); fzm = if(go, czm, fzm); ffoc = if(go, cfoc, ffoc);
tpit = if(go, min(max(cpit * 0.35 + (u1 - 0.5) * 2 * amp, -0.75), 0.75) * (1 - flat), tpit);
tyaw = if(go, min(max(cyaw * 0.35 + (u2 - 0.5) * 2 * amp, -0.65), 0.65) * (1 - flat), tyaw);
trol = if(go, min(max(crol * 0.35 + (u3 - 0.5) * 2 * amp, -0.6), 0.6), trol);
tfoc = if(go, 0.8 + 0.9 * u3 + 0.8 * flat, tfoc);
tzm = if(go, 1.04 + 0.5 * max(abs(tpit), abs(tyaw)) * 1.6 / tfoc + 0.25 * abs(trol) + 0.08 * u1, tzm);
ddur = if(go, min(max(ibi * 0.45, 0.12), 0.4), ddur);
eprev = if(go, 0, eprev);
e = min(max((aa - 0.05) / ddur, 0), 1);
e = e * e * (3 - 2 * e);
cpit = if(inact, fpit + (tpit - fpit) * e, cpit);
cyaw = if(inact, fyaw + (tyaw - fyaw) * e, cyaw);
crol = if(inact, frol + (trol - frol) * e, crol);
czm = if(inact, fzm + (tzm - fzm) * e, czm);
cfoc = if(inact, ffoc + (tfoc - ffoc) * e, cfoc);
cmv = abs(e - eprev) / dt * (abs(tpit - fpit) + abs(tyaw - fyaw) + abs(trol - frol) + abs(tzm - fzm) * 0.6);
dee = e - eprev;
eprev = e;
ttw = if(go, (above(u2, 0.5) * 2 - 1) * (0.5 + 2.2 * amp), ttw);
frzt = inact * below(aa, 0.07 + ddur);
done = inact * above(aa, 0.07 + ddur);
inact = inact * (1 - done);
at = if(done, 0, at);
frz = frz + (frzt - frz) * (1 - exp(-dt / 0.025));
q6 = frz; q7 = cpit; q8 = cyaw; q9 = crol; q10 = czm; q11 = min(cmv * 0.35, 0.8);
q12 = dee * (tyaw - fyaw); q13 = dee * (tpit - fpit); q14 = dee * (trol - frol); q15 = dee * (tzm - fzm);
q16 = dee * ttw * 0.5; q17 = sin(3.14159 * e) * ttw * inact; q18 = cfoc;
"""
NEB_LIQUID_TAPS = """  { vec2 lo = vec2(3.000, 0.000);
    float lw = texture(sampler_main, wuv + lo * texsize.zw).w;
    lt += lw * 0.0667; lg += lo * (lw * 0.0222); }
  { vec2 lo = vec2(4.243, 4.243);
    float lw = texture(sampler_main, wuv + lo * texsize.zw).w;
    lt += lw * 0.0667; lg += lo * (lw * 0.0111); }
  { vec2 lo = vec2(1.500, 2.598);
    float lw = texture(sampler_main, wuv + lo * texsize.zw).w;
    lt += lw * 0.0667; lg += lo * (lw * 0.0222); }
  { vec2 lo = vec2(-1.553, 5.796);
    float lw = texture(sampler_main, wuv + lo * texsize.zw).w;
    lt += lw * 0.0667; lg += lo * (lw * 0.0111); }
  { vec2 lo = vec2(-1.500, 2.598);
    float lw = texture(sampler_main, wuv + lo * texsize.zw).w;
    lt += lw * 0.0667; lg += lo * (lw * 0.0222); }
  { vec2 lo = vec2(-5.796, 1.553);
    float lw = texture(sampler_main, wuv + lo * texsize.zw).w;
    lt += lw * 0.0667; lg += lo * (lw * 0.0111); }
  { vec2 lo = vec2(-3.000, 0.000);
    float lw = texture(sampler_main, wuv + lo * texsize.zw).w;
    lt += lw * 0.0667; lg += lo * (lw * 0.0222); }
  { vec2 lo = vec2(-4.243, -4.243);
    float lw = texture(sampler_main, wuv + lo * texsize.zw).w;
    lt += lw * 0.0667; lg += lo * (lw * 0.0111); }
  { vec2 lo = vec2(-1.500, -2.598);
    float lw = texture(sampler_main, wuv + lo * texsize.zw).w;
    lt += lw * 0.0667; lg += lo * (lw * 0.0222); }
  { vec2 lo = vec2(1.553, -5.796);
    float lw = texture(sampler_main, wuv + lo * texsize.zw).w;
    lt += lw * 0.0667; lg += lo * (lw * 0.0111); }
  { vec2 lo = vec2(1.500, -2.598);
    float lw = texture(sampler_main, wuv + lo * texsize.zw).w;
    lt += lw * 0.0667; lg += lo * (lw * 0.0222); }
  { vec2 lo = vec2(5.796, -1.553);
    float lw = texture(sampler_main, wuv + lo * texsize.zw).w;
    lt += lw * 0.0667; lg += lo * (lw * 0.0111); }
"""
def gas_cloud(l):
    return f"""
  {{
    float z = fract({l}.0 / 3.0 + q1 * 0.6 + 0.17);
    float scale = mix(4.0, 0.35, z);
    float fade = smoothstep(0.0, 0.35, z) * smoothstep(1.0, 0.7, z);
    vec2 prt = vec2(pr.x * cos(swirl) - pr.y * sin(swirl), pr.x * sin(swirl) + pr.y * cos(swirl));
    vec2 g = prt * scale + vec2({l * 13.7:.1f}, {l * 29.3:.1f});
    vec2 id = floor(g);
    vec2 f = fract(g) - 0.5;
    float h = fract(sin(dot(id, vec2(269.5, 183.3))) * 43758.5453);
    float h2 = fract(h * 57.1);
    vec2 dd = f - (vec2(h, h2) - 0.5) * 0.3;
    float blob = smoothstep(0.45, 0.0, length(dd)) * step(0.45, h);
    clouds += mix(NOKKVI_RAMP1, NOKKVI_RAMP4, h2) * blob * blob * fade * 0.07 * (0.6 + 0.8 * q5);
  }}
"""
# One frame of trail flight, sampled at quarter steps along it: the gas turns
# up to ~0.1 rad a frame near the centre, and fewer taps leave beaded copies.
def neb_trail_tap(k):
    return f"""
  {{
    float tf = {k / 4:.2f};
    float an = sw * tf;
    vec2 ck = vec2(cs.x * cos(an) - cs.y * sin(an), cs.x * sin(an) + cs.y * cos(an));
    fa = max(fa, texture(sampler_main, ck / s / pow(zm, tf) + 0.5 + bend * tf).w * {1.0 if k == 4 else 0.9:.1f});
  }}
"""
presets["nokkvi - starfield nebula"] = preset(
    {"decay": 1.0, "wave_a": 0.0, "zoom": 0.99951, "warp": 0.0101, "warpscale": 1.331, "wrap": 0},
    " shader_body {\n" + HEAD + """
  vec2 p = (uv_orig - 0.5) * s;
  float cr = cos(q4); float sr = sin(q4);
  vec2 pr = vec2(p.x * cr - p.y * sr, p.x * sr + p.y * cr);
  vec2 bd = texsize.zw * 8.0;
  vec3 rgx = GetBlur1(uv + vec2(bd.x, 0.0)) - GetBlur1(uv - vec2(bd.x, 0.0));
  vec3 rgy = GetBlur1(uv + vec2(0.0, bd.y)) - GetBlur1(uv - vec2(0.0, bd.y));
  vec2 ux = uv + vec2(rgx.x, rgy.x) * texsize.zw * 4.0;
  vec2 uy = uv + vec2(rgx.y, rgy.y) * texsize.zw * 4.0;
  vec2 uz = uv + vec2(rgx.z, rgy.z) * texsize.zw * 4.0;
  vec3 pp;
  pp.x = texture(sampler_main, ux).x - (texture(sampler_main, ux).x - GetBlur3(ux).x) * 0.02;
  pp.y = texture(sampler_main, uy).y - (texture(sampler_main, uy).y - GetBlur3(uy).y) * 0.02;
  pp.z = texture(sampler_main, uz).z - (texture(sampler_main, uz).z - GetBlur3(uz).z) * 0.02;
  pp += (texture(sampler_noise_lq, uv_orig * texsize.xy * texsize_noise_lq.zw * 0.3 + rand_frame.xy).xyz - 0.5) * 0.1 * (bass - treb);
  pp = pp - (pp.yzx * 0.1 - 0.04);
  float edge = step(min(min(uv_orig.x, 1.0 - uv_orig.x), min(uv_orig.y, 1.0 - uv_orig.y)), 0.005);
  pp = mix(pp, vec3(q20, q21, q22), edge * 0.1);
  float sdd = length((uv_orig - vec2(q23, q24)) * s) / 0.049;
  vec3 sdc = mix(vec3(0.5, 1.0, 0.9), vec3(0.83, 0.93, 0.8), clamp(sdd, 0.0, 1.0));
  pp = mix(pp, sdc, step(sdd, 1.0) * mix(0.2, 1.0, clamp(sdd, 0.0, 1.0)));
""" + neb_swirl("swirl", "uv_orig") + """
  vec3 stars = vec3(0.0);
""" + "".join(star_layer(l, "swirl", " * 1.5") for l in range(STAR_LAYERS)) + """
  float si = min(max(max(stars.x, stars.y), stars.z), 1.0);
  vec2 cs = (uv_orig - 0.5) * s;
  float crad = 2.0 * length(cs) / max(s.x, s.y);
  float sw = q31 * """ + NEB_GASROT.format(r="crad") + """;
  float zm = 1.0 + 0.012 + q2 * 0.05;
  vec2 bend = vec2(rgx.x + rgx.y + rgx.z, rgy.x + rgy.y + rgy.z) * 0.333 * texsize.zw * 12.0;
  float fa = 0.0;
""" + "".join(neb_trail_tap(k) for k in range(1, 5)) + """
  float trail = max(fa * clamp(0.8 + q2 * 0.6, 0.8, 0.95), si) * step(2.5, frame);
  vec3 dom = pp * pp * pp * pp;
  dom = dom / max(dom.x + dom.y + dom.z, 0.0001);
  pp += vec3(dom.z, dom.x, dom.y) * trail * 0.15 * (1.0 - pp);
  vec2 mc = (uv_orig - 0.5) * s;
  float mn = texture(sampler_noise_hq, uv_orig * 0.5 + vec2(time * 0.013, time * 0.007)).x;
  float mtw = q16 * exp(-length(mc) * 1.3);
  vec2 mm = vec2(mc.x * cos(mtw) - mc.y * sin(mtw), mc.x * sin(mtw) + mc.y * cos(mtw));
  mm = vec2(mm.x * cos(q14) - mm.y * sin(q14), mm.x * sin(q14) + mm.y * cos(q14)) / (1.0 + q15 * 0.7);
  mm += vec2(q12, -q13) * 0.6;
  vec2 mdisp = (mm - mc) * (0.4 + 1.2 * mn) + vec2(0.0, (abs(q12) + abs(q13) + abs(q16) * 0.3) * (0.3 + mn) * 0.5);
  vec4 held = texture(sampler_main, (floor(uv_orig * texsize.xy) + 0.5) * texsize.zw + mdisp / s);
  ret = mix(pp, held.xyz, q6);
  ret_alpha = mix(trail, held.w, q6);
 }""",
    " shader_body {\n" + HEAD + """
  float cpit = q7 + 0.025 * sin(time * 0.13);
  float cyaw = q8 + 0.025 * sin(time * 0.11 + 1.3);
  vec3 cex = vec3(cos(cyaw), 0.0, -sin(cyaw));
  vec3 cey = vec3(sin(cpit) * sin(cyaw), cos(cpit), sin(cpit) * cos(cyaw));
  vec3 cen = vec3(cos(cpit) * sin(cyaw), -sin(cpit), cos(cpit) * cos(cyaw));
  vec3 crd = vec3((uv - 0.5) * s, q18);
  vec3 chit = crd * (q18 * cen.z / max(dot(crd, cen), 0.08)) - vec3(0.0, 0.0, q18);
  vec2 cpl = vec2(dot(chit, cex), dot(chit, cey));
  float ctw = q17 * exp(-length(cpl) * 1.3);
  cpl = vec2(cpl.x * cos(ctw) - cpl.y * sin(ctw), cpl.x * sin(ctw) + cpl.y * cos(ctw));
  cpl = vec2(cpl.x * cos(q9) - cpl.y * sin(q9), cpl.x * sin(q9) + cpl.y * cos(q9)) / max(q10, 0.5);
  vec2 wuv = 1.0 - abs(1.0 - mod(cpl / s + 0.5, 2.0));
  vec2 p = (wuv - 0.5) * s;
  vec2 nd = texsize.zw * 4.0;
  vec2 ng = vec2(GetBlur1(wuv + vec2(nd.x, 0.0)).y - GetBlur1(wuv - vec2(nd.x, 0.0)).y,
                 GetBlur1(wuv + vec2(0.0, nd.y)).y - GetBlur1(wuv - vec2(0.0, nd.y)).y);
  float relief = texture(sampler_fc_main, wuv - ng * 0.4).x;
  vec3 sp = texture(sampler_main, wuv).xyz;
  vec3 sw3 = sp * sp * sp;
  sw3 = sw3 / max(sw3.x + sw3.y + sw3.z, 0.0001);
""" + ramp("nc", "sw3.y * 0.5 + sw3.z + (relief - 0.5) * 0.3") + """
  vec3 neb = mix(NOKKVI_BG, nc, clamp(0.15 + 0.95 * relief, 0.0, 1.0)) * (0.75 + 0.5 * GetBlur1(wuv).z);
  neb = mix(neb, mix(NOKKVI_HIGHLIGHT, NOKKVI_TEXT, 0.4), clamp(length(ng) * 6.0, 0.0, 1.0) * 0.35);
  float cr = cos(q4); float sr = sin(q4);
  vec2 pr = vec2(p.x * cr - p.y * sr, p.x * sr + p.y * cr);
""" + neb_swirl("swirl", "wuv") + """
  vec3 clouds = vec3(0.0);
""" + "".join(gas_cloud(l) for l in range(3)) + """
  vec3 gb3 = GetBlur3(wuv);
  float dens = clamp((gb3.x + gb3.y + gb3.z) * 0.45 - 0.2 + (gb3.x - gb3.z) * 0.6, 0.0, 1.0);
  vec2 gq = p * 0.11 + vec2(time * 0.0035, -time * 0.0023) + vec2(q1 * 0.004, 0.0);
  float gn = texture(sampler_noise_hq, gq).x * 0.65 + texture(sampler_noise_hq, gq * 2.3 + 0.37).x * 0.35;
  float gas = smoothstep(0.3, 0.7, dens * 0.4 + gn * 1.1 - 0.28);
  float tr = texture(sampler_main, wuv).w;
  float lt = tr * 0.2;
  vec2 lg = vec2(0.0);
""" + NEB_LIQUID_TAPS + """
  float lq = smoothstep(0.08, 0.34, lt);
  float lrip = texture(sampler_noise_hq, wuv * 3.0 + vec2(time * 0.02, -time * 0.015)).x - 0.5;
  vec3 ln = normalize(vec3(-lg * 4.0 + vec2(lrip, -lrip) * 0.5 * lq, 1.0));
  vec3 ll = normalize(vec3(-0.45, 0.55, 0.7));
  float ldif = max(dot(ln, ll), 0.0);
  float lspec = pow(max(dot(reflect(-ll, ln), vec3(0.0, 0.0, 1.0)), 0.0), 40.0);
  float lfres = pow(1.0 - clamp(ln.z, 0.0, 1.0), 2.0);
  float lrel = texture(sampler_fc_main, wuv - ng * 0.4 + ln.xy * 0.035 * lq).x;
  vec3 nhue = neb / max(max(neb.x, neb.y), max(neb.z, 0.05));
  vec3 liquid = mix(mix(NOKKVI_BG, nc, lrel) * 1.25, nhue, 0.25) * (0.45 + 0.7 * ldif);
  liquid += NOKKVI_TEXT * (lspec * 1.2 + lfres * 0.45);
  vec3 lcore = mix(nhue, NOKKVI_TEXT, 0.6) * smoothstep(0.5, 1.0, tr);
  float lsh = 0.5 * (texture(sampler_main, wuv + vec2(0.45, -0.55) * texsize.zw * 6.0).w
                   + texture(sampler_main, wuv + vec2(0.45, -0.55) * texsize.zw * 9.0).w);
  float lop = clamp(0.72 * mix(1.6, 0.35, gas), 0.0, 0.95);
  vec3 col = mix(NOKKVI_BG, neb, clamp(0.55 * mix(0.1, 1.55, gas), 0.0, 1.0)) + clouds * (0.4 + 0.8 * gas);
  col *= 1.0 - 0.4 * smoothstep(0.05, 0.3, lsh) * (1.0 - lq) * lop;
  col += NOKKVI_ACCENT * exp(-length(p) * 6.0) * 0.3 * q3;
  col = mix(col, liquid, lq * lop);
  vec3 sl = clamp(lcore * 1.4 + mix(nhue, NOKKVI_TEXT, 0.5) * tr * 0.9 * (1.0 - lq), 0.0, 1.0) * lop;
  col = 1.0 - (1.0 - clamp(col, 0.0, 1.0)) * (1.0 - sl);
  col *= 0.9 + 0.1 * smoothstep(1.1, 0.2, length((uv - 0.5) * s));
  col += (texture(sampler_noise_lq, uv * texsize.xy / 256.0 + rand_frame.xy).x - 0.5) * 0.012;
  vec4 cprev = textureLod(sampler2D(sampler_prev_comp, sampler_prev_comp_samp), uv, 0.0);
  col = mix(col, cprev.rgb, clamp(q11, 0.0, 0.8) * cprev.a);
  ret = col;
 }""",
    init="pulse = 0; pop = 0; travel = 0; roll = 0; speed = 0; " + " ".join(f"sa{i} = 0;" for i in range(6))
         + " at = 0; sinceb = 1; ibi = 0.5; nacts = 0; ddur = 0.25; inact = 0; aa = 0; e = 0; eprev = 0; dee = 0; ttw = 0; frz = 0;"
         " cpit = 0; cyaw = 0; crol = 0; czm = 1; fpit = 0; fyaw = 0; frol = 0; fzm = 1; cfoc = 1.6; ffoc = 1.6; tfoc = 1.6;"
         " tpit = 0; tyaw = 0; trol = 0; tzm = 1;",
    frame=PULSE + NEB_CAMERA + "speed = speed * 0.9 + 0.1 * (0.012 + 0.03 * min(bass_att, 2) + 0.12 * q3 + 0.1 * pop);\n"
          "travel = travel + speed * 0.25 * (1 - frz);\nroll = roll + (0.0012 * (mid_att - 0.8) + 0.004 * q3 * sign(sin(time * 0.05))) * (1 - frz);\n"
          "q1 = travel;\nq2 = speed * 6;\nq4 = roll;\n"
          "q20 = min(max(0.5 * sin(time * 1.13), 0), 1);\n"
          "q21 = min(max(0.99 + 0.5 * sin(time * 1.23), 0), 1);\n"
          "q22 = min(max(1 + 0.5 * sin(time * 1.33), 0), 1);\n"
          "q23 = 0.5 + sin(time);\nq24 = 0.5 + cos(time * 0.65);\n"
          "q31 = min(0.7 + q2 * 0.8, 1.5);\n"
          + "".join(f"sa{i} = sa{i} - (sa{i} * 0.008 - q31 * {NEB_GASROT.format(r=r)}) * (1 - frz);\nq{25 + i} = sa{i};\n"
                    for i, r in enumerate(NEB_RADII)))
presets["nokkvi - starfield nebula"]["pixel_eqs_eel"] = (
    "zoom = zoom + 0.0095*(sin(10*ang) + sin(sin(time*2*sin(time)*rad))*0.3 - cos(rad)*0.1);\n"
    "rot = rot + q31*0.08*abs(0.746-rad)*sin(2.2*(0.5-rad)+5.7*sin(0.1*time));\n"
    "sx = sx + 0.01*(0.99*1-rad)*sin(0.733*time)*below(sin(time),0);\n"
    "sy = sy + 0.01*(0.99*1-rad)*cos(0.953*time)*above(sin(time),0);\n"
    "zoom = zoom - 0.015*(0.5*abs(3)-rad)*below(rad,1.5);")

# Living ink (no cover) ----------------------------------------------------
# Ink cascade: drops of ink shoot into dark water from every direction and the
# ink stays. The feedback is not a picture: it holds a 3D dye volume (y slices
# tiled as a 2D atlas from texel (0, 1); 96^3 at 1080p, 64^3 / 48^3 / 32^3 on
# smaller renders, picked from texsize), RGB = three dye species (three stops
# of the theme ramp, chosen per visit), A = heat (the beat glow). The WARP is
# the simulation, one step a frame: semi-Lagrangian advection through an
# analytic velocity field (divergence-free curl noise whose strength follows
# the mids, each live ring's swirl around its core circle and the drag of its
# travel), slow decay, and injection from the analytic drop model: a bead
# punches in on a kick, opens into a vortex ring, grows lobes and breaks into
# 3 to 7 tilted child rings (and again, for big bass-heavy drops), and every
# part deposits dye of the drop's species where it is. The model lives in a
# per-drop frame (ik_slot_frame): one of 64 travel directions spread over the
# sphere, aimed at a point near the tank's centre, so drops cross the tank
# from all sides. Each drop leaves a turbulent wake and each ring sheds a
# cloud that keeps drifting and curling after the shape is gone. Values are
# rounded stochastically and unbiased (floor(v * 255 + hash)), so slow decay
# never stalls; total mass is bounded (decay 3 s plus a small absolute loss,
# faster within half a unit of the walls so no cloud is cut by one).
# Beside the atlas the warp keeps a 4x coarser occupancy grid (any dye within
# two cells), which lets the render leap empty water, a header texel (grid
# size: a new grid size or a seeded start re-initialises the tank, which is
# then pre-filled with seven noise-warped blobs in the three species so the
# first frames are not empty) and one row of per-drop waveform harmonics.
# Renders smaller than 216 x 193 have no dye grid, only the young drops.
# The COMP is the render, at full resolution. Dense ink is a liquid BODY with
# a surface: the dye density, eroded by three octaves of noise (the lookup
# itself displaced by a noise offset) through a steep transfer, is marched
# until it crosses the body threshold, the crossing is bisected (4 steps) to
# the iso-surface, and that point is shaded as glossy liquid: a normal from
# the field's gradient, wrapped key light with a shadow tap and caustic dapple
# on upward faces, a tight specular, a Fresnel rim reflecting the water, the
# back light glowing through thin parts (thickness from one tap behind the
# surface), colour deepening with thickness. Below the body threshold the same
# field is a thin translucent VEIL, accumulated as volume in front of the
# bodies. The young drop (bead, then ring, up to IK_NF[1] s) joins the same
# field analytically (sphere-traced inside its bounding sphere), so it is
# crisp, and hands over to its own dye. The march has three field functions
# because the young-drop code is the expensive part: ik_bd (dye only, most
# steps), ik_bf (dye + every young drop, only between the ray's first entry
# into and last exit from a bounding sphere) and ik_b1 (dye + the one drop
# that was hit, for shading). Species keep their (chroma-boosted) ramp colours
# and mix by density. Water is a depth gradient (dark below, lit from above),
# with faint shafts and drifting motes. Jitter (a fixed per-pixel hash stepped
# by the golden ratio each frame) is resolved by TAA against
# sampler_prev_comp, reprojected with last frame's camera and clamped to the
# new sample (+/- 0.12).
# Music: each drop holds a snapshot of the moment it was born: the spectral
# balance (each band against its own 8 s average) picks the child count, the
# beat strength its size, the mids how turbulent it gets, the waveform's
# harmonics 2 to 5 (captured from get_wave into the data row on the spawn
# frame) bend its rim; species cycle so neighbours differ. Each beat lights
# the dense ink and the young ring's core (heat, in the theme's warm colour)
# and sends a band of that light out from the tank's centre through the ink
# bodies (5 units a second); it also kicks a damped spring (kvel / kpos, in
# ink time, so it plays as the clock resumes after the camera move) whose
# velocity scales the whole tank about its centre and twists it about a
# slowly turning axis: the ink thumps outward and recoils. The bass level
# (fast attack, 0.22 s release) lifts the body field in the render, so the
# bodies and young rings swell and thin with it (about their usual size); the treble level tilts the surface
# normals with fine drifting noise, so the highlights ripple. The mids stir the
# whole tank (0.1 to 0.32); loudness sets the ink clock and how soon the
# next drop may come (0.6-0.9 ink-seconds on a beat, 3 without one, and never
# before the oldest of the IK_NS slots has finished its life).
# Camera: it stands still and moves only with the music (starfield nebula's
# language). On a beat (or after 4 s without one) the ink clock stops, and the
# camera swings to a new pose around the tank in 42% of the beat interval
# (0.12-0.36 s, ease-out): a kick-sized orbit step, a new elevation and lens,
# and a roll that alternates sides (the twist); every 8th move re-frames
# widely, every 4th lands level. While it swings the COMP mixes in last
# frame's picture, reprojected through both cameras, at up to 85%: the frozen
# frame drags and twists as an echo smear and the new view resolves as the
# camera settles. The pose is (azimuth, elevation, lens, roll, look-at) at a
# fixed distance. Each move re-aims at where the live drops' rings will be
# 0.7 s later (their weighted centroid, ik_eel_drop) and sets the lens from
# their spread, so the ink fills the frame instead of sitting small in empty
# water. Each visit picks a mood from the audio at frame 3:
# studio black (side key), sunlit shallows (key from above, shafts) or abyss
# (dim key, strong glow), and which ramp stops the three species take.
# q map: q1/q2 azimuth, elevation; q3 lens and roll (12 bits each, ik_cam
# unpacks); q4-q6 the same last frame; q7 echo weight while the camera moves
# (> 0) or minus the TAA weight; q8 spawn flag (slot + 1 on the spawn frame)
# + bass swell (fraction); q9 ink clock; q10 mood + 4 * treble ripple
# (hundredths) + species ramp offset (fraction); q11 turbulence (fraction) + the kick spring's velocity
# ((v + 4) * 100, integer part); q12 this frame's ink-clock step (0 while the
# camera moves); q13/q14 the last two beats (age + 16 * strength in
# hundredths); q15/q16 the look-at point's offsets across and up the view
# (12 bits each), now and last frame; q17-q24 slot ages; q25-q32 slot codes
# (ik_code).
IK_NS = 8
IK_LIFE = 8.0
IK_TS = 3.0
IK_KOFF = 0.15
IK_TSURF = 0.14
IK_KSF = 1.7955  # 0.7 ln(1 + IK_TS / 0.25): the split depth
IK_SC = 1.25
IK_BMIN = (-2.4, -2.4, -2.4)
IK_BSIZE = (4.8, 4.8, 4.8)
IK_STEPS = 120
IK_DTS = 0.066
IK_CR = 5.0        # camera distance from the tank's centre
IK_CSTEP = 0.5     # spacing of the 4x4x4 lattice of aim points
IK_LEAD = 1.3      # a drop starts this many of its own sizes before its aim point...
IK_LEADMAX = 2.0   # ...but no farther than this
IK_RATE = 1.3      # ink clock scale (the clock stands still during a camera move)
IK_NF = (1.2, 1.9) # a young drop's analytic shape fades into its own dye over these ages
IK_TOK = {
    "@SC@": f"{IK_SC}", "@TS@": f"{IK_TS}", "@TS2@": f"{IK_TS + 1.2}", "@KSF@": f"{IK_KSF:.4f}",
    "@SR@": f"{0.36 * (1.0 + 0.09 * IK_TS):.4f}", "@L0@": f"{IK_LIFE - 2.2}", "@L1@": f"{IK_LIFE - 0.3}",
    "@LIFE@": f"{IK_LIFE}", "@DTS@": f"{IK_DTS}", "@CR@": f"{IK_CR}", "@CSTEP@": f"{IK_CSTEP}",
    "@LEAD@": f"{IK_LEAD}", "@LEADMAX@": f"{IK_LEADMAX}", "@NS2@": f"{2 * IK_NS}", "@NF0@": f"{IK_NF[0]}", "@NF1@": f"{IK_NF[1]}",
    "@BMIN@": "vec3({}, {}, {})".format(*IK_BMIN), "@BSIZE@": "vec3({}, {}, {})".format(*IK_BSIZE),
    "@BMAX@": "vec3({}, {}, {})".format(*(a + b for a, b in zip(IK_BMIN, IK_BSIZE))),
    "@RMIN@": "vec3({}, {}, {})".format(*(a - 0.4 for a in IK_BMIN)),
    "@RMAX@": "vec3({}, {}, {})".format(*(a + b + 0.4 for a, b in zip(IK_BMIN, IK_BSIZE))),
    "@STEPS@": f"{IK_STEPS}", "@KOFF@": f"{IK_KOFF}", "@TSURF@": f"{IK_TSURF}",
}
def ik_sub(text):
    for k, v in IK_TOK.items():
        text = text.replace(k, v)
    return text
def ik_lod0(shader):
    """Read the noise volumes at mip 0. A plain texture() takes its mip from
    screen-space derivatives, which are arbitrary inside the march, so the
    same point got a different field in the march loop and in the shading
    (the surface the march found was gone when it was shaded: black patches)."""
    import re
    out, i = [], 0
    for m in re.finditer(r"texture\((sampler_noisevol_(?:hq|lq)), ", shader):
        j, depth = m.end(), 1
        while depth:
            depth += (shader[j] == "(") - (shader[j] == ")")
            j += 1
        out.append(shader[i:m.start()] + f"textureLod(sampler3D({m.group(1)}, {m.group(1)}_samp), {shader[m.end():j - 1]}, 0.0)")
        i = j
    return "".join(out) + shader[i:]

IK_COMMON = ik_sub("""
float ik_ba(float bqa) {
  return bqa - 16.0 * floor(bqa / 16.0);
}
float ik_bs(float bqs) {
  return floor(bqs / 16.0) * 0.01;
}
float ik_hash(float hx) {
  float hp = fract(hx * 0.1031);
  hp *= hp + 33.33;
  hp *= hp + hp;
  return fract(hp);
}
float ik_vn() {
  return 32.0 + 16.0 * step(384.0, texsize.x) * step(337.0, texsize.y) + 16.0 * step(576.0, texsize.x) * step(513.0, texsize.y)
       + 32.0 * step(1080.0, texsize.x) * step(961.0, texsize.y);
}
float ik_vt() {
  return 6.0 + step(384.0, texsize.x) * step(337.0, texsize.y) + step(576.0, texsize.x) * step(513.0, texsize.y)
       + 2.0 * step(1080.0, texsize.x) * step(961.0, texsize.y);
}
float ik_vq() {
  return step(216.0, texsize.x) * step(193.0, texsize.y);
}
float ik_occ(vec3 op) {
  float on = ik_vn();
  float onc = on * 0.25;
  float otc = 3.0 + step(10.0, onc) + step(20.0, onc);
  vec3 og = clamp(floor((op - @BMIN@) / @BSIZE@ * onc), vec3(0.0), vec3(onc - 1.0));
  vec2 oo = vec2(ik_vt() * on, 1.0) + vec2(mod(og.y, otc), floor(og.y / otc)) * onc + og.xz;
  return texelFetch(sampler2D(sampler_pc_main, sampler_pc_main_samp), ivec2(oo), 0).x;
}
vec4 ik_vol(vec3 vp) {
  float vn = ik_vn();
  float vt = ik_vt();
  vec3 vg = (vp - @BMIN@) / @BSIZE@ * vn - 0.5;
  vec3 vin3 = step(vec3(-0.5), vg) * step(vg, vec3(vn - 0.5));
  float vin = vin3.x * vin3.y * vin3.z * ik_vq();
  float vk = clamp(vg.y, 0.0, vn - 1.0);
  float vk0 = floor(vk);
  float vf = vk - vk0;
  float vk1 = min(vk0 + 1.0, vn - 1.0);
  vec2 vij = clamp(vg.xz, vec2(0.0), vec2(vn - 1.0)) + 0.5;
  vec2 vo0 = vec2(mod(vk0, vt), floor(vk0 / vt)) * vn + vec2(0.0, 1.0);
  vec2 vo1 = vec2(mod(vk1, vt), floor(vk1 / vt)) * vn + vec2(0.0, 1.0);
  vec4 va = textureLod(sampler2D(sampler_fc_main, sampler_fc_main_samp), (vo0 + vij) * texsize.zw, 0.0);
  vec4 vb = textureLod(sampler2D(sampler_fc_main, sampler_fc_main_samp), (vo1 + vij) * texsize.zw, 0.0);
  return mix(va, vb, vf) * vin;
}
""")

IK_DROPFN = ik_sub("""
vec4 ik_drop(vec3 kp, float kage, float kcode, float kslot, float kdet) {
  float kn = mod(kcode, 8.0) + 3.0;
  float ksz = (0.8 + 0.15 * mod(floor(kcode / 64.0), 4.0)) * @SC@;
  float ktb = 0.6 + 0.3 * mod(floor(kcode / 1024.0), 4.0);
  float kseed = ik_hash(kcode * 0.00137 + kslot * 3.7);
  int kix = int(kslot) * 2;
  float kgmax = 2.0 + step(1.5, mod(floor(kcode / 64.0), 4.0)) * step(kn, 4.5);
  vec3 kq = kp / ksz;
  float kf0 = 0.7 * log(1.0 + kage / 0.25);
  float ktil = (0.12 + 0.22 * ik_hash(kseed + 5.1)) * smoothstep(0.3, 1.5, kage);
  float ktaz = ik_hash(kseed + 7.7) * 6.2831853;
  vec3 kax = vec3(cos(ktaz), 0.0, sin(ktaz));
  vec3 kc0 = vec3(0.0, @KOFF@ - kf0, 0.0);
  vec3 krl = kq - kc0;
  krl = krl * cos(ktil) + cross(kax, krl) * sin(ktil) + kax * dot(kax, krl) * (1.0 - cos(ktil));
  kq = krl + kc0;
  float kdep = max(-kq.y, 0.0);
  vec2 kcur = normalize(vec2(ik_hash(kseed + 0.37), ik_hash(kseed + 0.71)) - 0.5 + 0.001);
  kq.xz -= kcur * 0.025 * kdep * kdep * smoothstep(0.5, 6.0, kage);
  vec3 knz = texture(sampler_noisevol_hq, vec3(kq.x, kq.y + kf0 * 0.8, kq.z) * 0.45 + vec3(kseed * 5.0, q9 * 0.013, kslot * 0.31)).xyz - 0.5;
  float kwa = (0.07 + 0.1 * smoothstep(1.0, 8.0, kage)) * ktb;
  kq += knz * kwa;
  float kdens = 0.0;
  float kemit = 0.0;
  float ksd = 1000.0;
  float kfs = @DTS@;
  float kt = kage;
  float ksc = ksz;
  float kph = kseed * 6.2831853;
  float kgn = kn;
  vec2 kpc = vec2(cos(kgn * kph), sin(kgn * kph));
  float koff = -@KOFF@;
  vec3 kqq = kq;
  for (int kg = 0; kg < 3; kg++) {
    float kfg = float(kg);
    float kbl = smoothstep(0.02, 0.38, kt);
    float gR = 0.36 * kbl * (1.0 + 0.09 * kt);
    float gfall = mix(0.7 * log(1.0 + kt / 0.25), kf0, step(kfg, 0.5)) + koff;
    float ga = mix(0.078, 0.066, smoothstep(0.0, 0.3, kt)) * (1.0 + 0.07 * kt);
    float gA = smoothstep(1.1, @TS@, kt);
    float glive = 1.0 - smoothstep(@TS@, @TS2@, kt) * (1.0 - step(1.5, kfg)) * 0.85;
    vec3 rq = kqq + vec3(0.0, gfall, 0.0);
    float krho = length(rq.xz);
    vec2 ku1 = rq.xz / max(krho, 0.00001);
    vec2 ku2 = vec2(ku1.x * ku1.x - ku1.y * ku1.y, 2.0 * ku1.x * ku1.y);
    vec2 ku3 = vec2(ku2.x * ku1.x - ku2.y * ku1.y, ku2.x * ku1.y + ku2.y * ku1.x);
    vec2 ku4 = vec2(ku2.x * ku2.x - ku2.y * ku2.y, 2.0 * ku2.x * ku2.y);
    vec2 ku5 = vec2(ku4.x * ku1.x - ku4.y * ku1.y, ku4.x * ku1.y + ku4.y * ku1.x);
    vec2 ku6 = vec2(ku3.x * ku3.x - ku3.y * ku3.y, 2.0 * ku3.x * ku3.y);
    vec2 ku7 = vec2(ku6.x * ku1.x - ku6.y * ku1.y, ku6.x * ku1.y + ku6.y * ku1.x);
    vec2 kun = mix(ku3, ku4, step(3.5, kgn));
    kun = mix(kun, ku5, step(4.5, kgn));
    kun = mix(kun, ku6, step(5.5, kgn));
    kun = mix(kun, ku7, step(6.5, kgn));
    float klob = kun.x * kpc.x + kun.y * kpc.y;
    float khw = 0.0;
    if (kg == 0 && abs(krho - gR) < 2.5 * ga + 0.25 * gR && abs(rq.y) < 2.5 * ga + 1.2 * gR) {
      vec4 kh1 = texelFetch(sampler2D(sampler_pc_main, sampler_pc_main_samp), ivec2(kix, 0), 0) * 2.0 - 1.0;
      vec4 kh2 = texelFetch(sampler2D(sampler_pc_main, sampler_pc_main_samp), ivec2(kix + 1, 0), 0) * 2.0 - 1.0;
      khw = dot(kh1.xy, ku2) + dot(kh1.zw, ku3) + dot(kh2.xy, ku4) + dot(kh2.zw, ku5);
    }
    float kl01 = 0.5 + 0.5 * klob;
    float ksgv = 0.6 + 0.6 * ik_hash(kseed + kfg * 1.9 + 2.2);
    float rR = gR * (1.0 + gA * (0.12 * klob + 0.08 * khw));
    float ksag = gA * gR * (kl01 * kl01 * 0.9 * ksgv + 0.15 * khw);
    vec2 ktd = vec2(krho - rR, rq.y + ksag);
    float ktr = length(ktd);
    float kthin = 1.0 - 0.8 * gA * (1.0 - kl01);
    float ka2 = ga * (0.55 + 0.45 * kthin);
    float kcore = 0.0;
    if (ktr < 1.95 * ka2) {
      float kbase = 1.0 - ktr / (1.8 * ka2);
      float kero = 0.5;
      if (kdet > 0.5) {
        vec3 ken = rq * (0.2 / ga) + vec3(kseed * 9.0 + kfg * 2.3, q9 * 0.02, kslot * 1.7);
        kero = texture(sampler_noisevol_hq, ken).x * 0.65 + texture(sampler_noisevol_hq, ken * 2.3 + 0.41).x * 0.35;
      }
      float kbody = smoothstep(0.0, 0.1, kbase - 0.75 * (kero - 0.5) - 0.12);
      kcore = smoothstep(0.45, 0.8, kbase);
      kdens += (kbody * 0.85 + kcore * 0.6) * glive * kthin;
    }
    if (kcore > 0.0001) {
      float ke1 = (ik_ba(q13) - 0.18 * kfg) / 0.12;
      float ke2 = (ik_ba(q14) - 0.18 * kfg) / 0.12;
      float kgl = ik_bs(q13) * exp(-ke1 * ke1) + ik_bs(q14) * exp(-ke2 * ke2);
      kemit += sqrt(kcore) * glive * kthin * (kgl * 1.3 + (1.0 - step(0.5, kfg)) * smoothstep(0.35, 0.0, kt) * 1.2);
    }
    float ksdr = (ktr - 1.95 * ka2) * ksc;
    if (ksdr < ksd) { ksd = ksdr; kfs = clamp(0.35 * ka2 * ksc, mix(0.055, 0.08, step(0.5, kfg)), @DTS@); }
    if (kt < @TS@ || kqq.y > -@KSF@ + 0.25 || kfg > kgmax - 1.5) break;
    float kphi = atan(rq.z, rq.x);
    float ksect = 6.2831853 / kgn;
    float kck = mod(floor((kphi - kph) / ksect + 0.5), kgn);
    float kang = kph + kck * ksect;
    float kcc = cos(kang);
    float kcs = sin(kang);
    vec3 kcq = vec3(kcc * kqq.x + kcs * kqq.z, kqq.y, -kcs * kqq.x + kcc * kqq.z);
    float ksR = @SR@;
    float ksf = @KSF@ + koff;
    float kcsc = 0.42 * (0.75 + 0.5 * ik_hash(kseed * 3.3 + kck * 1.7 + kfg));
    kcq -= vec3(ksR * 1.05, -ksf - ksR * 0.9, 0.0);
    float ktbh = ik_hash(kseed + kck);
    vec2 ktcs = normalize(vec2(0.9 - 0.08 * ktbh, 0.43 + 0.18 * ktbh));
    kcq.xy = vec2(ktcs.x * kcq.x + ktcs.y * kcq.y, -ktcs.y * kcq.x + ktcs.x * kcq.y);
    kqq = kcq / kcsc;
    ksc *= kcsc;
    kt = (kt - @TS@) * 1.3;
    kph = ik_hash(kseed + kck * 7.13 + kfg * 3.1) * 6.2831853;
    kgn = max(kgn - 1.0 + floor(ik_hash(kseed + kck) * 2.0), 3.0);
    kpc = vec2(cos(kgn * kph), sin(kgn * kph));
    koff = 0.0;
  }
  float kfade = (1.0 - smoothstep(@L0@, @L1@, kage)) * mix(1.0, 1.0 - smoothstep(3.4, 5.8, kage), kdet);
  return vec4(kdens * kfade, kemit * kfade, ksd - kwa * ksz * 0.4, kfs);
}
""")

def ik_slot_vals(k):
    return ik_slot_frame(f"q{17 + k}", f"q{25 + k}")
def ik_slot_frame(age, code):
    """A drop's age, code and frame: the drop model lives in a local frame whose
    -y axis is the drop's travel direction (one of 64 directions spread evenly
    over the sphere, a Fibonacci lattice); `sex`, `sey`, `sez` are that frame's
    axes in the tank (a branchless orthonormal basis, Duff et al. 2017) and
    `so` its origin: the aim point (a 4x4x4 lattice about the tank's centre)
    backed off against the travel direction."""
    return ik_sub(f"""
        float sa = {age};
        float sc = {code};
        float sdi = mod(floor(sc / 4096.0), 64.0);
        float sdh = (2.0 * sdi + 1.0) / 64.0 - 1.0;
        float sdr = sqrt(max(1.0 - sdh * sdh, 0.0));
        vec3 sey = vec3(-sdr * cos(sdi * 2.3999632), sdh, -sdr * sin(sdi * 2.3999632));
        float ssg = step(0.0, sey.z) * 2.0 - 1.0;
        float sfa = -1.0 / (ssg + sey.z);
        float sfb = sey.x * sey.y * sfa;
        vec3 sez = vec3(1.0 + ssg * sey.x * sey.x * sfa, ssg * sfb, -ssg * sey.x);
        vec3 sex = vec3(sfb, ssg + sey.y * sey.y * sfa, -sey.y);
        float ssz = (0.8 + 0.15 * mod(floor(sc / 64.0), 4.0)) * @SC@;
        vec3 saim = (vec3(mod(floor(sc / 262144.0), 4.0), mod(floor(sc / 256.0), 4.0), mod(floor(sc / 1048576.0), 4.0)) - 1.5) * @CSTEP@;
        vec3 so = saim + sey * min(@LEAD@ * ssz, @LEADMAX@);
        float sspc = mod(floor(sc / 8.0), 8.0);
        vec3 smsk = vec3(1.0 - step(0.5, sspc), step(0.5, sspc) * (1.0 - step(1.5, sspc)), step(1.5, sspc));
""")
def ik_slot_bound(pv):
    return ik_sub(f"""
        float sf0 = 0.7 * log(1.0 + sa / 0.25) - @KOFF@;
        float sbr = ssz * (0.42 * smoothstep(0.02, 0.38, sa) * (1.0 + 0.09 * min(sa, 3.0)) + 0.45 + 0.95 * smoothstep(2.8, 5.5, sa)) + 0.12;
        float sbd = max(length({pv}.xz) - sbr, max({pv}.y + ssz * (sf0 - 0.45), -3.9 * ssz - {pv}.y));
        sbd = mix(1000.0, sbd, step(sa, @LIFE@));
""")

def ik_warp_slot(k):
    """Simulation cell: the ring's swirl and descent drag on the velocity, and
    the drop's dye (its species) and beat heat deposited where it is."""
    return ik_sub(f"""
      {{""" + ik_slot_vals(k) + """
        vec3 swp = wp - so;
        vec3 swl = vec3(dot(swp, sex), dot(swp, sey), dot(swp, sez));
""" + ik_slot_bound("swl") + f"""
        if (sa < 6.5) {{
          float swf = (0.7 * log(1.0 + sa / 0.25) - @KOFF@) * ssz;
          vec3 swd = swl + vec3(0.0, swf, 0.0);
          float swR = 0.36 * ssz * smoothstep(0.02, 0.38, sa) * (1.0 + 0.09 * sa);
          float swrho = length(swd.xz);
          vec2 swq = vec2(swrho - swR, swd.y);
          float swr2 = dot(swq, swq);
          float swa = 0.09 * ssz;
          float swg = 0.3 * ssz * exp(-sa / 2.2);
          vec2 swu = vec2(-swq.y, swq.x) * swg / (swr2 + swa * swa);
          vec2 swh = swd.xz / max(swrho, 0.001);
          float swy = swu.y - 0.7 / (0.25 + sa) * ssz * 0.6 * exp(-swr2 / (swR * swR + 0.05));
          wv += sex * (swh.x * swu.x) + sey * swy + sez * (swh.y * swu.x);
        }}
        if (sbd < 0.0) {{
          vec4 sr = ik_drop(swl, sa, sc, {k}.0, 0.0);
          wdep += sr.x * smsk;
          whd += sr.y;
        }}
      }}
""")

# The dye volume's field at a point (shared by the two march functions below):
# density through the erosion noise, species in `fsp`.
IK_DYEBODY = ik_sub("""
  float fv = -1.0;
  float focc = 0.0;
  fsp = vec4(0.0);
  if (ik_occ(fp) > 0.001) {
    focc = 1.0;
    vec3 fq = fp * vec3(0.44, 0.33, 0.44) + vec3(q9 * 0.004, q9 * 0.011, -q9 * 0.003);
    vec3 fcn = ik_sn(fq) - 0.5;
    vec3 fdp = fp + fcn * 0.2;
    vec4 fvd = ik_vol(fdp);
    float fsum = fvd.x + fvd.y + fvd.z;
    if (fsum > 0.02) {
      vec3 fq1 = fq * 2.6 + fcn * 0.35 + 0.17;
      vec3 fq2 = fq * 6.1 + fcn * 0.5 + 0.53;
      vec3 fq3 = fq * 4.3 + fcn * 0.3 + 0.29;
      float fno = ik_sn(fq1).x * 0.5 + texture(sampler_noisevol_hq, fq2).x * 0.3 + ik_snl(fq3) * 0.2;
      float fbs = smoothstep(0.05, 0.42, fsum);
      fv = fbs * 1.3 - 0.26 - (fno - 0.5) * (1.25 - 0.45 * fbs) + (fract(q8) - 0.4) * 0.25;
      fsp = vec4(fvd.xyz / fsum, fvd.w);
    }
  }
""")
# March function without the young drops: most steps of most rays are nowhere
# near one, and the per-drop bounding tests are not free.
IK_DYEFN = ik_sub("""
float ik_bd(vec3 fp, float ft, out vec4 fsp, out vec4 fax) {
""") + IK_DYEBODY + ik_sub("""
  fax = vec4(fv, 1000.0, focc, 0.0);
  return fv - @TSURF@ - (1.0 - smoothstep(0.9, 2.0, ft)) * 2.0;
}
""")

IK_NEARFN = ik_sub("""
float ik_snl(vec3 qnx) {
  vec3 qny = qnx * 32.0 + 0.5;
  vec3 qni = floor(qny);
  vec3 qnf = qny - qni;
  qnf = qnf * qnf * (3.0 - 2.0 * qnf);
  vec3 qnc = (qni + qnf - 0.5) / 32.0;
  return texture(sampler_noisevol_lq, qnc).x;
}
vec3 ik_sn(vec3 sx) {
  vec3 sy = sx * 32.0 + 0.5;
  vec3 si = floor(sy);
  vec3 sf = sy - si;
  sf = sf * sf * sf * (sf * (sf * 6.0 - 15.0) + 10.0);
  return texture(sampler_noisevol_hq, (si + sf - 0.5) / 32.0).xyz;
}
vec2 ik_near(vec3 np, float nage, float ncode, float nslot) {
  float nn = mod(ncode, 8.0) + 3.0;
  float nsz = (0.8 + 0.15 * mod(floor(ncode / 64.0), 4.0)) * @SC@;
  float ntb = 0.6 + 0.3 * mod(floor(ncode / 1024.0), 4.0);
  float nseed = ik_hash(ncode * 0.00137 + nslot * 3.7);
  vec3 nq = np / nsz;
  float nf0 = 0.7 * log(1.0 + nage / 0.25);
  float ntil = (0.12 + 0.22 * ik_hash(nseed + 5.1)) * smoothstep(0.3, 1.5, nage);
  float ntaz = ik_hash(nseed + 7.7) * 6.2831853;
  vec3 nax = vec3(cos(ntaz), 0.0, sin(ntaz));
  vec3 nc0 = vec3(0.0, @KOFF@ - nf0, 0.0);
  vec3 nrl = nq - nc0;
  nrl = nrl * cos(ntil) + cross(nax, nrl) * sin(ntil) + nax * dot(nax, nrl) * (1.0 - cos(ntil));
  nq = nrl + nc0;
  vec3 nnz = texture(sampler_noisevol_hq, vec3(nq.x, nq.y + nf0 * 0.8, nq.z) * 0.45 + vec3(nseed * 5.0, q9 * 0.013, nslot * 0.31)).xyz - 0.5;
  float nwa = (0.07 + 0.1 * smoothstep(1.0, 8.0, nage)) * ntb;
  nq += nnz * nwa;
  float nbl = smoothstep(0.02, 0.38, nage);
  float nR = 0.36 * nbl * (1.0 + 0.09 * nage);
  float nga = mix(0.078, 0.066, smoothstep(0.0, 0.3, nage)) * (1.0 + 0.07 * nage);
  float nA = smoothstep(1.1, @TS@, nage);
  vec3 nrq = nq + vec3(0.0, nf0 - @KOFF@, 0.0);
  float nrho = length(nrq.xz);
  vec2 nu1 = nrq.xz / max(nrho, 0.00001);
  vec2 nu2 = vec2(nu1.x * nu1.x - nu1.y * nu1.y, 2.0 * nu1.x * nu1.y);
  vec2 nu3 = vec2(nu2.x * nu1.x - nu2.y * nu1.y, nu2.x * nu1.y + nu2.y * nu1.x);
  vec2 nu4 = vec2(nu2.x * nu2.x - nu2.y * nu2.y, 2.0 * nu2.x * nu2.y);
  vec2 nu5 = vec2(nu4.x * nu1.x - nu4.y * nu1.y, nu4.x * nu1.y + nu4.y * nu1.x);
  vec2 nu6 = vec2(nu3.x * nu3.x - nu3.y * nu3.y, 2.0 * nu3.x * nu3.y);
  vec2 nu7 = vec2(nu6.x * nu1.x - nu6.y * nu1.y, nu6.x * nu1.y + nu6.y * nu1.x);
  vec2 nun = mix(nu3, nu4, step(3.5, nn));
  nun = mix(nun, nu5, step(4.5, nn));
  nun = mix(nun, nu6, step(5.5, nn));
  nun = mix(nun, nu7, step(6.5, nn));
  float nph = nseed * 6.2831853;
  float nlob = nun.x * cos(nn * nph) + nun.y * sin(nn * nph);
  int nix = int(nslot) * 2;
  vec4 nh1 = texelFetch(sampler2D(sampler_pc_main, sampler_pc_main_samp), ivec2(nix, 0), 0) * 2.0 - 1.0;
  vec4 nh2 = texelFetch(sampler2D(sampler_pc_main, sampler_pc_main_samp), ivec2(nix + 1, 0), 0) * 2.0 - 1.0;
  float nhw = dot(nh1.xy, nu2) + dot(nh1.zw, nu3) + dot(nh2.xy, nu4) + dot(nh2.zw, nu5);
  float nl01 = 0.5 + 0.5 * nlob;
  float nsgv = 0.6 + 0.6 * ik_hash(nseed + 2.2);
  float nrR = nR * (1.0 + nA * (0.12 * nlob + 0.08 * nhw));
  float nsag = nA * nR * (nl01 * nl01 * 0.9 * nsgv + 0.15 * nhw);
  vec2 ntd = vec2(nrho - nrR, nrq.y + nsag);
  float ntr = length(ntd);
  float nthin = 1.0 - 0.8 * nA * (1.0 - nl01);
  float na2 = nga * (0.55 + 0.45 * nthin);
  float nbase = 1.0 - ntr / (1.8 * na2);
  float nemit = 0.0;
  if (nbase > -0.4) {
    vec3 nen = nrq * (0.11 / nga) + vec3(nseed * 9.0, q9 * 0.02, nslot * 1.7);
    float nero = ik_sn(nen).x;
    nbase -= 0.5 * (nero - 0.5);
    float ne1 = ik_ba(q13) / 0.12;
    float ne2 = ik_ba(q14) / 0.12;
    nemit = smoothstep(-0.2, 0.5, nbase) * ((ik_bs(q13) * exp(-ne1 * ne1) + ik_bs(q14) * exp(-ne2 * ne2)) * 0.8 + smoothstep(0.4, 0.0, nage) * 2.0);
  }
  float nfade = smoothstep(@NF0@, @NF1@, nage);
  return vec2((nbase - 0.22 + (fract(q8) - 0.4) * 0.22 - nfade * 1.3) * 1.8 * na2 * nsz * 14.0, nemit * (1.0 - nfade));
}
""") + IK_DYEFN + ik_sub("""
float ik_bf(vec3 fp, float ft, """) + ", ".join(f"vec4 fb{k}" for k in range(IK_NS)) + ik_sub(""", out vec4 fsp, out vec4 fax) {
""") + IK_DYEBODY + ik_sub("""
  float fr = -1000.0;
  float frd = 1000.0;
  float fwin = 0.0;
""") + "".join(ik_sub(f"""
  float sbd{k} = length(fp - fb{k}.xyz) - fb{k}.w;
  if (sbd{k} < 0.0) {{""" + ik_slot_vals(k) + f"""
    vec3 srw = fp - so;
    vec3 srel = vec3(dot(srw, sex), dot(srw, sey), dot(srw, sez));
    vec2 snr = ik_near(srel, sa, sc, {k}.0);
    frd = min(frd, -snr.x / 14.0);
    if (snr.x > fr) {{
      fr = snr.x;
      if (snr.x > fv - @TSURF@) {{
        fsp = vec4(smsk, snr.y);
        fwin = {k + 1}.0;
      }}
    }}
  }} else {{
    frd = min(frd, sbd{k} + 0.03);
  }}
""") for k in range(IK_NS)) + ik_sub("""
  fax = vec4(fv, frd, focc, fwin);
  return max(fv - @TSURF@, fr) - (1.0 - smoothstep(0.9, 2.0, ft)) * 2.0;
}
float ik_b1(vec3 fp, float ft, float f1a, float f1c, float f1k, out vec4 fsp, out vec4 fax) {
""") + IK_DYEBODY + ik_sub("""
  float fr = -1000.0;
  if (f1k > -0.5) {""" + ik_slot_frame("f1a", "f1c") + """
    vec3 srw = fp - so;
    vec3 srel = vec3(dot(srw, sex), dot(srw, sey), dot(srw, sez));
    vec2 snr = ik_near(srel, sa, sc, f1k);
    fr = snr.x;
    if (snr.x > fv - @TSURF@) fsp = vec4(smsk, snr.y);
  }
  fax = vec4(fv, 1000.0, focc, 0.0);
  return max(fv - @TSURF@, fr) - (1.0 - smoothstep(0.9, 2.0, ft)) * 2.0;
}
""")

IK_WARP = IK_COMMON + IK_DROPFN + ik_sub("""
vec3 ik_curl(vec3 cup) {
  vec3 cuq = cup * 0.16 + vec3(q9 * 0.006, q9 * 0.009, -q9 * 0.004);
  float cue = 0.03;
  vec3 cx1 = texture(sampler_noisevol_hq, cuq + vec3(cue, 0.0, 0.0)).xyz;
  vec3 cx0 = texture(sampler_noisevol_hq, cuq - vec3(cue, 0.0, 0.0)).xyz;
  vec3 cy1 = texture(sampler_noisevol_hq, cuq + vec3(0.0, cue, 0.0)).xyz;
  vec3 cy0 = texture(sampler_noisevol_hq, cuq - vec3(0.0, cue, 0.0)).xyz;
  vec3 cz1 = texture(sampler_noisevol_hq, cuq + vec3(0.0, 0.0, cue)).xyz;
  vec3 cz0 = texture(sampler_noisevol_hq, cuq - vec3(0.0, 0.0, cue)).xyz;
  vec3 cdx = cx1 - cx0;
  vec3 cdy = cy1 - cy0;
  vec3 cdz = cz1 - cz0;
  return vec3(cdy.z - cdz.y, cdz.x - cdx.z, cdx.y - cdy.x) * (0.16 / (2.0 * cue));
}
""") + " shader_body {\n" + HEAD + ik_sub("""
  ivec2 ipx = ivec2(gl_FragCoord.xy);
  uvec2 du = uvec2(gl_FragCoord.xy) + uvec2(uint(frame) * 1973u, uint(frame) * 9277u);
  uint dh = du.x * 1664525u + du.y * 1013904223u;
  dh ^= dh >> 16u; dh *= 2246822519u; dh ^= dh >> 13u; dh *= 3266489917u; dh ^= dh >> 16u;
  vec4 h4 = vec4(float(dh & 255u), float((dh >> 8u) & 255u), float((dh >> 16u) & 255u), float((dh >> 24u) & 255u)) / 256.0 + 0.5 / 256.0;
  float vn = ik_vn();
  float vt = ik_vt();
  vec4 hnow = floor(vec4(vn, 141.0, 37.0, 78.0) + 0.5) / 255.0;
  vec4 hold = texelFetch(sampler2D(sampler_pc_main, sampler_pc_main_samp), ivec2(@NS2@, 0), 0);
  vec4 hdif = abs(hold - hnow);
  float vok = step(2.5, frame) * step(max(max(hdif.x, hdif.y), max(hdif.z, hdif.w)), 0.002);
  vec2 cxy = vec2(ipx) - vec2(0.0, 1.0);
  float ctx = floor(cxy.x / vn);
  float cty = floor(cxy.y / vn);
  float ckk = ctx + cty * vt;
  float onc = vn * 0.25;
  float otc = 3.0 + step(10.0, onc) + step(20.0, onc);
  if (ipx.y == 0 && ipx.x < @NS2@) {
    int dslot = ipx.x / 2;
    int dpart = ipx.x - dslot * 2;
    vec4 dold = texelFetch(sampler2D(sampler_pc_main, sampler_pc_main_samp), ipx, 0);
    vec4 dnew = dold;
    if (abs(floor(q8) - float(dslot + 1)) < 0.5) {
      float dk1 = 2.0 + 2.0 * float(dpart);
      float dk2 = dk1 + 1.0;
      float dc1 = 0.0;
      float ds1 = 0.0;
      float dc2 = 0.0;
      float ds2 = 0.0;
      float de2 = 0.0;
      for (int j = 0; j < 64; j++) {
        float dfj = (float(j) + 0.5) / 64.0;
        float dwj = get_wave(dfj);
        float daj = dfj * 6.2831853;
        dc1 += dwj * cos(dk1 * daj);
        ds1 += dwj * sin(dk1 * daj);
        dc2 += dwj * cos(dk2 * daj);
        ds2 += dwj * sin(dk2 * daj);
        de2 += dwj * dwj;
      }
      float drms = sqrt(de2 / 64.0) + 0.0001;
      vec4 dhv = vec4(dc1, ds1, dc2, ds2) / 32.0 / drms * 0.7;
      float dhm = max(length(dhv.xy), length(dhv.zw));
      dhv = dhv / max(dhm, 1.0);
      dnew = dhv * 0.5 + 0.5;
    }
    dnew = mix(vec4(0.5), dnew, step(2.5, frame));
    dnew = floor(dnew * 255.0 + 0.5) / 255.0;
    ret = dnew.xyz;
    ret_alpha = dnew.w;
  } else if (ipx.y == 0 && ipx.x == @NS2@) {
    ret = hnow.xyz;
    ret_alpha = hnow.w;
  } else if (ipx.y >= 1 && cxy.x < vt * vn && cxy.y < vt * vn && ckk < vn && ik_vq() > 0.5) {
    vec3 wp = @BMIN@ + (vec3(cxy.x - ctx * vn, ckk, cxy.y - cty * vn) + 0.5) / vn * @BSIZE@;
    float wkv = (floor(q11) / 100.0 - 4.0) * (1.0 - smoothstep(1.5, 2.3, length(wp)));
    vec3 wka = normalize(vec3(sin(q9 * 0.37), cos(q9 * 0.23), sin(q9 * 0.31 + 1.0)) + 0.001);
    vec3 wv = ik_curl(wp) * fract(q11) + wp * wkv + cross(wka, wp) * wkv;
    vec3 wdep = vec3(0.0);
    float whd = 0.0;
""") + "".join(ik_warp_slot(k) for k in range(IK_NS)) + ik_sub("""
    vec4 wd = ik_vol(wp - wv * q12);
    float wsum = wd.x + wd.y + wd.z;
    vec3 wwl = min(wp - @BMIN@, @BMAX@ - wp);
    float wwall = 1.0 - smoothstep(0.0, 0.5, min(wwl.x, min(wwl.y, wwl.z)));
    wd.xyz = max(wd.xyz * exp(-q12 * (1.0 / IKDECAY + 4.0 * wwall)) - 0.018 * q12, vec3(0.0));
    wd.xyz += (1.0 - wd.xyz) * min(wdep * IKDEP * q12, vec3(1.0));
    float wbeat = ik_bs(q13) * exp(-ik_ba(q13) / 0.07);
    wd.w = max(wd.w * exp(-q12 / 0.45), max(min(whd, 1.0), wbeat * smoothstep(0.3, 0.8, wsum) * 0.35));
    if (vok < 0.5) {
      vec3 pfn = texture(sampler_noisevol_hq, wp * 0.21 + 0.37).xyz - 0.5;
      vec3 pfq = wp + pfn * 1.1;
      vec3 pfd = vec3(0.0);
      for (int j = 0; j < 7; j++) {
        float pfj = float(j);
        vec3 pfc = vec3((ik_hash(pfj * 1.37 + 0.71) - 0.5) * 3.0, (ik_hash(pfj * 2.11 + 3.3) - 0.5) * 3.0, (ik_hash(pfj * 3.17 + 5.9) - 0.5) * 3.0);
        float pfr = 0.5 + 0.45 * ik_hash(pfj * 4.31 + 1.1);
        float pfv = smoothstep(pfr, 0.35 * pfr, length(pfq - pfc)) * 0.62;
        float pfs = mod(pfj, 3.0);
        pfd += pfv * vec3(1.0 - step(0.5, pfs), step(0.5, pfs) * (1.0 - step(1.5, pfs)), step(1.5, pfs));
      }
      wd = vec4(min(pfd, vec3(0.8)), 0.0);
    }
    wd = floor(clamp(wd, 0.0, 1.0) * 255.0 + h4) / 255.0;
    ret = wd.xyz;
    ret_alpha = wd.w;
  } else if (ipx.y >= 1 && cxy.x >= vt * vn && cxy.x < vt * vn + onc * otc && cxy.y < onc * otc && ik_vq() > 0.5) {
    vec2 oxy = cxy - vec2(vt * vn, 0.0);
    float otx = floor(oxy.x / onc);
    float oty = floor(oxy.y / onc);
    float okk = otx + oty * otc;
    float oacc = 0.0;
    if (okk < onc) {
      vec3 obase = vec3(oxy.x - otx * onc, okk, oxy.y - oty * onc) * 4.0 - 2.0;
      for (int oz = 0; oz < 8; oz++) {
        float ofk = clamp(obase.y + float(oz), 0.0, vn - 1.0);
        vec2 ofo = vec2(mod(ofk, vt), floor(ofk / vt)) * vn + vec2(0.0, 1.0);
        for (int oy = 0; oy < 4; oy++) {
          for (int ox = 0; ox < 4; ox++) {
            vec2 ofc = clamp(obase.xz + vec2(float(ox), float(oy)) * 2.0 + 1.0, vec2(0.5), vec2(vn - 0.5));
            vec4 ofv = textureLod(sampler2D(sampler_fc_main, sampler_fc_main_samp), (ofo + ofc) * texsize.zw, 0.0);
            oacc += ofv.x + ofv.y + ofv.z;
          }
        }
      }
    }
    ret = vec3(min(oacc * 40.0, 1.0) * vok, 0.0, 0.0);
    ret_alpha = 0.0;
  } else {
    ret = vec3(0.0);
    ret_alpha = 0.0;
  }
 }""").replace("IKDECAY", "3.0").replace("IKDEP", "1.6")

IK_CAMFN = ik_sub("""
float ik_cam(float caz, float cel, float clr, float cpn, out vec3 cro, out vec3 crt, out vec3 cup, out vec3 cfw) {
  vec3 cdir = vec3(cos(cel) * cos(caz), sin(cel), cos(cel) * sin(caz));
  vec3 cr0 = normalize(vec3(cdir.z, 0.0, -cdir.x));
  vec3 cu0 = cross(cdir, cr0);
  float clh = floor(clr / 4096.0);
  float crl = (clr - clh * 4096.0) / 4095.0 * 2.0 - 1.0;
  float cph = floor(cpn / 4096.0);
  float cpx = cph / 4095.0 * 5.0 - 2.5;
  float cpy = (cpn - cph * 4096.0) / 4095.0 * 5.0 - 2.5;
  vec3 cpos = cdir * @CR@;
  vec3 cfd = normalize(cr0 * cpx + cu0 * cpy - cpos);
  vec3 cr1 = normalize(vec3(-cfd.z, 0.0, cfd.x));
  vec3 cu1 = cross(cr1, cfd);
  vec3 cr2 = cr1 * cos(crl) + cu1 * sin(crl);
  cro = cpos;
  cfw = cfd;
  crt = cr2;
  cup = cross(cr2, cfd);
  return 0.3 + clh / 4095.0 * 0.9;
}
""")

IK_COMP = IK_COMMON + IK_NEARFN + IK_CAMFN + " shader_body {\n" + HEAD + ik_sub("""
  vec2 p = (uv - 0.5) * s;
  uvec2 du = uvec2(uv * texsize.xy);
  uint dh = du.x * 1664525u + du.y * 1013904223u;
  dh ^= dh >> 16u; dh *= 2246822519u; dh ^= dh >> 13u; dh *= 3266489917u; dh ^= dh >> 16u;
  float h01 = fract(float(dh & 65535u) / 65536.0 + mod(frame, 64.0) * 0.618034);
  vec3 ro = vec3(0.0);
  vec3 fw = vec3(0.0);
  vec3 rt = vec3(0.0);
  vec3 up = vec3(0.0);
  float vlens = ik_cam(q1, q2, q3, q15, ro, rt, up, fw);
  vec3 rd = normalize(fw + (p.x * rt + p.y * up) * vlens);
  float mood = mod(floor(q10), 4.0);
  vec3 L = normalize(mix(mix(vec3(0.6, 0.75, -0.3), vec3(0.2, 1.0, -0.15), step(0.5, mood)), vec3(0.15, 1.0, 0.3), step(1.5, mood)));
  float mkey = mix(1.0, 0.8, step(1.5, mood));
  float mglow = mix(1.0, 1.8, step(1.5, mood));
  float mshaft = mix(mix(0.3, 1.0, step(0.5, mood)), 0.25, step(1.5, mood));
  vec3 Lb = normalize(-fw + up * 0.45 + rt * 0.25);
  float cth = dot(rd, L);
  float hgb = 0.75 / pow(1.25 - dot(rd, Lb), 1.5) * 0.25;
  vec3 keyc = mix(NOKKVI_TEXT, NOKKVI_WARM, 0.15) * mkey;
  vec3 backc = mix(NOKKVI_HIGHLIGHT, NOKKVI_TEXT, 0.3);
  vec3 fillc = mix(NOKKVI_SURFACE, NOKKVI_HIGHLIGHT, 0.25);
  vec3 glowc = mix(NOKKVI_WARM, NOKKVI_HIGHLIGHT, 0.3);
""") + ramp("sc0", "fract(q10)") + ramp("sc1", "fract(q10 + 0.333)") + ramp("sc2", "fract(q10 + 0.667)") + ik_sub("""
  float scl0 = dot(sc0, vec3(0.2126, 0.7152, 0.0722));
  float scl1 = dot(sc1, vec3(0.2126, 0.7152, 0.0722));
  float scl2 = dot(sc2, vec3(0.2126, 0.7152, 0.0722));
  sc0 = clamp(scl0 * 0.92 + (sc0 - scl0) * clamp(0.5 / max(max(sc0.x, max(sc0.y, sc0.z)) - min(sc0.x, min(sc0.y, sc0.z)), 0.05), 1.4, 3.4), 0.0, 1.0);
  sc1 = clamp(scl1 * 0.92 + (sc1 - scl1) * clamp(0.5 / max(max(sc1.x, max(sc1.y, sc1.z)) - min(sc1.x, min(sc1.y, sc1.z)), 0.05), 1.4, 3.4), 0.0, 1.0);
  sc2 = clamp(scl2 * 0.92 + (sc2 - scl2) * clamp(0.5 / max(max(sc2.x, max(sc2.y, sc2.z)) - min(sc2.x, min(sc2.y, sc2.z)), 0.05), 1.4, 3.4), 0.0, 1.0);
  float yup = clamp(rd.y * 0.5 + 0.5, 0.0, 1.0);
  vec3 wdeep = mix(NOKKVI_BG * 0.35, mix(NOKKVI_BG, NOKKVI_ACCENT, 0.12) * 0.5, 0.5);
  vec3 wtop = mix(mix(NOKKVI_BG, NOKKVI_SURFACE, 0.55), NOKKVI_ACCENT, 0.06);
  vec3 water = mix(wdeep, wtop, smoothstep(0.2, 1.0, yup));
  water += mix(NOKKVI_SURFACE, NOKKVI_ACCENT, 0.3) * 0.25 * pow(max(dot(rd, Lb), 0.0), 2.5);
  water += wtop * 0.3 * pow(yup, 4.0);
  vec3 bmn = @RMIN@;
  vec3 bmx = @RMAX@;
  vec3 bi0 = (bmn - ro) / rd;
  vec3 bi1 = (bmx - ro) / rd;
  vec3 bnr = min(bi0, bi1);
  vec3 bfr = max(bi0, bi1);
  float tb0 = max(max(max(bnr.x, bnr.y), bnr.z), 0.2);
  float tb1 = min(min(bfr.x, bfr.y), bfr.z);
  vec3 trans = vec3(1.0);
  vec3 acc = vec3(0.0);
  float tmed = 40.0;
  float vnc = ik_vn() * 0.25;
  float hit = 0.0;
  float tsn = 1000.0;
  float tsx = -1000.0;
  float hk = -1.0;
  vec4 hs0 = vec4(0.0);
""") + "".join(ik_sub(f"""
  vec4 sb{k} = vec4(0.0, 1000.0, 0.0, 0.0);
  {{""" + ik_slot_vals(k) + f"""
    if (sa < @NF1@) {{
      float sf0 = 0.7 * log(1.0 + sa / 0.25) - @KOFF@;
      sb{k} = vec4(so - sey * (sf0 * ssz), ssz * (0.47 * smoothstep(0.02, 0.38, sa) * (1.0 + 0.09 * sa) + 0.3) + 0.08);
      vec3 sro = ro - sb{k}.xyz;
      float srb = dot(sro, rd);
      float srd = srb * srb - dot(sro, sro) + sb{k}.w * sb{k}.w;
      if (srd > 0.0) {{
        tsn = min(tsn, -srb - sqrt(srd));
        tsx = max(tsx, -srb + sqrt(srd));
      }}
    }}
  }}
""") for k in range(IK_NS)) + ik_sub("""
  float tt = tb0 + @DTS@ * h01;
  float tprev = tt;
  float tnr = tsn;
  vec4 bsp = vec4(0.0);
  vec4 bax = vec4(0.0);
  if (tb1 > tb0) {
    for (int i = 0; i < @STEPS@; i++) {
      vec3 pos = ro + rd * tt;
      float bfv = 0.0;
      if (tt >= tnr && tt <= tsx) {
        bfv = ik_bf(pos, tt, IKSB, bsp, bax);
        tnr = tt + max(bax.y, 0.0) * 0.85;
      } else {
        bfv = ik_bd(pos, tt, bsp, bax);
      }
      float tnl = mix(tnr, 1000.0, step(tsx, tt));
      if (bfv > 0.0) {
        hit = 1.0;
        hk = bax.w - 1.0;
        hs0 = bsp;
        break;
      }
      float veil = smoothstep(0.0, 0.08, bax.x) * smoothstep(0.9, 2.0, tt);
      float mstep = @DTS@ * mix(0.5, 1.0, smoothstep(0.03, 0.15, -bfv)) * mix(1.0, 2.0, step(bax.x, -0.99));
      if (bax.z < 0.5) {
        vec3 vgc = (pos - @BMIN@) / @BSIZE@ * vnc;
        vec3 vnx = (floor(vgc) + step(vec3(0.0), rd) - vgc) / vnc * @BSIZE@ / rd;
        mstep = max(mstep, min(vnx.x, min(vnx.y, vnx.z)) + 0.004 + @DTS@ * fract(h01 + float(i) * 0.618034));
      }
      mstep = max(min(mstep, max(tnl - tt, @DTS@ * 0.4)), 0.012);
      if (veil > 0.002) {
        vec3 vcm = sc0 * bsp.x + sc1 * bsp.y + sc2 * bsp.z;
        vec3 vsig = veil * 9.0 * (vcm * 0.7 + 0.03);
        vec3 vsgt = vsig + veil * 9.0 * (1.0 - vcm) * 0.8;
        vec3 vlin = keyc * (0.55 + 0.6 * max(cth, 0.0)) + backc * hgb * 2.6 + fillc * 0.4;
        vec3 vst = exp(-vsgt * mstep);
        acc += trans * vsig * vlin * (1.0 - vst) / max(vsgt, vec3(0.0001));
        trans *= vst;
        if (tmed > 39.0 && dot(trans, vec3(0.333)) < 0.55) tmed = tt;
      }
      tprev = tt;
      tt += mstep;
      if (tt > tb1 || max(trans.x, max(trans.y, trans.z)) < 0.03) break;
    }
  }
  vec3 col = water;
  float thit = 40.0;
  if (hit > 0.5) {
    float hka = q17;
    float hkc = q25;
IKHKSEL
    float bta = tprev;
    float btb = tt;
    for (int i = 0; i < 4; i++) {
      float btm = 0.5 * (bta + btb);
      vec3 bpm = ro + rd * btm;
      float bvm = ik_b1(bpm, btm, hka, hkc, hk, bsp, bax);
      if (bvm > 0.0) { btb = btm; } else { bta = btm; }
    }
    thit = btb;
    vec3 hp = ro + rd * thit;
    vec4 hsp = vec4(0.0);
    vec3 hpx = hp + vec3(0.035, 0.0, 0.0);
    vec3 hpy = hp + vec3(0.0, 0.035, 0.0);
    vec3 hpz = hp + vec3(0.0, 0.0, 0.035);
    vec3 hpi = hp + rd * 0.13;
    float hb0 = 0.0;
    float hbx = 0.0;
    float hby = 0.0;
    float hbz = 0.0;
    float hd1 = 0.0;
    if (hk > -0.5) {
      hb0 = ik_b1(hp, thit, hka, hkc, hk, hsp, bax);
      hbx = ik_b1(hpx, thit, hka, hkc, hk, bsp, bax);
      hby = ik_b1(hpy, thit, hka, hkc, hk, bsp, bax);
      hbz = ik_b1(hpz, thit, hka, hkc, hk, bsp, bax);
      hd1 = ik_b1(hpi, thit, hka, hkc, hk, bsp, bax);
    } else {
      hb0 = ik_bd(hp, thit, hsp, bax);
      hbx = ik_bd(hpx, thit, bsp, bax);
      hby = ik_bd(hpy, thit, bsp, bax);
      hbz = ik_bd(hpz, thit, bsp, bax);
      hd1 = ik_bd(hpi, thit, bsp, bax);
    }
    hsp = mix(hs0, hsp, step(0.01, hsp.x + hsp.y + hsp.z));
    vec3 hn = -normalize(vec3(hbx - hb0, hby - hb0, hbz - hb0) + vec3(0.00001));
    vec3 hrn = texture(sampler_noisevol_hq, hp * 1.9 + vec3(0.0, time * 0.35, 0.0)).xyz - 0.5;
    hn = normalize(hn + hrn * (floor(q10 / 4.0) * 0.009));
    hn = normalize(hn - rd * max(dot(hn, rd), 0.0) * 1.05);
    float hdep = smoothstep(-0.05, 0.4, hd1);
    float hws = max(hsp.x + hsp.y + hsp.z, 0.001);
    vec3 hcm = (sc0 * hsp.x + sc1 * hsp.y + sc2 * hsp.z) / hws;
    vec3 hpl = hp + hn * 0.05 + L * 0.28;
    vec4 hvl = ik_vol(hpl);
    float hsh = exp(-(hvl.x + hvl.y + hvl.z) * 4.0);
    float hndl = dot(hn, L);
    float hwrap = clamp(hndl * 0.55 + 0.45, 0.0, 1.0);
    vec2 hcz = (hp.xz - L.xz / max(L.y, 0.2) * (hp.y - 3.0)) * 0.11 + vec2(q9 * 0.01, q9 * 0.007);
    float hcau = mix(1.0, 0.45 + 1.3 * smoothstep(0.42, 0.72, texture(sampler_noise_hq, hcz).x), smoothstep(-0.1, 0.6, hn.y));
    vec3 hbody = mix(hcm, hcm * hcm * 0.8, 0.15 + 0.6 * hdep);
    vec3 hlit = hbody * (keyc * hwrap * (0.35 + 0.65 * hsh) * hcau * 1.25 + fillc * 0.5 + 0.2);
    hlit += hcm * backc * (1.0 - hdep) * (0.25 + 2.2 * hgb);
    vec3 hh = normalize(L - rd);
    float hspec = pow(max(dot(hn, hh), 0.0), 140.0) * 1.8 + pow(max(dot(hn, hh), 0.0), 24.0) * 0.12;
    hlit += keyc * hspec * hsh * step(0.0, hndl);
    float hfre = pow(1.0 - max(dot(hn, -rd), 0.0), 4.0);
    hlit = mix(hlit, mix(wdeep, wtop * 1.6, hn.y * 0.5 + 0.5), hfre * 0.55);
    hlit += glowc * hsp.w * mglow * 0.3;
    float hrr = length(hp);
    float hwa = ik_ba(q13);
    float hwb = ik_ba(q14);
    float hw1 = (hrr - 5.0 * hwa) / 0.45;
    float hw2 = (hrr - 5.0 * hwb) / 0.45;
    hlit += glowc * (ik_bs(q13) * exp(-hw1 * hw1 - hwa / 0.45) + ik_bs(q14) * exp(-hw2 * hw2 - hwb / 0.45)) * 0.7;
    float hfog = exp(-max(thit - 2.2, 0.0) * 0.11);
    col = mix(water, hlit, hfog);
    if (tmed > 39.0) tmed = thit;
  }
  float shaft = 0.0;
  for (int k = 0; k < 4; k++) {
    float sht = 1.0 + (float(k) + h01) * 1.7;
    vec3 shp = ro + rd * sht;
    vec2 shc = (shp.xz - L.xz / max(L.y, 0.2) * (shp.y - 3.0)) * 0.03 + vec2(q9 * 0.003, q9 * 0.002);
    float shn = texture(sampler_noise_hq, shc).x;
    shaft += smoothstep(0.5, 0.85, shn) * exp(min(shp.y - 3.0, 0.0) * 0.25) * step(sht, thit);
  }
  col += mix(NOKKVI_HIGHLIGHT, NOKKVI_TEXT, 0.4) * shaft * 0.06 * mshaft;
  float mote = 0.0;
  float mpx = vlens / min(texsize.x, texsize.y);
  for (int k = 0; k < 4; k++) {
    float mt = 1.2 + float(k) * 0.5;
    vec3 mp = (ro + rd * mt) / 0.5 + vec3(0.0, q9 * 0.05, 0.0);
    vec3 mi = floor(mp);
    vec3 mh3 = fract(mi * vec3(0.1031, 0.1030, 0.0973));
    mh3 += dot(mh3, mh3.yxz + 33.33);
    vec3 mh = fract((mh3.xxy + mh3.yxx) * mh3.zyx);
    vec3 mw = (mi + 0.2 + 0.6 * mh - vec3(0.0, q9 * 0.05, 0.0)) * 0.5;
    vec3 mrel = mw - ro;
    float mtt = dot(mrel, rd);
    float mdd = length(mrel - rd * mtt);
    float mrr = mpx * mtt * (1.0 + 1.6 * mh.x);
    mote += smoothstep(mrr, 0.3 * mrr, mdd) * step(0.72, mh.z) * step(mtt, thit) * (0.5 + 0.5 * mh.y);
  }
  col += mix(NOKKVI_TEXT, NOKKVI_HIGHLIGHT, 0.5) * mote * 0.3;
  col = col * trans + acc;
  col *= 0.86 + 0.14 * smoothstep(1.25, 0.35, length(p));
  float lm = dot(col, vec3(0.2126, 0.7152, 0.0722));
  vec3 tm = col * (1.0 - exp(-lm * 1.5)) / max(lm, 0.0001);
  float over = max(tm.x, max(tm.y, tm.z));
  tm = tm / max(over, 1.0);
  tm = mix(tm, vec3(1.0), clamp((over - 1.0) * 0.4, 0.0, 0.6));
  vec3 rop = vec3(0.0);
  vec3 fwp = vec3(0.0);
  vec3 rtp = vec3(0.0);
  vec3 upp = vec3(0.0);
  float plens = ik_cam(q4, q5, q6, q16, rop, rtp, upp, fwp);
  vec3 pvw = ro + rd * min(tmed, @CR@ + 1.0) - rop;
  vec2 puv = vec2(dot(pvw, rtp), dot(pvw, upp)) / (max(dot(pvw, fwp), 0.05) * max(plens, 0.2)) / s + 0.5;
  float hval = step(0.0, puv.x) * step(puv.x, 1.0) * step(0.0, puv.y) * step(puv.y, 1.0);
  vec4 hist = textureLod(sampler2D(sampler_prev_comp, sampler_prev_comp_samp), puv, 0.0);
  vec3 blm = textureLod(sampler2D(sampler_prev_comp, sampler_prev_comp_samp), uv, 4.5).xyz;
  tm += max(blm - 0.45, 0.0) * 0.2 * hist.w;
  vec3 res = mix(tm, clamp(hist.xyz, tm - 0.12, tm + 0.12), max(-q7, 0.0) * hist.w * hval * step(2.5, frame));
  float dedge = smoothstep(0.0, 0.04, puv.x) * smoothstep(0.0, 0.04, 1.0 - puv.x) * smoothstep(0.0, 0.04, puv.y) * smoothstep(0.0, 0.04, 1.0 - puv.y);
  res = mix(res, hist.xyz, max(q7, 0.0) * hist.w * dedge * step(2.5, frame));
  res += (h01 - 0.5) / 255.0;
  ret = res;
 }""").replace("IKSB, ", "".join(f"sb{k}, " for k in range(IK_NS))).replace(
    "IKHKSEL", "\n".join(f"    hka = mix(hka, q{17 + k}, step({k - 0.5}, hk));\n    hkc = mix(hkc, q{25 + k}, step({k - 0.5}, hk));" for k in range(1, IK_NS)))

def ik_code(nn, cq, sz, cy, tb, di, cx, cz):
    """A drop's code: child count - 3, species, size, aim point y, turbulence,
    travel direction (0..63), aim point x and z."""
    return nn + 8 * cq + 64 * sz + 256 * cy + 1024 * tb + 4096 * di + 262144 * cx + 1048576 * cz

IK_AGE0 = [1.2, 3.3, 5.4] + [99] * (IK_NS - 3)
IK_CODE0 = [ik_code(3, 0, 1, 2, 1, 5, 1, 2), ik_code(1, 1, 0, 1, 1, 41, 2, 1), ik_code(4, 2, 1, 2, 2, 22, 1, 1)] + [0] * (IK_NS - 3)
IK_INIT = (BEATS_INIT + " inkt = 0; since = 2; bavg = 1; mavg = 1; tavg = 1; lastcq = int(rand(3)); lastdi = int(rand(64));"
           " loud = 1; loud_m = 1; rate = 1; rate_m = 1; turb = 0.12; turb_m = 0.12; mood = 0; spb = 0.1; spawnf = 0;"
           " az = rand(628) / 100; el = 0.15; lens = 0.62; az0 = az; el0 = el; lens0 = lens; az1 = az; el1 = el; lens1 = lens;"
           " rol = 0; rol0 = 0; rol1 = 0; rdir = 1; kvel = 0; kpos = 0; bsw = 0; trp = 0; lpx = 0; lpy = 0; lpx0 = 0; lpy0 = 0; lpx1 = 0; lpy1 = 0;"
           " mvt = 9; mvd = 0.3; odir = 1; tsb = 0.5; ibi = 0.5; smv = 0; nmv = 0; pvalid = 0; "
           + " ".join(f"ika{k} = {IK_AGE0[k]}; ikc{k} = {IK_CODE0[k]};" for k in range(IK_NS)))
def ik_eel_drop(k):
    """Where drop k's ring will be 0.7 s from now (the same model as
    ik_slot_frame), and its weight in the camera's framing."""
    return (f"kc = ikc{k}; ka = min(ika{k} + 0.7, 6.5); kw{k} = min(max((6.5 - ika{k}) / 1.5, 0), 1);\n"
            f"kdi = int(kc / 4096) % 64; kdh = (2 * kdi + 1) / 64 - 1; kdr = sqrt(max(1 - kdh * kdh, 0));\n"
            f"ksz = (0.8 + 0.15 * (int(kc / 64) % 4)) * {IK_SC};\n"
            f"ktr = (0.7 * log(1 + ka / 0.25) - {IK_KOFF}) * ksz - min({IK_LEAD} * ksz, {IK_LEADMAX});\n"
            f"kx{k} = ((int(kc / 262144) % 4) - 1.5) * {IK_CSTEP} + kdr * cos(kdi * 2.3999632) * ktr;\n"
            f"ky{k} = ((int(kc / 256) % 4) - 1.5) * {IK_CSTEP} - kdh * ktr;\n"
            f"kz{k} = ((int(kc / 1048576) % 4) - 1.5) * {IK_CSTEP} + kdr * sin(kdi * 2.3999632) * ktr;\n"
            f"wsm = wsm + kw{k}; cgx = cgx + kw{k} * kx{k}; cgy = cgy + kw{k} * ky{k}; cgz = cgz + kw{k} * kz{k};\n")

IK_TARGETS = """odir = if(gom * above(rand(100), 75), -odir, odir);
az = if(gom * above(abs(az), 31.4159265), az - sign(az) * 62.8318531, az);
ur1 = rand(1000) / 1000 + bass_att * 2.9 + mid_att * 1.7; ur1 = (ur1 - int(ur1)) * 2 - 1;
ur2 = rand(1000) / 1000 + treb_att * 3.1 + bass_att * 1.3; ur2 = ur2 - int(ur2);
az0 = if(gom, az, az0); el0 = if(gom, el, el0); lens0 = if(gom, lens, lens0); rol0 = if(gom, rol, rol0); lpx0 = if(gom, lpx, lpx0); lpy0 = if(gom, lpy, lpy0);
rdir = if(gom, -rdir, rdir);
rol1 = if(gom, if(equal(nmv % 4, 0), 0, rdir * (0.15 + 0.4 * kk)), rol1);
az1 = if(gom, az + odir * (0.3 + 0.55 * kk + 0.8 * big), az1);
el1 = if(gom, if(big, ur1, min(max(el * 0.8 + ur1 * (0.25 + 0.4 * kk), -1.05), 1.05)), el1);
"""
IK_FRAME = "dt = min(1 / max(fps, 1), 0.1);\n" + BEATS + f"""tsb = tsb + dt;
ibi = if(trig, ibi + (min(tsb, 1.2) - ibi) * 0.4, ibi);
tsb = if(trig, 0, tsb);
en = min((1.2 * bass_att + mid_att + 0.8 * treb_att) / 3, 2);
""" + ease("loud", "en", "1.0") + ease("rate", f"{IK_RATE} * (0.65 + 0.7 * min(max(loud - 0.5, 0), 1))", "1.5") + f"""
mvt = mvt + dt; smv = smv + dt;
gom = max(trig, above(smv, 4)) * above(mvt, mvd) * above(smv, 0.22);
kk = if(trig, min(max((bs1 - 0.8) / 0.8, 0), 1), 0.25);
nmv = nmv + gom;
big = equal(nmv % 8, 0);
mvd = if(gom, if(trig, min(max(0.42 * ibi, 0.12), 0.36), 0.36), mvd);
mvt = if(gom, 0, mvt); smv = if(gom, 0, smv);
mvx = min(mvt / mvd, 1);
mvu = 1 - (1 - mvx) * (1 - mvx) * (1 - mvx);
frz = below(mvt, mvd);
mvs = min(max((mvx - 0.35) / 0.65, 0), 1); mvs = mvs * mvs * (3 - 2 * mvs);
drg = frz * 0.85 * (1 - mvs);
taaw = 0.8 * min(max((mvt - mvd) / 0.15, 0), 1);
dts = dt * rate * (1 - frz);
kdt = min(dts, 0.03);
bswg = min(max((bass - 0.6) / 1.9, 0), 1);
bsw = bsw + (bswg - bsw) * (1 - exp(-dt / if(above(bswg, bsw), 0.03, 0.22)));
trp = trp + (min(max((treb - 0.4) / 1.5, 0), 1) - trp) * (1 - exp(-dt / 0.07));
kvel = if(trig, min(kvel + 2.0 + 1.2 * kk, 3.2), kvel);
kvel = kvel + (-324 * kpos - 12.6 * kvel) * kdt;
kpos = kpos + kvel * kdt;
inkt = inkt + dts;
""" + "".join(f"ika{k} = min(ika{k} + dts, 99);\n" for k in range(IK_NS)) + f"""
vis = equal(frame, 3);
uv1 = rand(1000) / 1000 + bass_att * 3.7 + treb_att * 1.3; uv1 = uv1 - int(uv1);
mood = if(vis, int(uv1 * 3), mood);
spb = if(vis, uv1 * 7.3 - int(uv1 * 7.3), spb);
since = since + dts;
gap = 0.6 + 0.3 * (1 - min(max(loud - 0.7, 0), 1));
old = 0; oa = ika0;
""" + "".join(f"old = if(above(ika{k}, oa), {k}, old); oa = max(oa, ika{k});\n" for k in range(1, IK_NS)) + f"""go = max(trig * above(since, gap), above(since, 3)) * above(oa, {IK_LIFE - 0.8});
bavg = bavg + (bass_att - bavg) * (1 - exp(-dt / 8)); mavg = mavg + (mid_att - mavg) * (1 - exp(-dt / 8)); tavg = tavg + (treb_att - tavg) * (1 - exp(-dt / 8));
nb = bass_att / max(bavg, 0.01); nm = mid_att / max(mavg, 0.01); nt = treb_att / max(tavg, 0.01);
""" + ease("turb", "0.1 + 0.15 * min(max(nm - 0.7, 0), 1.5)", "1.0") + f"""cent = (0.5 * nm + nt) / max(nb + nm + nt, 0.01);
nn = min(max(int(5 + (cent - 0.5) * 14 + rand(2) - 0.5), 3), 7) - 3;
cq = (lastcq + 1 + above(rand(100), 75)) % 3;
lastcq = if(go, cq, lastcq);
sz = min(max(int((bs1 - 0.7) * 4.5), 0), 3);
tb = min(max(int((nm - 0.6) * 3.5), 0), 3);
uv2 = rand(1000) / 1000 + mid_att * 5.3 + bass * 1.7; uv2 = uv2 - int(uv2);
di = (lastdi + 13 + int(uv2 * 38)) % 64;
lastdi = if(go, di, lastdi);
code = nn + 8 * cq + 64 * sz + 256 * int(rand(4)) + 1024 * tb + 4096 * di + 262144 * int(rand(4)) + 1048576 * int(rand(4));
""" + "".join(f"ika{k} = if(go * equal(old, {k}), 0, ika{k}); ikc{k} = if(go * equal(old, {k}), code, ikc{k});\n" for k in range(IK_NS)) + f"""since = if(go, 0, since);
spawnf = if(go, old + 1, 0);
wsm = 0; cgx = 0; cgy = 0; cgz = 0;
""" + "".join(ik_eel_drop(k) for k in range(IK_NS)) + """cgx = cgx / max(wsm, 0.01); cgy = cgy / max(wsm, 0.01); cgz = cgz / max(wsm, 0.01);
sig = 0;
""" + "".join(f"sig = sig + kw{k} * (sqr(kx{k} - cgx) + sqr(ky{k} - cgy) + sqr(kz{k} - cgz));\n" for k in range(IK_NS)) + """sig = if(above(wsm, 0.01), sqrt(sig / max(wsm, 0.01)), 1);
""" + IK_TARGETS + f"""lpx1 = if(gom, 0.85 * min(max(cgx * sin(az1) - cgz * cos(az1), -1.6), 1.6), lpx1);
lpy1 = if(gom, 0.85 * min(max(-cgx * sin(el1) * cos(az1) + cgy * cos(el1) - cgz * sin(el1) * sin(az1), -1.6), 1.6), lpy1);
lens1 = if(gom, min(max(2 * min(max(0.9 * sig + 0.75, 1.4), 2.0) * (0.88 + 0.24 * ur2) / {IK_CR}, 0.36), 0.85), lens1);
az = az0 + (az1 - az0) * mvu; el = el0 + (el1 - el0) * mvu; lens = lens0 + (lens1 - lens0) * mvu; rol = rol0 + (rol1 - rol0) * mvu;
lpx = lpx0 + (lpx1 - lpx0) * mvu; lpy = lpy0 + (lpy1 - lpy0) * mvu;
lqi = int((min(max(lens, 0.3), 1.2) - 0.3) / 0.9 * 4095 + 0.5) * 4096 + int((min(max(rol, -1), 1) + 1) / 2 * 4095 + 0.5);
pqi = int((min(max(lpx, -2.5), 2.5) + 2.5) / 5 * 4095 + 0.5) * 4096 + int((min(max(lpy, -2.5), 2.5) + 2.5) / 5 * 4095 + 0.5);
paz = if(pvalid, paz, az); pel = if(pvalid, pel, el); plq = if(pvalid, plq, lqi); ppq = if(pvalid, ppq, pqi);
q1 = az; q2 = el; q3 = lqi; q4 = paz; q5 = pel; q6 = plq;
q15 = pqi; q16 = ppq;
paz = az; pel = el; plq = lqi; ppq = pqi; pvalid = 1;
q7 = if(frz, drg, -taaw); q8 = spawnf + min(bsw, 0.99) * 0.999; q9 = inkt; q10 = mood + 4 * int(trp * 100) + spb * 0.999; q11 = int((min(max(kvel, -4), 4) + 4) * 100 + 0.5) + min(turb, 0.99); q12 = dts;
q13 = min(ba1, 15.9) + 16 * int(bs1 * 100); q14 = min(ba2, 15.9) + 16 * int(bs2 * 100);
""" + "".join(f"q{17 + k} = ika{k}; q{25 + k} = ikc{k};\n" for k in range(IK_NS))
presets["nokkvi - living ink"] = preset({"decay": 0.0, "wave_a": 0.0, "zoom": 1.0}, IK_WARP, ik_lod0(IK_COMP),
                                        init=IK_INIT, frame=IK_FRAME)

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
# The river is the music: the warp keeps a spectrum history (FJ_HN rows of the
# FJ_NB band levels, the follower over its band's own slow mean, one row per
# 1/FJ_HRATE s committed on the clock, so it flows the same at any frame rate)
# and the water shows it in channel coordinates: across the river the bands
# mirror out from the centre (bass in the middle lanes, treble toward the
# cliffs), along it the history races ahead of the camera at FJ_VFLOW units/s
# (what you hear rises under the camera and flows down the fjord, taking every
# bend). A slow liquid warp melts the lanes. The level is real, glossy
# relief: the water surface is displaced by it (up to FJ_WAMP, and never more
# than 0.6 of the camera's height, so crests stay clear of the skimming
# camera; flat at the cliff margins, so the shore holds, and beyond 45 units),
# found by a short march between the crest plane and y = 0 plus bisection
# (on the bilinear history, one read a step; the shading uses the B-spline); it
# bends the reflections, neon contour lines trace it (each level its own
# colour from the theme ramp, drifting), and a body glow fills between them;
# the glow is strongest at night and fades in the shallows. Each kick is
# stamped into the history too (the third channel, a 0.2 s envelope from q1),
# so the beat flows downstream with its music: there the lines flare and their
# glow widens, the water body lights up, plankton in the water catches the
# light, and the river throws light on the cliff feet (sampled from the
# history at the rock's own distance). Beat never touches terrain.
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
# Cache: at most FJ_CWMAX x FJ_CHMAX texels (and FJ_SR rows fewer than the
# texture), x = depth, y = u = x - river
# path(z) (uniform; past |u| = FJ_U - 1.5 the comp adds analytic ridges that
# keep rising, fj_hx, since the clamped edge row stretched sideways forever
# showed as a dead-straight plateau edge over the far peaks). Depth texels are
# anchored to the world: spacing 256/N (N
# texels per 256 units, so the grid survives the wrap) from an origin snapped
# to that spacing, so a world point is sampled identically every frame (a
# camera-relative grid made small peaks and crests crawl). Each texel is the
# full height: water that swells and narrows along the path with hashed
# headlands; a cliff profile that plunges below the water and rises steeply to
# a convex shoulder, with a hashed bench part-way up; buttresses and bulges from
# ridged noise about as fine up the face as along it (vertical-only relief read
# as organ pipes); a rolling plateau with far peaks; gully erosion only on
# shoulders and slopes (on a cliff its small height steps became flutes).
# The relief pushes the wall out by an amount that depends on the height, and
# a heightfield cannot overhang: the wall's position was once estimated from
# the unrelieved height, so where the relief changed faster than the wall
# climbs the wall folded back on itself and baked knife-thin fins into the
# cache that shimmered like paper waves in flight. Now each depth column's wall
# is sampled at FJ_WK + 1 heights (even in height, true heights), made monotone
# by averaging the fill envelope (a bulge extends down) and the carve envelope
# (a bulge is trimmed back), and each texel inverts that monotone wall by binary
# search: no folds by construction. The polylines (one per column and side,
# 16-bit, (W + 1) / 4) sit in FJ_SR rows above the cache and are written each
# frame; the bake reads last frame's, shifted by how far the snapped origin
# moved (the camera block keeps that origin's index and the texture size). The
# first frames, a resized texture and the newest column fall back to the old
# direct estimate for that one frame.
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
# view, the music relief, Fresnel, a reflection march with shadows, depth tint, a foam
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
# spurs, hashes mod 64 at multiples of 0.25 per unit, snow line, the river's
# liquid warp, clouds via q25, flow mod 640), so the camera's wrap at 256 is
# seamless.
# Dream (after the mandelbox explorer's soft, echoing look): the comp also reads
# its own previous frame (sampler_prev_comp, with averaged blur levels) and
# blends it in after the tone map. The warp keeps the camera as 24-bit values
# (three channels; texels 8-10 hold the texture size and the cache origin's
# index for the wall strip, texel 11 the last committed history row) in a
# small block under the bands: row FJ_DH this frame's,
# row FJ_DH + 1 a copy of the last frame's, which the comp reads to reproject:
# each pixel's world point (rock hit; on water the reflection's mirror image
# along the view ray, its inverse depth weighted by Fresnel so a faint sky
# reflection stays near the surface; sky by direction) is projected with last
# frame's camera and the history is sampled there. The rock takes no sharp echo
# (resampling it every frame only softens detail in drifting bands); what moves
# or depends on the view on the water and in the sky (the river's glowing
# relief, ripples, aurora) leaves a short echo (q17). The blur taps read one level finer
# than their spread, so a moving glow slides instead of stepping. On top, a
# depth of field focused on the water just ahead (q3, aperture q24;
# the sky's blur capped at FJ_SKYCOC so stars and aurora survive) pulls each
# pixel toward the blurred history, never less than FJ_HALO: a soft halo
# everywhere and a glowing, softened distance. The history weight is zero for
# the first frames, behind the old camera and where the history has no alpha
# (the engine clears it on a resize), and fades out over the last 4% toward the
# screen edge (a hard cut left a seam). Every blend is convex, so the loop
# gain stays below 1; a 1/255 dither keeps the 8-bit history from sticking.
# q map: q1 river beat envelope, q3 focus distance, q17 echo, q24 aperture;
# q4 camera z (mod 256), q7/q8 camera x/y, q9 night,
# q10-q12 forward, q13 roll, q14 lens, q15/q16 key light (sun, or moon at
# night) azimuth/elevation, q19 liquid flow, q20 loudness, q21 treble shimmer,
# q22 rock tint, q25 cloud drift, q27 aurora strength; q2, q6, q18, q23, q26,
# q28-q32 free (PULSE writes q3 and q5 for SPEED; q3 is then reset to focus).
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
FJ_CAMN = 12
FJ_HR0 = FJ_DH + 2
FJ_HN = 240
FJ_HRATE = 60
FJ_VFLOW = 12.0
FJ_WAMP = 0.18
FJ_LCONT = 8.0
FJ_LDAY = 0.4
FJ_LFLARE = 1.8
FJ_LEXPO = 0.45
FJ_LPLANK = 1.4
FJ_LSPILL = 0.9
FJ_WK = 32
FJ_WKLOG = 5
FJ_SR = 2 * (FJ_WK + 1)
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
float fj_dec24(vec3 d24) {{
  vec3 db = floor(d24 * 255.0 + 0.5);
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
  return clamp(texsize.y - {FJ_SR}.0, 8.0, {FJ_CHMAX}.0);
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
float fj_prof(float pa, float pcl, float prr, float pbz, float pbw, float pn) {{
  float px = clamp(pa / prr, -0.4, 1.0);
  float pxlo = px * (pbz + 0.4 * pbw) / pbz;
  float pxmid = pbz + 0.4 * pbw + (px - pbz) * 0.6;
  float pxb = mix(pxlo, mix(pxmid, px, step(pbz + pbw, px)), step(pbz, px));
  float ph = max(pcl * (1.0 - pow(1.0 - pxb, 2.4)), -0.9);
  float ppl = max(pa - prr, 0.0);
  float ppw = smoothstep(0.0, 2.5, ppl);
  ph += ppw * ((pn - 0.35) * 1.2 + 0.06 * ppl);
  ph += smoothstep(2.0, 7.0, ppl) * max(pn - 0.3, 0.0) * 2.5;
  return ph;
}}
float fj_relief(float rzw, float ry, float rsd) {{
  vec2 rq1 = vec2(rzw * 0.75 + ry * 0.3, ry * 0.75 + rsd * 5.0);
  vec3 rn1 = fj_noised(rq1);
  vec2 rq2 = vec2(rzw * 2.25 - ry * 0.9 + 0.3, ry * 1.9 + rzw * 0.5 + 1.7);
  vec3 rn2 = fj_noised(rq2);
  return (1.0 - abs(2.0 * rn1.x - 1.0)) * 0.4 + (1.0 - abs(2.0 * rn2.x - 1.0)) * 0.13;
}}
float fj_wa(float wk, float wrr) {{
  float wt = clamp((wk - 2.0) / {FJ_WK - 2}.0, 0.0, 1.0);
  float wx = 1.0 - pow(1.0 - wt, 1.0 / 2.4);
  wx = mix(-0.4, mix(-0.15, wx, step(1.5, wk)), step(0.5, wk));
  return wx * wrr;
}}
float fj_wm(float wzw, float wsd, float wk) {{
  float wcl = fj_cliff(wzw, wsd);
  float wrr = wcl / 3.0;
  vec2 wbq = vec2(wzw * 0.25, wsd * 7.0);
  vec3 wbn = fj_noised(wbq);
  float wbz = 0.35 + 0.25 * wbn.x;
  float wbw = 0.1 + 0.12 * wbn.x;
  float wtop = wrr + fj_relief(wzw, wcl, wsd);
  float wfl = 1e9;
  float wcv = -1e9;
  for (int j = 0; j <= {FJ_WK}; j++) {{
    float wj = float(j);
    float waj = fj_wa(wj, wrr);
    float wyj = fj_prof(waj, wcl, wrr, wbz, wbw, 0.35);
    float wwj = waj + fj_relief(wzw, max(wyj, 0.0), wsd);
    wfl = mix(wfl, min(wfl, wwj), step(wk - 0.5, wj));
    wcv = mix(wcv, max(wcv, wwj), step(wj, wk + 0.5));
  }}
  return 0.5 * (wfl + min(wcv, wtop));
}}
float fj_wmr(float rip, float rs, float rk) {{
  vec2 ruv = fj_duv({FJ_DW + 1}.0 + rip, fj_ch() + rs * {FJ_WK + 1}.0 + rk);
  vec3 rv = fj_tex(ruv);
  return fj_dec(rv.xy) * 4.0 - 1.0;
}}
vec2 fj_base(float gu, float gu1, float gzw, float gcip) {{
  float gsd0 = gu < 0.0 ? -1.0 : 1.0;
  float gsd;
  float gsp = fj_spur(gzw, 25.6, 3.0, 3.0, gsd);
  float gon = step(0.0, gu * gsd);
  float gwf = max(fj_width(gzw) - gsp * gon * 1.3, 2.0);
  vec2 gp = vec2(gu, gzw);
  vec2 gp1 = vec2(gu1, gzw);
  vec2 gn = vec2(fj_eroded(gp), fj_eroded(gp1));
  float gcl = fj_cliff(gzw, gsd0);
  float grr = gcl / 3.0;
  vec2 gbq = vec2(gzw * 0.25, gsd0 * 7.0);
  vec3 gbn = fj_noised(gbq);
  float gbz = 0.35 + 0.25 * gbn.x;
  float gbw = 0.1 + 0.12 * gbn.x;
  vec2 gau = vec2(abs(gu), abs(gu1));
  vec2 gxc = gau - gwf + 0.6 * (gn - 0.35);
  float ga0 = -0.4 * grr;
  vec2 gh = vec2(0.0);
  if (gcip < -0.5) {{
    vec2 gyf = clamp((gau - gwf) / grr, 0.0, 1.0) * gcl;
    vec2 gaa = gxc - vec2(fj_relief(gzw, gyf.x, gsd0), fj_relief(gzw, gyf.y, gsd0));
    gh = vec2(fj_prof(gaa.x, gcl, grr, gbz, gbw, gn.x), fj_prof(gaa.y, gcl, grr, gbz, gbw, gn.y));
  }} else {{
    float gs = step(0.0, gsd0);
    float gtop = fj_wmr(gcip, gs, {FJ_WK}.0);
    vec2 gabove = step(gtop, gxc);
    vec2 gpa = mix(gxc, gxc - gtop + grr, gabove);
    gh = vec2(fj_prof(gpa.x, gcl, grr, gbz, gbw, gn.x), fj_prof(gpa.y, gcl, grr, gbz, gbw, gn.y));
    vec2 gin = (1.0 - gabove) * step(ga0, gxc);
    if (gin.x + gin.y > 0.5) {{
      vec2 blo = vec2(0.0);
      vec2 bhi = vec2({FJ_WK}.0);
      for (int it = 0; it < {FJ_WKLOG}; it++) {{
        vec2 bmid = floor((blo + bhi) * 0.5);
        vec2 bwm = vec2(fj_wmr(gcip, gs, bmid.x), fj_wmr(gcip, gs, bmid.y));
        vec2 blt = 1.0 - step(gxc, bwm);
        blo = mix(blo, bmid, blt);
        bhi = mix(bmid, bhi, blt);
      }}
      vec2 bwlo = vec2(fj_wmr(gcip, gs, blo.x), fj_wmr(gcip, gs, blo.y));
      vec2 bwhi = vec2(fj_wmr(gcip, gs, bhi.x), fj_wmr(gcip, gs, bhi.y));
      vec2 balo = vec2(fj_wa(blo.x, grr), fj_wa(blo.y, grr));
      vec2 bahi = vec2(fj_wa(bhi.x, grr), fj_wa(bhi.y, grr));
      vec2 bylo = vec2(fj_prof(balo.x, gcl, grr, gbz, gbw, 0.35), fj_prof(balo.y, gcl, grr, gbz, gbw, 0.35));
      vec2 byhi = vec2(fj_prof(bahi.x, gcl, grr, gbz, gbw, 0.35), fj_prof(bahi.y, gcl, grr, gbz, gbw, 0.35));
      vec2 bfr = clamp((gxc - bwlo) / max(bwhi - bwlo, vec2(0.00001)), 0.0, 1.0);
      gh = mix(gh, mix(bylo, byhi, bfr), gin);
    }}
  }}
  return gh;
}}
float fj_stripcol(float sci, float scib, float sczn, float scw) {{
  vec2 spuv = fj_duv(10.0, {FJ_DH}.0);
  vec3 spv = fj_tex(spuv);
  float spib = floor(fj_dec24(spv) * 16777215.0 + 0.5) - 1000.0;
  vec2 stxuv = fj_duv(8.0, {FJ_DH}.0);
  vec3 stxv = fj_tex(stxuv);
  vec2 styuv = fj_duv(9.0, {FJ_DH}.0);
  vec3 styv = fj_tex(styuv);
  float stx = fj_dec24(stxv) * 8192.0;
  float sty = fj_dec24(styv) * 8192.0;
  float ssh = mod(scib - spib + sczn * 0.5, sczn) - sczn * 0.5;
  float sip = sci + ssh;
  float sok = step(3.5, frame) * step(abs(stx - texsize.x), 0.5) * step(abs(sty - texsize.y), 0.5);
  sok *= step(0.0, sip) * step(sip, scw - 1.0) * step({FJ_SR + 8}.0, texsize.y);
  return mix(-1.0, sip, sok);
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
float fj_height(float cu, float czw, float cip, float czd, out float cmat) {{
  float ce = 0.06;
  vec2 chuv = fj_base(cu, cu + ce, czw, cip);
  float ch0 = chuv.x;
  float chu = chuv.y;
  float chip = mix(-1.0, cip + 1.0, step(-0.5, cip) * step(cip + 1.5, fj_cw()));
  vec2 chzv = fj_base(cu, cu, czw + czd, chip);
  float chz = chzv.x;
  vec2 cg = vec2((chu - ch0) / ce, (chz - ch0) / czd);
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
  if (cvi > 7.5) cvv = texsize.x / 8192.0;
  if (cvi > 8.5) cvv = texsize.y / 8192.0;
  float cvzd = fj_dzt();
  if (cvi > 9.5) cvv = (floor(q4 / cvzd) - ceil({FJ_ZB} / cvzd) + 1000.0) / 16777215.0;
  if (cvi > 10.5) cvv = floor(time * {FJ_HRATE}.0) / 16777215.0;
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
  }} else if (tx.x < {FJ_NB}.0 && dj < {FJ_HR0 + FJ_HN}.0) {{
    float hdi = floor(tx.x);
    float hj = dj - {FJ_HR0}.0;
    vec2 hcuv = fj_duv(11.0, {FJ_DH}.0);
    vec3 hcv = fj_tex(hcuv);
    float hcp = floor(fj_dec24(hcv) * 16777215.0 + 0.5);
    float hk = floor(time * {FJ_HRATE}.0) - hcp;
    hk = mix(1.0, clamp(hk, 0.0, {FJ_HN}.0), step(0.0, hk));
    vec2 hfuv = fj_duv(hdi, 1.0);
    vec3 hfv = fj_tex(hfuv);
    vec2 hmuv = fj_duv(hdi, 2.0);
    vec3 hmv = fj_tex(hmuv);
    float hfol = fj_dec(hfv.xy) * {FJ_SMAX};
    float hmean = max(fj_dec(hmv.xy) * {FJ_SMAX}, 0.2);
    float hlv = smoothstep(0.4, 2.2, hfol / hmean);
    vec2 hsuv = fj_duv(hdi, {FJ_HR0}.0 + max(hj - hk, 0.0));
    vec3 hsv = fj_tex(hsuv);
    vec3 hnew = vec3(fj_enc(hlv), clamp(q1, 0.0, 1.0));
    outc = mix(hsv, hnew, step(hj, hk - 0.5));
    outc = mix(outc, vec3(0.0), step(frame, 2.5));
  }} else {{
    float cw = fj_cw();
    float chh = fj_ch();
    float cxl = tx.x - {FJ_DW + 1}.0;
    float czd = fj_dzt();
    float czn = floor(256.0 / czd + 0.5);
    float cib = floor(q4 / czd) - ceil({FJ_ZB} / czd);
    float czi = cib + floor(cxl);
    float czw = mod(czi, czn) * czd;
    if (cxl >= 0.0 && cxl < cw && tx.y < chh) {{
      float ca = tx.y / chh;
      float cu = {FJ_U} * (2.0 * ca - 1.0);
      float cip = fj_stripcol(floor(cxl), cib, czn, cw);
      float cmat;
      float ch = fj_height(cu, czw, cip, czd, cmat);
      outc = vec3(fj_enc((ch - {FJ_HMIN}) / {FJ_HMAX - FJ_HMIN}), cmat);
    }} else if (cxl >= 0.0 && cxl < cw && tx.y < chh + {FJ_SR}.0) {{
      float sr = floor(tx.y - chh);
      float ss = floor(sr / {FJ_WK + 1}.0);
      float sk = sr - ss * {FJ_WK + 1}.0;
      float swm = fj_wm(czw, ss * 2.0 - 1.0, sk);
      outc = vec3(fj_enc((swm + 1.0) * 0.25), 0.0);
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
float fj_hx(float xu, float xz) {{
  float xe = max(abs(xu) - {FJ_U - 1.5}, 0.0);
  if (xe <= 0.0) {{
    return 0.0;
  }}
  float xs = xe * xe / (xe + 3.0);
  vec2 xq = vec2(xu * 0.3 + 11.0, xz * 0.25);
  vec3 xn = fj_noised(xq);
  vec2 xq2 = vec2(xu * 0.7 - 5.0, xz * 0.75 + 0.5);
  vec3 xn2 = fj_noised(xq2);
  float xr = (1.0 - abs(2.0 * xn.x - 1.0)) * 0.8 + xn2.x * 0.2;
  return xs * (0.15 + 0.55 * xr);
}}
float fj_h(vec3 hq3) {{
  float hu = hq3.x - fj_path(hq3.z);
  float hzr = hq3.z - q4;
  vec2 huv = fj_cuv(hu, hzr);
  vec3 hc = fj_tex(huv);
  return {FJ_HMIN} + {FJ_HMAX - FJ_HMIN} * fj_dec(hc.xy) + fj_hx(hu, hq3.z);
}}
vec3 fj_hs(vec3 sp) {{
  float su = sp.x - fj_path(sp.z);
  float szr = sp.z - q4;
  vec2 suv = fj_cuv(su, szr);
  vec2 sc = suv * texsize.xy - 0.5;
  sc = clamp(sc, vec2({FJ_DW + 2}.0, 1.0), vec2({FJ_DW + 1}.0 + fj_cw() - 3.0, fj_ch() - 3.0));
  vec2 sbase = vec2(0.0);
  vec3 sv = fj_bspline(sc, sbase);
  return vec3({FJ_HMIN} + {FJ_HMAX - FJ_HMIN} * fj_dec(sv.xy) + fj_hx(su, sp.z), sv.z, su);
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
vec2 fj_hist(float fhb, float fhr) {{
  vec2 fhc = vec2(clamp(fhb, 0.0, {FJ_NB - 2.5}), {FJ_HR0}.0 + clamp(fhr, 1.0, {FJ_HN - 3}.0));
  vec2 fhbase = vec2(0.0);
  vec3 fhv = fj_bspline(fhc, fhbase);
  return vec2(fj_dec(fhv.xy), fhv.z);
}}
vec3 fj_wlane(float wx, float wz) {{
  float wlu = wx - fj_path(wz);
  vec2 wlq = vec2(wlu * 0.5 + 3.0, wz * 0.25 - time * 0.2);
  vec3 wln = fj_noised(wlq);
  vec2 wlq2 = vec2(wlu * 0.5 - 7.0, wz * 0.25 + time * 0.13 + 5.0);
  vec3 wln2 = fj_noised(wlq2);
  float wlue = wlu + (wln.x - 0.5) * 0.9;
  float wlze = wz - q4 + (wln2.x - 0.5) * 1.6;
  float wlrow = max(wlze - 0.6, 0.0) / {FJ_VFLOW} * {FJ_HRATE}.0 - fract(time * {FJ_HRATE}.0);
  float wlwid = max(fj_width(wz) - 0.3, 1.5);
  float wlsg = wlue < 0.0 ? -1.0 : 1.0;
  return vec3(abs(wlue) / wlwid * {FJ_NB - 2.5}, wlrow, wlsg);
}}
float fj_wfade(float fb, float ft) {{
  return smoothstep({FJ_NB - 1.0}, {FJ_NB - 3.0}, fb) * smoothstep(45.0, 15.0, ft);
}}
float fj_wh(vec3 whp, float whamp, float wht) {{
  vec3 whl = fj_wlane(whp.x, whp.z);
  vec2 whuv = (vec2(clamp(whl.x, 0.0, {FJ_NB - 2.5}), {FJ_HR0}.0 + clamp(whl.y, 1.0, {FJ_HN - 3}.0)) + 0.5) * texsize.zw;
  vec3 whv = fj_tex(whuv);
  return whamp * fj_dec(whv.xy) * fj_wfade(whl.x, wht);
}}
float fj_cam(float cri) {{
  vec2 cruv = fj_duv(cri, {FJ_DH + 1}.0);
  return fj_dec24(fj_tex(cruv));
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
  float wamp = min({FJ_WAMP}, 0.6 * max(q8, 0.05));
  float tw = 1e9;
  if (rd.y < -0.0001) {{
    float twb = -ro.y / rd.y;
    tw = twb;
    if (twb < 45.0) {{
      float twa = max((ro.y - wamp) / -rd.y, 0.0);
      float wsteps = clamp(ceil((twb - twa) / 0.18), 3.0, 10.0);
      float wtp = twa;
      float wtc = twb;
      for (int i = 1; i <= 10; i++) {{
        if (float(i) > wsteps) break;
        float wti = twa + (twb - twa) * float(i) / wsteps;
        vec3 wpi = ro + rd * wti;
        if (wpi.y <= fj_wh(wpi, wamp, wti)) {{
          wtc = wti;
          break;
        }}
        wtp = wti;
      }}
      for (int k = 0; k < 4; k++) {{
        float wtm = 0.5 * (wtp + wtc);
        vec3 wpm = ro + rd * wtm;
        float wbel = step(wpm.y, fj_wh(wpm, wamp, wtm));
        wtc = mix(wtc, wtm, wbel);
        wtp = mix(wtm, wtp, wbel);
      }}
      tw = wtc;
    }}
  }}
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
    float sprow = max(pos.z - q4 - 0.6, 0.0) / {FJ_VFLOW} * {FJ_HRATE}.0 - fract(time * {FJ_HRATE}.0);
    vec2 sph = fj_hist({FJ_NB - 5}.0, sprow);
    float spx = clamp(sph.x, 0.0, 1.0) * 5.0;
    vec3 spc = mix(NOKKVI_RAMP0, NOKKVI_RAMP1, clamp(spx, 0.0, 1.0));
    spc = mix(spc, NOKKVI_RAMP2, clamp(spx - 1.0, 0.0, 1.0));
    spc = mix(spc, NOKKVI_RAMP3, clamp(spx - 2.0, 0.0, 1.0));
    spc = mix(spc, NOKKVI_RAMP4, clamp(spx - 3.0, 0.0, 1.0));
    spc = mix(spc, NOKKVI_RAMP5, clamp(spx - 4.0, 0.0, 1.0));
    float spk = mix({FJ_LDAY}, 1.0, q9) * exp(-max(pos.y, 0.0) * 1.6) * smoothstep(0.35, -0.2, n.y) * exp(-t * 0.03);
    col += spc * (0.12 * sph.x + {FJ_LSPILL} * sph.y) * spk * fj_rock(pos, n, pu, pmat) * 3.0;
    col = fj_fog(col, rd, t, pos.y, sun);
  }} else if (tw < 1e8) {{
    vec3 wp = ro + rd * tw;
    float wfade = exp(-tw * 0.05);
    vec2 wg0 = fj_ripple(wp.xz);
    vec2 wf2 = normalize(rd.xz + vec2(0.00001, 0.0));
    vec2 wl2 = vec2(-wf2.y, wf2.x);
    vec2 wg = wf2 * dot(wg0, wf2) + wl2 * dot(wg0, wl2) * 0.35;
    float wa = (0.012 + 0.03 * wfade) / (1.0 + 0.15 * tw);
    vec3 lln = fj_wlane(wp.x, wp.z);
    float lb = lln.x;
    float lrow = lln.y;
    float lsgn = lln.z;
    float lwid = max(fj_width(wp.z) - 0.3, 1.5);
    vec2 lh0 = fj_hist(lb, lrow);
    float lr0 = lh0.x;
    float lbt = lh0.y;
    vec2 lhb = fj_hist(lb + 0.5, lrow);
    float lrb = lhb.x;
    vec2 lhr = fj_hist(lb, lrow + 2.0);
    float lrr = lhr.x;
    vec2 lgr = vec2((lrb - lr0) / (0.5 * lwid / {FJ_NB - 2.5}) * lsgn, (lrr - lr0) / (2.0 * {FJ_VFLOW} / {FJ_HRATE}.0));
    float lamp = wamp * fj_wfade(lb, tw);
    vec3 wn = normalize(vec3(-wg.x * wa - lgr.x * lamp, 1.0, -wg.y * wa - lgr.y * lamp));
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
    float lk = lr0 * {FJ_LCONT};
    float lfw = max(fwidth(lk), 0.0001);
    float lcd = abs(fract(lk + 0.5) - 0.5);
    float lline = (1.0 - smoothstep(0.0, lfw * 1.2 + 0.05, lcd)) * smoothstep(0.03, 0.12, lr0);
    float lglow = exp(-lcd * lcd * mix(40.0, 12.0, lbt)) * smoothstep(0.03, 0.12, lr0);
    float lci = floor(lk + 0.5);
    float lcx = fract(lci * 0.19 + time * 0.02) * 5.0;
    vec3 lc = mix(NOKKVI_RAMP0, NOKKVI_RAMP1, clamp(lcx, 0.0, 1.0));
    lc = mix(lc, NOKKVI_RAMP2, clamp(lcx - 1.0, 0.0, 1.0));
    lc = mix(lc, NOKKVI_RAMP3, clamp(lcx - 2.0, 0.0, 1.0));
    lc = mix(lc, NOKKVI_RAMP4, clamp(lcx - 3.0, 0.0, 1.0));
    lc = mix(lc, NOKKVI_RAMP5, clamp(lcx - 4.0, 0.0, 1.0));
    float lbx = clamp(lr0, 0.0, 1.0) * 5.0;
    vec3 lbc = mix(NOKKVI_RAMP0, NOKKVI_RAMP1, clamp(lbx, 0.0, 1.0));
    lbc = mix(lbc, NOKKVI_RAMP2, clamp(lbx - 1.0, 0.0, 1.0));
    lbc = mix(lbc, NOKKVI_RAMP3, clamp(lbx - 2.0, 0.0, 1.0));
    lbc = mix(lbc, NOKKVI_RAMP4, clamp(lbx - 3.0, 0.0, 1.0));
    lbc = mix(lbc, NOKKVI_RAMP5, clamp(lbx - 4.0, 0.0, 1.0));
    float lemk = mix({FJ_LDAY}, 1.0, q9) * exp(-tw * 0.025) * smoothstep(0.0, 0.35, wdp);
    lemk *= smoothstep({FJ_NB - 1.0}, {FJ_NB - 3.0}, lb);
    vec3 lemit = (lbc * 0.4 * lr0 * lr0 + lc * (1.3 * lline + 0.35 * lglow) * (0.35 + lr0) * (1.0 + {FJ_LFLARE} * lbt)) * lemk;
    lemit += lbc * lbt * (0.08 + lglow) * (0.3 + lr0) * {FJ_LEXPO} * lemk;
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
    gspdot *= smoothstep(10.0, 3.0, tw);
    lemit += lc * gspdot * (0.5 * lglow + {FJ_LPLANK} * lbt) * lemk;
    col += lemit * (1.0 - 0.6 * fre);
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
  vec2 hedge = min(huv, 1.0 - huv);
  float hok = step(0.001, cvz) * smoothstep(0.0, 0.04, min(hedge.x, hedge.y)) * step(3.5, frame);
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


FJ_INIT = ("pulse = 0; pop = 0; dist = 0; " + SPEED_INIT +
           " act = int(rand(4)); acttm = 12; alt = 0.5; alt_m = 0.5; lk = 5; lk_m = 5; lkh = 0.3; lkh_m = 0.3;"
           " lat = 0.1; lat_m = 0.1; fov = 1; fov_m = 1; bank = 0; bank_m = 0; lsm = 1; flow = 0; hue = 0; vis = -1;"
           " aur = 1; fsp = 1; fsp_m = 1; gside = 1; saz = 0.8; rbt = 0;"
           "")
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
      "rbk = max(bass - bass_att, 0); rbt = max(rbt * exp(-dt / 0.2), min(rbk * 1.3, 1)); q1 = rbt;\n")

presets["nokkvi - fjord"] = preset({"decay": 0.0, "wave_a": 0.0, "zoom": 1.0}, FJ_WARP, FJ_COMP,
                                  init=FJ_INIT, frame=FJ_FRAME)

# Pirate signal (no cover) --------------------------------------------------
# The owner's avatar, the pirate smiley (assets/moon_face.svg), as a pirate
# TV broadcast. The face is an analytic shape (the SVG's circle, smile arc,
# eye, patch and strap fitted as distance functions, so it is sharp at any
# size and can blink, grin and pop), drawn over a living oil-and-sand field
# in the manner of maxawow: the feedback holds paint (x = thickness, y = its
# place on the theme gradient, z = heat), advected along its own contours
# and by its own colour, stirred by three whirlpools (bass, mid, treble) and
# eaten by a grainy subtractive fade, which the loop integrates into sand.
# The face sheds the paint: each angle around its rim is a frequency, so the
# spectrum pours out of it. On each beat the face is stamped into the paint
# and the field surges outward in a turning spiral, so echoes of the face
# bloom out from behind it and melt into the sand; the beat's glitch also
# tears rows of the paint (scars that stay). The signal is shown as an LCD:
# beats glitch it (torn rows, RGB split, datamosh blocks that stick from the
# last frame, a channel-swapped slab, a vertical jump), every fourth or a
# hard beat blows the image up into big LCD cells for a moment, and treble
# transients break it into static (snow, bright interference rows, row
# wobble) and sprinkle grains into the sand.
PF_FN = """
vec2 pf_face(vec2 pfq, float pfblink, float pfext) {
  vec2 pfv = vec2(pfq.x, -pfq.y) * 132.46;
  float pfdisc = length(pfv) - 132.46;
  float pfphi = atan(pfv.x, pfv.y);
  float pfin = step(abs(pfphi), 1.5708 + pfext);
  vec2 pfe = vec2(92.11 * cos(pfext), -92.11 * sin(pfext));
  vec2 pfax = vec2(abs(pfv.x), pfv.y);
  float pfarc = mix(length(pfax - pfe), abs(length(pfv) - 92.11), pfin) - 3.31;
  float pfry = 32.19 * (1.0 - 0.9 * pfblink);
  vec2 pfed = (pfv - vec2(-41.08, -43.58)) / vec2(25.74, pfry);
  float pfeye = (length(pfed) - 1.0) * min(25.74, pfry);
  vec2 pfpd = pfv - vec2(56.64, -60.13);
  float pfu = pfpd.x * 0.9427 + pfpd.y * 0.3336;
  float pfw = pfpd.y * 0.9427 - pfpd.x * 0.3336;
  float pfpatch = max((length(vec2(pfu / 40.64, pfw / 51.13)) - 1.0) * 40.64, 0.004 * pfu * pfu - 17.05 - pfw);
  vec2 pfsq = pfv - vec2(-68.59, -112.27);
  float pfsh = clamp(dot(pfsq, vec2(192.24, 68.01)) / 41582.0, 0.0, 1.0);
  vec2 pfso = pfsq - vec2(192.24, 68.01) * pfsh;
  float pfstrap = length(pfso) - 3.31;
  float pffeat = min(min(pfarc, pfeye), min(pfpatch, pfstrap));
  return vec2(pfdisc, pffeat) / 132.46;
}
"""
PF_OUTLINE = "0.0248"
# The face lies on a card tilted in perspective (the starfield nebula's
# camera, centred on the face): screen offset `pd` from the face centre is
# cast onto the plane (pitch, yaw, roll, scale `zm`, focal length `foc`) and
# comes back in the card's own coordinates as `{x}_q`; with `th`, `{x}_b` is
# the same point on the card's back face, `th` deeper (the coin's edge).
def pf_proj(x, pd, pit, yaw, rol, zm, foc, th=None):
    out = f"""
  float {x}_cp = cos({pit}); float {x}_sp = sin({pit});
  float {x}_cy = cos({yaw}); float {x}_sy = sin({yaw});
  vec3 {x}_ex = vec3({x}_cy, 0.0, -{x}_sy);
  vec3 {x}_ey = vec3({x}_sp * {x}_sy, {x}_cp, {x}_sp * {x}_cy);
  vec3 {x}_en = vec3({x}_cp * {x}_sy, -{x}_sp, {x}_cp * {x}_cy);
  vec3 {x}_rd = vec3({pd}, {foc});
  float {x}_dn = max(dot({x}_rd, {x}_en), 0.05);
  vec3 {x}_h = {x}_rd * ({foc} * {x}_en.z / {x}_dn) - vec3(0.0, 0.0, {foc});
  vec2 {x}_pl = vec2(dot({x}_h, {x}_ex), dot({x}_h, {x}_ey));
  float {x}_cr = cos({rol}); float {x}_sr = sin({rol});
  vec2 {x}_q = vec2({x}_pl.x * {x}_cr - {x}_pl.y * {x}_sr, {x}_pl.x * {x}_sr + {x}_pl.y * {x}_cr) / {zm};
"""
    if th:
        out += f"""
  vec3 {x}_hb = {x}_rd * (({foc} * {x}_en.z + {th}) / {x}_dn) - vec3(0.0, 0.0, {foc}) - {x}_en * {th};
  vec2 {x}_pb = vec2(dot({x}_hb, {x}_ex), dot({x}_hb, {x}_ey));
  vec2 {x}_b = vec2({x}_pb.x * {x}_cr - {x}_pb.y * {x}_sr, {x}_pb.x * {x}_sr + {x}_pb.y * {x}_cr) / {zm};
"""
    return out
# Camera acts (the starfield nebula's, per beat): the scene stops (q6, the
# warp holds the paint), the card is dragged from its old pose to a new one
# over ~45% of the beat interval while it hops to a new spot on screen
# (overshooting a little as it lands), and everything flows again. q21..q25 =
# pitch, yaw, roll, scale, focal length; q26..q29 = this frame's step of the
# first four (the comp's motion blur); q31 = drag speed (its echo smear);
# q11/q12 = the face's spot (-1..1 of the room the panel leaves it), q1/q2
# its step this frame.
PF_CAMERA = """cs = cs + dt;
ibi = if(trig, ibi * 0.8 + min(cs, 1.5) * 0.2, ibi);
cs = if(trig, 0, cs);
at = at + dt * (1 - inact);
go = below(inact, 0.5) * max(trig, above(at, 6));
inact = max(inact, go);
aa = if(go, 0, aa + dt * inact);
nacts = nacts + go;
u1 = rand(1000) / 1000 + bass_att * 3.7; u1 = u1 - int(u1);
u2 = rand(1000) / 1000 + mid_att * 5.3; u2 = u2 - int(u2);
u3 = rand(1000) / 1000 + treb_att * 7.1; u3 = u3 - int(u3);
flat = equal(nacts % 8, 0);
amp = min(0.35 + max(bk, 0) * 0.7, 0.85);
fpit = if(go, cpit, fpit); fyaw = if(go, cyaw, fyaw); frol = if(go, crol, frol); fzm = if(go, czm, fzm); ffoc = if(go, cfoc, ffoc);
tpit = if(go, min(max(cpit * 0.3 + (u1 - 0.5) * 2 * amp, -0.8), 0.8) * (1 - flat), tpit);
tyaw = if(go, min(max(cyaw * 0.3 + (u2 - 0.5) * 2 * amp, -0.8), 0.8) * (1 - flat), tyaw);
trol = if(go, min(max(crol * 0.3 + (u3 - 0.5) * 1.4 * amp, -0.5), 0.5) * (1 - flat), trol);
tfoc = if(go, 0.8 + u3 + 0.8 * flat, tfoc);
tzm = if(go, 0.85 + 0.4 * u1 * (1 - flat) + 0.15 * flat, tzm);
ddur = if(go, min(max(ibi * 0.45, 0.12), 0.4), ddur);
eprev = if(go, 0, eprev);
lin = min(max((aa - 0.05) / ddur, 0), 1);
e = lin * lin * (3 - 2 * lin);
lb1 = lin - 1; eb = 1 + 2.70158 * lb1 * lb1 * lb1 + 1.70158 * lb1 * lb1;
u4 = rand(1000) / 1000 + bass * 2.9; u4 = u4 - int(u4);
u5 = rand(1000) / 1000 + treb * 4.3; u5 = u5 - int(u5);
fpx = if(go, cpx, fpx); fpy = if(go, cpy, fpy);
mamp = min(0.55 + max(bk, 0), 1);
tpx = if(go, min(max(cpx * 0.25 + (u4 - 0.5) * 2 * mamp, -1), 1) * (1 - flat), tpx);
tpy = if(go, min(max(cpy * 0.25 + (u5 - 0.5) * 2 * mamp, -1), 1) * (1 - flat), tpy);
thop = if(go, 0.25 + 0.6 * u4 * mamp, thop);
cpx = if(inact, fpx + (tpx - fpx) * eb, cpx);
cpy = if(inact, fpy + (tpy - fpy) * eb, cpy);
hop = inact * sin(3.14159 * lin) * thop;
cpit = if(inact, fpit + (tpit - fpit) * e, cpit);
cyaw = if(inact, fyaw + (tyaw - fyaw) * e, cyaw);
crol = if(inact, frol + (trol - frol) * e, crol);
czm = if(inact, fzm + (tzm - fzm) * e, czm);
cfoc = if(inact, ffoc + (tfoc - ffoc) * e, cfoc);
cmv = abs(e - eprev) / dt * (abs(tpit - fpit) + abs(tyaw - fyaw) + abs(trol - frol) + abs(tzm - fzm) + 0.4 * (abs(tpx - fpx) + abs(tpy - fpy)));
dee = e - eprev;
eprev = e;
frzt = inact * below(aa, 0.07 + ddur);
done = inact * above(aa, 0.07 + ddur);
inact = inact * (1 - done);
at = if(done, 0, at);
frz = frz + (frzt - frz) * (1 - exp(-dt / 0.025));
q6 = frz; q21 = cpit; q22 = cyaw; q23 = crol; q24 = czm; q25 = cfoc;
q26 = dee * (tpit - fpit); q27 = dee * (tyaw - fyaw); q28 = dee * (trol - frol); q29 = dee * (tzm - fzm);
q31 = min(cmv * 0.3, 0.75);
"""
def pf_whirl(i, cx, cy, band):
    return f"""
  {{
    vec2 wc = vec2({cx}, {cy});
    vec2 wd = p - wc;
    float wr = 0.1 + 0.12 * min({band}_att, 2.0);
    float wk = max(wr * wr - dot(wd, wd), 0.0) * 0.35 * ({band} - 0.4) * {1 if i % 2 == 0 else -1}.0;
    sw += vec2(-wd.y, wd.x) * wk;
  }}
"""
PF_WARP = PF_FN + " shader_body {\n" + HEAD + """
  vec2 p = (uv_orig - 0.5) * s;
  vec2 fc = vec2(q11, q12) * max(0.5 * s - q13 * 0.85, vec2(0.02));
  vec2 d = p - fc;
  float an = q18;
  vec2 dr = vec2(d.x * cos(an) - d.y * sin(an), d.x * sin(an) + d.y * cos(an)) / (1.0022 + q16);
  vec2 sw = vec2(0.0);
""" + pf_whirl(0, "0.3 * sin(time * 0.618)", "0.22 * cos(time * 1.618)", "bass") \
    + pf_whirl(1, "0.45 * sin(time * 0.93 + 2.0)", "0.3 * cos(time * 0.71)", "mid") \
    + pf_whirl(2, "0.5 * sin(-time * 1.21)", "0.35 * cos(-time * 0.57 + 1.0)", "treb") + """
  vec2 suv = (fc + dr + sw) / s + 0.5;
  float gband = floor(uv_orig.y * 40.0);
  float gh = fract(sin(gband * 12.9898 + q8 * 78.233) * 43758.5453);
  suv.x += (gh - 0.5) * 0.02 * q7 * step(0.75, fract(gh * 57.3));
  suv += (texture(sampler_noise_lq, uv_orig * texsize.xy / 256.0 * 0.5 + rand_frame.xy).xy - 0.5) * texsize.zw;
  vec2 auv = 2.0 * suv - uv_orig;
  vec3 ahead = texture(sampler_main, auv).xyz - GetBlur1(auv);
  vec2 bd = texsize.zw * 3.0;
  float gx = GetBlur1(suv + vec2(bd.x, 0.0)).x - GetBlur1(suv - vec2(bd.x, 0.0)).x;
  float gy = GetBlur1(suv + vec2(0.0, bd.y)).x - GetBlur1(suv - vec2(0.0, bd.y)).x;
  vec2 flow = vec2(-gy, gx) * texsize.zw * (1.0 + 3.0 * min(mid_att, 2.0))
            + ahead.xy * (0.05 * clamp(vec2(bass, treb) - 1.0, 0.0, 1.0) + 0.004);
  vec3 f = texture(sampler_main, suv + flow).xyz;
  float grain = texture(sampler_noise_lq, uv_orig * texsize.xy / 256.0 + rand_frame.zw).x;
  float dens = f.x + (f.x - GetBlur1(suv + flow).x) * 0.06 - (0.0005 + (grain - 0.5) * 0.03);
  float hue = f.y;
  float hot = f.z * 0.965;
  vec3 held = texture(sampler_main, (floor(uv_orig * texsize.xy) + 0.5) * texsize.zw).xyz;
  dens = mix(dens, held.x, q6);
  hue = mix(hue, held.y, q6);
  hot = mix(hot, held.z, q6);
  float fr = q13;
""" + pf_proj("pw", "d", "q21", "q22", "q23", "q24", "q25") + """
  vec2 fq = pw_q / fr;
  float rd = length(fq) - 1.0;
  float band = abs(atan(fq.x, fq.y)) / 3.14159;
  float spec = get_fft(0.02 + band * 0.5);
  float feed = smoothstep(0.07, 0.01, rd) * step(0.0, rd) * clamp(spec * 2.2 * (0.6 + 0.6 * q5), 0.0, 1.0) * (0.3 + 0.7 * q30);
  float fhue = q17 + (band - 0.5) * 0.35;
  dens = max(dens, feed * 0.95);
  hue = mix(hue, 1.0 - abs(1.0 - mod(fhue + 2.0, 2.0)), feed);
  vec2 fs = pf_face(fq, 0.0, q19);
  float inside = smoothstep(0.01, -0.01, fs.x);
  float featm = max(smoothstep(0.02, 0.0, fs.y), smoothstep(0.02, 0.0, abs(fs.x) - 0.03));
  float stamp = q15 * q30 * max(inside, featm);
  dens = mix(dens, 0.02, stamp * featm);
  hue = mix(hue, q17, stamp);
  hot = max(hot, stamp * (1.0 - featm));
  dens += q10 * 0.4 * step(0.992, texture(sampler_noise_lq, uv_orig * texsize.xy / 256.0 + rand_frame.yx).y);
  ret = vec3(clamp(dens, 0.0, 1.0), clamp(hue, 0.0, 1.0), clamp(hot, 0.0, 1.0));
 }"""
PF_SCENE = PF_FN + """
vec3 pf_scene(vec2 psu) {
  vec2 ps = texsize.xy / min(texsize.x, texsize.y);
  vec2 pp = (psu - 0.5) * ps;
  vec3 pm = texture(sampler_main, psu).xyz;
  vec2 pe = texsize.zw;
  float pgx = texture(sampler_main, psu + vec2(pe.x, 0.0)).x - texture(sampler_main, psu - vec2(pe.x, 0.0)).x;
  float pgy = texture(sampler_main, psu + vec2(0.0, pe.y)).x - texture(sampler_main, psu - vec2(0.0, pe.y)).x;
  vec2 pg = vec2(pgx, pgy);
  vec2 pe4 = texsize.zw * 4.0;
  float pbx = GetBlur1(psu + vec2(pe4.x, 0.0)).x - GetBlur1(psu - vec2(pe4.x, 0.0)).x;
  float pby = GetBlur1(psu + vec2(0.0, pe4.y)).x - GetBlur1(psu - vec2(0.0, pe4.y)).x;
  vec2 pgb = vec2(pbx, pby);
""" + ramp("pcol", "pm.y") + """
  vec3 pn = normalize(vec3(-(pg * 14.0 + pgb * 5.0), 1.0));
  vec3 pl = normalize(vec3(0.5 * cos(time * 0.21), 0.45 + 0.2 * sin(time * 0.17), 0.8));
  float pdif = max(dot(pn, pl), 0.0);
  float pspec = pow(max(dot(reflect(-pl, pn), vec3(0.0, 0.0, 1.0)), 0.0), 24.0);
  float pthick = smoothstep(0.0, 0.2, pm.x);
  vec3 paint = mix(NOKKVI_BG, pcol, pthick * (0.55 + 0.45 * smoothstep(0.1, 0.7, pm.x))) * (0.35 + 0.9 * pdif);
  paint += NOKKVI_HIGHLIGHT * pspec * pthick * 0.6;
  vec2 pu1 = 0.3 * cos((psu - 0.5) * 2.0) - pg;
  vec2 pu2 = 0.3 * cos(pu1 * 12.0) - 9.0 * pg;
  paint += NOKKVI_HIGHLIGHT * clamp(0.04 / length(pu2), 0.0, 1.0) * pthick * 0.5;
  paint += mix(NOKKVI_HIGHLIGHT, NOKKVI_TEXT, 0.3) * smoothstep(0.0, 0.8, pm.z) * 0.8;
  vec2 pfspan = max(0.5 * ps - q13 * 0.85, vec2(0.02));
  vec2 pfc = vec2(q11, q12) * pfspan;
  float pr = q13;
  vec2 pd0 = pp - pfc;
  vec2 psd0 = pd0 - vec2(0.03, -0.04) * pr;
""" + pf_proj("ps", "psd0", "q21", "q22", "q23", "q24", "q25") + """
  float pshd = length(ps_q / pr) - 1.0;
  paint *= 1.0 - 0.6 * smoothstep(0.3, -0.05, pshd) * q30;
  float phc = fract(sin(dot(floor(psu * texsize.xy / 8.0), vec2(12.9898, 78.233))) * 43758.5453);
  float pvis = clamp((q30 * 1.3 - 0.15 - phc * 0.8) * 5.0, 0.0, 1.0);
  vec3 pfacc = vec3(0.0);
  float pfcov = 0.0;
  for (int k = 0; k < 5; k++) {
    float pkt = float(k) * 0.6;
    float pkpit = q21 - q26 * pkt;
    float pkyaw = q22 - q27 * pkt;
    float pkrol = q23 - q28 * pkt;
    float pkzm = q24 - q29 * pkt;
    vec2 pdk = pd0 + vec2(q1, q2) * pfspan * pkt;
""" + pf_proj("pk", "pdk", "pkpit", "pkyaw", "pkrol", "pkzm", "q25", th="0.035") + """
    vec2 pq = pk_q / pr;
    vec2 pqb = pk_b / pr;
    vec2 pf = pf_face(pq, q14, q19);
    float paw = 1.2 / (pr * min(texsize.x, texsize.y) * max(pk_en.z, 0.25) * pkzm);
    float pcv = smoothstep(paw, -paw, pf.x - """ + PF_OUTLINE + """);
    float pink = max(smoothstep(paw, -paw, abs(pf.x) - """ + PF_OUTLINE + """), smoothstep(paw, -paw, pf.y));
    float pedg = smoothstep(paw, -paw, length(pqb) - 1.0248) * (1.0 - pcv);
    vec2 pqd = pq * 0.6;
    vec3 pnl = vec3(pqd, sqrt(max(1.0 - dot(pqd, pqd), 0.05)));
    vec2 pdir = normalize(pqb + vec2(0.0001, 0.0));
    pnl = mix(pnl, vec3(pdir, 0.15), pedg);
    vec2 pnr = vec2(pnl.x * pk_cr + pnl.y * pk_sr, pnl.y * pk_cr - pnl.x * pk_sr);
    vec3 pnv = pk_ex * pnr.x + pk_ey * pnr.y - pk_en * pnl.z;
    vec3 pfn = normalize(vec3(pnv.x, pnv.y, -pnv.z));
    float pfd = max(dot(pfn, pl), 0.0);
    float pfs = pow(max(dot(reflect(-pl, pfn), vec3(0.0, 0.0, 1.0)), 0.0), 60.0);
    vec3 pfbase = mix(NOKKVI_TEXT, NOKKVI_ACCENT, 0.45);
    vec3 pfk = pfbase * (0.35 + 0.8 * pfd) + NOKKVI_TEXT * pfs * 0.7;
    float pedge = smoothstep(-0.03, 0.0, pf.y) * (1.0 - smoothstep(0.0, 0.004, pf.y));
    vec3 pinkc = NOKKVI_BG * 0.6 + NOKKVI_TEXT * (pfs * 0.5 + pedge * 0.12);
    pfk = mix(pfk, pinkc, pink * pcv);
    pfk += mix(NOKKVI_HIGHLIGHT, NOKKVI_TEXT, 0.3) * pm.z * 0.15 * (1.0 - pink) * pcv;
    pfk = mix(pfk, mix(NOKKVI_BG, pfbase, 0.35) * (0.3 + 0.9 * pfd) + NOKKVI_TEXT * pfs * 0.4, pedg);
    float pck = max(pcv, pedg);
    pfacc += pfk * pck;
    pfcov += pck;
  }
  vec3 pface = pfacc / max(pfcov, 0.0001);
  float pcov = pfcov * 0.2 * pvis;
  return mix(paint, pface, pcov);
}
"""
PF_COMP = PF_SCENE + " shader_body {\n" + HEAD + """
  float g = q7;
  float sd = q8;
  vec2 guv = uv;
  float rb = floor(uv.y * (24.0 + 40.0 * fract(sd * 0.137)));
  float rh = fract(sin(rb * 12.9898 + sd * 78.233) * 43758.5453);
  guv.x += (rh - 0.5) * 0.25 * g * step(1.0 - 0.45 * g, fract(rh * 91.7));
  guv.y += q20;
  float srow = floor(uv.y * texsize.y / 2.0);
  float swb = texture(sampler_noise_lq, vec2((srow + 0.5) / 256.0, rand_frame.x)).x - 0.5;
  guv.x += swb * q10 * 0.008;
  float lp = max(3.0, floor(min(texsize.x, texsize.y) / 150.0 + 0.5));
  float cell = floor(mix(1.0, lp * 3.0, q9));
  vec2 cpx = (floor(guv * texsize.xy / cell) + 0.5) * cell;
  guv = mix(guv, cpx * texsize.zw, step(1.5, cell));
  vec2 ro = vec2((1.5 + 16.0 * g) * texsize.z, 0.0);
  vec2 gr = guv + ro;
  vec2 gb = guv - ro;
  vec3 col = vec3(pf_scene(gr).x, pf_scene(guv).y, pf_scene(gb).z);
  float sb = floor(uv.y * 12.0);
  float sh = fract(sin(sb * 7.13 + sd * 1.7) * 43758.5453);
  col = mix(col, col.zxy, step(1.0 - 0.12 * g, sh));
  vec2 sq = floor(uv * texsize.xy / 2.0);
  float sn = texture(sampler_noise_lq, (sq + 0.5) / 256.0 + rand_frame.xy).x;
  float sline = step(0.94, texture(sampler_noise_lq, vec2(rand_frame.z, (srow + 0.5) / 256.0)).x);
  col = mix(col, mix(NOKKVI_BG, NOKKVI_TEXT, sn), clamp(q10 * 0.55, 0.0, 0.8));
  col += NOKKVI_TEXT * sline * q10 * 0.35;
  float lpm = max(lp, cell);
  vec2 lc = uv * texsize.xy / lpm;
  float sub = floor(fract(lc.x) * 3.0);
  vec3 lmask = vec3(1.0 - step(0.5, sub), step(0.5, sub) * (1.0 - step(1.5, sub)), step(1.5, sub));
  float lgap = step(fract(lc.y) * lpm, 1.0);
  vec3 lm = mix(vec3(0.3), vec3(1.0), lmask) * (1.0 - 0.6 * lgap);
  col = mix(col, col * lm * 1.8, 0.3 + 0.45 * q9);
  float bandy = 1.15 - fract(time * 0.23) * 1.3;
  vec2 bq = floor(uv * texsize.xy / (lp * 5.0));
  float bh = fract(sin(dot(bq, vec2(12.9898, 78.233)) + sd * 3.7) * 43758.5453);
  float mosh = step(1.0 - 0.3 * g, bh) * step(0.2, g);
  vec2 mo = floor((vec2(fract(bh * 37.1), fract(bh * 71.3)) - 0.5) * 4.0) * lp * texsize.zw;
  vec4 prev = textureLod(sampler2D(sampler_prev_comp, sampler_prev_comp_samp), uv + mo, 0.0);
  col *= 1.0 + 0.1 * exp(-pow((uv.y - bandy) / 0.06, 2.0));
  col *= 0.97 + 0.03 * sin(time * 46.0) * (1.0 + 3.0 * g);
  col *= 1.0 - 0.3 * pow(length((uv - 0.5) * s), 2.5);
  col += (texture(sampler_noise_lq, uv * texsize.xy / 256.0 + rand_frame.zw).x - 0.5) * 0.02;
  col = mix(col, prev.rgb, mosh * prev.a);
  vec4 pecho = textureLod(sampler2D(sampler_prev_comp, sampler_prev_comp_samp), uv, 0.0);
  col = mix(col, pecho.rgb, q31 * pecho.a);
  ret = col;
 }"""
PF_INIT = (BEATS_INIT + " pulse = 0; pop = 0; nb = 0; gl = 0; lb = 0; stt = 0; bt = 2; bph = 1; es = 0;"
           " rdir = 1; hb = 0.3; sm = 0; jmp = 0; gcool = 0; ng = 0; trt = 1; vtg = 1; vis = 1; ft = 0;"
           " cs = 0; ibi = 0.5; at = 0; inact = 0; aa = 0; nacts = 0; ddur = 0.25; e = 0; eprev = 0; dee = 0; frz = 0;"
           " cpit = 0; cyaw = 0; crol = 0; czm = 1; cfoc = 1.6; fpit = 0; fyaw = 0; frol = 0; fzm = 1; ffoc = 1.6;"
           " tpit = 0; tyaw = 0; trol = 0; tzm = 1; tfoc = 1.6;"
           " lin = 0; eb = 0; cpx = 0; cpy = 0; fpx = 0; fpy = 0; tpx = 0; tpy = 0; thop = 0; hop = 0; pxp = 0; pyp = 0;")
PF_FRAME = ("dt = 1 / max(fps, 1);\n" + PULSE + BEATS
    + "nb = nb + trig;\n"
      "gtr = trig * above(bk, 0.3 + 0.25 * gcool); gcool = if(gtr, 1, gcool * exp(-dt / 0.6));\n"
      "gl = if(gtr, max(gl, min(0.25 + (bk - 0.3) * 1.2, 1)), gl * exp(-dt / 0.07));\n"
      "q7 = gl * above(gl, 0.04);\n"
      "q8 = (int(time * 15) * 7 + nb * 13) % 101;\n"
      "ng = ng + gtr;\n"
      "lb = if(gtr * min(equal(ng % 6, 0) + above(bk, 1.1), 1), 1, lb * exp(-dt / 0.12));\n"
      "q9 = lb * above(lb, 0.03);\n"
      "trt = treb / max(treb_att, 0.05);\n"
      "stt = max(stt * exp(-dt / 0.07), min(max(trt - 1.55, 0) * 1.6, 1));\n"
      "q10 = stt * above(stt, 0.04);\n"
      + PF_CAMERA +
      "ft = ft + dt * (1 - frz);\n"
      "pxn = cpx + 0.06 * sin(ft * 0.31); pyn = min(cpy + hop + 0.06 * sin(ft * 0.43 + 1), 1.2);\n"
      "q1 = pxn - pxp; q2 = pyn - pyp; pxp = pxn; pyp = pyn;\n"
      "q11 = pxn; q12 = pyn;\n"
      "en = (bass_att + mid_att + treb_att) / 3;\n"
      "vtg = if(trig, 1, vtg * exp(-dt / (0.5 + 3 * min(max(en - 0.6, 0), 1))));\n"
      "vis = vis + (vtg - vis) * (1 - exp(-dt / if(above(vtg, vis), 0.07, 0.35)));\n"
      "q30 = vis;\n"
      "q13 = 0.3 * (1 + 0.035 * q5);\n"
      "bt = bt - dt; bst = below(bt, 0); bt = if(bst, 3 + rand(40) / 10, bt); bph = if(bst, 0, bph + dt);\n"
      "q14 = if(below(bph, 0.16), sin(3.14159 * bph / 0.16), 0);\n"
      "q15 = trig * min(0.5 + bs1 * 0.4, 1);\n"
      "es = if(trig, 0.03 * min(bs1, 1.3), es * exp(-dt / 0.18)); q16 = es;\n"
      "rdir = if(trig, -rdir, rdir);\n"
      "q18 = 0.0015 * sin(time * 0.07) + es * 0.4 * rdir;\n"
      "hb = hb + dt * 0.02 + trig * 0.13; hb = hb - 2 * int(hb / 2); q17 = 1 - abs(1 - hb);\n"
      "sm = sm + (min(max(bass_att + mid_att - 1.6, 0) * 0.25, 0.35) - sm) * (1 - exp(-dt / 0.5)); q19 = sm;\n"
      "jmp = if(trig * above(bs1, 1.0), (rand(100) / 100 - 0.5) * 0.12, jmp * exp(-dt / 0.06));\n"
      "q20 = jmp * above(abs(jmp), 0.002);\n")
presets["nokkvi - pirate signal"] = preset({"decay": 1.0, "wave_a": 0.0, "zoom": 1.0, "wrap": 0},
                                          PF_WARP, PF_COMP, init=PF_INIT, frame=PF_FRAME)

presets["nokkvi - black holes"] = black_holes_port(False)
presets["nokkvi - cover black holes"] = black_holes_port(True)
presets["nokkvi - maxawow"] = maxawow_port(False)
presets["nokkvi - cover maxawow"] = maxawow_port(True)

for name, p in presets.items():
    json.dump(p, open(os.path.join(OUT, name + ".json"), "w"), indent=1)
print(len(presets))
