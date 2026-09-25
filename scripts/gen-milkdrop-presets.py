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
LUM = "vec3(0.299, 0.587, 0.114)"
# Mirror-tiled cover lookup filling any aspect: `c` in 0..1 around the centre.
def cover(v, c, sampler="sampler_fw_cover"):
    return f'''
  vec2 {v}_m = 1.0 - abs(1.0 - mod({c}, 2.0));
  vec3 {v} = texture({sampler}, vec2({v}_m.x, 1.0 - {v}_m.y)).xyz;
'''

def preset(base, warp, comp, init='', frame=''):
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
PULSE = "kick = max(bass - bass_att, 0);\npulse = max(pulse * 0.86, min(kick * 0.9, 1));\nq3 = pulse;\n"

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
  r *= 1.0 + 0.10 * q3;
  float dep = 0.26 / r;
  vec2 t = vec2(a / 6.2831853 * 2.0 + q2 + 0.06 * dep, dep + q1);
  float cl = dot(texture(sampler_fw_cover, vec2(-t.x, -t.y)).xyz, {LUM});
  float lum = clamp((cl - 0.5) * 1.6 + 0.5, 0.0, 1.0);
''' + tone("col", "lum") + '''
  float band = pow(abs(sin((dep + q1) * 4.7123889)), 28.0);
  col += mix(NOKKVI_HIGHLIGHT, NOKKVI_TEXT, q3 * 0.5) * band * (0.25 + 0.9 * q3);
  col += NOKKVI_WARM * band * clamp(treb_att - 1.0, 0.0, 1.0) * 0.5;
  float fog = smoothstep(0.03, 0.42, r0);
  col = mix(NOKKVI_BG, col, fog * fog);
  col += NOKKVI_ACCENT * smoothstep(0.035, 0.0, abs(r0 - 0.07 - 0.05 * q3)) * (0.3 + 0.7 * q3);
  col += NOKKVI_ACCENT * GetBlur1(uv) * 0.9;
  col *= 0.85 + 0.15 * smoothstep(0.95, 0.3, r0);
  ret = col;
 }''',
    init="depth = 0; twist = 0; pulse = 0;",
    frame=PULSE + "depth = depth + 0.005 + 0.012 * min(bass_att, 2.5) + 0.03 * min(kick, 1.5);\n"
          "twist = twist + 0.0015 + 0.003 * (mid_att - 1);\nq1 = depth;\nq2 = twist;\n" + WAVE_THEME)

# 2. Halo -----------------------------------------------------------------
halo_box = "float box = 0.30 + 0.02 * clamp(bass_att, 0.0, 2.0) + 0.03 * q3;"
presets["nokkvi - cover halo"] = preset(
    {"zoom": 1.02, "rot": 0.0, "warp": 0.15, "decay": 0.97, "wave_mode": 0, "wave_a": 0.3, "wave_scale": 0.6},
    "uniform sampler2D sampler_fc_cover;\n shader_body {\n" + HEAD + f'''
  {halo_box}
  vec2 p = (uv_orig - 0.5) * s;
  float d = max(abs(p.x), abs(p.y));
  vec3 cov = texture(sampler_fc_cover, vec2(0.5, 0.5) + vec2(1.0, -1.0) * p / (2.0 * box)).xyz;
  float rim = (1.0 - step(box, d)) * smoothstep(box - 0.05, box, d);
  vec3 fb = texture(sampler_main, uv).xyz * 0.955 - 0.003;
  fb += cov * rim * (0.22 + 0.5 * q3);
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
  vec3 cov = texture(sampler_fc_cover, vec2(0.5, 0.5) + vec2(1.0, -1.0) * p / (2.0 * box)).xyz;
  float inside = 1.0 - smoothstep(box - 0.003, box, d);
  col = mix(col, cov, inside);
  float frame = smoothstep(0.010, 0.0, abs(d - box));
  col += NOKKVI_HIGHLIGHT * frame * (0.35 + 0.65 * max(q3, clamp(bass_att - 0.8, 0.0, 1.0)));
  ret = col;
 }}''',
    init="pulse = 0;",
    frame=PULSE + "rot = 0.006 * sin(time * 0.3) + 0.01 * q3;\nzoom = 1.015 + 0.02 * q3;\n" + WAVE_THEME)

# 3. Kaleido --------------------------------------------------------------
def kal(uvname, extra=""):
    return f'''
  vec2 p = ({uvname} - 0.5) * s;
  float r = length(p);
  float seg = 6.2831853 / 6.0;
  float a = mod(atan(p.y, p.x) + q1, seg);
  a = abs(a - seg * 0.5);
  vec2 k = vec2(cos(a), sin(a)) * r * (0.75 - 0.08 * clamp(bass_att, 0.0, 2.0) - 0.08 * q3);
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
  ret = col;
 }''',
    init="phase = 0; orbit = 0; pulse = 0;",
    frame=PULSE + "phase = phase + 0.002 + 0.004 * min(bass_att, 2) + 0.01 * q3;\norbit = orbit + 0.0015 + 0.002 * min(mid_att, 2);\nq1 = phase;\nq2 = orbit;")

# 4. Ripple (new) ---------------------------------------------------------
ripple_frame = PULSE + '''dt = 1 / max(fps, 1);
a1 = a1 + dt; a2 = a2 + dt; a3 = a3 + dt;
cool = max(cool - dt, 0);
spawn = above(kick, 0.3) * below(cool, 0.001);
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
  vec2 c = p * 0.62 + off * 0.055 + vec2(0.5 + 0.12 * sin(q13 * 2.0), 0.5 + 0.12 * cos(q13 * 1.3));
