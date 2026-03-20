#!/usr/bin/env python3
"""
Rename MVVM naming conventions to TEA-aligned names across the codebase.

Phase 1: Text replacements in .rs files (order matters - longest first)
Phase 2: File/directory moves

Usage: python3 scripts/rename_viewmodels.py [--dry-run]
"""

import os
import sys
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DRY_RUN = "--dry-run" in sys.argv

# ============================================================================
# Phase 1: Text replacements (applied in order - longest strings first)
# ============================================================================
REPLACEMENTS = [
    # --- Struct renames (longest first to avoid partial matches) ---
    ("AuthenticationViewModel", "AuthGateway"),
    ("SettingsViewModel", "SettingsService"),
    ("AlbumsViewModel", "AlbumsService"),
    ("ArtistsViewModel", "ArtistsService"),
    ("SongsViewModel", "SongsService"),
    ("QueueViewModel", "QueueService"),
    ("ShellViewModel", "AppService"),

    # --- Module path for shell.rs → app_service.rs (must come before generic) ---
    ("::shell::", "::app_service::"),

    # --- Module paths ---
    # Internal crate paths (data/src)
    ("crate::viewmodels::", "crate::backend::"),
    # External crate paths (src/)
    ("navidrome_data::viewmodels::", "navidrome_data::backend::"),
    # Doc references
    ("pub mod viewmodels;", "pub mod backend;"),

    # --- Field rename on NavidromeApp ---
    ("shell_vm", "app_service"),

    # --- Accessor methods on AppService (the public API) ---
    # These are method names called on the shell/AppService instance
    ("auth_viewmodel()", "auth()"),
    (".auth_viewmodel", ".auth_gateway"),
    ("queue_viewmodel()", "queue()"),
    ("albums_viewmodel()", "albums()"),
    ("artists_viewmodel()", "artists()"),
    ("songs_viewmodel()", "songs()"),

    # --- Internal field renames in data crate ---
    # These are struct fields inside the backend module files
    ("settings_viewmodel", "settings_service"),
    # auth_viewmodel as a field name (struct fields + local variables)
    # We need to be careful here - already handled .auth_viewmodel above
    # The remaining instances are field declarations and struct initializers

    # --- Doc comment cleanup ---
    ("Owned by ViewModel layer (not Model layer) per MVVM pattern",
     "UI-projected data"),
    ("per MVVM pattern", ""),
    ("/// ViewModel wrapper for SettingsManager",
     "/// Service wrapper for SettingsManager"),

    # --- Module declaration in mod.rs ---
    ("pub mod shell;", "pub mod app_service;"),
]

# Additional field-name replacements that only apply within data/src/backend/
# to avoid colliding with the accessor method renames above
BACKEND_ONLY_REPLACEMENTS = [
    # Rename the auth_viewmodel field/variable in struct bodies
    # (the .auth_viewmodel accessor was already handled globally)
    ("auth_viewmodel:", "auth_gateway:"),
    ("auth_viewmodel,", "auth_gateway,"),
    ("auth_viewmodel.", "auth_gateway."),
    ("let auth_viewmodel ", "let auth_gateway "),
    ("auth_viewmodel.clone()", "auth_gateway.clone()"),
    # queue_viewmodel as field
    ("queue_viewmodel:", "queue_service:"),
    ("queue_viewmodel,", "queue_service,"),
    ("queue_viewmodel.", "queue_service."),
    ("let queue_viewmodel ", "let queue_service "),
    # albums_viewmodel as field
    ("albums_viewmodel:", "albums_service:"),
    ("albums_viewmodel,", "albums_service,"),
    ("albums_viewmodel.", "albums_service."),
    ("let albums_viewmodel ", "let albums_service "),
    # artists_viewmodel as field
    ("artists_viewmodel:", "artists_service:"),
    ("artists_viewmodel,", "artists_service,"),
    ("artists_viewmodel.", "artists_service."),
    ("let artists_viewmodel ", "let artists_service "),
    # songs_viewmodel as field
    ("songs_viewmodel:", "songs_service:"),
    ("songs_viewmodel,", "songs_service,"),
    ("songs_viewmodel.", "songs_service."),
    ("let songs_viewmodel ", "let songs_service "),
]


