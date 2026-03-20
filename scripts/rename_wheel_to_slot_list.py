#!/usr/bin/env python3
"""Rename all 'wheel' references to 'slot_list' / 'SlotList' across the codebase.

Usage:
    python3 scripts/rename_wheel_to_slot_list.py

This script:
1. Renames files via `git mv`
2. Performs ordered string replacements across all .rs and .md files
3. Skips target/, dist/, deps/, reference-* directories
"""

import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# --- Phase 1: File renames (git mv) ---
FILE_RENAMES = [
    ("src/widgets/wheel.rs", "src/widgets/slot_list.rs"),
    ("src/widgets/wheel_view.rs", "src/widgets/slot_list_view.rs"),
    ("src/widgets/wheel_page.rs", "src/widgets/slot_list_page.rs"),
    ("src/widgets/base_wheel_layout.rs", "src/widgets/base_slot_list_layout.rs"),
    ("src/update/wheel.rs", "src/update/slot_list.rs"),
]

# --- Phase 2: Content replacements ---
# Order matters! Longest/most-specific first to avoid partial matches.
# Each tuple is (old, new).
REPLACEMENTS = [
    # --- Function names (longest first) ---
    ("wheel_view_with_scroll", "slot_list_view_with_scroll"),
    ("wheel_view_with_drag", "slot_list_view_with_drag"),
    ("wheel_background_container", "slot_list_background_container"),
    ("wheel_metadata_column", "slot_list_metadata_column"),
    ("wheel_artwork_column", "slot_list_artwork_column"),
    ("wheel_text_column", "slot_list_text_column"),
    ("wheel_index_column", "slot_list_index_column"),
    ("wheel_favorite_icon", "slot_list_favorite_icon"),
    ("wheel_star_rating", "slot_list_star_rating"),
    ("wheel_text(", "slot_list_text("),    # only match function call, not substring
    ("wheel_text,", "slot_list_text,"),    # match in doc comments like "wheel_text,"
    ("wheel_text`", "slot_list_text`"),    # match in markdown backticks
    ("`wheel_text`", "`slot_list_text`"),  # match in markdown backticks
    ("wheel_view(", "slot_list_view("),    # only match the bare function call
    ("wheel_view`", "slot_list_view`"),    # in docs

    # --- Base layout functions ---
    ("base_wheel_empty_artwork", "base_slot_list_empty_artwork"),
    ("base_wheel_empty_state", "base_slot_list_empty_state"),
    ("base_wheel_layout", "base_slot_list_layout"),

    # --- Type names (CamelCase) ---
    ("BaseWheelLayoutConfig", "BaseSlotListLayoutConfig"),
    ("WheelRowContext", "SlotListRowContext"),
    ("WheelSlotStyle", "SlotListSlotStyle"),
    ("WheelConfig", "SlotListConfig"),
    ("WheelPageState", "SlotListPageState"),
    ("WheelPageAction", "SlotListPageAction"),
    ("WheelMessage", "SlotListMessage"),
    ("WheelView", "SlotListView"),
    ("WheelEntry", "SlotListEntry"),

    # --- Constants ---
    ("WHEEL_SLOT_PADDING", "SLOT_LIST_SLOT_PADDING"),
    ("WHEEL_SLOT_BORDER_RADIUS", "SLOT_LIST_SLOT_BORDER_RADIUS"),

    # --- Message variants (per-view enums) ---
    ("WheelNavigateUp", "SlotListNavigateUp"),
    ("WheelNavigateDown", "SlotListNavigateDown"),
    ("WheelSetOffset", "SlotListSetOffset"),
    ("WheelSelectOffset", "SlotListSelectOffset"),
    ("WheelActivateCenter", "SlotListActivateCenter"),
    ("WheelToggleSortOrder", "SlotListToggleSortOrder"),
    ("WheelClickPlay", "SlotListClickPlay"),

    # --- Hotkey actions ---
    ("WheelUp", "SlotListUp"),
    ("WheelDown", "SlotListDown"),

    # --- Handler functions ---
    ("handle_wheel_message", "handle_slot_list_message"),
    ("handle_wheel_navigate_down", "handle_slot_list_navigate_down"),

    # --- Module declarations ---
    ("mod wheel;", "mod slot_list;"),
    ("mod wheel_page;", "mod slot_list_page;"),
    ("mod wheel_view;", "mod slot_list_view;"),
    ("mod base_wheel_layout;", "mod base_slot_list_layout;"),
    ("use wheel_page::", "use slot_list_page::"),
    ("use wheel_view::", "use slot_list_view::"),

    # --- Hotkey display strings ---
    ('"Wheel Up"', '"Slot List Up"'),
    ('"Wheel Down"', '"Slot List Down"'),
    ('"Navigate up in the wheel"', '"Navigate up in the slot list"'),
    ('"Navigate down in the wheel"', '"Navigate down in the slot list"'),

    # --- Hotkey category: "Wheel" → "Navigation" ---
    # Be very specific to only match the category return value
    ('=> "Wheel",', '=> "Navigation",'),

    # --- Doc comment / module-level references ---
    ("Wheel Navigation", "Slot List Navigation"),
    ("Wheel navigation", "Slot list navigation"),
    ("wheel navigation", "slot list navigation"),
    ("wheel-based", "slot-list-based"),
    ("Wheel-based", "Slot-list-based"),
    ("wheel-navigable", "slot-list-navigable"),
    ("Wheel-navigable", "Slot-list-navigable"),
    ("wheel views", "slot list views"),
    ("wheel view", "slot list view"),
    ("Wheel view", "Slot list view"),

    # --- File path references in comments/docs ---
    ("base_wheel_layout.rs", "base_slot_list_layout.rs"),
    ("wheel_view.rs", "slot_list_view.rs"),
    ("wheel_page.rs", "slot_list_page.rs"),
    ("update/wheel.rs", "update/slot_list.rs"),

    # Remaining: standalone "wheel" in comments (handled carefully)
    # These are generic enough they need careful context — skip for manual review
]

