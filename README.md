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

- Native PipeWire audio engine: gapless playback, a 10-band EQ, and AGC + ReplayGain volume normalization. Configurable crossfade (selectable curves, gapless-album and minimum-length policies), with optional fades on pause, stop, skip, and radio.
- Opt-in **bit-perfect** playback bypasses DSP (EQ, software volume, limiter) and follows each track's native sample rate. It runs in a Strict mode, or a Relaxed mode that still crossfades same-format tracks (same sample rate and channel count). The now-playing badge reads the real hardware clock and shows `BIT-PERFECT`, `RESAMPLED`, or `UNVERIFIED` rather than trusting the requested rate.
- GPU-accelerated visualizer with `bars`, `lines`, and `scope` (circular oscilloscope) modes. Beat-reactive neon glow and bloom, plus optional motion trails, echo, and CRT/film post-processing. `bars` and `lines` can sit in a band above the player bar or draw over the now-playing cover art.
- Browse albums, artists, songs, genres, playlists, internet radios, and similar songs, with inline expansion, split-view browsing, and multi-library filtering.
- Radio stations show real artwork: an uploaded logo or the stream's own now-playing image, remembered across restarts. Right-click "Set Custom Artwork" to upload a cover for any playlist or station to Navidrome.
- **Harbour** home view: collapsible discovery shelves for recently played, most played, and recently added, plus random genre and playlist cover mosaics. Up top is nokkvi's first whole-library search, matching artists, albums, songs, genres, and playlists at once.
- **Trawl** mix builder: gather artist, album, song, genre, and playlist seeds into a crate, then blend them into the queue by round-robin interleave, per-seed weight, or shuffle-all. Length, rating, and track-count filters apply, and you can save a blend as a normal playlist. The panel behind it is an animated day-and-night seascape around a trawling longship, with a few rare sights if you leave it up long enough.
- **Smart playlists**, created and edited in-app. Keyboard-first rules editor with live validation, seeded presets, a raw-JSON mode, and server-evaluated previews you can audition before saving (Enter plays a preview row). Rules also import from `.nsp` files. Every surface checks what your Navidrome version supports (0.61+ for rules editing).
- 23 built-in themes (default **Svalbard**, plus Gruvbox, Catppuccin, Dracula, Nord, Tokyo Night, Kanagawa, Everforest, Firmium, Enthroned, and more), picked from a searchable modal that paints each row in that theme's own palette. Hot-reload is instant, and dropping a `.toml` in `~/.config/nokkvi/themes/` adds your own. Two full icon sets ship: Phosphor by default, Lucide as the alternate.
- Persistent queue, multi-selection, drag-and-drop, star ratings, and scrobbling. Library plays route through your server's Last.fm and ListenBrainz agents; internet-radio streams scrobble directly from their broadcast metadata.
- Sync your play queue to Navidrome and pull it back on another device or a fresh session, resuming at the exact track and position. Needs the OpenSubsonic indexBasedQueue extension (Navidrome 0.58.5+).
- Synced lyrics over the Queue cover art, toggled from the player bar or hotkey `L`. Sources are a local `.lrc` store, your server's OpenSubsonic lyrics, or LRCLIB, whose fetches are cached to the local store for offline replay. Lines ease as they follow, a soft glyph halo (plus optional cover blur) keeps them readable over any art, and tracks dissolve into each other on crossfade. The visualizer keeps playing behind the words.
- Fully keyboard-driven with configurable shortcuts, MPRIS, an optional system tray icon, and right-click menus everywhere.
- Scriptable from the shell: `nokkvi <verb>` drives the running player over a local socket (transport, volume, modes, love and rate, queue push/pull). Handy for WM hotkeys and status bars.
- Designed on a tiling WM. The player bar folds controls into a kebab menu as width shrinks. Library views use a **slot-paginated list**: the viewport is always a fixed odd number of whole-row slots, never partial rows. Slot count adapts to window height (up to 29), and text, album artwork, and star icons scale with each slot.

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
