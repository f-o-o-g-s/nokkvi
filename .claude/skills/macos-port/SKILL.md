---
name: macos-port
description: Ground rules for porting nokkvi to macOS — read BEFORE touching any platform-specific code (anything behind cfg(target_os), PipeWire/ALSA audio, MPRIS, the ksni tray, D-Bus notifications, XDG paths, the IPC socket, xdg-open, rfd, window/close behaviour, Cmd shortcuts, the .app bundle) or when a build, test or lint fails only on macOS. Covers the current port status, cfg-gating conventions that keep Linux green, how to verify on a Mac, and which sibling skill owns each subsystem.
---

# Porting nokkvi to macOS

nokkvi was written Linux-only (Arch + Wayland/Hyprland + PipeWire). The owner is now making it a first-class **macOS** app too. This skill **overrides** every "Linux-only" / "No macOS support" statement in `CLAUDE.md`, `README.md` and `.claude/rules/`: those describe the history, not the goal. Linux stays a supported platform — a port that breaks Linux is a regression.

Sibling skills (load the one for the area you are touching):

| Area | Skill |
|------|-------|
| Audio output, bit-perfect, device rate, SFX sink | `macos-audio` |
| Now Playing / media keys (MPRIS), menu-bar tray, notifications, window hide/reopen, Cmd shortcuts, open URL, file dialog, hostname | `macos-desktop-integration` |
| `.app` bundle, icon, Info.plist, codesign/notarize, DMG, release workflow, how the owner launches a build | `macos-bundle` |

`platform-map.md` (next to this file) is the subsystem-by-subsystem inventory: Linux implementation, file, current macOS state, target approach. Read it before planning any port step, and **update its status column in the same commit** that changes a subsystem's state.

## Step 0: find out where the port actually is

The port is in progress and partly uncommitted. Never trust a status table (including the one in `platform-map.md`) over the tree:

```bash
git status --short
grep -rnE 'cfg\((not\()?(target_os|unix|windows)|cfg\(any\(target_os' src data/src nokkvi-ipc/src build.rs
grep -rnE 'proc/|/etc/|XDG_|xdg-open|dbus|zbus|ksni|pipewire' src data/src nokkvi-ipc/src
```

Uncommitted edits you did not make belong to the owner or a parallel session — build on them, don't revert or restage them (`/commit` stages only your own files).

## cfg-gating conventions

- **Name the OS you mean.** `cfg(unix)` is true on macOS. Use `target_os = "linux"` for Linux-only code (PipeWire, D-Bus, `/proc`), `target_os = "macos"` for macOS-only code. Reserve `not(target_os = "linux")` for a genuinely generic fallback (e.g. the cpal default sink), never as a synonym for "macOS".
- **One interface, per-OS backends.** The UI talks to desktop services through platform-neutral `*Event` / `*Command` / `*Connection` types (`MprisEvent`, `TrayEvent`, `NotificationCommand`, …) and handles them in `src/update/{mpris,tray,notifications}.rs`. Keep those types and handlers shared; put each OS's implementation in its own module/file (`services/mpris.rs` → e.g. `services/now_playing/{linux,macos}.rs` or `#[cfg]` submodules) behind the same `run() -> impl Sipper<Never, Event>` shape. Handler tests in `src/update/tests/` must then run unchanged on both OSes.
- **Gate at the declaration, not with blanket allows.** Prefer `#[cfg(target_os = "linux")] mod pipewire_output;` (as `data/src/audio/mod.rs` does) over a module-wide `allow(dead_code)`. When an item is only dead on one OS, use `#[cfg_attr(not(target_os = "linux"), expect(dead_code, reason = "…"))]` — `expect` fails once the code becomes live there, so the suppression can't go stale (code-standards.md). Every suppression carries a `reason`.
- **cfg'd enum variants need cfg'd arms.** `ActiveSink` (`data/src/audio/sfx_engine.rs`) is the model: the variant and each match arm carry the same `#[cfg]`; no `_ =>` (the `match_wildcard_for_single_variants` lint denies it anyway).
- **Platform crates go in target tables.** `[target.'cfg(target_os = "macos")'.dependencies]` / `[target.'cfg(target_os = "linux")'.dependencies]` in the crate's `Cargo.toml` (data/ already does this for `pipewire`). Root `Cargo.toml` still pulls `mpris-server`, `ksni`, `zbus` unconditionally — moving them under the Linux table is part of the port. **Adding any crate needs the owner's OK first** (CLAUDE.md): propose it with the reason and alternatives, then wait.
- **Keep the crate boundaries**: `nokkvi-data` and `nokkvi-ipc` stay iced-free on every OS. macOS-only UI glue (AppKit, `objc2`) lives in the root crate unless it is pure backend (CoreAudio in `data/`).
- **No `unsafe` without a `// SAFETY:` comment.** objc2/CoreAudio FFI is the main source of new `unsafe`; keep it in small wrapper fns, never sprinkled through handlers.
- **Main-thread APIs.** AppKit (status item, menus, NSApp activation, file panels) must run on the main thread, which iced/winit owns. Don't spawn a thread and call AppKit from it the way the ksni tray does on Linux — see `macos-desktop-integration`.

