# Changelog

## [Unreleased]

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
