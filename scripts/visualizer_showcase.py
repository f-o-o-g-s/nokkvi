#!/usr/bin/env python3
"""Cycle through visualizer presets AND color themes by modifying config.toml.

The app hot-reloads on config changes, so each combination becomes visible
immediately. Press Ctrl+C to stop and restore the original config.

Zero external dependencies — uses only Python 3.11+ stdlib.

Default mode:  each step changes BOTH the visualizer preset AND the color
               theme at the same time, keeping total runtime short while
               showcasing the maximum variety.

Usage:
    python3 scripts/visualizer_showcase.py                  # combined (default)
    python3 scripts/visualizer_showcase.py --both-modes      # dark pass → toggle → light pass
    python3 scripts/visualizer_showcase.py --presets-only    # presets only
    python3 scripts/visualizer_showcase.py --themes-only     # themes only
    python3 scripts/visualizer_showcase.py -i 4 -l           # 4s intervals, loop
    python3 scripts/visualizer_showcase.py --shuffle          # randomize order
"""

import argparse
import itertools
import random
import re
import shutil
import signal
import sys
import time
from pathlib import Path

CONFIG = Path.home() / ".config/nokkvi/config.toml"
BACKUP = CONFIG.with_suffix(".toml.showcase-bak")
THEMES_DIR = Path(__file__).resolve().parent.parent / "example_themes"

# ─── Visualizer presets ────────────────────────────────────────────────
# Each preset overrides [visualizer] and/or [visualizer.bars] keys.

