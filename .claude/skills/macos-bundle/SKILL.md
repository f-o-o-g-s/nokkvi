---
name: macos-bundle
description: Packaging and launching nokkvi as a macOS app — building the .app bundle (Info.plist, bundle id, .icns from the existing logo), ad-hoc vs Developer ID code signing, notarization, DMG/zip, Apple-silicon/universal targets, the macOS leg of the release workflow and /package, and how the owner runs a build for manual testing. Use when creating or changing bundle scripts, Info.plist, icons, release.yml macOS jobs, or when a feature (notifications, media keys, permissions) only works from a bundle. Load macos-port first for the general rules.
---

# macOS bundle and release

## Why a bundle matters even in development

A bare `target/release/nokkvi` runs fine for most testing, but on macOS several APIs key off the bundle:

- `UNUserNotificationCenter` refuses to work without a `CFBundleIdentifier`.
- The Dock icon, app name in the menu bar and Cmd+Tab come from `Info.plist` + `.icns` (a bare binary shows a generic exec icon and the binary name).
- Privacy prompts and per-app settings (Notifications, Login Items) are tracked per bundle id and per code signature — re-signing with a different identity resets them.

So once notifications or Now Playing land, the owner tests from the `.app`, not the bare binary. Say which one to launch in every handoff.

## Bundle layout

```
Nokkvi.app/Contents/
├── Info.plist
├── MacOS/nokkvi          # the release binary, unchanged
└── Resources/nokkvi.icns
```

Info.plist essentials: `CFBundleIdentifier` = `org.nokkvi.nokkvi` (same id as the Linux `application_id` and `.desktop` file), `CFBundleName`/`CFBundleDisplayName` = `Nokkvi`, `CFBundleExecutable` = `nokkvi`, `CFBundleIconFile` = `nokkvi`, `CFBundlePackageType` = `APPL`, `CFBundleShortVersionString` + `CFBundleVersion` from the `Cargo.toml` version (generate, never hand-edit a copy that can drift), `LSMinimumSystemVersion`, `NSHighResolutionCapable` = true, `LSApplicationCategoryType` = `public.app-category.music`. Add usage strings only for permissions the app really requests.

The CLI path (`nokkvi <verb>`, fork-before-iced in `src/main.rs` + `nokkvi-ipc`) must keep working against the bundled binary: `Nokkvi.app/Contents/MacOS/nokkvi play` talks to the running instance. A symlink on `PATH` (e.g. `~/.local/bin/nokkvi` → the bundled binary) is the macOS counterpart of `install.sh`.

## Icon

`assets/macos/nokkvi.icns` (committed) is rendered from `assets/macos/nokkvi_icon.svg`: the README mark (`assets/nokkvi_logo_readme.svg`) inset on an 824px dark graphite rounded-square plate (neutral greys in place of the mark's teal tint) centred on a 1024px canvas (the Big Sur+ icon grid). Both come from `sh scripts/gen-logo-assets.sh --macos-icon`, which needs only `rsvg-convert` (`brew install librsvg`) and `iconutil`, so it runs on a Mac where the script's other modes (scour, GNU `sed -i`) don't. Rerun it when the logo or the plate colours change and commit both files; builds never rasterize.

## Building

`scripts/bundle-macos.sh` is the one way to build the bundle, so the owner and CI produce the same thing: it runs `cargo build --release`, assembles `target/release/bundle/Nokkvi.app` with an Info.plist generated from the `cargo metadata` version, and ad-hoc signs it. `--install` swaps it into `~/Applications/Nokkvi.app` (stage-then-rename, so a running instance keeps its old binary) and re-registers it with LaunchServices; `--no-build` reuses the binary, `--debug` bundles the debug build. The development loop is: edit → `scripts/bundle-macos.sh --install` → quit and relaunch Nokkvi. A launch while an instance runs only hands off to that instance (single-instance lock), so quit the old one first to test a new build. Introducing `cargo-bundle` or another packaging crate/tool is a new dependency → ask the owner first; the script approach needs only Xcode Command Line Tools (`plutil`, `iconutil`, `codesign`, `hdiutil`).

Targets: the Mac here is Apple silicon (`aarch64-apple-darwin`). Intel (`x86_64-apple-darwin`) or a universal binary (`lipo -create`) only if the owner wants it — each extra target doubles the build and needs `rustup target add`, and cmake must cross-compile the bundled libopus.

Output goes under `target/` (e.g. `target/release/bundle/Nokkvi.app`), never into the source tree.

## Signing and notarization

- **Ad-hoc** (`codesign --force --deep -s - Nokkvi.app`): Apple silicon only runs signed code; the Rust linker already ad-hoc-signs the bare binary, but the bundle needs its own signature so Info.plist is sealed with it. Re-sign after every binary swap. Enough for the owner's own machine; no Developer ID identity is installed on this Mac.
- **Developer ID + notarization** is needed to distribute without Gatekeeper's "unidentified developer" block: a paid Apple Developer account, `codesign --options runtime --timestamp -s "Developer ID Application: …"`, then `xcrun notarytool submit … --wait` and `xcrun stapler staple`. This involves the owner's Apple credentials — never handle them yourself; the owner runs the credentialed steps or stores them as CI secrets.
- Hardened runtime may need entitlements (e.g. `com.apple.security.cs.allow-jit` is *not* needed for wgpu/Metal; network client access needs none outside the sandbox). Don't enable App Sandbox: the config watcher, IPC socket in `$TMPDIR` and arbitrary server URLs would all need exceptions.

## Release workflow

Today `.github/workflows/release.yml` builds only `x86_64-unknown-linux-gnu` and `/package` (`.claude/skills/package/SKILL.md`) uploads a tarball + `.sha256` and syncs the AUR. A macOS leg means:

1. A `macos-latest` job in `release.yml` that builds `aarch64-apple-darwin`, runs the bundle script, zips (`ditto -c -k --keepParent Nokkvi.app …zip`) or makes a DMG (`hdiutil create`), and attaches it with its `.sha256` to the same draft release.
2. `/package` step 7 (watch + publish) checks the macOS asset too; README install instructions gain a macOS section. Update the `package` skill in the same change — it is the canonical release procedure.
3. Unsigned/ad-hoc artifacts make users right-click → Open (or `xattr -dr com.apple.quarantine`); say so in the release notes until notarization exists.

Version bumps still go only through `/package` (code-standards.md).

## Handoff for manual testing

After the four gates and `cargo build --release`: tell the owner exactly what to launch — `target/release/nokkvi` from Terminal, or the rebuilt `.app` (and whether it needs re-signing) — and what to look at. Logs: `~/Library/Application Support/nokkvi/nokkvi.log` (truncated each launch).
