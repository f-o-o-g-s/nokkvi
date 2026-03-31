---
trigger: glob
globs: data/src/backend/**,data/src/services/**,data/src/types/**,data/src/credentials.rs
---

# Backend Services

## Service Architecture

```
AppService (orchestrator)
├── PlaybackController (audio engine + queue navigator)
│   ├── Transport, volume, modes, gapless transitions
│   ├── Playback history (Vec<Song>, capped, dedup-on-push)
│   └── reset_next_track() on mode toggles
├── Domain Services (Albums, Artists, Songs, Genres, Playlists, Queue, Settings, Auth)
│   └── Lazy-initialized via `tokio::sync::OnceCell` (not Mutex<Option<T>>)
├── ArtworkPrefetch — background artwork loading with progress tracking
└── TaskManager (centralized spawn tracking)
```

## Queue System

`SongPool` (HashMap) + `Queue` ordering (`song_ids` + `order` array + `current_order`):
- Shuffle off: identity order. Shuffle on: Fisher-Yates, current song anchored at start.
- **Navigation**: `peek_next_song()` → `transition_to_queued()` (single transition path for all auto/manual transitions). `get_next_song()` = peek + transition convenience.
- All mutations call `clear_queued()` to invalidate buffered next song.
- Progressive building: first 500 plays immediately, recursive `ProgressiveQueueAppendPage` chain for rest.
- Serialization: bincode `Encode`/`Decode` (~3× faster than JSON). `load_binary_or_json()` migrates legacy.

## Persistence

| Store | Location | Pattern |
|-------|----------|---------|
| **redb** | `services/state_storage.rs` | Queue ordering, encrypted password |
| **TOML config** | `services/toml_settings_io.rs` | Hot-reloadable via `toml_edit`. `verbose_config` mode writes all defaults. |
| **Theme files** | `services/theme_loader.rs` | Named `.toml` in `~/.config/nokkvi/themes/`. 11 built-in (compiled via `include_str!`, seeded on first run). Discovery, load/save, restore-builtin. |
| **Config writer** | `src/config_writer.rs` (UI crate) | Per-key TOML updates. `update_config_value()` → config.toml; `update_theme_value()` → active theme file. Atomic via temp + rename. |
| **Credentials** | `data/src/credentials.rs` | AES-256-GCM + PBKDF2, password in redb |

## SettingsManager (`services/settings.rs`)

Owns `PlayerSettings`, `TomlSettings`, `TomlViewPreferences`, `HotkeyConfig`, `StateStorage`. Loads config.toml → merges with redb → `PlayerSettings`. Per-field setters persist atomically. `reload_from_toml()` for hot-reload.

## Theme System

- `ThemeFile`: `name`, `font_family`, `dark: ThemePalette`, `light: ThemePalette`
- `ThemePalette`: background (7 levels), foreground (5 + gray), accent colors, 6 named color pairs, `VisualizerColors`
- `config.toml` stores `theme = "name"` — points to `~/.config/nokkvi/themes/{name}.toml`

## Domain Types

Types are **iced-free**. Key types: `PagedBuffer<T>`, `HotkeyConfig` (HashMap with O(1) lookup), `PlayerSettings` (read the struct for fields), `Queue`, `QueueSortMode` (physical sort), `PlaylistEditState` (dirty detection), `SongPool`.

## API Patterns

Per-domain modules in `services/api/`. Star API: optimistic UI + revert. Rating: +/- hotkeys. Playlist CRUD: Navidrome native REST (not Subsonic for writes). MPRIS: D-Bus background task.
