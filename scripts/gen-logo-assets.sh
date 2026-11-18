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
# macOS app icon (committed, regenerated only by --macos-icon, which needs only
# rsvg-convert + macOS's iconutil, so it runs on a Mac without scour/GNU sed):
#   assets/macos/nokkvi_icon.svg    1024px composite: rounded-square plate + README mark
#   assets/macos/nokkvi.icns        16..1024px icon set for Nokkvi.app (scripts/bundle-macos.sh)
#
# Usage:
#   sh scripts/gen-logo-assets.sh <artwork.svg>   rebuild master from new art, then derive all
#   sh scripts/gen-logo-assets.sh                 re-derive everything from the existing master
#   sh scripts/gen-logo-assets.sh --check         verify committed derived assets are in sync (exit 1 on drift)
#   sh scripts/gen-logo-assets.sh --macos-icon    rebuild the macOS icon from assets/nokkvi_logo_readme.svg
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

build_macos_icon() {
  # Big Sur+ icon grid: an 824px rounded-square plate centred on a 1024px canvas
  # (the 100px margin leaves room for the drop shadow the system expects), with
  # the mark inset inside it. The mark's square viewBox is reused verbatim.
  mark="assets/nokkvi_logo_readme.svg"
  out_dir="assets/macos"
  viewbox=$(sed -E 's/^<svg[^>]* viewBox="([^"]*)".*/\1/' "$mark")
  # The mark's teal-tinted greys are swapped for neutral ones here (macOS icon
  # only), so the icon reads as plain graphite with no green cast.
  inner=$(sed -E 's/^<svg[^>]*>//; s/<\/svg>[[:space:]]*$//' "$mark" \
    | sed 's/#6d9a94/#8e9096/g; s/#e7eceb/#ececee/g; s/#111817/#0c0c0d/g')
  mkdir -p "$out_dir"
  cat > "$out_dir/nokkvi_icon.svg" <<EOF
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024"><defs><linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#26262a"/><stop offset="1" stop-color="#0e0e10"/></linearGradient><filter id="sh" x="-10%" y="-10%" width="120%" height="125%"><feDropShadow dx="0" dy="10" stdDeviation="12" flood-color="#000" flood-opacity="0.3"/></filter></defs><rect x="100" y="100" width="824" height="824" rx="185" fill="url(#bg)" filter="url(#sh)"/><rect x="102" y="102" width="820" height="820" rx="183" fill="none" stroke="#fff" stroke-opacity="0.08" stroke-width="4"/><svg x="182" y="182" width="660" height="660" viewBox="$viewbox">$inner</svg></svg>
EOF
  set_dir=$(mktemp -d)/nokkvi.iconset
  mkdir -p "$set_dir"
  for s in 16 32 128 256 512; do
    rsvg-convert -w "$s" -h "$s" "$out_dir/nokkvi_icon.svg" -o "$set_dir/icon_${s}x${s}.png"
    d=$((s * 2))
    rsvg-convert -w "$d" -h "$d" "$out_dir/nokkvi_icon.svg" -o "$set_dir/icon_${s}x${s}@2x.png"
  done
  iconutil -c icns "$set_dir" -o "$out_dir/nokkvi.icns"
  rm -rf "$(dirname "$set_dir")"
  echo "Regenerated $out_dir/nokkvi_icon.svg and $out_dir/nokkvi.icns from $mark"
}

case "${1:-}" in
  --macos-icon)
    build_macos_icon
    exit 0
    ;;
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
