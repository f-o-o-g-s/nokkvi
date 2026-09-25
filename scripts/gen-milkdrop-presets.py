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
  vec2 t = vec2(a / 6.2831853 * 2.0 + q2 + 0.06 * dep, dep + q1);
  float cl = dot(texture(sampler_fw_cover, vec2(t.x, -t.y)).xyz, {LUM});
  float ca = dot(texture(sampler_fw_cover, vec2(t.x + 0.012, -t.y)).xyz, {LUM});
  float cd = dot(texture(sampler_fw_cover, vec2(t.x, -t.y - 0.02)).xyz, {LUM});
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
  float hoop = texture(sampler_main, hu).x * step(hu.y, 1.0);
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
# A gas giant drifts past in the foreground (faster when the ship surges),
# banded from the theme's gradient, lit by a fixed sun with a night side and
# an atmosphere rim that breathes with the bass; most carry a tilted ring
# whose far half hides behind the planet. When one leaves the screen the next
# rolls new size, height, ring, tilt and bands.
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
  vec2 pd = p - vec2(q12, q13);
  float pr = q14;
  float pl = length(pd);
  vec3 pn = vec3(pd.x, pd.y, sqrt(max(pr * pr - pl * pl, 0.0))) / pr;
  vec3 sun = normalize(vec3(0.6, 0.45, 0.55));
  float lam = max(dot(pn, sun), 0.0);
  float lat = asin(clamp(pn.y, -1.0, 1.0));
  float lon = atan(pn.x, pn.z) + q18;
  float pb1 = texture(sampler_noise_hq, vec2(lon * 0.08 + q17, lat * 0.9 + q17 * 0.3)).x;
  float pb2 = texture(sampler_noise_hq, vec2(lon * 0.25 + q18 * 0.3, lat * 2.5)).x;
  float bandv = clamp(pb1 * 0.7 + pb2 * 0.3, 0.0, 1.0);
""" + ramp("pcol", "bandv") + """
  vec3 planet = pcol * (0.08 + 0.95 * lam) + NOKKVI_TEXT * pow(lam, 8.0) * 0.15;
  float atmo = pow(1.0 - pn.z, 4.0);
  planet += NOKKVI_HIGHLIGHT * atmo * (0.3 + 0.3 * q3) * (0.3 + 0.7 * lam);
  float pmask = smoothstep(pr, pr - 0.003, pl);
  col = mix(col, planet, pmask);
  col += NOKKVI_HIGHLIGHT * smoothstep(pr * 1.25, pr, pl) * step(pr, pl) * (0.08 + 0.08 * q3);
  vec2 rc = vec2(pd.x, pd.y / q16);
  float rr = length(rc) / pr;
  float ringband = texture(sampler_noise_hq, vec2(rr * 0.35 + q17, 0.5)).x;
  float ringm = smoothstep(1.35, 1.45, rr) * smoothstep(2.3, 2.1, rr) * q15;
  ringm *= max(step(0.0, -pd.y), step(pr, pl));
  float sunside = clamp(dot(normalize(pd), sun.xy) * 0.5 + 0.5, 0.0, 1.0);
  vec3 ringcol = mix(NOKKVI_SURFACE, pcol, 0.5) * ringband * (0.35 + 0.5 * sunside) + NOKKVI_TEXT * ringband * 0.1;
  col = mix(col, ringcol, ringm * (0.55 + 0.35 * ringband));
  col *= 0.9 + 0.1 * smoothstep(1.1, 0.2, length(p));
  col += (grain - 0.5) * 0.012;
  ret = col;
 }""",
    init="pulse = 0; pop = 0; travel = 0; roll = 0; speed = 0; twist = 0.015; spiral = 0; "
         "px = 0.35; py = 0.08; prad = 0.16; pring = 1; ptilt = 0.3; pcol = 0.35; pspin = 0;",
    frame=PULSE + "speed = speed * 0.9 + 0.1 * (0.012 + 0.03 * min(bass_att, 2) + 0.12 * q3 + 0.1 * pop);\n"
          "travel = travel + speed * 0.25;\nroll = roll + 0.0012 * (mid_att - 0.8) + 0.004 * q3 * sign(sin(time * 0.05));\n"
          "q1 = travel;\nq2 = speed * 6;\nq4 = roll;\n"
          "twist = twist * 0.97 + 0.03 * (0.02 * sin(time * 0.11) + 0.012 * (mid_att - 1) + 0.03 * pop * sign(sin(time * 0.11)));\n"
          "spiral = spiral + twist * 60 * speed;\nq10 = twist;\nq11 = spiral;\n"
          "dt = 1 / max(fps, 1);\n"
          "px = px - (0.03 + 0.25 * speed) * dt;\n"
          "lim = 0.5 / aspectx + prad + 0.1;\n"
          "wrap = below(px, -lim);\n"
          "prad = if(wrap, 0.1 + 0.12 * rand(100) / 100, prad);\n"
          "px = if(wrap, 0.5 / aspectx + prad + 0.1, px);\n"
          "py = if(wrap, -0.28 + 0.56 * rand(100) / 100, py);\n"
          "pring = if(wrap, above(rand(100), 35), pring);\n"
          "ptilt = if(wrap, 0.18 + 0.3 * rand(100) / 100, ptilt);\n"
          "pcol = if(wrap, rand(100) / 100, pcol);\n"
          "pspin = pspin + dt * 0.05;\n"
          "q12 = px;\nq13 = py;\nq14 = prad;\nq15 = pring;\nq16 = ptilt;\nq17 = pcol;\nq18 = pspin;")

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

for name, p in presets.items():
    json.dump(p, open(os.path.join(OUT, name + ".json"), "w"), indent=1)
print(len(presets))