PRESETS = [
    {
        "name": "Classic Bars",
        "bars": dict(gradient_mode="static", peak_gradient_mode="static",
                     led_bars=False, bar_depth_3d=0.0, bar_perspective=0.0,
                     bar_width_min=8.0, bar_width_max=16.0, bar_spacing=4.0,
                     border_width=1.0, peak_mode="fall_accel"),
        "top": dict(monstercat=0.0, waves=True),
    },
    {
        "name": "Wave Gradient",
        "bars": dict(gradient_mode="wave", peak_gradient_mode="cycle",
                     led_bars=False, bar_depth_3d=0.0, bar_perspective=0.0),
    },
    {
        "name": "Energy Pulse",
        "bars": dict(gradient_mode="energy", peak_gradient_mode="height",
                     led_bars=False, bar_depth_3d=0.0, bar_perspective=0.0),
    },
    {
        "name": "Shimmer",
        "bars": dict(gradient_mode="shimmer", peak_gradient_mode="match",
                     led_bars=False, bar_depth_3d=0.0, bar_perspective=0.0),
    },
    {
        "name": "LED Bars",
        "bars": dict(led_bars=True, led_segment_height=4.0,
                     led_border_opacity=1.0, gradient_mode="static",
                     bar_depth_3d=0.0, bar_perspective=0.0),
    },
    {
        "name": "LED + Wave",
        "bars": dict(led_bars=True, led_segment_height=3.0,
                     gradient_mode="wave",
                     bar_depth_3d=0.0, bar_perspective=0.0),
    },
    {
        "name": "Thin Bars",
        "bars": dict(bar_width_min=2.0, bar_width_max=4.0,
                     bar_spacing=2.0, border_width=0.0,
                     led_bars=False, gradient_mode="wave",
                     bar_depth_3d=0.0, bar_perspective=0.0),
    },
    {
        "name": "Thick Bars",
        "bars": dict(bar_width_min=12.0, bar_width_max=20.0,
                     bar_spacing=4.0, border_width=2.0,
                     led_bars=False, gradient_mode="static",
                     bar_depth_3d=0.0, bar_perspective=0.0),
    },
    {
        "name": "Isometric 3D",
        "bars": dict(led_bars=False, gradient_mode="wave",
                     bar_width_min=8.0, bar_width_max=14.0,
                     bar_spacing=5.0, border_width=1.0,
                     bar_depth_3d=6.0, bar_perspective=0.0),
    },
    {
        "name": "Isometric + LED",
        "bars": dict(led_bars=True, led_segment_height=4.0,
                     gradient_mode="static",
                     bar_width_min=10.0, bar_width_max=16.0,
                     bar_spacing=6.0, border_width=1.0,
                     bar_depth_3d=8.0, bar_perspective=0.0),
    },
    {
        "name": "Perspective Lean",
        "bars": dict(led_bars=False, gradient_mode="wave",
                     bar_width_min=8.0, bar_width_max=14.0,
                     bar_spacing=4.0, border_width=1.0,
                     bar_depth_3d=0.0, bar_perspective=0.3),
    },
    {
        "name": "Perspective + Isometric",
        "bars": dict(led_bars=False, gradient_mode="shimmer",
                     bar_width_min=8.0, bar_width_max=14.0,
                     bar_spacing=5.0, border_width=1.0,
                     bar_depth_3d=5.0, bar_perspective=0.25),
    },
    {
        "name": "Full 3D + LED",
        "bars": dict(led_bars=True, led_segment_height=3.0,
                     gradient_mode="wave",
                     bar_width_min=10.0, bar_width_max=16.0,
                     bar_spacing=6.0, border_width=1.0,
                     bar_depth_3d=6.0, bar_perspective=0.2),
    },
    {
        "name": "Peak: Fade",
        "bars": dict(led_bars=False, gradient_mode="static",
                     peak_mode="fade", peak_hold_time=800, peak_fade_time=1200,
                     bar_depth_3d=0.0, bar_perspective=0.0),
    },
    {
        "name": "Peak: Fall",
        "bars": dict(peak_mode="fall", peak_hold_time=500,
                     bar_depth_3d=0.0, bar_perspective=0.0),
    },
    {
        "name": "No Peaks",
        "bars": dict(peak_mode="none",
                     bar_depth_3d=0.0, bar_perspective=0.0),
    },
    {
        "name": "Monstercat Smooth",
        "top": dict(monstercat=5.0, waves=False),
        "bars": dict(led_bars=False, gradient_mode="wave",
                     peak_mode="fall_accel",
                     bar_depth_3d=0.0, bar_perspective=0.0),
    },
    {
        "name": "Monstercat + 3D",
        "top": dict(monstercat=5.0, waves=False),
        "bars": dict(led_bars=False, gradient_mode="energy",
                     bar_width_min=8.0, bar_width_max=14.0,
                     bar_spacing=5.0, border_width=1.0,
                     bar_depth_3d=5.0, bar_perspective=0.15),
    },
]


def format_toml_value(v) -> str:
    """Format a Python value as a TOML literal."""
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, int):
        return str(v)
    if isinstance(v, float):
        return f"{v}" if v != int(v) else f"{v:.1f}"
    if isinstance(v, str):
        return f'"{v}"'
    raise ValueError(f"Unsupported type: {type(v)}")


def patch_toml(text: str, overrides: dict) -> str:
    """Replace values in TOML text via regex (preserves comments/ordering)."""
    for key, val in overrides.items():
        toml_val = format_toml_value(val)
        pattern = rf'^({re.escape(key)}\s*=\s*)(.+?)(\s*#.*)?$'
        text, n = re.subn(pattern, rf'\g<1>{toml_val}\g<3>', text,
                          count=1, flags=re.MULTILINE)
        if n == 0:
            print(f"    ⚠ key '{key}' not found, skipping")
    return text


def theme_name(path: Path) -> str:
    """Extract a display name from a theme filename."""
    name = path.stem
    # Strip common prefixes
    for prefix in ("config_", "config-"):
        if name.startswith(prefix):
            name = name[len(prefix):]
    return name.replace("_", " ").title()