def find_rs_files(directory: Path):
    """Find all .rs files under directory, excluding target/."""
    for root, dirs, files in os.walk(directory):
        # Skip target directory and hidden directories
        dirs[:] = [d for d in dirs if d not in ("target", ".git", "reference-iced", "reference-rmpc", "deps")]
        for f in files:
            if f.endswith(".rs"):
                yield Path(root) / f


def apply_replacements(content: str, replacements: list[tuple[str, str]]) -> str:
    """Apply text replacements in order."""
    for old, new in replacements:
        content = content.replace(old, new)
    return content


def main():
    if DRY_RUN:
        print("=== DRY RUN MODE ===\n")

    # Collect all .rs files
    rs_files = list(find_rs_files(ROOT))
    print(f"Found {len(rs_files)} .rs files\n")

    # Also process .md files for doc updates
    md_files = [ROOT / "things_to_solve.md", ROOT / "README.md"]
    md_files = [f for f in md_files if f.exists()]

    modified_count = 0

    # Phase 1: Apply text replacements
    print("Phase 1: Text replacements")
    print("-" * 40)

    for filepath in rs_files:
        original = filepath.read_text()
        modified = apply_replacements(original, REPLACEMENTS)

        # Apply backend-only replacements for files in the viewmodels dir
        if "viewmodels" in str(filepath) or "backend" in str(filepath):
            modified = apply_replacements(modified, BACKEND_ONLY_REPLACEMENTS)

        if modified != original:
            modified_count += 1
            rel = filepath.relative_to(ROOT)
            if DRY_RUN:
                # Show what changed
                orig_lines = original.splitlines()
                mod_lines = modified.splitlines()
                print(f"\n📝 {rel}:")
                for i, (ol, ml) in enumerate(zip(orig_lines, mod_lines)):
                    if ol != ml:
                        print(f"  L{i+1}: {ol.strip()}")
                        print(f"     → {ml.strip()}")
            else:
                filepath.write_text(modified)
                print(f"  ✅ {rel}")

    for filepath in md_files:
        original = filepath.read_text()
        modified = apply_replacements(original, REPLACEMENTS)
        if modified != original:
            modified_count += 1
            rel = filepath.relative_to(ROOT)
            if DRY_RUN:
                print(f"\n📝 {rel}: (doc updates)")
            else:
                filepath.write_text(modified)
                print(f"  ✅ {rel}")

    print(f"\n  Modified {modified_count} files")

    # Phase 2: File & directory moves
    print("\nPhase 2: File & directory moves")
    print("-" * 40)

    viewmodels_dir = ROOT / "data" / "src" / "viewmodels"
    backend_dir = ROOT / "data" / "src" / "backend"

    if viewmodels_dir.exists():
        # First rename shell.rs -> app_service.rs inside viewmodels/
        shell_file = viewmodels_dir / "shell.rs"
        app_service_file = viewmodels_dir / "app_service.rs"
        if shell_file.exists():
            if DRY_RUN:
                print(f"  📁 {shell_file.relative_to(ROOT)} → {app_service_file.name}")
            else:
                shell_file.rename(app_service_file)
                print(f"  ✅ shell.rs → app_service.rs")

        # Then rename the directory
        if DRY_RUN:
            print(f"  📁 {viewmodels_dir.relative_to(ROOT)}/ → {backend_dir.relative_to(ROOT)}/")
        else:
            viewmodels_dir.rename(backend_dir)
            print(f"  ✅ viewmodels/ → backend/")
    else:
        print("  ⚠️  viewmodels/ directory not found (already renamed?)")

    print("\n✨ Done!")
    if not DRY_RUN:
        print("\nNext steps:")
        print("  1. cargo build --workspace")
        print("  2. cargo test --workspace")
        print("  3. cargo clippy --workspace")
        print("  4. Fix any remaining compile errors")


if __name__ == "__main__":
    main()
