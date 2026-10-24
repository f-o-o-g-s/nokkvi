<p align="center"><img src="assets/nokkvi_logo_readme.svg" width="96" alt="Nokkvi logo" /></p>

<h1 align="center">Nokkvi</h1>

<p align="center">
  <a href="https://github.com/f-o-o-g-s/nokkvi/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/f-o-o-g-s/nokkvi/ci.yml?branch=main&label=CI" alt="CI status" /></a>
  <a href="https://aur.archlinux.org/packages/nokkvi-bin"><img src="https://img.shields.io/aur/version/nokkvi-bin?label=AUR" alt="AUR version" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/f-o-o-g-s/nokkvi" alt="License" /></a>
</p>

A native Rust/Iced client for [Navidrome](https://www.navidrome.org/) music server. Named after Old Norse *nökkvi*, a small, humble boat.

<img src="assets/nokkvi_demo.webp?v=5" width="100%" alt="Nokkvi theme and visualizer demo" />

*MilkDrop presets, Theater Mode, and theme swaps, tiled and floating.*

> **⚠️ AI-Generated Project**
>
> This entire codebase was written by AI (primarily Claude) with my direction. I'm not a developer. I come up with the ideas, test things, and use this as my daily music player, but I don't write code myself. You'll probably find patterns in here that experienced Rust developers would do differently. If you spot something that could be better, issues and PRs are welcome.

**Platform:** Linux only. Built and tested on Arch Linux (Wayland/Hyprland) with PipeWire v<!-- pipewire-version -->1.6.10<!-- /pipewire-version --> and Navidrome v<!-- navidrome-version -->0.64.2<!-- /navidrome-version -->. No Windows or macOS support.

**Network:** Built and tested on LAN. Nokkvi streams your original files and can't ask for a smaller version yet, so FLAC over a slow remote connection can keep stopping to buffer. To listen remotely, set a transcoding for Nokkvi's player in Navidrome under Settings > Players (those tracks play fine, but you can't seek within them).

## 📖 Documentation

Full guides, config reference, keyboard shortcuts, theming, and visualizer details are at the **[Nokkvi docs site](https://f-o-o-g-s.github.io/nokkvi-docs/)**.

## Highlights

- Native **PipeWire** engine: gapless playback, crossfade, a 10-band EQ, ReplayGain, and opt-in **bit-perfect** output with a badge that checks the real hardware clock.
- GPU visualizer (`bars`, `lines`, `scope`) with beat glow, bloom, reflection, and CRT effects, plus a **MilkDrop** mode with a live-preview preset picker and nokkvi's own theme-colored presets.
- **Theater Mode** (F11): the playing track's cover, visualizer, and lyrics fill the window.
- **Smart playlists** (Navidrome 0.61+) built in-app with a rules editor and server-evaluated previews; also imports `.nsp`.
- **Trawl** mix builder: blend artist, album, song, genre, and playlist seeds into one mix and save it as a playlist.
- Scriptable from the shell: `nokkvi <verb>` controls the running player (playback, volume, ratings, queue).
- **23 built-in themes** (default **Svalbard**), custom `.toml` themes that hot-reload, and optional accent and visualizer colors taken from the playing album cover.
- Built for tiling WMs: a width-adaptive player bar and a **slot-paginated list** that fits a whole number of rows to the window.
- Plus the essentials: browse albums, artists, songs, genres, playlists, and radio; a **Harbour** home screen; persistent queue with drag-and-drop; star ratings; lyrics synced by line or by word; similar songs, plus similar artists from ListenBrainz and Deezer; five seek bar styles (one draws the waveform); server queue sync; scrobbling; MPRIS; an optional tray icon; and full keyboard control, plus media keys and TV remotes.

### Non-Goals

Nokkvi is a fast, keyboard-driven music player, not a Navidrome admin panel. For now it leaves out server administration (library scans, user management), podcasts, jukebox mode, bookmarks, public sharing links, and offline downloads.

## Download

**Prebuilt binary (x86_64 Linux):** grab the latest tarball and its `.sha256` from [Releases](https://github.com/f-o-o-g-s/nokkvi/releases), then verify, extract, and install:

```bash
sha256sum -c nokkvi-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz.sha256
tar xzf nokkvi-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz
cd nokkvi-vX.Y.Z-x86_64-unknown-linux-gnu
./install.sh
```

It needs `pipewire`, `alsa-lib`, and `fontconfig` (Arch: `sudo pacman -S pipewire alsa-lib fontconfig`).

**Arch (AUR):** [`nokkvi-bin`](https://aur.archlinux.org/packages/nokkvi-bin) tracks the releases; [`nokkvi-git`](https://aur.archlinux.org/packages/nokkvi-git) builds from `main` (e.g. `paru -S nokkvi-bin`).

## Quickstart (build from source)

```bash
sudo pacman -S pipewire alsa-lib fontconfig pkgconf cmake clang # Arch build deps (cmake: bundled libopus; clang: PipeWire bindings)
cargo build --release                           # build
./install.sh                                    # install binary, .desktop, icon
```

`install.sh` puts the binary in `~/.local/bin/`. Config lives in `~/.config/nokkvi/`, runtime state and logs in `~/.local/state/nokkvi/`. Filing a bug? See [debug logging](CONTRIBUTING.md#debug-logging).

More in the docs: [Installation](https://f-o-o-g-s.github.io/nokkvi-docs/guides/installation/) · [Connecting to Navidrome](https://f-o-o-g-s.github.io/nokkvi-docs/guides/navidrome/) · [Configuration](https://f-o-o-g-s.github.io/nokkvi-docs/reference/config/) · [Keyboard shortcuts](https://f-o-o-g-s.github.io/nokkvi-docs/reference/hotkeys/) · [MPRIS](https://f-o-o-g-s.github.io/nokkvi-docs/guides/mpris/)

Build setup and contributor workflow (including the **nightly** rustfmt requirement) are in [CONTRIBUTING.md](CONTRIBUTING.md).

## Inspirations

- **[rmpc](https://github.com/mierak/rmpc)**: My previous daily driver, a terminal MPD client.
- **[Feishin](https://github.com/jeffvli/feishin)**: Referenced heavily for Navidrome API coverage and enums.
- **[mpd](https://github.com/MusicPlayerDaemon/MPD)**: Shaped the queue and consume logic.
- **[fooyin](https://github.com/fooyin/fooyin)**: Referenced for the native PipeWire implementation.
- **[cava](https://github.com/karlstav/cava)**: The visualizer DSP is a Rust port of cavacore, and the Monstercat smoothing comes from cava.c.
- **[OjoDrop](https://github.com/sho-run/ojodrop)** and **[Butterchurn](https://github.com/jberg/butterchurn)**: MilkDrop mode runs OjoDrop's Rust engine on presets from Butterchurn's converted pack.
- **[StepMania](https://github.com/stepmania/stepmania)**: Inspired the MusicWheel-style slot list and the roulette wheel.
- **Vim**: Inspired the color schemes and keyboard-first approach.
