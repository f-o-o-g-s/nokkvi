# Nokkvi

A native Rust/Iced client for [Navidrome](https://www.navidrome.org/) music server. Named after Old Norse *nökkvi*, a small, humble boat.

> **⚠️ AI-Generated Project**
>
> This entire codebase was written by AI (primarily Claude) with my direction. I'm not a developer. I come up with the ideas, test things, and use this as my daily music player, but I don't write code myself. You'll probably find patterns in here that experienced Rust developers would do differently. If you spot something that could be better, issues and PRs are welcome.

**Platform:** Linux only. Tested on Arch Linux with PipeWire and Hyprland (Wayland). No Windows or macOS support.

## Features

- GPU-accelerated audio visualizer (bars + lines modes, pure-Rust FFT via RustFFT with configurable opacity)
- Native PipeWire audio engine, featuring gapless playback and dual-stream crossfade, with hardware stream volume synchronization
- 10-band graphic equalizer with custom presets and precision DSP
- Peak limiter and perceptual volume curve for clean, natural-sounding output
- MPRIS D-Bus integration for media player controls
- Scrobbling support (last.fm / ListenBrainz via Navidrome)
- Hot-reloadable theme and visualizer configuration
- Side or top navigation layout with text, icons, or both
- Now-playing metadata strip (player bar or full-width top bar) with marquee scrolling and right-click context menu
- Optional rounded corners mode for the entire UI
- User-configurable keyboard shortcuts
- In-app settings editor with drill-down navigation, inline search, and preset themes (all preferences hot-reloadable via `config.toml`, including a Verbose Config mode)
- System font picker with live preview
- File-based logging to `~/.config/nokkvi/nokkvi.log`
- Get Info modal (Shift+I) — full metadata inspector with selectable text and copy support
- About modal with system diagnostic information, accessible via hamburger menu
- Show in File Manager — right-click songs to open their containing folder
- Inline three-tier expansion (Artist → Album → Track, Genre → Album → Track)
- Playlist management — create, rename, delete, save queue as playlist
- Split-view playlist editing with library browser panel (includes inline comment editing)
- Cross-pane drag-and-drop from library browser to queue with visual drop indicator
- Right-click context menus on all views (Add to Queue, Add to Playlist, Get Info, etc.)
- Queue drag-and-drop reordering and keyboard track reordering
- Star ratings (0–5) on albums, artists, songs, and queue items
- Scroll-to-adjust volume anywhere on the player bar
- Horizontal volume controls layout option (stacked beside player bar buttons)
- Slot list hover overlay with press darkening and flash micro-animations
- Toast notification system for user feedback
- Server-side pagination for large libraries with configurable page size (`PagedBuffer<T>`)
- Multi-selection batch operations across library and queue views (select with Ctrl/Shift, then right-click or use hotkeys)
- Drag-and-drop multi-selection batches from library to queue and within the queue
- Confirmation dialogs for destructive actions
- Queue persistence across app restarts (restores queue contents and current track)
- Non-wrapping slot list navigation with dynamic center slot
- Dynamic slot sizing with configurable row height (Settings → Interface)
- Modernized card-based login screen with theme-adaptive branding

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

### Formatting

Formatting requires the **nightly** toolchain:

```bash
rustup toolchain install nightly   # one-time setup
cargo +nightly fmt --all            # format
cargo +nightly fmt --all -- --check # verify without modifying
```

### Packaging for Distribution

To create a clean package for sharing with others (excludes build artifacts, reference materials, etc.):

```bash
./package.sh
```

This creates `dist/nokkvi-<version>-<commit>.zip` containing only the essential files needed to build the client, plus a `BUILD_INFO` file tracking the exact commit.


## Configuration

Configuration is stored in `~/.config/nokkvi/config.toml` (hot-reloadable).

### Built-in Themes

Nokkvi ships with several pre-configured built-in themes that are automatically seeded to `~/.config/nokkvi/themes/` on first launch:

- **Gruvbox** (default)
- **Gruvbox Blue**
- **Gruvbox Red**
- **Catppuccin** (Mocha / Latte)
- **Cryo** (Cool icy blue palette)
- **Dracula** (Classic dark / Alucard light)
- **Ember** (Warm orange/red palette)
- **Everforest** (Comfortable green/forest palette)
- **Kanagawa** (Wave / Lotus)
- **Nord** (Arctic, north-bluish palette)
- **Bio Luminal Swamplab** (Custom bioluminescent theme)

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
| `Shift+↑` | Toggle field up in ToggleSet (Visible Fields) |
| `Shift+↓` | Toggle field down in ToggleSet (Visible Fields) |

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

## Troubleshooting

### No audio output

Make sure PipeWire is running and the correct output device is selected.

### fontconfig not found

Install `fontconfig` for system font discovery:

```bash
pacman -S fontconfig
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for build instructions, guidelines, and the AI disclosure.

## License

[MIT](LICENSE). See [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md) for third-party attribution.
