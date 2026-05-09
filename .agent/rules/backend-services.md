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
│   └── reset_next_track() on mode toggles
├── Backend services composed on AppService:
│   AuthGateway, AlbumsService, ArtistsService, SongsService,
│   QueueService, SettingsService
├── On-demand API services (no backend wrapper) — built by
│   `AppService::genres_api()` / `playlists_api()` / `radios_api()` /
│   `similar_api()` / `songs_api()` from the auth gateway
├── Artwork — server-only, no client-side persistent cache
│   ├── `AlbumsService::artwork_client: Arc<reqwest::Client>` (bare reqwest)
│   ├── `AlbumsService::fetch_album_artwork(art_id, size, updated_at)` — single fetch path; every call hits Navidrome. Empty-bytes responses return an error (so retries can recover instead of caching a blank handle)
│   └── Session-scoped Handle reuse via UI's `album_art` (LRU 512) + `large_artwork` (LRU 200) maps
├── SSE — `data/src/services/navidrome_events.rs` parses events; the
│   subscription itself runs in the UI crate (`src/services/navidrome_sse.rs`)
│   and dispatches `RefreshResource { resources, is_wildcard }` → ID-anchored
│   slot-list reload. Non-wildcard events trigger silent re-fetch for any
│   affected album in `album_art` / `large_artwork`.
└── TaskManager — centralized spawn tracking + status channel for UI notifications, cancellation via `tokio_util::CancellationToken`
```

`backend/` modules:
- Service structs: `app_service.rs`, `playback_controller.rs`, `albums.rs`, `artists.rs`, `songs.rs`, `queue.rs`, `settings.rs`, `auth.rs`.
- UI-projected view-data only (no service): `genres.rs` (`GenreUIViewData`), `playlists.rs` (`PlaylistUIViewData`).
- Cross-entity helpers in `mod.rs`: `Starable` / `Ratable` / `PlayCountable` traits with shared list-mutation helpers (`update_starred_in_list`, `update_rating_in_list`, `increment_play_count_in_list`) and `flatten_participants()`.

`services/api/` per-domain modules: `albums.rs`, `artists.rs`, `songs.rs`, `genres.rs`, `playlists.rs`, `radios.rs`, `similar.rs`, `rating.rs`, `star.rs`, `subsonic.rs`, `client.rs`.

## Queue System

`SongPool` (HashMap) + `Queue` ordering (`song_ids` + `order` array + `current_order`). Modules: `services/queue/{mod, navigation, order, write_guard}.rs`.

- **Mutation guard** (`write_guard.rs`): every queue write goes through `QueueWriteGuard`, which forces the caller to pick a commit mode at the end of the borrow — `commit_save_all` (full save), `commit_save_order` (order-only), or `commit_no_save` (in-memory only). Drop without committing panics in debug, warns in release.
- **Navigation typestate** (`navigation.rs`): `peek_next_song()` returns a `PeekedQueue<'_>` guard whose only public consumer is `transition()`. The crate-internal `transition_to_queued_internal` is the actual mutator — no other call site can advance the queue without first peeking.
- Shuffle off: identity order. Shuffle on: Fisher-Yates with the current song anchored at index 0.
- Every queue mutation calls `clear_queued()` to invalidate the buffered next song.
- Mode toggles (`toggle_shuffle`, `set_repeat`, `toggle_consume`) return `ModeToggleEffect` so the playback controller can chain `reset_next_track()` on the engine uniformly.
- Progressive build: first 500 plays immediately; recursive `ProgressiveQueueAppendPage` chain for the rest.
- Serialization: bincode `Encode` / `Decode` (~3× faster than JSON). `load_binary_or_json()` migrates legacy.
- **Reshuffle on repeat wrap**: shuffle + repeat-playlist re-shuffles the order array when the queue wraps back to the start.

## Batch Operations