## Paths on macOS

`data/src/utils/paths.rs` uses `directories::BaseDirs`. On macOS that resolves to:

| Role | Linux | macOS (`directories`) |
|------|-------|-----------------------|
| config (`config.toml`, `themes/`, `sfx/`, `milkdrop/`) | `~/.config/nokkvi` | `~/Library/Application Support/nokkvi` |
| state (`app.redb`, `nokkvi.log`) | `~/.local/state/nokkvi` | no `state_dir` → falls back to `data_local_dir()` = `~/Library/Application Support/nokkvi` (same as config) |
| data (lyrics) | `~/.local/share/nokkvi` | `~/Library/Application Support/nokkvi` |
| cache (MPRIS art, waveforms) | `~/.cache/nokkvi` | `~/Library/Caches/nokkvi` |

Consequences: config, state and data share one directory on macOS, so `migrate_to_state_dir()` must no-op when the dirs are equal (it does), and a file name must not collide across roles. Doc comments and rules that say `~/.config/nokkvi` describe Linux — write new docs as "config dir (`~/.config/nokkvi` on Linux, `~/Library/Application Support/nokkvi` on macOS)". Code that hand-rolls XDG resolution (`src/services/mpris_art_writer.rs` `$XDG_CACHE_HOME`) must go through `nokkvi_data::utils::paths` instead. Tests must not assert `.config` / `.local` substrings.

IPC (`nokkvi-ipc/src/socket_path.rs`, `instance_lock.rs`): `$XDG_RUNTIME_DIR` is unset on macOS, so the socket and lock fall back to `/tmp` with a `$UID`-suffixed name — and `UID` is a shell variable, usually not exported to the environment, so the suffix becomes `default`. The per-user location on macOS is `$TMPDIR` (`/var/folders/…/T/`, 0700). Also: `sun_path` is 104 bytes on macOS (108 on Linux), so keep socket paths short. `flock` works on macOS APFS.

## Verifying on a Mac

The four CI gates from CLAUDE.md all run natively on macOS and must stay green there:

```bash
cargo build
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

Then `cargo build --release` for the owner. There is no release-build Stop hook on the Mac checkout (`local/` and `.claude/settings.local.json` are absent), so run the release build yourself before handing off, and say what to look at in the running window.

**Tests that read the real config** (2026-11-18): `widgets::slot_list::tests::highlight_slots_force_legible_ink` loads the active theme from the user's config dir, so it fails on a machine whose theme has equal `fg0`/`fg4` (moonfly) and passes with `HOME` pointed at an empty dir; it is not a platform failure. Check against a clean `HEAD` worktree, and with an empty `HOME`, before calling a failure platform-specific. Never `#[cfg]`-out or `#[ignore]` a test to make macOS green; make the code correct on both OSes, and give the test a per-OS expectation only where the behaviour is meant to differ. Linux-only probes (`/proc`, `/etc`, `/sys`) are the usual cause of a real macOS failure: `grep -rn '"/proc\|"/etc\|"/sys' src data/src nokkvi-ipc/src`.

**Linux can't be compiled from the Mac**: `pipewire-sys`/`libspa-sys` run bindgen against system PipeWire headers. So:

- Every `#[cfg(target_os = "linux")]` edit is unverified until Linux CI (ubuntu-latest, `.github/workflows/ci.yml`) passes. Say so in the handoff. Read the Linux branch of each `cfg` you touch as carefully as if you were compiling it by eye: imports used only on one side, `expect(unused_…)` that would be unfulfilled on Linux, arms missing for cfg'd variants.
- Optional local check if Docker is available: `git archive HEAD | docker run -i --rm rust:latest sh -c 'mkdir /src && tar x -C /src && apt-get update && apt-get install -y libpipewire-0.3-dev libfontconfig1-dev pkg-config libasound2-dev cmake libclang-dev && cd /src && cargo clippy --workspace --all-targets -- -D warnings'` (archive a commit, never mount the live tree with `target/`).
- CI runs a `macos-latest` job alongside `ubuntu-latest` in `.github/workflows/ci.yml` (fmt-check, clippy `-D warnings`, test, release build; no apt step since `cmake` is preinstalled on GitHub's macOS runners).

## When the port changes conventions

The rule files and CLAUDE.md still describe a Linux-only app. When a commit makes a subsystem work on macOS, update in the same commit: the status column in `platform-map.md`, the matching `.claude/rules/` file (most often `gotchas/desktop.md`, `audio-engine.md`, `backend-services.md`), and any CLAUDE.md line that is now wrong (system deps, XDG roots, "Linux-only"). `/sync-rules` audits the rest.
