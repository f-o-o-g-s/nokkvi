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
├── Domain Services (Albums, Artists, Songs, Genres, Playlists, Radios, Similar, Queue, Settings, Auth)
│   └── Lazy-initialized via `tokio::sync::OnceCell` (not Mutex<Option<T>>)
├── ArtworkPrefetch — background artwork loading with pagination, progress tracking, and dynamic cache key mappings
├── NavidromeEvents — SSE subscription (`services/navidrome_events.rs`) → parses server-sent events → triggers background library refresh with ID-based anchoring
└── TaskManager (centralized spawn tracking)
```

## Queue System

`SongPool` (HashMap) + `Queue` ordering (`song_ids` + `order` array + `current_order`):
- Shuffle off: identity order. Shuffle on: Fisher-Yates, current song anchored at start.
- **Navigation**: `peek_next_song()` → `transition_to_queued()` (single transition path for all auto/manual transitions). `get_next_song()` = peek + transition convenience.
- All mutations call `clear_queued()` to invalidate buffered next song.
- Progressive building: first 500 plays immediately, recursive `ProgressiveQueueAppendPage` chain for rest.
- Serialization: bincode `Encode`/`Decode` (~3× faster than JSON). `load_binary_or_json()` migrates legacy.
- **Reshuffle on repeat wrap**: when shuffle + repeat-playlist are both active, the order array is re-shuffled when the queue wraps back to start.

## Batch Operations

`BatchPayload` + `BatchItem` (`data/src/types/batch.rs`): multi-selection batch processing for queue add, playlist add, and context menu actions. `BatchItem` variants: `Song`, `Album`, `Artist`, `Genre`, `Playlist`. Built from `evaluate_context_menu()` resolved indices in visual top-to-bottom order.

## Persistence

| Store | Location | Pattern |
|-------|----------|---------|
| **redb** | `services/state_storage.rs` | Queue ordering, encrypted password |
| **TOML config** | `services/toml_settings_io.rs` | Hot-reloadable via `toml_edit`. `verbose_config` mode writes all defaults. |
| **Theme files** | `services/theme_loader.rs` | Named `.toml` in `~/.config/nokkvi/themes/`. 21 built-in (compiled via `include_str!`, seeded on first run). Discovery, load/save, restore-builtin. |
| **Config writer** | `src/config_writer.rs` (UI crate) | Per-key TOML updates. `update_config_value()` → config.toml; `update_theme_value()` → active theme file. Atomic via temp + rename. |
| **Credentials** | `data/src/credentials.rs` | AES-256-GCM + PBKDF2, password in redb |

## SettingsManager (`services/settings.rs`)

Owns `PlayerSettings`, `TomlSettings`, `TomlViewPreferences`, `HotkeyConfig`, `StateStorage`. Loads config.toml → merges with redb → `PlayerSettings`. Per-field setters persist atomically. `reload_from_toml()` for hot-reload. Settings include artwork render resolutions, library page thresholds, and font family.

## Theme System

- `ThemeFile`: `name`, `dark: ThemePalette`, `light: ThemePalette`
- `ThemePalette`: background (7 levels), foreground (5 + gray), accent colors, 6 named color pairs, `VisualizerColors`
- `config.toml` stores `theme = "name"` — points to `~/.config/nokkvi/themes/{name}.toml`
- Font is a **global setting** (`font_family` in `PlayerSettings`/`TomlSettings`), not tied to `ThemeFile`

## Domain Types

Types are **iced-free**. Key types: `PagedBuffer<T>`, `HotkeyConfig` (HashMap with O(1) lookup), `PlayerSettings` (read the struct for fields — includes `font_family`, `library_page_size`), `Queue`, `QueueSortMode` (physical sort), `PlaylistEditState` (dirty detection), `SongPool`, `BatchPayload`/`BatchItem`, `LibraryFilter` (ID-based cross-view navigation filter).

## API Patterns

Per-domain modules in `services/api/`. Star API: optimistic UI + revert. Rating: +/- hotkeys. Playlist CRUD: Navidrome native REST (not Subsonic for writes). MPRIS: D-Bus background task (exposes full metadata, handles standard playback and radio). Internet Radio: Subsonic `getInternetRadioStations` + mutative CRUD tasks.