`BatchPayload` + `BatchItem` (`data/src/types/batch.rs`) — multi-selection batch processing for queue add, playlist add, context menu actions. `BatchItem` variants: `Song`, `Album`, `Artist`, `Genre`, `Playlist`. Built in visual top-to-bottom order via `evaluate_context_menu()` resolved indices.

## Persistence

| Store | Location | Pattern |
|-------|----------|---------|
| **redb** (`~/.local/state/nokkvi/app.redb`) | `services/state_storage.rs` | Generic key/value (`save` / `load` JSON, `save_binary` / `load_binary` bincode). Stores queue order + song pool, `user_settings`, JWT, Subsonic credential |
| **TOML config** (`~/.config/nokkvi/config.toml`, `config.debug.toml` in debug builds) | `services/toml_settings_io.rs` | Hot-reloadable via `toml_edit`. `verbose_config` writes all defaults |
| **Theme files** (`~/.config/nokkvi/themes/`) | `services/theme_loader.rs` | Named `.toml`. **21 built-in** (compiled via `include_str!`, seeded on first run; `everforest` is the first-run default). Discovery, load/save, restore-builtin |
| **Artwork** | (no disk cache) | Server-only. Session-scoped Handle reuse in UI maps |
| **Config writer** | `src/config_writer.rs` (UI crate) | Typed `ConfigKey { AppScalar, AppArrayEntry, Theme, ThemeArrayEntry }`. Per-key TOML updates, atomic via temp + rename |
| **Credentials** | `data/src/credentials.rs` | `server_url` / `username` in `config.toml`; JWT + Subsonic credential in redb. **No password on disk** — JWT auto-refreshes via `X-ND-Authorization`; expired JWT (48h default) drops to the login screen |

## SettingsService & SettingsManager

`SettingsService` (`backend/settings.rs`) is a thin async wrapper around `SettingsManager` (`services/settings.rs`). The wrapper's pure pass-throughs are generated by file-private `delegate_setter!` / `delegate_getter!` macros — only methods that need a cast, multi-arg signature, or two-call-under-one-lock sequence stay inline.

`SettingsManager` owns `PlayerSettings`, `TomlSettings`, `TomlViewPreferences`, `HotkeyConfig`, `StateStorage`. Loads `config.toml` → merges with redb → `PlayerSettings`. Per-field setters persist atomically. `reload_from_toml()` for hot-reload.

`PlayerSettings` is the live in-memory union of every user-tunable knob; split into per-domain submodules under `data/src/types/player_settings/` (`artwork`, `library`, `navigation`, `playback`, `slot_list`, `strip`, `visualizer`). `TomlSettings` is the on-disk shape of `[settings]`. Notable knobs include `font_family`, `library_page_size`, `artwork_resolution`, `volume_normalization` + ReplayGain, per-view column flags (`{view}_show_*`), artwork column, tray, nav, `slot_row_height`, `track_info_display`, default-playlist, strip. Read the structs for the full set.

### `define_settings!` registration

Every setting backed by the Settings UI is registered via the `define_settings!` macro (`data/src/services/settings_tables/{general,interface,playback}.rs`). Each entry declares a key, label, scalar/array type, default, on_dispatch hook, and ui_meta cluster; the macro emits the dispatch arm + the per-tab `dump_<tab>_player_settings` helper that round-trips into `PlayerSettings`. Use `SettingsSideEffect` (`data/src/types/settings_side_effect.rs`) variants to thread side effects (toasts, atomic flag flips, library reloads) back to the UI from the on_dispatch hook. Hotkey actions follow the same single-table pattern in `data/src/types/hotkey_config/action.rs`. Per-tab `SettingsData`, `SettingItem`, `SettingMeta`, and `SettingsEntry` live in `data/src/types/`.

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
- `Queue`, `QueueSortMode` (physical sort: Album/Artist/Title/Duration/Genre/Rating/MostPlayed/Random — Random re-rolls on re-select)
- `ModeToggleEffect` (`mode_toggle.rs`) returned from queue mode toggles to chain engine resets
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
