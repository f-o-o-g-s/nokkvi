<p align="center"><img src="assets/nokkvi_logo_readme.svg" width="96" alt="Nokkvi logo" /></p>

# Nokkvi

<p align="center">
  <a href="https://github.com/f-o-o-g-s/nokkvi/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/f-o-o-g-s/nokkvi/ci.yml?branch=main&label=CI" alt="CI status" /></a>
  <a href="https://aur.archlinux.org/packages/nokkvi-bin"><img src="https://img.shields.io/aur/version/nokkvi-bin?label=AUR" alt="AUR version" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/f-o-o-g-s/nokkvi" alt="License" /></a>
</p>

A native Rust/Iced client for [Navidrome](https://www.navidrome.org/) music server. Named after Old Norse *nökkvi*, a small, humble boat.

<img src="assets/nokkvi_demo.webp?v=2" width="100%" alt="Nokkvi theme and visualizer demo" />

*Demo showcasing Nokkvi's GPU-accelerated audio visualizers and how the layout adapts to different window sizes.*

> **⚠️ AI-Generated Project**
>
> This entire codebase was written by AI (primarily Claude) with my direction. I'm not a developer. I come up with the ideas, test things, and use this as my daily music player, but I don't write code myself. You'll probably find patterns in here that experienced Rust developers would do differently. If you spot something that could be better, issues and PRs are welcome.

**Platform:** Linux only. Built and tested on Arch Linux (Wayland/Hyprland) with PipeWire v<!-- pipewire-version -->1.6.7<!-- /pipewire-version --> and Navidrome v<!-- navidrome-version -->0.62.0<!-- /navidrome-version -->. No Windows or macOS support.
**Network:** Built and tested on LAN, with background read-ahead buffering and pause-and-rebuffer on underrun so streaming over slower or remote (WAN) connections stays smooth.

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

- Native PipeWire audio engine with gapless playback, crossfade, AGC + ReplayGain volume normalization, and a 10-band EQ
- Opt-in **bit-perfect** playback that bypasses DSP (EQ, software volume, limiter) and follows each track's native sample rate, in a Strict mode or a Relaxed mode that still crossfades same-rate tracks, with a now-playing badge that reads the real hardware clock and honestly shows `BIT-PERFECT`, `RESAMPLED`, or `UNVERIFIED` rather than trusting the requested rate
- GPU-accelerated visualizer with `bars`, `lines`, and `scope` (circular oscilloscope) modes, beat-reactive neon glow and bloom, plus optional motion trails, echo, and CRT/film post-processing; `bars`/`lines` can sit in a band above the player bar or draw over the now-playing cover art
- Browse albums, artists, songs, genres, playlists, internet radios, and similar artists; inline expansion, split-view browsing, and multi-library filtering included
- 23 built-in themes (default **Svalbard**, plus Gruvbox, Catppuccin, Dracula, Nord, Tokyo Night, Kanagawa, Everforest, Firmium, ...) with instant hot-reload; drop a `.toml` in `~/.config/nokkvi/themes/` to add your own
- Persistent queue, multi-selection, drag-and-drop, star ratings, and scrobbling (Last.fm / ListenBrainz)
- Fully keyboard-driven with configurable shortcuts, MPRIS, optional system tray icon, and right-click menus everywhere
- Designed on a tiling WM — player bar folds controls into a kebab menu as width shrinks; library views use a **slot-paginated list** (the viewport is a fixed odd number of whole-row slots — never partials) where the slot count adapts to window height (up to 29) and text, album artwork, and star icons scale with each slot

Full feature tour and `config.toml` reference: [docs](https://f-o-o-g-s.github.io/nokkvi-docs/).

### Non-Goals

Nokkvi is a fast, keyboard-driven music player, not a Navidrome admin panel. These features are intentionally left out for now:
- Server administration (library scans, user management)
- Podcasts
- Jukebox mode
- Smart playlist generation
- Bookmarks
- Public sharing links
- Offline download
- Lyrics

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
