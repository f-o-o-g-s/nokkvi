#!/usr/bin/env python3
"""Migrate Nokkvi theme files from Gruvbox-heritage naming to semantic naming.

Renames:
  [dark.red]    → [dark.danger]     [light.red]    → [light.danger]
  [dark.green]  → [dark.success]    [light.green]  → [light.success]
  [dark.yellow] → [dark.warning]    [light.yellow] → [light.warning]
  normal = ...  → base = ...        (within semantic color sections)

Removes:
  [dark.purple], [dark.aqua], [dark.orange]
  [light.purple], [light.aqua], [light.orange]

Adds:
  [dark.star]  (cloned from [dark.warning] values)
  [light.star] (cloned from [light.warning] values)

Creates .bak backups. Idempotent — skips files already migrated.

Usage:
  python3 migrate_themes.py                    # Migrate ~/.config/nokkvi/themes/
  python3 migrate_themes.py /path/to/themes/   # Migrate specific directory
"""

import glob
import os
import shutil
import sys


def migrate_theme(filepath: str) -> bool:
    """Migrate a single theme file. Returns True if modified."""
    with open(filepath) as f:
        content = f.read()

    # Skip if already migrated
    if "[dark.danger]" in content or "[dark.star]" in content:
        return False

    # Skip if no old-style sections exist
    if "[dark.red]" not in content:
        return False

    renames = {
        "[dark.red]": "[dark.danger]",
        "[dark.green]": "[dark.success]",
        "[dark.yellow]": "[dark.warning]",
        "[light.red]": "[light.danger]",
        "[light.green]": "[light.success]",
        "[light.yellow]": "[light.warning]",
    }

    dead_sections = {
        "[dark.purple]", "[dark.aqua]", "[dark.orange]",
        "[light.purple]", "[light.aqua]", "[light.orange]",
    }

    lines = content.split("\n")
    new_lines = []
    skip = False

    for line in lines:
        stripped = line.strip()

        if stripped in dead_sections:
            skip = True
            continue

        if stripped.startswith("[") and skip:
            skip = False

        if skip:
            continue

        for old, new in renames.items():
            if stripped == old:
                line = line.replace(old, new)
                break

        if "normal = " in line and not skip:
            line = line.replace("normal = ", "base = ")

        new_lines.append(line)

    # Add [dark.star] and [light.star] sections
    result = []
    i = 0
    while i < len(new_lines):
        result.append(new_lines[i])
        stripped = new_lines[i].strip()

        for prefix in ("[dark.warning]", "[light.warning]"):
            if stripped == prefix and i + 2 < len(new_lines):
                result.append(new_lines[i + 1])
                result.append(new_lines[i + 2])
                i += 3
                base_line = new_lines[i - 2]
                bright_line = new_lines[i - 1]
                star_section = prefix.replace("warning", "star")
                result.append("")
                result.append(star_section)
                result.append(base_line)
                result.append(bright_line)
                break
        else:
            i += 1
            continue
        continue

    new_content = "\n".join(result)

    # Create backup
    backup = filepath + ".bak"
    shutil.copy2(filepath, backup)

    with open(filepath, "w") as f:
        f.write(new_content)

    return True


def main():
    if len(sys.argv) > 1:
        themes_dir = sys.argv[1]
    else:
        themes_dir = os.path.expanduser("~/.config/nokkvi/themes")

    if not os.path.isdir(themes_dir):
        print(f"Themes directory not found: {themes_dir}")
        sys.exit(1)

    files = sorted(glob.glob(os.path.join(themes_dir, "*.toml")))
    if not files:
        print(f"No .toml files found in {themes_dir}")
        sys.exit(0)

    migrated = 0
    skipped = 0
    for f in files:
        name = os.path.basename(f)
        if migrate_theme(f):
            print(f"  ✓ Migrated: {name}")
            migrated += 1
        else:
            print(f"  · Skipped (already migrated): {name}")
            skipped += 1

    print(f"\nDone: {migrated} migrated, {skipped} skipped")


if __name__ == "__main__":
    main()
