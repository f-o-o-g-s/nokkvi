# Nokkvi

A native Rust/Iced client for [Navidrome](https://www.navidrome.org/) music server. Named after Old Norse *nökkvi*, a small, humble boat.

<img src="assets/nokkvi_demo.webp" width="100%" alt="Nokkvi theme and visualizer demo" />

> **⚠️ AI-Generated Project**
>
> This entire codebase was written by AI (primarily Claude) with my direction. I'm not a developer. I come up with the ideas, test things, and use this as my daily music player, but I don't write code myself. You'll probably find patterns in here that experienced Rust developers would do differently. If you spot something that could be better, issues and PRs are welcome.

**Platform:** Linux only. Tested on Arch Linux with PipeWire v<!-- pipewire-version -->1.6.2<!-- /pipewire-version --> and Hyprland (Wayland) v<!-- hyprland-version -->0.54.3<!-- /hyprland-version --> against Navidrome v<!-- navidrome-version -->0.61.1<!-- /navidrome-version -->. No Windows or macOS support.

## Features

### Audio
- Native PipeWire audio engine with gapless playback and crossfade
- Real-time hardware volume synchronization with your desktop
- 10-band graphic equalizer with custom presets
- GPU-accelerated visualizer (bars and line modes)

### Library & Playback
- Browse by albums, artists, songs, genres, and playlists
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
- User-configurable keyboard shortcuts for every action
- System font picker with live preview
- Right-click context menus everywhere, including "Show in File Manager"
- MPRIS D-Bus integration for desktop media controls
- In-app settings editor with drill-down navigation and inline search

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


## Configuration

Configuration is stored in `~/.config/nokkvi/config.toml` (hot-reloadable).

### Built-in Themes

Nokkvi ships with 21 built-in themes that are automatically seeded to `~/.config/nokkvi/themes/` on first launch. These include popular editor and terminal color schemes like Gruvbox (default), Catppuccin, Dracula, Nord, Kanagawa, Everforest, Tokyo Night, Solarized, and more. Every theme includes both dark and light palettes plus visualizer colors.

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

[MIT](LICENSE). See [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md) for third-party attribution.
