<p align="center"><img src="assets/nokkvi_logo_readme.svg" width="96" alt="Nokkvi logo" /></p>

<h1 align="center">Nokkvi</h1>

<p align="center">
  <a href="https://github.com/f-o-o-g-s/nokkvi/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/f-o-o-g-s/nokkvi/ci.yml?branch=main&label=CI" alt="CI status" /></a>
  <a href="https://aur.archlinux.org/packages/nokkvi-bin"><img src="https://img.shields.io/aur/version/nokkvi-bin?label=AUR" alt="AUR version" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/f-o-o-g-s/nokkvi" alt="License" /></a>
</p>

A native Rust/Iced client for [Navidrome](https://www.navidrome.org/) music server. Named after Old Norse *nökkvi*, a small, humble boat.

<img src="assets/nokkvi_demo.webp?v=2" width="100%" alt="Nokkvi theme and visualizer demo" />

*Demo of Nokkvi's GPU-accelerated audio visualizers and how the layout adapts to different window sizes.*

> **⚠️ AI-Generated Project**
>
> This entire codebase was written by AI (primarily Claude) with my direction. I'm not a developer. I come up with the ideas, test things, and use this as my daily music player, but I don't write code myself. You'll probably find patterns in here that experienced Rust developers would do differently. If you spot something that could be better, issues and PRs are welcome.

**Platform:** Linux only. Built and tested on Arch Linux (Wayland/Hyprland) with PipeWire v<!-- pipewire-version -->1.6.8<!-- /pipewire-version --> and Navidrome v<!-- navidrome-version -->0.63.2<!-- /navidrome-version -->. No Windows or macOS support.
**Network:** Built and tested on LAN. Background read-ahead buffering and pause-and-rebuffer on underrun keep streaming smooth over slower or remote (WAN) connections.

## 📖 Documentation

Full guides, config reference, keyboard shortcuts, theming, and visualizer details are at the **[Nokkvi docs site](https://f-o-o-g-s.github.io/nokkvi-docs/)**.

## Inspirations

Things that shaped this project:

- **[rmpc](https://github.com/mierak/rmpc)**: My previous daily driver, a terminal MPD client.
- **[Feishin](https://github.com/jeffvli/feishin)**: Referenced heavily for Navidrome API coverage and enums.
- **[mpd](https://github.com/MusicPlayerDaemon/MPD)**: Shaped the queue and consume logic.
- **[fooyin](https://github.com/fooyin/fooyin)**: Referenced for the native PipeWire implementation.
- **[cava](https://github.com/karlstav/cava)**: The visualizer DSP (`spectrum.rs`) is a Rust port of cavacore, and the Monstercat smoothing filter is ported from cava.c.
- **[StepMania](https://github.com/stepmania/stepmania)**: Inspired the MusicWheel-style slot list (fixed odd-row centered viewport, height-adaptive) and the roulette wheel's discrete-tick decel.
- **Vim**: Inspired the color schemes and keyboard-first approach.

## Highlights

- Native **PipeWire** audio engine: gapless playback, a 10-band EQ, ReplayGain and AGC normalization, and configurable crossfade with transition fades.
- Opt-in **bit-perfect** playback (Strict or Relaxed, follows the native sample rate) with a badge that reads the real hardware clock as `BIT-PERFECT`, `RESAMPLED`, or `UNVERIFIED`.
- GPU visualizer with `bars`, `lines`, and `scope` modes plus beat glow, bloom, trails, echo, and CRT, shown in a band above the player bar or over the cover art.
- **Smart playlists** authored in-app: a keyboard-first rules editor with live validation, presets, a raw-JSON mode, and server-evaluated previews. Also imports `.nsp` (needs Navidrome 0.61+).
- **Trawl** mix builder: blend artist, album, song, genre, and playlist seeds into a crate (round-robin, weighted, or shuffle-all) and save the result as a playlist.
- Scriptable from the shell: `nokkvi <verb>` drives the running player over a local socket (transport, volume, modes, love and rate, queue push/pull).
- **23 built-in themes** (default **Svalbard**), drop-in `.toml` with instant hot-reload, a searchable picker that paints each row in its own palette, and two icon sets (Phosphor, Lucide).
- Built for tiling WMs: a width-adaptive player bar and a **slot-paginated list** (up to 29 whole rows, no scrollbar, contents scale per slot).
- Plus the essentials: browse albums, artists, songs, genres, playlists, radio, and similar songs; a **Harbour** home with whole-library search; persistent queue, multi-select, drag-and-drop, star ratings; synced lyrics; custom cover art; server queue sync (Navidrome 0.58.5+); scrobbling (library and radio); MPRIS; an optional tray icon; and full keyboard control.

Full feature tour and `config.toml` reference: [docs](https://f-o-o-g-s.github.io/nokkvi-docs/).

### Non-Goals

Nokkvi is a fast, keyboard-driven music player, not a Navidrome admin panel. These features are intentionally left out for now:
- Server administration (library scans, user management)
- Podcasts
- Jukebox mode
- Bookmarks
- Public sharing links
- Offline download

## Download

**Prebuilt binary (x86_64 Linux):** grab the latest tarball from [Releases](https://github.com/f-o-o-g-s/nokkvi/releases). Each release ships `nokkvi-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` plus a matching `.sha256`. Verify, extract, and install:

```bash
sha256sum -c nokkvi-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz.sha256
tar xzf nokkvi-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz
cd nokkvi-vX.Y.Z-x86_64-unknown-linux-gnu
./install.sh
```

Runtime requirements: `pipewire`, `alsa-lib`, and `fontconfig` installed system-wide (Arch: `sudo pacman -S pipewire alsa-lib fontconfig`).

**Arch (AUR):** [`nokkvi-bin`](https://aur.archlinux.org/packages/nokkvi-bin) tracks the released tarballs above; [`nokkvi-git`](https://aur.archlinux.org/packages/nokkvi-git) builds from `main`. Install with your AUR helper of choice (e.g. `yay -S nokkvi-bin` or `paru -S nokkvi-bin`).

## Quickstart (build from source)

```bash
sudo pacman -S pipewire alsa-lib fontconfig pkgconf cmake # Arch system deps (cmake builds bundled libopus)
cargo build --release                           # build
./install.sh                                    # install binary, .desktop, icon
```

The binary goes to `target/release/nokkvi`; `install.sh` copies it to `~/.local/bin/nokkvi` with the desktop entry and icon. Config lives in `~/.config/nokkvi/`; runtime state and logs live in `~/.local/state/nokkvi/`. See [debug logging](CONTRIBUTING.md#debug-logging) for the `RUST_LOG` escape hatch when filing bug reports.

More detail in the docs:
- [Installation](https://f-o-o-g-s.github.io/nokkvi-docs/guides/installation/)
- [Connecting to Navidrome](https://f-o-o-g-s.github.io/nokkvi-docs/guides/navidrome/)
- [Configuration reference](https://f-o-o-g-s.github.io/nokkvi-docs/reference/config/)
- [Keyboard shortcuts](https://f-o-o-g-s.github.io/nokkvi-docs/reference/hotkeys/)
- [Media controls (MPRIS)](https://f-o-o-g-s.github.io/nokkvi-docs/guides/mpris/)

Build setup and contributor workflow (including the **nightly** rustfmt requirement) are in [CONTRIBUTING.md](CONTRIBUTING.md).
