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

def preset(base, warp, comp, init='', frame=''):
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
    return {"version": 2, "baseVals": b, "shapes": [offs]*4, "waves": [off]*4,
            "init_eqs_eel": init, "frame_eqs_eel": frame, "pixel_eqs_eel": "",
            "warp": warp, "comp": comp}

WAVE_THEME = "wave_r = NOKKVI_HIGHLIGHT_R;\nwave_g = NOKKVI_HIGHLIGHT_G;\nwave_b = NOKKVI_HIGHLIGHT_B;\n"
# Beat envelope in q3: jumps on a kick, decays over ~0.3 s.
# q3: a kick envelope (bass jumping above its follower), ~0.3 s.
# q5: an instant pop from the raw bass level, ~0.15 s: brightness flashes and
#     punches hit on the beat instead of easing in.
PULSE = ("kick = max(bass - bass_att, 0);\npulse = max(pulse * 0.86, min(kick * 0.9, 1));\nq3 = pulse;\n"
         "pop = max(pop * 0.78, min(max(bass - 1.2, 0) * 0.55, 1.4));\nq5 = pop;\n")

presets = {}

# 1. Tunnel ---------------------------------------------------------------
presets["nokkvi - cover tunnel"] = preset(
    {"zoom": 1.03, "rot": 0.02, "decay": 0.93, "wave_mode": 0, "wave_a": 0.7, "wave_scale": 0.45, "wave_smoothing": 0.5},
    "",
    "uniform sampler2D sampler_fw_cover;\n shader_body {\n" + HEAD + f'''
  vec2 p = (uv - 0.5) * s;
  p += vec2(0.07 * sin(time * 0.23), 0.06 * cos(time * 0.19));
  float r0 = max(length(p), 0.002);
  float a = atan(p.y, p.x);
  float r = r0 * (1.0 + 0.07 * sin(a * 3.0 + q1 * 1.7) * clamp(mid_att, 0.0, 2.0));
  r *= 1.0 + 0.10 * q3 + 0.08 * q5;
  float dep = 0.26 / r;
  vec2 t = vec2(a / 6.2831853 * 2.0 + q2 + 0.06 * dep, dep + q1);
  float cl = dot(texture(sampler_fw_cover, vec2(t.x, -t.y)).xyz, {LUM});
  float lum = clamp((cl - 0.5) * 1.6 + 0.5, 0.0, 1.0);
''' + tone("col", "lum") + '''
  float band = pow(abs(sin((dep + q1) * 4.7123889)), 28.0);
  col += mix(NOKKVI_HIGHLIGHT, NOKKVI_TEXT, q3 * 0.5) * band * (0.25 + 0.9 * q3 + 0.8 * q5);
  col += NOKKVI_WARM * band * clamp(treb_att - 1.0, 0.0, 1.0) * 0.5;
  float fog = smoothstep(0.03, 0.42, r0);
  col = mix(NOKKVI_BG, col, fog * fog);
  col += NOKKVI_ACCENT * smoothstep(0.035, 0.0, abs(r0 - 0.07 - 0.05 * q3 - 0.04 * q5)) * (0.3 + 0.7 * q3 + 0.6 * q5);
  col *= 1.0 + 0.45 * q5;
  col += NOKKVI_ACCENT * GetBlur1(uv) * 0.9;
  col *= 0.85 + 0.15 * smoothstep(0.95, 0.3, r0);
  ret = col;
 }''',
    init="depth = 0; twist = 0; pulse = 0; pop = 0;",
    frame=PULSE + "depth = depth + 0.005 + 0.012 * min(bass_att, 2.5) + 0.03 * min(kick, 1.5) + 0.02 * pop;\n"
          "twist = twist + 0.0015 + 0.003 * (mid_att - 1);\nq1 = depth;\nq2 = twist;\n" + WAVE_THEME)