# Directories to skip entirely
SKIP_DIRS = {"target", "dist", "deps", ".git", "scripts"}
SKIP_PREFIXES = ("reference-",)

# File extensions to process
PROCESS_EXTENSIONS = {".rs", ".md", ".toml"}


def should_skip_dir(dirname: str) -> bool:
    if dirname in SKIP_DIRS:
        return True
    for prefix in SKIP_PREFIXES:
        if dirname.startswith(prefix):
            return True
    return False


def collect_files(root: str) -> list[str]:
    """Collect all files to process."""
    result = []
    for dirpath, dirnames, filenames in os.walk(root):
        # Filter out skip dirs in-place so os.walk doesn't descend
        dirnames[:] = [d for d in dirnames if not should_skip_dir(d)]
        for fname in filenames:
            _, ext = os.path.splitext(fname)
            if ext in PROCESS_EXTENSIONS:
                result.append(os.path.join(dirpath, fname))
    return result


def rename_files():
    """Phase 1: Rename files using git mv."""
    print("=== Phase 1: File Renames ===")
    for old_rel, new_rel in FILE_RENAMES:
        old_abs = os.path.join(ROOT, old_rel)
        new_abs = os.path.join(ROOT, new_rel)
        if os.path.exists(old_abs):
            print(f"  git mv {old_rel} -> {new_rel}")
            subprocess.run(["git", "mv", old_abs, new_abs], cwd=ROOT, check=True)
        else:
            print(f"  SKIP (not found): {old_rel}")


def replace_in_files():
    """Phase 2: Content replacements."""
    print("\n=== Phase 2: Content Replacements ===")
    files = collect_files(ROOT)
    modified_count = 0

    for filepath in files:
        try:
            with open(filepath, "r", encoding="utf-8") as f:
                content = f.read()
        except (UnicodeDecodeError, PermissionError):
            continue

        original = content
        for old, new in REPLACEMENTS:
            content = content.replace(old, new)

        if content != original:
            with open(filepath, "w", encoding="utf-8") as f:
                f.write(content)
            rel = os.path.relpath(filepath, ROOT)
            print(f"  Modified: {rel}")
            modified_count += 1

    print(f"\n  Total files modified: {modified_count}")


def main():
    os.chdir(ROOT)
    print(f"Working directory: {ROOT}\n")

    rename_files()
    replace_in_files()

    print("\n=== Done! ===")
    print("Next steps:")
    print("  1. cargo +nightly fmt --all")
    print("  2. cargo clippy")
    print("  3. cargo test")
    print("  4. cargo build --release")


if __name__ == "__main__":
    main()