''' + cover("cov", "c") + f'''
  float lum = clamp((dot(cov, {LUM}) - 0.5) * 1.5 + 0.5, 0.0, 1.0);
''' + tone("col", "lum") + '''
  col = mix(col, NOKKVI_HIGHLIGHT, clamp(crest, 0.0, 1.0) * 0.6);
  col += NOKKVI_TEXT * pow(clamp(crest, 0.0, 1.0), 3.0) * 0.45;
  col = mix(col, NOKKVI_BG, clamp(-crest, 0.0, 1.0) * 0.35);
  col *= 0.8 + 0.2 * smoothstep(1.0, 0.3, length(p));
  ret = col;
 }''',
    init="pulse = 0; a1 = 9; a2 = 9; a3 = 9; slot = 0; cool = 0; drift = 0; x1 = 0; y1 = 0; x2 = 0; y2 = 0; x3 = 0; y3 = 0;",
    frame=ripple_frame)

# Orb: the cover wrapped on a spinning, lit sphere; kicks send rings outward.
orb_r = "float R = 0.31 + 0.035 * q3 + 0.01 * clamp(bass_att, 0.0, 2.0);"
presets["nokkvi - cover orb"] = preset(
    {"zoom": 1.025, "rot": 0.004, "decay": 1.0},
    " shader_body {\n" + HEAD + f"""
  {orb_r}
  vec2 p = (uv_orig - 0.5) * s;
  float d = length(p);
  vec3 fb = texture(sampler_main, uv).xyz * 0.925 - 0.003;
  float ring = smoothstep(0.008, 0.0, abs(d - R - 0.006));
  fb += NOKKVI_ACCENT * ring * (0.05 + 0.75 * q3);
  ret = max(fb, vec3(0.0));
 }}""",
    "uniform sampler2D sampler_fw_cover;\n shader_body {\n" + HEAD + f"""
  {orb_r}
  vec2 p = (uv - 0.5) * s;
  float d2 = dot(p, p);
  vec3 trail = texture(sampler_main, uv).xyz + GetBlur1(uv) * 0.9;
  float tl = clamp(dot(trail, {LUM}) * 1.4, 0.0, 1.0);
""" + tone("bg", "tl") + f"""
  vec3 col = bg;
  float edge = smoothstep(R * R, (R - 0.004) * (R - 0.004), d2);
  if (d2 < R * R) {{
    vec3 n = vec3(p.x, -p.y, sqrt(max(R * R - d2, 0.0))) / R;
    float ct = cos(0.35); float st = sin(0.35);
    n = vec3(n.x, n.y * ct - n.z * st, n.y * st + n.z * ct);
    float cs = cos(q1); float sn = sin(q1);
    vec3 m = vec3(n.x * cs + n.z * sn, n.y, -n.x * sn + n.z * cs);
    float lon = atan(m.x, m.z);
    float lat = asin(clamp(m.y, -1.0, 1.0));
    vec3 cov = texture(sampler_fw_cover, vec2(lon / 3.14159265 + 0.5, 0.5 - lat / 3.14159265)).xyz;
    vec3 L = normalize(vec3(-0.45, 0.55, 0.75));
    vec3 nv = vec3(p.x, -p.y, sqrt(max(R * R - d2, 0.0))) / R;
    float diff = max(dot(nv, L), 0.0);
    float spec = pow(max(dot(reflect(-L, nv), vec3(0.0, 0.0, 1.0)), 0.0), 28.0);
    float rim = pow(1.0 - nv.z, 3.0);
    vec3 lit = cov * (0.22 + 0.9 * diff) + NOKKVI_TEXT * spec * 0.55;
    lit += NOKKVI_HIGHLIGHT * rim * (0.5 + 0.8 * q3);
    col = mix(bg, lit, edge);
  }}
  col += NOKKVI_WARM * smoothstep(0.02, 0.0, abs(sqrt(d2) - R)) * clamp(treb_att - 1.0, 0.0, 1.0) * 0.4;
  ret = col;
 }}""",
    init="pulse = 0; spin = 0;",
    frame=PULSE + "spin = spin + 0.006 + 0.008 * min(mid_att, 2) + 0.03 * q3;\nq1 = spin;")

# Starfield: warp-speed streaks in the theme's gradient; no cover.
presets["nokkvi - starfield"] = preset(
    {"zoom": 1.045, "rot": 0.0, "decay": 1.0},
    " shader_body {\n" + HEAD + """
  vec2 g = vec2(110.0, 62.0);
  vec2 cell = floor(uv_orig * g);
  float h = fract(sin(dot(cell + floor(time * 7.0) * vec2(3.1, 1.7), vec2(12.9898, 78.233))) * 43758.5453);
  float star = step(0.9955 - 0.004 * q3, h);
  vec2 f = fract(uv_orig * g) - 0.5;
  star *= smoothstep(0.45, 0.0, length(f));
  // Sample halfway back along the zoom too, so a star's per-frame jumps
  // join into one streak instead of a dotted line.
  vec3 fb = max(
    texture(sampler_main, uv).xyz,
    max(texture(sampler_main, mix(uv, uv_orig, 0.33)).xyz, texture(sampler_main, mix(uv, uv_orig, 0.66)).xyz)
  ) * (0.9 - 0.04 * q3);
  fb += vec3(star) * (0.7 + 0.8 * q3) * smoothstep(0.02, 0.2, length((uv_orig - 0.5) * s));
  ret = fb;
 }""",
    " shader_body {\n" + HEAD + f"""
  vec2 p = (uv - 0.5) * s;
  float r = length(p);
  vec3 m = texture(sampler_main, uv).xyz + GetBlur1(uv) * 0.6;
  float lum = clamp(dot(m, {LUM}) * 1.5, 0.0, 1.0);
""" + ramp("hue", "clamp(r * 1.4 + 0.15 * sin(q1), 0.0, 1.0)") + f"""
  vec3 col = mix(NOKKVI_BG, hue, smoothstep(0.0, 0.45, lum));
  col = mix(col, NOKKVI_TEXT, smoothstep(0.75, 1.0, lum) * 0.7);
  col += NOKKVI_ACCENT * exp(-r * 9.0) * (0.15 + 0.5 * q3);
  col += NOKKVI_WARM * smoothstep(0.85, 1.0, lum) * q3 * 0.3;
  ret = col;
 }}""",
    init="pulse = 0; phase = 0;",
    frame=PULSE + "phase = phase + 0.01 + 0.02 * min(mid_att, 2);\nq1 = phase;\nzoom = 1.035 + 0.02 * min(bass_att, 2) + 0.05 * q3;\nrot = 0.004 * sin(time * 0.2);")

# 6. Aurora (new; no cover) -----------------------------------------------
presets["nokkvi - aurora"] = preset(
    {"decay": 1.0, "wave_mode": 6, "additivewave": 1, "wave_a": 0.9, "wave_scale": 1.2, "wave_smoothing": 0.6, "wave_thick": 1, "wave_y": 0.55},
    " shader_body {\n" + HEAD + '''
  vec2 p = (uv - 0.5);
  vec2 flow = vec2(sin(p.y * 6.0 + time * 0.35 + q1), cos(p.x * 5.0 - time * 0.27));
  vec2 src = uv - vec2(0.0, 0.0025) + flow * 0.0022 * (0.6 + clamp(mid_att, 0.0, 2.0));
  vec3 fb = texture(sampler_main, src).xyz * (0.976 - 0.01 * q3);
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
  ret = col;
 }}''',
    init="pulse = 0; phase = 0;",
    frame=PULSE + "phase = phase + 0.01 + 0.03 * min(bass_att, 2);\nq1 = phase;\n"
          "wave_y = 0.5 + 0.12 * sin(time * 0.21);\nwave_r = NOKKVI_TEXT_R;\nwave_g = NOKKVI_TEXT_G;\nwave_b = NOKKVI_TEXT_B;\nwave_a = 0.5 + 0.5 * min(bass_att, 1);")

for name, p in presets.items():
    json.dump(p, open(os.path.join(OUT, name + ".json"), "w"), indent=1)
print(len(presets))