# 2. Halo -----------------------------------------------------------------
halo_box = "float box = 0.30 + 0.02 * clamp(bass_att, 0.0, 2.0) + 0.03 * q3 + 0.035 * q5;"
presets["nokkvi - cover halo"] = preset(
    {"zoom": 1.02, "rot": 0.0, "warp": 0.15, "decay": 0.97, "wave_mode": 0, "wave_a": 0.3, "wave_scale": 0.6},
    "uniform sampler2D sampler_fc_cover;\n shader_body {\n" + HEAD + f'''
  {halo_box}
  vec2 p = (uv_orig - 0.5) * s;
  float d = max(abs(p.x), abs(p.y));
  vec3 cov = texture(sampler_fc_cover, vec2(0.5, 0.5) + vec2(1.0, -1.0) * p / (2.0 * box)).xyz;
  float rim = (1.0 - step(box, d)) * smoothstep(box - 0.05, box, d);
  vec3 fb = texture(sampler_main, uv).xyz * 0.955 - 0.003;
  fb += cov * rim * (0.22 + 0.5 * q3 + 0.45 * q5);
  ret = max(fb, vec3(0.0));
 }}''',
    "uniform sampler2D sampler_fc_cover;\n shader_body {\n" + HEAD + f'''
  {halo_box}
  vec2 p = (uv - 0.5) * s;
  float d = max(abs(p.x), abs(p.y));
  vec3 fb = texture(sampler_main, uv).xyz + GetBlur1(uv) * 0.8;
  float lum = clamp(dot(fb, {LUM}) * 1.3, 0.0, 1.0);
''' + tone("col", "lum") + f'''
  col += NOKKVI_WARM * smoothstep(0.75, 1.0, lum) * clamp(treb_att - 0.8, 0.0, 1.0) * 0.35;
  col *= 1.0 + 0.4 * q5;
  vec3 cov = texture(sampler_fc_cover, vec2(0.5, 0.5) + vec2(1.0, -1.0) * p / (2.0 * box)).xyz;
  float inside = 1.0 - smoothstep(box - 0.003, box, d);
  col = mix(col, cov, inside);
  float frame = smoothstep(0.010, 0.0, abs(d - box));
  col += NOKKVI_HIGHLIGHT * frame * (0.35 + 0.65 * max(q3, clamp(bass_att - 0.8, 0.0, 1.0)) + 0.9 * q5);
  ret = col;
 }}''',
    init="pulse = 0; pop = 0;",
    frame=PULSE + "rot = 0.006 * sin(time * 0.3) + 0.01 * q3;\nzoom = 1.015 + 0.02 * q3;\n" + WAVE_THEME)

# 3. Kaleido --------------------------------------------------------------
def kal(uvname, extra=""):
    return f'''
  vec2 p = ({uvname} - 0.5) * s;
  float r = length(p);
  float seg = 6.2831853 / 6.0;
  float a = mod(atan(p.y, p.x) + q1, seg);
  a = abs(a - seg * 0.5);
  vec2 k = vec2(cos(a), sin(a)) * r * (0.75 - 0.08 * clamp(bass_att, 0.0, 2.0) - 0.08 * q3 - 0.12 * q5);
  k += vec2(0.5, 0.5) + 0.24 * vec2(cos(q2), sin(q2 * 0.7));
  float cl = dot(texture(sampler_fw_cover, vec2(k.x, 1.0 - k.y)).xyz, {LUM});
  float lum = clamp((cl - 0.5) * 1.9 + 0.5, 0.0, 1.0);
'''
presets["nokkvi - cover kaleido"] = preset(
    {"zoom": 0.985, "rot": 0.01, "decay": 0.97},
    "uniform sampler2D sampler_fw_cover;\n shader_body {\n" + HEAD + kal("uv_orig") + '''
  vec3 fb = texture(sampler_main, uv).xyz * 0.88;
  ret = max(fb, vec3((1.0 - lum) * 0.5 * clamp(bass_att - 0.7, 0.0, 1.5)));
 }''',
    "uniform sampler2D sampler_fw_cover;\n shader_body {\n" + HEAD + kal("uv") + tone("col", "lum") + '''
  col += NOKKVI_HIGHLIGHT * GetBlur1(uv) * 0.7;
  col += NOKKVI_WARM * smoothstep(0.85, 1.0, lum) * clamp(treb_att - 0.9, 0.0, 1.0) * 0.35;
  col += NOKKVI_TEXT * smoothstep(0.012, 0.0, abs(r - 0.35 - 0.2 * q3)) * q3 * 0.5;
  col *= 0.7 + 0.3 * smoothstep(0.9, 0.15, r);
  col *= 1.0 + 0.4 * q5;
  ret = col;
 }''',
    init="phase = 0; orbit = 0; pulse = 0; pop = 0;",
    frame=PULSE + "phase = phase + 0.002 + 0.004 * min(bass_att, 2) + 0.01 * q3 + 0.012 * pop;\norbit = orbit + 0.0015 + 0.002 * min(mid_att, 2);\nq1 = phase;\nq2 = orbit;")

