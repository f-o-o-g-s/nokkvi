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
│   ├── source_generation counter (AtomicU64) — prevents stale track-completion callbacks
│   └── reset_next_track() on mode toggles — clears stale gapless/crossfade prep
├── Domain Services (Albums, Artists, Songs, Genres, Playlists, Queue, Settings, Auth)
│   └── Lazy-initialized via `tokio::sync::OnceCell` (not Mutex<Option<T>>)
├── ArtworkPrefetch (artwork_prefetch.rs) — background artwork loading with progress tracking
├── TaskManager (centralized spawn tracking)
└── Font Discovery (font-kit, LazyLock-cached system font enumeration)
```

### Service Initialization

Domain services use `Arc<OnceCell<T>>` for lazy init via `get_or_try_init()` — guarantees atomic init-once semantics without double-locking hazards.

## Server-Side Pagination

Library data loaded in 500-item pages via `PagedBuffer<T>` (`data/src/types/paged_buffer.rs`):

- **Initial load**: first page + `X-Total-Count` header → `set_first_page()`
- **Scroll-triggered prefetch**: `needs_fetch(viewport_offset)` returns `Some((start, end))` → `LoadPage` action → `append_page()`
- **Guard against duplicates**: `set_loading(true)` before fetch prevents concurrent requests
- `PagedBuffer<T>` implements `Deref<Target = [T]>` — drop-in replacement for `Vec<T>`
- Queue uses `SongPool` + `Queue::song_ids` + `Queue::order` for ordering — not paginated from API

### Queue Order Array

`Queue.order` maps play-order positions → `song_ids` indices:
- Shuffle off: identity `[0, 1, 2, …]`
- Shuffle on: Fisher-Yates shuffled, current song anchored at start
- `current_order` tracks position within `order` (not `song_ids`)
- `queued`: order-index of pre-buffered next song (gapless/crossfade prep)

**Navigation pattern** (`peek_next_song` → `transition_to_queued`):
1. `peek_next_song()` — computes next from order array, sets `queued`, returns `NextSongResult` without updating indices. Used for gapless/crossfade preparation.
2. `transition_to_queued()` — consumes `queued`, updates `current_index`/`current_order`, returns `TransitionResult`. Single transition path for all automatic and manual transitions.
3. `get_next_song()` — convenience: peek + transition in one call (used by manual skip).

All queue mutations call `clear_queued()` to invalidate the buffered next song.

### Progressive Queue Building

Playing from Songs view: first 500-song page plays immediately. Remaining pages load via recursive `ProgressiveQueueAppendPage` chain (~200ms per page). `progressive_queue_generation` counter lets stale chains self-cancel.

### Logout Flow

- `AppService::new_with_storage()` reuses existing `StateStorage` (redb handle) across logout/login
- `TaskManager::shutdown()` signals all background tasks to stop
- Audio engine stopped to prevent orphaned streams

## API Client Patterns (`services/api/`)

- Per-domain API modules: `star.rs`, `rating.rs`, `playlists.rs`, etc.
- Star API: per-view starring with optimistic UI + revert on failure
- Rating API: +/- hotkeys, love (star) auto-sets 5 stars
- **Playlist CRUD**: Navidrome native REST API for mutations (not Subsonic API for writes)

## Persistence

| Store | Location | Pattern |
|-------|----------|---------|
| **redb** | `services/state_storage.rs` | Single DB: queue ordering, encrypted password (API keys) |
| **Queue songs** | `services/queue/` | `SongPool` + `Queue` ordering. Native `bincode::Encode`/`Decode` (~3× faster than JSON). `load_binary_or_json()` migrates legacy data. Directory module: `mod.rs` (mutations/persistence), `order.rs` (order array), `navigation.rs` (peek/transition/next/previous). |
| **TOML config** | `services/toml_settings_io.rs` | All user preferences (General, Interface, Playback, Hotkeys, Theme, Visualizer). Atomic updates via `toml_edit`, hot-reloadable. `verbose_config` mode writes all settings including defaults. |
| **Credentials** | `credentials.rs` (at `data/src/`) | AES-256-GCM + PBKDF2. Password stored in redb. |

### SettingsManager (`services/settings.rs`)

Central orchestrator for all user preferences:
- Owns `PlayerSettings`, `TomlSettings`, `TomlViewSettings`, `HotkeyConfig`, `StateStorage`
- Loads from `config.toml` → merges with redb state → `PlayerSettings`
- Per-field setters (e.g., `set_volume()`, `set_crossfade_enabled()`) persist to TOML atomically
- `write_all_toml_public()` — writes all sections (used by verbose_config toggle)
- `reload_from_toml()` — re-reads config.toml for hot-reload
- `is_verbose_config()` / `set_verbose_config()` — controls whether default values are written to TOML

## Domain Types (`types/`)

- Types are **iced-free** — no UI framework dependencies in the data crate
- `PagedBuffer<T>`: generic windowed buffer for server-side pagination
- `HotkeyConfig`: `HashMap<HotkeyAction, KeyCombo>` with `lookup()` for O(1) dispatch
- `PlayerSettings`: persisted to `config.toml` (hot-reloadable) via TOML IO. Fields include: volume, sfx_volume, sound_effects_enabled, visualization_mode, scrobbling_enabled, scrobble_threshold, start_view, stable_viewport, auto_follow_playing, enter_behavior, local_music_path, rounded_mode, nav_layout, nav_display_mode, track_info_display (`TrackInfoDisplay` enum: Off/PlayerBar/TopBar/ProgressTrack), slot_row_height (`SlotRowHeight` enum: Compact/Default/Comfortable/Spacious), opacity_gradient, crossfade_enabled, crossfade_duration_secs, default_playlist_id/name, quick_add_to_playlist, horizontal_volume, volume_normalization, normalization_level (`NormalizationLevel` enum: Quiet/Normal/Loud), strip_show_title/artist/album/format_info (visible fields toggles), strip_click_action (`StripClickAction` enum: GoToQueue/GoToAlbum/GoToArtist/CopyTrackInfo/DoNothing), active_playlist_id/name/comment (persisted across restarts), verbose_config (bool)
- `TomlSettings`: intermediate struct for TOML parsing — maps `[general]`/`[interface]`/`[playback]` sections
- `TomlViewSettings`: per-section TOML mappings for theme and visualizer
- `ViewPreferences`: per-view sort/search persistence
- `Queue`: lightweight ordering struct — `song_ids`, `order` (play-order array), `current_index`, `current_order`, `queued`, mode flags. Bincode-serialized.
- `QueueSortMode`: physical sort (no QueueOrder)
- `PlaylistEditState`: tracks playlist snapshot for dirty detection (name, comment, track order). `is_dirty()`, `is_name_dirty()`, `is_comment_dirty()`.
- `ReactiveProperty<T>` / `ReactiveVecProperty<T>`: `Arc<RwLock<T>>` wrappers (parking_lot)
- `SongPool`: `HashMap<String, Song>` separating song data from queue ordering
- Serialization: serde for redb and API; bincode for large payloads (song pool, queue ordering)

## MPRIS Integration (`services/mpris.rs`)

D-Bus media player control — runs as a cancellable background task. Updates `Position` property and emits `Seeked` signal.