def apply_theme(original_text: str, theme_path: Path) -> str:
    """Merge a theme file's [theme.*] and [visualizer.bars.dark/light] sections
    into the user's config, preserving credentials and non-theme settings.

    Strategy: read the theme file as text. For each TOML section that starts
    with [theme.] or [visualizer.bars.dark] or [visualizer.bars.light],
    replace the corresponding section in the original config.
    """
    theme_text = theme_path.read_text()

    # Parse into sections: find all [section.header] blocks
    section_re = re.compile(r'^(\[.+?\])\s*$', re.MULTILINE)

    def extract_sections(text: str) -> dict:
        """Return {header: content} for each TOML section."""
        matches = list(section_re.finditer(text))
        sections = {}
        for i, m in enumerate(matches):
            header = m.group(1)
            start = m.start()
            end = matches[i + 1].start() if i + 1 < len(matches) else len(text)
            sections[header] = text[start:end]
        return sections

    orig_sections = extract_sections(original_text)
    theme_sections = extract_sections(theme_text)

    # Sections to merge from theme
    merge_prefixes = ("[theme.", "[visualizer.bars.dark", "[visualizer.bars.light")

    result = original_text
    for header, content in theme_sections.items():
        if any(header.startswith(p) for p in merge_prefixes):
            if header in orig_sections:
                result = result.replace(orig_sections[header], content)
    return result


def apply_preset(text: str, preset: dict) -> str:
    """Apply a visualizer preset's overrides to config text."""
    if "top" in preset:
        text = patch_toml(text, preset["top"])
    if "bars" in preset:
        text = patch_toml(text, preset["bars"])
    return text


def set_light_mode(text: str, enabled: bool) -> str:
    """Set theme.light_mode in config text (hot-reloaded by the app).

    If the key already exists, update it via regex.  Otherwise insert it
    right before the first [theme.*] sub-section using TOML dotted-key
    syntax so it belongs to the [theme] table.
    """
    toml_val = "true" if enabled else "false"
    # Try to replace existing key (either dotted or bare form)
    pattern = r'^((?:theme\.)?light_mode\s*=\s*)(.+?)(\s*#.*)?$'
    new_text, n = re.subn(pattern, rf'\g<1>{toml_val}\g<3>', text,
                          count=1, flags=re.MULTILINE)
    if n > 0:
        return new_text

    # Key doesn't exist yet — insert before first [theme.*] section
    # using dotted notation so TOML knows it's under [theme]
    m = re.search(r'^\[theme\.', text, re.MULTILINE)
    if m:
        insert_at = m.start()
        return text[:insert_at] + f"theme.light_mode = {toml_val}\n\n" + text[insert_at:]

    # Fallback: append to end (shouldn't happen in practice)
    return text + f"\ntheme.light_mode = {toml_val}\n"


def build_combined_steps(presets, themes, shuffle=False):
    """Build a list of (preset, theme_path|None) pairs that cycles through
    both lists simultaneously.  The total number of steps equals
    max(len(presets), len(themes)) so every preset and every theme is
    shown at least once.
    """
    n = max(len(presets), len(themes))
    preset_cycle = list(itertools.islice(itertools.cycle(presets), n))
    theme_cycle = list(itertools.islice(itertools.cycle(themes), n))

    steps = list(zip(preset_cycle, theme_cycle))

    if shuffle:
        random.shuffle(steps)

    return steps


