# Nokkvi

A native Rust/Iced client for [Navidrome](https://www.navidrome.org/) music server. Named after Old Norse *nökkvi*, a small, humble boat.

*Demo showcasing Nokkvi's GPU-accelerated audio visualizers and instant theme hot-reloading in action (automated via a script).*
<img src="assets/nokkvi_demo.webp" width="100%" alt="Nokkvi theme and visualizer demo" />

> **⚠️ AI-Generated Project**
>
> This entire codebase was written by AI (primarily Claude) with my direction. I'm not a developer. I come up with the ideas, test things, and use this as my daily music player, but I don't write code myself. You'll probably find patterns in here that experienced Rust developers would do differently. If you spot something that could be better, issues and PRs are welcome.

**Platform:** Linux only. Tested on Arch Linux with PipeWire v<!-- pipewire-version -->1.6.2<!-- /pipewire-version --> and Hyprland v<!-- hyprland-version -->0.54.3<!-- /hyprland-version --> against Navidrome v<!-- navidrome-version -->0.61.1<!-- /navidrome-version -->. No Windows or macOS support.
**Network:** Designed and tested primarily as a local network client (LAN). Performance and reliability over WAN/remote internet connections are unknown.

## Inspirations

Nokkvi draws heavy inspiration from several excellent projects:

- **[rmpc](https://github.com/mierak/rmpc)**: A terminal-based MPD client and previous daily driver that provided early inspiration.
- **[Feishin](https://github.com/jeffvli/feishin)**: Referenced tremendously for their comprehensive Navidrome API implementation and enums.
- **[mpd](https://github.com/MusicPlayerDaemon/MPD)**: Heavily influenced Nokkvi's robust queue and consume logic.
- **[fooyin](https://github.com/fooyin/fooyin)**: Referenced for their native PipeWire implementation.
- **[StepMania](https://github.com/stepmania/stepmania)**: Inspired the video-game-like feel and drill-down navigation of the settings menu.
- **Vim**: Inspired the built-in color schemes and the highly keyboard-centric approach to navigation.

## Features

### Audio
- Native PipeWire audio engine with gapless playback and crossfade
- Real-time hardware volume synchronization with your desktop
- 10-band graphic equalizer with custom presets
- GPU-accelerated visualizer (bars and line modes)

### Library & Playback
- Browse by albums, artists, songs, genres, playlists, and algorithmic similarities
- Algorithmic exploration — endlessly dive into mathematically related discographies via context menu "Find Similar" and "Top Songs" actions
- Inline expansion — drill into artists/genres to see albums and tracks without leaving the view
- Star ratings (0–5) and favorites on everything
- Multi-selection with Ctrl/Shift — batch add to queue, rate, favorite, etc.
- Drag-and-drop from library to queue, and reorder within the queue
- Queue persistence across restarts
- Scrobbling (last.fm / ListenBrainz via Navidrome)
- Playlist management — create, rename, delete, and split-view editing with a library browser panel

### Interface
- 21 built-in themes inspired by popular editor color schemes (Gruvbox, Catppuccin, Dracula, Nord, Tokyo Night, Kanagawa, Everforest, and more) — or create your own
- Hot-reloadable configuration — themes, visualizer settings, and all preferences update live
- Configurable layouts — side or top navigation, player bar or top-bar metadata strip, rounded corners mode
- User-configurable keyboard shortcuts for most actions
- System font picker with live preview
- Right-click context menus everywhere, including "Show in File Manager"
- MPRIS D-Bus integration for desktop media controls
- In-app settings editor with drill-down navigation and inline search

### Non-Goals
Nokkvi is designed to be a fast, highly-themed, keyboard-driven music player—not a full administrative dashboard or a 1:1 replica of the Navidrome web UI. The following Navidrome features are intentionally **not** implemented at this time (though some may be considered for future releases):
- Server administration (triggering library scans, user management)
- Internet Radio and Podcasts
- Jukebox mode (server-side playback control)
- Smart playlist generation (filtering rules configuration)
- Bookmarks (audiobook/podcast resume positions)
- Public sharing links
- Media downloading for offline usage
- Lyrics integration

## Dependencies (Arch Linux)

```bash
pacman -S pipewire fontconfig pkg-config
```

| Package | Purpose |
|---------|---------|
| `pipewire` | PipeWire development headers (native audio output via `libpipewire-0.3`) |
| `fontconfig` | Font discovery for the system font picker (used by `font-kit`) |
| `pkg-config` | Build-time dependency resolution for native libraries |

> **Troubleshooting:** No audio but volume looks correct? Ensure your desktop environments sound daemon (e.g. PipeWire) is running.
> **Note:** Assumes you have Rust installed via [rustup](https://rustup.rs/) or the `rust` package. The **nightly toolchain** is required for formatting (`cargo +nightly fmt --all`). Keep your toolchain up to date (`rustup update`) — some dependencies require a recent compiler.

## Building

```bash
cargo build --release
```

The binary will be at `target/release/nokkvi`.

### Installation

After building, install the binary, desktop entry, and icon for your user:

```bash
./install.sh
```

This copies the binary to `~/.local/bin/nokkvi` and sets up the `.desktop` file and SVG icon so your app launcher can find it.

### Formatting

Formatting requires the **nightly** toolchain:

```bash
rustup toolchain install nightly   # one-time setup
cargo +nightly fmt --all            # format
cargo +nightly fmt --all -- --check # verify without modifying
```


## Data & Configuration

All application data and configuration is localized to `~/.config/nokkvi/`:

- `config.toml`: User preferences, theme selection, and visualizer settings (hot-reloadable).
- `app.redb`: Unified database file storing your session tokens, saved queue state, and application settings.
- `cache/`: Persistent disk cache for album and artist artwork.
- `themes/`: Directory containing all built-in and user-created `.toml` theme files.
- `sfx/`: Directory containing all UI sound effects (you can drop in custom `.wav` files to override the defaults).

### Artwork Prefetching & Cache

To guarantee instantaneous load times and a fluid, 60fps scrolling experience, Nokkvi doesn't fetch album art on-the-fly like a web browser. Instead, a background service automatically downloads and caches your *entire* library's album and artist artwork (both thumbnail and high-resolution sizes) to `~/.config/nokkvi/cache/` after you log in. 

Depending on your library size, this means Nokkvi will consume local disk space to store these images (often a few hundred megabytes for larger libraries). This aggressive caching strategy is what allows the native application interface to remain perfectly responsive during rapid navigation without being bottlenecked by network latency.

### Built-in Themes

Nokkvi ships with 21 built-in themes that are automatically seeded to `~/.config/nokkvi/themes/` on first launch. These include popular editor and terminal color schemes like Adwaita (default), Gruvbox, Catppuccin, Dracula, Nord, Kanagawa, Everforest, Tokyo Night, Solarized, and more. Every theme includes both dark and light palettes plus visualizer colors.

**To change your theme:** Simply open the in-app **Settings -> Theme** menu, and pick one from the "Select Theme" list. It will apply instantly.

**To create a custom theme:**
1. Copy an existing file in `~/.config/nokkvi/themes/` to a new name (e.g. `my_theme.toml`).
2. Edit the hex colors in your new file.
3. Open Nokkvi Settings, and your custom theme will automatically appear in the list!

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

## Keyboard Shortcuts

All keyboard shortcuts are **user-configurable** via the Settings view (Hotkeys tab). The defaults are listed below.

### View Switching

| Key | Action |
|-----|--------|
| `1` | Switch to Queue view |
| `2` | Switch to Albums view |
| `3` | Switch to Artists view |
| `4` | Switch to Songs view |
| `5` | Switch to Genres view |
| `6` | Switch to Playlists view |
| `` ` `` (backtick) | Toggle Settings view |

### Playback Controls

| Key | Action |
|-----|--------|
| `Space` | Toggle play/pause |
| `x` | Toggle random/shuffle mode |
| `z` | Toggle repeat mode |
| `c` | Toggle consume mode |
| `s` | Toggle sound effects |
| `v` | Cycle visualization mode |
| `q` | Toggle 10-band equalizer |

### Navigation & UI

| Key | Action |
|-----|--------|
| `Backspace` | Navigate slot list up |
| `Tab` | Navigate slot list down |
| `Enter` | Activate center slot list item |
| `Shift+Enter` | Expand center item inline (drill into children) |
| `Ctrl+E` | Toggle library browser panel beside queue |
| `/` | Focus search input |
| `r` | Refresh current view data from the server |
| `Esc` | Collapse inline expansion; if none, clear search; if in Settings, exit |

### Sort Controls

| Key | Action |
|-----|--------|
| `←` | Cycle sort mode backward |
| `→` | Cycle sort mode forward |
| `Page Up` | Toggle sort order (ascending/descending) |

### Settings View

| Key | Action |
|-----|--------|
| `Delete` | Reset focused setting to default |
| `↑` | Toggle field up in ToggleSet (Visible Fields) |
| `↓` | Toggle field down in ToggleSet (Visible Fields) |

### Item Actions

| Key | Action | Views |
|-----|--------|-------|
| `Shift+I` | Open Get Info modal for selected item | All library views + Queue |
| `Shift+S` | Find similar songs for the currently playing track | All |
| `Shift+T` | Show top songs for the currently playing track's artist | All |
| `Shift+C` | Center on currently playing | All (view-aware: finds album, artist, song, or genre) |
| `Shift+L` | Toggle star/favorite on selected item | Queue, Albums, Artists, Songs, Genres, Playlists (expansion-aware) |
| `Shift+A` | Add centered item to queue | Albums, Artists, Songs, Genres, Playlists |
| `=` / `-` | Increase / decrease rating (0–5 stars) | Queue, Albums, Artists, Songs, Genres, Playlists (expansion-aware) |
| `Ctrl+D` | Remove centered item from queue | Queue only |
| `Shift+D` | Clear entire queue | Queue only |
| `Shift+↑` | Move centered track up in queue | Queue only |
| `Shift+↓` | Move centered track down in queue | Queue only |
| `Ctrl+S` | Save queue as playlist | Queue only |

### Inline Expansion

`Shift+Enter` expands the centered item to show its children inline within the slot list:

| View | Expansion Levels |
|------|------------------|
| Albums | Album → Tracks |
| Artists | Artist → Albums → Tracks |
| Playlists | Playlist → Tracks |
| Genres | Genre → Albums → Tracks |

While expanded, `Shift+L`, `=`/`-`, and `Shift+A` act on the child item when the center slot is a child row. Press `Esc` to collapse back (collapses innermost level first).

## Known Issues

### Application crash on narrow window resize
Resizing the application window horizontally to be extremely narrow will abruptly crash the application. This happens because our underlying user-interface framework (`iced`) currently struggles to safely draw images using your graphics card when they are shrunken down to less than a single pixel wide. 

During testing, we attempted to write safety checks in Nokkvi to hide artwork before it gets that small. However, we found that the framework attempts to calculate and draw those tiny images before our safety checks even have a chance to run, so we ultimately did not keep these ineffective workarounds in the codebase.

We suspect this is a bug in the framework itself, and we have submitted a potential upstream fix ([PR #3292](https://github.com/iced-rs/iced/pull/3292)). Until this is reviewed and merged by the maintainers, please avoid squishing the window footprint too tightly!

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for build instructions, guidelines, and the AI disclosure.

## License

[GNU General Public License v3.0](LICENSE). See [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md) for third-party attribution.
