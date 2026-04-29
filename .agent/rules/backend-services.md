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
│   └── Lazy-initialized via `tokio::sync::OnceCell`
├── Artwork — server-only, no client-side persistent cache
│   ├── `AlbumsService::artwork_client: Arc<reqwest::Client>` (bare reqwest)
│   ├── `AlbumsService::fetch_album_artwork(art_id, size, updated_at)` — single fetch path; every call hits Navidrome
│   └── Session-scoped Handle reuse via UI's `album_art` (LRU 512) + `large_artwork` (LRU 200) maps
├── NavidromeEvents — SSE subscription (`services/navidrome_events.rs`) → `RefreshResource { resources, is_wildcard }` → ID-anchored slot-list reload; non-wildcard events trigger silent re-fetch for any affected album in `album_art` / `large_artwork`
└── TaskManager — centralized spawn tracking + status channel for UI notifications, cancellation via `tokio_util::CancellationToken`
```

`backend/` modules: `app_service.rs`, `playback_controller.rs`, `albums.rs`, `artists.rs`, `songs.rs`, `genres.rs`, `playlists.rs`, `queue.rs`, `settings.rs`, `auth.rs`.

`services/api/` per-domain modules: `albums.rs`, `artists.rs`, `songs.rs`, `genres.rs`, `playlists.rs`, `radios.rs`, `similar.rs`, `rating.rs`, `star.rs`, `subsonic.rs`, `client.rs`. Radios and Similar live in API only — no separate backend service.

## Queue System

`SongPool` (HashMap) + `Queue` ordering (`song_ids` + `order` array + `current_order`):
- Shuffle off: identity order. Shuffle on: Fisher-Yates with the current song anchored at index 0.
- **Navigation** (`services/queue/navigation.rs`): `peek_next_song()` → `transition_to_queued()` is the single transition path for all auto/manual transitions. `get_next_song()` = peek + transition convenience.
- Every queue mutation calls `clear_queued()` to invalidate the buffered next song.
- Progressive build: first 500 plays immediately; recursive `ProgressiveQueueAppendPage` chain for the rest.
- Serialization: bincode `Encode` / `Decode` (~3× faster than JSON). `load_binary_or_json()` migrates legacy.
- **Reshuffle on repeat wrap**: shuffle + repeat-playlist re-shuffles the order array when the queue wraps back to the start.

## Batch Operations

`BatchPayload` + `BatchItem` (`data/src/types/batch.rs`) — multi-selection batch processing for queue add, playlist add, context menu actions. `BatchItem` variants: `Song`, `Album`, `Artist`, `Genre`, `Playlist`. Built in visual top-to-bottom order via `evaluate_context_menu()` resolved indices.

## Persistence

| Store | Location | Pattern |
|-------|----------|---------|
| **redb** | `services/state_storage.rs` | Queue ordering, encrypted password |
| **TOML config** | `services/toml_settings_io.rs` | Hot-reloadable via `toml_edit`. `verbose_config` writes all defaults |
| **Theme files** | `services/theme_loader.rs` | Named `.toml` in `~/.config/nokkvi/themes/`. **21 built-in** (compiled via `include_str!`, seeded on first run). Discovery, load/save, restore-builtin |
| **Artwork** | (no disk cache) | Server-only. Session-scoped Handle reuse in UI maps |
| **Config writer** | `src/config_writer.rs` (UI crate) | Typed `ConfigKey { AppScalar, AppArrayEntry, Theme, ThemeArrayEntry }`. Per-key TOML updates, atomic via temp + rename |
| **Credentials** | `data/src/credentials.rs` | AES-256-GCM + PBKDF2; password lives in redb |

## SettingsManager (`services/settings.rs`)

Owns `PlayerSettings`, `TomlSettings`, `TomlViewPreferences`, `HotkeyConfig`, `StateStorage`. Loads `config.toml` → merges with redb → `PlayerSettings`. Per-field setters persist atomically. `reload_from_toml()` for hot-reload.

`PlayerSettings` includes: `font_family`, `library_page_size`, `artwork_resolution`, `volume_normalization` (+ ReplayGain knobs), per-view column-visibility flags (`queue_show_*`, `albums_show_*`, `songs_show_*`, `artists_show_*`), `artwork_column_mode` / `artwork_column_stretch_fit` / `artwork_column_width_pct`, `show_tray_icon` / `close_to_tray`, `nav_layout` (`Top` / `Side` / `None`), `slot_row_height`, `track_info_display`. Read the struct for the full set.

## Theme System

- `ThemeFile`: `name`, `dark: ThemePalette`, `light: ThemePalette`
- `ThemePalette`: `BackgroundConfig` (7 levels), `ForegroundConfig` (5 + gray), `AccentConfig`, four `SemanticColorConfig` (danger / success / warning / star), `VisualizerColors`
- `config.toml` stores `theme = "name"` — points to `~/.config/nokkvi/themes/{name}.toml`
- Font is a **global setting** (`font_family` in `PlayerSettings` / `TomlSettings`), decoupled from `ThemeFile`

## Domain Types (`data/src/types/`)

Iced-free. Key types:
- `PagedBuffer<T>` — replaces `Vec<T>` for library data. `Deref<Target=[T]>`. **`generation()`** monotonic counter bumps on every mutation (use for `(query, generation)` filter-cache keys)
- `HotkeyConfig` — HashMap with O(1) lookup
- `PlayerSettings`, `TomlSettings`, `TomlViewPreferences`
- `Queue`, `QueueSortMode` (physical sort: Album/Artist/Title/Duration/Genre/Rating/MostPlayed)
- `SongPool`, `BatchPayload` / `BatchItem`
- `LibraryFilter` — ID-based cross-view navigation filter
- `PlaylistEditState` — dirty detection
- `InfoModalItem` — owned data for the Get Info modal
- `ReactiveProperty<T>` / `ReactiveVecProperty<T>` (`reactive.rs`) — thread-safe shared property with subscribe-on-change

## API Patterns

- **Star**: optimistic UI + revert on failure
- **Rating**: +/- hotkeys
- **Playlist CRUD**: Navidrome-native REST (not Subsonic) for writes
- **MPRIS**: D-Bus background task; full metadata for both queue songs and radio streams
- **Tray**: ksni-based StatusNotifierItem in `src/services/tray.rs`; emits `TrayEvent` to the UI
- **Internet Radio**: Subsonic `getInternetRadioStations` + mutative CRUD tasks