# 4. Ripple (new) ---------------------------------------------------------
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
'''
def wave(i, x, y, a):
    return f'''
  vec2 d{i} = p - vec2({x}, {y});
  float l{i} = max(length(d{i}), 0.0001);
  float f{i} = l{i} - {a} * 0.45;
  float w{i} = sin(f{i} * 38.0) * exp(-abs(f{i}) * 7.0) * exp(-{a} * 1.1);
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
''' + wave(1, "q4", "q5", "q6") + wave(2, "q7", "q8", "q9") + wave(3, "q10", "q11", "q12") + '''
  vec2 c = p * 0.62 + off * 0.055 * (1.0 + 0.8 * q5) + vec2(0.5 + 0.12 * sin(q13 * 2.0), 0.5 + 0.12 * cos(q13 * 1.3));
''' + cover("cov", "c") + f'''
  float lum = clamp((dot(cov, {LUM}) - 0.5) * 1.5 + 0.5, 0.0, 1.0);
''' + tone("col", "lum") + '''
  col = mix(col, NOKKVI_HIGHLIGHT, clamp(crest, 0.0, 1.0) * 0.6);
  col += NOKKVI_TEXT * pow(clamp(crest, 0.0, 1.0), 3.0) * (0.45 + 0.5 * q5);
  col = mix(col, NOKKVI_BG, clamp(-crest, 0.0, 1.0) * 0.35);
  col *= 0.8 + 0.2 * smoothstep(1.0, 0.3, length(p));
  col *= 1.0 + 0.3 * q5;
  ret = col;
 }''',
    init="pulse = 0; pop = 0; a1 = 9; a2 = 9; a3 = 9; slot = 0; cool = 0; drift = 0; x1 = 0; y1 = 0; x2 = 0; y2 = 0; x3 = 0; y3 = 0;",
    frame=ripple_frame)

# Orb: the cover on a spinning, lit sphere that sheds its colours as paint:
# a thin rim feeds the feedback, which curls away through a noise flow field
# (sharpened against its blur so strokes stay crisp), and the sphere's edge
# melts into it. Kicks shed more paint and swell the orb.
orb_r = "float R = 0.26 + 0.03 * q3 + 0.035 * q5 + 0.01 * clamp(bass_att, 0.0, 2.0);"
ORB_SPHERE = """
    vec3 n = vec3(p.x, -p.y, sqrt(max(R * R - d2, 0.0))) / R;
    float ct = cos(0.35); float st = sin(0.35);
    vec3 nt = vec3(n.x, n.y * ct - n.z * st, n.y * st + n.z * ct);
    float cs = cos(q1); float sn = sin(q1);
    vec3 m = vec3(nt.x * cs + nt.z * sn, nt.y, -nt.x * sn + nt.z * cs);
    float lon = atan(m.x, m.z);
    float lat = asin(clamp(m.y, -1.0, 1.0));
    vec3 cov = texture(sampler_fw_cover, vec2(lon / 3.14159265 + 0.5, 0.5 + lat / 3.14159265)).xyz;
"""
presets["nokkvi - cover orb"] = preset(
    {"zoom": 1.0, "rot": 0.0, "decay": 1.0},
    "uniform sampler2D sampler_fw_cover;\n shader_body {\n" + HEAD + f"""
  {orb_r}
  vec2 p = (uv_orig - 0.5) * s;
  float d = length(p);
  // Flow: two octaves of noise steer the paint; a gentle swirl around the orb
  // and an outward drift carry it away from the source.
  vec2 nuv = uv_orig * 0.45 + vec2(q2 * 0.02, -q2 * 0.013);
  float flow_ang = (texture(sampler_noise_hq, nuv).x + 0.5 * texture(sampler_noise_hq, nuv * 2.1 + 0.3).x) * 9.0;
  vec2 flow = vec2(cos(flow_ang), sin(flow_ang)) * (0.0026 + 0.0026 * clamp(mid_att, 0.0, 2.0));
  vec2 swirl = vec2(-p.y, p.x) / max(d, 0.08) * 0.0018;
  vec2 outward = p / max(d, 0.05) * (0.0008 + 0.004 * q3 + 0.006 * q5);
  vec2 src = uv - (flow + swirl + outward) / s;
  vec3 fb = texture(sampler_main, src).xyz;
  // Paint diffuses a little as it travels (soft, blended strokes), then fades.
  fb = mix(fb, GetBlur1(src), 0.1);
  fb = fb * 0.992 - 0.001;
  // The rim sheds the cover's colours into the flow.
  float d2 = dot(p, p);
  float band = smoothstep(0.05, 0.0, abs(d - R * 0.96));
  if (d < R) {{
""" + ORB_SPHERE + f"""
    fb = mix(fb, cov, band * (0.12 + 0.4 * q3 + 0.5 * q5));
  }}
  ret = clamp(fb, 0.0, 1.0);
 }}""",
    "uniform sampler2D sampler_fw_cover;\n shader_body {\n" + HEAD + f"""
  {orb_r}
  vec2 p = (uv - 0.5) * s;
  float d2 = dot(p, p);
  float d = sqrt(d2);
  // The paint field: the cover's own colours, sunk towards the theme's
  // background where thin and lifted by the theme's highlight where dense.
  vec3 paint = texture(sampler_main, uv).xyz;
  vec3 halo = GetBlur2(uv);
  float pl = dot(paint, {LUM});
  vec3 col = mix(NOKKVI_BG, paint, smoothstep(0.0, 0.25, pl + 0.15));
  col += NOKKVI_HIGHLIGHT * dot(halo, {LUM}) * 0.25;
  // A wobbling edge, so the orb melts into its paint.
  float wob = (texture(sampler_noise_hq, p * 0.7 + q2 * 0.006).x - 0.5) * 0.022;
  float Rw = R + wob;
  if (d < Rw) {{
""" + ORB_SPHERE + f"""
    vec3 L = normalize(vec3(-0.45, 0.55, 0.75));
    float diff = max(dot(n, L), 0.0);
    float spec = pow(max(dot(reflect(-L, n), vec3(0.0, 0.0, 1.0)), 0.0), 28.0);
    float rim = pow(1.0 - n.z, 3.0);
    vec3 lit = cov * (0.25 + 0.85 * diff) + NOKKVI_TEXT * spec * 0.5;
    lit += NOKKVI_HIGHLIGHT * rim * (0.35 + 0.6 * q3 + 0.8 * q5);
    float edge = smoothstep(Rw, Rw - 0.02, d);
    col = mix(col, lit, edge);
  }}
  col += NOKKVI_WARM * smoothstep(0.02, 0.0, abs(d - R)) * clamp(treb_att - 1.0, 0.0, 1.0) * 0.3;
  ret = col;
 }}""",
    init="pulse = 0; pop = 0; spin = 0; drift = 0;",
    frame=PULSE + "spin = spin + 0.005 + 0.006 * min(mid_att, 2) + 0.02 * q3;\n"
          "drift = drift + 0.01 + 0.02 * min(bass_att, 2);\nq1 = spin;\nq2 = drift;")

# Starfield: 3D star layers flown through with parallax, drawn into the
# feedback so every star leaves a motion trail that zooms outward (longer at
# speed); each star is tied to a frequency and flares with it; a soft nebula
# in the theme's gradient sits behind; kicks surge the speed.
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
  // Nebula: domain-warped sine layers. Smooth everywhere, so no texel grid
  // can show (a snapped noise texture left square patches).
  vec2 w = p * 2.2 + vec2(q1 * 0.35, q1 * 0.2);
  w += 0.7 * vec2(sin(w.y * 1.7 + q1 * 0.3), sin(w.x * 1.3 - q1 * 0.2));
  w += 0.35 * vec2(sin(w.y * 3.1 + 1.0), sin(w.x * 2.9 + 2.0));
  float n = 0.5 + 0.25 * sin(w.x * 1.1 + w.y * 0.7)
          + 0.15 * sin(-w.x * 0.6 + w.y * 1.9 + 1.3)
          + 0.1 * sin(w.x * 2.3 - w.y * 2.1 + 0.4);
  float neb = pow(smoothstep(0.5, 0.95, n), 1.6) * (0.3 + 0.3 * clamp(bass_att, 0.0, 2.0) + 0.5 * q5);
""" + ramp("nc", "n") + """
  float grain = texture(sampler_noise_lq, uv * texsize.xy / 256.0 + rand_frame.xy).x;
  vec3 col = NOKKVI_BG + nc * neb * 0.55;
  col += texture(sampler_main, uv).xyz + GetBlur1(uv) * 0.35;
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
""")

# 6. Aurora (new; no cover) -----------------------------------------------
presets["nokkvi - aurora"] = preset(
    {"decay": 1.0, "wave_mode": 6, "additivewave": 1, "wave_a": 0.9, "wave_scale": 1.2, "wave_smoothing": 0.6, "wave_thick": 1, "wave_y": 0.55},
    " shader_body {\n" + HEAD + '''
  vec2 p = (uv - 0.5);
  vec2 flow = vec2(sin(p.y * 6.0 + time * 0.35 + q1), cos(p.x * 5.0 - time * 0.27));
  vec2 src = uv - vec2(0.0, 0.0025) + flow * 0.0022 * (0.6 + clamp(mid_att, 0.0, 2.0));
  vec3 fb = texture(sampler_main, src).xyz * (0.976 - 0.01 * q3);
  float spec = get_fft(0.02 + uv_orig.x * 0.55);
  float line = 0.62 - spec * 0.35;
  fb += vec3(smoothstep(0.018, 0.0, abs(uv_orig.y - line)) * spec * (0.5 + 0.6 * q5));
  fb += (GetBlur1(src) - fb) * 0.08;
  ret = max(fb - 0.002, vec3(0.0));
 }''',
    " shader_body {\n" + HEAD + f'''
  vec3 m = texture(sampler_main, uv).xyz + GetBlur2(uv) * 0.9;
  float lum = clamp(dot(m, {LUM}) * 1.2, 0.0, 1.0);
  float hue = abs(fract(uv.x * 0.5 + uv.y * 0.25 + q1 * 0.04) * 2.0 - 1.0);
''' + ramp("band", "hue") + f'''
  vec3 col = mix(NOKKVI_BG, band, smoothstep(0.0, 0.5, lum));
  col = mix(col, NOKKVI_TEXT, smoothstep(0.8, 1.0, lum) * 0.6);
  col += NOKKVI_WARM * smoothstep(0.9, 1.0, lum) * clamp(treb_att - 1.0, 0.0, 1.0) * 0.4;
  col = mix(NOKKVI_BG, col, 0.85 + 0.15 * q3);
  col *= 1.0 + 0.3 * q5;
  ret = col;
 }}''',
    init="pulse = 0; pop = 0; phase = 0;",
    frame=PULSE + "phase = phase + 0.01 + 0.03 * min(bass_att, 2);\nq1 = phase;\n"
          "wave_y = 0.5 + 0.12 * sin(time * 0.21);\nwave_r = NOKKVI_TEXT_R;\nwave_g = NOKKVI_TEXT_G;\nwave_b = NOKKVI_TEXT_B;\nwave_a = 0.5 + 0.5 * min(bass_att, 1);")

for name, p in presets.items():
    json.dump(p, open(os.path.join(OUT, name + ".json"), "w"), indent=1)
print(len(presets))
