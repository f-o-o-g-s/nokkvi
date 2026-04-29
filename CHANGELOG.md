# Changelog

## [Unreleased]

## v0.3.5 — 2026-04-28

### Fixes
- **Close to Tray on Wayland** — close-to-tray (introduced in 0.3.4) now actually hides the window on Hyprland / KDE / GNOME / sway. The previous implementation issued `iced::window::set_mode(Hidden)`, which winit's Wayland backend treats as a documented no-op because compositors own surface visibility — the close handler ran and the state flipped, but the window stayed on screen. Switched the iced runtime from `iced::application` to `iced::daemon` so the runtime stays alive when the last window closes, then close-to-tray dispatches `iced::window::close(id)` and the tray "Show" path opens a fresh window via `iced::window::open(...)`. App state, audio, MPRIS, scrobbling, and the tray subscription all survive the close/reopen cycle unchanged.

## v0.3.4 — 2026-04-28

### Features
- **System Tray Integration** — opt-in StatusNotifierItem icon (via `ksni`, pure-Rust SNI over zbus — no GTK, no libappindicator) keeps nokkvi playing in the background. Two general-tab toggles: **Show Tray Icon** registers/tears down the tray live, **Close to Tray** redirects window-manager close (Alt+F4, `wmctrl -c`) into `window::set_mode(Hidden)` instead of quitting the runtime. Tray menu offers Show/Hide, Play/Pause (label flips with state), Next, Previous, Quit; left-click toggles window visibility. Linux desktops without a tray host (Hyprland without waybar's `tray` module, bare wlroots) are unaffected since the feature is opt-in. Tooltip and Play/Pause label are kept current via the same push that drives MPRIS metadata.
- **Artwork Column Display Mode + Draggable Width** — the large artwork column gains four modes (Auto / Always Native / Always Stretched / Never) with a Cover/Fill sub-toggle for the stretched variant, plus a between-column drag handle as the sole resize affordance in always modes (live atomic preview during drag, TOML write on release). Auto preserves bit-identical layout to the previous behavior. Width fraction is clamped to `[0.05, 0.80]` at the service boundary and persists across mode switches.
- **Granular Player-Bar Mode Cull + Narrow-Tier Transports** — each of the seven player-bar modes (random/repeat/repeat-queue/consume/EQ/crossfade/SFX) now has its own enter threshold with 40px hysteresis, so a slow drag-resize moves exactly one element per crossing instead of culling 2–3 at once. Folded modes appear in a new `PlayerModesMenu` kebab (vertical-ellipsis glyph to disambiguate from the app hamburger) with leading checkmarks per row and an accent-dot badge on the trigger when any folded mode is active. The transport row collapses from 5 to 3 buttons (prev / play-or-pause / next) at narrow widths on its own threshold; the middle button keeps a fixed hit target across the play/pause glyph swap.

### Fixes
- **Mutually Exclusive Overlay Menus** — the hamburger, player-bar kebab, view-header checkbox dropdowns, and right-click context menus could all be open simultaneously because each widget owned its own `is_open` state. Open/closed is now lifted to a single `Nokkvi.open_menu` coordinator and each widget is controlled — opening any new menu replaces whatever was open before. Iced's overlay-first dispatch order makes "click another menu's trigger to switch directly" work without ordering hacks. Anchored overlays auto-close on view switch and window resize so they don't strand themselves at stale screen positions.

## v0.3.3 — 2026-04-27

### Fixes
- **Crash on Sub-Pixel Image Layouts** — pinned `iced` to a fork rev containing [PR #3292](https://github.com/iced-rs/iced/pull/3292), which fixes a `return`-instead-of-`continue` typo in `wgpu/src/image/State::prepare`. Without it, any image in a render batch with bounds rounding to less than one physical pixel desyncs the GPU instance buffer from recorded layer groups. This presented as missing slot artwork and player-bar SVGs at certain window sizes (most reliably triggered by the Songs view in Most Played sort) and then as a SIGABRT (`Instance N extends beyond limit M imposed by the buffer in slot 0`) on the next prepare cycle. Pinned to `f-o-o-g-s/iced` rev `8d69450c` (upstream `12a01265` plus the two-line fix); will revert to upstream once #3292 lands.
- **Cargo License SPDX Identifier** — workspace `license` field updated to the current SPDX identifier `GPL-3.0-only`. The deprecated bare `GPL-3.0` form would eventually have tripped tooling that validates against the current registry.

### Improvements
- **Launcher Icon Rendering** — added a 512px PNG raster (`assets/nokkvi.png`) alongside the existing SVG. Some launchers fail to render the gradient SVG via older librsvg paths and fall back to a generic icon; the PNG ensures consistent icon display across desktop environments.

### Internal
- **Release CI → Docs Dispatch** — `.github/workflows/release.yml` now fires `repository_dispatch` at `f-o-o-g-s/nokkvi-docs` after a release publish so the documentation site regenerates automatically.
- **/package Workflow** — Step 8 now syncs both `nokkvi-bin` and `nokkvi-git` AUR packages after release publish, with an idempotency guard on `nokkvi-bin` that prevents `pkgrel` regression if the workflow re-runs for the same version.
- **README + CHANGELOG Cleanup** — Known Issues section removed from the README now that the iced sub-pixel bug is patched; AUR badge placeholders replaced with live package links; CHANGELOG pruned to public-release commits.
- **Cargo.lock Sync** — committed stale lockfile sync to 0.3.2; gitignored `scheduled_tasks.lock`.

## v0.3.2 — 2026-04-27

### Fixes
- **About Modal "Commit: unknown"** — the About modal no longer shows a "Commit: unknown" placeholder when the binary is built outside a git context (e.g. from the GitHub-auto-generated source tarball). The Commit row is now hidden cleanly in that case. Git-clone builds and the prebuilt CI release tarball still display the real short commit hash. Mirrors rmpc's `vergen-gitcl` + `option_env!` pattern.

## v0.3.1 — 2026-04-27

### Features
- **ReplayGain Playback** — added per-track and per-album volume normalization driven by the `replayGain` tags surfaced by the Subsonic API. Replaces the boolean AGC toggle with a four-mode picker (Off / ReplayGain Track / ReplayGain Album / AGC), plus a pre-amp slider, untagged-track fallback (configurable dB or fall-through to AGC), and peak-aware clipping prevention. Static gains are applied via rodio's `amplify` source ahead of the limiter, so both sides of a crossfade are pre-leveled with no convergence delay. Existing `volume_normalization: true` settings are migrated in-place to AGC mode on first load.

### Fixes
- **Workspace License Metadata** — corrected the workspace `license` field to `GPL-3.0` so both `nokkvi` and `nokkvi-data` crates inherit the correct value; previously cargo reported no license at all despite the `LICENSE` file and README always specifying GPLv3.

### Improvements
- **Quieter Default Logging** — terminal launches no longer dump 50+ debug lines on startup. The stderr layer now defaults to `WARN+`, while the file log at `~/.config/nokkvi/nokkvi.log` keeps full debug context for bug reports. `RUST_LOG`, when set, applies consistently to both layers.
- **Tarball Releases** — added a Download section to the README pointing at the new GitHub Releases tarball (`nokkvi-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` + `.sha256`). `install.sh` auto-detects whether it's running from an extracted tarball or a source build and installs from the right location.

### Internal
- **Tag-Driven Release Workflow** — added `.github/workflows/release.yml`. Pushing a `vX.Y.Z` tag (or `workflow_dispatch`) builds a release-mode x86_64 Linux binary on `ubuntu-latest`, bundles it with `assets/`, `themes/`, `install.sh`, README, CHANGELOG, and LICENSE into a versioned tarball, computes a sha256, and attaches both files to a draft GitHub Release whose body is the matching CHANGELOG section. Manual dispatch produces the same tarball as a workflow artifact for dry-run testing without creating a tag or release.
- **Bump-Policy Gate** — the release workflow now compares each pushed tag against the most recent prior tag and decides the bump kind. Patch bumps proceed silently and may grow arbitrarily; minor and major bumps require an annotated-tag message body containing `allow-minor-bump` or `allow-major-bump`. Lightweight tags pushed for a non-patch bump fail the gate before the build step runs. Non-strictly-increasing tags are also rejected.
- **/package Workflow Rewrite** — replaced the local source-zip flow with a tag-driven release workflow. Defaults to a patch bump regardless of changeset type; minor/major bumps require explicit user authorization in the conversation. Updated both `.agent/workflows/package.md` and `.claude/commands/package.md` to stay in sync.
- **Retired `package.sh`** — removed the local source-zip script (and its `dist/` output dir) now that GitHub Releases produce the binary tarball directly and auto-generate source archives for every tag.
