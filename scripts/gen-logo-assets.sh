#!/usr/bin/env sh
# Regenerate every committed logo asset from one canonical master SVG.
#
# DEVELOPER / RELEASE-TIME ONLY — this is NOT a cargo build step. CI and the
# AUR/tarball build consume the committed assets and never run this (there is no
# rasterizer in their dependency set). Run it by hand when the logo artwork
# changes, then commit the master plus all four derived outputs together.
#
# Pinned tooling (other versions may round path coords / PNG bytes differently,
# which `--check` would then flag as drift):
#   scour 0.38.2, rsvg-convert 2.62.x (librsvg)
#
# Source of truth:
#   assets/logo/nokkvi_master.svg   canonical, scoured, lowercased-hex; KEEPS the
#                                   artwork's per-path stroke widths (the boat
#                                   strips them at runtime; the static/About/icon
#                                   uses need them to match the authored art).
# Derived (committed):
#   assets/nokkvi_logo.svg          runtime themed template (= master)
#   assets/org.nokkvi.nokkvi.svg    desktop scalable icon (= master)
#   assets/nokkvi_logo_readme.svg   README mark (id-stripped, lighter)
#   assets/org.nokkvi.nokkvi.png    512px raster (tray include_bytes! + hicolor)
#
# Usage:
#   sh scripts/gen-logo-assets.sh <artwork.svg>   rebuild master from new art, then derive all
#   sh scripts/gen-logo-assets.sh                 re-derive everything from the existing master
#   sh scripts/gen-logo-assets.sh --check         verify committed derived assets are in sync (exit 1 on drift)
set -eu

ROOT=$(git rev-parse --show-toplevel)
cd "$ROOT"

MASTER="assets/logo/nokkvi_master.svg"

build_master() {
  # $1 = source artwork, $2 = output master path
  scour -i "$1" -o "$2" \
    --remove-metadata --enable-comment-stripping \
    --strip-xml-space --no-line-breaks --remove-descriptive-elements >/dev/null
  # Lowercase every #rrggbb. Inkscape emits mixed case (#CBA576 vs #cba576) and
  # Rust's str::replace is case-sensitive, so the runtime token swap in
  # src/embedded_svg.rs needs canonical lowercase to hit every path.
  sed -i 's/#\([0-9A-Fa-f]\{6\}\)/#\L\1/g' "$2"
  # Drop the vestigial root width/height so only the square viewBox governs
  # sizing (parity with the old template; removes the boat's viewport/viewBox
  # scale ambiguity since the container sizes the sprite explicitly).
  sed -i 's/ width="1024"//; s/ height="1024"//' "$2"
}

derive_into() {
  # $1 = master to derive from, $2 = dest prefix ("" for repo root, "$tmp/" for --check)
  m="$1"; d="$2"
  cp "$m" "${d}assets/nokkvi_logo.svg"
  cp "$m" "${d}assets/org.nokkvi.nokkvi.svg"
  # README mark: strip ALL ids (no runtime boat anchor needed here) + the xml
  # prolog so it embeds cleanly via <img>.
  scour -i "$m" -o "${d}assets/nokkvi_logo_readme.svg" \
    --remove-metadata --enable-id-stripping --strip-xml-prolog --no-line-breaks >/dev/null
  rsvg-convert -w 512 -h 512 "${d}assets/org.nokkvi.nokkvi.svg" -o "${d}assets/org.nokkvi.nokkvi.png"
}

case "${1:-}" in
  --check)
    tmp=$(mktemp -d)
    mkdir -p "$tmp/assets"
    derive_into "$MASTER" "$tmp/"
    rc=0
    for f in nokkvi_logo.svg org.nokkvi.nokkvi.svg nokkvi_logo_readme.svg org.nokkvi.nokkvi.png; do
      if ! cmp -s "assets/$f" "$tmp/assets/$f"; then
        echo "DRIFT: assets/$f differs from a fresh regen of $MASTER" >&2
        rc=1
      fi
    done
    rm -rf "$tmp"
    [ "$rc" -eq 0 ] && echo "OK: all derived logo assets are in sync with $MASTER"
    exit "$rc"
    ;;
  "")
    [ -f "$MASTER" ] || { echo "no master at $MASTER; pass the source artwork once to create it" >&2; exit 1; }
    ;;
  *)
    build_master "$1" "$MASTER"
    ;;
esac

derive_into "$MASTER" ""
echo "Regenerated logo assets from $MASTER"
