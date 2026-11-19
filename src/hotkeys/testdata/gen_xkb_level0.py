"""Regenerate xkb_level0.json: what each physical key types with no modifiers
on 30 real XKB layouts, the table `hotkeys::layout::xkb_tests` runs the default
hotkeys over.

    python3 src/hotkeys/testdata/gen_xkb_level0.py > src/hotkeys/testdata/xkb_level0.json

Needs libxkbcommon and xkeyboard-config (Arch: both come with any desktop). It
mirrors winit's `key_without_modifiers` on Linux, which is what iced reports as
`KeyPressed.key`: xkb_keymap_key_get_syms_by_level(layout 0, level 0), one
keysym, then keysym -> UTF-8. A dead_* keysym is stored as null (winit
Key::Dead, which iced reports as Key::Unidentified).
"""
import ctypes, ctypes.util, json, sys

xkb = ctypes.CDLL(ctypes.util.find_library("xkbcommon"))

class RuleNames(ctypes.Structure):
    _fields_ = [(n, ctypes.c_char_p) for n in ("rules", "model", "layout", "variant", "options")]

xkb.xkb_context_new.restype = ctypes.c_void_p
xkb.xkb_context_new.argtypes = [ctypes.c_int]
xkb.xkb_keymap_new_from_names.restype = ctypes.c_void_p
xkb.xkb_keymap_new_from_names.argtypes = [ctypes.c_void_p, ctypes.POINTER(RuleNames), ctypes.c_int]
xkb.xkb_keymap_key_get_syms_by_level.restype = ctypes.c_int
xkb.xkb_keymap_key_get_syms_by_level.argtypes = [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_uint32, ctypes.POINTER(ctypes.POINTER(ctypes.c_uint32))]
xkb.xkb_keysym_get_name.argtypes = [ctypes.c_uint32, ctypes.c_char_p, ctypes.c_size_t]
xkb.xkb_keysym_to_utf8.argtypes = [ctypes.c_uint32, ctypes.c_char_p, ctypes.c_size_t]
xkb.xkb_keymap_unref.argtypes = [ctypes.c_void_p]

# W3C code -> evdev KEY_* number (xkb keycode = evdev + 8)
EVDEV = {
    "Backquote": 41, "Digit1": 2, "Digit2": 3, "Digit3": 4, "Digit4": 5, "Digit5": 6,
    "Digit6": 7, "Digit7": 8, "Digit8": 9, "Digit9": 10, "Digit0": 11, "Minus": 12, "Equal": 13,
    "KeyQ": 16, "KeyW": 17, "KeyE": 18, "KeyR": 19, "KeyT": 20, "KeyY": 21, "KeyU": 22,
    "KeyI": 23, "KeyO": 24, "KeyP": 25, "BracketLeft": 26, "BracketRight": 27,
    "KeyA": 30, "KeyS": 31, "KeyD": 32, "KeyF": 33, "KeyG": 34, "KeyH": 35, "KeyJ": 36,
    "KeyK": 37, "KeyL": 38, "Semicolon": 39, "Quote": 40, "Backslash": 43,
    "KeyZ": 44, "KeyX": 45, "KeyC": 46, "KeyV": 47, "KeyB": 48, "KeyN": 49, "KeyM": 50,
    "Comma": 51, "Period": 52, "Slash": 53, "IntlBackslash": 86,
}

LAYOUTS = [
    ("us", ""), ("us", "intl"), ("us", "dvorak"), ("us", "colemak"), ("gb", ""),
    ("de", ""), ("ch", ""), ("fr", ""), ("be", ""), ("it", ""), ("es", ""), ("pt", ""),
    ("se", ""), ("no", ""), ("pl", ""), ("cz", ""), ("hu", ""), ("tr", ""),
    ("ru", ""), ("ua", ""), ("by", ""), ("bg", ""), ("rs", ""), ("gr", ""), ("il", ""),
    ("ara", ""), ("ir", ""), ("th", ""), ("kr", ""), ("jp", ""),
]

ctx = xkb.xkb_context_new(0)
out = {}
for layout, variant in LAYOUTS:
    names = RuleNames(b"evdev", b"pc105", layout.encode(), variant.encode(), b"")
    km = xkb.xkb_keymap_new_from_names(ctx, ctypes.byref(names), 0)
    if not km:
        print(f"skip {layout}({variant}): no keymap", file=sys.stderr)
        continue
    keys = {}
    for code, ev in EVDEV.items():
        syms = ctypes.POINTER(ctypes.c_uint32)()
        n = xkb.xkb_keymap_key_get_syms_by_level(km, ev + 8, 0, 0, ctypes.byref(syms))
        if n != 1:
            continue
        sym = syms[0]
        name = ctypes.create_string_buffer(64)
        xkb.xkb_keysym_get_name(sym, name, 64)
        name = name.value.decode()
        utf = ctypes.create_string_buffer(16)
        xkb.xkb_keysym_to_utf8(sym, utf, 16)
        text = utf.value.decode("utf-8", "replace")
        if name.startswith("dead_"):
            keys[code] = None
        elif text and not (len(text) == 1 and ord(text) < 0x20):
            keys[code] = text
        # A named keysym (Japanese Zenkaku_Hankaku) is left out: winit names
        # it, so it passes through the rewrite and binds nothing either way.
    xkb.xkb_keymap_unref(km)
    out[f"{layout}({variant})" if variant else layout] = keys

version = "unknown"
try:
    with open("/usr/share/pkgconfig/xkeyboard-config.pc") as pc:
        version = next(l.split(":", 1)[1].strip() for l in pc if l.startswith("Version:"))
except (OSError, StopIteration):
    pass

# One layout per line, so a regenerated table diffs by layout.
print("{")
print(f'"xkeyboard_config": {json.dumps(version)},')
print('"layouts": {')
rows = [f"{json.dumps(name)}: {json.dumps(keys, ensure_ascii=False, sort_keys=True)}" for name, keys in sorted(out.items())]
print(",\n".join(rows))
print("}")
print("}")
