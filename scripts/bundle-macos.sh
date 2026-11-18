#!/usr/bin/env bash
# Build Nokkvi.app for macOS from the cargo binary.
#
# Needs only Xcode Command Line Tools (plutil, codesign, ditto); no packaging
# crate. The icon is the committed assets/macos/nokkvi.icns
# (regenerate with `sh scripts/gen-logo-assets.sh --macos-icon`).
#
# Usage:
#   scripts/bundle-macos.sh              cargo build --release, then assemble
#                                        target/release/bundle/Nokkvi.app
#   scripts/bundle-macos.sh --install    same, then install to ~/Applications/Nokkvi.app
#                                        (Launchpad / Spotlight / Dock pick it up there)
#   scripts/bundle-macos.sh --no-build   reuse the existing target/release/nokkvi
#   scripts/bundle-macos.sh --debug      bundle the debug build (target/debug/bundle)
#
# Development loop: edit code → run this with --install → relaunch the app.
# The bundled binary is a plain copy, so every rebuild needs a re-bundle; the
# script re-signs ad-hoc each time so Info.plist stays sealed with the binary.
set -euo pipefail

[[ "$(uname -s)" == "Darwin" ]] || { echo "bundle-macos.sh runs on macOS only" >&2; exit 1; }

ROOT=$(git rev-parse --show-toplevel)
cd "$ROOT"

profile=release
build=1
install=0
for arg in "$@"; do
  case "$arg" in
    --install) install=1 ;;
    --no-build) build=0 ;;
    --debug) profile=debug ;;
    -h | --help) sed -n '2,18p' "$0"; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

if [[ "$build" -eq 1 ]]; then
  if [[ "$profile" == release ]]; then cargo build --release -p nokkvi; else cargo build -p nokkvi; fi
fi

bin="target/$profile/nokkvi"
[[ -x "$bin" ]] || { echo "no binary at $bin; build first" >&2; exit 1; }

# Version comes from the root package so the bundle never drifts from Cargo.toml.
version=$(cargo metadata --no-deps --format-version 1 \
  | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="nokkvi"))')

app="target/$profile/bundle/Nokkvi.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/nokkvi"
cp assets/macos/nokkvi.icns "$app/Contents/Resources/nokkvi.icns"

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleIdentifier</key><string>org.nokkvi.nokkvi</string>
	<key>CFBundleName</key><string>Nokkvi</string>
	<key>CFBundleDisplayName</key><string>Nokkvi</string>
	<key>CFBundleExecutable</key><string>nokkvi</string>
	<key>CFBundleIconFile</key><string>nokkvi</string>
	<key>CFBundlePackageType</key><string>APPL</string>
	<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
	<key>CFBundleShortVersionString</key><string>$version</string>
	<key>CFBundleVersion</key><string>$version</string>
	<key>LSMinimumSystemVersion</key><string>11.0</string>
	<key>LSApplicationCategoryType</key><string>public.app-category.music</string>
	<key>NSHighResolutionCapable</key><true/>
	<key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
EOF
plutil -lint "$app/Contents/Info.plist" >/dev/null

# Ad-hoc signature: Apple silicon only runs signed code, and the bundle needs
# its own seal over Info.plist + resources (the linker only signed the binary).
codesign --force --sign - "$app"

echo "Built $app (v$version, $profile)"

if [[ "$install" -eq 1 ]]; then
  dest="$HOME/Applications/Nokkvi.app"
  mkdir -p "$HOME/Applications"
  # Stage then swap, so a running instance keeps its (now unlinked) binary
  # instead of having its pages rewritten underneath it.
  staging="$HOME/Applications/.Nokkvi.app.new"
  rm -rf "$staging"
  ditto "$app" "$staging"
  rm -rf "$dest"
  mv "$staging" "$dest"
  # Nudge LaunchServices so a changed icon/Info.plist shows up right away.
  /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$dest" || true
  echo "Installed $dest"
fi