def main():
    parser = argparse.ArgumentParser(
        description="Cycle visualizer presets and color themes (combined by default)")
    parser.add_argument("--interval", "-i", type=float, default=2.5,
                        help="Seconds per step (default: 2.5)")
    parser.add_argument("--loop", "-l", action="store_true",
                        help="Loop continuously until Ctrl+C")
    parser.add_argument("--presets-only", action="store_true",
                        help="Skip theme cycling (presets only)")
    parser.add_argument("--themes-only", action="store_true",
                        help="Skip presets (themes only)")
    parser.add_argument("--separate", action="store_true",
                        help="Use old sequential mode (presets first, then themes)")
    parser.add_argument("--both-modes", action="store_true",
                        help="Run dark mode pass, then light mode pass (automated via config.toml)")
    parser.add_argument("--shuffle", action="store_true",
                        help="Randomize the order of steps")
    # Keep old flag as hidden alias
    parser.add_argument("--no-themes", action="store_true",
                        help=argparse.SUPPRESS)
    args = parser.parse_args()

    # Alias
    if args.no_themes:
        args.presets_only = True

    if not CONFIG.exists():
        print(f"✗ Config not found: {CONFIG}")
        sys.exit(1)

    # Discover themes
    themes = sorted(THEMES_DIR.glob("*.toml")) if THEMES_DIR.exists() else []

    # Back up
    original_text = CONFIG.read_text()
    shutil.copy2(CONFIG, BACKUP)

    # Figure out mode description
    if args.presets_only:
        mode_label = "presets only"
    elif args.themes_only:
        mode_label = "themes only"
    elif args.separate:
        mode_label = "sequential (presets → themes)"
    else:
        mode_label = "combined (preset + theme each step)"

    if args.both_modes:
        mode_label += " + dark/light"

    print(f"📋 Backed up config → {BACKUP.name}")
    print(f"⏱  Interval: {args.interval}s per step")
    print(f"🎨 Themes:   {len(themes)} found in example_themes/")
    print(f"🔧 Presets:  {len(PRESETS)}")
    print(f"🎯 Mode:     {mode_label}")
    print(f"🔀 Shuffle:  {'yes' if args.shuffle else 'no'}")
    print(f"🔁 Loop:     {'yes' if args.loop else 'single pass'}")

    # Estimate total duration
    if args.presets_only:
        total_steps = len(PRESETS)
    elif args.themes_only:
        total_steps = len(themes)
    elif args.separate:
        total_steps = len(PRESETS) + len(themes)
    else:
        total_steps = max(len(PRESETS), len(themes)) if themes else len(PRESETS)

    passes = 2 if args.both_modes else 1
    est_time = total_steps * args.interval * passes
    print(f"⏳ Estimated: {total_steps} steps × {args.interval}s"
          f"{f' × {passes} passes' if passes > 1 else ''}"
          f" = ~{est_time:.0f}s per cycle")
    print(f"   Press Ctrl+C to stop and restore.\n")

    def restore(_sig=None, _frame=None):
        print("\n\n🔄 Restoring original config...")
        CONFIG.write_text(original_text)
        BACKUP.unlink(missing_ok=True)
        print("✓ Done.")
        sys.exit(0)

    signal.signal(signal.SIGINT, restore)
    signal.signal(signal.SIGTERM, restore)

    def run_steps(label_suffix="", light_mode=None, both_modes=False):
        """Execute one full pass of steps. Returns when complete.
        If light_mode is not None, every config write includes the light_mode setting.
        If both_modes is True, each step is shown in dark then light mode.
        """
        def finalize(text, lm=light_mode):
            """Apply light_mode override to config text if requested."""
            if lm is not None:
                return set_light_mode(text, lm)
            return text

        # ── Combined mode (default) ────────────────────────────────
        if not args.presets_only and not args.themes_only and not args.separate:
            if themes:
                steps = build_combined_steps(PRESETS, themes, args.shuffle)
                total = len(steps)
                mode_info = " (☾/☀ each step)" if both_modes else ""
                print(f"━━━ Combined{label_suffix}: {total} steps{mode_info} ━━━")
                for i, (preset, theme_path) in enumerate(steps, 1):
                    text = apply_theme(original_text, theme_path)
                    text = apply_preset(text, preset)
                    t_name = theme_name(theme_path)
                    if both_modes:
                        CONFIG.write_text(finalize(text, lm=False))
                        print(f"  [{i:2d}/{total}] ☾  🔧 {preset['name']:<24s}  🎨 {t_name}")
                        time.sleep(args.interval)
                        CONFIG.write_text(finalize(text, lm=True))
                        print(f"  [{i:2d}/{total}] ☀  🔧 {preset['name']:<24s}  🎨 {t_name}")
                        time.sleep(args.interval)
                    else:
                        CONFIG.write_text(finalize(text))
                        print(f"  [{i:2d}/{total}] 🔧 {preset['name']:<24s}  🎨 {t_name}")
                        time.sleep(args.interval)
            else:
                print(f"━━━ Visualizer Presets{label_suffix} (no themes found) ━━━")
                presets = list(PRESETS)
                if args.shuffle:
                    random.shuffle(presets)
                for i, preset in enumerate(presets, 1):
                    text = apply_preset(original_text, preset)
                    if both_modes:
                        CONFIG.write_text(finalize(text, lm=False))
                        print(f"  [{i:2d}/{len(presets)}] ☾  🔧 {preset['name']}")
                        time.sleep(args.interval)
                        CONFIG.write_text(finalize(text, lm=True))
                        print(f"  [{i:2d}/{len(presets)}] ☀  🔧 {preset['name']}")
                        time.sleep(args.interval)
                    else:
                        CONFIG.write_text(finalize(text))
                        print(f"  [{i:2d}/{len(presets)}] 🔧 {preset['name']}")
                        time.sleep(args.interval)

        # ── Sequential / legacy mode ───────────────────────────────
        elif args.separate:
            print(f"━━━ Visualizer Presets{label_suffix} ━━━")
            presets = list(PRESETS)
            if args.shuffle:
                random.shuffle(presets)
            for i, preset in enumerate(presets, 1):
                text = apply_preset(original_text, preset)
                CONFIG.write_text(finalize(text))
                print(f"  [{i:2d}/{len(presets)}] {preset['name']}")
                time.sleep(args.interval)

            CONFIG.write_text(finalize(original_text))
            time.sleep(0.3)

            if themes:
                print(f"\n━━━ Color Themes{label_suffix} ━━━")
                theme_list = list(themes)
                if args.shuffle:
                    random.shuffle(theme_list)
                for i, theme_path in enumerate(theme_list, 1):
                    name = theme_name(theme_path)
                    text = apply_theme(original_text, theme_path)
                    CONFIG.write_text(finalize(text))
                    print(f"  [{i:2d}/{len(theme_list)}] {name}")
                    time.sleep(args.interval)

                CONFIG.write_text(finalize(original_text))
                time.sleep(0.3)

        # ── Presets only ───────────────────────────────────────────
        elif args.presets_only:
            print(f"━━━ Visualizer Presets{label_suffix} ━━━")
            presets = list(PRESETS)
            if args.shuffle:
                random.shuffle(presets)
            for i, preset in enumerate(presets, 1):
                text = apply_preset(original_text, preset)
                CONFIG.write_text(finalize(text))
                print(f"  [{i:2d}/{len(presets)}] {preset['name']}")
                time.sleep(args.interval)

            CONFIG.write_text(finalize(original_text))
            time.sleep(0.3)

        # ── Themes only ────────────────────────────────────────────
        elif args.themes_only and themes:
            print(f"━━━ Color Themes{label_suffix} ━━━")
            theme_list = list(themes)
            if args.shuffle:
                random.shuffle(theme_list)
            for i, theme_path in enumerate(theme_list, 1):
                name = theme_name(theme_path)
                text = apply_theme(original_text, theme_path)
                CONFIG.write_text(finalize(text))
                print(f"  [{i:2d}/{len(theme_list)}] {name}")
                time.sleep(args.interval)

            CONFIG.write_text(finalize(original_text))
            time.sleep(0.3)

    try:
        running = True
        pass_num = 0
        while running:
            pass_num += 1
            if args.loop:
                print(f"━━━ Pass {pass_num} ━━━")

            if args.both_modes:
                run_steps(both_modes=True)
            else:
                run_steps()

            if not args.loop:
                running = False
    finally:
        restore()


if __name__ == "__main__":
    main()
