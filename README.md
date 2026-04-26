# Nokkvi

A native Rust/Iced client for [Navidrome](https://www.navidrome.org/) music server. Named after Old Norse *nökkvi*, a small, humble boat.

*Demo showcasing Nokkvi's GPU-accelerated audio visualizers and instant theme hot-reloading in action (automated via a script).*
<img src="assets/nokkvi_demo.webp" width="100%" alt="Nokkvi theme and visualizer demo" />

> **⚠️ AI-Generated Project**
>
> This entire codebase was written by AI (primarily Claude) with my direction. I'm not a developer. I come up with the ideas, test things, and use this as my daily music player, but I don't write code myself. You'll probably find patterns in here that experienced Rust developers would do differently. If you spot something that could be better, issues and PRs are welcome.

**Platform:** Linux only. Built and tested on Arch Linux (Wayland/Hyprland) with PipeWire v<!-- pipewire-version -->1.6.4<!-- /pipewire-version --> and Navidrome v<!-- navidrome-version -->0.61.2<!-- /navidrome-version -->. No Windows or macOS support.
**Network:** Designed and tested primarily as a local network client (LAN). Performance and reliability over WAN/remote internet connections are unknown.

## 📖 Documentation

Full guides, configuration reference, theming palette, keyboard shortcuts, and visualizer internals live at the **[Nokkvi documentation site](https://f-o-o-g-s.github.io/nokkvi-docs/)**. The README below is an at-a-glance summary — anything deeper belongs in the docs.

## Inspirations

Nokkvi draws heavy inspiration from several excellent projects:

- **[rmpc](https://github.com/mierak/rmpc)**: A terminal-based MPD client and previous daily driver that provided early inspiration.
- **[Feishin](https://github.com/jeffvli/feishin)**: Referenced tremendously for their comprehensive Navidrome API implementation and enums.
- **[mpd](https://github.com/MusicPlayerDaemon/MPD)**: Heavily influenced Nokkvi's robust queue and consume logic.
- **[fooyin](https://github.com/fooyin/fooyin)**: Referenced for their native PipeWire implementation.
- **[StepMania](https://github.com/stepmania/stepmania)**: Inspired the video-game-like feel and drill-down navigation of the settings menu.
- **Vim**: Inspired the built-in color schemes and the highly keyboard-centric approach to navigation.

## Highlights

- Native PipeWire audio engine with gapless playback, crossfade, EBU R128 normalization, and a 10-band equalizer
- GPU-accelerated visualizer with `bars` and `lines` modes plus rich gradient and peak controls
- Browse albums, artists, songs, genres, playlists, internet radios, and algorithmic similarities — with inline expansion and split-view library browsing
- 21 built-in themes (Gruvbox, Catppuccin, Dracula, Nord, Tokyo Night, Kanagawa, Everforest, …) with instant hot-reload — drop your own `.toml` in `~/.config/nokkvi/themes/` to add a custom theme
- Persistent queue, multi-selection, drag-and-drop, star ratings, and scrobbling through Navidrome (Last.fm / ListenBrainz)
- Keyboard-driven UI with user-configurable shortcuts, MPRIS D-Bus integration, and right-click context menus everywhere

See the [docs](https://f-o-o-g-s.github.io/nokkvi-docs/) for the full feature tour and every option exposed in `config.toml`.

### Non-Goals

Nokkvi is designed to be a fast, highly-themed, keyboard-driven music player—not a full administrative dashboard or a 1:1 replica of the Navidrome web UI. The following Navidrome features are intentionally **not** implemented at this time (though some may be considered for future releases):
- Server administration (triggering library scans, user management)
- Podcasts
- Jukebox mode (server-side playback control)
- Smart playlist generation (filtering rules configuration)
- Bookmarks (audiobook/podcast resume positions)
- Public sharing links
- Media downloading for offline usage
- Lyrics integration

## Quickstart

```bash
sudo pacman -S pipewire fontconfig pkg-config   # Arch system deps
cargo build --release                           # build
./install.sh                                    # install binary, .desktop, icon
```

The release binary lands at `target/release/nokkvi`; `install.sh` copies it to `~/.local/bin/nokkvi` and sets up the desktop entry and icon. Per-user data lives in `~/.config/nokkvi/` (`config.toml`, `app.redb`, `themes/`, `sfx/`).

Full setup, server connection, and configuration walkthroughs are in the docs:
- [Installation](https://f-o-o-g-s.github.io/nokkvi-docs/guides/installation/)
- [Connecting to Navidrome](https://f-o-o-g-s.github.io/nokkvi-docs/guides/navidrome/)
- [Configuration reference](https://f-o-o-g-s.github.io/nokkvi-docs/reference/config/)
- [Keyboard shortcuts](https://f-o-o-g-s.github.io/nokkvi-docs/reference/hotkeys/)

> **Troubleshooting:** No audio but volume looks correct? Ensure your desktop's sound daemon (PipeWire) is running.

Build prerequisites and contributor workflow (including the **nightly** rustfmt requirement) are in [CONTRIBUTING.md](CONTRIBUTING.md).

## Media Controls (MPRIS)

The client exposes MPRIS D-Bus controls as `nokkvi`. Use `playerctl` to control playback from keybinds or scripts.

### Hyprland example

```conf
# Media player controls via playerctl (MPRIS)
bind = $mainMod ALT, space, exec, playerctl -p nokkvi play-pause
bind = $mainMod ALT, right, exec, playerctl -p nokkvi next
bind = $mainMod ALT, left, exec, playerctl -p nokkvi previous
binde = $mainMod ALT, up, exec, playerctl -p nokkvi volume 0.01+
binde = $mainMod ALT, down, exec, playerctl -p nokkvi volume 0.01-
```

### CLI usage

```bash
playerctl -p nokkvi play-pause
playerctl -p nokkvi next
playerctl -p nokkvi previous
playerctl -p nokkvi volume 0.05+
playerctl -p nokkvi metadata   # show current track info
```

## Known Issues

### Application crash on narrow window resize
Resizing the application window horizontally to be extremely narrow will abruptly crash the application. This happens because our underlying user-interface framework (`iced`) currently struggles to safely draw images using your graphics card when they are shrunken down to less than a single pixel wide.

During testing, we attempted to write safety checks in Nokkvi to hide artwork before it gets that small. However, we found that the framework attempts to calculate and draw those tiny images before our safety checks even have a chance to run, so we ultimately did not keep these ineffective workarounds in the codebase.

We suspect this is a bug in the framework itself, and we have submitted a potential upstream fix ([PR #3292](https://github.com/iced-rs/iced/pull/3292)). Until this is reviewed and merged by the maintainers, please avoid squishing the window footprint too tightly!

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for build instructions, guidelines, and the AI disclosure.

## License

[GNU General Public License v3.0](LICENSE). See [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md) for third-party attribution.
