# Platform map: Linux → macOS

Status as of 2026-11-18 (re-check with the greps in SKILL.md step 0; update this table in the commit that changes a row).

Status legend: **works** = native on macOS · **fallback** = builds and runs, generic or degraded behaviour · **stub** = compiled out / no-op on macOS · **broken** = compiles, misbehaves.

| Subsystem | Linux implementation | Files | macOS status | Target on macOS | Skill |
|-----------|---------------------|-------|--------------|-----------------|-------|
| Music output | Native PipeWire stream (`NativePipeWireSink`), ALSA/cpal fallback | `data/src/audio/pipewire_output.rs`, `sfx_engine.rs::open_preferred_sink` | fallback (cpal → CoreAudio default device) | cpal/CoreAudio default device; optional native CoreAudio sink for device selection + rate switching | `macos-audio` |
| Bit-perfect mode | PipeWire `node.rate` request + `/proc/asound` device-rate read + graph probe for the HOLDER | `graph_probe.rs`, `src/state/playback.rs`, `src/update/playback.rs` | stub (`probe_sink_holder` → `None`, `/proc/asound` read finds nothing) | CoreAudio `kAudioDevicePropertyNominalSampleRate` set/read on the output device; hide or relabel the badge until then | `macos-audio` |
| SFX engine | Same preferred-sink path | `data/src/audio/sfx_engine.rs` | fallback | Shares the music sink strategy | `macos-audio` |
| Media keys / Now Playing | MPRIS over D-Bus (`mpris-server`) | `src/services/mpris.rs`, `mpris_macos.rs`, `mpris_art_writer.rs`, `src/update/mpris.rs` | works (`src/services/mpris_macos.rs`: `souvlaki` → `MPNowPlayingInfoCenter` + `MPRemoteCommandCenter`, same `MprisEvent`s; art goes as a `file://` URL from the art writer) | — | `macos-desktop-integration` |
| Tray | StatusNotifierItem via `ksni` on its own thread | `src/services/tray.rs`, `src/update/tray.rs` | stub | `NSStatusItem` menu-bar extra on the main thread | `macos-desktop-integration` |
| Desktop notifications | `org.freedesktop.Notifications` via `zbus` | `src/services/notifications.rs` | stub | `UNUserNotificationCenter` (needs a bundle id → `.app`) | `macos-desktop-integration` |
| Close-to-tray / hide window | Wayland can't hide a surface → window is closed and reopened | `src/update/tray.rs`, `src/update/window.rs`, `main_window_settings()` | unverified | macOS can hide (`NSApp.hide`/`orderOut`); Dock-icon click must reopen | `macos-desktop-integration` |
| Keyboard shortcuts | Bare keys + Shift/Ctrl/Alt/Super in `HotkeyConfig` | `src/hotkeys/global.rs`, `data/src/types/hotkey_config/key_combo.rs` | done: Cmd is a bindable modifier, shortcut defaults use ⌘ (`KeyCombo::primary`), system Cmd chords are refused (`is_os_reserved`), Cmd+click multi-selects, labels use ⌃⌥⇧⌘ glyphs | — | `macos-desktop-integration` |
| Open URL | `xdg-open` | `src/update/about_modal.rs` | broken (`xdg-open` doesn't exist) | `open <url>` | `macos-desktop-integration` |
| File picker | `rfd` with `xdg-portal` feature | `src/update/components/custom_artwork.rs` | works in principle (rfd uses NSOpenPanel on macOS); unverified | keep rfd; check main-thread + glob filters | `macos-desktop-integration` |
| Hostname (player name) | `/proc/sys/kernel/hostname`, `/etc/hostname` | `data/src/services/api_key.rs::host_name` | broken (both reads fail → no host suffix) | `libc::gethostname` (libc is already a dep) or `scutil --get ComputerName` | `macos-desktop-integration` |
| PID liveness | `kill(pid, 0)` (was `/proc/<pid>`) | `src/process_liveness.rs::pid_is_alive`, used by `services/mpris_art_writer.rs` and the `src/update/rules_editor.rs` orphan draft sweep | works (shared helper; `EPERM` counts as alive) | — | `macos-port` |
| Window app id | `PlatformSpecific { application_id: "org.nokkvi.nokkvi" }` | `src/main.rs::main_window_settings` (Linux), `scripts/bundle-macos.sh` (macOS) | works (`org.nokkvi.nokkvi` in the generated Info.plist) | — | `macos-bundle` |
| Config/state/data/cache dirs | XDG via `directories` | `data/src/utils/paths.rs` | works (state falls back to `data_local_dir`, migration no-ops when equal) | `~/Library/Application Support/nokkvi`, `~/Library/Caches/nokkvi` | `macos-port` |
| MPRIS art cache path | Hand-rolled `$XDG_CACHE_HOME`/`~/.cache` | `src/services/mpris_art_writer.rs` | wrong dir on macOS (not `~/Library/Caches`) | Route through `paths::get_cache_dir()` | `macos-port` |
| IPC socket + launch lock | `$XDG_RUNTIME_DIR` or `/tmp` + `$UID` | `nokkvi-ipc/src/socket_path.rs`, `instance_lock.rs` | fallback (`/tmp/nokkvi-default-<pid>.sock`) | `$TMPDIR` (per-user, 0700) | `macos-port` |
| Config file watcher | `notify` (inotify) | `data/src/utils/paths.rs` (`was_internal_write`), config watcher | unverified (FSEvents backend) | FSEvents reports canonical paths (`/private/var/...` for temp dirs); compare canonicalized paths | `macos-port` |
| Fonts | `font-kit` → fontconfig | `data/` (font enumeration) | works in principle (CoreText backend) | — | — |
| GPU / visualizer / MilkDrop | wgpu (Vulkan) | `src/` visualizer, `particle-milkdrop` | unverified on Metal | wgpu Metal backend; watch limits/bind groups and surface formats | — |
| Install | `install.sh` → `~/.local/bin`, `.desktop`, hicolor icon | `install.sh`, `assets/org.nokkvi.nokkvi.desktop`; macOS: `scripts/bundle-macos.sh`, `assets/macos/` | works (`bundle-macos.sh --install` → ad-hoc-signed `~/Applications/Nokkvi.app`) | Developer ID signing + notarization for distribution | `macos-bundle` |
| Release | x86_64-unknown-linux-gnu tarball, AUR | `.github/workflows/release.yml`, `/package` skill | n/a | `aarch64-apple-darwin` (+ optionally universal) `.app` in a DMG/zip | `macos-bundle` |
| CI | ubuntu-latest only | `.github/workflows/ci.yml` | none | add a `macos-latest` job | `macos-port` |
| System deps | `pacman -S pipewire alsa-lib fontconfig pkgconf cmake clang` | CLAUDE.md, `flake.nix`, `PKGBUILD` | — | Xcode Command Line Tools + `brew install cmake` (bundled libopus); no PipeWire | `macos-port` |
